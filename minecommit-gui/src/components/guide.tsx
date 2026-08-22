import { useState } from "react"
import {
  Clock,
  HardDrive,
  LifeBuoy,
  Laptop,
  Save,
  Shield,
  Upload,
  type LucideIcon,
} from "lucide-react"

import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog"
import { useI18n, type TranslationKey } from "@/contexts/i18n"
import { cn } from "@/lib/utils"

/**
 * The manual.
 *
 * A player who has never used Git has no model of what "backing up" a world
 * even means here -- whether it overwrites, where it goes, what happens if two
 * computers disagree. None of that fits in a tooltip, and all of it decides
 * whether they trust the app with a world they care about. So it lives in one
 * place they can open at any time, written as answers rather than features.
 */

interface Chapter {
  id: string
  icon: LucideIcon
  topic: TranslationKey
  paragraphs: TranslationKey[]
}

const CHAPTERS: Chapter[] = [
  {
    id: "what",
    icon: Save,
    topic: "guide.what.topic",
    paragraphs: ["guide.what.p1", "guide.what.p2", "guide.what.p3", "guide.what.p4"],
  },
  {
    id: "backup",
    icon: Upload,
    topic: "guide.backup.topic",
    paragraphs: [
      "guide.backup.p1",
      "guide.backup.p2",
      "guide.backup.p3",
      "guide.backup.p4",
    ],
  },
  {
    id: "twoPcs",
    icon: Laptop,
    topic: "guide.twoPcs.topic",
    paragraphs: [
      "guide.twoPcs.p1",
      "guide.twoPcs.p2",
      "guide.twoPcs.p3",
      "guide.twoPcs.p4",
    ],
  },
  {
    id: "history",
    icon: Clock,
    topic: "guide.history.topic",
    paragraphs: ["guide.history.p1", "guide.history.p2", "guide.history.p3"],
  },
  {
    id: "github",
    icon: Shield,
    topic: "guide.github.topic",
    paragraphs: [
      "guide.github.p1",
      "guide.github.p2",
      "guide.github.p3",
      "guide.github.p4",
    ],
  },
  {
    id: "where",
    icon: HardDrive,
    topic: "guide.where.topic",
    paragraphs: ["guide.where.p1", "guide.where.p2", "guide.where.p3"],
  },
  {
    id: "trouble",
    icon: LifeBuoy,
    topic: "guide.trouble.topic",
    paragraphs: [
      "guide.trouble.p1",
      "guide.trouble.p2",
      "guide.trouble.p3",
      "guide.trouble.p4",
    ],
  },
]

export function GuideDialog({
  open,
  onOpenChange,
  startAt,
}: {
  open: boolean
  onOpenChange: (open: boolean) => void
  /** Which chapter to land on, for links that ask a particular question. */
  startAt?: string
}) {
  const { t } = useI18n()
  const [current, setCurrent] = useState(startAt ?? CHAPTERS[0].id)
  const chapter = CHAPTERS.find((entry) => entry.id === current) ?? CHAPTERS[0]

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="flex max-h-[85svh] flex-col gap-4 sm:max-w-3xl">
        <DialogHeader>
          <DialogTitle>{t("guide.title")}</DialogTitle>
        </DialogHeader>

        <div className="flex min-h-0 flex-1 gap-6">
          <nav className="flex w-52 shrink-0 flex-col gap-0.5 overflow-y-auto">
            {CHAPTERS.map((entry) => (
              <button
                key={entry.id}
                type="button"
                onClick={() => setCurrent(entry.id)}
                className={cn(
                  "flex items-center gap-2 rounded-md px-2 py-1.5 text-left text-sm transition-colors",
                  entry.id === chapter.id
                    ? "bg-muted font-medium"
                    : "text-muted-foreground hover:bg-muted/60 hover:text-foreground"
                )}
              >
                <entry.icon className="size-4 shrink-0" />
                <span className="truncate">{t(entry.topic)}</span>
              </button>
            ))}
          </nav>

          <article className="min-w-0 flex-1 overflow-y-auto pr-1">
            <h3 className="mb-3 flex items-center gap-2 text-base font-medium">
              <chapter.icon className="size-4 shrink-0 text-muted-foreground" />
              {t(chapter.topic)}
            </h3>
            <div className="flex flex-col gap-3">
              {chapter.paragraphs.map((key) => (
                <p key={key} className="text-sm leading-relaxed text-muted-foreground">
                  {t(key)}
                </p>
              ))}
            </div>
          </article>
        </div>
      </DialogContent>
    </Dialog>
  )
}
