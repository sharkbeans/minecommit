import { useCallback, useEffect, useMemo, useState } from "react"
import { invoke } from "@tauri-apps/api/core"
import { openUrl } from "@tauri-apps/plugin-opener"
import {
  Check,
  ExternalLink,
  Loader2,
  Lock,
  ShieldCheck,
  Wallet,
  type LucideIcon,
} from "lucide-react"

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
import { Field, FieldGroup } from "@/components/ui/field"
import { Label } from "@/components/ui/label"
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
import { GithubMark, type GitHubAccount } from "@/components/github-account"
import { useI18n, type TranslationKey } from "@/contexts/i18n"
import type { Save } from "@/contexts/saves"
import { asRepositoryName, cloudErrorLabel, type GrantedRepository } from "@/lib/cloud"

/** The name suggested for a new backup space, and for a world inside one. */
const SUGGESTED_SPACE = "minecraft-worlds"

function errorText(error: unknown) {
  return error instanceof Error ? error.message : String(error)
}

function openExternal(url: string) {
  void openUrl(url).catch((error) => console.error("Unable to open browser:", error))
}

/** How often to look for the permission the player is granting on GitHub. */
const POLL_MS = 4000

/**
 * How many times, before giving up and waiting to be asked.
 *
 * A dialog somebody walked away from should not keep talking to GitHub for the
 * rest of the afternoon; six minutes is longer than the step takes.
 */
const MAX_POLLS = 90

type Stage = "signIn" | "create" | "grant" | "pick"

/**
 * Setting a world up for online backup, as a walkthrough rather than a form.
 *
 * MineCommit cannot create a repository or grant itself access to one -- both
 * would need authority over the whole account, which is exactly the breadth
 * that picking individual repositories avoids. So two of the four steps happen
 * on github.com, and the only thing that makes them survivable for someone who
 * has never been there is telling them precisely which buttons to press, in
 * GitHub's own words, and then noticing when they are done rather than asking
 * them to come back and press refresh.
 */
export function CloudSetupDialog({
  open,
  onOpenChange,
  save,
  account,
  onNeedSignIn,
  onConnected,
}: {
  open: boolean
  onOpenChange: (open: boolean) => void
  save: Save | null
  account: GitHubAccount | null
  onNeedSignIn: () => void
  onConnected: () => Promise<void> | void
}) {
  const { t } = useI18n()
  const world = save?.name ?? ""

  const [repos, setRepos] = useState<GrantedRepository[] | null>(null)
  const [manualStage, setManualStage] = useState<Stage | null>(null)
  const [openedCreatePage, setOpenedCreatePage] = useState(false)
  const [pickedRepo, setPickedRepo] = useState("")
  // Follows from the world and nothing can change it, so it is derived rather
  // than held: there is no state here for a stale name to survive in.
  const branch = asRepositoryName(world)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState("")
  const [justFound, setJustFound] = useState(false)
  const [polls, setPolls] = useState(0)

  const loadRepos = useCallback(async () => {
    try {
      const granted = await invoke<GrantedRepository[]>("github_repositories")
      setRepos(granted)
      setError("")
      return granted
    } catch (err) {
      setRepos([])
      setError(cloudErrorLabel(errorText(err), t))
      return [] as GrantedRepository[]
    }
  }, [t])

  useEffect(() => {
    if (!open || !account) return
    let ignore = false
    invoke<GrantedRepository[]>("github_repositories")
      .then((granted) => {
        if (ignore) return
        setRepos(granted)
      })
      .catch((err) => {
        if (ignore) return
        setRepos([])
        setError(cloudErrorLabel(errorText(err), t))
      })
    return () => {
      ignore = true
    }
  }, [account, open, t])

  // Derived, not stored: a selection kept in state can outlive the list it
  // came from, and a Select whose value matches nothing renders empty next to
  // a button that does nothing and never says why.
  const chosen =
    pickedRepo && repos?.some((repo) => repo.clone_url === pickedRepo)
      ? pickedRepo
      : (repos?.[0]?.clone_url ?? "")

  const stage: Stage = !account
    ? "signIn"
    : (manualStage ?? (repos !== null && repos.length === 0 ? "create" : "pick"))

  // While the player is on GitHub ticking a repository there is nothing for
  // them to do here, and nothing tells us when they finish. So keep asking:
  // the moment the permission lands, this moves itself on.
  useEffect(() => {
    if (!open || stage !== "grant" || polls >= MAX_POLLS) return
    const timer = setInterval(() => {
      void (async () => {
        setPolls((count) => count + 1)
        const granted = await loadRepos()
        if (granted.length > 0) {
          setJustFound(true)
          setManualStage("pick")
        }
      })()
    }, POLL_MS)
    return () => clearInterval(timer)
  }, [loadRepos, open, polls, stage])

  const connect = useCallback(async () => {
    if (!save || busy || !chosen) return
    setBusy(true)
    setError("")
    try {
      await invoke<Save>("configure_save_cloud", {
        name: save.name,
        remoteUrl: chosen,
        branch: branch.trim(),
      })
      await onConnected()
      onOpenChange(false)
    } catch (err) {
      setError(cloudErrorLabel(errorText(err), t))
    } finally {
      setBusy(false)
    }
  }, [branch, busy, chosen, onConnected, onOpenChange, save, t])

  const openInstallPage = useCallback(async () => {
    try {
      openExternal(await invoke<string>("github_install_url"))
    } catch (err) {
      setError(errorText(err))
    }
  }, [])

  const body = useMemo(() => {
    switch (stage) {
      case "signIn":
        return <SignInStep />
      case "create":
        return (
          <CreateStep
            opened={openedCreatePage}
            onOpen={() => {
              setOpenedCreatePage(true)
              openExternal(
                `https://github.com/new?name=${encodeURIComponent(SUGGESTED_SPACE)}&visibility=private`
              )
            }}
            onMade={() => setManualStage("grant")}
          />
        )
      case "grant":
        return (
          <GrantStep
            watching={polls < MAX_POLLS}
            onCheck={() => {
              setPolls(0)
              void loadRepos()
            }}
          />
        )
      case "pick":
        return (
          <PickStep
            repos={repos}
            chosen={chosen}
            onChoose={setPickedRepo}
            branch={branch}
            found={justFound}
            onAddAnother={() => {
              setManualStage("create")
              setOpenedCreatePage(false)
            }}
          />
        )
    }
  }, [
    branch,
    chosen,
    justFound,
    loadRepos,
    openedCreatePage,
    polls,
    repos,
    stage,
  ])

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-lg">
        <DialogHeader>
          {(stage === "create" || stage === "grant") && (
            <p className="text-xs font-medium tracking-wide text-muted-foreground uppercase">
              {t("setup.stepOf", { n: stage === "create" ? 1 : 2, total: 2 })}
            </p>
          )}
          <DialogTitle>
            {stage === "pick"
              ? t("setup.pick.title", { world })
              : stage === "signIn"
                ? t("setup.signIn.title")
                : stage === "create"
                  ? t("setup.create.title")
                  : t("setup.grant.title")}
          </DialogTitle>
          <DialogDescription>
            {stage === "pick"
              ? t("setup.pick.body")
              : stage === "signIn"
                ? t("setup.signIn.body")
                : stage === "create"
                  ? t("setup.create.body")
                  : t("setup.grant.body")}
          </DialogDescription>
        </DialogHeader>

        {body}

        {error && <p className="text-sm break-words text-destructive">{error}</p>}

        <DialogFooter>
          <DialogClose render={<Button variant="outline">{t("common.cancel")}</Button>} />
          {stage === "signIn" && (
            <Button
              onClick={() => {
                onOpenChange(false)
                onNeedSignIn()
              }}
            >
              <GithubMark data-icon="inline-start" className="size-4" />
              {t("gh.signIn")}
            </Button>
          )}
          {stage === "create" &&
            (openedCreatePage ? (
              <Button onClick={() => setManualStage("grant")}>{t("setup.create.made")}</Button>
            ) : (
              <Button
                onClick={() => {
                  setOpenedCreatePage(true)
                  openExternal(
                    `https://github.com/new?name=${encodeURIComponent(SUGGESTED_SPACE)}&visibility=private`
                  )
                }}
              >
                <ExternalLink data-icon="inline-start" />
                {t("setup.create.open")}
              </Button>
            ))}
          {stage === "grant" && (
            <Button onClick={() => void openInstallPage()}>
              <ExternalLink data-icon="inline-start" />
              {t("setup.grant.open")}
            </Button>
          )}
          {stage === "pick" && (
            <Button
              onClick={() => void connect()}
              disabled={busy || !chosen || !branch.trim() || repos === null}
            >
              {busy ? t("setup.pick.working") : t("setup.pick.action")}
            </Button>
          )}
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}

/* ── Step: sign in ───────────────────────────────────────────────────────── */

/** Where a player with no GitHub account starts. */
const SIGN_UP_URL = "https://github.com/signup"

function SignInStep() {
  const { t } = useI18n()
  const points: Array<[LucideIcon, TranslationKey]> = [
    [Wallet, "setup.signIn.free"],
    [Lock, "setup.signIn.private"],
    [ShieldCheck, "setup.signIn.password"],
  ]
  return (
    <div className="flex flex-col gap-3">
      <ul className="flex flex-col gap-2.5">
        {points.map(([Icon, key]) => (
          <li key={key} className="flex items-start gap-2.5 text-sm">
            <Icon className="mt-0.5 size-4 shrink-0 text-muted-foreground" />
            <span className="text-muted-foreground">{t(key)}</span>
          </li>
        ))}
      </ul>
      <p className="text-sm text-muted-foreground">
        {t("setup.signIn.noAccount")}{" "}
        <button
          type="button"
          className="text-foreground underline underline-offset-2"
          onClick={() => openExternal(SIGN_UP_URL)}
        >
          {t("setup.signIn.createAccount")}
        </button>
      </p>
    </div>
  )
}

/* ── Step: make a backup space ───────────────────────────────────────────── */

function NumberedSteps({ steps }: { steps: string[] }) {
  return (
    <ol className="flex flex-col gap-2.5">
      {steps.map((text, index) => (
        <li key={text} className="flex items-start gap-3 text-sm">
          <span className="mt-px flex size-5 shrink-0 items-center justify-center rounded-full bg-muted text-xs font-medium">
            {index + 1}
          </span>
          <span className="text-muted-foreground">{text}</span>
        </li>
      ))}
    </ol>
  )
}

function CreateStep({
  opened,
  onOpen,
  onMade,
}: {
  opened: boolean
  onOpen: () => void
  onMade: () => void
}) {
  const { t } = useI18n()
  return (
    <div className="flex flex-col gap-4">
      <NumberedSteps
        steps={[
          t("setup.create.step1", { name: SUGGESTED_SPACE }),
          t("setup.create.step2"),
          t("setup.create.step3"),
          t("setup.create.step4"),
        ]}
      />
      {opened ? (
        <Button variant="ghost" size="sm" className="self-start" onClick={onOpen}>
          <ExternalLink data-icon="inline-start" />
          {t("setup.create.open")}
        </Button>
      ) : (
        <button
          type="button"
          className="self-start text-sm text-muted-foreground underline underline-offset-4 hover:text-foreground"
          onClick={onMade}
        >
          {t("setup.create.skip")}
        </button>
      )}
    </div>
  )
}

/* ── Step: allow MineCommit to use it ────────────────────────────────────── */

function GrantStep({ watching, onCheck }: { watching: boolean; onCheck: () => void }) {
  const { t } = useI18n()
  return (
    <div className="flex flex-col gap-4">
      <NumberedSteps
        steps={[t("setup.grant.step1"), t("setup.grant.step2"), t("setup.grant.step3")]}
      />
      <div className="flex items-center gap-2 rounded-lg bg-muted/60 px-3 py-2">
        {watching && (
          <Loader2 className="size-4 shrink-0 animate-spin text-muted-foreground" />
        )}
        <span className="min-w-0 flex-1 text-xs text-muted-foreground">
          {watching ? t("setup.grant.waiting") : t("setup.grant.stopped")}
        </span>
        <Button variant="ghost" size="sm" onClick={onCheck}>
          {t("common.checkNow")}
        </Button>
      </div>
    </div>
  )
}

/* ── Step: choose where this world goes ──────────────────────────────────── */

function PickStep({
  repos,
  chosen,
  onChoose,
  branch,
  found,
  onAddAnother,
}: {
  repos: GrantedRepository[] | null
  chosen: string
  onChoose: (url: string) => void
  branch: string
  found: boolean
  onAddAnother: () => void
}) {
  const { t } = useI18n()

  if (repos === null) {
    return (
      <p className="flex items-center gap-2 py-6 text-sm text-muted-foreground">
        <Loader2 className="size-4 animate-spin" />
        {t("setup.pick.loading")}
      </p>
    )
  }

  return (
    <FieldGroup>
      {found && (
        <p className="flex items-center gap-2 text-sm text-emerald-600 dark:text-emerald-400">
          <Check className="size-4" />
          {t("setup.grant.found")}
        </p>
      )}
      <Field>
        <Label htmlFor="setup-repo">{t("setup.pick.repo")}</Label>
        <Select value={chosen} onValueChange={(value) => onChoose(value ?? "")}>
          <SelectTrigger id="setup-repo" className="w-full">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {repos.map((repo) => (
              <SelectItem key={repo.clone_url} value={repo.clone_url}>
                {repo.full_name}
                {repo.private ? ` · ${t("setup.pick.private")}` : ""}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
        <p className="text-xs text-muted-foreground">{t("setup.pick.repoHelp")}</p>
      </Field>
      {/* Shown, not asked. The name a world is backed up under is the name
          every other computer will know it by, so it is settled once here and
          never typed again: a world renamed on one machine is the same world
          under two names, and nothing afterwards can tell they are the same. */}
      <Field>
        <Label>{t("setup.pick.name")}</Label>
        <p className="text-sm">{branch}</p>
        <p className="text-xs text-muted-foreground">{t("setup.pick.nameHelp")}</p>
      </Field>
      <button
        type="button"
        className="self-start text-sm text-muted-foreground underline underline-offset-4 hover:text-foreground"
        onClick={onAddAnother}
      >
        {t("setup.pick.another")}
      </button>
    </FieldGroup>
  )
}
