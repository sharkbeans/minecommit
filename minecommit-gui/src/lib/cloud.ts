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

/** A repository the player has given MineCommit access to. */
export interface GrantedRepository {
  full_name: string
  clone_url: string
  private: boolean
}

/** GitHub allows letters, digits, dot, dash and underscore in a name. */
export function asRepositoryName(world: string) {
  return (
    world
      .trim()
      .replace(/[^A-Za-z0-9._-]+/g, "-")
      .replace(/^-+|-+$/g, "") || "minecraft-world"
  )
}

/** A world copy an earlier restore left in the saves folder. */
export interface OldCopy {
  world: string
  path: string
  taken: string | null
  bytes: number
}

/** "4.2 GB" -- coarse on purpose; the point is whether it is worth clearing. */
export function fileSize(bytes: number): string {
  const units = ["B", "KB", "MB", "GB", "TB"]
  let size = bytes
  let unit = 0
  while (size >= 1024 && unit < units.length - 1) {
    size /= 1024
    unit += 1
  }
  return `${size < 10 && unit > 0 ? size.toFixed(1) : Math.round(size)} ${units[unit]}`
}

export interface FoundWorld {
  name: string
  path: string
  last_played: string | null
}

export interface WorldState {
  /** Null when it could not be answered without taking Minecraft's lock. */
  idle: boolean | null
  last_played: string | null
}

/** How far the running backup or restore has got, from the Rust side. */
export interface BackupProgress {
  done: number
  total: number
}

/**
 * The fraction done, or null when there is nothing to divide by.
 *
 * A file can be read more than once and a handful are only skipped, so the
 * count can drift past the total or stop just short of it. Neither is worth
 * explaining to the player: clamp, and let the phase ending fill the bar.
 */
export function fractionDone(progress: BackupProgress | null): number | null {
  if (!progress || progress.total <= 0) return null
  return Math.min(1, Math.max(0, progress.done / progress.total))
}

/**
 * Seconds still to go, guessed from how long the work so far has taken.
 *
 * Held back until a twentieth of the way in: before that the rate is measured
 * over so little work that the estimate swings by minutes between updates,
 * which is worse than showing nothing.
 */
export function secondsRemaining(
  fraction: number | null,
  elapsedSeconds: number
): number | null {
  if (fraction === null || fraction < 0.05 || fraction >= 1) return null
  if (elapsedSeconds < 3) return null
  return Math.round((elapsedSeconds / fraction) * (1 - fraction))
}

/** "about 2 min left", in the coarsest unit that is still honest. */
export function remainingLabel(
  seconds: number,
  t: (key: TranslationKey, values?: Record<string, string | number>) => string
): string {
  if (seconds < 60) return t("state.secondsLeft", { seconds: Math.max(5, Math.round(seconds / 5) * 5) })
  const minutes = Math.round(seconds / 60)
  if (minutes < 60) return t("state.minutesLeft", { minutes })
  return t("state.hoursLeft", { hours: Math.round(seconds / 360) / 10 })
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
  // it only overrides the states whose action is to back up. Unknown is not
  // "open": the backup checks properly before it runs, and claiming a world is
  // in use when nobody knows would hide the button for no reason.
  const busy = world?.idle === false

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
