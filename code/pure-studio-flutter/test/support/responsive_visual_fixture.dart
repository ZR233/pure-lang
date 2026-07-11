import 'package:pure_studio_flutter/src/domain/models/studio_models.dart';

const responsiveVisualSessionTitle =
    'Responsive layout audit for an intentionally long Pure Studio session '
    'title that must truncate before neighboring controls at every supported '
    'desktop viewport';
const responsiveVisualProviderName =
    'DeepSeek Enterprise Provider With An Intentionally Long Display Name '
    'For Responsive Workspace Credential Verification';
const responsiveVisualProviderSubtitle =
    'Primary workspace credential for a deliberately long-running production '
    'environment with regional routing and failover';

const responsiveVisualProviders = [
  ProviderSettingsView(
    id: 'deepseek',
    templateKind: 'deepseek',
    name: responsiveVisualProviderName,
    subtitle: responsiveVisualProviderSubtitle,
    baseUrl: 'https://api.deepseek.com',
    hasBearerToken: true,
    defaultModel: 'deepseek-reasoner',
    models: [
      ProviderModelView(
        slug: 'deepseek-reasoner',
        displayName: 'DeepSeek Reasoner',
        reasoningEfforts: ['high', 'max'],
      ),
      ProviderModelView(
        slug: 'deepseek-v4-flash',
        displayName: 'DeepSeek V4 Flash',
        reasoningEfforts: ['high', 'max'],
      ),
    ],
    status: 'ready',
    usageLabel: 'Balance available',
    modelCount: '2',
    providerKind: 'deep_seek',
  ),
  ProviderSettingsView(
    id: 'openai-enterprise',
    templateKind: 'openai',
    name: 'OpenAI Enterprise Gateway',
    subtitle: 'Organization project with regional data controls',
    baseUrl: 'https://api.openai.com/v1',
    hasBearerToken: true,
    defaultModel: 'gpt-5.4',
    models: [
      ProviderModelView(
        slug: 'gpt-5.4',
        displayName: 'GPT-5.4',
        reasoningEfforts: ['medium', 'high'],
      ),
      ProviderModelView(
        slug: 'gpt-5.4-mini',
        displayName: 'GPT-5.4 mini',
        reasoningEfforts: ['medium', 'high'],
      ),
    ],
    status: 'ready',
    usageLabel: 'Usage unavailable',
    modelCount: '2',
    providerKind: 'open_ai',
  ),
  ProviderSettingsView(
    id: 'anthropic-direct',
    templateKind: 'anthropic',
    name: 'Anthropic Direct',
    subtitle: 'Production reasoning and review workloads',
    baseUrl: 'https://api.anthropic.com',
    hasBearerToken: true,
    defaultModel: 'claude-opus-4-6',
    models: [
      ProviderModelView(
        slug: 'claude-opus-4-6',
        displayName: 'Claude Opus 4.6',
        reasoningEfforts: ['high'],
      ),
      ProviderModelView(
        slug: 'claude-sonnet-4-6',
        displayName: 'Claude Sonnet 4.6',
        reasoningEfforts: ['high'],
      ),
    ],
    status: 'ready',
    usageLabel: 'Usage unavailable',
    modelCount: '2',
    providerKind: 'anthropic',
  ),
  ProviderSettingsView(
    id: 'local-openai-compatible',
    templateKind: 'openai-compatible',
    name: 'Local OpenAI-Compatible Gateway',
    subtitle: 'Workspace fallback served from the local network',
    baseUrl: 'http://127.0.0.1:11434/v1',
    hasBearerToken: false,
    defaultModel: 'qwen3-coder',
    models: [
      ProviderModelView(
        slug: 'qwen3-coder',
        displayName: 'Qwen3 Coder',
        reasoningEfforts: ['enabled'],
      ),
      ProviderModelView(
        slug: 'qwen3-coder-fast',
        displayName: 'Qwen3 Coder Fast',
        reasoningEfforts: ['enabled'],
      ),
    ],
    status: 'missingCredential',
    usageLabel: 'Credential optional',
    modelCount: '2',
    providerKind: 'open_ai',
  ),
  ProviderSettingsView(
    id: 'zhipu-coding-plan',
    templateKind: 'zhipu-coding-plan',
    name: 'Zhipu Coding Plan',
    subtitle: 'Zhipu Platform',
    baseUrl: 'https://open.bigmodel.cn/api/coding/paas/v4',
    hasBearerToken: true,
    defaultModel: 'glm-5.2',
    models: [
      ProviderModelView(
        slug: 'glm-5.2',
        displayName: 'GLM-5.2',
        reasoningEfforts: ['enabled'],
      ),
    ],
    status: 'ready',
    usageLabel: 'Coding plan ready',
    modelCount: '1',
    providerKind: 'zhipu',
  ),
];

const responsiveVisualProviderUsages = [
  ProviderUsageView(
    providerId: 'deepseek',
    updatedAt: 1735689600,
    status: 'ready',
    usageKind: 'deepseekBalance',
    balance: DeepSeekBalanceUsageView(
      isAvailable: true,
      balances: [
        DeepSeekBalanceInfoView(
          currency: 'CNY',
          totalBalance: '88.00',
          grantedBalance: '8.00',
          toppedUpBalance: '80.00',
        ),
      ],
    ),
  ),
  ProviderUsageView(
    providerId: 'zhipu-coding-plan',
    updatedAt: 1735689600,
    status: 'ready',
    usageKind: 'zhipuCodingPlan',
    codingPlan: ZhipuCodingPlanUsageView(
      level: 'Pro',
      limits: [
        ZhipuQuotaLimitView(
          window: 'fiveHour',
          label: 'five hour',
          percentage: 75,
          total: 100,
          remaining: 25,
          nextResetAt: 1735689600,
          usageDetails: [],
        ),
        ZhipuQuotaLimitView(
          window: 'weekly',
          label: 'weekly',
          percentage: 50,
          total: 200,
          remaining: 100,
          nextResetAt: 1735689600,
          usageDetails: [],
        ),
        ZhipuQuotaLimitView(
          window: 'mcpMonthly',
          label: 'mcp',
          percentage: 20,
          nextResetAt: 1735689600,
          usageDetails: [],
        ),
      ],
    ),
  ),
];

StudioState responsiveVisualState() {
  final timestamp = DateTime.fromMillisecondsSinceEpoch(1735689600000);
  const project = StudioProject(
    id: 'project-1',
    name: 'pure-lang-responsive-workspace',
    path: r'C:\workspace\pure-lang\responsive-visual-regression',
  );
  final session = StudioSession(
    id: 'session-1',
    projectId: project.id,
    title: responsiveVisualSessionTitle,
    mode: CompileMode.auto,
    updatedAt: timestamp,
  );
  final messages = [
    TimelineMessage(
      id: 'message-user',
      sessionId: session.id,
      role: 'user',
      createdAt: timestamp,
      sequence: 0,
    ),
    TimelineMessage(
      id: 'message-assistant',
      sessionId: session.id,
      role: 'assistant',
      createdAt: timestamp,
      sequence: 1,
    ),
  ];
  final parts = [
    TimelinePartSnapshot(
      id: 'part-user',
      messageId: messages.first.id,
      sessionId: session.id,
      turnId: 'turn-1',
      type: TimelinePartType.text,
      order: 0,
      revision: 0,
      sequence: 0,
      text:
          'Check the chat, activity summary, and provider settings at every '
          'target viewport.',
      status: 'completed',
      createdAt: timestamp,
      updatedAt: timestamp,
      textChannel: TimelineTextChannel.user,
    ),
    TimelinePartSnapshot(
      id: 'part-assistant',
      messageId: messages.last.id,
      sessionId: session.id,
      turnId: 'turn-1',
      type: TimelinePartType.text,
      order: 0,
      revision: 0,
      sequence: 1,
      text:
          '### Responsive verification\n\n'
          '- Conversation content remains readable.\n'
          '- Status details stay above their trigger.\n'
          '- Provider rows keep actions accessible.',
      status: 'completed',
      createdAt: timestamp,
      updatedAt: timestamp,
      textChannel: TimelineTextChannel.finalAnswer,
    ),
  ];
  return StudioState(
    projects: const [project],
    sessions: [session],
    messagesBySession: {session.id: messages},
    partSnapshotsBySession: {
      session.id: {for (final part in parts) part.id: part},
    },
    agentsBySession: {
      session.id: {
        'agent-reviewer': StudioAgentView(
          id: 'agent-reviewer',
          sessionId: session.id,
          path: 'root/reviewer',
          role: 'reviewer',
          task: 'Audit responsive layout and visual geometry',
          status: 'running',
          summary: 'Checking the activity popover against its trigger.',
          updatedAt: timestamp,
        ),
        'agent-worker': StudioAgentView(
          id: 'agent-worker',
          sessionId: session.id,
          path: 'root/worker',
          role: 'worker',
          task: 'Capture responsive screenshots',
          status: 'completed',
          updatedAt: timestamp,
        ),
      },
    },
    providers: responsiveVisualProviders,
    defaultProviderId: 'deepseek',
    providerUsages: responsiveVisualProviderUsages,
    roles: const [
      RoleSettingsView(
        key: 'planner',
        providerId: 'deepseek',
        model: 'deepseek-reasoner',
        effort: 'high',
      ),
    ],
    mcpServers: const [],
    selectedProjectId: project.id,
    selectedSessionId: session.id,
    permissionMode: PermissionMode.requestApproval,
    turnPhase: TurnPhase.idle,
    runtime: const SessionRuntimeView(
      model: 'deepseek-reasoner',
      contextTokens: 42000,
      contextWindow: 100000,
      totalTokens: 128000,
      costLabel: 'CNY 12.34',
      activeSkills: ['flutter-ui'],
      activeMcpServers: ['dart'],
      activeLspServers: ['rust-analyzer'],
      agentCount: 2,
    ),
    pendingInteractions: const [],
  );
}
