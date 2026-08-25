import { useEffect, useState } from "react"
import { Loader2 } from "lucide-react"

import { useI18n } from "@/contexts/i18n"
import {
  clock,
  fractionDone,
  progressReadout,
  remainingLabel,
  secondsRemaining,
  PHASE_TITLE,
  type BackupProgress,
} from "@/lib/cloud"

/**
 * How far a download or a restore has got, in one compact block.
 *
 * A big world takes minutes to come down, and "Downloading…" beside a spinner
 * for all of them is the same picture as an app that has hung. What is missing
 * is any sense of the size of the wait, so this says all three things it can
 * honestly say: how far along, how much of what, and how long it has been.
 *
 * Before the first reading arrives there is nothing to divide by, so it counts
 * up alone rather than showing a bar sitting at zero.
 */
export function TransferProgress({
  progress,
  fallback,
}: {
  progress: BackupProgress | null
  /** What to call the work before the first reading says what it is. */
  fallback: string
}) {
  const { locale, t } = useI18n()

  // Only fills the gap before the first report. Everything after that is timed
  // in Rust, on a clock that stops while the computer is asleep -- this
  // window's timers are throttled when it is not on screen, which is exactly
  // when a long download is left to run.
  const [ticks, setTicks] = useState(0)
  useEffect(() => {
    const timer = setInterval(() => setTicks((count) => count + 1), 1000)
    return () => clearInterval(timer)
  }, [])

  const fraction = fractionDone(progress)
  const phase = progress?.phase ?? "idle"
  const headline = PHASE_TITLE[phase]
  const readout = progressReadout(progress, locale, t)
  const seconds = progress ? progress.job_seconds : ticks
  const left = secondsRemaining(fraction, progress?.phase_seconds ?? 0)

  return (
    <div className="flex flex-col gap-2 py-1">
      <p className="flex items-center gap-2 text-sm">
        {fraction === null && (
          <Loader2 className="size-4 shrink-0 animate-spin text-muted-foreground" />
        )}
        <span className="min-w-0 truncate">{headline ? t(headline) : fallback}</span>
      </p>

      {fraction !== null && (
        <div
          role="progressbar"
          aria-valuemin={0}
          aria-valuemax={100}
          aria-valuenow={Math.round(fraction * 100)}
          className="h-2 w-full overflow-hidden rounded-full bg-muted"
        >
          <div
            className="h-full rounded-full bg-primary transition-[width] duration-300 ease-out"
            style={{ width: `${Math.round(fraction * 1000) / 10}%` }}
          />
        </div>
      )}

      <p className="flex items-baseline justify-between gap-2 font-mono text-xs text-muted-foreground">
        <span>
          {fraction !== null ? `${Math.floor(fraction * 100)}% · ` : ""}
          {readout || t("state.elapsed", { time: clock(seconds) })}
        </span>
        {readout && (
          <span className="shrink-0">
            {left !== null
              ? remainingLabel(left, t)
              : t("state.elapsed", { time: clock(seconds) })}
          </span>
        )}
      </p>
    </div>
  )
}
