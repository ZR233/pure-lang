import { RotateCcw, Save } from "lucide-react";
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Textarea } from "@/components/ui/textarea";
import type { InstructionsInput, InstructionsRecord } from "../types";

type InstructionsSettingsProps = {
  instructions: InstructionsRecord;
  onSaveInstructionsSettings: (input: InstructionsInput) => Promise<boolean>;
};

type Draft = {
  baseOverride: string;
  developer: string;
  user: string;
  projectDocMaxBytes: string;
  fallbackFilenames: string;
};

export function InstructionsSettings({
  instructions,
  onSaveInstructionsSettings,
}: InstructionsSettingsProps) {
  const { t } = useTranslation();
  const [draft, setDraft] = useState<Draft>(() => draftFromInstructions(instructions));
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    setDraft(draftFromInstructions(instructions));
  }, [instructions]);

  async function save() {
    setSaving(true);
    try {
      await onSaveInstructionsSettings(inputFromDraft(draft));
    } finally {
      setSaving(false);
    }
  }

  function update(patch: Partial<Draft>) {
    setDraft((current) => ({ ...current, ...patch }));
  }

  return (
    <section className="space-y-6">
      <div className="flex items-start justify-between gap-4">
        <div>
          <h2 className="text-lg font-semibold text-foreground">
            {t("settings.instructions.title")}
          </h2>
          <p className="text-sm text-muted-foreground">
            {t("settings.instructions.description")}
          </p>
        </div>
        <div className="flex items-center gap-2">
          <Button
            variant="outline"
            size="sm"
            disabled={saving}
            onClick={() => setDraft(draftFromInstructions(instructions))}
          >
            <RotateCcw size={16} className="mr-1" />
            {t("actions.reset")}
          </Button>
          <Button size="sm" disabled={saving} onClick={() => void save()}>
            <Save size={16} className="mr-1" />
            {t("actions.save")}
          </Button>
        </div>
      </div>

      <div className="grid gap-5 max-w-4xl">
        <TextAreaField
          id="instructions-base"
          label={t("settings.instructions.baseOverride")}
          description={t("settings.instructions.baseOverrideDesc")}
          value={draft.baseOverride}
          disabled={saving}
          onChange={(baseOverride) => update({ baseOverride })}
        />
        <TextAreaField
          id="instructions-developer"
          label={t("settings.instructions.developer")}
          description={t("settings.instructions.developerDesc")}
          value={draft.developer}
          disabled={saving}
          onChange={(developer) => update({ developer })}
        />
        <TextAreaField
          id="instructions-user"
          label={t("settings.instructions.user")}
          description={t("settings.instructions.userDesc")}
          value={draft.user}
          disabled={saving}
          onChange={(user) => update({ user })}
        />

        <div className="grid grid-cols-[220px_1fr] gap-4">
          <div className="space-y-2">
            <Label htmlFor="instructions-doc-bytes">
              {t("settings.instructions.projectDocMaxBytes")}
            </Label>
            <Input
              id="instructions-doc-bytes"
              type="number"
              min={0}
              step={1024}
              disabled={saving}
              value={draft.projectDocMaxBytes}
              onChange={(event) => update({ projectDocMaxBytes: event.target.value })}
            />
          </div>
          <div className="space-y-2">
            <Label htmlFor="instructions-fallback">
              {t("settings.instructions.fallbackFilenames")}
            </Label>
            <Input
              id="instructions-fallback"
              disabled={saving}
              value={draft.fallbackFilenames}
              onChange={(event) => update({ fallbackFilenames: event.target.value })}
              placeholder="PURE.md, PROJECT.md"
            />
            <p className="text-xs text-muted-foreground">
              {t("settings.instructions.fallbackFilenamesDesc")}
            </p>
          </div>
        </div>
      </div>
    </section>
  );
}

function TextAreaField({
  id,
  label,
  description,
  value,
  disabled,
  onChange,
}: {
  id: string;
  label: string;
  description: string;
  value: string;
  disabled: boolean;
  onChange: (value: string) => void;
}) {
  return (
    <div className="space-y-2">
      <Label htmlFor={id}>{label}</Label>
      <Textarea
        id={id}
        disabled={disabled}
        value={value}
        onChange={(event) => onChange(event.target.value)}
        className="min-h-28"
      />
      <p className="text-xs text-muted-foreground">{description}</p>
    </div>
  );
}

function draftFromInstructions(instructions: InstructionsRecord): Draft {
  return {
    baseOverride: instructions.baseOverride,
    developer: instructions.developer,
    user: instructions.user,
    projectDocMaxBytes: String(instructions.projectDocMaxBytes),
    fallbackFilenames: instructions.projectDocFallbackFilenames.join(", "),
  };
}

function inputFromDraft(draft: Draft): InstructionsInput {
  const projectDocMaxBytes = Number.parseInt(draft.projectDocMaxBytes, 10);
  return {
    baseOverride: draft.baseOverride,
    developer: draft.developer,
    user: draft.user,
    projectDocMaxBytes: Number.isFinite(projectDocMaxBytes)
      ? Math.max(0, projectDocMaxBytes)
      : 65536,
    projectDocFallbackFilenames: draft.fallbackFilenames
      .split(",")
      .map((name) => name.trim())
      .filter(Boolean),
  };
}
