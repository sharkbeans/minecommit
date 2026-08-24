import { useCallback, useEffect, useMemo, useState } from "react"
import { invoke } from "@tauri-apps/api/core"
import { open as openFolderDialog } from "@tauri-apps/plugin-dialog"
import { openUrl } from "@tauri-apps/plugin-opener"
import { ExternalLink, Loader2 } from "lucide-react"

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
import { Skeleton } from "@/components/ui/skeleton"
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
import { GithubMark, type GitHubAccount } from "@/components/github-account"
import { GrassBlock } from "@/components/block-icon"
import { useCommitAuthor } from "@/contexts/commit-author"
import { localeOptions, useI18n, type Locale } from "@/contexts/i18n"
import { useSaves, type Save } from "@/contexts/saves"
import {
  cloudErrorLabel,
  fileSize,
  relativeTime,
  type FoundWorld,
  type GrantedRepository,
  type OldCopy,
} from "@/lib/cloud"
import { cn } from "@/lib/utils"

function errorText(error: unknown) {
  return error instanceof Error ? error.message : String(error)
}

function openExternal(url: string) {
  void openUrl(url).catch((error) => console.error("Unable to open browser:", error))
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
 * computer that does not have the world yet, a download from GitHub.
 */
export function AddWorldDialog({
  open,
  onOpenChange,
  savesFolder,
  account,
  accountLoaded,
  onNeedSignIn,
  onAdded,
}: {
  open: boolean
  onOpenChange: (open: boolean) => void
  savesFolder: string
  account: GitHubAccount | null
  accountLoaded: boolean
  onNeedSignIn: () => void
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
            account={account}
            accountLoaded={accountLoaded}
            onNeedSignIn={() => {
              onOpenChange(false)
              onNeedSignIn()
            }}
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
    // The saves folder is read from disk when the app starts, and this dialog
    // can be open before that lands. Scanning "" fails, and the failure used to
    // stay on screen in red long after the real folder arrived and the list
    // filled in behind it.
    if (!savesFolder) return
    let ignore = false
    invoke<FoundWorld[]>("list_worlds_in_folder", { folder: savesFolder })
      .then((worlds) => {
        if (ignore) return
        setFound(worlds)
        setError("")
      })
      .catch((err) => {
        if (ignore) return
        setFound([])
        setError(errorText(err))
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

  // Scanning a saves folder reads every world's level.dat, so on a spinning
  // disk it is slow enough to look broken. Rows of the right shape say what is
  // coming; a bare spinner says only that something is happening.
  if (found === null) {
    return (
      <>
        <div className="flex flex-col gap-1 py-2">
          {[0, 1, 2].map((row) => (
            <div key={row} className="flex items-center gap-3 px-2 py-2">
              <Skeleton className="size-4 rounded" />
              <div className="flex flex-1 flex-col gap-1.5">
                <Skeleton className="h-3.5 w-40" />
                <Skeleton className="h-3 w-24" />
              </div>
            </div>
          ))}
        </div>
        <p className="flex items-center gap-2 text-sm text-muted-foreground">
          <Loader2 className="size-4 animate-spin" />
          {t("add.scanning")}
        </p>
      </>
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
                  <GrassBlock className="size-4" />
                  <span className="min-w-0 flex-1">
                    <span className="block truncate text-sm">{world.name}</span>
                    <span className="block truncate text-xs text-muted-foreground">
                      {world.last_played
                        ? t("add.played", { when: relativeTime(world.last_played, locale) })
                        : t("add.neverPlayed")}
                      {/* Two worlds with similar folder names are told apart by
                          the version they were last opened in, which is also
                          the thing that decides whether they still open. */}
                      {world.version ? ` · ${world.version}` : ""}
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
          {adding && <Loader2 data-icon="inline-start" className="animate-spin" />}
          {adding ? t("add.adding") : t("add.add")}
        </Button>
      </DialogFooter>
    </>
  )
}

/**
 * Bringing a world down from GitHub.
 *
 * The player is already signed in and MineCommit already knows which spaces it
 * may read, so asking for a URL would be asking them to fetch something the app
 * is holding. Two dropdowns instead: which space, which world. Pasting an
 * address stays available for the one case the lists cannot cover -- a world
 * someone else backed up and shared a link to.
 */
function AddFromCloud({
  savesFolder,
  account,
  accountLoaded,
  onNeedSignIn,
  onAdded,
}: {
  savesFolder: string
  account: GitHubAccount | null
  accountLoaded: boolean
  onNeedSignIn: () => void
  onAdded: (name: string) => void
}) {
  const { t } = useI18n()
  const { refreshSaves } = useSaves()
  const [byLink, setByLink] = useState(false)
  const [repos, setRepos] = useState<GrantedRepository[] | null>(null)
  const [pickedRepo, setPickedRepo] = useState("")
  const [link, setLink] = useState("")
  const [lookedUp, setLookedUp] = useState("")
  const [worlds, setWorlds] = useState<{ source: string; names: string[] } | null>(null)
  const [pickedWorld, setPickedWorld] = useState("")
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState("")

  useEffect(() => {
    if (!account || byLink) return
    let ignore = false
    invoke<GrantedRepository[]>("github_repositories")
      .then((granted) => {
        if (!ignore) setRepos(granted)
      })
      .catch((err) => {
        if (!ignore) {
          setRepos([])
          setError(cloudErrorLabel(errorText(err), t))
        }
      })
    return () => {
      ignore = true
    }
  }, [account, byLink, t])

  // Everything below follows from the chosen address, so it is derived rather
  // than copied into state: nothing can be left over from an earlier choice.
  const source = byLink ? lookedUp : pickedRepo || repos?.[0]?.clone_url || ""
  const names = worlds?.source === source ? worlds.names : null
  const loadingWorlds = source !== "" && names === null
  const world = pickedWorld && names?.includes(pickedWorld) ? pickedWorld : (names?.[0] ?? "")
  // The name is the one the world was backed up under, and cannot be changed
  // here. A world renamed on the way down is the same world under two names on
  // two computers, and nothing afterwards can tell they are the same.
  const name = world

  useEffect(() => {
    if (!source) return
    let ignore = false
    invoke<string[]>("list_remote_branches", { remoteUrl: source })
      .then((found) => {
        if (!ignore) setWorlds({ source, names: found })
      })
      .catch((err) => {
        if (!ignore) {
          setWorlds({ source, names: [] })
          setError(cloudErrorLabel(errorText(err), t))
        }
      })
    return () => {
      ignore = true
    }
  }, [source, t])

  const download = useCallback(async () => {
    if (!world || !name.trim() || busy) return
    setBusy(true)
    setError("")
    try {
      const result = await invoke<{ success: boolean; error: string | null }>(
        "clone_save_from_cloud",
        {
          name: name.trim(),
          savePath: joinPath(savesFolder, name.trim()),
          remoteUrl: source,
          branch: world,
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
  }, [busy, name, onAdded, refreshSaves, savesFolder, source, t, world])

  if (!byLink && !accountLoaded) {
    return (
      <div className="flex flex-col gap-3 py-2">
        <Skeleton className="h-4 w-56" />
        <Skeleton className="h-9 w-full" />
      </div>
    )
  }

  // Signed in and nothing granted is otherwise an empty dropdown above a dead
  // button: the player is told nothing, and the fix lives on a GitHub page
  // they have no way to reach from here.
  if (!byLink && repos !== null && repos.length === 0) {
    return (
      <>
        <p className="py-2 text-sm text-muted-foreground">{t("add.cloudNoSpaces")}</p>
        {error && <p className="text-sm break-words text-destructive">{error}</p>}
        <DialogFooter className="sm:justify-between">
          <Button variant="ghost" size="sm" onClick={() => setByLink(true)}>
            {t("add.haveLink")}
          </Button>
          <div className="flex gap-2">
            <DialogClose render={<Button variant="outline">{t("common.cancel")}</Button>} />
            <Button
              onClick={() => {
                void invoke<string>("github_install_url")
                  .then(openExternal)
                  .catch((err) => setError(errorText(err)))
              }}
            >
              <ExternalLink data-icon="inline-start" />
              {t("gh.chooseRepos")}
            </Button>
          </div>
        </DialogFooter>
      </>
    )
  }

  if (!byLink && !account) {
    return (
      <>
        <p className="py-2 text-sm text-muted-foreground">{t("add.cloudSignIn")}</p>
        <DialogFooter>
          <Button variant="outline" onClick={() => setByLink(true)}>
            {t("add.haveLink")}
          </Button>
          <Button onClick={onNeedSignIn}>
            <GithubMark data-icon="inline-start" className="size-4" />
            {t("gh.signIn")}
          </Button>
        </DialogFooter>
      </>
    )
  }

  return (
    <>
      <p className="text-sm text-muted-foreground">{t("add.cloudIntro")}</p>

      <FieldGroup>
        {byLink ? (
          <Field>
            <Label htmlFor="cloud-link">{t("add.cloudAddress")}</Label>
            <div className="flex gap-2">
              <Input
                id="cloud-link"
                value={link}
                placeholder="https://github.com/them/our-world.git"
                onChange={(event) => setLink(event.target.value)}
              />
              <Button
                variant="outline"
                onClick={() => setLookedUp(link.trim())}
                disabled={!link.trim() || link.trim() === lookedUp}
              >
                {t("add.cloudLookup")}
              </Button>
            </div>
            <p className="text-xs text-muted-foreground">{t("add.cloudAddressHelp")}</p>
          </Field>
        ) : repos === null ? (
          <Field>
            <Label>{t("add.cloudRepo")}</Label>
            <Skeleton className="h-9 w-full" />
          </Field>
        ) : (
          <Field>
            <Label htmlFor="cloud-repo">{t("add.cloudRepo")}</Label>
            <Select
              value={source}
              onValueChange={(value) => {
                setPickedRepo(value ?? "")
                setPickedWorld("")
              }}
            >
              <SelectTrigger id="cloud-repo" className="w-full">
                <SelectValue placeholder={t("add.cloudRepo")} />
              </SelectTrigger>
              <SelectContent>
                {repos.map((repo) => (
                  <SelectItem key={repo.clone_url} value={repo.clone_url}>
                    {repo.full_name}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </Field>
        )}

        {loadingWorlds && (
          <p className="flex items-center gap-2 text-sm text-muted-foreground">
            <Loader2 className="size-4 animate-spin" />
            {t("add.cloudLoadingWorlds")}
          </p>
        )}

        {names !== null && names.length === 0 && (
          <p className="text-sm text-muted-foreground">{t("add.cloudNoWorlds")}</p>
        )}

        {names !== null && names.length > 0 && (
          <>
            <Field>
              <Label htmlFor="cloud-world">{t("add.cloudWorld")}</Label>
              <Select
                value={world}
                onValueChange={(value) => setPickedWorld(value ?? "")}
              >
                <SelectTrigger id="cloud-world" className="w-full">
                  <SelectValue placeholder={t("add.cloudWorld")} />
                </SelectTrigger>
                <SelectContent>
                  {names.map((option) => (
                    <SelectItem key={option} value={option}>
                      {option}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
              <p className="text-xs text-muted-foreground">
                {t("add.cloudNameFixed", { name })}
              </p>
            </Field>
          </>
        )}
      </FieldGroup>

      {busy && (
        <p className="text-xs text-muted-foreground">{t("add.downloadingHelp")}</p>
      )}
      {error && <p className="text-sm break-words text-destructive">{error}</p>}

      <DialogFooter className="sm:justify-between">
        {account && (
          <Button variant="ghost" size="sm" onClick={() => setByLink((current) => !current)}>
            {byLink ? t("add.usePicker") : t("add.haveLink")}
          </Button>
        )}
        <div className="flex gap-2">
          <DialogClose render={<Button variant="outline">{t("common.cancel")}</Button>} />
          <Button onClick={() => void download()} disabled={busy || !world || !name.trim()}>
            {busy && <Loader2 data-icon="inline-start" className="animate-spin" />}
            {busy ? t("add.downloading") : t("add.download")}
          </Button>
        </div>
      </DialogFooter>
    </>
  )
}

/* ── The copies restores left behind ────────────────────────────────────── */

/**
 * Clearing out the worlds MineCommit left in the saves folder.
 *
 * A restore keeps the world it replaced, and until now it kept it right beside
 * the original. Minecraft lists any folder holding a level.dat, so those copies
 * turned up in the game as worlds of their own, with names differing only by a
 * Unix timestamp -- which makes choosing what to play a guessing game.
 *
 * Moving is the default and deleting is the second option, because a copy is
 * the world as it was before a restore and can hold an afternoon that was never
 * backed up anywhere else.
 */
export function OldCopiesDialog({
  open,
  onOpenChange,
  savesFolder,
  onCleared,
}: {
  open: boolean
  onOpenChange: (open: boolean) => void
  savesFolder: string
  onCleared: () => void
}) {
  const { locale, t } = useI18n()
  const [copies, setCopies] = useState<OldCopy[] | null>(null)
  const [busy, setBusy] = useState<"move" | "delete" | null>(null)
  const [confirming, setConfirming] = useState(false)
  const [done, setDone] = useState("")
  const [error, setError] = useState("")

  useEffect(() => {
    if (!open || !savesFolder) return
    let ignore = false
    invoke<OldCopy[]>("list_old_copies", { folder: savesFolder })
      .then((found) => {
        if (!ignore) setCopies(found)
      })
      .catch((err) => {
        if (ignore) return
        setCopies([])
        setError(errorText(err))
      })
    return () => {
      ignore = true
    }
  }, [open, savesFolder])

  const paths = useMemo(() => (copies ?? []).map((copy) => copy.path), [copies])
  const total = useMemo(
    () => (copies ?? []).reduce((sum, copy) => sum + copy.bytes, 0),
    [copies]
  )

  const run = useCallback(
    async (what: "move" | "delete") => {
      if (busy || paths.length === 0) return
      setBusy(what)
      setError("")
      try {
        if (what === "move") {
          const where = await invoke<string>("tidy_old_copies", {
            folder: savesFolder,
            paths,
          })
          setDone(t("oldCopies.moved", { where }))
        } else {
          await invoke<number>("delete_old_copies", { folder: savesFolder, paths })
          setDone(t("oldCopies.none"))
        }
        setCopies([])
        onCleared()
      } catch (err) {
        setError(errorText(err))
      } finally {
        setBusy(null)
        setConfirming(false)
      }
    },
    [busy, onCleared, paths, savesFolder, t]
  )

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-lg">
        <DialogHeader>
          <DialogTitle>{t("oldCopies.title")}</DialogTitle>
          <DialogDescription>{t("oldCopies.body")}</DialogDescription>
        </DialogHeader>

        {copies === null ? (
          <p className="flex items-center gap-2 py-6 text-sm text-muted-foreground">
            <Loader2 className="size-4 animate-spin" />
            {t("oldCopies.scanning")}
          </p>
        ) : copies.length === 0 ? (
          <p className="py-4 text-sm text-muted-foreground">
            {done || t("oldCopies.none")}
          </p>
        ) : (
          <>
            <ul className="max-h-64 divide-y overflow-y-auto">
              {copies.map((copy) => (
                <li key={copy.path} className="flex items-center gap-3 py-2">
                  <span className="min-w-0 flex-1">
                    <span className="block truncate text-sm">
                      {t("oldCopies.from", { world: copy.world })}
                    </span>
                    <span className="block truncate text-xs text-muted-foreground">
                      {copy.taken
                        ? t("oldCopies.taken", { when: relativeTime(copy.taken, locale) })
                        : ""}
                    </span>
                  </span>
                  <span className="shrink-0 font-mono text-xs text-muted-foreground">
                    {fileSize(copy.bytes)}
                  </span>
                </li>
              ))}
            </ul>
            <p className="text-xs text-muted-foreground">
              {t("oldCopies.total", { size: fileSize(total) })}
            </p>
          </>
        )}

        {confirming && (
          <p className="text-sm text-destructive">{t("oldCopies.deleteConfirm")}</p>
        )}
        {error && <p className="text-sm break-words text-destructive">{error}</p>}

        <DialogFooter className="sm:justify-between">
          {copies !== null && copies.length > 0 && (
            <Button
              variant="ghost"
              size="sm"
              className="text-destructive hover:text-destructive"
              disabled={busy !== null}
              onClick={() => (confirming ? void run("delete") : setConfirming(true))}
            >
              {busy === "delete" && <Loader2 data-icon="inline-start" className="animate-spin" />}
              {busy === "delete"
                ? t("oldCopies.deleting")
                : confirming
                  ? t("oldCopies.deleteYes")
                  : t("oldCopies.deleteAction")}
            </Button>
          )}
          <div className="flex gap-2">
            <DialogClose render={<Button variant="outline">{t("common.close")}</Button>} />
            {copies !== null && copies.length > 0 && (
              <Button disabled={busy !== null} onClick={() => void run("move")}>
                {busy === "move" && <Loader2 data-icon="inline-start" className="animate-spin" />}
                {busy === "move" ? t("oldCopies.moving") : t("oldCopies.moveAction")}
              </Button>
            )}
          </div>
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
            {busy && <Loader2 data-icon="inline-start" className="animate-spin" />}
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
            {busy && <Loader2 data-icon="inline-start" className="animate-spin" />}
            {busy ? t("removeWorld.removing") : t("removeWorld.action")}
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
              {savesFolder ? (
                <Input
                  id="settings-folder"
                  value={savesFolder}
                  readOnly
                  className="font-mono text-xs"
                />
              ) : (
                <Skeleton className="h-9 flex-1" />
              )}
              <Button variant="outline" onClick={() => void pickFolder()}>
                {t("dash.change")}
              </Button>
            </div>
            <p className="text-xs text-muted-foreground">{t("dash.savesFolderHelp")}</p>
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
            <p className="text-xs text-muted-foreground">{t("settings.identityHelp")}</p>
          </Field>
        </FieldGroup>
        <DialogFooter>
          <Button onClick={() => void close()}>{t("common.close")}</Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
