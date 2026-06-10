import { useTranslation } from "react-i18next";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Separator } from "@/components/ui/separator";
import type { ProjectRecord, ProviderRecord, SessionRecord } from "../types";

type RecentActivity = {
  id: string;
  title: string;
  detail: string;
};

type ContextPanelProps = {
  selectedProject: ProjectRecord | null;
  sessions: SessionRecord[];
  timelineCount: number;
  providers: ProviderRecord[];
  recentActivities: RecentActivity[];
};

export function ContextPanel({
  selectedProject,
  sessions,
  timelineCount,
  providers,
  recentActivities,
}: ContextPanelProps) {
  const { t } = useTranslation();

  return (
    <aside className="w-72 shrink-0 border-l border-border bg-muted/30 p-4 flex flex-col gap-4">
      <Card>
        <CardHeader className="pb-2">
          <CardTitle className="text-sm">{t("context.project")}</CardTitle>
        </CardHeader>
        <CardContent>
          <p className="text-sm font-semibold">{selectedProject?.name ?? t("context.noProject")}</p>
          <p className="text-xs text-muted-foreground mt-0.5">{selectedProject?.path ?? t("context.chooseFolder")}</p>
        </CardContent>
      </Card>

      <Card>
        <CardHeader className="pb-2">
          <CardTitle className="text-sm">{t("context.runtime")}</CardTitle>
        </CardHeader>
        <CardContent className="p-0">
          <div className="flex justify-between text-sm py-2 px-6 border-b border-border/50 last:border-0">
            <span className="text-muted-foreground">{t("context.sessions")}</span>
            <span className="font-semibold">{sessions.length}</span>
          </div>
          <div className="flex justify-between text-sm py-2 px-6 border-b border-border/50 last:border-0">
            <span className="text-muted-foreground">{t("context.messages")}</span>
            <span className="font-semibold">{timelineCount}</span>
          </div>
          <div className="flex justify-between text-sm py-2 px-6 border-b border-border/50 last:border-0">
            <span className="text-muted-foreground">{t("context.providers")}</span>
            <span className="font-semibold">{providers.length}</span>
          </div>
        </CardContent>
      </Card>

      <Card>
        <CardHeader className="pb-2">
          <CardTitle className="text-sm">{t("context.tools")}</CardTitle>
        </CardHeader>
        <CardContent>
          {recentActivities.length === 0 ? (
            <p className="text-xs text-muted-foreground">{t("context.noActivity")}</p>
          ) : (
            <div className="flex flex-col gap-2">
              {recentActivities.map((item) => (
                <div key={item.id}>
                  <div className="flex justify-between text-sm">
                    <span className="font-semibold">{item.title}</span>
                    <span className="text-xs text-muted-foreground">{item.detail}</span>
                  </div>
                  <Separator className="mt-1.5" />
                </div>
              ))}
            </div>
          )}
        </CardContent>
      </Card>
    </aside>
  );
}
