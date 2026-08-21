import { useCallback, useEffect, useRef, useState } from "react"
import { invoke } from "@tauri-apps/api/core"
import { listen } from "@tauri-apps/api/event"
import { openUrl } from "@tauri-apps/plugin-opener"
import { useCommitAuthor } from "@/contexts/commit-author"
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
          <DialogTitle>提交到 Git 以备份</DialogTitle>
          <DialogDescription>填写提交信息作为备注</DialogDescription>
        </DialogHeader>
        <FieldGroup>
          <Field>
            <Label htmlFor="branch">分支</Label>
            <Input id="branch" name="branch" value={branch} disabled />
          </Field>
          <Field>
            <Label htmlFor="message">提交信息</Label>
            <Textarea
              id="message"
              name="message"
              placeholder="例如：刷怪塔完工"
              value={message}
              onChange={(e) => setMessage(e.target.value)}
            />
          </Field>
          <Field>
            <Label htmlFor="name">你的游戏昵称</Label>
            <Input
              id="name"
              name="name"
              placeholder="例如：HairlessVillager"
              value={name}
              onChange={(e) => setName(e.target.value)}
            />
          </Field>
          <Field>
            <Label htmlFor="email">联系邮箱</Label>
            <Input
              id="email"
              name="email"
              type="email"
              placeholder="例如：hairlessvilager@foxmail.com"
              value={email}
              onChange={(e) => setEmail(e.target.value)}
            />
          </Field>
        </FieldGroup>
        <DialogFooter>
          <DialogClose
            render={
              <Button variant="outline" disabled={committing}>
                取消
              </Button>
            }
          ></DialogClose>
          <Button onClick={handleSubmit} disabled={committing}>
            {committing ? "提交中..." : "提交"}
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
          <DialogTitle>确定要恢复最近提交吗？</DialogTitle>
          <DialogDescription>
            这将会用 Git
            仓库中最新的提交覆盖当前存档。如果存档已存在，将被重命名为
            .&lt;时间戳&gt;.snapshot 备份。
          </DialogDescription>
        </DialogHeader>
        {selectedSave && (
          <SaveHoverCard save={selectedSave}>
            <Button variant="link">{selectedSave.name}</Button>
          </SaveHoverCard>
        )}
        <DialogFooter>
          <DialogClose
            render={<Button variant="outline">取消</Button>}
          ></DialogClose>
          <Button onClick={handleRestore}>恢复</Button>
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
          <DialogTitle>推送分支到远程仓库</DialogTitle>
        </DialogHeader>
        <FieldGroup>
          <Field>
            <Label htmlFor="push-remote">远程仓库地址</Label>
            <Input
              id="push-remote"
              name="push-remote"
              placeholder="https://example.com/user/save.git"
              value={remote}
              onChange={(e) => setRemote(e.target.value)}
            />
          </Field>
          <Field>
            <Label htmlFor="push-branch">推送分支</Label>
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
                取消
              </Button>
            }
          ></DialogClose>
          <Button onClick={handlePush} disabled={pushing || !branch}>
            {pushing ? "推送中..." : "推送"}
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
          <DialogTitle>从远程仓库拉取分支</DialogTitle>
        </DialogHeader>
        <FieldGroup>
          <Field>
            <Label htmlFor="pull-remote">远程仓库地址</Label>
            <Input
              id="pull-remote"
              name="pull-remote"
              placeholder="https://example.com/user/save.git"
              value={remote}
              onChange={(e) => setRemote(e.target.value)}
            />
          </Field>
          <Field>
            <Label htmlFor="pull-branch">拉取分支</Label>
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
                取消
              </Button>
            }
          ></DialogClose>
          <Button onClick={handlePull} disabled={pulling || !branch}>
            {pulling ? "拉取中..." : "拉取"}
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

const CLOUD_STATE_LABELS: Record<CloudState, string> = {
  not_configured: "未配置",
  empty: "尚无备份",
  up_to_date: "已同步",
  local_ahead: "本地备份等待上传",
  remote_ahead: "有可下载的云端备份",
  diverged: "检测到冲突",
}

function cloudErrorLabel(error: string) {
  const lower = error.toLowerCase()
  if (lower.includes("failed to start git") || lower.includes("install git")) {
    return "未找到 Git。请安装 Git 后重新打开 MineCommit；不需要学习或运行任何 Git 命令。"
  }
  if (
    lower.includes("authentication") ||
    lower.includes("permission denied") ||
    lower.includes("could not read username") ||
    lower.includes("terminal prompts disabled")
  ) {
    return "认证失败：请使用 Git Credential Manager、SSH 密钥或已配置的 Git 凭据。"
  }
  if (
    lower.includes("network") ||
    lower.includes("resolve host") ||
    lower.includes("connection") ||
    lower.includes("timed out") ||
    lower.includes("unreachable")
  ) {
    return "网络不可用：请检查网络连接和远程地址。"
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
  device: string | null
) {
  if (!commit) return "—"
  const time = timestamp ? new Date(timestamp).toLocaleString() : "时间未知"
  const source = device || "设备未知"
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
      setError(cloudErrorLabel(String(err)))
    } finally {
      setChecking(false)
    }
  }, [selectedSave])

  useEffect(() => {
    void refresh()
  }, [refresh, refreshToken])

  if (!selectedSave) return null

  const state = status?.state
  const isConflict = state === "diverged"
  const remote = status?.remote_url || selectedSave.remote_repo_path || "未配置"
  const cloudConfigured = Boolean(status?.remote_url || selectedSave.remote_repo_path)

  return (
    <Card className="w-full max-w-xl">
      <CardHeader className="gap-2">
        <div className="flex items-center justify-between gap-4">
          <CardTitle className="text-base">{selectedSave.name}</CardTitle>
          <Button variant="ghost" size="icon-sm" onClick={() => void refresh()} disabled={checking}>
            <RefreshCw className={checking ? "animate-spin" : ""} />
            <span className="sr-only">刷新云端状态</span>
          </Button>
        </div>
        <CardDescription>
          云端状态：{checking ? "检查中…" : state ? CLOUD_STATE_LABELS[state] : "无法检查"}
        </CardDescription>
      </CardHeader>
      <CardContent className="flex flex-col gap-3 text-sm">
        <div className="grid grid-cols-[auto_1fr] gap-x-4 gap-y-1 break-all text-muted-foreground">
          <span>远程</span>
          <span>{remote}</span>
          <span>分支</span>
          <span>{status?.branch || selectedSave.default_branch || "main"}</span>
          <span>本地备份</span>
          <span>
            {backupDetails(
              status?.local_commit ?? null,
              status?.local_timestamp ?? null,
              status?.local_device ?? null
            )}
          </span>
          <span>云端备份</span>
          <span>
            {backupDetails(
              status?.remote_commit ?? null,
              status?.remote_timestamp ?? null,
              status?.remote_device ?? null
            )}
          </span>
        </div>
        {error && <p className="text-destructive">{error}</p>}
        {isConflict && (
          <p className="text-destructive">
            本地和云端都已变化。MineCommit 不会自动合并 Minecraft 存档，两个历史都已保留。
          </p>
        )}
        <div className="flex flex-wrap gap-2 pt-1">
          {!cloudConfigured ? (
            <Button onClick={onSetup} disabled={checking}>
              <CloudUpload data-icon="inline-start" />
              启用云端备份
            </Button>
          ) : state === "empty" ? (
            <Button onClick={onCommit} disabled={checking}>
              <HardDriveDownload data-icon="inline-start" />
              创建第一份本地备份
            </Button>
          ) : state === "local_ahead" ? (
            <Button onClick={onUpload} disabled={checking}>
              <CloudUpload data-icon="inline-start" />
              上传新备份
            </Button>
          ) : state === "remote_ahead" || state === "up_to_date" ? (
            <Button onClick={onSync} disabled={checking || isConflict}>
              <CloudDownload data-icon="inline-start" />
              同步后再游玩
            </Button>
          ) : (
            <Button onClick={() => void refresh()} disabled={checking}>
              <RefreshCw data-icon="inline-start" />
              重新检查云端状态
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
      setError(cloudErrorLabel(String(err)))
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
              <DialogTitle>启用云端备份</DialogTitle>
              <DialogDescription>
                MineCommit 会把「{selectedSave?.name}」的备份安全同步到一个仅你可见的 GitHub 仓库。
                Minecraft 不需要安装模组、插件或 Realms。
              </DialogDescription>
            </DialogHeader>
            <div className="grid gap-3 py-2 sm:grid-cols-2">
              <Button className="h-auto justify-start py-4 text-left" onClick={() => setStep("existing-account")}>
                <span>
                  <span className="block">我已有 GitHub 账号</span>
                  <span className="mt-1 block text-xs font-normal opacity-75">
                    引导我创建一个私有云端备份仓库
                  </span>
                </span>
              </Button>
              <Button className="h-auto justify-start py-4 text-left" variant="outline" onClick={() => setStep("create-account")}>
                <span>
                  <span className="block">我还没有 GitHub 账号</span>
                  <span className="mt-1 block text-xs font-normal text-muted-foreground">
                    用免费账号开始，再继续设置
                  </span>
                </span>
              </Button>
            </div>
            <p className="text-sm text-muted-foreground">
              还没有安装 Git？请先 <button type="button" className="text-primary underline" onClick={() => openExternal("https://git-scm.com/downloads")}>下载 Git</button>，安装后重新打开 MineCommit。
            </p>
            <DialogFooter>
              <DialogClose render={<Button variant="outline" />}>稍后再说</DialogClose>
            </DialogFooter>
          </>
        )}

        {step === "create-account" && (
          <>
            <DialogHeader>
              <DialogTitle>创建免费的 GitHub 账号</DialogTitle>
              <DialogDescription>
                GitHub 是保存私有备份的云端服务。创建账号后，回到这里继续；MineCommit 不会要求你输入 Git 命令。
              </DialogDescription>
            </DialogHeader>
            <ol className="list-decimal space-y-2 pl-5 text-sm text-muted-foreground">
              <li>
                打开 <button type="button" className="text-primary underline" onClick={() => openExternal("https://github.com/signup")}>github.com/signup</button> 并完成注册。
              </li>
              <li>验证邮箱并登录 GitHub。</li>
              <li>回到 MineCommit，继续创建你的私有云端备份。</li>
            </ol>
            <DialogFooter>
              <Button variant="outline" onClick={() => setStep("welcome")}>返回</Button>
              <Button onClick={() => setStep("repository")}>我已登录 GitHub</Button>
            </DialogFooter>
          </>
        )}

        {step === "existing-account" && (
          <>
            <DialogHeader>
              <DialogTitle>使用已有 GitHub 账号</DialogTitle>
              <DialogDescription>
                先登录 GitHub，然后创建一个空的私有仓库。下个页面会逐步说明该怎么做。
              </DialogDescription>
            </DialogHeader>
            <p className="text-sm text-muted-foreground">
              如果你还没有登录，可以先打开 <button type="button" className="text-primary underline" onClick={() => openExternal("https://github.com/login")}>github.com/login</button>。
            </p>
            <DialogFooter>
              <Button variant="outline" onClick={() => setStep("welcome")}>返回</Button>
              <Button onClick={() => setStep("repository")}>下一步</Button>
            </DialogFooter>
          </>
        )}

        {step === "repository" && (
          <>
            <DialogHeader>
              <DialogTitle>创建私有云端备份</DialogTitle>
              <DialogDescription>
                这一步只需做一次。这个仓库专门保存「{selectedSave?.name}」的 MineCommit 备份。
              </DialogDescription>
            </DialogHeader>
            <ol className="list-decimal space-y-2 pl-5 text-sm text-muted-foreground">
              <li>
                在 GitHub 打开 <button type="button" className="text-primary underline" onClick={() => openExternal("https://github.com/new?visibility=private")}>创建新仓库</button>，为它取一个名字，例如 <code>minecraft-survival-backup</code>。
              </li>
              <li>将可见性设为 <strong className="text-foreground">Private</strong>。</li>
              <li>
                保持“Add a README file”、“Add .gitignore”和“Choose a license”均未选中；仓库必须是空的，才能安全开始首次同步。
              </li>
              <li>创建后点击 <strong className="text-foreground">Code → HTTPS</strong>，复制以 <code>https://github.com/</code> 开头的地址。</li>
            </ol>
            <FieldGroup className="pt-2">
              <Field>
                <Label htmlFor="cloud-url">粘贴 GitHub 仓库地址</Label>
                <Input
                  id="cloud-url"
                  placeholder="https://github.com/你的用户名/minecraft-survival-backup.git"
                  value={remoteUrl}
                  onChange={(event) => setRemoteUrl(event.target.value)}
                />
              </Field>
              <Field>
                <Label htmlFor="cloud-branch">备份分支</Label>
                <Input id="cloud-branch" value={branch} onChange={(event) => setBranch(event.target.value)} />
              </Field>
            </FieldGroup>
            <p className="text-sm text-muted-foreground">
              MineCommit 不会保存你的 GitHub 密码或令牌。首次上传时，Git Credential Manager 或你的 SSH 密钥会安全地处理登录。
            </p>
            {error && <p className="text-sm text-destructive">{error}</p>}
            <DialogFooter>
              <Button variant="outline" onClick={() => setStep("welcome")} disabled={connecting}>返回</Button>
              <Button onClick={() => void connectCloud()} disabled={connecting || !remoteUrl.trim() || !branch.trim()}>
                {connecting ? "连接中…" : "连接私有云端备份"}
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
      label: "提交 / 备份",
      onClick: () => {
        setCommitDialogKey((k) => k + 1)
        setCommitDialogOpen(true)
      },
    },
    {
      icon: <HardDriveUpload />,
      label: "恢复最近提交",
      onClick: () => setRestoreDialogOpen(true),
      separator: true,
    },
    {
      icon: <CloudUpload />,
      label: "上传 / 推送",
      onClick: () => {
        setPushDialogKey((k) => k + 1)
        setPushDialogOpen(true)
      },
    },
    {
      icon: <CloudDownload />,
      label: "下载 / 拉取",
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
