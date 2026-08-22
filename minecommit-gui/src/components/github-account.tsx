import { useCallback, useState } from "react"
import { invoke } from "@tauri-apps/api/core"
import { openUrl } from "@tauri-apps/plugin-opener"
import { ExternalLink, LogOut, UserRound } from "lucide-react"

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
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuGroup,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu"
import { Field, FieldGroup } from "@/components/ui/field"
import { Input } from "@/components/ui/input"
import { Label } from "@/components/ui/label"
import { useI18n } from "@/contexts/i18n"

/** Lucide dropped its brand icons, and GitHub's mark is the recognisable one. */
function GithubMark({ className }: { className?: string }) {
  return (
    <svg viewBox="0 0 16 16" fill="currentColor" aria-hidden className={className}>
      <path d="M8 0C3.58 0 0 3.58 0 8c0 3.54 2.29 6.53 5.47 7.59.4.07.55-.17.55-.38 0-.19-.01-.82-.01-1.49-2.01.37-2.53-.49-2.69-.94-.09-.23-.48-.94-.82-1.13-.28-.15-.68-.52-.01-.53.63-.01 1.08.58 1.23.82.72 1.21 1.87.87 2.33.66.07-.52.28-.87.51-1.07-1.78-.2-3.64-.89-3.64-3.95 0-.87.31-1.59.82-2.15-.08-.2-.36-1.02.08-2.12 0 0 .67-.21 2.2.82.64-.18 1.32-.27 2-.27s1.36.09 2 .27c1.53-1.04 2.2-.82 2.2-.82.44 1.1.16 1.92.08 2.12.51.56.82 1.27.82 2.15 0 3.07-1.87 3.75-3.65 3.95.29.25.54.73.54 1.48 0 1.07-.01 1.93-.01 2.2 0 .21.15.46.55.38A8.01 8.01 0 0 0 16 8c0-4.42-3.58-8-8-8Z" />
    </svg>
  )
}

export interface GitHubAccount {
  login: string
  avatar_url: string | null
  /** Whether a repository can be created without asking for the token again. */
  can_create_repository: boolean
}

/**
 * Where a MineCommit token comes from. The scope and description are filled in
 * so the player only has to scroll down and press the green button.
 */
const NEW_TOKEN_URL =
  "https://github.com/settings/tokens/new?scopes=repo&description=MineCommit"

const SIGN_UP_URL = "https://github.com/signup"

function openExternal(url: string) {
  void openUrl(url).catch((error) => console.error("Unable to open browser:", error))
}

function errorText(error: unknown) {
  return error instanceof Error ? error.message : String(error)
}

/* ── The dropdown in the top right ───────────────────────────────────────── */

export function AccountMenu({
  account,
  onSignIn,
  onSignedOut,
}: {
  account: GitHubAccount | null
  onSignIn: () => void
  onSignedOut: () => void
}) {
  const { t } = useI18n()

  const signOut = useCallback(async () => {
    try {
      await invoke("github_sign_out")
    } catch (error) {
      console.error("Sign out failed:", error)
    }
    onSignedOut()
  }, [onSignedOut])

  return (
    <DropdownMenu>
      <DropdownMenuTrigger
        render={
          <Button variant="ghost" size="sm" className="gap-2">
            {account?.avatar_url ? (
              <img
                src={account.avatar_url}
                alt=""
                className="size-5 rounded-full"
              />
            ) : account ? (
              <UserRound />
            ) : (
              <GithubMark className="size-4" />
            )}
            <span className="max-w-32 truncate">
              {account ? account.login : t("gh.notSignedIn")}
            </span>
            {!account && (
              <span className="size-1.5 rounded-full bg-amber-500" aria-hidden />
            )}
          </Button>
        }
      />
      <DropdownMenuContent align="end" className="w-56">
        <DropdownMenuLabel>{t("gh.account")}</DropdownMenuLabel>
        {account ? (
          <DropdownMenuGroup>
            <DropdownMenuLabel className="text-xs font-normal text-muted-foreground">
              {t("gh.signedInAs", { login: account.login })}
            </DropdownMenuLabel>
            <DropdownMenuItem
              onClick={() => openExternal(`https://github.com/${account.login}`)}
            >
              <ExternalLink />
              {t("gh.openProfile")}
            </DropdownMenuItem>
            <DropdownMenuSeparator />
            <DropdownMenuItem onClick={() => void signOut()}>
              <LogOut />
              {t("gh.signOut")}
            </DropdownMenuItem>
          </DropdownMenuGroup>
        ) : (
          <DropdownMenuGroup>
            <DropdownMenuItem onClick={onSignIn}>
              <GithubMark className="size-4" />
              {t("gh.signIn")}
            </DropdownMenuItem>
          </DropdownMenuGroup>
        )}
      </DropdownMenuContent>
    </DropdownMenu>
  )
}

/* ── Signing in ──────────────────────────────────────────────────────────── */

/**
 * Sign-in by access token.
 *
 * A token rather than a password means MineCommit never handles the password,
 * the token can be revoked from GitHub alone, and Git can keep it in the
 * platform's credential store to authenticate pushes.
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
  const [token, setToken] = useState("")
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState("")

  const signIn = useCallback(async () => {
    if (!token.trim() || busy) return
    setBusy(true)
    setError("")
    try {
      const account = await invoke<GitHubAccount>("github_sign_in", { token: token.trim() })
      setToken("")
      onSignedIn(account)
      onOpenChange(false)
    } catch (err) {
      setError(errorText(err))
    } finally {
      setBusy(false)
    }
  }, [busy, onOpenChange, onSignedIn, token])

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-lg">
        <DialogHeader>
          <DialogTitle>{t("gh.signInTitle")}</DialogTitle>
          <DialogDescription>{t("gh.signInBody")}</DialogDescription>
        </DialogHeader>

        <div className="flex flex-col gap-3">
          <div className="flex items-center gap-2 text-sm">
            <span className="text-muted-foreground">{t("gh.noAccount")}</span>
            <Button variant="link" className="h-auto p-0" onClick={() => openExternal(SIGN_UP_URL)}>
              {t("gh.createAccount")}
            </Button>
          </div>

          <Button variant="outline" onClick={() => openExternal(NEW_TOKEN_URL)}>
            <GithubMark data-icon="inline-start" className="size-4" />
            {t("gh.getToken")}
          </Button>
          <p className="text-xs text-muted-foreground">{t("gh.getTokenSteps")}</p>

          <FieldGroup>
            <Field>
              <Label htmlFor="github-token">{t("gh.tokenLabel")}</Label>
              <Input
                id="github-token"
                type="password"
                value={token}
                placeholder="ghp_…"
                autoComplete="off"
                onChange={(e) => setToken(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === "Enter") void signIn()
                }}
              />
              <p className="text-xs text-muted-foreground">{t("gh.tokenHelp")}</p>
            </Field>
          </FieldGroup>

          {error && <p className="text-sm break-words text-destructive">{error}</p>}
        </div>

        <DialogFooter>
          <DialogClose render={<Button variant="outline">{t("common.cancel")}</Button>} />
          <Button onClick={() => void signIn()} disabled={busy || !token.trim()}>
            {busy ? t("gh.signingIn") : t("gh.signIn")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
