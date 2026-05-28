import type {
  ChatMessage,
  ProjectRecord,
  SessionRecord,
  TrackedToolCall,
} from "../types";

export type StudioStateLike = {
  projects: ProjectRecord[];
  sessions: SessionRecord[];
  messages: ChatMessage[];
  selectedProjectId: string | null;
  selectedSessionId: string | null;
  streamingText: string;
  thinkingText: string;
  toolCalls: Map<string, TrackedToolCall>;
};
