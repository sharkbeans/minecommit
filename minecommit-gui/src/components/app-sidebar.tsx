import { NavLink, useNavigate } from "react-router-dom"
import {
  Sidebar,
  SidebarContent,
  SidebarFooter,
  SidebarGroup,
  SidebarHeader,
  SidebarMenu,
  SidebarMenuButton,
  SidebarMenuItem,
} from "@/components/ui/sidebar"
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuGroup,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuRadioGroup,
  DropdownMenuRadioItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu"
import {
  Archive,
  ChevronDown,
  HardDrive,
  // History,
  House,
  // LayoutDashboard,
  Settings,
} from "lucide-react"
import { useSaves } from "@/contexts/saves"
import { useI18n } from "@/contexts/i18n"

export function AppSidebar() {
  const navigate = useNavigate()
  const { saves, selectedSave, setSelectedSave } = useSaves()
  const { t } = useI18n()
  const navItems = [{ to: "/", label: t("sidebar.home"), icon: House }]

  return (
    <Sidebar collapsible="icon">
      <SidebarHeader>
        <SidebarMenu>
          <SidebarMenuItem>
            <DropdownMenu>
              <DropdownMenuTrigger
                render={
                  <SidebarMenuButton>
                    <HardDrive />
                    {selectedSave ? selectedSave.name : t("sidebar.chooseWorld")}
                    <ChevronDown className="ml-auto" />
                  </SidebarMenuButton>
                }
              />
              <DropdownMenuContent className="w-auto" align="start">
                <DropdownMenuGroup>
                  <DropdownMenuItem onClick={() => navigate("/save-manage")}>
                    <Archive />
                    {t("sidebar.manageWorlds")}
                  </DropdownMenuItem>
                </DropdownMenuGroup>
                <DropdownMenuSeparator />
                <DropdownMenuGroup>
                  <DropdownMenuLabel>{t("sidebar.recentWorlds")}</DropdownMenuLabel>
                  {saves.length === 0 ? (
                    <DropdownMenuItem disabled>{t("sidebar.noWorlds")}</DropdownMenuItem>
                  ) : (
                    <DropdownMenuRadioGroup
                      value={selectedSave?.name ?? ""}
                      onValueChange={(value) => {
                        const save = saves.find((s) => s.name === value)
                        if (save) setSelectedSave(save)
                      }}
                    >
                      {[...saves]
                        .sort((a, b) => b.last_access.localeCompare(a.last_access))
                        .map((save) => (
                        <DropdownMenuRadioItem key={save.name} value={save.name}>
                          {save.name}
                        </DropdownMenuRadioItem>
                      ))}
                    </DropdownMenuRadioGroup>
                  )}
                </DropdownMenuGroup>
              </DropdownMenuContent>
            </DropdownMenu>
          </SidebarMenuItem>
        </SidebarMenu>
      </SidebarHeader>
      <SidebarContent>
        <SidebarGroup>
          <SidebarMenu>
            {navItems.map((item) => (
              <SidebarMenuItem key={item.to}>
                <SidebarMenuButton render={<NavLink to={item.to} end />}>
                  <item.icon />
                  {item.label}
                </SidebarMenuButton>
              </SidebarMenuItem>
            ))}
          </SidebarMenu>
        </SidebarGroup>
      </SidebarContent>
      <SidebarFooter>
        <SidebarMenu>
          <SidebarMenuItem>
            <SidebarMenuButton render={<NavLink to="/settings" />}>
              <Settings />
              {t("sidebar.settings")}
            </SidebarMenuButton>
          </SidebarMenuItem>
        </SidebarMenu>
      </SidebarFooter>
    </Sidebar>
  )
}
