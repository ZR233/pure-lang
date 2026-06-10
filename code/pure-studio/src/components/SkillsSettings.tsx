import { AlertTriangle, BookOpen, RefreshCw, Search } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Badge } from "@/components/ui/badge";
import { listDiscoveredSkills } from "../lib/tauri";
import { errorText } from "../lib/utils";
import type { DiscoveredSkillsPayload, SkillRecord } from "../types";

type SkillsSettingsProps = {
  selectedProjectId: string | null;
};

function searchableSkillText(skill: SkillRecord) {
  return [
    skill.name,
    skill.description,
    skill.category ?? "",
    skill.scope,
    skill.path,
    ...skill.platforms,
  ]
    .join(" ")
    .toLowerCase();
}

export function SkillsSettings({ selectedProjectId }: SkillsSettingsProps) {
  const { t } = useTranslation();
  const [payload, setPayload] = useState<DiscoveredSkillsPayload | null>(null);
  const [search, setSearch] = useState("");
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [reloadKey, setReloadKey] = useState(0);

  useEffect(() => {
    if (!selectedProjectId) {
      setPayload(null);
      setError(null);
      setLoading(false);
      return;
    }
    let cancelled = false;
    setLoading(true);
    setError(null);
    listDiscoveredSkills(selectedProjectId)
      .then((nextPayload) => {
        if (!cancelled) {
          setPayload(nextPayload);
        }
      })
      .catch((loadError) => {
        if (!cancelled) {
          setError(errorText(loadError));
        }
      })
      .finally(() => {
        if (!cancelled) {
          setLoading(false);
        }
      });
    return () => {
      cancelled = true;
    };
  }, [selectedProjectId, reloadKey]);

  const filteredSkills = useMemo(() => {
    const query = search.trim().toLowerCase();
    const skills = payload?.skills ?? [];
    if (!query) {
      return skills;
    }
    return skills.filter((skill) => searchableSkillText(skill).includes(query));
  }, [payload?.skills, search]);

  return (
    <section className="space-y-4">
      <div className="flex items-start justify-between gap-4">
        <div>
          <h2 className="text-lg font-semibold text-foreground">{t("skills.title")}</h2>
          <p className="text-sm text-muted-foreground">{t("skills.subtitle")}</p>
        </div>
        <div className="flex items-center gap-2">
          <div className="relative">
            <Search className="absolute left-2.5 top-2.5 h-4 w-4 text-muted-foreground" />
            <Input
              className="pl-9"
              value={search}
              onChange={(event) => setSearch(event.target.value)}
              placeholder={t("skills.searchPlaceholder")}
            />
          </div>
          <Button
            variant="outline"
            size="sm"
            onClick={() => setReloadKey((current) => current + 1)}
            disabled={!selectedProjectId || loading}
          >
            <RefreshCw size={16} className="mr-1" />
            {t("actions.reload")}
          </Button>
        </div>
      </div>

      <div className="flex items-center gap-3 text-sm text-muted-foreground">
        <span>{t("skills.count", { count: payload?.skills.length ?? 0 })}</span>
        {payload?.projectDir ? <code className="text-xs">{payload.projectDir}</code> : null}
      </div>

      {payload?.warnings.length ? (
        <div className="flex items-center gap-2 rounded-lg border border-border bg-muted/50 p-3 text-sm text-muted-foreground">
          <AlertTriangle size={16} className="shrink-0 text-amber-500" />
          <span>{payload.warnings.slice(0, 3).join(" · ")}</span>
        </div>
      ) : null}

      {!selectedProjectId ? (
        <div className="flex flex-col items-center gap-2 py-16 text-center">
          <BookOpen size={28} className="text-muted-foreground" />
          <strong className="text-sm text-muted-foreground">{t("skills.noProject")}</strong>
        </div>
      ) : loading ? (
        <div className="flex flex-col items-center gap-2 py-16 text-center">
          <BookOpen size={28} className="text-muted-foreground" />
          <strong className="text-sm text-muted-foreground">{t("skills.loading")}</strong>
        </div>
      ) : error ? (
        <div className="flex flex-col items-center gap-2 py-16 text-center">
          <AlertTriangle size={28} className="text-destructive" />
          <strong className="text-sm text-destructive">{t("skills.loadFailed")}</strong>
          <span className="text-sm text-muted-foreground">{error}</span>
        </div>
      ) : filteredSkills.length === 0 ? (
        <div className="flex flex-col items-center gap-2 py-16 text-center">
          <BookOpen size={28} className="text-muted-foreground" />
          <strong className="text-sm text-muted-foreground">
            {search.trim() ? t("skills.noMatches") : t("skills.empty")}
          </strong>
        </div>
      ) : (
        <div className="grid gap-2">
          {filteredSkills.map((skill) => (
            <article
              className="p-3 rounded-lg border border-border hover:bg-muted/30 transition-colors"
              key={`${skill.scope}-${skill.name}-${skill.path}`}
            >
              <div className="flex items-start justify-between gap-4">
                <div className="min-w-0">
                  <strong className="text-sm text-foreground">{skill.name}</strong>
                  <p className="text-xs text-muted-foreground mt-0.5">{skill.description}</p>
                </div>
                <Badge variant="secondary" className="shrink-0">
                  {t(`skills.scope.${skill.scope}`)}
                </Badge>
              </div>
              <div className="flex items-center gap-2 mt-2">
                <Badge variant="outline" className="text-xs">
                  {skill.category ?? t("skills.uncategorized")}
                </Badge>
                <Badge variant="outline" className="text-xs">
                  {skill.platforms.length > 0
                    ? skill.platforms.join(", ")
                    : t("skills.allPlatforms")}
                </Badge>
              </div>
              <code className="mt-2 block text-xs text-muted-foreground">{skill.path}</code>
            </article>
          ))}
        </div>
      )}
    </section>
  );
}
