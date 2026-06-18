import {
  Archive,
  Bot,
  Clock,
  Folder,
  FolderOpen,
  FolderPlus,
  MessageSquarePlus,
  Plus,
  Search,
  Settings,
  Sparkles,
  X,
} from "lucide-solid";
import { For, Show } from "solid-js";
import type { SessionRecord } from "../../types";
import i18n from "../../i18n";

export function ProjectSidebar(props: {
  projects: { id: string; name: string; path: string }[];
  sessions: SessionRecord[];
  selectedProjectId: string | null;
  selectedSessionId: string | null;
  busy: boolean;
  onAddProject: (path: string) => void;
  onChooseProjectDirectory: () => void;
  onSelectProject: (id: string) => void;
  onArchiveProject: (id: string) => void;
  onNewSession: () => void;
  onSelectSession: (id: string) => void;
  onDeleteSession: (id: string) => void;
  onOpenSettings: () => void;
}) {
  let pathInput: HTMLInputElement | undefined;
  const openTypedProject = () => {
    const value = pathInput?.value.trim();
    if (!value) return;
    props.onAddProject(value);
    if (pathInput) pathInput.value = "";
  };
  const confirmArchiveProject = (project: { id: string; name: string }) => {
    if (!window.confirm(i18n.t("projects.confirmArchive", { name: project.name }))) return;
    props.onArchiveProject(project.id);
  };
  const confirmDeleteSession = (session: SessionRecord) => {
    if (!window.confirm(i18n.t("sessions.confirmDelete", { title: session.title }))) return;
    props.onDeleteSession(session.id);
  };

  return (
    <aside class="studio-sidebar">
      <div class="sidebar-primary-nav" aria-label="Pure Studio">
        <div class="brand">
          <Sparkles size={16} />
          <span>{i18n.t("brand.title")}</span>
        </div>
        <button type="button" onClick={props.onNewSession} disabled={props.busy || !props.selectedProjectId}>
          <MessageSquarePlus size={16} />
          <span>{i18n.t("common.newSession")}</span>
        </button>
        <button type="button">
          <Search size={16} />
          <span>{i18n.t("nav.search")}</span>
        </button>
        <button type="button" data-active="true">
          <Sparkles size={16} />
          <span>{i18n.t("nav.plugins")}</span>
        </button>
        <button type="button">
          <Clock size={16} />
          <span>{i18n.t("nav.automation")}</span>
        </button>
      </div>

      <div class="project-add">
        <input
          ref={pathInput}
          placeholder={i18n.t("projects.addPlaceholder")}
          onKeyDown={(event) => {
            if (event.key === "Enter") {
              event.preventDefault();
              openTypedProject();
            }
          }}
        />
        <button type="button" onClick={props.onChooseProjectDirectory} title={i18n.t("common.chooseFolder")}>
          <FolderOpen size={14} />
        </button>
        <button type="button" onClick={openTypedProject} title={i18n.t("projects.openTypedPath")}>
          <FolderPlus size={14} />
        </button>
      </div>

      <div class="sidebar-section">
        <div class="section-title">{i18n.t("nav.projects")}</div>
        <Show when={props.projects.length > 0} fallback={<div class="sidebar-empty">{i18n.t("projects.empty")}</div>}>
          <For each={props.projects}>
            {(project) => (
              <div class="sidebar-row" data-active={project.id === props.selectedProjectId || undefined}>
                <button
                  type="button"
                  class="sidebar-item row-main"
                  onClick={() => props.onSelectProject(project.id)}
                  title={project.path}
                >
                  <Folder size={15} class="sidebar-item-icon" />
                  <span>{project.name}</span>
                  <span class="sidebar-health-dot" aria-hidden="true" />
                </button>
                <button
                  type="button"
                  class="sidebar-row-action"
                  disabled={props.busy && project.id === props.selectedProjectId}
                  onClick={() => confirmArchiveProject(project)}
                  title={i18n.t("projects.archive")}
                >
                  <Archive size={13} />
                </button>
              </div>
            )}
          </For>
        </Show>
      </div>

      <div class="sidebar-section grow">
        <div class="section-title with-action">
          <span>{i18n.t("nav.sessions")}</span>
          <button type="button" onClick={props.onNewSession} disabled={props.busy || !props.selectedProjectId} title={i18n.t("common.newSession")}><Plus size={14} /></button>
        </div>
        <div class="session-list">
          <Show when={props.sessions.length > 0} fallback={<div class="sidebar-empty">{props.selectedProjectId ? i18n.t("sessions.empty") : i18n.t("projects.openHint")}</div>}>
            <For each={props.sessions}>
              {(session) => (
                <div class="sidebar-row session" data-active={session.id === props.selectedSessionId || undefined}>
                  <button
                    type="button"
                    class="sidebar-item session row-main"
                    onClick={() => props.onSelectSession(session.id)}
                  >
                    <Bot size={14} class="sidebar-item-icon" />
                    <span>{session.title}</span>
                    <small><Clock size={11} />{session.mode}</small>
                  </button>
                  <button
                    type="button"
                    class="sidebar-row-action"
                    disabled={props.busy && session.id === props.selectedSessionId}
                    onClick={() => confirmDeleteSession(session)}
                    title={i18n.t("sessions.close")}
                  >
                    <X size={13} />
                  </button>
                </div>
              )}
            </For>
          </Show>
        </div>
      </div>

      <button type="button" class="sidebar-settings" onClick={props.onOpenSettings}>
        <Settings size={15} />
        <span>{i18n.t("nav.settings")}</span>
      </button>
    </aside>
  );
}
