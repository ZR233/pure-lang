import { FolderOpen, MessageSquare, Plus, Settings } from "lucide-react";
import { useTranslation } from "react-i18next";
import type { ProjectRecord, SessionRecord } from "../types";
import { initials } from "../lib/utils";

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
  onOpenSettings: () => void;
  chooseFolder: () => void;
};

export function ProjectRail({
  projects,
  sessions,
  selectedProjectId,
  selectedSessionId,
  onAddProject,
  onSelectProject,
  onNewSession,
  onSelectSession,
  onOpenSettings,
  chooseFolder,
}: ProjectRailProps) {
  const { t } = useTranslation();

  return (
    <aside className="project-rail">
      <div className="rail-header">
        <div className="brand">
          <div className="brand-mark">P</div>
          <span className="brand-name">Pure Studio</span>
        </div>
        <button className="rail-icon-btn" onClick={onOpenSettings} title={t("nav.settings")}>
          <Settings size={16} />
        </button>
      </div>

      <button
        className="new-session-btn"
        disabled={!selectedProjectId}
        onClick={onNewSession}
      >
        <Plus size={16} />
        {t("common.newSession")}
      </button>

      <section className="rail-section" style={{ flex: "0 0 auto", maxHeight: 180 }}>
        <div className="section-label">
          {t("nav.projects")}
          <button className="section-label-btn" onClick={chooseFolder} title={t("common.chooseFolder")}>
            <FolderOpen size={12} />
          </button>
        </div>
        <div className="project-list">
          {projects.map((project) => (
            <button
              key={project.id}
              className={`project-row ${project.id === selectedProjectId ? "active" : ""}`}
              onClick={() => onSelectProject(project.id)}
            >
              <span className="project-avatar">{initials(project.name) || "P"}</span>
              <div className="project-row-text">
                <strong>{project.name}</strong>
                <small>{project.path}</small>
              </div>
            </button>
          ))}
        </div>
      </section>

      <section className="rail-section sessions-section">
        <div className="section-label">{t("nav.sessions")}</div>
        <div className="session-list">
          {sessions.map((session) => (
            <button
              key={session.id}
              className={`session-row ${session.id === selectedSessionId ? "active" : ""}`}
              onClick={() => onSelectSession(session.id)}
            >
              <span className="session-dot" />
              <span className="session-title">{session.title}</span>
              <span className="session-time">{session.updatedAt}</span>
            </button>
          ))}
        </div>
      </section>
    </aside>
  );
}
