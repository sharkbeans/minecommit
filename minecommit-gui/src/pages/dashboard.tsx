import { useCallback, useEffect, useMemo, useRef, useState } from "react"
import { invoke } from "@tauri-apps/api/core"
import { listen } from "@tauri-apps/api/event"
import {
  AlertTriangle,
  Check,
  CloudDownload,
  CloudOff,
  Folder,
  Loader2,
  Lock,
  Plus,
  RotateCcw,
  Settings2,
  Upload,
} from "lucide-react"

import { Button } from "@/components/ui/button"
import { Textarea } from "@/components/ui/textarea"
import { RollingLogDialog, type Operation } from "@/components/rolling-log"
import type { LogLine } from "@/components/log-viewer"
import {
  AddWorldDialog,
  ConnectCloudDialog,
  RemoveWorldDialog,
  RestorePointDialog,
  SettingsDialog,
} from "@/components/world-dialogs"
import { useCommitAuthor } from "@/contexts/commit-author"
import { useI18n, type TranslationKey } from "@/contexts/i18n"
import { useSaves, type Save } from "@/contexts/saves"
import {
  absoluteTime,
  cloudErrorLabel,
  playedSinceBackup,
  relativeTime,
  repoLabel,
  situationOf,
  type BackupResult,
  type CloudStatus,
  type HistoryEntry,
  type Situation,
  type WorldState,
} from "@/lib/cloud"
import { cn } from "@/lib/utils"

/** The note recorded when the player does not write one. */
const DEFAULT_NOTE = "Backup"

type Busy = "backup" | "latest" | "restore" | null

interface Outcome {
  tone: "good" | "warn" | "bad"
  title: TranslationKey
  body?: TranslationKey
  detail?: string
  retry?: boolean
}

function errorText(error: unknown) {
  return error instanceof Error ? error.message : String(error)
}

/* ── Badges ──────────────────────────────────────────────────────────────── */

const BADGES: Record<Situation, { key: TranslationKey; className: string } | null> = {
  checking: null,
  in_use: { key: "badge.inUse", className: "text-muted-foreground" },
  no_cloud: { key: "badge.localOnly", className: "text-muted-foreground" },
  first_backup: { key: "badge.needsBackup", className: "text-amber-600 dark:text-amber-400" },
  needs_backup: { key: "badge.needsBackup", className: "text-amber-600 dark:text-amber-400" },
  up_to_date: { key: "badge.backedUp", className: "text-emerald-600 dark:text-emerald-400" },
  newer_in_cloud: { key: "badge.newerInCloud", className: "text-sky-600 dark:text-sky-400" },
  conflict: { key: "badge.conflict", className: "text-destructive" },
}

/* ── Page ────────────────────────────────────────────────────────────────── */

export function DashboardPage() {
  const { locale, t } = useI18n()
  const { saves, loaded, refreshSaves, selectedSave, setSelectedSave } = useSaves()
  const { author } = useCommitAuthor()

  const [savesFolder, setSavesFolder] = useState("")
  const [thisDevice, setThisDevice] = useState("")
  const [statuses, setStatuses] = useState<Record<string, CloudStatus | null>>({})
  const [worldStates, setWorldStates] = useState<Record<string, WorldState>>({})
  const [historyByWorld, setHistoryByWorld] = useState<Record<string, HistoryEntry[]>>({})
  const [note, setNote] = useState("")
  const [busy, setBusy] = useState<Busy>(null)
  const [outcome, setOutcome] = useState<Outcome | null>(null)
  const [statusError, setStatusError] = useState("")

  const [logs, setLogs] = useState<LogLine[]>([])
  const [logOpen, setLogOpen] = useState(false)
  const [logFinished, setLogFinished] = useState(false)
  const [operation, setOperation] = useState<Operation>("commit")
  const unlisteners = useRef<Array<() => void>>([])

  const [addOpen, setAddOpen] = useState(false)
  const [connectOpen, setConnectOpen] = useState(false)
  const [removeOpen, setRemoveOpen] = useState(false)
  const [restorePoint, setRestorePoint] = useState<HistoryEntry | null>(null)
  const [settingsOpen, setSettingsOpen] = useState(false)

  /* Saves folder ------------------------------------------------------- */

  useEffect(() => {
    invoke<string>("get_saves_folder")
      .then(setSavesFolder)
      .catch(() => setSavesFolder(""))
    invoke<string>("device_name")
      .then(setThisDevice)
      .catch(() => setThisDevice(""))
  }, [])

  const changeSavesFolder = useCallback((folder: string) => {
    setSavesFolder(folder)
    void invoke("set_saves_folder", { folder }).catch(() => {})
  }, [])

  /* Per-world state ---------------------------------------------------- */

  const readStatus = useCallback(
    async (save: Save, refresh: boolean) => {
      try {
        const status = await invoke<CloudStatus>("get_cloud_status", {
          gitDir: save.repo_path,
          branch: save.default_branch || "main",
          refresh,
        })
        setStatuses((current) => ({ ...current, [save.name]: status }))
        if (refresh) setStatusError("")
        return status
      } catch (err) {
        setStatuses((current) => ({ ...current, [save.name]: null }))
        if (refresh) setStatusError(cloudErrorLabel(errorText(err), t))
        return null
      }
    },
    [t]
  )

  const readWorldState = useCallback(async (save: Save) => {
    try {
      const world = await invoke<WorldState>("world_state", { saveDir: save.path })
      setWorldStates((current) => ({ ...current, [save.name]: world }))
    } catch {
      // A missing folder just means no play-time signal; the badge still works.
    }
  }, [])

  // The list badges read cached refs only, so opening the app does not wait on
  // the network once per world. The world folders are read too: without them a
  // world played since its last backup would sit in the list claiming to be
  // backed up, which is the confusion this dashboard exists to remove.
  useEffect(() => {
    saves.forEach((save) => {
      void readStatus(save, false)
      void readWorldState(save)
    })
  }, [saves, readStatus, readWorldState])

  const refreshSelected = useCallback(async () => {
    if (!selectedSave) return
    await readStatus(selectedSave, true)
    await readWorldState(selectedSave)
    try {
      const entries = await invoke<HistoryEntry[]>("list_history", {
        gitDir: selectedSave.repo_path,
        branch: selectedSave.default_branch || "main",
        limit: 50,
      })
      setHistoryByWorld((current) => ({ ...current, [selectedSave.name]: entries }))
    } catch {
      setHistoryByWorld((current) => ({ ...current, [selectedSave.name]: [] }))
    }
  }, [readStatus, readWorldState, selectedSave])

  useEffect(() => {
    void (async () => {
      await refreshSelected()
    })()
  }, [refreshSelected])

  const selectWorld = useCallback(
    (save: Save | null) => {
      setSelectedSave(save)
      setOutcome(null)
      setNote("")
      // The previous world's failure says nothing about this one.
      setStatusError("")
      // Keeps "most recently played" meaningful, which is what the app opens on.
      if (save) void invoke("access_save", { name: save.name }).catch(() => {})
    },
    [setSelectedSave]
  )

  /* Log streaming ------------------------------------------------------ */

  const startLogging = useCallback(async (op: Operation) => {
    setLogs([])
    setLogFinished(false)
    setOperation(op)
    unlisteners.current.forEach((stop) => stop())
    const line = await listen<LogLine>("commit-log", (event) => {
      setLogs((current) => [...current, event.payload])
    })
    const finished = await listen("commit-finished", () => setLogFinished(true))
    unlisteners.current = [line, finished]
  }, [])

  useEffect(
    () => () => {
      unlisteners.current.forEach((stop) => stop())
    },
    []
  )

  const latestLog = logs.length > 0 ? logs[logs.length - 1].message : ""

  // A successful backup confirms itself and then gets out of the way. Warnings
  // and failures stay until the player dismisses them.
  useEffect(() => {
    if (outcome?.tone !== "good") return
    const timer = setTimeout(() => setOutcome(null), 5000)
    return () => clearTimeout(timer)
  }, [outcome])

  /* Derived ------------------------------------------------------------ */

  const history = selectedSave ? historyByWorld[selectedSave.name] ?? null : null
  const status = selectedSave ? statuses[selectedSave.name] ?? null : null
  const world = selectedSave ? worldStates[selectedSave.name] ?? null : null
  const hasCloud = Boolean(status?.remote_url || selectedSave?.remote_repo_path)
  const situation = useMemo(
    () => situationOf(status, world, hasCloud),
    [hasCloud, status, world]
  )

  /* Actions ------------------------------------------------------------ */

  const backUp = useCallback(async () => {
    if (!selectedSave || busy) return
    // The only case worth skipping is a backup that is already recorded and
    // waiting to upload: recording it again would add a duplicate entry to the
    // history with nothing new in it.
    const commitFirst = !(
      status?.state === "local_ahead" && !playedSinceBackup(status, world)
    )

    setBusy("backup")
    setOutcome(null)
    await startLogging("commit")
    try {
      const result = await invoke<BackupResult>("backup_and_upload", {
        saveDir: selectedSave.path,
        gitDir: selectedSave.repo_path,
        branch: selectedSave.default_branch || "main",
        remote: selectedSave.remote_repo_path || "",
        message: note.trim() || DEFAULT_NOTE,
        authorName: author.name,
        authorEmail: author.email,
        extraPatterns: [],
        ignorePatterns: [],
        commitFirst,
      })

      if (!result.backed_up) {
        setOutcome({ tone: "bad", title: "result.failed", detail: result.error ?? undefined })
      } else if (result.upload_error) {
        setOutcome({
          tone: "warn",
          title: "result.uploadFailed",
          body: "result.uploadFailedHelp",
          detail: cloudErrorLabel(result.upload_error, t),
          retry: true,
        })
      } else if (result.uploaded) {
        setOutcome({ tone: "good", title: "result.done" })
      } else {
        setOutcome({ tone: "good", title: "result.localOnly" })
      }
      setNote("")
    } catch (err) {
      setOutcome({ tone: "bad", title: "result.failed", detail: errorText(err) })
    } finally {
      setBusy(null)
      await refreshSelected()
    }
  }, [author, busy, note, refreshSelected, selectedSave, startLogging, status, t, world])

  const getLatest = useCallback(async () => {
    if (!selectedSave || busy) return
    setBusy("latest")
    setOutcome(null)
    await startLogging("pull")
    try {
      const result = await invoke<{ success: boolean; error: string | null }>("perform_pull", {
        saveDir: selectedSave.path,
        gitDir: selectedSave.repo_path,
        remote: selectedSave.remote_repo_path || "",
        branch: selectedSave.default_branch || "main",
      })
      setOutcome(
        result.success
          ? { tone: "good", title: "result.restored" }
          : {
              tone: "bad",
              title: "result.failed",
              detail: cloudErrorLabel(result.error ?? "", t),
            }
      )
    } catch (err) {
      setOutcome({ tone: "bad", title: "result.failed", detail: errorText(err) })
    } finally {
      setBusy(null)
      await refreshSelected()
    }
  }, [busy, refreshSelected, selectedSave, startLogging, t])

  const restoreTo = useCallback(async () => {
    if (!selectedSave || !restorePoint || busy) return
    setBusy("restore")
    setOutcome(null)
    await startLogging("restore")
    try {
      const result = await invoke<{ success: boolean; error: string | null }>(
        "perform_restore",
        {
          saveDir: selectedSave.path,
          gitDir: selectedSave.repo_path,
          branch: selectedSave.default_branch || "main",
          commit: restorePoint.id,
        }
      )
      setOutcome(
        result.success
          ? { tone: "good", title: "result.restored" }
          : { tone: "bad", title: "result.failed", detail: result.error ?? undefined }
      )
    } catch (err) {
      setOutcome({ tone: "bad", title: "result.failed", detail: errorText(err) })
    } finally {
      setBusy(null)
      setRestorePoint(null)
      await refreshSelected()
    }
  }, [busy, refreshSelected, restorePoint, selectedSave, startLogging])

  /* Render ------------------------------------------------------------- */

  return (
    <div className="flex h-svh w-full flex-col">
      <header className="flex shrink-0 items-center gap-3 border-b px-4 py-2">
        <button
          type="button"
          onClick={() => setSettingsOpen(true)}
          className="flex min-w-0 items-center gap-2 rounded-md px-2 py-1 text-xs text-muted-foreground hover:bg-muted hover:text-foreground"
          title={savesFolder}
        >
          <Folder className="size-3.5 shrink-0" />
          <span className="truncate font-mono">{savesFolder || "…"}</span>
        </button>
        <div className="ml-auto flex items-center gap-3">
          {selectedSave && hasCloud && (
            <span
              className="flex items-center gap-1.5 text-xs text-muted-foreground"
              title={status?.remote_url ?? selectedSave.remote_repo_path}
            >
              <span
                className={cn(
                  "size-1.5 rounded-full",
                  statusError ? "bg-destructive" : "bg-emerald-500"
                )}
              />
              {repoLabel(status?.remote_url ?? selectedSave.remote_repo_path)}
            </span>
          )}
          {author.name && (
            <span className="text-xs text-muted-foreground">{author.name}</span>
          )}
          <Button variant="ghost" size="icon-sm" onClick={() => setSettingsOpen(true)}>
            <Settings2 />
            <span className="sr-only">{t("dash.settings")}</span>
          </Button>
        </div>
      </header>

      <div className="flex min-h-0 flex-1">
        <nav className="flex w-60 shrink-0 flex-col border-r">
          <p className="px-4 pt-4 pb-2 text-xs font-medium tracking-wide text-muted-foreground uppercase">
            {t("dash.worlds")}
          </p>
          <ul className="min-h-0 flex-1 overflow-y-auto px-2">
            {saves.map((save) => {
              const badge = BADGES[situationOf(
                statuses[save.name] ?? null,
                worldStates[save.name] ?? null,
                Boolean(statuses[save.name]?.remote_url || save.remote_repo_path)
              )]
              return (
                <li key={save.name}>
                  <button
                    type="button"
                    onClick={() => selectWorld(save)}
                    className={cn(
                      "w-full rounded-md px-2 py-2 text-left transition-colors",
                      selectedSave?.name === save.name ? "bg-muted" : "hover:bg-muted/60"
                    )}
                  >
                    <span className="block truncate text-sm">{save.name}</span>
                    {badge && (
                      <span className={cn("block truncate text-xs", badge.className)}>
                        {t(badge.key)}
                      </span>
                    )}
                  </button>
                </li>
              )
            })}
            {loaded && saves.length === 0 && (
              <li className="px-2 py-2 text-sm text-muted-foreground">{t("dash.noWorlds")}</li>
            )}
          </ul>
          <div className="border-t p-2">
            <Button
              variant="ghost"
              className="w-full justify-start"
              onClick={() => setAddOpen(true)}
            >
              <Plus data-icon="inline-start" />
              {t("dash.addWorld")}
            </Button>
          </div>
        </nav>

        <main className="min-w-0 flex-1 overflow-y-auto">
          {!selectedSave ? (
            <div className="flex h-full flex-col items-center justify-center gap-3 text-center">
              <p className="text-sm text-muted-foreground">
                {saves.length === 0 ? t("dash.noWorldsHelp") : t("dash.selectWorld")}
              </p>
              {saves.length === 0 && (
                <Button onClick={() => setAddOpen(true)}>
                  <Plus data-icon="inline-start" />
                  {t("dash.addWorld")}
                </Button>
              )}
            </div>
          ) : (
            <div className="mx-auto flex max-w-2xl flex-col gap-6 p-6">
              <div className="flex items-start justify-between gap-4">
                <div className="min-w-0">
                  <h1 className="truncate text-xl font-semibold">{selectedSave.name}</h1>
                  <p className="text-sm text-muted-foreground">
                    {status?.local_timestamp
                      ? t("dash.lastBackedUp", {
                          when: relativeTime(status.local_timestamp, locale),
                        })
                      : t("dash.neverBackedUp")}
                  </p>
                </div>
                <Button variant="ghost" size="sm" onClick={() => setRemoveOpen(true)}>
                  {t("dash.remove")}
                </Button>
              </div>

              <ActionCard
                situation={situation}
                busy={busy}
                note={note}
                setNote={setNote}
                latestLog={latestLog}
                outcome={outcome}
                statusError={statusError}
                onBackUp={() => void backUp()}
                onGetLatest={() => void getLatest()}
                onConnect={() => setConnectOpen(true)}
                onShowLog={() => setLogOpen(true)}
                onRecheck={() => void refreshSelected()}
                onDismiss={() => setOutcome(null)}
              />

              <section className="flex flex-col gap-2">
                <h2 className="text-xs font-medium tracking-wide text-muted-foreground uppercase">
                  {t("dash.history")}
                </h2>
                {history === null ? (
                  <p className="py-3 text-sm text-muted-foreground">{t("dash.checking")}</p>
                ) : history.length === 0 ? (
                  <p className="py-3 text-sm text-muted-foreground">{t("dash.noHistory")}</p>
                ) : (
                  <ul className="flex flex-col divide-y">
                    {history.map((entry, index) => (
                      <li
                        key={entry.id}
                        className="group flex items-center gap-3 py-2.5"
                      >
                        <span
                          className={cn(
                            "size-2 shrink-0 rounded-full",
                            index === 0 ? "bg-primary" : "bg-border"
                          )}
                        />
                        <span className="min-w-0 flex-1">
                          <span
                            className="block truncate text-sm"
                            title={absoluteTime(entry.timestamp, locale)}
                          >
                            {relativeTime(entry.timestamp, locale)}
                          </span>
                          <span className="block truncate text-xs text-muted-foreground">
                            {entry.device
                              ? entry.device === thisDevice
                                ? t("dash.thisDevice")
                                : entry.device
                              : t("dash.unknownDevice")}
                            {entry.note && entry.note !== DEFAULT_NOTE ? ` · ${entry.note}` : ""}
                          </span>
                        </span>
                        <Button
                          variant="ghost"
                          size="sm"
                          className="opacity-0 transition-opacity group-hover:opacity-100 focus-visible:opacity-100"
                          onClick={() => setRestorePoint(entry)}
                          disabled={busy !== null}
                        >
                          <RotateCcw data-icon="inline-start" />
                          {t("dash.restore")}
                        </Button>
                      </li>
                    ))}
                  </ul>
                )}
              </section>
            </div>
          )}
        </main>
      </div>

      <AddWorldDialog
        key={`addOpen-${addOpen}`}
        open={addOpen}
        onOpenChange={setAddOpen}
        savesFolder={savesFolder}
        onAdded={(name) => {
          // The freshly added world is the one the player came here to see.
          void (async () => {
            await refreshSaves()
            if (!name) return
            const all = await invoke<Save[]>("list_saves")
            const added = all.find((save) => save.name === name)
            if (added) selectWorld(added)
          })()
        }}
      />
      <ConnectCloudDialog
        key={`connectOpen-${connectOpen}`}
        open={connectOpen}
        onOpenChange={setConnectOpen}
        save={selectedSave}
        onConnected={async () => {
          await refreshSaves()
          await refreshSelected()
        }}
      />
      <RemoveWorldDialog
        key={`removeOpen-${removeOpen}`}
        open={removeOpen}
        onOpenChange={setRemoveOpen}
        save={selectedSave}
        onRemoved={async () => {
          selectWorld(null)
          await refreshSaves()
        }}
      />
      <RestorePointDialog
        open={restorePoint !== null}
        onOpenChange={(open) => {
          if (!open) setRestorePoint(null)
        }}
        when={restorePoint ? absoluteTime(restorePoint.timestamp, locale) : ""}
        busy={busy === "restore"}
        onConfirm={() => void restoreTo()}
      />
      <SettingsDialog
        key={`settingsOpen-${settingsOpen}`}
        open={settingsOpen}
        onOpenChange={setSettingsOpen}
        savesFolder={savesFolder}
        onSavesFolderChange={changeSavesFolder}
      />
      <RollingLogDialog
        open={logOpen}
        onOpenChange={setLogOpen}
        operation={operation}
        logs={logs}
        finished={logFinished}
      />
    </div>
  )
}

/* ── The one card that says what to do ───────────────────────────────────── */

const SITUATION_COPY: Record<
  Situation,
  { title: TranslationKey; body?: TranslationKey } | null
> = {
  checking: null,
  in_use: { title: "state.inUse.title", body: "state.inUse.body" },
  no_cloud: { title: "state.noCloud.title", body: "state.noCloud.body" },
  first_backup: { title: "state.firstBackup.title", body: "state.firstBackup.body" },
  needs_backup: { title: "state.needsBackup.title", body: "state.needsBackup.body" },
  up_to_date: { title: "state.upToDate.title", body: "state.upToDate.body" },
  newer_in_cloud: { title: "state.newerInCloud.title", body: "state.newerInCloud.body" },
  conflict: { title: "state.conflict.title", body: "state.conflict.body" },
}

function ActionCard({
  situation,
  busy,
  note,
  setNote,
  latestLog,
  outcome,
  statusError,
  onBackUp,
  onGetLatest,
  onConnect,
  onShowLog,
  onRecheck,
  onDismiss,
}: {
  situation: Situation
  busy: Busy
  note: string
  setNote: (note: string) => void
  latestLog: string
  outcome: Outcome | null
  statusError: string
  onBackUp: () => void
  onGetLatest: () => void
  onConnect: () => void
  onShowLog: () => void
  onRecheck: () => void
  onDismiss: () => void
}) {
  const { t } = useI18n()
  const copy = SITUATION_COPY[situation]

  const Icon =
    situation === "up_to_date"
      ? Check
      : situation === "conflict"
        ? AlertTriangle
        : situation === "newer_in_cloud"
          ? CloudDownload
          : situation === "no_cloud"
            ? CloudOff
            : situation === "in_use"
              ? Lock
              : Upload

  if (busy !== null) {
    return (
      <div className="flex flex-col items-center gap-3 rounded-xl border p-8 text-center">
        <Loader2 className="size-6 animate-spin text-muted-foreground" />
        <p className="text-sm font-medium">
          {busy === "backup"
            ? t("state.backingUp")
            : busy === "latest"
              ? t("state.gettingLatest")
              : t("restoreTo.working")}
        </p>
        {latestLog && (
          <p className="max-w-full truncate font-mono text-xs text-muted-foreground">
            {latestLog}
          </p>
        )}
        <Button variant="ghost" size="sm" onClick={onShowLog}>
          {t("dash.showLog")}
        </Button>
      </div>
    )
  }

  if (outcome) {
    return (
      <div
        className={cn(
          "flex flex-col items-center gap-3 rounded-xl border p-8 text-center",
          outcome.tone === "good" && "border-emerald-500/30 bg-emerald-500/5",
          outcome.tone === "warn" && "border-amber-500/30 bg-amber-500/5",
          outcome.tone === "bad" && "border-destructive/30 bg-destructive/5"
        )}
      >
        {outcome.tone === "good" ? (
          <Check className="size-6 text-emerald-600 dark:text-emerald-400" />
        ) : (
          <AlertTriangle
            className={cn(
              "size-6",
              outcome.tone === "warn"
                ? "text-amber-600 dark:text-amber-400"
                : "text-destructive"
            )}
          />
        )}
        <p className="text-sm font-medium">{t(outcome.title)}</p>
        {outcome.body && (
          <p className="text-sm text-muted-foreground">{t(outcome.body)}</p>
        )}
        {outcome.detail && (
          <p className="max-w-full text-xs break-words text-muted-foreground">
            {outcome.detail}
          </p>
        )}
        <div className="flex gap-2">
          {outcome.retry && <Button onClick={onBackUp}>{t("result.retryUpload")}</Button>}
          <Button variant="outline" size="sm" onClick={onDismiss}>
            {t("common.close")}
          </Button>
          <Button variant="ghost" size="sm" onClick={onShowLog}>
            {t("dash.showLog")}
          </Button>
        </div>
      </div>
    )
  }

  if (statusError) {
    return (
      <div className="flex flex-col items-center gap-3 rounded-xl border border-destructive/30 bg-destructive/5 p-8 text-center">
        <AlertTriangle className="size-6 text-destructive" />
        <p className="text-sm font-medium">{t("cloud.cannotCheck")}</p>
        <p className="max-w-full text-xs break-words text-muted-foreground">{statusError}</p>
        <Button variant="outline" onClick={onRecheck}>
          {t("dash.recheck")}
        </Button>
      </div>
    )
  }

  if (!copy) {
    return (
      <div className="flex items-center justify-center gap-2 rounded-xl border p-8 text-sm text-muted-foreground">
        <Loader2 className="size-4 animate-spin" />
        {t("dash.checking")}
      </div>
    )
  }

  const canBackUp =
    situation === "needs_backup" || situation === "first_backup" || situation === "up_to_date"

  return (
    <div className="flex flex-col items-center gap-4 rounded-xl border p-8 text-center">
      <Icon
        className={cn(
          "size-6",
          situation === "up_to_date" && "text-emerald-600 dark:text-emerald-400",
          situation === "conflict" && "text-destructive",
          situation === "newer_in_cloud" && "text-sky-600 dark:text-sky-400",
          (situation === "no_cloud" || situation === "in_use") && "text-muted-foreground",
          (situation === "needs_backup" || situation === "first_backup") &&
            "text-amber-600 dark:text-amber-400"
        )}
      />
      <div className="flex flex-col gap-1">
        <p className="font-medium">{t(copy.title)}</p>
        {copy.body && <p className="text-sm text-muted-foreground">{t(copy.body)}</p>}
      </div>

      {canBackUp && (
        <>
          <Button
            size="lg"
            variant={situation === "up_to_date" ? "outline" : "default"}
            onClick={onBackUp}
          >
            <Upload data-icon="inline-start" />
            {t("state.backUpNow")}
          </Button>
          <Textarea
            rows={2}
            value={note}
            placeholder={t("dash.notePlaceholder")}
            aria-label={t("dash.addNote")}
            className="max-w-sm resize-none text-sm"
            onChange={(e) => setNote(e.target.value)}
          />
        </>
      )}

      {(situation === "newer_in_cloud" || situation === "conflict") && (
        <Button size="lg" onClick={onGetLatest}>
          <CloudDownload data-icon="inline-start" />
          {t("state.newerInCloud.action")}
        </Button>
      )}

      {situation === "no_cloud" && (
        <Button size="lg" onClick={onConnect}>
          {t("state.noCloud.action")}
        </Button>
      )}
    </div>
  )
}
