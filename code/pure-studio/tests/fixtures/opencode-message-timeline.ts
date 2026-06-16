import type {
  InteractionRequest,
  InteractionResolution,
  StudioEventEnvelope,
  StudioMessage,
  StudioMessageProjection,
  StudioPart,
  StudioPartProjection,
} from "../../src/types";

const sessionId = "session-golden";
const turnId = "turn-golden-1";
const userMessageId = `${turnId}:user`;
const assistantMessageId = `${turnId}:assistant`;

function message(
  id: string,
  role: StudioMessage["role"],
  createdAt: number,
  status: StudioMessage["status"] = "streaming",
): StudioMessage {
  return {
    messageId: id,
    sessionId,
    turnId,
    role,
    status,
    createdAt,
    updatedAt: createdAt,
  };
}

function textPart(
  id: string,
  messageId: string,
  order: number,
  text: string,
  textChannel: StudioPart["textChannel"],
  status: StudioPart["status"] = "streaming",
): StudioPart {
  return {
    partId: id,
    messageId,
    sessionId,
    turnId,
    partType: "text",
    order,
    status,
    createdAt: order,
    updatedAt: order,
    textChannel,
    text,
  };
}

function reasoningPart(
  id: string,
  order: number,
  text: string,
  status: StudioPart["status"] = "streaming",
): StudioPart {
  return {
    partId: id,
    messageId: assistantMessageId,
    sessionId,
    turnId,
    partType: "reasoning",
    order,
    status,
    createdAt: order,
    updatedAt: order,
    textChannel: null,
    text,
  };
}

function toolPart(
  id: string,
  order: number,
  status: StudioPart["status"],
  result: string | null = null,
): StudioPart {
  return {
    partId: id,
    messageId: assistantMessageId,
    sessionId,
    turnId,
    partType: "tool",
    order,
    status,
    createdAt: order,
    updatedAt: order,
    textChannel: null,
    text: "",
    tool: {
      toolCallId: id,
      name: "bash",
      arguments: JSON.stringify({ command: "pwd" }),
      result,
      exitCode: result === null ? null : 0,
      timedOut: false,
      workingDirectory: "D:/repo",
      denialReason: null,
    },
  };
}

function event(sequence: number, kind: StudioEventEnvelope["kind"]): StudioEventEnvelope {
  return {
    eventId: `golden-event-${sequence}`,
    sessionId,
    turnId,
    sequence,
    createdAt: sequence,
    kind,
  };
}

const userMessage = message(userMessageId, "user", 1, "completed");
const userTextPart = textPart(`${userMessageId}:text`, userMessageId, 1, "Please inspect the migration.", "user", "completed");
const assistantMessage = message(assistantMessageId, "assistant", 2);

const userInputInteraction: InteractionRequest = {
  interactionId: "interaction-user-input-1",
  kind: "userInput",
  status: "pending",
  scope: {
    sessionId,
    turnId,
  },
  payload: {
    type: "userInput",
    questions: [
      {
        id: "style",
        header: "Style",
        question: "Choose the response style.",
        options: [
          { label: "Concise", description: "Keep the answer short." },
          { label: "Detailed", description: "Include extra context." },
        ],
      },
      {
        id: "notes",
        header: "Notes",
        question: "Add any extra constraints.",
        isOther: true,
        options: [
          { label: "Include risks", description: "Mention possible regressions." },
        ],
      },
      {
        id: "token",
        header: "Token",
        question: "Paste the secret token.",
        isSecret: true,
        options: null,
      },
    ],
  },
  createdAt: 10,
  updatedAt: 10,
};

const userInputExpectedResolution: InteractionResolution = {
  type: "userInput",
  answers: {
    style: { answers: ["Concise"] },
    notes: { answers: ["Include risks", "add rollback plan"] },
    token: { answers: ["sk-test-secret"] },
  },
};

const reasoningStageEmpty = reasoningPart("stage-reasoning", 2, "");
const commentaryStageEmpty = textPart("stage-commentary", assistantMessageId, 3, "", "commentary");
const reasoningStageTerminal = reasoningPart("stage-reasoning", 2, "# Check\nfirst clue\n\nDone.", "completed");
const commentaryStageTerminal = textPart(
  "stage-commentary",
  assistantMessageId,
  3,
  "I will inspect fixtures.",
  "commentary",
  "completed",
);

const toolAwaitingApproval = toolPart("tool-call-1", 2, "awaitingApproval");
const toolApproved = toolPart("tool-call-1", 2, "approved");
const toolCompleted = toolPart("tool-call-1", 2, "completed", "D:/repo\n");
const toolApprovalInteraction: InteractionRequest = {
  interactionId: "interaction-tool-approval-1",
  kind: "toolApproval",
  status: "pending",
  scope: {
    sessionId,
    turnId,
    toolId: "tool-call-1",
  },
  payload: {
    type: "toolApproval",
    name: "bash",
    arguments: { command: "pwd" },
    workingDirectory: "D:/repo",
    parentAgentId: null,
  },
  createdAt: 20,
  updatedAt: 20,
};
const toolApprovalResolved: InteractionRequest = {
  ...toolApprovalInteraction,
  status: "resolved",
  resolvedAt: 23,
  updatedAt: 23,
  resolution: {
    type: "toolApproval",
    decision: "approved",
    reason: null,
  },
};
const postToolReasoning = reasoningPart("post-tool-reasoning", 3, "Tool completed, continue final answer.", "completed");
const postToolFinalText = textPart(
  "post-tool-final",
  assistantMessageId,
  4,
  "The command succeeded.",
  "final",
  "completed",
);

const reloadReasoningEmpty = reasoningPart("reload-reasoning", 2, "");
const reloadTextEmpty = textPart("reload-final", assistantMessageId, 3, "", "final");
const reloadReasoningTerminal = reasoningPart("reload-reasoning", 2, "Durable reasoning snapshot.", "completed");
const reloadTextTerminal = textPart("reload-final", assistantMessageId, 3, "Durable final answer.", "final", "completed");

const reasonAEmpty = reasoningPart("reason-a", 2, "");
const reasonBEmpty = reasoningPart("reason-b", 3, "");
const reasonATerminal = reasoningPart("reason-a", 2, "alpha terminal", "completed");

export const opencodeTimelineFixtures = {
  sessionId,
  turnId,
  userMessageId,
  assistantMessageId,
  userInput: {
    interaction: userInputInteraction,
    draft: {
      style: { selected: ["Concise"] },
      notes: { selected: ["Include risks"], freeText: "  add rollback plan  " },
      token: { freeText: "  sk-test-secret  " },
    },
    expectedResolution: userInputExpectedResolution,
  },
  reasoningCommentaryStaged: {
    liveEvents: [
      event(1, { type: "messageUpdated", message: userMessage }),
      event(2, { type: "messagePartUpdated", part: userTextPart }),
      event(3, { type: "messageUpdated", message: assistantMessage }),
      event(4, { type: "messagePartUpdated", part: reasoningStageEmpty }),
      event(5, {
        type: "messagePartDelta",
        delta: {
          sessionId,
          messageId: assistantMessageId,
          partId: reasoningStageEmpty.partId,
          field: "reasoningText",
          delta: "# Check\nfirst clue",
          chunkIndex: 0,
        },
      }),
      event(6, { type: "messagePartUpdated", part: commentaryStageEmpty }),
      event(7, {
        type: "messagePartDelta",
        delta: {
          sessionId,
          messageId: assistantMessageId,
          partId: commentaryStageEmpty.partId,
          field: "text",
          delta: "I will inspect fixtures.",
        },
      }),
    ],
    terminalEvents: [
      event(8, { type: "messagePartUpdated", part: reasoningStageTerminal }),
      event(9, { type: "messagePartUpdated", part: commentaryStageTerminal }),
    ],
    reasoningPartId: reasoningStageEmpty.partId,
    commentaryPartId: commentaryStageEmpty.partId,
    expectedLiveReasoning: "# Check\nfirst clue",
    expectedLiveCommentary: "I will inspect fixtures.",
    expectedTerminalReasoning: reasoningStageTerminal.text,
    expectedTerminalCommentary: commentaryStageTerminal.text,
    expectedRowKeys: [
      `user-message:${userMessageId}`,
      `assistant-part:${userMessageId}:part:${assistantMessageId}:${reasoningStageEmpty.partId}`,
      `assistant-part:${userMessageId}:part:${assistantMessageId}:${commentaryStageEmpty.partId}`,
      "bottom-spacer",
    ],
  },
  toolApprovalContinuation: {
    startEvents: [
      event(10, { type: "messageUpdated", message: userMessage }),
      event(11, { type: "messagePartUpdated", part: userTextPart }),
      event(12, { type: "messageUpdated", message: assistantMessage }),
      event(13, { type: "messagePartUpdated", part: toolAwaitingApproval }),
      event(14, {
        type: "interactionChanged",
        event: { interaction: toolApprovalInteraction },
      }),
    ],
    approvalEvents: [
      event(15, { type: "messagePartUpdated", part: toolApproved }),
      event(16, {
        type: "interactionChanged",
        event: { interaction: toolApprovalResolved },
      }),
    ],
    continuationEvents: [
      event(17, { type: "messagePartUpdated", part: toolCompleted }),
      event(18, { type: "messagePartUpdated", part: postToolReasoning }),
      event(19, { type: "messagePartUpdated", part: postToolFinalText }),
    ],
    toolPartId: toolAwaitingApproval.partId,
    expectedRowKeys: [
      `user-message:${userMessageId}`,
      `assistant-part:${userMessageId}:part:${assistantMessageId}:${toolAwaitingApproval.partId}`,
      `assistant-part:${userMessageId}:part:${assistantMessageId}:${postToolReasoning.partId}`,
      `assistant-part:${userMessageId}:part:${assistantMessageId}:${postToolFinalText.partId}`,
      "bottom-spacer",
    ],
  },
  reloadBackfill: {
    liveEvents: [
      event(20, { type: "messageUpdated", message: userMessage }),
      event(21, { type: "messagePartUpdated", part: userTextPart }),
      event(22, { type: "messageUpdated", message: assistantMessage }),
      event(23, { type: "messagePartUpdated", part: reloadReasoningEmpty }),
      event(24, {
        type: "messagePartDelta",
        delta: {
          sessionId,
          messageId: assistantMessageId,
          partId: reloadReasoningEmpty.partId,
          field: "reasoningText",
          delta: "Live reasoning overlay.",
        },
      }),
      event(25, { type: "messagePartUpdated", part: reloadTextEmpty }),
      event(26, {
        type: "messagePartDelta",
        delta: {
          sessionId,
          messageId: assistantMessageId,
          partId: reloadTextEmpty.partId,
          field: "text",
          delta: "Live final overlay.",
        },
      }),
    ],
    messageSnapshots: [
      { message: userMessage, sequence: 21 },
      { message: { ...assistantMessage, status: "completed" }, sequence: 26 },
    ] as StudioMessageProjection[],
    partSnapshots: [
      { part: userTextPart, sequence: 21 },
      { part: reloadReasoningTerminal, sequence: 27 },
      { part: reloadTextTerminal, sequence: 28 },
    ] as StudioPartProjection[],
    nextSequence: 29,
    reasoningPartId: reloadReasoningEmpty.partId,
    finalPartId: reloadTextEmpty.partId,
    expectedTerminalReasoning: reloadReasoningTerminal.text,
    expectedTerminalFinal: reloadTextTerminal.text,
    expectedRowKeys: [
      `user-message:${userMessageId}`,
      `assistant-part:${userMessageId}:part:${assistantMessageId}:${reloadReasoningEmpty.partId}`,
      `assistant-part:${userMessageId}:part:${assistantMessageId}:${reloadTextEmpty.partId}`,
      "bottom-spacer",
    ],
  },
  reasoningEdgeCases: {
    baseEvents: [
      event(30, { type: "messageUpdated", message: userMessage }),
      event(31, { type: "messagePartUpdated", part: userTextPart }),
      event(32, { type: "messageUpdated", message: assistantMessage }),
      event(33, { type: "messagePartUpdated", part: reasonAEmpty }),
      event(34, { type: "messagePartUpdated", part: reasonBEmpty }),
    ],
    edgeEvents: [
      event(35, {
        type: "messagePartDelta",
        delta: {
          sessionId,
          messageId: assistantMessageId,
          partId: reasonAEmpty.partId,
          field: "reasoningText",
          delta: "stale alpha",
        },
      }),
      event(36, {
        type: "messagePartDelta",
        delta: {
          sessionId,
          messageId: assistantMessageId,
          partId: "orphan-reason",
          field: "reasoningText",
          delta: "lost",
        },
      }),
      event(37, {
        type: "messagePartDelta",
        delta: {
          sessionId,
          messageId: assistantMessageId,
          partId: reasonBEmpty.partId,
          field: "reasoningText",
          delta: "beta live",
        },
      }),
      event(38, { type: "messagePartUpdated", part: reasonATerminal }),
    ],
    reasonAId: reasonAEmpty.partId,
    reasonBId: reasonBEmpty.partId,
    orphanPartId: "orphan-reason",
    expectedReasonAText: reasonATerminal.text,
    expectedReasonBOverlay: "beta live",
    expectedRowKeys: [
      `user-message:${userMessageId}`,
      `assistant-part:${userMessageId}:part:${assistantMessageId}:${reasonAEmpty.partId}`,
      `assistant-part:${userMessageId}:part:${assistantMessageId}:${reasonBEmpty.partId}`,
      "bottom-spacer",
    ],
  },
};
