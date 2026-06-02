import { AlertTriangle, BookOpen, RefreshCw, Search } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { listDiscoveredSkills } from "../lib/tauri";
import { errorText } from "../lib/utils";
import type { DiscoveredSkillsPayload, SkillRecord, SkillScope } from "../types";

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

function scopeClass(scope: SkillScope) {
  return `skill-scope skill-scope-${scope}`;
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
    <section className="skills-settings">
      <div className="skills-console-head">
        <div>
          <h2>{t("skills.title")}</h2>
          <p>{t("skills.subtitle")}</p>
        </div>
        <div className="skills-console-tools">
          <label className="search-box">
            <Search size={16} />
            <input
              value={search}
              onChange={(event) => setSearch(event.target.value)}
              placeholder={t("skills.searchPlaceholder")}
            />
          </label>
          <button
            onClick={() => setReloadKey((current) => current + 1)}
            disabled={!selectedProjectId || loading}
          >
            <RefreshCw size={16} />
            {t("actions.reload")}
          </button>
        </div>
      </div>

      <div className="skills-meta-row">
        <span>{t("skills.count", { count: payload?.skills.length ?? 0 })}</span>
        {payload?.projectDir ? <code>{payload.projectDir}</code> : null}
      </div>

      {payload?.warnings.length ? (
        <div className="skills-warning">
          <AlertTriangle size={16} />
          <span>{payload.warnings.slice(0, 3).join(" · ")}</span>
        </div>
      ) : null}

      {!selectedProjectId ? (
        <div className="skills-empty-state">
          <BookOpen size={28} />
          <strong>{t("skills.noProject")}</strong>
        </div>
      ) : loading ? (
        <div className="skills-empty-state">
          <BookOpen size={28} />
          <strong>{t("skills.loading")}</strong>
        </div>
      ) : error ? (
        <div className="skills-empty-state error">
          <AlertTriangle size={28} />
          <strong>{t("skills.loadFailed")}</strong>
          <span>{error}</span>
        </div>
      ) : filteredSkills.length === 0 ? (
        <div className="skills-empty-state">
          <BookOpen size={28} />
          <strong>{search.trim() ? t("skills.noMatches") : t("skills.empty")}</strong>
        </div>
      ) : (
        <div className="skills-list">
          {filteredSkills.map((skill) => (
            <article className="skill-row" key={`${skill.scope}-${skill.name}-${skill.path}`}>
              <div className="skill-row-main">
                <div>
                  <strong>{skill.name}</strong>
                  <p>{skill.description}</p>
                </div>
                <span className={scopeClass(skill.scope)}>{t(`skills.scope.${skill.scope}`)}</span>
              </div>
              <div className="skill-row-tags">
                <span>{skill.category ?? t("skills.uncategorized")}</span>
                <span>
                  {skill.platforms.length > 0
                    ? skill.platforms.join(", ")
                    : t("skills.allPlatforms")}
                </span>
              </div>
              <code>{skill.path}</code>
            </article>
          ))}
        </div>
      )}
    </section>
  );
}
