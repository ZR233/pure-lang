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
    <section className="space-y-6">
      <div>
        <h2 className="text-lg font-semibold text-foreground">{t("settings.security.title")}</h2>
        <p className="text-sm text-muted-foreground">{t("settings.security.description")}</p>
      </div>

      <div className="grid grid-cols-3 gap-3" role="radiogroup" aria-label={t("settings.security.title")}>
        {permissionOptions.map(({ mode, icon: Icon }) => {
          const active = mode === permissionMode;
          return (
            <button
              key={mode}
              type="button"
              className={`flex items-start gap-3 p-4 rounded-lg border transition-colors text-left ${
                active
                  ? "border-primary bg-primary/5 text-foreground"
                  : "border-border hover:border-primary/30 text-foreground"
              }`}
              role="radio"
              aria-checked={active}
              onClick={() => {
                if (!active) {
                  void onSavePermissionMode(mode);
                }
              }}
            >
              <span className="mt-0.5">
                <Icon size={18} className="text-muted-foreground" />
              </span>
              <span className="grid gap-1">
                <strong className="text-sm font-medium">{t(`permissionMode.${mode}`)}</strong>
                <small className="text-xs text-muted-foreground">{t(`settings.security.modeDesc.${mode}`)}</small>
              </span>
            </button>
          );
        })}
      </div>
    </section>
  );
}
