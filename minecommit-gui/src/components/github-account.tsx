import { useCallback, useEffect, useRef, useState } from "react"
import { invoke } from "@tauri-apps/api/core"
import { openUrl } from "@tauri-apps/plugin-opener"
import { ExternalLink, FolderCog, Loader2, LogOut, UserRound } from "lucide-react"

import { Button } from "@/components/ui/button"
import { Skeleton } from "@/components/ui/skeleton"
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog"
import { useI18n } from "@/contexts/i18n"

/** Lucide dropped its brand icons, and GitHub's mark is the recognisable one. */
export function GithubMark({ className }: { className?: string }) {
  return (
    <svg viewBox="0 0 16 16" fill="currentColor" aria-hidden className={className}>
      <path d="M8 0C3.58 0 0 3.58 0 8c0 3.54 2.29 6.53 5.47 7.59.4.07.55-.17.55-.38 0-.19-.01-.82-.01-1.49-2.01.37-2.53-.49-2.69-.94-.09-.23-.48-.94-.82-1.13-.28-.15-.68-.52-.01-.53.63-.01 1.08.58 1.23.82.72 1.21 1.87.87 2.33.66.07-.52.28-.87.51-1.07-1.78-.2-3.64-.89-3.64-3.95 0-.87.31-1.59.82-2.15-.08-.2-.36-1.02.08-2.12 0 0 .67-.21 2.2.82.64-.18 1.32-.27 2-.27s1.36.09 2 .27c1.53-1.04 2.2-.82 2.2-.82.44 1.1.16 1.92.08 2.12.51.56.82 1.27.82 2.15 0 3.07-1.87 3.75-3.65 3.95.29.25.54.73.54 1.48 0 1.07-.01 1.93-.01 2.2 0 .21.15.46.55.38A8.01 8.01 0 0 0 16 8c0-4.42-3.58-8-8-8Z" />
    </svg>
  )
}

export interface GitHubAccount {
  login: string
  avatar_url: string | null
}

/** Where a player who has never used GitHub starts. */
const SIGN_UP_URL = "https://github.com/signup"

function openExternal(url: string) {
  void openUrl(url).catch((error) => console.error("Unable to open browser:", error))
}

function errorText(error: unknown) {
  return error instanceof Error ? error.message : String(error)
}

/* ── The dropdown in the top right ───────────────────────────────────────── */

/** One row in the account menu, styled to match the rest of the app's menus. */
function MenuRow({
  onClick,
  children,
}: {
  onClick: () => void
  children: React.ReactNode
}) {
  return (
    <button
      type="button"
      role="menuitem"
      onClick={onClick}
      className="flex min-h-7 w-full items-center gap-2 rounded-xl px-2 py-1.5 text-left text-sm hover:bg-accent hover:text-accent-foreground focus-visible:bg-accent focus-visible:text-accent-foreground focus-visible:outline-none [&_svg]:size-4 [&_svg]:shrink-0"
    >
      {children}
    </button>
  )
}

/**
 * The account menu.
 *
 * Written out by hand rather than with the shared dropdown primitive: that
 * primitive's trigger does not open its menu in this app -- verified by
 * dispatching a full pointer sequence at it and watching `aria-expanded` stay
 * false, with the menu itself rendering correctly when forced open. This is the
 * only menu MineCommit has and it holds three rows, so owning fifteen lines of
 * open/close is cheaper than depending on machinery that does not work.
 */
export function AccountMenu({
  account,
  loaded,
  onSignIn,
  onSignedOut,
  onChooseRepositories,
}: {
  account: GitHubAccount | null
  /** False until the saved account has been read back from disk. */
  loaded: boolean
  onSignIn: () => void
  onSignedOut: () => void
  onChooseRepositories: () => void
}) {
  const { t } = useI18n()
  const [open, setOpen] = useState(false)
  const holder = useRef<HTMLDivElement>(null)

  const signOut = useCallback(async () => {
    try {
      await invoke("github_sign_out")
    } catch (error) {
      console.error("Sign out failed:", error)
    }
    onSignedOut()
  }, [onSignedOut])

  // A menu that cannot be dismissed by looking away is a trap, so close on a
  // press anywhere else and on Escape.
  useEffect(() => {
    if (!open) return
    const onPress = (event: MouseEvent) => {
      if (!holder.current?.contains(event.target as Node)) setOpen(false)
    }
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") setOpen(false)
    }
    document.addEventListener("mousedown", onPress)
    document.addEventListener("keydown", onKey)
    return () => {
      document.removeEventListener("mousedown", onPress)
      document.removeEventListener("keydown", onKey)
    }
  }, [open])

  // Reading the saved account takes a moment. Rendering "Sign in to GitHub"
  // meanwhile would tell an already signed-in player the opposite of the truth
  // and then correct itself, which reads as a glitch.
  if (!loaded) {
    return <Skeleton className="h-8 w-28 rounded-md" />
  }

  // Signed out there is exactly one thing to do, so it is a button. Putting it
  // behind a menu that opens to reveal a single item only adds a click and a
  // place to get stuck.
  if (!account) {
    return (
      <Button variant="outline" size="sm" className="gap-2" onClick={onSignIn}>
        <GithubMark className="size-4" />
        {t("gh.signIn")}
      </Button>
    )
  }

  const choose = (act: () => void) => () => {
    setOpen(false)
    act()
  }

  return (
    <div className="relative" ref={holder}>
      <Button
        variant="ghost"
        size="sm"
        className="gap-2"
        aria-haspopup="menu"
        aria-expanded={open}
        onClick={() => setOpen((current) => !current)}
      >
        {account.avatar_url ? (
          <img src={account.avatar_url} alt="" className="size-5 rounded-full" />
        ) : (
          <UserRound />
        )}
        <span className="max-w-32 truncate">{account.login}</span>
      </Button>

      {open && (
        <div
          role="menu"
          className="absolute top-full right-0 z-50 mt-1 w-56 rounded-2xl bg-popover p-1 text-popover-foreground shadow-lg ring-1 ring-foreground/5 dark:ring-foreground/10"
        >
          <p className="px-2 py-1 text-xs text-muted-foreground">
            {t("gh.signedInAs", { login: account.login })}
          </p>
          <MenuRow onClick={choose(onChooseRepositories)}>
            <FolderCog />
            {t("gh.chooseRepos")}
          </MenuRow>
          <MenuRow
            onClick={choose(() => openExternal(`https://github.com/${account.login}`))}
          >
            <ExternalLink />
            {t("gh.openProfile")}
          </MenuRow>
          <div className="my-1 h-px bg-border" />
          <MenuRow onClick={choose(() => void signOut())}>
            <LogOut />
            {t("gh.signOut")}
          </MenuRow>
        </div>
      )}
    </div>
  )
}

/* ── Signing in ──────────────────────────────────────────────────────────── */

interface SignInRequest {
  user_code: string
  verification_uri: string
  expires_in_seconds: number
  retry_in_seconds: number
}

type SignInProgress =
  | { status: "waiting"; retry_in_seconds: number }
  | { status: "authorized"; account: GitHubAccount }
  | { status: "denied" }
  | { status: "expired" }

/**
 * Sign in with GitHub.
 *
 * GitHub shows a code, the player types it on github.com, and MineCommit waits.
 * Nothing is typed into MineCommit, and no password or token passes through it:
 * GitHub hands back an access token afterwards, which goes straight to Git's
 * credential store so pushes authenticate on their own.
 */
export function GitHubSignInDialog({
  open,
  onOpenChange,
  onSignedIn,
}: {
  open: boolean
  onOpenChange: (open: boolean) => void
  onSignedIn: (account: GitHubAccount) => void
}) {
  const { t } = useI18n()
  const [request, setRequest] = useState<SignInRequest | null>(null)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState("")
  const [copied, setCopied] = useState(false)
  const cancelled = useRef(false)

  const finish = useCallback(() => {
    cancelled.current = true
    void invoke("github_cancel_sign_in").catch(() => {})
    onOpenChange(false)
  }, [onOpenChange])

  const start = useCallback(async () => {
    if (busy) return
    setBusy(true)
    setError("")
    cancelled.current = false
    try {
      const started = await invoke<SignInRequest>("github_start_sign_in")
      setRequest(started)
      openExternal(started.verification_uri)

      // Wait for the player to finish on GitHub's page, asking at the pace
      // GitHub sets. It tells us to slow down if we ask too often.
      let wait = started.retry_in_seconds
      const deadline = Date.now() + started.expires_in_seconds * 1000
      while (!cancelled.current && Date.now() < deadline) {
        await new Promise((resolve) => setTimeout(resolve, wait * 1000))
        if (cancelled.current) return
        const progress = await invoke<SignInProgress>("github_poll_sign_in")
        if (progress.status === "authorized") {
          onSignedIn(progress.account)
          onOpenChange(false)
          return
        }
        if (progress.status === "denied") {
          setError(t("gh.denied"))
          setRequest(null)
          return
        }
        if (progress.status === "expired") {
          setError(t("gh.expired"))
          setRequest(null)
          return
        }
        wait = progress.retry_in_seconds
      }
      if (!cancelled.current) {
        setError(t("gh.expired"))
        setRequest(null)
      }
    } catch (err) {
      setError(errorText(err))
      setRequest(null)
    } finally {
      setBusy(false)
    }
  }, [busy, onOpenChange, onSignedIn, t])

  const copyCode = useCallback(async () => {
    if (!request) return
    try {
      await navigator.clipboard.writeText(request.user_code)
      setCopied(true)
      setTimeout(() => setCopied(false), 2000)
    } catch {
      // The code is on screen either way.
    }
  }, [request])

  return (
    <Dialog open={open} onOpenChange={(next) => (next ? onOpenChange(true) : finish())}>
      <DialogContent className="sm:max-w-md">
        <DialogHeader>
          <DialogTitle>{t("gh.signInTitle")}</DialogTitle>
          <DialogDescription>{t("gh.signInBody")}</DialogDescription>
        </DialogHeader>

        {request ? (
          <div className="flex flex-col items-center gap-4 py-2">
            <p className="text-sm text-muted-foreground">{t("gh.enterCode")}</p>
            <button
              type="button"
              onClick={() => void copyCode()}
              title={t("gh.copy")}
              className="rounded-lg border px-6 py-3 font-mono text-2xl tracking-[0.3em] transition-colors hover:bg-muted"
            >
              {request.user_code}
            </button>
            <span className="text-xs text-muted-foreground">
              {copied ? t("gh.copied") : t("gh.tapToCopy")}
            </span>

            <p className="flex items-center gap-2 text-sm text-muted-foreground">
              <Loader2 className="size-4 animate-spin" />
              {t("gh.waiting")}
            </p>

            <Button variant="link" onClick={() => openExternal(request.verification_uri)}>
              <ExternalLink data-icon="inline-start" />
              {t("gh.reopenPage")}
            </Button>
            <p className="text-center text-xs break-all text-muted-foreground">
              {request.verification_uri}
            </p>
          </div>
        ) : (
          <div className="flex flex-col gap-3 py-2">
            <p className="text-sm text-muted-foreground">{t("gh.howItWorks")}</p>
            <div className="flex items-center gap-2 text-sm">
              <span className="text-muted-foreground">{t("gh.noAccount")}</span>
              <Button
                variant="link"
                className="h-auto p-0"
                onClick={() => openExternal(SIGN_UP_URL)}
              >
                {t("gh.createAccount")}
              </Button>
            </div>
          </div>
        )}

        {error && <p className="text-sm break-words text-destructive">{error}</p>}

        <DialogFooter>
          <Button variant="outline" onClick={finish}>
            {t("common.cancel")}
          </Button>
          {!request && (
            <Button onClick={() => void start()} disabled={busy}>
              <GithubMark data-icon="inline-start" className="size-4" />
              {busy ? t("gh.signingIn") : t("gh.signIn")}
            </Button>
          )}
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
