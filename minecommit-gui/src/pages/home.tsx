import { useCallback, useEffect, useRef, useState } from "react"
import { invoke } from "@tauri-apps/api/core"
import { listen } from "@tauri-apps/api/event"
import { openUrl } from "@tauri-apps/plugin-opener"
import { useCommitAuthor } from "@/contexts/commit-author"
import { useI18n, type TranslationKey } from "@/contexts/i18n"
import { Dock } from "@/components/unlumen-ui/dock"
import {
  CloudDownload,
  CloudUpload,
  HardDriveDownload,
  HardDriveUpload,
  RefreshCw,
} from "lucide-react"
import { Button } from "@/components/ui/button"
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card"
import { useSaves } from "@/contexts/saves"
import {
  Dialog,
  DialogClose,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog"
import { Field, FieldGroup } from "@/components/ui/field"
import { Input } from "@/components/ui/input"
import { Label } from "@/components/ui/label"
import { Textarea } from "@/components/ui/textarea"
import { RollingLogDialog, type Operation } from "@/components/rolling-log"
import type { LogLine } from "@/components/log-viewer"
import { SaveHoverCard } from "@/components/save-hover-card"

function CommitDialog({
  open,
  onOpenChange,
  onCommitStart,
}: {
  open: boolean
  onOpenChange: (open: boolean) => void
  onCommitStart: () => void
}) {
  const { author } = useCommitAuthor()
  const { selectedSave } = useSaves()
  const { t } = useI18n()
  const [committing, setCommitting] = useState(false)
  const [message, setMessage] = useState("-")
  const [name, setName] = useState(author.name || "")

  const [email, setEmail] = useState(author.email || "")


  const branch = selectedSave?.default_branch ?? "main"

  const handleSubmit = useCallback(async () => {
    if (!selectedSave || committing) return
    setCommitting(true)

    const finalMessage = message || "-"
    const authorName = name || ""
    const authorEmail = email || ""

    // Open log dialog and close commit dialog immediately
    onOpenChange(false)
    onCommitStart()

    invoke<{ success: boolean; error: string | null }>("perform_commit", {
      saveDir: selectedSave.path,
      gitDir: selectedSave.repo_path,
      branch,
      message: finalMessage,
      authorName,
      authorEmail,
      extraPatterns: [],
      ignorePatterns: [],
      useRepack: true,
    })
      .then((result) => {
        if (!result.success) {
          console.error("Commit failed:", result.error)
        }
      })
      .catch((err) => {
        console.error("Commit error:", err)
      })
      .finally(() => {
        setCommitting(false)
      })
  }, [
    selectedSave,
    committing,
    message,
    name,
    email,
    branch,
    onCommitStart,
    onOpenChange,
  ])

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-lg">
        <DialogHeader>
          <DialogTitle>{t("commit.title")}</DialogTitle>
          <DialogDescription>{t("commit.description")}</DialogDescription>
        </DialogHeader>
        <FieldGroup>
          <Field>
            <Label htmlFor="branch">{t("commit.branch")}</Label>
            <Input id="branch" name="branch" value={branch} disabled />
          </Field>
          <Field>
            <Label htmlFor="message">{t("commit.message")}</Label>
            <Textarea
              id="message"
              name="message"
              placeholder={t("commit.messagePlaceholder")}
              value={message}
              onChange={(e) => setMessage(e.target.value)}
            />
          </Field>
          <Field>
            <Label htmlFor="name">{t("commit.playerName")}</Label>
            <Input
              id="name"
              name="name"
              placeholder={t("commit.playerNamePlaceholder")}
              value={name}
              onChange={(e) => setName(e.target.value)}
            />
          </Field>
          <Field>
            <Label htmlFor="email">{t("commit.email")}</Label>
            <Input
              id="email"
              name="email"
              type="email"
              placeholder={t("commit.emailPlaceholder")}
              value={email}
              onChange={(e) => setEmail(e.target.value)}
            />
          </Field>
        </FieldGroup>
        <DialogFooter>
          <DialogClose
            render={
              <Button variant="outline" disabled={committing}>
                {t("common.cancel")}
              </Button>
            }
          ></DialogClose>
          <Button onClick={handleSubmit} disabled={committing}>
            {committing ? t("commit.creating") : t("commit.create")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}

function RestoreDialog({
  open,
  onOpenChange,
  onRestoreStart,
}: {
  open: boolean
  onOpenChange: (open: boolean) => void
  onRestoreStart: () => void
}) {
  const { selectedSave } = useSaves()
  const { t } = useI18n()

  const handleRestore = useCallback(async () => {
    if (!selectedSave) return
    onOpenChange(false)
    onRestoreStart()

    invoke<{ success: boolean; error: string | null }>("perform_restore", {
      saveDir: selectedSave.path,
      gitDir: selectedSave.repo_path,
      branch: selectedSave.default_branch || "main",
    })
      .then((result) => {
        if (!result.success) {
          console.error("Restore failed:", result.error)
        }
      })
      .catch((err) => {
        console.error("Restore error:", err)
      })
  }, [selectedSave, onOpenChange, onRestoreStart])

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="min-w-lg break-all">
        <DialogHeader>
          <DialogTitle>{t("restore.title")}</DialogTitle>
          <DialogDescription>{t("restore.description")}</DialogDescription>
        </DialogHeader>
        {selectedSave && (
          <SaveHoverCard save={selectedSave}>
            <Button variant="link">{selectedSave.name}</Button>
          </SaveHoverCard>
        )}
        <DialogFooter>
          <DialogClose
            render={<Button variant="outline">{t("common.cancel")}</Button>}
          ></DialogClose>
          <Button onClick={handleRestore}>{t("restore.action")}</Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}

function PushDialog({
  open,
  onOpenChange,
  onPushStart,
}: {
  open: boolean
  onOpenChange: (open: boolean) => void
  onPushStart: () => void
}) {
  const { selectedSave } = useSaves()
  const { t } = useI18n()

  const [pushing, setPushing] = useState(false)
  const [remote, setRemote] = useState(selectedSave?.remote_repo_path ?? "")
  const [branch, setBranch] = useState(selectedSave?.default_branch ?? "main")

  const handlePush = useCallback(async () => {
    const save = selectedSave
    if (!save || pushing || !branch) return
    setPushing(true)

    onOpenChange(false)
    onPushStart()

    invoke<{ success: boolean; error: string | null }>("perform_push", {
      gitDir: save.repo_path,
      remote,
      branch,
    })
      .then((result) => {
        if (!result.success) {
          console.error("Push failed:", result.error)
        }
      })
      .catch((err) => {
        console.error("Push error:", err)
      })
      .finally(() => {
        setPushing(false)
      })
  }, [selectedSave, pushing, remote, branch, onPushStart, onOpenChange])

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-lg">
        <DialogHeader>
          <DialogTitle>{t("push.title")}</DialogTitle>
        </DialogHeader>
        <FieldGroup>
          <Field>
            <Label htmlFor="push-remote">{t("push.remote")}</Label>
            <Input
              id="push-remote"
              name="push-remote"
              placeholder="https://example.com/user/save.git"
              value={remote}
              onChange={(e) => setRemote(e.target.value)}
            />
          </Field>
          <Field>
            <Label htmlFor="push-branch">{t("push.branch")}</Label>
            <Input
              id="push-branch"
              name="push-branch"
              placeholder="main"
              value={branch}
              onChange={(e) => setBranch(e.target.value)}
            />
          </Field>
        </FieldGroup>
        <DialogFooter>
          <DialogClose
            render={
              <Button variant="outline" disabled={pushing}>
                {t("common.cancel")}
              </Button>
            }
          ></DialogClose>
          <Button onClick={handlePush} disabled={pushing || !branch}>
            {pushing ? t("push.uploading") : t("push.action")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}

function PullDialog({
  open,
  onOpenChange,
  onPullStart,
}: {
  open: boolean
  onOpenChange: (open: boolean) => void
  onPullStart: () => void
}) {
  const { selectedSave } = useSaves()
  const { t } = useI18n()
  const [pulling, setPulling] = useState(false)
  const [remote, setRemote] = useState(selectedSave?.remote_repo_path ?? "")
  const [branch, setBranch] = useState(selectedSave?.default_branch ?? "main")


  const handlePull = useCallback(async () => {
    const save = selectedSave
    if (!save || pulling || !branch) return
    setPulling(true)

    onOpenChange(false)
    onPullStart()

    invoke<{ success: boolean; error: string | null }>("perform_pull", {
      saveDir: save.path,
      gitDir: save.repo_path,
      remote,
      branch,
    })
      .then((result) => {
        if (!result.success) {
          console.error("Pull failed:", result.error)
        }
      })
      .catch((err) => {
        console.error("Pull error:", err)
      })
      .finally(() => {
        setPulling(false)
      })
  }, [selectedSave, pulling, remote, branch, onPullStart, onOpenChange])

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-lg">
        <DialogHeader>
          <DialogTitle>{t("pull.title")}</DialogTitle>
        </DialogHeader>
        <FieldGroup>
          <Field>
            <Label htmlFor="pull-remote">{t("pull.remote")}</Label>
            <Input
              id="pull-remote"
              name="pull-remote"
              placeholder="https://example.com/user/save.git"
              value={remote}
              onChange={(e) => setRemote(e.target.value)}
            />
          </Field>
          <Field>
            <Label htmlFor="pull-branch">{t("pull.branch")}</Label>
            <Input
              id="pull-branch"
              name="pull-branch"
              placeholder="main"
              value={branch}
              onChange={(e) => setBranch(e.target.value)}
            />
          </Field>
        </FieldGroup>
        <DialogFooter>
          <DialogClose
            render={
              <Button variant="outline" disabled={pulling}>
                {t("common.cancel")}
              </Button>
            }
          ></DialogClose>
          <Button onClick={handlePull} disabled={pulling || !branch}>
            {pulling ? t("pull.downloading") : t("pull.action")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}

type CloudState =
  | "not_configured"
  | "empty"
  | "up_to_date"
  | "local_ahead"
  | "remote_ahead"
  | "diverged"

interface CloudStatus {
  remote_name: string | null
  remote_url: string | null
  branch: string
  local_commit: string | null
  local_timestamp: string | null
  local_device: string | null
  remote_commit: string | null
  remote_timestamp: string | null
  remote_device: string | null
  state: CloudState
}

const CLOUD_STATE_KEYS: Record<CloudState, TranslationKey> = {
  not_configured: "cloud.notConfigured",
  empty: "cloud.empty",
  up_to_date: "cloud.upToDate",
  local_ahead: "cloud.localAhead",
  remote_ahead: "cloud.remoteAhead",
  diverged: "cloud.diverged",
}

function cloudErrorLabel(error: string, t: (key: TranslationKey) => string) {
  const lower = error.toLowerCase()
  if (lower.includes("failed to start git") || lower.includes("install git")) {
    return t("cloud.gitMissing")
  }
  if (
    lower.includes("authentication") ||
    lower.includes("permission denied") ||
    lower.includes("could not read username") ||
    lower.includes("terminal prompts disabled")
  ) {
    return t("cloud.authenticationFailed")
  }
  if (
    lower.includes("network") ||
    lower.includes("resolve host") ||
    lower.includes("connection") ||
    lower.includes("timed out") ||
    lower.includes("unreachable")
  ) {
    return t("cloud.networkUnavailable")
  }
  return error
}

function shortCommit(commit: string | null) {
  return commit ? commit.slice(0, 12) : "—"
}

function openExternal(url: string) {
  void openUrl(url).catch((error) => {
    console.error("Unable to open browser:", error)
  })
}

function backupDetails(
  commit: string | null,
  timestamp: string | null,
  device: string | null,
  locale: string,
  t: (key: TranslationKey) => string
) {
  if (!commit) return "—"
  const time = timestamp ? new Date(timestamp).toLocaleString(locale) : t("cloud.unknownTime")
  const source = device || t("cloud.unknownDevice")
  return `${time} · ${source} · ${shortCommit(commit)}`
}

function CloudSyncCard({
  onSync,
  onSetup,
  onCommit,
  onUpload,
  refreshToken,
}: {
  onSync: () => void
  onSetup: () => void
  onCommit: () => void
  onUpload: () => void
  refreshToken: number
}) {
  const { selectedSave } = useSaves()
  const { locale, t } = useI18n()
  const [status, setStatus] = useState<CloudStatus | null>(null)
  const [error, setError] = useState("")
  const [checking, setChecking] = useState(false)

  const refresh = useCallback(async () => {
    if (!selectedSave) return
    setChecking(true)
    setError("")
    try {
      const next = await invoke<CloudStatus>("get_cloud_status", {
        gitDir: selectedSave.repo_path,
        branch: selectedSave.default_branch || "main",
        refresh: true,
      })
      setStatus(next)
    } catch (err) {
      setStatus(null)
      setError(cloudErrorLabel(String(err), t))
    } finally {
      setChecking(false)
    }
  }, [selectedSave, t])

  useEffect(() => {
    void refresh()
  }, [refresh, refreshToken])

  if (!selectedSave) return null

  const state = status?.state
  const isConflict = state === "diverged"
  const remote = status?.remote_url || selectedSave.remote_repo_path || t("cloud.notConfigured")
  const cloudConfigured = Boolean(status?.remote_url || selectedSave.remote_repo_path)

  return (
    <Card className="w-full max-w-xl">
      <CardHeader className="gap-2">
        <div className="flex items-center justify-between gap-4">
          <CardTitle className="text-base">{selectedSave.name}</CardTitle>
          <Button variant="ghost" size="icon-sm" onClick={() => void refresh()} disabled={checking}>
            <RefreshCw className={checking ? "animate-spin" : ""} />
            <span className="sr-only">{t("cloud.refreshStatus")}</span>
          </Button>
        </div>
        <CardDescription>
          {t("cloud.status", {
            status: checking ? t("cloud.checking") : state ? t(CLOUD_STATE_KEYS[state]) : t("cloud.cannotCheck"),
          })}
        </CardDescription>
      </CardHeader>
      <CardContent className="flex flex-col gap-3 text-sm">
        <div className="grid grid-cols-[auto_1fr] gap-x-4 gap-y-1 break-all text-muted-foreground">
          <span>{t("cloud.remote")}</span>
          <span>{remote}</span>
          <span>{t("cloud.branch")}</span>
          <span>{status?.branch || selectedSave.default_branch || "main"}</span>
          <span>{t("cloud.localBackup")}</span>
          <span>
            {backupDetails(
              status?.local_commit ?? null,
              status?.local_timestamp ?? null,
              status?.local_device ?? null,
              locale,
              t
            )}
          </span>
          <span>{t("cloud.cloudBackup")}</span>
          <span>
            {backupDetails(
              status?.remote_commit ?? null,
              status?.remote_timestamp ?? null,
              status?.remote_device ?? null,
              locale,
              t
            )}
          </span>
        </div>
        {error && <p className="text-destructive">{error}</p>}
        {isConflict && (
          <p className="text-destructive">
            {t("cloud.conflictHelp")}
          </p>
        )}
        <div className="flex flex-wrap gap-2 pt-1">
          {!cloudConfigured ? (
            <Button onClick={onSetup} disabled={checking}>
              <CloudUpload data-icon="inline-start" />
              {t("cloud.enable")}
            </Button>
          ) : state === "empty" ? (
            <Button onClick={onCommit} disabled={checking}>
              <HardDriveDownload data-icon="inline-start" />
              {t("cloud.firstBackup")}
            </Button>
          ) : state === "local_ahead" ? (
            <Button onClick={onUpload} disabled={checking}>
              <CloudUpload data-icon="inline-start" />
              {t("cloud.upload")}
            </Button>
          ) : state === "remote_ahead" || state === "up_to_date" ? (
            <Button onClick={onSync} disabled={checking || isConflict}>
              <CloudDownload data-icon="inline-start" />
              {t("cloud.sync")}
            </Button>
          ) : (
            <Button onClick={() => void refresh()} disabled={checking}>
              <RefreshCw data-icon="inline-start" />
              {t("cloud.recheck")}
            </Button>
          )}
        </div>
      </CardContent>
    </Card>
  )
}

type CloudSetupStep = "welcome" | "create-account" | "existing-account" | "repository"

function CloudSetupDialog({
  open,
  onOpenChange,
  onConfigured,
}: {
  open: boolean
  onOpenChange: (open: boolean) => void
  onConfigured: () => Promise<void>
}) {
  const { selectedSave } = useSaves()
  const { t } = useI18n()
  const [step, setStep] = useState<CloudSetupStep>("welcome")
  const [remoteUrl, setRemoteUrl] = useState("")
  const [branch, setBranch] = useState("main")
  const [connecting, setConnecting] = useState(false)
  const [error, setError] = useState("")

  useEffect(() => {
    if (open) {
      setStep("welcome")
      setRemoteUrl(selectedSave?.remote_repo_path ?? "")
      setBranch(selectedSave?.default_branch || "main")
      setConnecting(false)
      setError("")
    }
  }, [open, selectedSave])

  async function connectCloud() {
    if (!selectedSave || !remoteUrl.trim() || !branch.trim()) return
    setConnecting(true)
    setError("")
    try {
      await invoke("configure_save_cloud", {
        name: selectedSave.name,
        remoteUrl: remoteUrl.trim(),
        branch: branch.trim(),
      })
      await onConfigured()
      onOpenChange(false)
    } catch (err) {
      setError(cloudErrorLabel(String(err), t))
    } finally {
      setConnecting(false)
    }
  }

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-xl">
        {step === "welcome" && (
          <>
            <DialogHeader>
              <DialogTitle>{t("wizard.title")}</DialogTitle>
              <DialogDescription>{t("wizard.welcome", { world: selectedSave?.name })}</DialogDescription>
            </DialogHeader>
            <div className="grid gap-3 py-2 sm:grid-cols-2">
              <Button className="h-auto justify-start py-4 text-left" onClick={() => setStep("existing-account")}>
                <span>
                  <span className="block">{t("wizard.haveAccount")}</span>
                  <span className="mt-1 block text-xs font-normal opacity-75">
                    {t("wizard.haveAccountDetail")}
                  </span>
                </span>
              </Button>
              <Button className="h-auto justify-start py-4 text-left" variant="outline" onClick={() => setStep("create-account")}>
                <span>
                  <span className="block">{t("wizard.noAccount")}</span>
                  <span className="mt-1 block text-xs font-normal text-muted-foreground">
                    {t("wizard.noAccountDetail")}
                  </span>
                </span>
              </Button>
            </div>
            <p className="text-sm text-muted-foreground">
              {t("wizard.needGit")} {" "}
              <button type="button" className="text-primary underline" onClick={() => openExternal("https://git-scm.com/downloads")}>{t("wizard.downloadGit")}</button>
              {t("wizard.restartAfterGit")}
            </p>
            <DialogFooter>
              <DialogClose render={<Button variant="outline" />}>{t("wizard.later")}</DialogClose>
            </DialogFooter>
          </>
        )}

        {step === "create-account" && (
          <>
            <DialogHeader>
              <DialogTitle>{t("wizard.createAccountTitle")}</DialogTitle>
              <DialogDescription>{t("wizard.createAccountDescription")}</DialogDescription>
            </DialogHeader>
            <ol className="list-decimal space-y-2 pl-5 text-sm text-muted-foreground">
              <li>
                {t("wizard.createAccountStep1")} {" "}
                <button type="button" className="text-primary underline" onClick={() => openExternal("https://github.com/signup")}>github.com/signup</button>{" "}
                {t("wizard.createAccountStep1After")}
              </li>
              <li>{t("wizard.createAccountStep2")}</li>
              <li>{t("wizard.createAccountStep3")}</li>
            </ol>
            <DialogFooter>
              <Button variant="outline" onClick={() => setStep("welcome")}>{t("common.back")}</Button>
              <Button onClick={() => setStep("repository")}>{t("wizard.loggedIn")}</Button>
            </DialogFooter>
          </>
        )}

        {step === "existing-account" && (
          <>
            <DialogHeader>
              <DialogTitle>{t("wizard.existingAccountTitle")}</DialogTitle>
              <DialogDescription>{t("wizard.existingAccountDescription")}</DialogDescription>
            </DialogHeader>
            <p className="text-sm text-muted-foreground">
              {t("wizard.loginPrompt")} {" "}
              <button type="button" className="text-primary underline" onClick={() => openExternal("https://github.com/login")}>github.com/login</button>{t("wizard.loginPromptAfter")}
            </p>
            <DialogFooter>
              <Button variant="outline" onClick={() => setStep("welcome")}>{t("common.back")}</Button>
              <Button onClick={() => setStep("repository")}>{t("wizard.next")}</Button>
            </DialogFooter>
          </>
        )}

        {step === "repository" && (
          <>
            <DialogHeader>
              <DialogTitle>{t("wizard.repositoryTitle")}</DialogTitle>
              <DialogDescription>{t("wizard.repositoryDescription", { world: selectedSave?.name })}</DialogDescription>
            </DialogHeader>
            <ol className="list-decimal space-y-2 pl-5 text-sm text-muted-foreground">
              <li>
                {t("wizard.repositoryStep1")} {" "}
                <button type="button" className="text-primary underline" onClick={() => openExternal("https://github.com/new?visibility=private")}>{t("wizard.newRepository")}</button>{" "}
                {t("wizard.repositoryStep1After")} <code>minecraft-survival-backup</code>
              </li>
              <li>{t("wizard.repositoryStep2")}</li>
              <li>
                {t("wizard.repositoryStep3")}
              </li>
              <li>{t("wizard.repositoryStep4")}</li>
            </ol>
            <FieldGroup className="pt-2">
              <Field>
                <Label htmlFor="cloud-url">{t("wizard.repositoryAddress")}</Label>
                <Input
                  id="cloud-url"
                  placeholder={t("wizard.repositoryAddressPlaceholder")}
                  value={remoteUrl}
                  onChange={(event) => setRemoteUrl(event.target.value)}
                />
              </Field>
              <Field>
                <Label htmlFor="cloud-branch">{t("wizard.backupBranch")}</Label>
                <Input id="cloud-branch" value={branch} onChange={(event) => setBranch(event.target.value)} />
              </Field>
            </FieldGroup>
            <p className="text-sm text-muted-foreground">
              {t("wizard.credentialHelp")}
            </p>
            {error && <p className="text-sm text-destructive">{error}</p>}
            <DialogFooter>
              <Button variant="outline" onClick={() => setStep("welcome")} disabled={connecting}>{t("common.back")}</Button>
              <Button onClick={() => void connectCloud()} disabled={connecting || !remoteUrl.trim() || !branch.trim()}>
                {connecting ? t("wizard.connecting") : t("wizard.connect")}
              </Button>
            </DialogFooter>
          </>
        )}
      </DialogContent>
    </Dialog>
  )
}

export function HomePage() {
  const { selectedSave, refreshSaves } = useSaves()
  const { t } = useI18n()
  const [commitDialogOpen, setCommitDialogOpen] = useState(false)
  const [restoreDialogOpen, setRestoreDialogOpen] = useState(false)
  const [pushDialogOpen, setPushDialogOpen] = useState(false)
  const [pullDialogOpen, setPullDialogOpen] = useState(false)
  const [cloudSetupOpen, setCloudSetupOpen] = useState(false)
  const [logDialogOpen, setLogDialogOpen] = useState(false)
  const [commitDialogKey, setCommitDialogKey] = useState(0)
  const [pushDialogKey, setPushDialogKey] = useState(0)
  const [pullDialogKey, setPullDialogKey] = useState(0)
  const [cloudRefreshToken, setCloudRefreshToken] = useState(0)
  const [operation, setOperation] = useState<Operation>("commit")
  const [commitLogs, setCommitLogs] = useState<LogLine[]>([])
  const [commitFinished, setCommitFinished] = useState(false)
  const unlistenRefs = useRef<Array<() => void>>([])

  // Clean up event listeners when log dialog closes
  useEffect(() => {
    if (!logDialogOpen) {
      unlistenRefs.current.forEach((fn) => fn())
      unlistenRefs.current = []
    }
  }, [logDialogOpen])

  const setupLogListeners = useCallback(async (op: Operation) => {
    setCommitLogs([])
    setCommitFinished(false)
    setOperation(op)

    unlistenRefs.current.forEach((fn) => fn())
    unlistenRefs.current = []

    const unlisten1 = await listen<LogLine>("commit-log", (event) => {
      setCommitLogs((prev) => [...prev, event.payload])
    })
    const unlisten2 = await listen("commit-finished", () => {
      setCommitFinished(true)
      setCloudRefreshToken((token) => token + 1)
    })
    unlistenRefs.current = [unlisten1, unlisten2]

    setLogDialogOpen(true)
  }, [])

  const handleCommitStart = useCallback(async () => {
    await setupLogListeners("commit")
  }, [setupLogListeners])

  const handleRestoreStart = useCallback(async () => {
    await setupLogListeners("restore")
  }, [setupLogListeners])

  const handlePushStart = useCallback(async () => {
    await setupLogListeners("push")
  }, [setupLogListeners])

  const handlePullStart = useCallback(async () => {
    await setupLogListeners("pull")
  }, [setupLogListeners])

  const items = [
    {
      icon: <HardDriveDownload />,
      label: t("dock.backup"),
      onClick: () => {
        setCommitDialogKey((k) => k + 1)
        setCommitDialogOpen(true)
      },
    },
    {
      icon: <HardDriveUpload />,
      label: t("dock.restore"),
      onClick: () => setRestoreDialogOpen(true),
      separator: true,
    },
    {
      icon: <CloudUpload />,
      label: t("dock.upload"),
      onClick: () => {
        setPushDialogKey((k) => k + 1)
        setPushDialogOpen(true)
      },
    },
    {
      icon: <CloudDownload />,
      label: t("dock.download"),
      onClick: () => {
        setPullDialogKey((k) => k + 1)
        setPullDialogOpen(true)
      },
    },
  ]

  return (
    <div className="flex w-full flex-col items-center justify-center gap-4">
      <CloudSyncCard
        onSync={() => {
          setPullDialogKey((k) => k + 1)
          setPullDialogOpen(true)
        }}
        onSetup={() => setCloudSetupOpen(true)}
        onCommit={() => {
          setCommitDialogKey((k) => k + 1)
          setCommitDialogOpen(true)
        }}
        onUpload={() => {
          setPushDialogKey((k) => k + 1)
          setPushDialogOpen(true)
        }}
        refreshToken={cloudRefreshToken}
      />
      <Dock items={items} />
      {selectedSave && (
        <SaveHoverCard save={selectedSave}>
          <Button variant="link" className="text-muted-foreground">
            {selectedSave.name}
          </Button>
        </SaveHoverCard>
      )}
      <CommitDialog
        key={`commit-${commitDialogKey}`}
        open={commitDialogOpen}
        onOpenChange={setCommitDialogOpen}
        onCommitStart={handleCommitStart}
      />
      <RestoreDialog
        open={restoreDialogOpen}
        onOpenChange={setRestoreDialogOpen}
        onRestoreStart={handleRestoreStart}
      />
      <PushDialog
        key={`push-${pushDialogKey}`}
        open={pushDialogOpen}
        onOpenChange={setPushDialogOpen}
        onPushStart={handlePushStart}
      />
      <PullDialog
        key={`pull-${pullDialogKey}`}
        open={pullDialogOpen}
        onOpenChange={setPullDialogOpen}
        onPullStart={handlePullStart}
      />
      <CloudSetupDialog
        open={cloudSetupOpen}
        onOpenChange={setCloudSetupOpen}
        onConfigured={refreshSaves}
      />
      <RollingLogDialog
        open={logDialogOpen}
        onOpenChange={setLogDialogOpen}
        operation={operation}
        logs={commitLogs}
        finished={commitFinished}
      />
    </div>
  )
}
