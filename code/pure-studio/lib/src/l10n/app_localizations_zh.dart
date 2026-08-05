// ignore: unused_import
import 'package:intl/intl.dart' as intl;
import 'app_localizations.dart';

// ignore_for_file: type=lint

/// The translations for Chinese (`zh`).
class AppLocalizationsZh extends AppLocalizations {
  AppLocalizationsZh([String locale = 'zh']) : super(locale);

  @override
  String get appTitle => 'Pure Studio';

  @override
  String get sidebarProjects => '项目';

  @override
  String get sidebarSessions => '会话';

  @override
  String get sidebarCloseProject => '关闭项目';

  @override
  String get sidebarArchiveSession => '归档会话';

  @override
  String get sidebarNewSession => '新建会话';

  @override
  String get sidebarOpenProject => '打开项目';

  @override
  String get sidebarSettings => '设置';

  @override
  String get runtimeFatalTitle => 'Pure Studio 无法启动';

  @override
  String get runtimeFatalRetry => '重试';

  @override
  String recoveryGlobalWarning(int count) {
    return '有 $count 个恢复问题需要处理';
  }

  @override
  String get recoveryCleanupTooltip => '查看安全清理';

  @override
  String get recoveryCleanupTitle => '清理恢复问题？';

  @override
  String get recoveryCleanupBody => 'Pure Studio 只会清理自身创建的任务资源。继续前请检查可能丢失的工作。';

  @override
  String get recoveryCleanupNoResources => '未发现仍存在的任务资源。';

  @override
  String get recoveryCleanupPresenceAbsent => '已缺失';

  @override
  String get recoveryCleanupPresenceComplete => '完整';

  @override
  String get recoveryCleanupPresencePartial => '部分缺失';

  @override
  String get recoveryCleanupDirty => '有未提交修改';

  @override
  String recoveryCleanupAhead(int count) {
    return '$count 个未合并提交';
  }

  @override
  String recoveryCleanupChangedFiles(int count) {
    return '$count 个变更文件';
  }

  @override
  String get recoveryCleanupCancel => '取消';

  @override
  String get recoveryCleanupConfirm => '清理';

  @override
  String get recoveryCleanupRefreshPreview => '刷新预览';

  @override
  String recoveryCleanupFailed(String error) {
    return '清理失败：$error';
  }

  @override
  String get projectCleanupTitle => '移除项目并清理 Pure worktree？';

  @override
  String get projectCleanupBody =>
      '这会从 Studio 移除项目，并永久放弃所有 Pure 自建任务 worktree 中的未提交修改和未合并提交。用户主工作区不会被删除或修改。';

  @override
  String get projectCleanupConfirm => '移除项目并清理';

  @override
  String get sidebarNew => '新建';

  @override
  String get sidebarOpen => '打开';

  @override
  String get shellNoSession => '没有会话';

  @override
  String shellSessionUpdated(String mode, String time) {
    return '$mode · 更新于 $time';
  }

  @override
  String get settingsBack => '返回';

  @override
  String get settingsBackToChat => '返回聊天';

  @override
  String get settingsWorkspaceGroup => '工作区';

  @override
  String get settingsSystemGroup => '系统';

  @override
  String get settingsProvidersTab => '服务';

  @override
  String get settingsInstructionsTab => '指令';

  @override
  String get settingsSkillsTab => '技能';

  @override
  String get settingsRolesTab => '角色';

  @override
  String get settingsMcpTab => 'MCP';

  @override
  String get settingsSecurityTab => '安全';

  @override
  String get settingsGeneralTab => '通用';

  @override
  String get composerHint => '描述你的需求...';

  @override
  String get composerSend => '发送';

  @override
  String get composerStop => '停止';

  @override
  String get permissionModeTooltip => '权限模式';

  @override
  String get compileModeSimple => '简洁';

  @override
  String get compileModeTask => '任务';

  @override
  String get permissionModeRequestApproval => '请求';

  @override
  String get permissionModeAutoReview => '审查';

  @override
  String get permissionModeFullAccess => '完全';

  @override
  String get statusCost => '费用';

  @override
  String get statusCostDetailTitle => '会话费用';

  @override
  String get statusTotalTokensLabel => '总 token';

  @override
  String get statusModelLabel => '模型';

  @override
  String get statusCapabilitiesTitle => '活动能力';

  @override
  String get statusSessionMode => '会话模式';

  @override
  String get statusSessionModeLocked => '任务执行期间无法切换会话模式';

  @override
  String get statusPlannerModel => 'Planner 模型';

  @override
  String get statusExecutorModel => 'Executor 模型';

  @override
  String get statusReasoningEffort => '思考等级';

  @override
  String get statusContextLabel => '上下文';

  @override
  String get statusTurnQueued => '排队中';

  @override
  String get statusTurnPreparing => '准备上下文';

  @override
  String get statusTurnResponding => '回复中';

  @override
  String get statusTurnPlanning => '规划中';

  @override
  String get statusTurnRunningTool => '运行工具';

  @override
  String get statusTurnWaitingForApproval => '等待工具授权';

  @override
  String get statusTurnWaitingForUserInput => '等待输入';

  @override
  String get statusTurnWaitingForPlanConfirmation => '等待计划确认';

  @override
  String get statusTurnPersisting => '保存本轮结果';

  @override
  String get statusInteractionToolApproval => '等待工具授权';

  @override
  String get statusInteractionUserInput => '等待输入';

  @override
  String get statusInteractionPlanConfirmation => '等待计划确认';

  @override
  String statusContextTooltip(
    int contextTokens,
    int contextWindow,
    int percent,
    int totalTokens,
    String model,
  ) {
    return '上下文：$contextTokens/$contextWindow（$percent%）\n\n总 token：$totalTokens\n\n模型：$model';
  }

  @override
  String statusContextTooltipNoModel(
    int contextTokens,
    int contextWindow,
    int percent,
    int totalTokens,
  ) {
    return '上下文：$contextTokens/$contextWindow（$percent%）\n\n总 token：$totalTokens';
  }

  @override
  String statusSkillsCount(int count) {
    return '$count 个 skill';
  }

  @override
  String statusMcpCount(int count) {
    return '$count 个 MCP';
  }

  @override
  String statusLspCount(int count) {
    return '$count 个 LSP';
  }

  @override
  String statusAgentsCount(int count) {
    return '$count 个 agent';
  }

  @override
  String get composerAgentRuntimeDriven => '此 Agent 会话由运行时驱动';

  @override
  String get statusTaskSection => '任务协调';

  @override
  String get statusTaskBranch => '分支';

  @override
  String get statusTaskHead => 'HEAD';

  @override
  String get statusTaskWorkUnits => '工作单元';

  @override
  String get statusTaskAgents => '代理';

  @override
  String get statusTaskCompletions => '完成报告';

  @override
  String get statusTaskMerges => '合并与冲突';

  @override
  String get statusTaskReviews => '代码审查';

  @override
  String get statusTaskWorktree => '工作树';

  @override
  String get statusTaskCommit => '提交';

  @override
  String get statusTaskSource => '来源';

  @override
  String get statusTaskConflicts => '冲突';

  @override
  String get statusTaskRequest => '调用请求';

  @override
  String get statusTaskSummary => '摘要';

  @override
  String get statusTaskStage => '阶段';

  @override
  String get statusTaskNextStep => '下一步';

  @override
  String get statusTaskSummaryAge => '摘要更新时间';

  @override
  String get statusTaskVerification => '验证';

  @override
  String get statusTaskScope => '范围';

  @override
  String get statusTaskCompletionRevision => '完成版本';

  @override
  String get statusTaskFindings => '审查发现';

  @override
  String get statusTaskError => '错误';

  @override
  String get statusTaskPhasePlanning => '规划中';

  @override
  String get statusTaskPhasePendingConfirmation => '等待确认';

  @override
  String get statusTaskPhaseDesignUpdating => '更新设计';

  @override
  String get statusTaskPhaseImplementing => '实施中';

  @override
  String get statusTaskPhaseMerging => '合并中';

  @override
  String get statusTaskPhaseResolvingConflict => '解决冲突';

  @override
  String get statusTaskPhaseReviewing => '审查中';

  @override
  String get statusTaskPhaseReworking => '返工中';

  @override
  String get statusTaskPhaseStopping => '停止中';

  @override
  String get statusTaskPhaseCompleted => '任务已完成';

  @override
  String get statusTaskPhaseBlocked => '任务已阻塞';

  @override
  String get statusTaskPhaseFailed => '任务失败';

  @override
  String get statusTaskPhaseCancelled => '任务已取消';

  @override
  String get statusTaskStatusPending => '待处理';

  @override
  String get statusTaskStatusQueued => '排队中';

  @override
  String get statusTaskStatusRunning => '运行中';

  @override
  String get statusTaskStatusAwaitingCompletion => '等待完成报告';

  @override
  String get statusTaskStatusReadyForReview => '等待审查';

  @override
  String get statusTaskStatusReviewing => '审查中';

  @override
  String get statusTaskStatusChangesRequested => '等待修改';

  @override
  String get statusTaskStatusApproved => '已批准';

  @override
  String get statusTaskStatusMerging => '合并中';

  @override
  String get statusTaskStatusMerged => '已合并';

  @override
  String get statusTaskStatusNoDelivery => '无需交付';

  @override
  String get statusTaskStatusCompleted => '已完成';

  @override
  String get statusTaskStatusFailed => '失败';

  @override
  String get statusTaskStatusCancelled => '已取消';

  @override
  String get statusTaskStatusConflicted => '存在冲突';

  @override
  String get statusTaskStatusVerifying => '验证中';

  @override
  String get statusTaskStatusAborted => '已中止';

  @override
  String get statusTaskStatusPass => '已通过';

  @override
  String get statusTaskStatusChangesRequired => '需要修改';

  @override
  String get statusTaskStatusBlocked => '已阻塞';

  @override
  String get statusSkillsSection => 'Skills';

  @override
  String get statusMcpSection => 'MCP';

  @override
  String get statusLspSection => 'LSP';

  @override
  String get statusSubagentsSection => 'Subagents';

  @override
  String get statusAgentChipTooltip => '子代理状态';

  @override
  String get agentDetailTitle => '子代理';

  @override
  String agentDetailSummary(int count, int running) {
    return '$count 个 · $running 运行中';
  }

  @override
  String get agentDetailEmpty => '暂无子代理';

  @override
  String get agentDetailStatusQueued => '排队中';

  @override
  String get agentDetailStatusRunning => '运行中';

  @override
  String get agentDetailStatusWaiting => '等待中';

  @override
  String get agentDetailStatusCompleted => '已完成';

  @override
  String get agentDetailStatusErrored => '出错';

  @override
  String get agentDetailStatusInterrupted => '已中断';

  @override
  String get agentDetailStatusShutdown => '已关闭';

  @override
  String get agentDetailStatusNotFound => '未找到';

  @override
  String get agentDetailSummaryLabel => '摘要';

  @override
  String get agentDetailErrorLabel => '错误';

  @override
  String get agentDetailReasonLabel => '原因';

  @override
  String get agentDetailPathLabel => '路径';

  @override
  String get timelineEmptyTitle => '还没有消息';

  @override
  String get timelineEmptyMessage => '打开项目或开始会话后继续。';

  @override
  String get timelineJumpToLatest => '跳到最新';

  @override
  String get timelineNew => '新内容';

  @override
  String get timelineReasoningFallback => '思考';

  @override
  String get timelineReasoningActive => '思考中';

  @override
  String get timelineReasoningCompleted => '已思考';

  @override
  String get timelineReasoningEmpty => '没有可展示的思考内容。';

  @override
  String get timelineToolFallback => 'Tool';

  @override
  String get timelineToolGroupTitle => '工具活动';

  @override
  String timelineToolGroupSummary(int count) {
    return '$count 个工具';
  }

  @override
  String timelineToolGroupSummaryRunning(int count, int runningCount) {
    return '$count 个工具，$runningCount 个运行中';
  }

  @override
  String timelineToolGroupSummaryIssues(int count, int issueCount) {
    return '$count 个工具，$issueCount 个需要注意';
  }

  @override
  String timelineToolGroupSummaryRunningWithIssues(
    int count,
    int runningCount,
    int issueCount,
  ) {
    return '$count 个工具，$runningCount 个运行中，$issueCount 个需要注意';
  }

  @override
  String get timelinePlanFallback => '计划';

  @override
  String get timelineAgentFallback => 'Agent';

  @override
  String timelineToolCompleted(String name) {
    return '$name 已完成';
  }

  @override
  String timelineToolFailed(String name) {
    return '$name 失败';
  }

  @override
  String timelineToolDenied(String name) {
    return '$name 被拒绝';
  }

  @override
  String timelineToolAwaitingApproval(String name) {
    return '$name 等待授权';
  }

  @override
  String timelineToolRunning(String name) {
    return '$name 运行中';
  }

  @override
  String timelineToolExitCode(int code) {
    return '退出码 $code';
  }

  @override
  String get timelineToolTimedOut => '已超时';

  @override
  String get timelineAgentSubagent => '子代理';

  @override
  String get timelineAgentSubagentMessage => '子代理消息';

  @override
  String get timelineAgentWaiting => '等待子代理';

  @override
  String get timelineAgentClose => '关闭子代理';

  @override
  String get timelineTodoListFallback => '待办列表';

  @override
  String get timelineTodoPending => '待处理';

  @override
  String get timelineTodoInProgress => '进行中';

  @override
  String get timelineTodoCompleted => '已完成';

  @override
  String get interactionQuestionsTitle => '几个问题想确认';

  @override
  String get interactionLastQuestion => '最后一题';

  @override
  String get interactionContinueAfterAnswer => '回答后继续';

  @override
  String get taskResumeTitle => '任务已在重启后暂停';

  @override
  String get taskResumeBody => '从 canonical Task 与 agent 状态继续，不追加新的提示词。';

  @override
  String get taskResumeAction => '继续任务';

  @override
  String get interactionSubmitEmptyAnswersHint => '提交后未答问题保留空数组。';

  @override
  String interactionAnsweredPendingHint(int answeredCount, int pendingCount) {
    return '已答 $answeredCount 题 · $pendingCount 题待答';
  }

  @override
  String get interactionPreviousQuestion => '上一题';

  @override
  String get interactionNextQuestion => '下一题';

  @override
  String get interactionSubmitAnswers => '提交答案';

  @override
  String get interactionNeedInputTitle => '需要你的输入';

  @override
  String get interactionAnswerHint => 'Pure 会把这条回答作为当前问题的答案继续执行。';

  @override
  String get interactionAnswerButton => '回答';

  @override
  String get interactionAnswerLabel => '答案';

  @override
  String interactionQuestionProgress(int current, int total) {
    return '问题 $current / $total';
  }

  @override
  String interactionAnsweredCount(int count) {
    return '$count 已答';
  }

  @override
  String interactionQuestionTooltip(int index) {
    return '问题 $index';
  }

  @override
  String get interactionQuestionFallback => '问题';

  @override
  String get interactionOtherLabel => '其它';

  @override
  String get interactionSecretHint => '输入秘密答案';

  @override
  String get interactionTextHint => '输入你的回答...';

  @override
  String get interactionPermissionTitle => '需要权限';

  @override
  String get interactionPermissionSubtitle => 'Pure 想运行一个工具调用';

  @override
  String get interactionPermissionFooterHint =>
      '工具将在当前工作目录执行；可在 composer 中调整权限模式。';

  @override
  String get interactionReject => '拒绝';

  @override
  String get interactionApprove => '批准';

  @override
  String get interactionReasonLabel => '原因';

  @override
  String get interactionPlanConfirmTitle => '实施此计划？';

  @override
  String get interactionPlanConfirmSubtitle => '确认实施，或直接写下调整要求';

  @override
  String get interactionPlanEditingFooterHint => '只会发送你的调整说明，不会回传计划正文。';

  @override
  String interactionPlanImplementFooterHint(String mode) {
    return '实施会切回$mode模式并开始执行。';
  }

  @override
  String get interactionPlanIgnore => '忽略';

  @override
  String get interactionPlanAdjust => '告诉 Pure 如何调整';

  @override
  String get interactionPlanImplement => '实施此计划';

  @override
  String get interactionPlanAdjustHint => '输入要调整的要求...';

  @override
  String get interactionPlanAdjustSubmit => '提交调整';

  @override
  String get interactionPlanEditingNotice => '继续规划：只提交你的调整说明。';

  @override
  String get interactionPlanViewNotice => '计划内容不会在这里编辑；请在 timeline 中查看完整计划。';

  @override
  String get interactionPlanContinueReason => 'continue planning';

  @override
  String get settingsProvidersTitle => '服务';

  @override
  String get settingsProvidersSubtitle => '模型服务、凭据、模型和用量';

  @override
  String get settingsRefreshUsage => '刷新用量';

  @override
  String get settingsAddProvider => '添加 provider';

  @override
  String get settingsSearchProviders => '搜索 providers';

  @override
  String get settingsNoProvidersMatchTitle => '没有匹配的 providers';

  @override
  String get settingsNoProvidersMatchMessage => '清空搜索以查看所有已配置 providers。';

  @override
  String get settingsNoProvidersTitle => '没有 providers';

  @override
  String get settingsNoProvidersMessage => '添加 provider 后配置凭据和 models。';

  @override
  String get settingsDefaultProvider => '默认 provider';

  @override
  String get settingsSetAsDefaultProvider => '设为默认';

  @override
  String get settingsOpenDetails => '打开详情';

  @override
  String get settingsProviderActions => 'Provider 操作';

  @override
  String get settingsEditProvider => '编辑 provider';

  @override
  String get settingsDeleteProvider => '删除 provider';

  @override
  String get settingsNoProviderSelected => '未选择 provider';

  @override
  String get settingsProviderTitle => 'Provider 信息';

  @override
  String get settingsProviderModelsTitle => '模型';

  @override
  String get settingsProviderConnectionTitle => '连接';

  @override
  String get settingsProviderDefaultModelsTitle => '默认 models';

  @override
  String get settingsProviderCustomModelsTitle => '自定义 models';

  @override
  String get settingsNewProvider => '新建 provider';

  @override
  String get settingsProviderKey => 'Provider key';

  @override
  String get settingsTemplate => 'Template';

  @override
  String get settingsCustomProvider => '自定义 Provider';

  @override
  String get settingsDefaultModel => 'Default model';

  @override
  String get settingsApiKey => 'API key';

  @override
  String get settingsApiKeyKeepCurrent => 'API key（留空以保留当前值）';

  @override
  String get settingsConfigured => '已配置';

  @override
  String get settingsMissing => '缺失';

  @override
  String get settingsDisplayName => 'Display name';

  @override
  String get settingsProtocolType => 'Protocol type';

  @override
  String get settingsBaseUrl => 'Base URL';

  @override
  String get settingsModelSlug => 'Model slug';

  @override
  String get settingsReasoningEfforts => 'Reasoning efforts';

  @override
  String get settingsEdit => '编辑';

  @override
  String get settingsCancel => '取消';

  @override
  String get settingsSave => '保存';

  @override
  String get settingsAddModel => '添加 model';

  @override
  String get settingsRemoveModel => '移除 model';

  @override
  String get settingsNoCustomModels => '没有自定义 models';

  @override
  String settingsBundledModels(int count) {
    return '$count 个内置';
  }

  @override
  String get settingsDefaultBadge => 'default';

  @override
  String get settingsReadyBadge => 'ready';

  @override
  String get settingsSetupBadge => 'setup';

  @override
  String get settingsUsageTitle => '用量';

  @override
  String settingsUsageUpdated(String updatedAt) {
    return '更新于 $updatedAt';
  }

  @override
  String get settingsUsageAvailableBalance => '可用余额';

  @override
  String get settingsUsageBalanceUnavailable => '余额不可用';

  @override
  String settingsUsageGranted(String amount) {
    return '赠送 $amount';
  }

  @override
  String settingsUsageToppedUp(String amount) {
    return '充值 $amount';
  }

  @override
  String get settingsUsageRefreshing => '正在刷新用量...';

  @override
  String get settingsUsageChecking => '正在检查用量...';

  @override
  String get settingsUsageCheckingShort => '正在检查用量';

  @override
  String get settingsUsageNotLoaded => '用量未加载';

  @override
  String get settingsUsageUnsupported => '不支持';

  @override
  String get settingsUsageNotSupported => '不支持用量查询';

  @override
  String get settingsUsageMissingKey => '缺少 key';

  @override
  String get settingsUsageFailed => '用量查询失败';

  @override
  String get settingsUsageQueryFailed => '用量查询失败';

  @override
  String get settingsUsageApiKeyMissing => '未配置 provider API key';

  @override
  String settingsUsageUnsupportedForProvider(String providerName) {
    return '$providerName 不支持用量查询';
  }

  @override
  String get settingsUsageNotChecked => '未检查';

  @override
  String get settingsUsageUnavailable => '用量不可用';

  @override
  String get settingsUsageError => '无法加载用量';

  @override
  String get settingsUsageNoQuota => '没有返回额度详情。';

  @override
  String get settingsUsageTools => 'Tools';

  @override
  String get settingsUsageToken => 'Token 用量';

  @override
  String get settingsUsageSpend => '花费';

  @override
  String get settingsUsageRemaining => '剩余';

  @override
  String get settingsUsageUsed => '已用';

  @override
  String get settingsUsageFiveHourQuota => '5 小时额度';

  @override
  String get settingsUsageWeeklyQuota => '每周额度';

  @override
  String get settingsUsageMcpQuota => 'MCP 额度';

  @override
  String get settingsUsageQuota => '额度';

  @override
  String settingsUsageQuotaRemaining(String remaining, String total) {
    return '剩余 $remaining / $total';
  }

  @override
  String settingsUsageQuotaUsed(String current, String total) {
    return '已用 $current / $total';
  }

  @override
  String settingsUsagePercentRemaining(String percent) {
    return '剩余 $percent';
  }

  @override
  String settingsUsageReset(String time) {
    return '$time 重置';
  }

  @override
  String get settingsInstructionsTitle => '指令';

  @override
  String get settingsInstructionsSubtitle => '注入到每轮对话；停止输入后自动保存。';

  @override
  String get settingsBaseInstructions => 'Base instructions';

  @override
  String get settingsDeveloperInstructions => 'Developer instructions';

  @override
  String get settingsUserContext => 'User context';

  @override
  String get settingsInstructionHint => '在这里添加项目指导';

  @override
  String get settingsSkillsTitle => '技能';

  @override
  String get settingsSkillsSubtitle => '禁用过于嘈杂的 skills，或发现项目/用户/系统 skill 目录。';

  @override
  String get settingsDiscover => '发现';

  @override
  String get settingsDiscovering => '发现中';

  @override
  String get settingsFilterSkills => '过滤 skills';

  @override
  String get settingsSkillDisabled => '此工作区已禁用';

  @override
  String get settingsSkillEnabled => '已启用';

  @override
  String get settingsOpenProjectToDiscoverSkills => '打开项目以发现 skills';

  @override
  String get settingsNoSkillsMatchFilter => '没有匹配的 skills';

  @override
  String get settingsSkillsDiscoverySources =>
      'Skills 会从当前工作区以及已配置的用户/系统来源中发现。';

  @override
  String get settingsClearSearchOrDiscoverAgain => '清空搜索，或重新运行发现。';

  @override
  String get settingsNoSkillsTitle => '没有找到 skills';

  @override
  String get settingsNoSkillsMessage => '换个过滤条件，或发现当前项目的 skills。';

  @override
  String get settingsRolesTitle => '角色';

  @override
  String get settingsRolesSubtitle => '为每个固定 agent role 选择 provider/model 默认值。';

  @override
  String get settingsRoleExplorerDescription => '探索代码并收集上下文。';

  @override
  String get settingsRolePlannerDescription => '起草计划并组织意图。';

  @override
  String get settingsRoleExecutorDescription => '应用编辑并运行工具。';

  @override
  String get settingsRoleReviewerDescription => '审查结果并验证风险。';

  @override
  String get settingsRoleFallbackDescription => 'Studio role';

  @override
  String get settingsModelField => 'Model';

  @override
  String get settingsMcpTitle => 'MCP';

  @override
  String get settingsMcpSubtitle => 'Model Context Protocol servers 和内联端点。';

  @override
  String get settingsEndpoint => 'Endpoint';

  @override
  String get settingsMcpEmptyTitle => '没有 MCP servers';

  @override
  String get settingsMcpEmptyMessage => '已配置的 MCP servers 会显示在这里。';

  @override
  String get settingsSecurityTitle => '安全';

  @override
  String get settingsSecuritySubtitle => '选择此工作区默认审批姿态。';

  @override
  String get settingsSecurityModeSubtitle => '工具执行权限模式；修改会立即生效。';

  @override
  String settingsCurrentMode(String mode) {
    return '当前：$mode';
  }

  @override
  String get settingsWorkspaceBoundary => '工作区边界策略保持不变。';

  @override
  String get settingsGeneralTitle => '通用';

  @override
  String get settingsGeneralSubtitle => '界面偏好保存到 Studio store。';

  @override
  String get settingsFollowSystemTheme => '跟随系统主题';

  @override
  String get settingsFollowSystemThemeSubtitle => '随操作系统切换亮色和暗色模式。';

  @override
  String get settingsFollowActiveTurn => '跟随当前 turn';

  @override
  String get settingsFollowActiveTurnSubtitle =>
      '让新的 timeline 输出保持 pinned 到最新 turn。';

  @override
  String get settingsCompactTimeline => 'Compact timeline';

  @override
  String get settingsCompactTimelineSubtitle => '减少消息间距，适合更密集阅读。';

  @override
  String get settingsWebSearchTitle => 'Web 搜索';

  @override
  String get settingsWebSearchSubtitle =>
      '通过符合条件的 OpenAI 账户执行搜索；修改从下一次 turn 起生效。';

  @override
  String get settingsWebSearchConfiguredMode => '已配置模式';

  @override
  String get settingsWebSearchEffectiveMode => '有效模式';

  @override
  String get settingsWebSearchProvider => 'OpenAI Provider';

  @override
  String get settingsWebSearchModel => '搜索模型';

  @override
  String get settingsWebSearchMode => '模式';

  @override
  String get settingsWebSearchModeDisabled => '禁用';

  @override
  String get settingsWebSearchModeCached => '缓存';

  @override
  String get settingsWebSearchModeIndexed => '索引';

  @override
  String get settingsWebSearchModeLive => '实时';

  @override
  String get settingsWebSearchContextSize => '上下文大小';

  @override
  String get settingsWebSearchContextLow => '低';

  @override
  String get settingsWebSearchContextMedium => '中';

  @override
  String get settingsWebSearchContextHigh => '高';

  @override
  String get settingsServiceDefault => '服务默认值';

  @override
  String get settingsWebSearchAllowedDomains => '允许的域名';

  @override
  String get settingsWebSearchDomainsHint => 'example.com, docs.example.com';

  @override
  String get settingsWebSearchCountry => '国家';

  @override
  String get settingsWebSearchRegion => '地区';

  @override
  String get settingsWebSearchCity => '城市';

  @override
  String get settingsWebSearchTimezone => '时区';

  @override
  String get settingsWebSearchAvailable => '可用';

  @override
  String get settingsWebSearchDisabled => '已禁用';

  @override
  String get settingsWebSearchMissingCredential => '缺少凭证';

  @override
  String get settingsWebSearchUnsupportedModel => '模型不支持';

  @override
  String get settingsWebSearchMissingCredentialReason =>
      '没有源自 OpenAI preset 且凭证有效的 Provider，远程 Web 搜索已完全禁用。';

  @override
  String get settingsWebSearchUnsupportedModelReason =>
      '当前路由无法暴露函数工具或 hosted Web 搜索。';

  @override
  String get settingsNotAvailable => '不可用';

  @override
  String get settingsSaveWebSearch => '保存 Web 搜索';

  @override
  String get settingsStudioUpdateTitle => 'Pure Studio 更新';

  @override
  String settingsStudioUpdateDisabled(String version) {
    return '当前版本 $version。仅 Windows 正式版会自动检查更新。';
  }

  @override
  String settingsStudioUpdateCurrent(String version) {
    return '当前版本：$version';
  }

  @override
  String settingsStudioUpdateChecking(String version) {
    return '当前版本 $version，正在检查更新…';
  }

  @override
  String settingsStudioUpdateLatest(String version) {
    return '当前版本 $version 已是最新版。';
  }

  @override
  String settingsStudioUpdateAvailable(String current, String latest) {
    return '已安装 $current，可升级到 $latest。';
  }

  @override
  String settingsStudioUpdateDownloading(String version, int progress) {
    return '正在下载 $version：$progress%';
  }

  @override
  String settingsStudioUpdateVerifying(String version) {
    return '正在验证 $version…';
  }

  @override
  String settingsStudioUpdateInstallerLaunched(String version) {
    return '$version 安装程序已启动。';
  }

  @override
  String settingsStudioUpdateFailed(String error) {
    return '更新失败：$error';
  }

  @override
  String get settingsStudioUpdateBusy => '请先等待当前 turn 或 task 结束，再安装更新。';

  @override
  String get settingsStudioUpdateCheck => '检查更新';

  @override
  String get settingsStudioUpdateInstall => '下载并安装';

  @override
  String get settingsStudioUpdateReleaseNotes => '发行说明';

  @override
  String get timelineWebSearchTitle => 'Web 搜索';

  @override
  String get timelineWebSearchSearching => '正在搜索网页';

  @override
  String get timelineWebSearchOpening => '正在打开网页';

  @override
  String get timelineWebSearchFinding => '正在页内查找';

  @override
  String get timelineWebSearchResults => '结果链接';
}

/// The translations for Chinese, using the Han script (`zh_Hans`).
class AppLocalizationsZhHans extends AppLocalizationsZh {
  AppLocalizationsZhHans() : super('zh_Hans');

  @override
  String get appTitle => 'Pure Studio';

  @override
  String get sidebarProjects => '项目';

  @override
  String get sidebarSessions => '会话';

  @override
  String get sidebarCloseProject => '关闭项目';

  @override
  String get sidebarArchiveSession => '归档会话';

  @override
  String get sidebarNewSession => '新建会话';

  @override
  String get sidebarOpenProject => '打开项目';

  @override
  String get sidebarSettings => '设置';

  @override
  String get runtimeFatalTitle => 'Pure Studio 无法启动';

  @override
  String get runtimeFatalRetry => '重试';

  @override
  String recoveryGlobalWarning(int count) {
    return '有 $count 个恢复问题需要处理';
  }

  @override
  String get recoveryCleanupTooltip => '查看安全清理';

  @override
  String get recoveryCleanupTitle => '清理恢复问题？';

  @override
  String get recoveryCleanupBody => 'Pure Studio 只会清理自身创建的任务资源。继续前请检查可能丢失的工作。';

  @override
  String get recoveryCleanupNoResources => '未发现仍存在的任务资源。';

  @override
  String get recoveryCleanupPresenceAbsent => '已缺失';

  @override
  String get recoveryCleanupPresenceComplete => '完整';

  @override
  String get recoveryCleanupPresencePartial => '部分缺失';

  @override
  String get recoveryCleanupDirty => '有未提交修改';

  @override
  String recoveryCleanupAhead(int count) {
    return '$count 个未合并提交';
  }

  @override
  String recoveryCleanupChangedFiles(int count) {
    return '$count 个变更文件';
  }

  @override
  String get recoveryCleanupCancel => '取消';

  @override
  String get recoveryCleanupConfirm => '清理';

  @override
  String get recoveryCleanupRefreshPreview => '刷新预览';

  @override
  String recoveryCleanupFailed(String error) {
    return '清理失败：$error';
  }

  @override
  String get projectCleanupTitle => '移除项目并清理 Pure worktree？';

  @override
  String get projectCleanupBody =>
      '这会从 Studio 移除项目，并永久放弃所有 Pure 自建任务 worktree 中的未提交修改和未合并提交。用户主工作区不会被删除或修改。';

  @override
  String get projectCleanupConfirm => '移除项目并清理';

  @override
  String get sidebarNew => '新建';

  @override
  String get sidebarOpen => '打开';

  @override
  String get shellNoSession => '没有会话';

  @override
  String shellSessionUpdated(String mode, String time) {
    return '$mode · 更新于 $time';
  }

  @override
  String get settingsBack => '返回';

  @override
  String get settingsBackToChat => '返回聊天';

  @override
  String get settingsWorkspaceGroup => '工作区';

  @override
  String get settingsSystemGroup => '系统';

  @override
  String get settingsProvidersTab => '服务';

  @override
  String get settingsInstructionsTab => '指令';

  @override
  String get settingsSkillsTab => '技能';

  @override
  String get settingsRolesTab => '角色';

  @override
  String get settingsMcpTab => 'MCP';

  @override
  String get settingsSecurityTab => '安全';

  @override
  String get settingsGeneralTab => '通用';

  @override
  String get composerHint => '描述你的需求...';

  @override
  String get composerSend => '发送';

  @override
  String get composerStop => '停止';

  @override
  String get permissionModeTooltip => '权限模式';

  @override
  String get compileModeSimple => '简洁';

  @override
  String get compileModeTask => '任务';

  @override
  String get permissionModeRequestApproval => '请求';

  @override
  String get permissionModeAutoReview => '审查';

  @override
  String get permissionModeFullAccess => '完全';

  @override
  String get statusCost => '费用';

  @override
  String get statusCostDetailTitle => '会话费用';

  @override
  String get statusTotalTokensLabel => '总 token';

  @override
  String get statusModelLabel => '模型';

  @override
  String get statusCapabilitiesTitle => '活动能力';

  @override
  String get statusSessionMode => '会话模式';

  @override
  String get statusSessionModeLocked => '任务执行期间无法切换会话模式';

  @override
  String get statusPlannerModel => 'Planner 模型';

  @override
  String get statusExecutorModel => 'Executor 模型';

  @override
  String get statusReasoningEffort => '思考等级';

  @override
  String get statusContextLabel => '上下文';

  @override
  String get statusTurnQueued => '排队中';

  @override
  String get statusTurnPreparing => '准备上下文';

  @override
  String get statusTurnResponding => '回复中';

  @override
  String get statusTurnPlanning => '规划中';

  @override
  String get statusTurnRunningTool => '运行工具';

  @override
  String get statusTurnWaitingForApproval => '等待工具授权';

  @override
  String get statusTurnWaitingForUserInput => '等待输入';

  @override
  String get statusTurnWaitingForPlanConfirmation => '等待计划确认';

  @override
  String get statusTurnPersisting => '保存本轮结果';

  @override
  String get statusInteractionToolApproval => '等待工具授权';

  @override
  String get statusInteractionUserInput => '等待输入';

  @override
  String get statusInteractionPlanConfirmation => '等待计划确认';

  @override
  String statusContextTooltip(
    int contextTokens,
    int contextWindow,
    int percent,
    int totalTokens,
    String model,
  ) {
    return '上下文：$contextTokens/$contextWindow（$percent%）\n\n总 token：$totalTokens\n\n模型：$model';
  }

  @override
  String statusContextTooltipNoModel(
    int contextTokens,
    int contextWindow,
    int percent,
    int totalTokens,
  ) {
    return '上下文：$contextTokens/$contextWindow（$percent%）\n\n总 token：$totalTokens';
  }

  @override
  String statusSkillsCount(int count) {
    return '$count 个 skill';
  }

  @override
  String statusMcpCount(int count) {
    return '$count 个 MCP';
  }

  @override
  String statusLspCount(int count) {
    return '$count 个 LSP';
  }

  @override
  String statusAgentsCount(int count) {
    return '$count 个 agent';
  }

  @override
  String get composerAgentRuntimeDriven => '此 Agent 会话由运行时驱动';

  @override
  String get statusTaskSection => '任务协调';

  @override
  String get statusTaskBranch => '分支';

  @override
  String get statusTaskHead => 'HEAD';

  @override
  String get statusTaskWorkUnits => '工作单元';

  @override
  String get statusTaskAgents => '代理';

  @override
  String get statusTaskCompletions => '完成报告';

  @override
  String get statusTaskMerges => '合并与冲突';

  @override
  String get statusTaskReviews => '代码审查';

  @override
  String get statusTaskWorktree => '工作树';

  @override
  String get statusTaskCommit => '提交';

  @override
  String get statusTaskSource => '来源';

  @override
  String get statusTaskConflicts => '冲突';

  @override
  String get statusTaskRequest => '调用请求';

  @override
  String get statusTaskSummary => '摘要';

  @override
  String get statusTaskStage => '阶段';

  @override
  String get statusTaskNextStep => '下一步';

  @override
  String get statusTaskSummaryAge => '摘要更新时间';

  @override
  String get statusTaskVerification => '验证';

  @override
  String get statusTaskScope => '范围';

  @override
  String get statusTaskCompletionRevision => '完成版本';

  @override
  String get statusTaskFindings => '审查发现';

  @override
  String get statusTaskError => '错误';

  @override
  String get statusTaskPhasePlanning => '规划中';

  @override
  String get statusTaskPhasePendingConfirmation => '等待确认';

  @override
  String get statusTaskPhaseDesignUpdating => '更新设计';

  @override
  String get statusTaskPhaseImplementing => '实施中';

  @override
  String get statusTaskPhaseMerging => '合并中';

  @override
  String get statusTaskPhaseResolvingConflict => '解决冲突';

  @override
  String get statusTaskPhaseReviewing => '审查中';

  @override
  String get statusTaskPhaseReworking => '返工中';

  @override
  String get statusTaskPhaseStopping => '停止中';

  @override
  String get statusTaskPhaseCompleted => '任务已完成';

  @override
  String get statusTaskPhaseBlocked => '任务已阻塞';

  @override
  String get statusTaskPhaseFailed => '任务失败';

  @override
  String get statusTaskPhaseCancelled => '任务已取消';

  @override
  String get statusTaskStatusPending => '待处理';

  @override
  String get statusTaskStatusQueued => '排队中';

  @override
  String get statusTaskStatusRunning => '运行中';

  @override
  String get statusTaskStatusAwaitingCompletion => '等待完成报告';

  @override
  String get statusTaskStatusReadyForReview => '等待审查';

  @override
  String get statusTaskStatusReviewing => '审查中';

  @override
  String get statusTaskStatusChangesRequested => '等待修改';

  @override
  String get statusTaskStatusApproved => '已批准';

  @override
  String get statusTaskStatusMerging => '合并中';

  @override
  String get statusTaskStatusMerged => '已合并';

  @override
  String get statusTaskStatusNoDelivery => '无需交付';

  @override
  String get statusTaskStatusCompleted => '已完成';

  @override
  String get statusTaskStatusFailed => '失败';

  @override
  String get statusTaskStatusCancelled => '已取消';

  @override
  String get statusTaskStatusConflicted => '存在冲突';

  @override
  String get statusTaskStatusVerifying => '验证中';

  @override
  String get statusTaskStatusAborted => '已中止';

  @override
  String get statusTaskStatusPass => '已通过';

  @override
  String get statusTaskStatusChangesRequired => '需要修改';

  @override
  String get statusTaskStatusBlocked => '已阻塞';

  @override
  String get statusSkillsSection => 'Skills';

  @override
  String get statusMcpSection => 'MCP';

  @override
  String get statusLspSection => 'LSP';

  @override
  String get statusSubagentsSection => 'Subagents';

  @override
  String get statusAgentChipTooltip => '子代理状态';

  @override
  String get agentDetailTitle => '子代理';

  @override
  String agentDetailSummary(int count, int running) {
    return '$count 个 · $running 运行中';
  }

  @override
  String get agentDetailEmpty => '暂无子代理';

  @override
  String get agentDetailStatusQueued => '排队中';

  @override
  String get agentDetailStatusRunning => '运行中';

  @override
  String get agentDetailStatusWaiting => '等待中';

  @override
  String get agentDetailStatusCompleted => '已完成';

  @override
  String get agentDetailStatusErrored => '出错';

  @override
  String get agentDetailStatusInterrupted => '已中断';

  @override
  String get agentDetailStatusShutdown => '已关闭';

  @override
  String get agentDetailStatusNotFound => '未找到';

  @override
  String get agentDetailSummaryLabel => '摘要';

  @override
  String get agentDetailErrorLabel => '错误';

  @override
  String get agentDetailReasonLabel => '原因';

  @override
  String get agentDetailPathLabel => '路径';

  @override
  String get timelineEmptyTitle => '还没有消息';

  @override
  String get timelineEmptyMessage => '打开项目或开始会话后继续。';

  @override
  String get timelineJumpToLatest => '跳到最新';

  @override
  String get timelineNew => '新内容';

  @override
  String get timelineReasoningFallback => '思考';

  @override
  String get timelineReasoningActive => '思考中';

  @override
  String get timelineReasoningCompleted => '已思考';

  @override
  String get timelineReasoningEmpty => '没有可展示的思考内容。';

  @override
  String get timelineToolFallback => 'Tool';

  @override
  String get timelineToolGroupTitle => '工具活动';

  @override
  String timelineToolGroupSummary(int count) {
    return '$count 个工具';
  }

  @override
  String timelineToolGroupSummaryRunning(int count, int runningCount) {
    return '$count 个工具，$runningCount 个运行中';
  }

  @override
  String timelineToolGroupSummaryIssues(int count, int issueCount) {
    return '$count 个工具，$issueCount 个需要注意';
  }

  @override
  String timelineToolGroupSummaryRunningWithIssues(
    int count,
    int runningCount,
    int issueCount,
  ) {
    return '$count 个工具，$runningCount 个运行中，$issueCount 个需要注意';
  }

  @override
  String get timelinePlanFallback => '计划';

  @override
  String get timelineAgentFallback => 'Agent';

  @override
  String timelineToolCompleted(String name) {
    return '$name 已完成';
  }

  @override
  String timelineToolFailed(String name) {
    return '$name 失败';
  }

  @override
  String timelineToolDenied(String name) {
    return '$name 被拒绝';
  }

  @override
  String timelineToolAwaitingApproval(String name) {
    return '$name 等待授权';
  }

  @override
  String timelineToolRunning(String name) {
    return '$name 运行中';
  }

  @override
  String timelineToolExitCode(int code) {
    return '退出码 $code';
  }

  @override
  String get timelineToolTimedOut => '已超时';

  @override
  String get timelineAgentSubagent => '子代理';

  @override
  String get timelineAgentSubagentMessage => '子代理消息';

  @override
  String get timelineAgentWaiting => '等待子代理';

  @override
  String get timelineAgentClose => '关闭子代理';

  @override
  String get timelineTodoListFallback => '待办列表';

  @override
  String get timelineTodoPending => '待处理';

  @override
  String get timelineTodoInProgress => '进行中';

  @override
  String get timelineTodoCompleted => '已完成';

  @override
  String get interactionQuestionsTitle => '几个问题想确认';

  @override
  String get interactionLastQuestion => '最后一题';

  @override
  String get interactionContinueAfterAnswer => '回答后继续';

  @override
  String get taskResumeTitle => '任务已在重启后暂停';

  @override
  String get taskResumeBody => '从 canonical Task 与 agent 状态继续，不追加新的提示词。';

  @override
  String get taskResumeAction => '继续任务';

  @override
  String get interactionSubmitEmptyAnswersHint => '提交后未答问题保留空数组。';

  @override
  String interactionAnsweredPendingHint(int answeredCount, int pendingCount) {
    return '已答 $answeredCount 题 · $pendingCount 题待答';
  }

  @override
  String get interactionPreviousQuestion => '上一题';

  @override
  String get interactionNextQuestion => '下一题';

  @override
  String get interactionSubmitAnswers => '提交答案';

  @override
  String get interactionNeedInputTitle => '需要你的输入';

  @override
  String get interactionAnswerHint => 'Pure 会把这条回答作为当前问题的答案继续执行。';

  @override
  String get interactionAnswerButton => '回答';

  @override
  String get interactionAnswerLabel => '答案';

  @override
  String interactionQuestionProgress(int current, int total) {
    return '问题 $current / $total';
  }

  @override
  String interactionAnsweredCount(int count) {
    return '$count 已答';
  }

  @override
  String interactionQuestionTooltip(int index) {
    return '问题 $index';
  }

  @override
  String get interactionQuestionFallback => '问题';

  @override
  String get interactionOtherLabel => '其它';

  @override
  String get interactionSecretHint => '输入秘密答案';

  @override
  String get interactionTextHint => '输入你的回答...';

  @override
  String get interactionPermissionTitle => '需要权限';

  @override
  String get interactionPermissionSubtitle => 'Pure 想运行一个工具调用';

  @override
  String get interactionPermissionFooterHint =>
      '工具将在当前工作目录执行；可在 composer 中调整权限模式。';

  @override
  String get interactionReject => '拒绝';

  @override
  String get interactionApprove => '批准';

  @override
  String get interactionReasonLabel => '原因';

  @override
  String get interactionPlanConfirmTitle => '实施此计划？';

  @override
  String get interactionPlanConfirmSubtitle => '确认实施，或直接写下调整要求';

  @override
  String get interactionPlanEditingFooterHint => '只会发送你的调整说明，不会回传计划正文。';

  @override
  String interactionPlanImplementFooterHint(String mode) {
    return '实施会切回$mode模式并开始执行。';
  }

  @override
  String get interactionPlanIgnore => '忽略';

  @override
  String get interactionPlanAdjust => '告诉 Pure 如何调整';

  @override
  String get interactionPlanImplement => '实施此计划';

  @override
  String get interactionPlanAdjustHint => '输入要调整的要求...';

  @override
  String get interactionPlanAdjustSubmit => '提交调整';

  @override
  String get interactionPlanEditingNotice => '继续规划：只提交你的调整说明。';

  @override
  String get interactionPlanViewNotice => '计划内容不会在这里编辑；请在 timeline 中查看完整计划。';

  @override
  String get interactionPlanContinueReason => 'continue planning';

  @override
  String get settingsProvidersTitle => '服务';

  @override
  String get settingsProvidersSubtitle => '模型服务、凭据、模型和用量';

  @override
  String get settingsRefreshUsage => '刷新用量';

  @override
  String get settingsAddProvider => '添加 provider';

  @override
  String get settingsSearchProviders => '搜索 providers';

  @override
  String get settingsNoProvidersMatchTitle => '没有匹配的 providers';

  @override
  String get settingsNoProvidersMatchMessage => '清空搜索以查看所有已配置 providers。';

  @override
  String get settingsNoProvidersTitle => '没有 providers';

  @override
  String get settingsNoProvidersMessage => '添加 provider 后配置凭据和 models。';

  @override
  String get settingsDefaultProvider => '默认 provider';

  @override
  String get settingsSetAsDefaultProvider => '设为默认';

  @override
  String get settingsOpenDetails => '打开详情';

  @override
  String get settingsProviderActions => 'Provider 操作';

  @override
  String get settingsEditProvider => '编辑 provider';

  @override
  String get settingsDeleteProvider => '删除 provider';

  @override
  String get settingsNoProviderSelected => '未选择 provider';

  @override
  String get settingsProviderTitle => 'Provider 信息';

  @override
  String get settingsProviderModelsTitle => '模型';

  @override
  String get settingsProviderConnectionTitle => '连接';

  @override
  String get settingsProviderDefaultModelsTitle => '默认 models';

  @override
  String get settingsProviderCustomModelsTitle => '自定义 models';

  @override
  String get settingsNewProvider => '新建 provider';

  @override
  String get settingsProviderKey => 'Provider key';

  @override
  String get settingsTemplate => 'Template';

  @override
  String get settingsCustomProvider => '自定义 Provider';

  @override
  String get settingsDefaultModel => 'Default model';

  @override
  String get settingsApiKey => 'API key';

  @override
  String get settingsApiKeyKeepCurrent => 'API key（留空以保留当前值）';

  @override
  String get settingsConfigured => '已配置';

  @override
  String get settingsMissing => '缺失';

  @override
  String get settingsDisplayName => 'Display name';

  @override
  String get settingsProtocolType => 'Protocol type';

  @override
  String get settingsBaseUrl => 'Base URL';

  @override
  String get settingsModelSlug => 'Model slug';

  @override
  String get settingsReasoningEfforts => 'Reasoning efforts';

  @override
  String get settingsEdit => '编辑';

  @override
  String get settingsCancel => '取消';

  @override
  String get settingsSave => '保存';

  @override
  String get settingsAddModel => '添加 model';

  @override
  String get settingsRemoveModel => '移除 model';

  @override
  String get settingsNoCustomModels => '没有自定义 models';

  @override
  String settingsBundledModels(int count) {
    return '$count 个内置';
  }

  @override
  String get settingsDefaultBadge => 'default';

  @override
  String get settingsReadyBadge => 'ready';

  @override
  String get settingsSetupBadge => 'setup';

  @override
  String get settingsUsageTitle => '用量';

  @override
  String settingsUsageUpdated(String updatedAt) {
    return '更新于 $updatedAt';
  }

  @override
  String get settingsUsageAvailableBalance => '可用余额';

  @override
  String get settingsUsageBalanceUnavailable => '余额不可用';

  @override
  String settingsUsageGranted(String amount) {
    return '赠送 $amount';
  }

  @override
  String settingsUsageToppedUp(String amount) {
    return '充值 $amount';
  }

  @override
  String get settingsUsageRefreshing => '正在刷新用量...';

  @override
  String get settingsUsageChecking => '正在检查用量...';

  @override
  String get settingsUsageCheckingShort => '正在检查用量';

  @override
  String get settingsUsageNotLoaded => '用量未加载';

  @override
  String get settingsUsageUnsupported => '不支持';

  @override
  String get settingsUsageNotSupported => '不支持用量查询';

  @override
  String get settingsUsageMissingKey => '缺少 key';

  @override
  String get settingsUsageFailed => '用量查询失败';

  @override
  String get settingsUsageQueryFailed => '用量查询失败';

  @override
  String get settingsUsageApiKeyMissing => '未配置 provider API key';

  @override
  String settingsUsageUnsupportedForProvider(String providerName) {
    return '$providerName 不支持用量查询';
  }

  @override
  String get settingsUsageNotChecked => '未检查';

  @override
  String get settingsUsageUnavailable => '用量不可用';

  @override
  String get settingsUsageError => '无法加载用量';

  @override
  String get settingsUsageNoQuota => '没有返回额度详情。';

  @override
  String get settingsUsageTools => 'Tools';

  @override
  String get settingsUsageToken => 'Token 用量';

  @override
  String get settingsUsageSpend => '花费';

  @override
  String get settingsUsageRemaining => '剩余';

  @override
  String get settingsUsageUsed => '已用';

  @override
  String get settingsUsageFiveHourQuota => '5 小时额度';

  @override
  String get settingsUsageWeeklyQuota => '每周额度';

  @override
  String get settingsUsageMcpQuota => 'MCP 额度';

  @override
  String get settingsUsageQuota => '额度';

  @override
  String settingsUsageQuotaRemaining(String remaining, String total) {
    return '剩余 $remaining / $total';
  }

  @override
  String settingsUsageQuotaUsed(String current, String total) {
    return '已用 $current / $total';
  }

  @override
  String settingsUsagePercentRemaining(String percent) {
    return '剩余 $percent';
  }

  @override
  String settingsUsageReset(String time) {
    return '$time 重置';
  }

  @override
  String get settingsInstructionsTitle => '指令';

  @override
  String get settingsInstructionsSubtitle => '注入到每轮对话；停止输入后自动保存。';

  @override
  String get settingsBaseInstructions => 'Base instructions';

  @override
  String get settingsDeveloperInstructions => 'Developer instructions';

  @override
  String get settingsUserContext => 'User context';

  @override
  String get settingsInstructionHint => '在这里添加项目指导';

  @override
  String get settingsSkillsTitle => '技能';

  @override
  String get settingsSkillsSubtitle => '禁用过于嘈杂的 skills，或发现项目/用户/系统 skill 目录。';

  @override
  String get settingsDiscover => '发现';

  @override
  String get settingsDiscovering => '发现中';

  @override
  String get settingsFilterSkills => '过滤 skills';

  @override
  String get settingsSkillDisabled => '此工作区已禁用';

  @override
  String get settingsSkillEnabled => '已启用';

  @override
  String get settingsOpenProjectToDiscoverSkills => '打开项目以发现 skills';

  @override
  String get settingsNoSkillsMatchFilter => '没有匹配的 skills';

  @override
  String get settingsSkillsDiscoverySources =>
      'Skills 会从当前工作区以及已配置的用户/系统来源中发现。';

  @override
  String get settingsClearSearchOrDiscoverAgain => '清空搜索，或重新运行发现。';

  @override
  String get settingsNoSkillsTitle => '没有找到 skills';

  @override
  String get settingsNoSkillsMessage => '换个过滤条件，或发现当前项目的 skills。';

  @override
  String get settingsRolesTitle => '角色';

  @override
  String get settingsRolesSubtitle => '为每个固定 agent role 选择 provider/model 默认值。';

  @override
  String get settingsRoleExplorerDescription => '探索代码并收集上下文。';

  @override
  String get settingsRolePlannerDescription => '起草计划并组织意图。';

  @override
  String get settingsRoleExecutorDescription => '应用编辑并运行工具。';

  @override
  String get settingsRoleReviewerDescription => '审查结果并验证风险。';

  @override
  String get settingsRoleFallbackDescription => 'Studio role';

  @override
  String get settingsModelField => 'Model';

  @override
  String get settingsMcpTitle => 'MCP';

  @override
  String get settingsMcpSubtitle => 'Model Context Protocol servers 和内联端点。';

  @override
  String get settingsEndpoint => 'Endpoint';

  @override
  String get settingsMcpEmptyTitle => '没有 MCP servers';

  @override
  String get settingsMcpEmptyMessage => '已配置的 MCP servers 会显示在这里。';

  @override
  String get settingsSecurityTitle => '安全';

  @override
  String get settingsSecuritySubtitle => '选择此工作区默认审批姿态。';

  @override
  String get settingsSecurityModeSubtitle => '工具执行权限模式；修改会立即生效。';

  @override
  String settingsCurrentMode(String mode) {
    return '当前：$mode';
  }

  @override
  String get settingsWorkspaceBoundary => '工作区边界策略保持不变。';

  @override
  String get settingsGeneralTitle => '通用';

  @override
  String get settingsGeneralSubtitle => '界面偏好保存到 Studio store。';

  @override
  String get settingsFollowSystemTheme => '跟随系统主题';

  @override
  String get settingsFollowSystemThemeSubtitle => '随操作系统切换亮色和暗色模式。';

  @override
  String get settingsFollowActiveTurn => '跟随当前 turn';

  @override
  String get settingsFollowActiveTurnSubtitle =>
      '让新的 timeline 输出保持 pinned 到最新 turn。';

  @override
  String get settingsCompactTimeline => 'Compact timeline';

  @override
  String get settingsCompactTimelineSubtitle => '减少消息间距，适合更密集阅读。';

  @override
  String get settingsWebSearchTitle => 'Web 搜索';

  @override
  String get settingsWebSearchSubtitle =>
      '通过符合条件的 OpenAI 账户执行搜索；修改从下一次 turn 起生效。';

  @override
  String get settingsWebSearchConfiguredMode => '已配置模式';

  @override
  String get settingsWebSearchEffectiveMode => '有效模式';

  @override
  String get settingsWebSearchProvider => 'OpenAI Provider';

  @override
  String get settingsWebSearchModel => '搜索模型';

  @override
  String get settingsWebSearchMode => '模式';

  @override
  String get settingsWebSearchModeDisabled => '禁用';

  @override
  String get settingsWebSearchModeCached => '缓存';

  @override
  String get settingsWebSearchModeIndexed => '索引';

  @override
  String get settingsWebSearchModeLive => '实时';

  @override
  String get settingsWebSearchContextSize => '上下文大小';

  @override
  String get settingsWebSearchContextLow => '低';

  @override
  String get settingsWebSearchContextMedium => '中';

  @override
  String get settingsWebSearchContextHigh => '高';

  @override
  String get settingsServiceDefault => '服务默认值';

  @override
  String get settingsWebSearchAllowedDomains => '允许的域名';

  @override
  String get settingsWebSearchDomainsHint => 'example.com, docs.example.com';

  @override
  String get settingsWebSearchCountry => '国家';

  @override
  String get settingsWebSearchRegion => '地区';

  @override
  String get settingsWebSearchCity => '城市';

  @override
  String get settingsWebSearchTimezone => '时区';

  @override
  String get settingsWebSearchAvailable => '可用';

  @override
  String get settingsWebSearchDisabled => '已禁用';

  @override
  String get settingsWebSearchMissingCredential => '缺少凭证';

  @override
  String get settingsWebSearchUnsupportedModel => '模型不支持';

  @override
  String get settingsWebSearchMissingCredentialReason =>
      '没有源自 OpenAI preset 且凭证有效的 Provider，远程 Web 搜索已完全禁用。';

  @override
  String get settingsWebSearchUnsupportedModelReason =>
      '当前路由无法暴露函数工具或 hosted Web 搜索。';

  @override
  String get settingsNotAvailable => '不可用';

  @override
  String get settingsSaveWebSearch => '保存 Web 搜索';

  @override
  String get settingsStudioUpdateTitle => 'Pure Studio 更新';

  @override
  String settingsStudioUpdateDisabled(String version) {
    return '当前版本 $version。仅 Windows 正式版会自动检查更新。';
  }

  @override
  String settingsStudioUpdateCurrent(String version) {
    return '当前版本：$version';
  }

  @override
  String settingsStudioUpdateChecking(String version) {
    return '当前版本 $version，正在检查更新…';
  }

  @override
  String settingsStudioUpdateLatest(String version) {
    return '当前版本 $version 已是最新版。';
  }

  @override
  String settingsStudioUpdateAvailable(String current, String latest) {
    return '已安装 $current，可升级到 $latest。';
  }

  @override
  String settingsStudioUpdateDownloading(String version, int progress) {
    return '正在下载 $version：$progress%';
  }

  @override
  String settingsStudioUpdateVerifying(String version) {
    return '正在验证 $version…';
  }

  @override
  String settingsStudioUpdateInstallerLaunched(String version) {
    return '$version 安装程序已启动。';
  }

  @override
  String settingsStudioUpdateFailed(String error) {
    return '更新失败：$error';
  }

  @override
  String get settingsStudioUpdateBusy => '请先等待当前 turn 或 task 结束，再安装更新。';

  @override
  String get settingsStudioUpdateCheck => '检查更新';

  @override
  String get settingsStudioUpdateInstall => '下载并安装';

  @override
  String get settingsStudioUpdateReleaseNotes => '发行说明';

  @override
  String get timelineWebSearchTitle => 'Web 搜索';

  @override
  String get timelineWebSearchSearching => '正在搜索网页';

  @override
  String get timelineWebSearchOpening => '正在打开网页';

  @override
  String get timelineWebSearchFinding => '正在页内查找';

  @override
  String get timelineWebSearchResults => '结果链接';
}
