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
  manualPath,
  onSetManualPath,
  onAddProject,
  sessionActionsDisabled,
}: ProjectRailProps) {
  const { t } = useTranslation();

  return (
    <aside className="flex flex-col w-60 shrink-0 h-screen bg-sidebar-background text-sidebar-foreground border-r border-sidebar-border overflow-hidden max-[480px]:hidden">
      {/* Header */}
      <div className="flex items-center justify-between px-4 py-3 border-b border-sidebar-border">
        <span className="text-sm font-bold tracking-tight">Pure Studio</span>
        <Button
          variant="ghost"
          size="icon"
          title={t("settings.title")}
          aria-label={t("settings.title")}
          onClick={onOpenSettings}
        >
          <Settings size={15} />
        </Button>
      </div>

      {/* Projects */}
      <section className="flex flex-col gap-0.5 px-2.5 py-1">
        <div className="text-xs font-semibold uppercase tracking-wider text-sidebar-muted-foreground px-3 py-2">
          {t("nav.projects")}
        </div>
        <div className="flex flex-col gap-px">
          {projects.map((project) => {
            const selected = project.id === selectedProjectId;
            return (
              <button
                key={project.id}
                type="button"
                className={`flex items-center gap-2 px-3 py-1.5 rounded-lg text-left text-sm ${selected ? "bg-sidebar-accent text-sidebar-accent-foreground" : "hover:bg-sidebar-accent"}`}
                onClick={() => onSelectProject(project.id)}
              >
                <span
                  className={`w-6 h-6 rounded-full flex items-center justify-center text-[10px] font-bold flex-shrink-0 ${selected ? "bg-primary text-primary-foreground" : "bg-sidebar-accent text-sidebar-muted-foreground"}`}
                >
                  {initials(project.name)}
                </span>
                <span className="flex-1 min-w-0 truncate font-medium">{project.name}</span>
              </button>
            );
          })}
        </div>
        <div className="flex items-center gap-1.5 px-3 py-1.5">
          <button
            type="button"
            className="flex items-center gap-1.5 text-xs text-sidebar-muted-foreground hover:text-sidebar-foreground"
            onClick={onNewSession}
          >
            <Plus size={13} />
            <span>{t("common.newSession")}</span>
          </button>
        </div>
        <div className="flex items-center gap-1.5 px-3 py-1">
          <FolderOpen size={13} className="text-sidebar-muted-foreground flex-shrink-0" />
          <input
            type="text"
            className="flex-1 min-w-0 bg-transparent border-b border-sidebar-border text-xs py-0.5 px-1 outline-none placeholder:text-sidebar-muted-foreground"
            placeholder={t("projects.addPlaceholder")}
            value={manualPath}
            onChange={(e) => onSetManualPath(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter" && manualPath.trim()) {
                onAddProject(manualPath.trim());
              }
            }}
          />
          <button
            type="button"
            className="text-sidebar-muted-foreground hover:text-sidebar-foreground"
            onClick={chooseFolder}
          >
            <FolderOpen size={13} />
          </button>
        </div>
      </section>

      {/* Sessions */}
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
                  className={`group grid w-full grid-cols-[minmax(0,1fr)_2rem] items-center rounded-lg ${selected ? "bg-sidebar-accent text-sidebar-accent-foreground" : "hover:bg-sidebar-accent"}`}
                >
                  <button
                    type="button"
                    className="flex min-w-0 items-center gap-2 px-2 py-1.5 text-left"
                    onClick={() => onSelectSession(session.id)}
                    title={session.title}
                  >
                    <span className={`w-1.5 h-1.5 rounded-full flex-shrink-0 ${selected ? "bg-primary" : "bg-sidebar-muted-foreground/60"}`} />
                    <span className="block min-w-0 flex-1 overflow-hidden text-ellipsis whitespace-nowrap text-sm font-medium">
                      {session.title}
                    </span>
                  </button>
                  <Button
                    variant="ghost"
                    size="icon"
                    className={`h-7 w-8 shrink-0 rounded-md text-sidebar-muted-foreground hover:text-destructive focus-visible:visible group-hover:visible disabled:opacity-30 ${selected ? "" : "invisible"}`}
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
