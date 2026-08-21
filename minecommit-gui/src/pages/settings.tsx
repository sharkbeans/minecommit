import { useState } from "react"
import { useCommitAuthor } from "@/contexts/commit-author"
import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import { Label } from "@/components/ui/label"
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card"
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
import { localeOptions, useI18n, type Locale } from "@/contexts/i18n"

export function SettingsPage() {
  const { author, loaded, setAuthor } = useCommitAuthor()
  const { locale, setLocale, t } = useI18n()
  const [name, setName] = useState(author.name)
  const [email, setEmail] = useState(author.email)
  const [saving, setSaving] = useState(false)
  const [saved, setSaved] = useState(false)

  const handleSave = async () => {
    setSaving(true)
    try {
      await setAuthor(name, email)
      setSaved(true)
      setTimeout(() => setSaved(false), 2000)
    } catch {
      // ignore
    } finally {
      setSaving(false)
    }
  }

  if (!loaded) {
    return (
      <div className="flex min-h-svh items-center justify-center p-6">
        <p className="text-muted-foreground">{t("common.loading")}</p>
      </div>
    )
  }

  return (
    <div className="mx-auto flex w-full max-w-lg flex-col gap-6 p-6" key={`${author.name}-${author.email}`}>
      <h1 className="text-2xl font-bold">{t("settings.title")}</h1>

      <Card>
        <CardHeader>
          <CardTitle>{t("settings.language")}</CardTitle>
          <CardDescription>{t("settings.languageDescription")}</CardDescription>
        </CardHeader>
        <CardContent>
          <Select
            value={locale}
            onValueChange={(value) => {
              if (value === "en" || value === "zh-CN") setLocale(value as Locale)
            }}
          >
            <SelectTrigger className="w-full">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {localeOptions.map((option) => (
                <SelectItem key={option.value} value={option.value}>
                  {option.label}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle>{t("settings.commitAuthor")}</CardTitle>
          <CardDescription>
            {t("settings.commitAuthorDescription")}
          </CardDescription>
        </CardHeader>
        <CardContent className="flex flex-col gap-4">
          <div className="flex flex-col gap-2">
            <Label htmlFor="author-name">{t("settings.name")}</Label>
            <Input
              id="author-name"
              placeholder={t("settings.namePlaceholder")}
              defaultValue={author.name}
              onChange={(e) => setName(e.target.value)}
            />
          </div>
          <div className="flex flex-col gap-2">
            <Label htmlFor="author-email">{t("settings.email")}</Label>
            <Input
              id="author-email"
              placeholder={t("settings.emailPlaceholder")}
              defaultValue={author.email}
              onChange={(e) => setEmail(e.target.value)}
            />
          </div>
          <div className="flex items-center gap-3">
            <Button onClick={handleSave} disabled={saving}>
              {saving ? t("common.saving") : t("common.save")}
            </Button>
            {saved && (
              <span className="text-sm text-green-600 dark:text-green-400">
                {t("settings.saved")}
              </span>
            )}
          </div>
        </CardContent>
      </Card>
    </div>
  )
}
