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
  manualPath,
  onSetManualPath,
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
      <div className="brand">
        <div className="brand-mark">P</div>
        <div>
          <div className="brand-title">Pure Studio</div>
          <div className="brand-subtitle">{t("brand.subtitle")}</div>
        </div>
      </div>

      <button className="settings-entry" onClick={onOpenSettings}>
        <Settings size={17} />
        <span>{t("nav.settings")}</span>
      </button>

      <section className="rail-section">
        <div className="section-heading">
          <span>{t("nav.projects")}</span>
          <button className="icon-button" onClick={chooseFolder} title={t("common.chooseFolder")}>
            <FolderOpen size={16} />
          </button>
        </div>
        <div className="path-add">
          <input
            value={manualPath}
            onChange={(event) => onSetManualPath(event.target.value)}
            onKeyDown={(event) => {
              if (event.key === "Enter") {
                onAddProject(manualPath);
              }
            }}
            placeholder={t("common.projectPath")}
          />
          <button className="icon-button" onClick={() => onAddProject(manualPath)}>
            <Plus size={16} />
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
              <span>
                <strong>{project.name}</strong>
                <small>{project.path}</small>
              </span>
            </button>
          ))}
        </div>
      </section>

      <section className="rail-section sessions-section">
        <div className="section-heading">
          <span>{t("nav.sessions")}</span>
          <button
            className="icon-button"
            disabled={!selectedProjectId}
            onClick={onNewSession}
            title={t("common.newSession")}
          >
            <Plus size={16} />
          </button>
        </div>
        <div className="session-list">
          {sessions.map((session) => (
            <button
              key={session.id}
              className={`session-row ${session.id === selectedSessionId ? "active" : ""}`}
              onClick={() => onSelectSession(session.id)}
            >
              <MessageSquare size={16} />
              <span>
                <strong>{session.title}</strong>
                <small>{session.updatedAt}</small>
              </span>
            </button>
          ))}
        </div>
      </section>
    </aside>
  );
}
