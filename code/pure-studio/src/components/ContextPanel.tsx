import { useTranslation } from "react-i18next";
import type { ChatMessage, ProjectRecord, ProviderRecord, SessionRecord } from "../types";

type RecentActivity = {
  id: string;
  title: string;
  detail: string;
};

type ContextPanelProps = {
  selectedProject: ProjectRecord | null;
  sessions: SessionRecord[];
  messages: ChatMessage[];
  providers: ProviderRecord[];
  recentActivities: RecentActivity[];
};

export function ContextPanel({
  selectedProject,
  sessions,
  messages,
  providers,
  recentActivities,
}: ContextPanelProps) {
  const { t } = useTranslation();

  return (
    <aside className="context-panel">
      <section className="context-card">
        <h2>{t("context.project")}</h2>
        <p className="context-title">{selectedProject?.name ?? t("context.noProject")}</p>
        <p className="muted">{selectedProject?.path ?? t("context.chooseFolder")}</p>
      </section>
      <section className="context-card">
        <h2>{t("context.runtime")}</h2>
        <div className="metric-row">
          <span>{t("context.sessions")}</span>
          <strong>{sessions.length}</strong>
        </div>
        <div className="metric-row">
          <span>{t("context.messages")}</span>
          <strong>{messages.length}</strong>
        </div>
        <div className="metric-row">
          <span>{t("context.providers")}</span>
          <strong>{providers.length}</strong>
        </div>
      </section>
      <section className="context-card">
        <h2>{t("context.tools")}</h2>
        {recentActivities.length === 0 ? (
          <p className="muted">{t("context.noActivity")}</p>
        ) : (
          recentActivities.map((item) => (
            <div className="activity-row" key={item.id}>
              <strong>{item.title}</strong>
              <span>{item.detail}</span>
            </div>
          ))
        )}
      </section>
    </aside>
  );
}
