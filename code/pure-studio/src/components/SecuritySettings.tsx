import { Bot, Unlock, UserCheck, type LucideIcon } from "lucide-react";
import { useTranslation } from "react-i18next";
import type { PermissionMode } from "../types";

type SecuritySettingsProps = {
  permissionMode: PermissionMode;
  onSavePermissionMode: (mode: PermissionMode) => Promise<void>;
};

const permissionOptions: Array<{
  mode: PermissionMode;
  icon: LucideIcon;
}> = [
  { mode: "request-approval", icon: UserCheck },
  { mode: "auto-review", icon: Bot },
  { mode: "full-access", icon: Unlock },
];

export function SecuritySettings({
  permissionMode,
  onSavePermissionMode,
}: SecuritySettingsProps) {
  const { t } = useTranslation();

  return (
    <section className="settings-panel security-settings">
      <div className="settings-section-heading">
        <h2>{t("settings.security.title")}</h2>
        <p>{t("settings.security.description")}</p>
      </div>

      <div className="permission-mode-grid" role="radiogroup" aria-label={t("settings.security.title")}>
        {permissionOptions.map(({ mode, icon: Icon }) => {
          const active = mode === permissionMode;
          return (
            <button
              key={mode}
              type="button"
              className={`permission-mode-option${active ? " active" : ""}`}
              role="radio"
              aria-checked={active}
              onClick={() => {
                if (!active) {
                  void onSavePermissionMode(mode);
                }
              }}
            >
              <span className="permission-mode-icon">
                <Icon size={18} />
              </span>
              <span>
                <strong>{t(`permissionMode.${mode}`)}</strong>
                <small>{t(`settings.security.modeDesc.${mode}`)}</small>
              </span>
            </button>
          );
        })}
      </div>
    </section>
  );
}
