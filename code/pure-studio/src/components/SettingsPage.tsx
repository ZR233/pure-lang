import { ArrowLeft } from "lucide-react";
import type { Dispatch, SetStateAction } from "react";
import { useTranslation } from "react-i18next";
import type { SettingsTab } from "../state/studio-state";
import type {
  ProviderRecord,
  ProviderSettingsSaveSnapshot,
  ProviderTemplateRecord,
  ProviderUsageRecord,
  McpServerInput,
  McpServerRecord,
  PermissionMode,
  RoleRecord,
} from "../types";
import { Button } from "@/components/ui/button";
import { Tabs, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { McpSettings } from "./McpSettings";
import { ProviderSettings } from "./ProviderSettings";
import { RoleSettings } from "./RoleSettings";
import { SecuritySettings } from "./SecuritySettings";
import { SkillsSettings } from "./SkillsSettings";

type SettingsPageProps = {
  activeSettingsTab: SettingsTab;
  providers: ProviderRecord[];
  mcpServers: McpServerRecord[];
  providerTemplates: ProviderTemplateRecord[];
  providerUsages: ProviderUsageRecord[];
  providerUsagesLoading: boolean;
  providerUsageError: string | null;
  roles: RoleRecord[];
  selectedProjectId: string | null;
  selectedProviderId: string | null;
  providerSearch: string;
  configExists: boolean;
  permissionMode: PermissionMode;
  setRoles: Dispatch<SetStateAction<RoleRecord[]>>;
  setProviderSearch: Dispatch<SetStateAction<string>>;
  onClose: () => void;
  onSetActiveTab: (tab: SettingsTab) => void;
  onSaveProviderSettings: (snapshot?: ProviderSettingsSaveSnapshot) => Promise<boolean>;
  onSavePermissionMode: (mode: PermissionMode) => Promise<void>;
  onSaveMcpSettings: (servers: McpServerInput[]) => Promise<boolean>;
  onRefreshProviderUsages: () => void;
};

const SETTINGS_TABS: SettingsTab[] = ["providers", "skills", "roles", "mcp", "security", "general"];

export function SettingsPage({
  activeSettingsTab,
  providers,
  mcpServers,
  providerTemplates,
  providerUsages,
  providerUsagesLoading,
  providerUsageError,
  roles,
  selectedProjectId,
  selectedProviderId,
  providerSearch,
  configExists,
  permissionMode,
  onClose,
  onSetActiveTab,
  onSaveProviderSettings,
  setRoles,
  setProviderSearch,
  onSavePermissionMode,
  onSaveMcpSettings,
  onRefreshProviderUsages,
}: SettingsPageProps) {
  const { t } = useTranslation();

  return (
    <div className="fixed inset-0 z-100 grid grid-rows-[auto_auto_1fr] bg-background">
      <header className="flex items-center justify-between gap-4 px-6 py-4 border-b border-border bg-card">
        <div className="flex items-center gap-3">
          <Button variant="ghost" size="icon" onClick={onClose}>
            <ArrowLeft size={18} />
          </Button>
          <div>
            <h1 className="text-lg font-semibold text-foreground">{t("settings.title")}</h1>
            <p className="text-sm text-muted-foreground">
              {configExists ? "~/.pure/config.toml" : t("settings.defaultConfigDraft")}
            </p>
          </div>
        </div>
      </header>

      <Tabs
        value={activeSettingsTab}
        onValueChange={(value) => onSetActiveTab(value as SettingsTab)}
        className="w-[min(760px,calc(100vw-52px))] mx-6 mt-3.5"
      >
        <TabsList className="w-full justify-start">
          {SETTINGS_TABS.map((tab) => (
            <TabsTrigger key={tab} value={tab}>
              {t(`settings.tabs.${tab}`)}
            </TabsTrigger>
          ))}
        </TabsList>
      </Tabs>

      <div className="overflow-auto px-6 py-4">
        {activeSettingsTab === "providers" ? (
          <ProviderSettings
            providers={providers}
            templates={providerTemplates}
            providerUsages={providerUsages}
            providerUsagesLoading={providerUsagesLoading}
            providerUsageError={providerUsageError}
            selectedProviderId={selectedProviderId}
            providerSearch={providerSearch}
            setProviderSearch={setProviderSearch}
            onSaveProviderSettings={onSaveProviderSettings}
            onRefreshProviderUsages={onRefreshProviderUsages}
          />
        ) : activeSettingsTab === "roles" ? (
          <RoleSettings
            providers={providers}
            roles={roles}
            setRoles={setRoles}
            onSaveRoles={(nextRoles) => onSaveProviderSettings({ roles: nextRoles })}
          />
        ) : activeSettingsTab === "skills" ? (
          <SkillsSettings selectedProjectId={selectedProjectId} />
        ) : activeSettingsTab === "mcp" ? (
          <McpSettings servers={mcpServers} onSaveMcpSettings={onSaveMcpSettings} />
        ) : activeSettingsTab === "security" ? (
          <SecuritySettings
            permissionMode={permissionMode}
            onSavePermissionMode={onSavePermissionMode}
          />
        ) : (
          <div className="flex flex-col items-center justify-center gap-2 py-16 text-center">
            <h2 className="text-lg font-semibold text-foreground">{t("settings.comingSoon")}</h2>
            <p className="text-sm text-muted-foreground">{t("settings.comingSoonDesc")}</p>
          </div>
        )}
      </div>
    </div>
  );
}
