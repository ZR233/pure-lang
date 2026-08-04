import 'package:pure_studio/src/domain/models/studio_models.dart';

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
    wireProtocol: 'chat_completions',
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
    wireProtocol: 'responses',
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
    wireProtocol: 'chat_completions',
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
    wireProtocol: 'responses',
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
    wireProtocol: 'chat_completions',
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
  final session = StudioThread(
    id: 'session-1',
    projectId: project.id,
    title: responsiveVisualSessionTitle,
    mode: StudioMode.simple,
    agentPath: 'agent-planner',
    role: 'planner',
    status: 'idle',
    updatedAt: timestamp,
  );
  final agentSessions = [
    StudioThread(
      id: 'session-reviewer',
      projectId: project.id,
      title: 'Responsive reviewer',
      mode: StudioMode.simple,
      createdAt: timestamp.add(const Duration(seconds: 1)),
      updatedAt: timestamp.add(const Duration(seconds: 1)),
      parentThreadId: session.id,
      rootThreadId: session.id,
      agentPath: 'agent-reviewer',
      role: 'reviewer',
      status: 'running',
    ),
    StudioThread(
      id: 'session-worker',
      projectId: project.id,
      title: 'Capture worker',
      mode: StudioMode.simple,
      createdAt: timestamp.add(const Duration(seconds: 2)),
      updatedAt: timestamp.add(const Duration(seconds: 2)),
      parentThreadId: session.id,
      rootThreadId: session.id,
      agentPath: 'agent-worker',
      role: 'worker',
      status: 'completed',
    ),
  ];
  final items = [
    ThreadItemView(
      id: 'item-user',
      threadId: session.id,
      turnId: 'turn-1',
      kind: ThreadItemKind.userMessage,
      ordinal: 0,
      revision: 0,
      text:
          'Check the chat, activity summary, and provider settings at every '
          'target viewport.',
      status: 'completed',
      createdAt: timestamp,
      updatedAt: timestamp,
    ),
    ThreadItemView(
      id: 'item-reasoning-1',
      threadId: session.id,
      turnId: 'turn-1',
      kind: ThreadItemKind.reasoning,
      ordinal: 1,
      revision: 0,
      text:
          '## Inspecting the timeline\n\n'
          'Checking how compact activity rows preserve conversation order.',
      reasoningSummary: const ['Inspecting the timeline'],
      reasoningContent: const [
        'Checking how compact activity rows preserve conversation order.',
      ],
      status: 'completed',
      createdAt: timestamp,
      updatedAt: timestamp,
    ),
    ThreadItemView(
      id: 'item-reasoning-2',
      threadId: session.id,
      turnId: 'turn-1',
      kind: ThreadItemKind.reasoning,
      ordinal: 2,
      revision: 0,
      text:
          '## Verifying responsive behavior\n\n'
          'Comparing wide and narrow timeline layouts.',
      reasoningSummary: const ['Verifying responsive behavior'],
      reasoningContent: const ['Comparing wide and narrow timeline layouts.'],
      status: 'completed',
      createdAt: timestamp,
      updatedAt: timestamp,
    ),
    ThreadItemView(
      id: 'item-assistant',
      threadId: session.id,
      turnId: 'turn-1',
      kind: ThreadItemKind.agentMessage,
      channel: AgentMessageChannel.finalAnswer,
      ordinal: 3,
      revision: 0,
      text:
          '### Responsive verification\n\n'
          '- Conversation content remains readable.\n'
          '- Status details stay above their trigger.\n'
          '- Provider rows keep actions accessible.',
      status: 'completed',
      createdAt: timestamp,
      updatedAt: timestamp,
    ),
  ];
  return StudioState(
    projects: const [project],
    threads: [session, ...agentSessions],
    workspacesByThread: {
      session.id: ThreadWorkspace(
        thread: session,
        revision: 1,
        items: items,
        interactions: const [],
        runtime: const ThreadRuntimeView(
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
      ),
    },
    workspaceUiByThread: {
      session.id: const WorkspaceUiState(
        syncState: AgentWorkspaceSyncState.ready,
      ),
    },
    providers: responsiveVisualProviders,
    defaultProviderId: 'deepseek',
    providerUsages: responsiveVisualProviderUsages,
    roles: const [
      RoleSettingsView(
        key: 'executor',
        providerId: 'deepseek',
        model: 'deepseek-reasoner',
        effort: 'high',
      ),
      RoleSettingsView(
        key: 'planner',
        providerId: 'deepseek',
        model: 'deepseek-reasoner',
        effort: 'high',
      ),
    ],
    mcpServers: const [],
    selectedProjectId: project.id,
    selectedThreadId: session.id,
    permissionMode: PermissionMode.requestApproval,
  );
}

StudioState responsiveVisualReasoningState() {
  final state = responsiveVisualState();
  final threadId = state.selectedThreadId!;
  final workspace = state.workspacesByThread[threadId]!;
  final items = [...workspace.items]
    ..removeWhere((item) => item.id == 'part-assistant');
  final index = items.indexWhere((item) => item.id == 'part-reasoning-2');
  final current = items[index];
  items[index] = current.copyWith(
    revision: current.revision + 1,
    reasoningSummary: const ['## Updating the active reasoning summary'],
    reasoningContent: const [
      'The latest section replaces the same compact activity line.',
    ],
    status: 'streaming',
  );
  return state.copyWith(
    workspacesByThread: {
      ...state.workspacesByThread,
      threadId: workspace.copyWith(
        items: items,
        activeTurn: StudioTurnView(
          turnId: current.turnId,
          threadId: threadId,
          state: const StudioTurnState.inProgress(StudioTurnActivity.thinking),
          updatedAt: current.updatedAt,
        ),
      ),
    },
  );
}

StudioState responsiveVisualToolState() {
  final state = responsiveVisualState();
  final threadId = state.selectedThreadId!;
  final workspace = state.workspacesByThread[threadId]!;
  final items = [...workspace.items]
    ..removeWhere((item) => item.id == 'part-assistant');
  final reasoning = items.firstWhere((item) => item.id == 'part-reasoning-2');
  items.add(
    ThreadItemView(
      id: 'part-tool-active',
      threadId: threadId,
      turnId: reasoning.turnId,
      ordinal: 3,
      revision: 0,
      status: 'running',
      createdAt: reasoning.createdAt,
      updatedAt: reasoning.updatedAt,
      kind: ThreadItemKind.toolCall,
      tool: const TimelineToolPart(
        toolCallId: 'tool-call-active',
        name: 'exec',
        arguments:
            '{"command":"flutter test test/widget_test.dart",'
            '"workingDirectory":"code/pure-studio"}',
      ),
    ),
  );
  return state.copyWith(
    workspacesByThread: {
      ...state.workspacesByThread,
      threadId: workspace.copyWith(
        items: items,
        activeTurn: StudioTurnView(
          turnId: reasoning.turnId,
          threadId: threadId,
          state: const StudioTurnState.inProgress(
            StudioTurnActivity.runningTool,
          ),
          updatedAt: reasoning.updatedAt,
        ),
      ),
    },
  );
}
