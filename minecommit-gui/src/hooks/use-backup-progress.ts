import { useEffect, useState } from "react"
import { listen } from "@tauri-apps/api/event"

import type { BackupProgress } from "@/lib/cloud"

/**
 * How far the running job has got, for as long as `active` is true.
 *
 * The Rust side reports this for every long job it runs, but until now only
 * the dashboard listened -- so anything started from a dialog showed a spinner
 * and nothing else, for however many minutes it took.
 */
export function useBackupProgress(active: boolean): BackupProgress | null {
  const [progress, setProgress] = useState<BackupProgress | null>(null)

  useEffect(() => {
    if (!active) return
    let stop: Array<() => void> = []
    let cancelled = false

    void (async () => {
      // A new run starts from nothing. The last run's numbers are not this
      // one's, and there is nothing to draw until the first reading lands.
      setProgress(null)
      const moved = await listen<BackupProgress>("backup-progress", (event) => {
        setProgress(event.payload)
      })
      const finished = await listen("commit-finished", () => setProgress(null))
      // Subscribing takes a round trip to the Rust side, and a dialog can be
      // closed inside it. Without this the listeners outlive the component.
      if (cancelled) {
        moved()
        finished()
        return
      }
      stop = [moved, finished]
    })()

    return () => {
      cancelled = true
      stop.forEach((off) => off())
    }
  }, [active])

  // Nothing is running, so nothing is the honest answer -- even while a last
  // reading is still held from the run that just finished.
  return active ? progress : null
}
