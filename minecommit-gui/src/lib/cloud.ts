import type { TranslationKey } from "@/contexts/i18n"

export type CloudState =
  | "not_configured"
  | "empty"
  | "up_to_date"
  | "local_ahead"
  | "remote_ahead"
  | "diverged"

export interface CloudStatus {
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

export interface HistoryEntry {
  id: string
  timestamp: string
  device: string | null
  note: string
}

export interface FoundWorld {
  name: string
  path: string
  last_played: string | null
}

export interface WorldState {
  idle: boolean
  last_played: string | null
}

export interface BackupResult {
  backed_up: boolean
  uploaded: boolean
  error: string | null
  upload_error: string | null
}

/**
 * What the player should do next about a world. This is the single value the
 * dashboard renders from: every headline, badge and button is chosen by it, so
 * there is exactly one place where "what is going on" is decided.
 */
export type Situation =
  | "checking"
  | "in_use"
  | "no_cloud"
  | "first_backup"
  | "needs_backup"
  | "up_to_date"
  | "newer_in_cloud"
  | "conflict"

export function situationOf(
  status: CloudStatus | null,
  world: WorldState | null,
  hasCloud: boolean
): Situation {
  if (!status) return "checking"

  // Minecraft holding the world open blocks a backup but not a diagnosis, so
  // it only overrides the states whose action is to back up.
  const busy = world ? !world.idle : false

  switch (status.state) {
    case "not_configured":
      return hasCloud ? "first_backup" : "no_cloud"
    case "diverged":
      return "conflict"
    case "remote_ahead":
      return "newer_in_cloud"
    case "empty":
      return busy ? "in_use" : "first_backup"
    case "local_ahead":
      return busy ? "in_use" : "needs_backup"
    case "up_to_date":
      if (playedSinceBackup(status, world)) return busy ? "in_use" : "needs_backup"
      return "up_to_date"
  }
}

/**
 * Whether the world folder has changed since the newest backup was taken.
 *
 * level.dat is rewritten when Minecraft saves, so its timestamp is a cheap
 * stand-in for "there is something new to record". A false positive costs one
 * duplicate history entry; a false negative would tell the player they are
 * backed up when they are not, so ties count as changed.
 */
export function playedSinceBackup(
  status: CloudStatus | null,
  world: WorldState | null
): boolean {
  if (!world?.last_played) return false
  if (!status?.local_timestamp) return true
  return new Date(world.last_played).getTime() >= new Date(status.local_timestamp).getTime()
}

/** Turn Git's plumbing errors into something a player can act on. */
export function cloudErrorLabel(error: string, t: (key: TranslationKey) => string) {
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

const RELATIVE_UNITS: Array<[Intl.RelativeTimeFormatUnit, number]> = [
  ["year", 31536000],
  ["month", 2592000],
  ["week", 604800],
  ["day", 86400],
  ["hour", 3600],
  ["minute", 60],
]

/** "5 minutes ago", "yesterday" — the only time format the dashboard shows. */
export function relativeTime(iso: string | null | undefined, locale: string): string {
  if (!iso) return ""
  const then = new Date(iso).getTime()
  if (Number.isNaN(then)) return ""

  const seconds = Math.round((then - Date.now()) / 1000)
  const format = new Intl.RelativeTimeFormat(locale, { numeric: "auto" })
  for (const [unit, size] of RELATIVE_UNITS) {
    if (Math.abs(seconds) >= size) return format.format(Math.round(seconds / size), unit)
  }
  return format.format(Math.round(seconds), "second")
}

/** The full date, for the tooltip behind a relative time. */
export function absoluteTime(iso: string | null | undefined, locale: string): string {
  if (!iso) return ""
  const date = new Date(iso)
  return Number.isNaN(date.getTime()) ? "" : date.toLocaleString(locale)
}

/**
 * Shorten a repository URL to the "owner/name" people recognise. Anything that
 * does not look like a hosted repository is returned unchanged, since a local
 * path is still more useful than an empty label.
 */
export function repoLabel(url: string | null | undefined): string {
  if (!url) return ""
  const trimmed = url.trim().replace(/\.git$/, "").replace(/\/+$/, "")
  const parts = trimmed.split(/[/:]/).filter(Boolean)
  return parts.length >= 2 ? parts.slice(-2).join("/") : trimmed
}
