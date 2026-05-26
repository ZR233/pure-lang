import { Boxes, ChevronDown, Cpu, Database, FolderGit2, Settings } from "lucide-react";
import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";
import type { ProjectRecord, SessionRecord, SessionRuntime } from "../types";

type SessionStatusBarProps = {
  runtime: SessionRuntime | null;
  selectedSession: SessionRecord | null;
  selectedProject: ProjectRecord | null;
};

function formatTokenCount(value?: number | null): string {
  const tokens = value ?? 0;
  if (tokens >= 1_000_000) {
    return `${formatNumber(tokens / 1_000_000)}m`;
  }
  if (tokens >= 1_000) {
    return `${formatNumber(tokens / 1_000)}k`;
  }
  return tokens.toString();
}

function formatNumber(value: number): string {
  return Number.isInteger(value) ? value.toFixed(0) : value.toFixed(1);
}

function formatPercent(value?: number | null): string {
  if (value == null) {
    return "0%";
  }
  return `${Math.round(value * 100)}%`;
}

function formatCost(runtime: SessionRuntime | null, fallback: string): string {
  if (!runtime?.currency || runtime.estimatedCost == null) {
    return fallback;
  }
  return `${runtime.currency} ${runtime.estimatedCost.toFixed(2)}`;
}

function formatPrice(value: number | null | undefined, fallback: string): string {
  return value == null ? fallback : value.toFixed(2);
}

function contextPercent(runtime: SessionRuntime | null): number {
  if (!runtime?.contextWindow) {
    return 0;
  }
  return Math.min(100, (runtime.latestContextTokens / runtime.contextWindow) * 100);
}

function StatusPopover({
  className,
  trigger,
  children,
}: {
  className?: string;
  trigger: ReactNode;
  children: ReactNode;
}) {
  return (
    <div className={`status-popover-wrap ${className ?? ""}`} tabIndex={0}>
      {trigger}
      <div className="status-popover">{children}</div>
    </div>
  );
}

function ListPopover({ title, items }: { title: string; items: string[] }) {
  const { t } = useTranslation();

  return (
    <div className="status-list-popover">
      <strong>{title}</strong>
      {items.length === 0 ? (
        <span className="status-empty">{t("statusBar.notConfigured")}</span>
      ) : (
        items.map((item) => (
          <span key={item} className="status-list-row">
            <span className="status-list-icon">{item.slice(0, 1).toUpperCase()}</span>
            {item}
          </span>
        ))
      )}
    </div>
  );
}

export function SessionStatusBar({
  runtime,
  selectedSession,
  selectedProject,
}: SessionStatusBarProps) {
  const { t } = useTranslation();
  const contextLabel = `${formatTokenCount(runtime?.latestContextTokens)} / ${formatTokenCount(runtime?.contextWindow)}`;
  const costLabel = formatCost(runtime, t("statusBar.costNotConfigured"));
  const contextWidth = `${contextPercent(runtime)}%`;
  const skills = runtime?.activeSkills ?? [];
  const mcpServers = runtime?.activeMcpServers ?? [];
  const notConfigured = t("statusBar.notConfigured");

  return (
    <div className="session-status-bar" aria-label={t("statusBar.label")}>
      <div className="status-group status-left">
        <StatusPopover
          trigger={
            <button className="status-item status-model" type="button">
              <Cpu size={14} />
              <span>{t("statusBar.model")}</span>
              <strong>{runtime?.model ?? t("statusBar.noModel")}</strong>
              <ChevronDown size={13} />
            </button>
          }
        >
          <div className="status-price-popover">
            <div className="status-popover-title">
              {t("statusBar.modelPricing")}
              <Settings size={13} />
            </div>
            <span>currency: {runtime?.currency ?? notConfigured}</span>
            <span>inputPricePerMTok: {formatPrice(runtime?.inputPricePerMTok, notConfigured)}</span>
            <span>
              outputPricePerMTok: {formatPrice(runtime?.outputPricePerMTok, notConfigured)}
            </span>
            <span>
              cacheReadPricePerMTok:{" "}
              {formatPrice(runtime?.cacheReadPricePerMTok, notConfigured)}
            </span>
          </div>
        </StatusPopover>

        <div className="status-item status-session">
          <FolderGit2 size={14} />
          <span>{t("statusBar.session")}</span>
          <strong>{selectedSession?.title ?? t("conversation.defaultTitle")}</strong>
        </div>
        <div className="status-item status-workspace">
          <Database size={14} />
          <strong>{selectedProject?.name ?? t("context.noProject")}</strong>
        </div>
      </div>

      <div className="status-group status-center">
        <StatusPopover
          className="context-popover-wrap"
          trigger={
            <button className="status-item status-context" type="button">
              <span>{t("statusBar.context")}</span>
              <strong>{contextLabel}</strong>
              <span className="context-meter" aria-hidden="true">
                <span style={{ width: contextWidth }} />
              </span>
            </button>
          }
        >
          <div className="status-usage-popover">
            <span>
              {t("statusBar.cacheHit")} <strong>{formatPercent(runtime?.cacheHitRate)}</strong>
            </span>
            <span>
              {t("statusBar.input")} <strong>{formatTokenCount(runtime?.promptTokens)}</strong>
            </span>
            <span>
              {t("statusBar.output")} <strong>{formatTokenCount(runtime?.completionTokens)}</strong>
            </span>
            <span>
              {t("statusBar.cacheRead")}{" "}
              <strong>{formatTokenCount(runtime?.cachedPromptTokens)}</strong>
            </span>
            <hr />
            <span>
              {t("statusBar.cost")} <strong>{costLabel}</strong>
            </span>
            <small>{t("statusBar.costHint")}</small>
          </div>
        </StatusPopover>

        <div className="status-item status-cost">
          <span>{t("statusBar.cost")}</span>
          <strong>{costLabel}</strong>
        </div>
      </div>

      <div className="status-group status-right">
        <StatusPopover
          trigger={
            <button className="status-item status-count" type="button">
              <Boxes size={14} />
              <strong>Skills {skills.length}</strong>
              <ChevronDown size={13} />
            </button>
          }
        >
          <ListPopover title="Skills" items={skills} />
        </StatusPopover>
        <StatusPopover
          trigger={
            <button className="status-item status-count" type="button">
              <Database size={14} />
              <strong>MCP {mcpServers.length}</strong>
              <ChevronDown size={13} />
            </button>
          }
        >
          <ListPopover title="MCP" items={mcpServers} />
        </StatusPopover>
      </div>
    </div>
  );
}
