import type {
  InteractionKind,
  InteractionRequest,
  InteractionResolution,
  UserQuestion,
} from "../../types";

export type UserInputDraft = Record<string, {
  selected?: string[];
  freeText?: string;
}>;

export type ToolApprovalDecision = "approved" | "denied";

export type PlanConfirmationDecision =
  | "implementFreshContext"
  | "continuePlanning"
  | "dismiss";

export type InteractionComposerState = "pending" | "responding" | "error";

export function buildUserInputResolution(questions: UserQuestion[], draft: UserInputDraft): InteractionResolution {
  const answers: Record<string, { answers: string[] }> = {};
  for (const question of questions) {
    const item = draft[question.id] ?? {};
    answers[question.id] = { answers: userQuestionAnswers(question, item) };
  }
  return { type: "userInput", answers };
}

export function buildToolApprovalResolution(
  decision: ToolApprovalDecision,
  reason: string,
): InteractionResolution {
  const trimmed = reason.trim();
  return {
    type: "toolApproval",
    decision,
    reason: trimmed ? trimmed : null,
  };
}

export function buildPlanConfirmationResolution(
  decision: PlanConfirmationDecision,
  content: string,
  reason: string,
): InteractionResolution {
  switch (decision) {
    case "implementFreshContext":
      return {
        type: "planConfirmation",
        decision: "implementFreshContext",
      };
    case "continuePlanning":
      return {
        type: "planConfirmation",
        decision: "continuePlanning",
        content,
        reason: reasonOrDefault(reason, "continue planning"),
      };
    case "dismiss":
      return {
        type: "planConfirmation",
        decision: "dismiss",
        reason: reasonOrDefault(reason, "dismissed"),
      };
  }
}

export function interactionTitle(interaction: InteractionRequest) {
  switch (interaction.kind) {
    case "planConfirmation":
      return "Plan confirmation";
    case "toolApproval":
      return "Tool approval";
    case "userInput":
      return "Input required";
  }
}

export function interactionPhase(kind: InteractionKind) {
  switch (kind) {
    case "toolApproval":
      return "toolApproval";
    case "userInput":
      return "userInput";
    case "planConfirmation":
      return "planConfirmation";
  }
}

export function activeInteractionPhase(interaction: InteractionRequest | null | undefined) {
  if (!interaction || interaction.status !== "pending") return null;
  return interactionPhase(interaction.kind);
}

export function prettyJson(value: unknown) {
  if (typeof value === "string") {
    try {
      return JSON.stringify(JSON.parse(value), null, 2);
    } catch {
      return value;
    }
  }
  return JSON.stringify(value, null, 2);
}

function userQuestionAnswers(question: UserQuestion, draft: UserInputDraft[string]) {
  const answers = [...(draft.selected ?? [])];
  const freeText = draft.freeText?.trim();
  if ((question.isOther || !question.options?.length) && freeText) answers.push(freeText);
  return answers;
}

function reasonOrDefault(reason: string, fallback: string) {
  const trimmed = reason.trim();
  return trimmed ? trimmed : fallback;
}
