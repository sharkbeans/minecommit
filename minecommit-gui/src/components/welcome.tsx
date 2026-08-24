import { HelpCircle, History, Laptop, Lock, Plus } from "lucide-react"

import { Button } from "@/components/ui/button"
import { GrassBlock } from "@/components/block-icon"
import { useI18n, type TranslationKey } from "@/contexts/i18n"

/**
 * The first thing a new player sees.
 *
 * The old empty state said "Add a world to start backing it up", which assumes
 * the reader already knows what MineCommit does with a world and why they would
 * hand one over. Someone who downloaded this because a world got corrupted once
 * needs the promise first and the button second.
 */
export function Welcome({
  onAddWorld,
  onOpenGuide,
}: {
  onAddWorld: () => void
  onOpenGuide: () => void
}) {
  const { t } = useI18n()

  const features: Array<{
    icon: typeof History
    title: TranslationKey
    body: TranslationKey
  }> = [
    { icon: History, title: "welcome.safe.title", body: "welcome.safe.body" },
    { icon: Laptop, title: "welcome.anywhere.title", body: "welcome.anywhere.body" },
    { icon: Lock, title: "welcome.private.title", body: "welcome.private.body" },
  ]

  return (
    <div className="mx-auto flex h-full max-w-2xl flex-col justify-center gap-8 p-8">
      <div className="flex flex-col items-center gap-3 text-center">
        <GrassBlock className="size-12" />
        <h1 className="text-2xl font-semibold text-balance">{t("welcome.title")}</h1>
        <p className="text-sm leading-relaxed text-pretty text-muted-foreground">
          {t("welcome.body")}
        </p>
      </div>

      <ul className="flex flex-col gap-4">
        {features.map((feature) => (
          <li key={feature.title} className="flex items-start gap-3">
            <span className="flex size-9 shrink-0 items-center justify-center rounded-xl bg-muted">
              <feature.icon className="size-4" />
            </span>
            <span className="min-w-0">
              <span className="block text-sm font-medium">{t(feature.title)}</span>
              <span className="block text-sm text-muted-foreground">{t(feature.body)}</span>
            </span>
          </li>
        ))}
      </ul>

      <div className="flex flex-col items-center gap-2">
        <Button size="lg" onClick={onAddWorld}>
          <Plus data-icon="inline-start" />
          {t("welcome.start")}
        </Button>
        <Button variant="ghost" size="sm" onClick={onOpenGuide}>
          <HelpCircle data-icon="inline-start" />
          {t("welcome.how")}
        </Button>
      </div>
    </div>
  )
}
