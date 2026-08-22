import { StrictMode } from "react"
import { createRoot } from "react-dom/client"

import "./index.css"
import { App } from "./App.tsx"
import { ThemeProvider } from "@/components/theme-provider.tsx"
import { TooltipProvider } from "@/components/ui/tooltip.tsx"
import { SavesProvider } from "@/contexts/saves-context.tsx"
import { CommitAuthorProvider } from "@/contexts/commit-author-context.tsx"
import { I18nProvider } from "@/contexts/i18n.tsx"

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <ThemeProvider>
      <TooltipProvider>
        <I18nProvider>
          <SavesProvider>
            <CommitAuthorProvider>
              <App />
            </CommitAuthorProvider>
          </SavesProvider>
        </I18nProvider>
      </TooltipProvider>
    </ThemeProvider>
  </StrictMode>
)
