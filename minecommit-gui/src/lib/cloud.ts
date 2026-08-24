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
  /** True while it sits among the worlds, where Minecraft lists it as one. */
  in_saves_folder: boolean
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
  /** The in-game name, when the folder has been renamed away from it. */
  level_name: string | null
  version: string | null
}

/** What level.dat says about a world. Every field may be missing. */
export interface LevelInfo {
  level_name: string | null
  version_name: string | null
  data_version: number | null
  snapshot_version: boolean
  game_mode: string | null
  hardcore: boolean
  difficulty: string | null
  difficulty_locked: boolean
  cheats: boolean
  last_played: number | null
  world_age_ticks: number | null
  /** Text, not a number: a 64-bit seed does not survive a JSON number. */
  seed: string | null
  spawn: [number, number, number] | null
  data_packs: string[]
  modded: boolean
}

export interface WorldDetails {
  level: LevelInfo | null
  bytes: number
}

/** One Minecraft day. */
const TICKS_PER_DAY = 24000

/** Which in-game day the world has reached. */
export function inGameDay(level: LevelInfo | null): number | null {
  if (level?.world_age_ticks == null) return null
  return Math.floor(level.world_age_ticks / TICKS_PER_DAY)
}

const GAME_MODES: Record<string, TranslationKey> = {
  survival: "world.mode.survival",
  creative: "world.mode.creative",
  adventure: "world.mode.adventure",
  spectator: "world.mode.spectator",
}

const DIFFICULTIES: Record<string, TranslationKey> = {
  peaceful: "world.difficulty.peaceful",
  easy: "world.difficulty.easy",
  normal: "world.difficulty.normal",
  hard: "world.difficulty.hard",
}

/**
 * The translated name of a game mode or difficulty.
 *
 * Minecraft has added modes and difficulties before and may again, and the
 * value arrives as whatever name the world itself used. A name with no
 * translation is shown as the world spelled it rather than dropped: telling
 * the player their world is on some difficulty this build has not heard of is
 * right, and telling them it has none is not.
 */
function nameOf(
  value: string | null,
  known: Record<string, TranslationKey>,
  t: (key: TranslationKey) => string
): string | null {
  if (!value) return null
  const key = known[value]
  return key ? t(key) : value.charAt(0).toUpperCase() + value.slice(1)
}

export function gameModeName(
  mode: string | null,
  t: (key: TranslationKey) => string
): string | null {
  return nameOf(mode, GAME_MODES, t)
}

export function difficultyName(
  difficulty: string | null,
  t: (key: TranslationKey) => string
): string | null {
  return nameOf(difficulty, DIFFICULTIES, t)
}

export interface WorldState {
  /** Null when it could not be answered without taking Minecraft's lock. */
  idle: boolean | null
  last_played: string | null
}

/** What the running work is doing. Matches `Phase` in `progress.rs`. */
export type Phase = "idle" | "reading" | "writing" | "downloading" | "uploading"

/** How far the running backup or restore has got, from the Rust side. */
export interface BackupProgress {
  phase: Phase
  files_done: number
  files_total: number
  bytes_done: number
  /** Zero when the size of the job is not knowable, as during a transfer. */
  bytes_total: number
  /** Seconds, from a clock that stops while the computer is asleep. */
  phase_seconds: number
  job_seconds: number
}

/**
 * Whether this phase's byte count is a size a player would recognise.
 *
 * Reading a world measures the world folder, and a transfer measures what
 * actually crosses the network -- both are sizes somebody could check for
 * themselves. Restoring is different: a world is stored one chunk at a time and
 * uncompressed, so Git can tell which chunks changed, and the same world that
 * takes 3.1 GB in the saves folder is 81 GB of stored pieces. That is a true
 * number and a useless one, so a restore is counted in pieces instead.
 */
const BYTES_ARE_A_SIZE: Record<Phase, boolean> = {
  idle: false,
  reading: true,
  writing: false,
  downloading: true,
  uploading: true,
}

/** The headline for each phase. Only "idle" has none, and never shows. */
export const PHASE_TITLE: Record<Phase, TranslationKey | null> = {
  idle: null,
  reading: "state.phase.reading",
  writing: "state.phase.writing",
  downloading: "state.phase.downloading",
  uploading: "state.phase.uploading",
}

/**
 * The fraction done, or null when there is nothing to divide by.
 *
 * Bytes first: a world is a few hundred multi-megabyte region files next to a
 * few thousand tiny ones, so a bar driven by the file count sprints through the
 * small ones and then sits still. During a transfer Git reports how much has
 * arrived but never how much is coming, so there the object count is all there
 * is.
 *
 * A file can be read more than once and a handful are only skipped, so either
 * count can drift past its total or stop just short of it. Neither is worth
 * explaining to the player: clamp, and let the phase ending fill the bar.
 */
export function fractionDone(progress: BackupProgress | null): number | null {
  if (!progress) return null
  const [done, total] =
    progress.bytes_total > 0
      ? [progress.bytes_done, progress.bytes_total]
      : [progress.files_done, progress.files_total]
  if (total <= 0) return null
  return Math.min(1, Math.max(0, done / total))
}

/**
 * How large the job is in total, and whether that is known or guessed.
 *
 * Reading a world or rebuilding one is measured before it starts, so the total
 * is exact. A transfer is not: Git says only how much has arrived, so the total
 * is extrapolated from the share of objects done -- and held back until enough
 * of them are through that the guess is not wild. A guessed figure is marked as
 * one wherever it is shown.
 */
export function totalBytes(
  progress: BackupProgress | null,
  fraction: number | null
): { bytes: number; estimated: boolean } | null {
  if (!progress || !BYTES_ARE_A_SIZE[progress.phase]) return null
  if (progress.bytes_total > 0) return { bytes: progress.bytes_total, estimated: false }
  if (fraction === null || fraction < 0.05 || progress.bytes_done <= 0) return null
  return { bytes: Math.round(progress.bytes_done / fraction), estimated: true }
}

/**
 * The figure shown beside the percentage: how much of the job is done, in
 * whichever unit this phase is honestly measured in.
 */
export function progressReadout(
  progress: BackupProgress | null,
  locale: string,
  t: (key: TranslationKey, values?: Record<string, string | number>) => string
): string {
  if (!progress) return ""
  if (!BYTES_ARE_A_SIZE[progress.phase]) {
    if (progress.files_total <= 0) return ""
    return t("state.piecesDone", {
      done: progress.files_done.toLocaleString(locale),
      total: progress.files_total.toLocaleString(locale),
    })
  }
  const total = totalBytes(progress, fractionDone(progress))
  if (total) return sizePair(progress.bytes_done, total.bytes, locale, total.estimated)
  // A transfer that has not moved enough yet for its size to be worth guessing.
  return progress.bytes_done > 0
    ? t("state.downloaded", { size: fileSize(progress.bytes_done) })
    : ""
}

const SIZE_UNITS = ["B", "KB", "MB", "GB", "TB"]

/** Megabytes: the unit people read a download in. */
const MB = 2

/**
 * "351 / 1,024 MB" -- both halves in one unit, chosen from the total.
 *
 * Sizing each half on its own produces "999 MB / 1.0 GB", which reads as two
 * unrelated numbers and hides how close the end is. And the largest unit that
 * fits is not the most useful one: a gigabyte job shown in gigabytes spends its
 * first half reading "0.4 / 1.0 GB", where a whole hundred megabytes of
 * progress moves the first digit once. So step back down until the total is a
 * two-digit number, but never below megabytes, which is the unit a transfer is
 * spoken about in.
 *
 * `approximate` marks a total that was extrapolated rather than measured.
 */
export function sizePair(
  done: number,
  total: number,
  locale: string,
  approximate = false
): string {
  let unit = 0
  let scale = 1
  while (total / scale >= 1024 && unit < SIZE_UNITS.length - 1) {
    scale *= 1024
    unit += 1
  }
  while (unit > MB && total / scale < 10) {
    scale /= 1024
    unit -= 1
  }
  const show = (value: number) => {
    const scaled = value / scale
    return scaled < 10 && unit > 0
      ? scaled.toFixed(1)
      : Math.round(scaled).toLocaleString(locale)
  }
  return `${show(done)} / ${approximate ? "~" : ""}${show(total)} ${SIZE_UNITS[unit]}`
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
