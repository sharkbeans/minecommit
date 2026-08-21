import { Card, CardHeader, CardTitle, CardContent } from "@/components/ui/card"
import {
  Table,
  TableHeader,
  TableBody,
  TableHead,
  TableRow,
  TableCell,
} from "@/components/ui/table"
import {
  Empty,
  EmptyContent,
  EmptyDescription,
  EmptyHeader,
  EmptyMedia,
  EmptyTitle,
} from "@/components/ui/empty"
import { Button } from "@/components/ui/button"
import {
  Dialog,
  DialogClose,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog"
import { Field, FieldGroup, FieldLabel } from "@/components/ui/field"
import { Input } from "@/components/ui/input"
import { Checkbox } from "@/components/ui/checkbox"
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
import { useState } from "react"
import { Trash2, HardDrive, FolderOpen } from "lucide-react"
import { invoke } from "@tauri-apps/api/core"
import { open as openFolderDialog } from "@tauri-apps/plugin-dialog"
import { useSaves } from "@/contexts/saves"
import { Label } from "@/components/ui/label"
import { SaveHoverCard } from "@/components/save-hover-card"
import { useI18n } from "@/contexts/i18n"

function EmptySave({ onAddTrack }: { onAddTrack: () => void }) {
  const { t } = useI18n()

  return (
    <Empty>
      <EmptyHeader>
        <EmptyMedia variant="icon">
          <HardDrive />
        </EmptyMedia>
        <EmptyTitle>{t("worlds.emptyTitle")}</EmptyTitle>
        <EmptyDescription>
          <p>{t("worlds.emptyLine1")}</p>
          <p>{t("worlds.emptyLine2")}</p>
        </EmptyDescription>
      </EmptyHeader>
      <EmptyContent>
        <Button onClick={onAddTrack}>{t("worlds.add")}</Button>
      </EmptyContent>
    </Empty>
  )
}

type AddTrackStep = "select" | "confirm" | "select-branch" | "init"

function AddTrackDialog({
  open,
  onOpenChange,
  onSaveAdded,
}: {
  open: boolean
  onOpenChange: (open: boolean) => void
  onSaveAdded: () => void
}) {
  const { t } = useI18n()
  const [step, setStep] = useState<AddTrackStep>("select")

  // Form state (pre-filled after folder selection)
  const [name, setName] = useState("")
  const [path, setPath] = useState("")
  const [localRepoPath, setLocalRepoPath] = useState("")
  const [error, setError] = useState("")
  const [submitting, setSubmitting] = useState(false)
  const [initializing, setInitializing] = useState(false)
  const [selecting, setSelecting] = useState(false)
  const [branches, setBranches] = useState<string[]>([])
  const [defaultBranch, setDefaultBranch] = useState("")
  const [initBranch, setInitBranch] = useState("main")

  function resetAll() {
    setStep("select")
    setName("")
    setPath("")
    setLocalRepoPath("")
    setError("")
    setSubmitting(false)
    setSelecting(false)
    setBranches([])
    setDefaultBranch("")
    setInitBranch("main")
  }

  function handleOpenChange(open: boolean) {
    if (!open) {
      resetAll()
    }
    onOpenChange(open)
  }

  // --- Step: select ---
  async function handleSelectFolder() {
    setSelecting(true)
    try {
      const selected = await openFolderDialog({
        directory: true,
        multiple: false,
        title: t("worlds.selectFolder"),
      })
      if (!selected) return

      if (typeof selected !== "string") {
        setError(t("worlds.singleFolderRequired"))
        return
      }

      // Derive fields via backend.
      const info = await invoke<{ name: string; repo_path: string }>(
        "derive_save_info",
        { path: selected }
      )
      setName(info.name)
      setPath(selected)
      setLocalRepoPath(info.repo_path)
      setError("")
      setStep("confirm")
    } catch (err) {
      setError(String(err))
    } finally {
      setSelecting(false)
    }
  }

  // --- Step: confirm ---
  async function handleSubmit(e: { preventDefault: () => void }) {
    e.preventDefault()
    setError("")
    setSubmitting(true)
    try {
      const exists = await invoke<boolean>("check_repo_exists", {
        repoPath: localRepoPath,
      })
      if (!exists) {
        setSubmitting(false)
        setStep("init")
        return
      }
      // Repo exists — fetch branches and let user select one
      setSubmitting(false)
      const branchList = await invoke<string[]>("list_branches", {
        repoPath: localRepoPath,
      })
      setBranches(branchList)
      if (branchList.length === 0) {
        // No commits yet — read HEAD ref to determine the default branch
        const headRef = await invoke<string>("get_head_ref", {
          repoPath: localRepoPath,
        })
        await invoke("add_save", {
          name,
          path,
          repoPath: localRepoPath,
          remoteRepoPath: "",
          defaultBranch: headRef,
        })
        onOpenChange(false)
        resetAll()
        onSaveAdded()
        return
      }
      // Pick first branch as default if available
      setDefaultBranch(branchList[0])
      setStep("select-branch")
    } catch (err) {
      setError(String(err))
      setSubmitting(false)
    }
  }

  async function handleInitComplete(branchName: string) {
    setInitializing(true)
    try {
      await invoke("init_bare_repo", {
        repoPath: localRepoPath,
        defaultBranch: branchName,
      })
      await invoke("add_save", {
        name,
        path,
        repoPath: localRepoPath,
        remoteRepoPath: "",
        defaultBranch: branchName,
      })
      onOpenChange(false)
      resetAll()
      onSaveAdded()
    } catch (err) {
      setError(String(err))
      setStep("confirm")
    } finally {
      setInitializing(false)
    }
  }

  async function handleBranchConfirm() {
    setSubmitting(true)
    try {
      await invoke("add_save", {
        name,
        path,
        repoPath: localRepoPath,
        remoteRepoPath: "",
        defaultBranch,
      })
      onOpenChange(false)
      resetAll()
      onSaveAdded()
    } catch (err) {
      setError(String(err))
    } finally {
      setSubmitting(false)
    }
  }

  function handleInitCancel() {
    setStep("confirm")
  }

  function handleBack() {
    resetAll()
  }

  return (
    <Dialog open={open} onOpenChange={handleOpenChange}>
      <DialogContent>
        {step === "select" && (
          <>
            <DialogHeader>
              <DialogTitle>{t("worlds.selectTitle")}</DialogTitle>
              <DialogDescription>
                {t("worlds.selectDescription")}
              </DialogDescription>
            </DialogHeader>
            <div className="flex flex-col items-center gap-4 py-4">
              <Button
                size="lg"
                disabled={selecting}
                onClick={handleSelectFolder}
                className="w-full max-w-xs"
              >
                <FolderOpen data-icon="inline-start" />
                {selecting ? t("worlds.selecting") : t("worlds.selectFolder")}
              </Button>
              {error && (
                <p className="w-full max-w-xs text-sm text-destructive" role="alert">
                  {error}
                </p>
              )}
            </div>
            <DialogFooter>
              <DialogClose render={<Button variant="outline" />}>
                {t("common.cancel")}
              </DialogClose>
            </DialogFooter>
          </>
        )}

        {step === "confirm" && (
          <form
            onSubmit={(e) => e.preventDefault()}
            className="flex flex-col gap-4"
          >
            <DialogHeader>
              <DialogTitle>{t("worlds.confirmTitle")}</DialogTitle>
              <DialogDescription>
                {t("worlds.confirmDescription")}
              </DialogDescription>
            </DialogHeader>
            <FieldGroup>
              <Field>
                <FieldLabel htmlFor="save-name">{t("worlds.name")}</FieldLabel>
                <Input
                  id="save-name"
                  placeholder={t("worlds.namePlaceholder")}
                  value={name}
                  onChange={(e) => setName(e.target.value)}
                  required
                />
              </Field>
              <Field>
                <FieldLabel htmlFor="save-path">{t("worlds.path")}</FieldLabel>
                <Input
                  id="save-path"
                  placeholder={t("worlds.pathPlaceholder")}
                  value={path}
                  onChange={(e) => setPath(e.target.value)}
                  required
                />
              </Field>
              <p className="text-sm text-muted-foreground">
                {t("worlds.localBackupHelp")}
              </p>
              {error && <p className="text-sm text-destructive">{error}</p>}
            </FieldGroup>
            <DialogFooter className="mt-6">
              <Button variant="outline" type="button" onClick={handleBack}>
                {t("common.back")}
              </Button>
              <Button
                type="button"
                disabled={submitting}
                onClick={handleSubmit}
              >
                {submitting ? t("worlds.tracking") : t("worlds.track")}
              </Button>
            </DialogFooter>
          </form>
        )}

        {step === "select-branch" && (
          <>
            <DialogHeader>
              <DialogTitle>{t("worlds.branchTitle")}</DialogTitle>
              <DialogDescription>
                {t("worlds.branchDescription")}
              </DialogDescription>
            </DialogHeader>
            <FieldGroup>
              <Field>
                <FieldLabel htmlFor="select-branch">{t("worlds.defaultBranch")}</FieldLabel>
                <Select
                  value={defaultBranch}
                  onValueChange={(v) => setDefaultBranch(v ?? "")}
                >
                  <SelectTrigger className="w-full">
                    <SelectValue placeholder={t("worlds.selectBranch")} />
                  </SelectTrigger>
                  <SelectContent>
                    {branches.map((branch) => (
                      <SelectItem key={branch} value={branch}>
                        {branch}
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
              </Field>
            </FieldGroup>
            {error && <p className="text-sm text-destructive">{error}</p>}
            <DialogFooter>
              <Button variant="outline" onClick={() => setStep("confirm")}>
                {t("common.back")}
              </Button>
              <Button
                disabled={submitting || !defaultBranch}
                onClick={handleBranchConfirm}
              >
                {submitting ? t("worlds.tracking") : t("worlds.confirmTrack")}
              </Button>
            </DialogFooter>
          </>
        )}

        {step === "init" && (
          <>
            <DialogHeader>
              <DialogTitle>{t("worlds.initTitle")}</DialogTitle>
              <DialogDescription>
                {t("worlds.initDescription")}
              </DialogDescription>
            </DialogHeader>
            <FieldGroup>
              <Field>
                <FieldLabel htmlFor="init-branch">{t("worlds.branchName")}</FieldLabel>
                <Input
                  id="init-branch"
                  placeholder="main"
                  value={initBranch}
                  onChange={(e) => setInitBranch(e.target.value)}
                />
              </Field>
            </FieldGroup>
            {error && <p className="text-sm text-destructive">{error}</p>}
            <DialogFooter>
              <Button variant="outline" onClick={handleInitCancel}>
                {t("common.back")}
              </Button>
              <Button
                disabled={initializing}
                onClick={() => {
                  if (!initBranch.trim()) {
                    setError(t("worlds.branchRequired"))
                    return
                  }
                  handleInitComplete(initBranch.trim())
                }}
              >
                {initializing ? t("worlds.initializing") : t("worlds.initialize")}
              </Button>
            </DialogFooter>
          </>
        )}
      </DialogContent>
    </Dialog>
  )
}

export function SaveManagePage() {
  const { saves, loaded, refreshSaves } = useSaves()
  const { t } = useI18n()
  const [dialogOpen, setDialogOpen] = useState(false)
  const [deleteDialogOpen, setDeleteDialogOpen] = useState(false)
  const [deleteTarget, setDeleteTarget] = useState<string | null>(null)
  const [deleteRepoChecked, setDeleteRepoChecked] = useState(false)
  const [error, setError] = useState("")

  async function handleConfirmDelete() {
    if (!deleteTarget) return
    try {
      await invoke("delete_save", {
        name: deleteTarget,
        deleteRepo: deleteRepoChecked,
      })
      await refreshSaves()
      setDeleteDialogOpen(false)
      setDeleteTarget(null)
      setDeleteRepoChecked(false)
    } catch (err) {
      setError(String(err))
    }
  }

  return (
    <div className="flex w-full flex-col gap-4 p-4">
      <Card className="h-full">
        <CardHeader>
          <div className="flex items-end justify-between">
            <div>
              <CardTitle>{t("worlds.listTitle")}</CardTitle>
            </div>
            {saves.length > 0 && (
              <Button onClick={() => setDialogOpen(true)}>{t("worlds.add")}</Button>
            )}
          </div>
        </CardHeader>
        <CardContent>
          {error && <p className="mb-4 text-sm text-destructive">{error}</p>}
          {!loaded ? (
            <p className="text-sm text-muted-foreground">{t("worlds.loading")}</p>
          ) : saves.length === 0 ? (
            <EmptySave onAddTrack={() => setDialogOpen(true)} />
          ) : (
            <Table className="table-fixed">
              <TableHeader>
                <TableRow>
                  <TableHead className="w-auto text-muted-foreground">
                    {t("worlds.name")}
                  </TableHead>
                  <TableHead className="w-52 text-muted-foreground">
                    {t("worlds.lastOpened")}
                  </TableHead>
                  <TableHead className="w-18">
                    <span className="sr-only">{t("worlds.actions")}</span>
                  </TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {saves.map((save) => (
                  <SaveHoverCard key={save.name} save={save}>
                    <TableRow>
                      <TableCell className="truncate text-left">
                        {save.name}
                      </TableCell>
                      <TableCell>{save.last_access}</TableCell>
                      <TableCell className="text-right">
                        <Button
                          variant="ghost"
                          size="icon-sm"
                          className="cursor-pointer"
                          onClick={(e) => {
                            e.stopPropagation()
                            setDeleteTarget(save.name)
                            setDeleteRepoChecked(false)
                            setDeleteDialogOpen(true)
                          }}
                        >
                          <Trash2 />
                        </Button>
                      </TableCell>
                    </TableRow>
                  </SaveHoverCard>
                ))}
              </TableBody>
            </Table>
          )}
        </CardContent>
      </Card>

      <AddTrackDialog
        open={dialogOpen}
        onOpenChange={setDialogOpen}
        onSaveAdded={refreshSaves}
      />

      <Dialog open={deleteDialogOpen} onOpenChange={setDeleteDialogOpen}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>{t("worlds.deleteTitle")}</DialogTitle>
            <DialogDescription>
              {t("worlds.deleteDescription", { world: deleteTarget ?? "" })}
            </DialogDescription>
          </DialogHeader>
          <FieldGroup>
            <Field orientation="horizontal" data-disabled>
              <Checkbox
                id="delete-repo-checkbox"
                name="delete-repo-checkbox"
                checked={deleteRepoChecked}
                onCheckedChange={(checked) =>
                  setDeleteRepoChecked(checked === true)
                }
              />
              <Label htmlFor="delete-repo-checkbox">
                {t("worlds.deleteRepository")}
              </Label>
            </Field>
          </FieldGroup>
          <DialogFooter>
            <Button
              variant="outline"
              onClick={() => {
                setDeleteDialogOpen(false)
                setDeleteTarget(null)
                setDeleteRepoChecked(false)
              }}
            >
              {t("common.cancel")}
            </Button>
            <Button variant="destructive" onClick={handleConfirmDelete}>
              {t("worlds.delete")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  )
}
