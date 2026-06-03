import { ArrowLeft } from "lucide-react";
import type { Dispatch, SetStateAction } from "react";
import { useTranslation } from "react-i18next";
import type { SettingsTab } from "../state/studio-state";
import type {
  ProviderRecord,
  ProviderSettingsSaveSnapshot,
  ProviderTemplateRecord,
  RoleRecord,
} from "../types";
import { ProviderSettings } from "./ProviderSettings";
import { RoleSettings } from "./RoleSettings";
import { SkillsSettings } from "./SkillsSettings";

type SettingsPageProps = {
  activeSettingsTab: SettingsTab;
  providers: ProviderRecord[];
  providerTemplates: ProviderTemplateRecord[];
  roles: RoleRecord[];
  selectedProjectId: string | null;
  selectedProviderId: string | null;
  providerSearch: string;
  configExists: boolean;
  setRoles: Dispatch<SetStateAction<RoleRecord[]>>;
  setProviderSearch: Dispatch<SetStateAction<string>>;
  onClose: () => void;
  onSetActiveTab: (tab: SettingsTab) => void;
  onSaveProviderSettings: (snapshot?: ProviderSettingsSaveSnapshot) => Promise<boolean>;
};

const SETTINGS_TABS: SettingsTab[] = ["providers", "skills", "roles", "security", "general"];

export function SettingsPage({
  activeSettingsTab,
  providers,
  providerTemplates,
  roles,
  selectedProjectId,
  selectedProviderId,
  providerSearch,
  configExists,
  onClose,
  onSetActiveTab,
  onSaveProviderSettings,
  setRoles,
  setProviderSearch,
}: SettingsPageProps) {
  const { t } = useTranslation();

  return (
    <section className="settings-page">
      <header className="settings-header">
        <button className="back-button" onClick={onClose}>
          <ArrowLeft size={18} />
        </button>
        <div>
          <h1>{t("settings.title")}</h1>
          <p>{configExists ? "~/.pure/config.toml" : t("settings.defaultConfigDraft")}</p>
        </div>
      </header>

      <nav className="settings-tabs">
        {SETTINGS_TABS.map((tab) => (
          <button
            key={tab}
            className={tab === activeSettingsTab ? "active" : ""}
            onClick={() => onSetActiveTab(tab)}
          >
            {t(`settings.tabs.${tab}`)}
          </button>
        ))}
      </nav>

      {activeSettingsTab === "providers" ? (
        <ProviderSettings
          providers={providers}
          templates={providerTemplates}
          selectedProviderId={selectedProviderId}
          providerSearch={providerSearch}
          setProviderSearch={setProviderSearch}
          onSaveProviderSettings={onSaveProviderSettings}
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
      ) : (
        <div className="settings-placeholder">
          <h2>{t("settings.comingSoon")}</h2>
          <p>{t("settings.comingSoonDesc")}</p>
        </div>
      )}
    </section>
  );
}
