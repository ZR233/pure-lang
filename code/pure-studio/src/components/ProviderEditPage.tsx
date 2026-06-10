import { ArrowLeft, CheckCircle2, Link2, Save, X } from "lucide-react";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import type { ModelRecord, ProviderKind, ProviderRecord, ProviderTemplateRecord } from "../types";
import { allModels, providerStatusClass, translateStatus } from "../lib/utils";
import { ProviderModelEditor } from "./ProviderModelEditor";

type ProviderEditPageProps = {
  mode: "create" | "edit";
  provider: ProviderRecord;
  templates: ProviderTemplateRecord[];
  isSaving: boolean;
  onCancel: () => void;
  onSave: () => void;
  onChangeTemplate: (kind: ProviderKind) => void;
  onUpdateProvider: (updater: (provider: ProviderRecord) => ProviderRecord) => void;
  onAddCustomModel: () => void;
  onUpdateCustomModel: (index: number, patch: Partial<ModelRecord>) => void;
  onRemoveCustomModel: (index: number) => void;
};

export function ProviderEditPage({
  mode,
  provider,
  templates,
  isSaving,
  onCancel,
  onSave,
  onChangeTemplate,
  onUpdateProvider,
  onAddCustomModel,
  onUpdateCustomModel,
  onRemoveCustomModel,
}: ProviderEditPageProps) {
  const { t } = useTranslation();
  const models = allModels(provider);

  return (
    <section className="space-y-4">
      <header className="flex items-center justify-between gap-4">
        <div className="flex items-center gap-3">
          <Button variant="ghost" size="icon" disabled={isSaving} onClick={onCancel}>
            <ArrowLeft size={18} />
          </Button>
          <div>
            <div className="flex items-center gap-2">
              <h2 className="text-lg font-semibold text-foreground">
                {mode === "create" ? t("provider.newProvider") : provider.name || provider.id}
              </h2>
              <span className={`inline-flex items-center gap-1 text-xs ${providerStatusClass(provider)}`}>
                <CheckCircle2 size={14} />
                {translateStatus(provider.status, t)}
              </span>
            </div>
            <p className="text-sm text-muted-foreground flex items-center gap-1">
              <Link2 size={14} />
              {provider.baseUrl || t("provider.defaultBaseUrl")}
            </p>
          </div>
        </div>
        <div className="flex items-center gap-2">
          <Button variant="outline" disabled={isSaving} onClick={onCancel}>
            <X size={16} className="mr-1" />
            {t("actions.cancel")}
          </Button>
          <Button disabled={isSaving} onClick={onSave}>
            <Save size={16} className="mr-1" />
            {t("actions.save")}
          </Button>
        </div>
      </header>

      <div className="overflow-auto">
        <Card className="p-6 space-y-4">
          <div className="grid grid-cols-2 gap-4">
            <div className="space-y-2">
              <Label htmlFor="provider-key">{t("provider.providerKey")}</Label>
              <Input
                id="provider-key"
                disabled={isSaving}
                value={provider.id}
                onChange={(event) =>
                  onUpdateProvider((current) => ({
                    ...current,
                    id: event.target.value,
                  }))
                }
              />
            </div>
            <div className="space-y-2">
              <Label htmlFor="provider-type">{t("provider.providerType")}</Label>
              <Select
                disabled={isSaving}
                value={provider.templateKind}
                onValueChange={(value) => onChangeTemplate(value as ProviderKind)}
              >
                <SelectTrigger id="provider-type">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  {templates.map((template) => (
                    <SelectItem key={template.id} value={template.id}>
                      {template.name}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>
            <div className="space-y-2">
              <Label htmlFor="provider-name">{t("provider.displayName")}</Label>
              <Input
                id="provider-name"
                disabled={isSaving}
                value={provider.name}
                onChange={(event) =>
                  onUpdateProvider((current) => ({
                    ...current,
                    name: event.target.value,
                  }))
                }
              />
            </div>
            <div className="space-y-2">
              <Label>{t("provider.protocolType")}</Label>
              <div className="flex h-10 items-center px-3 rounded-md border border-border bg-muted text-sm text-muted-foreground">
                {provider.providerKind}
              </div>
            </div>
          </div>

          <div className="space-y-2">
            <Label htmlFor="provider-base-url">{t("provider.baseUrl")}</Label>
            <Input
              id="provider-base-url"
              disabled={isSaving}
              value={provider.baseUrl}
              onChange={(event) =>
                onUpdateProvider((current) => ({
                  ...current,
                  baseUrl: event.target.value,
                }))
              }
            />
          </div>

          <div className="space-y-2">
            <Label htmlFor="provider-api-key">{t("provider.apiKey")}</Label>
            <Input
              id="provider-api-key"
              disabled={isSaving}
              type="password"
              value={provider.bearerToken}
              onChange={(event) =>
                onUpdateProvider((current) => ({
                  ...current,
                  bearerToken: event.target.value,
                }))
              }
            />
          </div>

          <div className="space-y-2">
            <Label htmlFor="provider-default-model">{t("provider.defaultModel")}</Label>
            <Select
              disabled={isSaving}
              value={provider.defaultModel}
              onValueChange={(value) =>
                onUpdateProvider((current) => ({
                  ...current,
                  defaultModel: value,
                }))
              }
            >
              <SelectTrigger id="provider-default-model">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {models.map((model) => (
                  <SelectItem key={model.slug} value={model.slug}>
                    {model.displayName} ({model.slug})
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>
        </Card>

        <ProviderModelEditor
          provider={provider}
          disabled={isSaving}
          onAddCustomModel={onAddCustomModel}
          onUpdateCustomModel={onUpdateCustomModel}
          onRemoveCustomModel={onRemoveCustomModel}
        />
      </div>
    </section>
  );
}
