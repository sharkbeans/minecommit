import {
  HoverCard,
  HoverCardTrigger,
  HoverCardContent,
} from "@/components/ui/hover-card"
import type { Save } from "@/contexts/saves"
import type { ReactElement } from "react"
import { useI18n } from "@/contexts/i18n"

interface SaveHoverCardProps {
  save: Save
  children: ReactElement
}

export function SaveHoverCard({ save, children }: SaveHoverCardProps) {
  const { t } = useI18n()

  return (
    <HoverCard>
      <HoverCardTrigger render={children}></HoverCardTrigger>
      <HoverCardContent align="start" className="w-auto text-xs">
        <div className="flex flex-col gap-2">
          <div>
            <p className="text-muted-foreground">{t("hover.worldName")}</p>
            <p className="break-all">{save.name}</p>
          </div>
          <div>
            <p className="text-muted-foreground">{t("hover.lastOpened")}</p>
            <p className="break-all">{save.last_access}</p>
          </div>
          <div>
            <p className="text-muted-foreground">{t("hover.worldPath")}</p>
            <p className="break-all">{save.path}</p>
          </div>
          <div>
            <p className="text-muted-foreground">{t("hover.repositoryPath")}</p>
            <p className="break-all">{save.repo_path}</p>
          </div>
          <div>
            <p className="text-muted-foreground">{t("hover.cloudRepository")}</p>
            {save.remote_repo_path ? (
              <p className="break-all">{save.remote_repo_path}</p>
            ) : (
              <p className="break-all text-muted-foreground">{t("hover.notConfigured")}</p>
            )}
          </div>
          <div>
            <p className="text-muted-foreground">{t("hover.defaultBranch")}</p>
            <p className="break-all">{save.default_branch || "—"}</p>
          </div>
        </div>
      </HoverCardContent>
    </HoverCard>
  )
}
