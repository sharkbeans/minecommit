import { useCallback, useEffect, useMemo, useState } from "react"
import { invoke } from "@tauri-apps/api/core"
import { open as openFolderDialog } from "@tauri-apps/plugin-dialog"
import { Loader2 } from "lucide-react"

import { Button } from "@/components/ui/button"
import { Checkbox } from "@/components/ui/checkbox"
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
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
import { useCommitAuthor } from "@/contexts/commit-author"
import { localeOptions, useI18n, type Locale } from "@/contexts/i18n"
import { useSaves, type Save } from "@/contexts/saves"
import { cloudErrorLabel, relativeTime, type FoundWorld } from "@/lib/cloud"
import { cn } from "@/lib/utils"

function errorText(error: unknown) {
  return error instanceof Error ? error.message : String(error)
}

/** Join a folder and a name using whichever separator the folder already uses. */
function joinPath(folder: string, name: string) {
  const separator = folder.includes("\\") && !folder.includes("/") ? "\\" : "/"
  return `${folder.replace(/[\\/]+$/, "")}${separator}${name}`
}

/* ── Add a world ─────────────────────────────────────────────────────────── */

type AddTab = "local" | "cloud"

/**
 * Adding a world never asks for a path. The saves folder is already known, so
 * the choice is a list of the worlds actually sitting in it -- or, for a
 * computer that does not have the world yet, a download from the cloud.
 */
export function AddWorldDialog({
  open,
  onOpenChange,
  savesFolder,
  onAdded,
}: {
  open: boolean
  onOpenChange: (open: boolean) => void
  savesFolder: string
  onAdded: (name: string) => void
}) {
  const { t } = useI18n()
  const [tab, setTab] = useState<AddTab>("local")

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-lg">
        <DialogHeader>
          <DialogTitle>{t("add.title")}</DialogTitle>
        </DialogHeader>

        <div className="flex gap-1 rounded-lg bg-muted p-1">
          {(["local", "cloud"] as const).map((value) => (
            <button
              key={value}
              type="button"
              onClick={() => setTab(value)}
              className={cn(
                "flex-1 rounded-md px-3 py-1.5 text-sm transition-colors",
                tab === value
                  ? "bg-background shadow-sm"
                  : "text-muted-foreground hover:text-foreground"
              )}
            >
              {value === "local" ? t("add.fromThisPc") : t("add.fromCloud")}
            </button>
          ))}
        </div>

        {tab === "local" ? (
          <AddFromThisPc
            savesFolder={savesFolder}
            onAdded={(name) => {
              onOpenChange(false)
              onAdded(name)
            }}
          />
        ) : (
          <AddFromCloud
            savesFolder={savesFolder}
            onAdded={(name) => {
              onOpenChange(false)
              onAdded(name)
            }}
          />
        )}
      </DialogContent>
    </Dialog>
  )
}

function AddFromThisPc({
  savesFolder,
  onAdded,
}: {
  savesFolder: string
  onAdded: (name: string) => void
}) {
  const { locale, t } = useI18n()
  const { saves, refreshSaves } = useSaves()
  const [found, setFound] = useState<FoundWorld[] | null>(null)
  const [chosen, setChosen] = useState<Set<string>>(new Set())
  const [adding, setAdding] = useState(false)
  const [error, setError] = useState("")

  useEffect(() => {
    let ignore = false
    invoke<FoundWorld[]>("list_worlds_in_folder", { folder: savesFolder })
      .then((worlds) => {
        if (!ignore) setFound(worlds)
      })
      .catch((err) => {
        if (!ignore) {
          setFound([])
          setError(errorText(err))
        }
      })
    return () => {
      ignore = true
    }
  }, [savesFolder])

  const tracked = useMemo(() => new Set(saves.map((save) => save.path)), [saves])
  const available = useMemo(
    () => (found ?? []).filter((world) => !tracked.has(world.path)),
    [found, tracked]
  )

  const toggle = (path: string) =>
    setChosen((current) => {
      const next = new Set(current)
      if (next.has(path)) next.delete(path)
      else next.add(path)
      return next
    })

  const handleAdd = useCallback(async () => {
    if (chosen.size === 0 || adding) return
    setAdding(true)
    setError("")
    let lastAdded = ""
    try {
      for (const world of available) {
        if (!chosen.has(world.path)) continue
        const derived = await invoke<{ name: string; repo_path: string }>(
          "derive_save_info",
          { path: world.path }
        )
        await invoke<Save>("add_save", {
          name: derived.name,
          path: world.path,
          repoPath: derived.repo_path,
          remoteRepoPath: "",
          defaultBranch: "main",
        })
        lastAdded = derived.name
      }
      await refreshSaves()
      onAdded(lastAdded)
    } catch (err) {
      setError(errorText(err))
    } finally {
      setAdding(false)
    }
  }, [adding, available, chosen, onAdded, refreshSaves])

  if (found === null) {
    return (
      <p className="flex items-center gap-2 py-6 text-sm text-muted-foreground">
        <Loader2 className="size-4 animate-spin" />
        {t("add.scanning")}
      </p>
    )
  }

  return (
    <>
      <div className="max-h-72 overflow-y-auto">
        {available.length === 0 ? (
          <p className="py-6 text-sm text-muted-foreground">
            {found.length === 0 ? t("add.noneFound") : t("add.allAdded")}
          </p>
        ) : (
          <ul className="flex flex-col">
            {available.map((world) => (
              <li key={world.path}>
                <label className="flex cursor-pointer items-center gap-3 rounded-md px-2 py-2 hover:bg-muted">
                  <Checkbox
                    checked={chosen.has(world.path)}
                    onCheckedChange={() => toggle(world.path)}
                  />
                  <span className="min-w-0 flex-1">
                    <span className="block truncate text-sm">{world.name}</span>
                    <span className="block truncate text-xs text-muted-foreground">
                      {world.last_played
                        ? t("add.played", { when: relativeTime(world.last_played, locale) })
                        : t("add.neverPlayed")}
                    </span>
                  </span>
                </label>
              </li>
            ))}
          </ul>
        )}
      </div>

      {error && <p className="text-sm text-destructive">{error}</p>}

      <DialogFooter>
        <DialogClose render={<Button variant="outline">{t("common.cancel")}</Button>} />
        <Button onClick={() => void handleAdd()} disabled={adding || chosen.size === 0}>
          {adding ? t("add.adding") : t("add.add")}
        </Button>
      </DialogFooter>
    </>
  )
}

function AddFromCloud({
  savesFolder,
  onAdded,
}: {
  savesFolder: string
  onAdded: (name: string) => void
}) {
  const { t } = useI18n()
  const { refreshSaves } = useSaves()
  const [address, setAddress] = useState("")
  const [branches, setBranches] = useState<string[] | null>(null)
  const [branch, setBranch] = useState("")
  const [name, setName] = useState("")
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState("")

  const lookUp = useCallback(async () => {
    if (!address.trim() || busy) return
    setBusy(true)
    setError("")
    setBranches(null)
    try {
      const found = await invoke<string[]>("list_remote_branches", {
        remoteUrl: address.trim(),
      })
      setBranches(found)
      const first = found[0] ?? ""
      setBranch(first)
      if (!name.trim()) setName(first)
    } catch (err) {
      setError(cloudErrorLabel(errorText(err), t))
    } finally {
      setBusy(false)
    }
  }, [address, busy, name, t])

  const download = useCallback(async () => {
    if (!branch || !name.trim() || busy) return
    setBusy(true)
    setError("")
    try {
      const result = await invoke<{ success: boolean; error: string | null }>(
        "clone_save_from_cloud",
        {
          name: name.trim(),
          savePath: joinPath(savesFolder, name.trim()),
          remoteUrl: address.trim(),
          branch,
        }
      )
      if (!result.success) {
        setError(cloudErrorLabel(result.error ?? "", t))
        return
      }
      await refreshSaves()
      onAdded(name.trim())
    } catch (err) {
      setError(cloudErrorLabel(errorText(err), t))
    } finally {
      setBusy(false)
    }
  }, [address, branch, busy, name, onAdded, refreshSaves, savesFolder, t])

  return (
    <>
      <FieldGroup>
        <Field>
          <Label htmlFor="cloud-address">{t("add.cloudAddress")}</Label>
          <div className="flex gap-2">
            <Input
              id="cloud-address"
              value={address}
              placeholder="https://github.com/you/my-world.git"
              onChange={(e) => {
                setAddress(e.target.value)
                setBranches(null)
              }}
            />
            <Button
              variant="outline"
              onClick={() => void lookUp()}
              disabled={busy || !address.trim()}
            >
              {busy && branches === null ? t("add.cloudLookingUp") : t("add.cloudLookup")}
            </Button>
          </div>
          <p className="text-xs text-muted-foreground">{t("add.cloudAddressHelp")}</p>
        </Field>

        {branches !== null && (
          <>
            <Field>
              <Label htmlFor="cloud-branch">{t("add.cloudBranch")}</Label>
              <Select value={branch} onValueChange={(value) => setBranch(value ?? "")}>
                <SelectTrigger id="cloud-branch" className="w-full">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  {branches.map((option) => (
                    <SelectItem key={option} value={option}>
                      {option}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </Field>
            <Field>
              <Label htmlFor="cloud-name">{t("add.cloudName")}</Label>
              <Input
                id="cloud-name"
                value={name}
                onChange={(e) => setName(e.target.value)}
              />
              <p className="text-xs text-muted-foreground">{t("add.downloadHelp")}</p>
            </Field>
          </>
        )}
      </FieldGroup>

      {error && <p className="text-sm text-destructive">{error}</p>}

      <DialogFooter>
        <DialogClose render={<Button variant="outline">{t("common.cancel")}</Button>} />
        <Button
          onClick={() => void download()}
          disabled={busy || branches === null || !branch || !name.trim()}
        >
          {busy && branches !== null ? t("add.downloading") : t("add.download")}
        </Button>
      </DialogFooter>
    </>
  )
}

/* ── Connect a world to GitHub ───────────────────────────────────────────── */

export function ConnectCloudDialog({
  open,
  onOpenChange,
  save,
  onConnected,
}: {
  open: boolean
  onOpenChange: (open: boolean) => void
  save: Save | null
  onConnected: () => Promise<void> | void
}) {
  const { t } = useI18n()
  const [address, setAddress] = useState(() => save?.remote_repo_path ?? "")
  const [branch, setBranch] = useState(
    () => save?.default_branch || save?.name || "main"
  )
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState("")

  const connect = useCallback(async () => {
    if (!save || busy) return
    setBusy(true)
    setError("")
    try {
      await invoke<Save>("configure_save_cloud", {
        name: save.name,
        remoteUrl: address.trim(),
        branch: branch.trim(),
      })
      await onConnected()
      onOpenChange(false)
    } catch (err) {
      setError(cloudErrorLabel(errorText(err), t))
    } finally {
      setBusy(false)
    }
  }, [address, branch, busy, onConnected, onOpenChange, save, t])

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-lg">
        <DialogHeader>
          <DialogTitle>{t("connect.title")}</DialogTitle>
          <DialogDescription>{t("connect.body")}</DialogDescription>
        </DialogHeader>
        <FieldGroup>
          <Field>
            <Label htmlFor="connect-address">{t("connect.address")}</Label>
            <Input
              id="connect-address"
              value={address}
              placeholder="https://github.com/you/my-worlds.git"
              onChange={(e) => setAddress(e.target.value)}
            />
          </Field>
          <Field>
            <Label htmlFor="connect-branch">{t("connect.branch")}</Label>
            <Input
              id="connect-branch"
              value={branch}
              onChange={(e) => setBranch(e.target.value)}
            />
            <p className="text-xs text-muted-foreground">{t("connect.branchHelp")}</p>
          </Field>
        </FieldGroup>
        <p className="text-xs text-muted-foreground">{t("connect.credentialHelp")}</p>
        {error && <p className="text-sm text-destructive">{error}</p>}
        <DialogFooter>
          <DialogClose render={<Button variant="outline">{t("common.cancel")}</Button>} />
          <Button
            onClick={() => void connect()}
            disabled={busy || !address.trim() || !branch.trim()}
          >
            {busy ? t("connect.connecting") : t("connect.connect")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}

/* ── Go back to a point in history ───────────────────────────────────────── */

export function RestorePointDialog({
  open,
  onOpenChange,
  when,
  busy,
  onConfirm,
}: {
  open: boolean
  onOpenChange: (open: boolean) => void
  when: string
  busy: boolean
  onConfirm: () => void
}) {
  const { t } = useI18n()
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-md">
        <DialogHeader>
          <DialogTitle>{t("restoreTo.title")}</DialogTitle>
          <DialogDescription>{t("restoreTo.body", { when })}</DialogDescription>
        </DialogHeader>
        <DialogFooter>
          <DialogClose render={<Button variant="outline">{t("common.cancel")}</Button>} />
          <Button onClick={onConfirm} disabled={busy}>
            {busy ? t("restoreTo.working") : t("restoreTo.action")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}

/* ── Remove a world ──────────────────────────────────────────────────────── */

export function RemoveWorldDialog({
  open,
  onOpenChange,
  save,
  onRemoved,
}: {
  open: boolean
  onOpenChange: (open: boolean) => void
  save: Save | null
  onRemoved: () => Promise<void> | void
}) {
  const { t } = useI18n()
  const [alsoBackups, setAlsoBackups] = useState(false)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState("")

  const remove = useCallback(async () => {
    if (!save || busy) return
    setBusy(true)
    setError("")
    try {
      await invoke("delete_save", { name: save.name, deleteRepo: alsoBackups })
      await onRemoved()
      onOpenChange(false)
    } catch (err) {
      setError(errorText(err))
    } finally {
      setBusy(false)
    }
  }, [alsoBackups, busy, onOpenChange, onRemoved, save])

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-md">
        <DialogHeader>
          <DialogTitle>{t("removeWorld.title", { world: save?.name ?? "" })}</DialogTitle>
          <DialogDescription>{t("removeWorld.body")}</DialogDescription>
        </DialogHeader>
        <label className="flex cursor-pointer items-center gap-3 text-sm">
          <Checkbox
            checked={alsoBackups}
            onCheckedChange={(checked) => setAlsoBackups(checked === true)}
          />
          {t("removeWorld.alsoBackups")}
        </label>
        {error && <p className="text-sm text-destructive">{error}</p>}
        <DialogFooter>
          <DialogClose render={<Button variant="outline">{t("common.cancel")}</Button>} />
          <Button variant="destructive" onClick={() => void remove()} disabled={busy}>
            {t("removeWorld.action")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}

/* ── Settings ────────────────────────────────────────────────────────────── */

export function SettingsDialog({
  open,
  onOpenChange,
  savesFolder,
  onSavesFolderChange,
}: {
  open: boolean
  onOpenChange: (open: boolean) => void
  savesFolder: string
  onSavesFolderChange: (folder: string) => void
}) {
  const { locale, setLocale, t } = useI18n()
  const { author, setAuthor } = useCommitAuthor()
  const [name, setName] = useState(author.name)
  const [email, setEmail] = useState(author.email)

  const pickFolder = useCallback(async () => {
    const picked = await openFolderDialog({ directory: true, defaultPath: savesFolder })
    if (typeof picked === "string") onSavesFolderChange(picked)
  }, [onSavesFolderChange, savesFolder])

  const close = useCallback(async () => {
    // Identity is only ever used to label backups, so saving it on close keeps
    // the dialog free of a button whose effect the player cannot see.
    if (name !== author.name || email !== author.email) {
      try {
        await setAuthor(name, email)
      } catch {
        // Keeping the old identity is harmless; backups still record a device.
      }
    }
    onOpenChange(false)
  }, [author.email, author.name, email, name, onOpenChange, setAuthor])

  return (
    <Dialog open={open} onOpenChange={(next) => (next ? onOpenChange(true) : void close())}>
      <DialogContent className="sm:max-w-lg">
        <DialogHeader>
          <DialogTitle>{t("dash.settings")}</DialogTitle>
        </DialogHeader>
        <FieldGroup>
          <Field>
            <Label htmlFor="settings-folder">{t("dash.savesFolder")}</Label>
            <div className="flex gap-2">
              <Input id="settings-folder" value={savesFolder} readOnly className="font-mono text-xs" />
              <Button variant="outline" onClick={() => void pickFolder()}>
                {t("dash.change")}
              </Button>
            </div>
          </Field>
          <Field>
            <Label htmlFor="settings-language">{t("settings.language")}</Label>
            <Select
              value={locale}
              onValueChange={(value) => {
                if (value === "en" || value === "zh-CN") setLocale(value as Locale)
              }}
            >
              <SelectTrigger id="settings-language" className="w-full">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {localeOptions.map((option) => (
                  <SelectItem key={option.value} value={option.value}>
                    {option.label}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </Field>
          <Field>
            <Label htmlFor="settings-name">{t("settings.name")}</Label>
            <Input
              id="settings-name"
              value={name}
              placeholder={t("settings.namePlaceholder")}
              onChange={(e) => setName(e.target.value)}
            />
          </Field>
          <Field>
            <Label htmlFor="settings-email">{t("settings.email")}</Label>
            <Input
              id="settings-email"
              value={email}
              placeholder={t("settings.emailPlaceholder")}
              onChange={(e) => setEmail(e.target.value)}
            />
          </Field>
        </FieldGroup>
        <DialogFooter>
          <Button onClick={() => void close()}>{t("common.close")}</Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
