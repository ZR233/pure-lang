import { FolderOpen, Plus, Settings, Trash2 } from "lucide-react";
import { useTranslation } from "react-i18next";
import type { ProjectRecord, SessionRecord } from "../types";
import { initials } from "../lib/utils";
import { Button } from "@/components/ui/button";
import { ScrollArea } from "@/components/ui/scroll-area";

type ProjectRailProps = {
  projects: ProjectRecord[];
  sessions: SessionRecord[];
  selectedProjectId: string | null;
  selectedSessionId: string | null;
  manualPath: string;
  onSetManualPath: (value: string) => void;
  onAddProject: (path: string) => void;
  onSelectProject: (id: string) => void;
  onNewSession: () => void;
  onSelectSession: (id: string) => void;
  onDeleteSession: (id: string) => void;
  onOpenSettings: () => void;
  chooseFolder: () => void;
  sessionActionsDisabled?: boolean;
};

export function ProjectRail({
  projects,
  sessions,
  selectedProjectId,
  selectedSessionId,
  onSelectProject,
  onNewSession,
  onSelectSession,
  onDeleteSession,
  onOpenSettings,
  chooseFolder,
  sessionActionsDisabled = false,
}: ProjectRailProps) {
  const { t } = useTranslation();

  return (
    <aside className="flex flex-col w-60 shrink-0 h-screen bg-sidebar-background text-sidebar-foreground border-r border-sidebar-border overflow-hidden max-[480px]:hidden">
      <div className="flex items-center justify-between px-3.5 pt-3.5 pb-2.5 border-b border-sidebar-border">
        <div className="flex items-center gap-2.5">
          <div className="w-7.5 h-7.5 rounded-lg bg-primary text-primary-foreground grid place-items-center font-extrabold text-sm">P</div>
          <span className="text-sm font-bold text-sidebar-foreground tracking-tight">Pure Studio</span>
        </div>
        <Button variant="ghost" size="icon" className="text-sidebar-muted-foreground hover:text-sidebar-foreground hover:bg-sidebar-accent h-7.5 w-7.5 rounded-md" onClick={onOpenSettings} title={t("nav.settings")}>
          <Settings size={16} />
        </Button>
      </div>

      <Button variant="outline" className="mx-2.5 mt-2.5 w-[calc(100%-20px)] gap-1.5" disabled={!selectedProjectId} onClick={onNewSession}>
        <Plus size={16} />
        {t("common.newSession")}
      </Button>

      <section className="flex flex-col gap-0.5 px-2.5 py-1" style={{ flex: "0 0 auto", maxHeight: 180 }}>
        <div className="text-xs font-semibold uppercase tracking-wider text-sidebar-muted-foreground flex items-center justify-between px-3 py-2">
          {t("nav.projects")}
          <Button variant="ghost" size="icon" className="h-5.5 w-5.5 rounded text-sidebar-muted-foreground hover:text-sidebar-foreground hover:bg-sidebar-accent" onClick={chooseFolder} title={t("common.chooseFolder")}>
            <FolderOpen size={12} />
          </Button>
        </div>
        <div className="flex flex-col gap-px overflow-y-auto min-h-0">
          {projects.map((project) => (
            <Button
              key={project.id}
              variant="ghost"
              className={`w-full justify-start gap-2.5 px-2 py-1.5 h-auto text-left rounded-lg ${project.id === selectedProjectId ? "bg-sidebar-accent text-sidebar-accent-foreground" : "hover:bg-sidebar-accent"}`}
              onClick={() => onSelectProject(project.id)}
            >
              <span className="w-7 h-7 rounded-lg bg-primary/10 text-primary grid place-items-center font-bold text-xs flex-shrink-0">{initials(project.name) || "P"}</span>
              <div className="min-w-0 flex-1">
                <strong className="block truncate text-sm font-medium text-sidebar-foreground">{project.name}</strong>
                <small className="block truncate text-[10px] text-sidebar-muted-foreground">{project.path}</small>
              </div>
            </Button>
          ))}
        </div>
      </section>

      <section className="flex flex-col gap-0.5 px-2.5 py-1 flex-1 min-h-0 overflow-hidden">
        <div className="text-xs font-semibold uppercase tracking-wider text-sidebar-muted-foreground px-3 py-2">
          {t("nav.sessions")}
        </div>
        <ScrollArea className="flex-1">
          <div className="flex flex-col gap-px pr-3">
            {sessions.map((session) => {
              const selected = session.id === selectedSessionId;
              return (
                <div
                  key={session.id}
                  className={`group flex w-full items-center rounded-lg ${selected ? "bg-sidebar-accent text-sidebar-accent-foreground" : "hover:bg-sidebar-accent"}`}
                >
                  <button
                    type="button"
                    className="flex min-w-0 flex-1 items-center gap-2 px-2 py-1.5 text-left"
                    onClick={() => onSelectSession(session.id)}
                  >
                    <span className={`w-1.5 h-1.5 rounded-full flex-shrink-0 ${selected ? "bg-primary" : "bg-sidebar-muted-foreground/60"}`} />
                    <span className="flex-1 min-w-0 truncate font-medium">{session.title}</span>
                    <span className="text-[10px] text-sidebar-muted-foreground flex-shrink-0">{session.updatedAt}</span>
                  </button>
                  <Button
                    variant="ghost"
                    size="icon"
                    className={`mr-1 h-7 w-7 shrink-0 rounded-md text-sidebar-muted-foreground transition-opacity hover:text-destructive focus-visible:opacity-100 group-hover:opacity-100 disabled:opacity-30 ${selected ? "opacity-100" : "opacity-0"}`}
                    disabled={sessionActionsDisabled}
                    onClick={(event) => {
                      event.stopPropagation();
                      onDeleteSession(session.id);
                    }}
                    title={t("sessions.delete")}
                    aria-label={t("sessions.delete")}
                  >
                    <Trash2 size={13} />
                  </Button>
                </div>
              );
            })}
          </div>
        </ScrollArea>
      </section>
    </aside>
  );
}
