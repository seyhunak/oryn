import { Activity, Bot, Cable, FlaskConical, FolderKanban, Gauge, LayoutDashboard, Server, Settings } from "lucide-react";

export const NAV = [
  { href: "/overview", label: "Overview", icon: LayoutDashboard },
  { href: "/projects", label: "Projects", icon: FolderKanban },
  { href: "/studio", label: "Test Studio", icon: FlaskConical },
  { href: "/planner", label: "AI Test Planner", icon: Bot },
  { href: "/executions", label: "Executions", icon: Activity },
  { href: "/quality", label: "Quality Intelligence", icon: Gauge },
  { href: "/workers", label: "Workers", icon: Server },
  { href: "/integrations", label: "Integrations", icon: Cable },
  { href: "/settings", label: "Settings", icon: Settings },
] as const;
