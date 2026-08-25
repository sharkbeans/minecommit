import { Fragment, useCallback, useEffect, useMemo, useRef, useState, type ReactNode } from "react"
import { invoke } from "@tauri-apps/api/core"
import { listen } from "@tauri-apps/api/event"
import { openUrl } from "@tauri-apps/plugin-opener"
import {
  AlertTriangle,
  Check,
  ChevronDown,
  ChevronLeft,
  ChevronRight,
  CloudDownload,
  CloudOff,
  Folder,
  HelpCircle,
  Loader2,
  Lock,
  Plus,
  RefreshCw,
  RotateCcw,
  Settings2,
  Upload,
} from "lucide-react"
import { Copy } from "lucide-react"

import { Button } from "@/components/ui/button"
import { Skeleton } from "@/components/ui/skeleton"
import { Textarea } from "@/components/ui/textarea"
import { RollingLogDialog, type Operation } from "@/components/rolling-log"
import type { LogLine } from "@/components/log-viewer"
import {
  AddWorldDialog,
  OldCopiesDialog,
  RemoveWorldDialog,
  RestorePointDialog,
  SettingsDialog,
} from "@/components/world-dialogs"
import { CloudSetupDialog } from "@/components/cloud-setup"
import { GuideDialog } from "@/components/guide"
import { Welcome } from "@/components/welcome"
import { GrassBlock } from "@/components/block-icon"
import {
  AccountMenu,
  GitHubSignInDialog,
  type GitHubAccount,
} from "@/components/github-account"
import { useCommitAuthor } from "@/contexts/commit-author"
import { useI18n, type TranslationKey } from "@/contexts/i18n"
import { useSaves, type Save } from "@/contexts/saves"
import {
  absoluteTime,
  clock,
  cloudErrorLabel,
  difficultyName,
  fileSize,
  fractionDone,
  gameModeName,
  progressReadout,
  inGameDay,
  playedSinceBackup,
  relativeTime,
  remainingLabel,
  secondsRemaining,
  repoLabel,
  situationOf,
  PHASE_TITLE,
  type BackupProgress,
  type BackupResult,
  type CloudStatus,
  type HistoryEntry,
  type LevelInfo,
  type OldCopy,
  type Situation,
  type WorldDetails,
  type WorldState,
} from "@/lib/cloud"
import { cn } from "@/lib/utils"

/** The note recorded when the player does not write one. */
const DEFAULT_NOTE = "Backup"

/**
 * How many backups are listed at once.
 *
 * A world backed up after every session has hundreds of entries, and every one
 * of them was drawn into a list with no bottom -- so the newest, which is the
 * only one most people ever want, sat above a page that scrolled for a minute.
 */
const BACKUPS_PER_PAGE = 10

/** How far back the history goes. Older than this, nobody is scrolling. */
const HISTORY_LIMIT = 200

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

/**
 * The tail of a saves folder path.
 *
 * A Prism or MultiMC install nests saves five directories deep, and the full
 * path crowds out everything else in the header while telling the player
 * nothing they did not already know. The last two parts are enough to
 * recognise which install this is; the rest is on the tooltip.
 */
function shortFolder(path: string) {
  const parts = path.split(/[\\/]+/).filter(Boolean)
  if (parts.length <= 2) return path
  return `…${path.includes("\\") ? "\\" : "/"}${parts.slice(-2).join(path.includes("\\") ? "\\" : "/")}`
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
  const [account, setAccount] = useState<GitHubAccount | null>(null)
  const [accountLoaded, setAccountLoaded] = useState(false)
  const [statuses, setStatuses] = useState<Record<string, CloudStatus | null>>({})
  const [worldStates, setWorldStates] = useState<Record<string, WorldState>>({})
  const [historyByWorld, setHistoryByWorld] = useState<Record<string, HistoryEntry[]>>({})
  const [detailsByWorld, setDetailsByWorld] = useState<Record<string, WorldDetails>>({})
  const [note, setNote] = useState("")
  const [historyPage, setHistoryPage] = useState(0)
  const [busy, setBusy] = useState<Busy>(null)
  const [outcome, setOutcome] = useState<Outcome | null>(null)
  const [statusError, setStatusError] = useState("")
  const [checking, setChecking] = useState(false)
  const [checkedAt, setCheckedAt] = useState<string | null>(null)

  const [progress, setProgress] = useState<BackupProgress | null>(null)
  const [logs, setLogs] = useState<LogLine[]>([])
  const [logOpen, setLogOpen] = useState(false)
  const [logFinished, setLogFinished] = useState(false)
  const [operation, setOperation] = useState<Operation>("commit")
  const unlisteners = useRef<Array<() => void>>([])
  const checkingRef = useRef(false)

  const [addOpen, setAddOpen] = useState(false)
  const [connectOpen, setConnectOpen] = useState(false)
  const [removeOpen, setRemoveOpen] = useState(false)
  const [restorePoint, setRestorePoint] = useState<HistoryEntry | null>(null)
  const [settingsOpen, setSettingsOpen] = useState(false)
  const [signInOpen, setSignInOpen] = useState(false)
  const [guideOpen, setGuideOpen] = useState(false)
  const [copiesOpen, setCopiesOpen] = useState(false)
  const [oldCopies, setOldCopies] = useState<OldCopy[]>([])

  /* Saves folder ------------------------------------------------------- */

  useEffect(() => {
    invoke<string>("get_saves_folder")
      .then(setSavesFolder)
      .catch(() => setSavesFolder(""))
    invoke<string>("device_name")
      .then(setThisDevice)
      .catch(() => setThisDevice(""))
    invoke<GitHubAccount | null>("github_account")
      .then(setAccount)
      .catch(() => setAccount(null))
      .finally(() => setAccountLoaded(true))
  }, [])

  // Restores before 0.11 left the world they replaced beside the original,
  // where Minecraft lists it as a world of its own. Nothing else in the app
  // would ever mention them, so they have to be looked for.
  const countOldCopies = useCallback((folder: string) => {
    if (!folder) return
    invoke<OldCopy[]>("list_old_copies", { folder })
      .then(setOldCopies)
      .catch(() => setOldCopies([]))
  }, [])

  useEffect(() => {
    countOldCopies(savesFolder)
  }, [countOldCopies, savesFolder])

  const changeSavesFolder = useCallback((folder: string) => {
    setSavesFolder(folder)
    void invoke("set_saves_folder", { folder }).catch(() => {})
  }, [])

  /* Per-world state ---------------------------------------------------- */

  const readStatus = useCallback(
    async (save: Save, refresh: boolean): Promise<CloudStatus | null> => {
      const load = () =>
        invoke<CloudStatus>("get_cloud_status", {
          gitDir: save.repo_path,
          branch: save.default_branch || "main",
          refresh,
        })

      const succeeded = (status: CloudStatus) => {
        setStatuses((current) => ({ ...current, [save.name]: status }))
        if (refresh) setStatusError("")
        return status
      }
      const failed = (message: string) => {
        setStatuses((current) => ({ ...current, [save.name]: null }))
        if (refresh) setStatusError(message)
        return null
      }

      try {
        return succeeded(await load())
      } catch (err) {
        const message = errorText(err)

        // A tracked world with no backup repository was added before adding
        // one created it. Nothing can happen to that world until it exists and
        // creating it is always what the player wants, so do it and carry on
        // rather than reporting a dead end nothing in the app can clear.
        if (!message.includes("not a bare Git repository")) {
          return failed(cloudErrorLabel(message, t))
        }
        try {
          await invoke("repair_world_repository", { name: save.name })
          return succeeded(await load())
        } catch (repairError) {
          return failed(cloudErrorLabel(errorText(repairError), t))
        }
      }
    },
    [t]
  )

  // What the world says about itself: version, mode, seed, size on disk. None
  // of it is needed to back the world up, so a world that cannot answer simply
  // shows less rather than showing an error.
  const readDetails = useCallback(async (save: Save) => {
    try {
      const found = await invoke<WorldDetails>("world_details", { saveDir: save.path })
      setDetailsByWorld((current) => ({ ...current, [save.name]: found }))
    } catch {
      // Leave whatever was there; an unreadable world is not news.
    }
  }, [])

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

  /* Checking every world ------------------------------------------------ */

  // Only ever when asked. This used to run on a timer, which meant MineCommit
  // reached into every world every few minutes for as long as it was open --
  // and the first thing that turned out to be wrong with what it did there was
  // wrong once every three minutes, all evening, instead of once.
  //
  // One world at a time: each cloud check is a round trip to GitHub, and firing
  // them together on a ten-world folder is a burst of authenticated requests
  // for no gain, since nothing is waiting on the answer.
  const checkEveryWorld = useCallback(async () => {
    // A slow round can still be going when the button is pressed again.
    if (checkingRef.current) return
    checkingRef.current = true
    setChecking(true)
    try {
      for (const save of saves) {
        await readStatus(save, true)
        await readWorldState(save)
      }
      setCheckedAt(new Date().toISOString())
    } finally {
      checkingRef.current = false
      setChecking(false)
    }
  }, [readStatus, readWorldState, saves])

  const refreshSelected = useCallback(async () => {
    if (!selectedSave) return
    await readStatus(selectedSave, true)
    await readWorldState(selectedSave)
    void readDetails(selectedSave)
    try {
      const entries = await invoke<HistoryEntry[]>("list_history", {
        gitDir: selectedSave.repo_path,
        branch: selectedSave.default_branch || "main",
        limit: HISTORY_LIMIT,
      })
      setHistoryByWorld((current) => ({ ...current, [selectedSave.name]: entries }))
    } catch {
      setHistoryByWorld((current) => ({ ...current, [selectedSave.name]: [] }))
    }
  }, [readDetails, readStatus, readWorldState, selectedSave])

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
      // Page four of the last world's backups is not page four of this one's.
      setHistoryPage(0)
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
    setProgress(null)
    setOperation(op)
    unlisteners.current.forEach((stop) => stop())
    const line = await listen<LogLine>("commit-log", (event) => {
      setLogs((current) => [...current, event.payload])
    })
    const moved = await listen<BackupProgress>("backup-progress", (event) => {
      setProgress(event.payload)
    })
    const finished = await listen("commit-finished", () => setLogFinished(true))
    unlisteners.current = [line, moved, finished]
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
  // Clamped rather than stored: a backup taken while page three is open
  // lengthens the history, and a restore that rewinds it can shorten it out
  // from under whichever page is showing.
  const historyPages = Math.max(1, Math.ceil((history?.length ?? 0) / BACKUPS_PER_PAGE))
  const page = Math.min(historyPage, historyPages - 1)
  const firstOnPage = page * BACKUPS_PER_PAGE
  const backupsOnPage = history?.slice(firstOnPage, firstOnPage + BACKUPS_PER_PAGE) ?? []
  const details = selectedSave ? detailsByWorld[selectedSave.name] : undefined
  // Space first, because until now these were only ever described as clutter in
  // the world list -- and the ones that are not in the world list, which is most
  // of them, were never mentioned at all.
  const copiesSize = oldCopies.reduce((sum, copy) => sum + copy.bytes, 0)
  const copiesInGame = oldCopies.filter((copy) => copy.in_saves_folder).length
  const status = selectedSave ? statuses[selectedSave.name] ?? null : null
  const world = selectedSave ? worldStates[selectedSave.name] ?? null : null
  const hasCloud = Boolean(status?.remote_url || selectedSave?.remote_repo_path)
  const situation = useMemo(
    () => situationOf(status, world, hasCloud),
    [hasCloud, status, world]
  )

  /* Actions ------------------------------------------------------------ */

  // Which repositories MineCommit may touch is decided on GitHub, and a player
  // who granted the wrong one has nowhere else to go and change it.
  const openInstallPage = useCallback(async () => {
    try {
      await openUrl(await invoke<string>("github_install_url"))
    } catch (err) {
      console.error("Unable to open GitHub:", err)
    }
  }, [])

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
        {savesFolder ? (
          <button
            type="button"
            onClick={() => setSettingsOpen(true)}
            className="flex min-w-0 items-center gap-2 rounded-md px-2 py-1 text-xs text-muted-foreground hover:bg-muted hover:text-foreground"
            title={savesFolder}
          >
            <Folder className="size-3.5 shrink-0" />
            <span className="truncate font-mono">{shortFolder(savesFolder)}</span>
          </button>
        ) : (
          <Skeleton className="h-6 w-56 rounded-md" />
        )}
        <div className="ml-auto flex items-center gap-3">
          {selectedSave && hasCloud && (
            <span
              className="flex max-w-48 items-center gap-1.5 text-xs whitespace-nowrap text-muted-foreground"
              title={status?.remote_url ?? selectedSave.remote_repo_path}
            >
              <span
                className={cn(
                  "size-1.5 rounded-full",
                  statusError ? "bg-destructive" : "bg-emerald-500"
                )}
              />
              <span className="truncate">
                {repoLabel(status?.remote_url ?? selectedSave.remote_repo_path)}
              </span>
            </span>
          )}
          <AccountMenu
            account={account}
            loaded={accountLoaded}
            onSignIn={() => setSignInOpen(true)}
            onSignedOut={() => setAccount(null)}
            onChooseRepositories={() => void openInstallPage()}
          />
          <Button
            variant="ghost"
            size="icon-sm"
            disabled={checking}
            title={
              checkedAt
                ? t("dash.checkedAgo", { when: relativeTime(checkedAt, locale) })
                : t("dash.checkNow")
            }
            onClick={() => void checkEveryWorld()}
          >
            <RefreshCw className={cn(checking && "animate-spin")} />
            <span className="sr-only">{t("dash.checkNow")}</span>
          </Button>
          <Button
            variant="ghost"
            size="icon-sm"
            title={t("dash.help")}
            onClick={() => setGuideOpen(true)}
          >
            <HelpCircle />
            <span className="sr-only">{t("dash.help")}</span>
          </Button>
          <Button
            variant="ghost"
            size="icon-sm"
            title={t("dash.settings")}
            onClick={() => setSettingsOpen(true)}
          >
            <Settings2 />
            <span className="sr-only">{t("dash.settings")}</span>
          </Button>
        </div>
      </header>

      {oldCopies.length > 0 && (
        <div className="flex shrink-0 items-center gap-3 border-b bg-amber-500/10 px-4 py-2 text-sm">
          <AlertTriangle className="size-4 shrink-0 text-amber-600 dark:text-amber-400" />
          <span className="min-w-0 flex-1 truncate">
            {copiesInGame > 0
              ? t("oldCopies.bannerInGame", {
                  count: oldCopies.length,
                  size: fileSize(copiesSize),
                  inGame: copiesInGame,
                })
              : t("oldCopies.banner", {
                  count: oldCopies.length,
                  size: fileSize(copiesSize),
                })}
          </span>
          <Button size="sm" variant="outline" onClick={() => setCopiesOpen(true)}>
            {t("oldCopies.review")}
          </Button>
        </div>
      )}

      <div className="flex min-h-0 flex-1">
        <nav className="flex w-60 shrink-0 flex-col border-r">
          <p className="px-4 pt-4 pb-2 text-xs font-medium tracking-wide text-muted-foreground uppercase">
            {t("dash.worlds")}
          </p>
          <ul className="min-h-0 flex-1 overflow-y-auto px-2">
            {!loaded &&
              [0, 1, 2].map((row) => (
                <li key={row} className="px-2 py-2">
                  <Skeleton className="h-4 w-32" />
                  <Skeleton className="mt-1.5 h-3 w-20" />
                </li>
              ))}
            {saves.map((save) => {
              const worldSituation = situationOf(
                statuses[save.name] ?? null,
                worldStates[save.name] ?? null,
                Boolean(statuses[save.name]?.remote_url || save.remote_repo_path)
              )
              const badge = BADGES[worldSituation]
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
                    <span className="flex items-center gap-2">
                      <GrassBlock className="size-3.5" />
                      <span className="truncate text-sm">{save.name}</span>
                    </span>
                    {badge ? (
                      <span className={cn("block truncate text-xs", badge.className)}>
                        {t(badge.key)}
                      </span>
                    ) : (
                      // Every other row carries a second line. Leaving this one
                      // blank while the status loads makes the whole list jump
                      // as each world reports in.
                      <Skeleton
                        className="mt-1 h-3 w-16 rounded"
                        title={t("badge.checking")}
                      />
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
              disabled={!loaded}
              onClick={() => setAddOpen(true)}
            >
              <Plus data-icon="inline-start" />
              {t("dash.addWorld")}
            </Button>
          </div>
        </nav>

        <main className="min-w-0 flex-1 overflow-y-auto">
          {!loaded ? (
            <div className="flex h-full flex-col items-center justify-center gap-3">
              <Loader2 className="size-5 animate-spin text-muted-foreground" />
              <p className="text-sm text-muted-foreground">{t("dash.loading")}</p>
            </div>
          ) : saves.length === 0 ? (
            <Welcome
              onAddWorld={() => setAddOpen(true)}
              onOpenGuide={() => setGuideOpen(true)}
            />
          ) : !selectedSave ? (
            <div className="flex h-full flex-col items-center justify-center gap-3 text-center">
              <p className="text-sm text-muted-foreground">{t("dash.selectWorld")}</p>
            </div>
          ) : (
            <div className="mx-auto flex max-w-2xl flex-col gap-6 p-6">
              <div className="flex items-start justify-between gap-4">
                <div className="min-w-0">
                  <h1 className="flex min-w-0 items-center gap-2.5 text-xl font-semibold">
                    <GrassBlock className="size-5" />
                    <span className="truncate">{selectedSave.name}</span>
                  </h1>
                  {status === null && !statusError ? (
                    // "Never backed up" is the wrong thing to say to someone
                    // whose world is backed up; it is only true once the
                    // repository has actually answered.
                    <Skeleton className="mt-1.5 h-4 w-40 rounded" />
                  ) : (
                    <p className="text-sm text-muted-foreground">
                      {status?.local_timestamp
                        ? t("dash.lastBackedUp", {
                            when: relativeTime(status.local_timestamp, locale),
                          })
                        : t("dash.neverBackedUp")}
                      {checking
                        ? ` · ${t("dash.checking")}`
                        : checkedAt
                          ? ` · ${t("dash.checkedAgo", { when: relativeTime(checkedAt, locale) })}`
                          : ""}
                    </p>
                  )}
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
                progress={progress}
                onBackUp={() => void backUp()}
                onGetLatest={() => void getLatest()}
                onConnect={() => setConnectOpen(true)}
                onShowLog={() => setLogOpen(true)}
                onRecheck={() => void refreshSelected()}
                onDismiss={() => setOutcome(null)}
              />

              <WorldDetailsSection details={details} />

              <section className="flex flex-col gap-2">
                <h2 className="text-xs font-medium tracking-wide text-muted-foreground uppercase">
                  {t("dash.history")}
                </h2>
                {history === null ? (
                  <ul className="flex flex-col divide-y">
                    {[0, 1, 2].map((row) => (
                      <li key={row} className="flex items-center gap-3 py-2.5">
                        <Skeleton className="size-2 shrink-0 rounded-full" />
                        <span className="flex min-w-0 flex-1 flex-col gap-1.5">
                          <Skeleton className="h-3.5 w-32 rounded" />
                          <Skeleton className="h-3 w-20 rounded" />
                        </span>
                      </li>
                    ))}
                  </ul>
                ) : history.length === 0 ? (
                  <p className="py-3 text-sm text-muted-foreground">{t("dash.noHistory")}</p>
                ) : (
                  <>
                    <ul className="flex flex-col divide-y">
                      {backupsOnPage.map((entry, index) => (
                        <li
                          key={entry.id}
                          className="group flex items-center gap-3 py-2.5"
                        >
                          <span
                            className={cn(
                              "size-2 shrink-0 rounded-full",
                              firstOnPage + index === 0 ? "bg-primary" : "bg-border"
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
                            className="text-muted-foreground hover:text-foreground"
                            onClick={() => setRestorePoint(entry)}
                            disabled={busy !== null}
                          >
                            <RotateCcw data-icon="inline-start" />
                            {t("dash.restore")}
                          </Button>
                        </li>
                      ))}
                    </ul>
                    {/* Only where there is more than one page. A world with
                        four backups should not be told which page it is on. */}
                    {historyPages > 1 && (
                      <div className="flex items-center justify-between gap-2 pt-1">
                        <Button
                          variant="ghost"
                          size="sm"
                          disabled={page === 0}
                          onClick={() => setHistoryPage(page - 1)}
                        >
                          <ChevronLeft data-icon="inline-start" />
                          {t("dash.newer")}
                        </Button>
                        <span className="text-xs text-muted-foreground">
                          {t("dash.page", { page: page + 1, pages: historyPages })}
                        </span>
                        <Button
                          variant="ghost"
                          size="sm"
                          disabled={page >= historyPages - 1}
                          onClick={() => setHistoryPage(page + 1)}
                        >
                          {t("dash.older")}
                          <ChevronRight data-icon="inline-end" />
                        </Button>
                      </div>
                    )}
                  </>
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
        onSavesFolderChange={changeSavesFolder}
        account={account}
        accountLoaded={accountLoaded}
        onNeedSignIn={() => setSignInOpen(true)}
        onAdded={(name) => {
          // The freshly added world is the one the player came here to see.
          void (async () => {
            await refreshSaves()
            if (!name) return
            const all = await invoke<Save[]>("list_saves")
            const added = all.find((save) => save.name === name)
            if (!added) return
            selectWorld(added)
            if (!added.remote_repo_path) setConnectOpen(true)
          })()
        }}
      />
      <CloudSetupDialog
        key={`connectOpen-${connectOpen}`}
        open={connectOpen}
        onOpenChange={setConnectOpen}
        save={selectedSave}
        account={account}
        onNeedSignIn={() => setSignInOpen(true)}
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
      <GitHubSignInDialog
        key={`signInOpen-${signInOpen}`}
        open={signInOpen}
        onOpenChange={setSignInOpen}
        onSignedIn={(signedIn) => {
          setAccount(signedIn)
          // Signing in is nearly always something the player started in order
          // to connect a world, so carry them back to that.
          if (selectedSave && !selectedSave.remote_repo_path) setConnectOpen(true)
        }}
      />
      <OldCopiesDialog
        key={`copiesOpen-${copiesOpen}`}
        open={copiesOpen}
        onOpenChange={setCopiesOpen}
        savesFolder={savesFolder}
        onCleared={() => countOldCopies(savesFolder)}
      />
      <GuideDialog
        key={`guideOpen-${guideOpen}`}
        open={guideOpen}
        onOpenChange={setGuideOpen}
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

/* ── While something is running ──────────────────────────────────────────── */

/** After this long, silence starts to look like a hang rather than work. */
const SLOW_AFTER_SECONDS = 20

/**
 * A first backup of a large world reads and hashes every region file, which can
 * run for minutes with nothing to show for it. A spinner alone is the same
 * picture as a frozen app, so this counts out loud: the clock keeps moving even
 * when the log line does not, and past twenty seconds it says outright that
 * waiting is the expected outcome.
 */
function WorkingCard({
  busy,
  progress,
  latestLog,
  onShowLog,
}: {
  busy: Exclude<Busy, null>
  progress: BackupProgress | null
  latestLog: string
  onShowLog: () => void
}) {
  const { locale, t } = useI18n()
  const fraction = fractionDone(progress)
  const phase = progress?.phase ?? "idle"

  // Both clocks are measured in Rust, on a clock that stops while the computer
  // is asleep. A laptop suspended mid-backup used to come back reporting the
  // hours it spent asleep as time spent working, and predicting an ending most
  // of a day away. The window cannot measure this itself either way: its timers
  // are throttled when it is not on screen, which is exactly when a long backup
  // is left to run.
  //
  // The estimate comes from the current phase alone. A backup reads the world
  // and then uploads it, and timing the upload against the minutes already
  // spent reading would promise an ending long after the real one.
  //
  // The local timer only fills the gap before the first report arrives, so the
  // card is never sitting at "0s so far" while work is starting.
  const [ticks, setTicks] = useState(0)

  useEffect(() => {
    const timer = setInterval(() => setTicks((count) => count + 1), 1000)
    return () => clearInterval(timer)
  }, [])

  const seconds = progress ? progress.job_seconds : ticks
  const left = secondsRemaining(fraction, progress?.phase_seconds ?? 0)

  const headline = PHASE_TITLE[phase]
  // Reading a world walks real files in the saves folder, and saying how many
  // tells the player something the size does not. Every other phase counts
  // stored pieces or Git objects, which are not files and must not be called
  // any -- that count is already the main readout there.
  const counting = phase === "reading"

  const size = progressReadout(progress, locale, t)

  return (
    <div className="flex flex-col items-center gap-3 rounded-xl border p-8 text-center">
      {fraction === null && <Loader2 className="size-6 animate-spin text-muted-foreground" />}
      <div className="flex w-full max-w-sm flex-col gap-2">
        <p className="text-sm font-medium">
          {headline
            ? t(headline)
            : busy === "backup"
              ? t("state.backingUp")
              : busy === "latest"
                ? t("state.gettingLatest")
                : t("restoreTo.working")}
        </p>

        {fraction !== null && (
          <>
            <div
              role="progressbar"
              aria-valuemin={0}
              aria-valuemax={100}
              aria-valuenow={Math.round(fraction * 100)}
              className="h-2 w-full overflow-hidden rounded-full bg-muted"
            >
              <div
                className="h-full rounded-full bg-primary transition-[width] duration-300 ease-out"
                style={{ width: `${Math.round(fraction * 1000) / 10}%` }}
              />
            </div>
            <p className="flex items-baseline justify-between gap-2 font-mono text-xs text-muted-foreground">
              <span>{Math.floor(fraction * 100)}%</span>
              <span className="truncate">{size}</span>
            </p>
          </>
        )}

        <p className="flex items-baseline justify-between gap-2 font-mono text-xs text-muted-foreground">
          <span>{t("state.elapsed", { time: clock(seconds) })}</span>
          {left !== null && <span>{remainingLabel(left, t)}</span>}
          {left === null && fraction === null && size && <span>{size}</span>}
        </p>

        {/* A count of files says something a size does not: whether the work
            left is one huge region file or ten thousand small ones. During a
            transfer the same number counts Git's objects, which are not files
            and must not be labelled as any. */}
        {counting && progress && progress.files_total > 0 && (
          <p className="font-mono text-xs text-muted-foreground/70">
            {t("state.filesDone", {
              done: progress.files_done.toLocaleString(locale),
              total: progress.files_total.toLocaleString(locale),
            })}
          </p>
        )}
      </div>
      {latestLog && (
        <p className="max-w-full truncate font-mono text-xs text-muted-foreground">
          {latestLog}
        </p>
      )}
      {seconds >= SLOW_AFTER_SECONDS && busy === "backup" && (
        <p className="max-w-sm text-xs text-balance text-muted-foreground">
          {t("state.backingUpSlow")}
        </p>
      )}
      {/* Half a million pieces for one world is a startling number to read
          without being told why there are that many. */}
      {seconds >= SLOW_AFTER_SECONDS && phase === "writing" && (
        <p className="max-w-sm text-xs text-balance text-muted-foreground">
          {t("state.restoringSlow")}
        </p>
      )}
      <Button variant="ghost" size="sm" onClick={onShowLog}>
        {t("dash.showLog")}
      </Button>
    </div>
  )
}


/* ── What the world says about itself ────────────────────────────────────── */

/**
 * Copy text without a clipboard plugin.
 *
 * The webview's clipboard API is the right one and works in a secure context,
 * but it rejects outright in a few packaging setups, and a copy button that
 * silently does nothing is worse than no button. The old selection-based route
 * is deprecated everywhere and still implemented everywhere, which is exactly
 * what a fallback needs to be.
 */
async function copyText(text: string): Promise<boolean> {
  try {
    await navigator.clipboard.writeText(text)
    return true
  } catch {
    try {
      const field = document.createElement("textarea")
      field.value = text
      field.setAttribute("readonly", "")
      field.style.position = "fixed"
      field.style.opacity = "0"
      document.body.appendChild(field)
      field.select()
      const copied = document.execCommand("copy")
      document.body.removeChild(field)
      return copied
    } catch {
      return false
    }
  }
}

function DetailRow({ label, children }: { label: string; children: ReactNode }) {
  return (
    <>
      <dt className="text-muted-foreground">{label}</dt>
      <dd className="min-w-0">{children}</dd>
    </>
  )
}

/** A property of the world that is either on or off, like hardcore. */
function Flag({ label }: { label: string }) {
  return (
    <span className="rounded border px-1.5 py-px text-xs text-muted-foreground">{label}</span>
  )
}

/**
 * The world's own details, as Minecraft records them.
 *
 * Everything here comes out of level.dat and every field of it is optional: the
 * file has changed shape repeatedly over fifteen years and mods add and remove
 * parts of it. A row whose value is missing is left out rather than shown
 * empty, so an old world looks sparse instead of broken.
 */
function WorldDetailsSection({ details }: { details: WorldDetails | undefined }) {
  const { locale, t } = useI18n()
  const [copied, setCopied] = useState(false)
  const [more, setMore] = useState(false)

  useEffect(() => {
    if (!copied) return
    const timer = setTimeout(() => setCopied(false), 1500)
    return () => clearTimeout(timer)
  }, [copied])

  const level: LevelInfo | null = details?.level ?? null
  const mode = gameModeName(level?.game_mode ?? null, t)
  const difficulty = difficultyName(level?.difficulty ?? null, t)
  const day = inGameDay(level)
  // Nothing to open when the world records none of it, which is every world
  // old enough or modded enough not to write these fields.
  const hasMore = Boolean(level && (day !== null || level.seed || level.data_packs.length > 0))

  return (
    <section className="flex flex-col gap-2">
      <h2 className="text-xs font-medium tracking-wide text-muted-foreground uppercase">
        {t("world.details")}
      </h2>

      {details === undefined ? (
        <dl className="grid grid-cols-[max-content_minmax(0,1fr)] gap-x-4 gap-y-2 py-1 text-sm">
          {[0, 1, 2, 3].map((row) => (
            <Fragment key={row}>
              <Skeleton className="h-3.5 w-20 rounded" />
              <Skeleton className="h-3.5 w-32 rounded" />
            </Fragment>
          ))}
        </dl>
      ) : (
        <dl className="grid grid-cols-[max-content_minmax(0,1fr)] items-baseline gap-x-4 gap-y-2 py-1 text-sm">
          {level?.version_name && (
            <DetailRow label={t("world.version")}>
              {level.snapshot_version
                ? t("world.snapshotVersion", { version: level.version_name })
                : level.version_name}
            </DetailRow>
          )}

          {mode && (
            <DetailRow label={t("world.mode")}>
              <span className="flex flex-wrap items-baseline gap-1.5">
                {mode}
                {/* Hardcore is the one that matters most to say out loud: a
                    death ends the world, which is exactly when somebody
                    reaches for a backup. */}
                {level?.hardcore && <Flag label={t("world.hardcore")} />}
                {level?.cheats && <Flag label={t("world.cheats")} />}
                {level?.modded && <Flag label={t("world.modded")} />}
              </span>
            </DetailRow>
          )}

          {difficulty && (
            <DetailRow label={t("world.difficulty")}>
              {level?.difficulty_locked
                ? t("world.difficultyLocked", { difficulty })
                : difficulty}
            </DetailRow>
          )}

          {details && details.bytes > 0 && (
            <DetailRow label={t("world.size")}>{fileSize(details.bytes)}</DetailRow>
          )}

          {more && day !== null && (
            <DetailRow label={t("world.age")}>
              {t("world.day", { day: day.toLocaleString(locale) })}
            </DetailRow>
          )}

          {more && level?.seed && (
            <DetailRow label={t("world.seed")}>
              <span className="flex min-w-0 items-baseline gap-2">
                <span className="min-w-0 truncate font-mono text-xs select-all">
                  {level.seed}
                </span>
                <Button
                  variant="ghost"
                  size="sm"
                  className="h-6 shrink-0 px-1.5 text-xs text-muted-foreground"
                  onClick={() => {
                    copyText(level.seed ?? "")
                      .then(setCopied)
                      .catch(() => setCopied(false))
                  }}
                >
                  <Copy data-icon="inline-start" />
                  {copied ? t("world.copied") : t("world.copySeed")}
                </Button>
              </span>
            </DetailRow>
          )}

          {more && level && level.data_packs.length > 0 && (
            <DetailRow label={t("world.dataPacks")}>
              <span className="break-words">{level.data_packs.join(", ")}</span>
            </DetailRow>
          )}
        </dl>
      )}

      {details && !level && (
        <p className="text-sm text-muted-foreground">{t("world.detailsUnreadable")}</p>
      )}

      {/* What stays open is what tells one world from another at a glance. The
          seed, the data packs and the in-game day are things a player goes
          looking for on purpose, and keeping them open pushed the backups off
          the bottom of the window. Spawn is gone: it is a coordinate the game
          shows on its own compass, and nothing here is decided by it. */}
      {hasMore && (
        <Button
          variant="ghost"
          size="sm"
          className="self-start text-muted-foreground hover:text-foreground"
          onClick={() => setMore((open) => !open)}
        >
          <ChevronDown
            data-icon="inline-start"
            className={cn("transition-transform", more && "rotate-180")}
          />
          {more ? t("world.less") : t("world.more")}
        </Button>
      )}
    </section>
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
  progress,
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
  progress: BackupProgress | null
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
      <WorkingCard
        key={busy}
        busy={busy}
        progress={progress}
        latestLog={latestLog}
        onShowLog={onShowLog}
      />
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
          {/* Above the button, because a note offered afterwards is a note
              nobody writes: the button has already been pressed. */}
          <Textarea
            rows={2}
            value={note}
            placeholder={t("dash.notePlaceholder")}
            aria-label={t("dash.addNote")}
            className="max-w-sm resize-none text-sm"
            onChange={(e) => setNote(e.target.value)}
          />
          <Button
            size="lg"
            variant={situation === "up_to_date" ? "outline" : "default"}
            onClick={onBackUp}
          >
            <Upload data-icon="inline-start" />
            {t("state.backUpNow")}
          </Button>
        </>
      )}

      {(situation === "newer_in_cloud" || situation === "conflict") && (
        <Button size="lg" onClick={onGetLatest}>
          <CloudDownload data-icon="inline-start" />
          {t("state.newerInCloud.action")}
        </Button>
      )}

      {situation === "no_cloud" && (
        <div className="flex flex-col items-center gap-2">
          <Button size="lg" onClick={onConnect}>
            {t("state.noCloud.action")}
          </Button>
          <Button variant="ghost" size="sm" onClick={onBackUp}>
            <Upload data-icon="inline-start" />
            {t("state.backUpNow")}
          </Button>
        </div>
      )}
    </div>
  )
}
