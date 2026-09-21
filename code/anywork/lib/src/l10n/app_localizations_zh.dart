// ignore: unused_import
import 'package:intl/intl.dart' as intl;

import 'app_localizations.dart';

// ignore_for_file: type=lint

/// The translations for Chinese (`zh`).
class AppLocalizationsZh extends AppLocalizations {
  AppLocalizationsZh([String locale = 'zh']) : super(locale);

  @override
  String get promptAccepted => '已受理';

  @override
  String get composerInterruptHint => '发送将打断并继续 · Esc 停止';

  @override
  String get composerSendAndContinue => '发送并继续';

  @override
  String get appTitle => '糊来帮';

  @override
  String get sidebarProjects => '项目';

  @override
  String get sidebarSessions => '会话';

  @override
  String get sidebarLoadingMore => '正在加载更多会话…';

  @override
  String get sidebarLoadError => '会话目录分页加载失败';

  @override
  String get shutdownTitle => '正在安全退出';

  @override
  String get shutdownPhaseStoppingSubscriptions => '正在停止订阅';

  @override
  String get shutdownPhaseCancellingTurns => '正在停止会话任务';

  @override
  String get shutdownPhaseFlushingPersistence => '正在保存会话';

  @override
  String get shutdownPhaseStoppingAgents => '正在停止协作智能体';

  @override
  String get shutdownPhaseStoppingMcp => '正在关闭 MCP';

  @override
  String get shutdownPhaseStoppingLsp => '正在关闭语言服务';

  @override
  String get shutdownPhaseStopped => '退出流程已完成';

  @override
  String shutdownPendingCommits(int count) {
    return '仍有 $count 项更改未保存';
  }

  @override
  String get sidebarCloseProject => '关闭项目';

  @override
  String get sidebarArchiveSession => '归档会话';

  @override
  String get sidebarArchiveSessionFailed => '无法归档该会话。';

  @override
  String get sidebarArchiveSessionBusy => '无法归档该会话，它可能仍在运行。';

  @override
  String sidebarArchiveSessionFailedReason(
    String reason,
    String correlationId,
  ) {
    return '无法归档该会话：$reason（诊断编号：$correlationId）';
  }

  @override
  String get sidebarArchiveSessionConfirmTitle => '归档会话';

  @override
  String get sidebarArchiveSessionConfirmBody =>
      '归档会先结束该会话正在运行的工作，并删除该会话树的工作树及其未提交、未整合的内容，然后将其从列表中移除。是否继续？';

  @override
  String get sidebarArchiveSessionConfirmAction => '归档';

  @override
  String get sidebarRenameSession => '重命名会话';

  @override
  String get sidebarRenameSessionTitle => '重命名会话';

  @override
  String get sidebarRenameSessionInput => '会话标题';

  @override
  String get sidebarRenameSessionEmpty => '请输入会话标题。';

  @override
  String get sidebarRenameSessionTooLong => '会话标题最多 80 个字符。';

  @override
  String get sidebarRenameSessionFailed => '无法重命名该会话。';

  @override
  String get commonCancel => '取消';

  @override
  String get commonSave => '保存';

  @override
  String get sidebarNewSession => '新建会话';

  @override
  String get sidebarSessionWorktree => '会话工作树';

  @override
  String get sidebarOpenProject => '打开项目';

  @override
  String get sidebarSettings => '设置';

  @override
  String get runtimeFatalTitle => '糊来帮无法启动';

  @override
  String get runtimeFatalRetry => '重试';

  @override
  String persistenceDegraded(int count) {
    return '保存暂时不可用，尚有 $count 项更改未保存；可以继续会话。';
  }

  @override
  String persistenceRecovering(int count) {
    return '保存已恢复，正在补存此前未保存的 $count 项更改；可以继续会话。';
  }

  @override
  String persistenceBlocked(int count) {
    return '保存已阻塞，仍有 $count 项更改未保存，需要处理；可以继续会话。';
  }

  @override
  String get persistenceRetry => '重试保存';

  @override
  String get threadUnopenedTitle => '会话尚未打开';

  @override
  String get threadUnopenedBody => '当前仅选中该会话。打开后会读取当前状态与历史；在你发送消息之前不会恢复任何执行。';

  @override
  String get threadOpenAction => '打开会话';

  @override
  String get timelineItemBodyLoad => '加载完整内容';

  @override
  String get timelineItemBodyTruncated => '本页显示的是超大条目的截断预览。';

  @override
  String get timelineItemBodyLoading => '正在加载完整内容…';

  @override
  String get timelineItemBodyPending => '完整内容尚未写入历史，请稍后重试。';

  @override
  String get timelineItemBodyRetry => '重试';

  @override
  String get timelineItemBodyUnavailable => '当前数据源无法提供完整内容。';

  @override
  String get persistenceQueueTitle => '持久化诊断';

  @override
  String get persistenceQueueUnavailable => '当前数据源无法提供队列压力。';

  @override
  String persistenceQueuePending(int operations, int bytes) {
    return '排队：$operations 项操作，$bytes 字节';
  }

  @override
  String persistenceQueueInFlight(int bytes) {
    return '正在写入：$bytes 字节';
  }

  @override
  String persistenceQueueOldestAge(int millis) {
    return '最老未保存：$millis 毫秒';
  }

  @override
  String get persistenceQueuePressurePaused => '存储压力下已暂停新的推理准入。';

  @override
  String persistenceQueueError(String message) {
    return '最近错误：$message';
  }

  @override
  String get persistenceQueueRefresh => '刷新诊断';

  @override
  String recoveryGlobalWarning(int count) {
    return '有 $count 个恢复问题需要处理';
  }

  @override
  String get sidebarNew => '新建';

  @override
  String get sidebarOpen => '打开';

  @override
  String get shellNoSession => '新会话';

  @override
  String get startPageWelcome => '想从哪里开始？';

  @override
  String startPageProject(String project) {
    return '当前项目：$project';
  }

  @override
  String get startPageOpenProjectTitle => '请先打开项目';

  @override
  String get startPageOpenProjectBody => '从侧栏打开一个项目后即可发送第一条消息。';

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
  String get settingsProvidersTab => '模型服务商';

  @override
  String get settingsInstructionsTab => '指令';

  @override
  String get settingsSkillsTab => '技能';

  @override
  String get settingsAgentsTab => '智能体';

  @override
  String get settingsMcpTab => 'MCP';

  @override
  String get settingsLspTab => 'LSP';

  @override
  String get settingsStatisticsTab => '统计';

  @override
  String get settingsSecurityTab => '安全';

  @override
  String get settingsGeneralTab => '通用';

  @override
  String get settingsSshTab => 'SSH';

  @override
  String get settingsSshTitle => '远程开发';

  @override
  String get settingsSshSubtitle =>
      '服务器列表来自你的 ~/.ssh/config；anywork 只管理自己写入的标记块。';

  @override
  String get settingsSshAdd => '添加服务器';

  @override
  String get settingsSshEmpty => '尚未配置 SSH 服务器。';

  @override
  String get settingsSshManagedByCore => 'OpenSSH 与轻量远程助手由本机统一管理。';

  @override
  String get settingsSshTest => '测试连接';

  @override
  String get settingsSshReconnect => '重新连接';

  @override
  String get settingsSshReconnectHint => '重新连接以加载最新远程环境；正在运行的远程命令会中断。';

  @override
  String get settingsSshOpenProject => '打开项目';

  @override
  String get settingsSshEdit => '编辑';

  @override
  String get settingsSshDelete => '删除';

  @override
  String get settingsSshReady => '已连接';

  @override
  String get settingsSshDeleteTitle => '删除 SSH 服务器？';

  @override
  String settingsSshDeleteBody(String name) {
    return '确定删除 $name？请先移除使用该服务器的项目。';
  }

  @override
  String get settingsSshAlias => '别名（Host）';

  @override
  String get settingsSshProjectName => '项目名称';

  @override
  String get settingsSshHost => '主机';

  @override
  String get settingsSshUsername => '用户名';

  @override
  String get settingsSshPort => '端口';

  @override
  String get settingsSshIdentityFile => '私钥文件（可选）';

  @override
  String get settingsSshIdentityHelper => '留空时使用 ssh-agent。';

  @override
  String get settingsSshReadOnlyEntry => '手写条目——请直接编辑 ~/.ssh/config。';

  @override
  String get settingsSshSave => '保存';

  @override
  String get settingsSshAliasRequired => '请输入别名';

  @override
  String get settingsSshAliasInvalid => '别名必须是单个词，不能含空格或通配字符';

  @override
  String get settingsSshAliasHelper => '写入 ~/.ssh/config 的 Host 条目；创建后不可改名。';

  @override
  String get settingsSshHostRequired => '请输入主机地址';

  @override
  String get settingsSshUsernameRequired => '请输入用户名';

  @override
  String get settingsSshPortInvalid => '端口必须是 1 到 65535 之间的数字';

  @override
  String get settingsSshChooseDirectory => '选择远端目录';

  @override
  String get settingsSshOpenThisDirectory => '打开此目录';

  @override
  String get settingsSshGo => '前往';

  @override
  String get settingsSshDirectoryPathLabel => '远端路径';

  @override
  String get settingsSshDirectoryPathHint => '/home/user/project';

  @override
  String get settingsSshPathRequired => '请输入远端目录路径';

  @override
  String get settingsSshPathAbsolute => '远端路径必须以 / 开头，且必须是绝对路径';

  @override
  String get settingsSshDirectoryEmpty => '此目录为空';

  @override
  String get settingsSshDirectoryEmptyHint => '此处没有子目录，但仍可打开当前目录。';

  @override
  String get settingsSshUp => '上一级目录';

  @override
  String get settingsSshOpenFailed => '无法打开该目录，请检查服务器后重试。';

  @override
  String get composerHint => '描述你的需求…';

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
  String get permissionModeRequestApproval => '请求授权';

  @override
  String get permissionModeAutoReview => '自动审查';

  @override
  String get permissionModeFullAccess => '完全访问';

  @override
  String get statusCost => '费用';

  @override
  String get statusTotalTokensLabel => '总词元数';

  @override
  String get statusModelLabel => '模型';

  @override
  String get statusCapabilitiesTitle => '当前启用项';

  @override
  String get statusSessionMode => '会话模式';

  @override
  String get statusSessionModeLocked => '会话运行或工作流活动期间无法切换会话模式';

  @override
  String get composerWorkspaceModeLabel => '会话工作区';

  @override
  String get composerWorkspaceModeLocal => '本地目录';

  @override
  String get composerWorkspaceModeWorktree => '新建工作树';

  @override
  String get statusPlannerModel => '主智能体模型';

  @override
  String get statusExecutorModel => '执行者模型';

  @override
  String get statusReasoningEffort => '思考强度';

  @override
  String get statusContextLabel => '上下文';

  @override
  String get statusCacheLabel => '缓存';

  @override
  String get statusCacheReportedOnlyLabel => '基于已报告数据';

  @override
  String get statusCacheHitTokensLabel => '缓存命中';

  @override
  String get statusCacheMissTokensLabel => '缓存未命中';

  @override
  String get statusCacheWriteTokensLabel => '缓存写入';

  @override
  String get statusReasoningTokensLabel => '推理词元数';

  @override
  String get statusInferenceCountLabel => '推理次数';

  @override
  String get statusCacheSavingsLabel => '缓存节省';

  @override
  String get statusUnpricedUsageLabel => '部分用量未计入费用估算';

  @override
  String get sessionAllAgentsCostTooltip => '会话全部智能体费用';

  @override
  String get sessionMoreActionsTooltip => '更多操作';

  @override
  String get sessionOpenInVsCode => '在 VS Code 中打开';

  @override
  String get sessionVsCodeOpenFailed => '打开 VS Code 失败';

  @override
  String get sessionVsCodeServerMissing => '该项目的 SSH 别名已不在 ~/.ssh/config 中';

  @override
  String get statusCurrentAgentTokenSpeed => '当前智能体词元速度';

  @override
  String get settingsStatisticsTitle => '统计';

  @override
  String get settingsStatisticsSubtitle => '按模型服务商实例、实际模型与思考强度汇总最近成功调用。';

  @override
  String get settingsStatisticsSummaryTitle => '模型性能';

  @override
  String get settingsStatisticsHistoryTitle => '调用历史';

  @override
  String get settingsStatisticsAllModels => '全部模型';

  @override
  String get settingsStatisticsEmpty => '暂无完整性能样本。';

  @override
  String get settingsStatisticsMismatchesOnly => '仅看不一致';

  @override
  String get settingsStatisticsMismatchEmpty => '当前筛选条件下没有模型不一致记录。';

  @override
  String get statisticsModel => '模型服务商 / 模型';

  @override
  String get statisticsConfiguredModel => '配置模型';

  @override
  String get statisticsSentModel => '发送模型';

  @override
  String get statisticsReportedModel => '返回模型';

  @override
  String get statisticsModelMatched => '一致';

  @override
  String get statisticsModelMismatched => '不一致';

  @override
  String get statisticsModelUnreported => '未报告';

  @override
  String get statisticsModelLegacyUnknown => '未采集';

  @override
  String get statisticsModelUnavailable => '无';

  @override
  String get statisticsReasoningEffort => '思考强度';

  @override
  String get statisticsReasoningEffortUnspecified => '未指定/未记录';

  @override
  String get statisticsSpeed => '速度';

  @override
  String get statisticsSamples => '样本数';

  @override
  String get statisticsOutputTokens => '输出词元数';

  @override
  String get statisticsAverageTtft => '平均 TTFT';

  @override
  String get statisticsAverageResponse => '平均响应时间';

  @override
  String get statisticsCompletedAt => '完成时间';

  @override
  String get statisticsDecode => '解码';

  @override
  String get statisticsTotalResponse => '总响应';

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
  String get statusTurnPersisting => '保存本轮结果';

  @override
  String get statusInteractionToolApproval => '等待工具授权';

  @override
  String get statusInteractionUserInput => '等待输入';

  @override
  String statusContextTooltip(
    int contextTokens,
    int contextWindow,
    int percent,
    int totalTokens,
    String model,
  ) {
    return '上下文：$contextTokens/$contextWindow（$percent%）\n\n总词元数：$totalTokens\n\n模型：$model';
  }

  @override
  String statusContextTooltipNoModel(
    int contextTokens,
    int contextWindow,
    int percent,
    int totalTokens,
  ) {
    return '上下文：$contextTokens/$contextWindow（$percent%）\n\n总词元数：$totalTokens';
  }

  @override
  String statusSkillsCount(int count) {
    return '$count 个技能';
  }

  @override
  String statusMcpCount(int count) {
    return '$count 个 MCP 服务器';
  }

  @override
  String statusLspCount(int count) {
    return '$count 个语言服务器';
  }

  @override
  String get statusLspIndexing => '索引中';

  @override
  String get statusLspBusy => '处理中';

  @override
  String statusLspActivityPercentage(int percentage) {
    return '$percentage%';
  }

  @override
  String statusAgentsCount(int count) {
    return '$count 个智能体';
  }

  @override
  String get composerAgentRuntimeDriven => '此智能体会话由运行时驱动';

  @override
  String get statusSkillsSection => '技能';

  @override
  String get statusMcpSection => 'MCP';

  @override
  String get statusLspSection => 'LSP';

  @override
  String get statusSubagentsSection => '子智能体';

  @override
  String get statusAgentChipTooltip => '子智能体状态';

  @override
  String get agentDetailTitle => '子智能体';

  @override
  String agentDetailSummary(int count, int running) {
    return '共 $count 个，$running 个运行中';
  }

  @override
  String get agentDetailEmpty => '暂无子智能体';

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
  String get timelineEmptyMessage => '打开项目或开始会话后即可开始。';

  @override
  String get timelineExternalLinkOpenFailed => '无法打开此链接。';

  @override
  String get timelineAttachment => '附件';

  @override
  String get timelineImageLoadFailed => '无法加载此图片。';

  @override
  String get timelineImageRetry => '重试';

  @override
  String get timelineImageClose => '关闭图片预览';

  @override
  String timelineRemoteImageSource(String host) {
    return '来自 $host 的外部图片';
  }

  @override
  String get timelineRemoteImageOpen => '点击后加载并预览';

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
  String get timelineToolFallback => '工具';

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
  String timelineSkillActivated(String name) {
    return '已激活技能 · $name';
  }

  @override
  String timelineSkillAgentActivated(String name) {
    return '智能体已激活技能 · $name';
  }

  @override
  String get timelineParentAgent => '主智能体';

  @override
  String timelineSkillUserActivated(String name) {
    return '用户激活技能 · $name';
  }

  @override
  String get timelineAgentFallback => '智能体';

  @override
  String get timelineViewImageRead => '已读取图片';

  @override
  String get timelineViewImageReading => '正在读取图片';

  @override
  String get timelineViewImageFailed => '读取图片失败';

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
  String timelineToolCancelled(String name) {
    return '$name 已取消';
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
  String get agentDetailStatusClosing => '正在关闭';

  @override
  String get agentDetailStatusCleanupFailed => '清理失败';

  @override
  String timelineToolQueued(String name) {
    return '$name 排队中';
  }

  @override
  String timelineToolCancelling(String name) {
    return '$name 正在取消';
  }

  @override
  String timelineToolInterrupted(String name) {
    return '$name 已中断';
  }

  @override
  String get timelineAgentSubagent => '子智能体';

  @override
  String get timelineAgentSubagentMessage => '子智能体消息';

  @override
  String get timelineAgentWaiting => '等待子智能体';

  @override
  String get timelineAgentClose => '关闭子智能体';

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
  String get timelineRolledBack => '已从有效上下文回退';

  @override
  String get interactionSubmitEmptyAnswersHint => '未作答的问题将留空提交。';

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
  String get interactionAnswerHint => '糊来帮会把这条回答作为当前问题的答案继续执行。';

  @override
  String get interactionAnswerButton => '回答';

  @override
  String get interactionAnswerLabel => '答案';

  @override
  String interactionQuestionProgress(int current, int total) {
    return '问题 $current/$total';
  }

  @override
  String interactionAnsweredCount(int count) {
    return '已答 $count 题';
  }

  @override
  String interactionQuestionTooltip(int index) {
    return '问题 $index';
  }

  @override
  String get interactionQuestionFallback => '问题';

  @override
  String get interactionOtherLabel => '其他';

  @override
  String get interactionSecretHint => '输入敏感信息';

  @override
  String get interactionTextHint => '输入你的回答…';

  @override
  String get interactionPermissionTitle => '需要权限';

  @override
  String get interactionPermissionSubtitle => '糊来帮希望调用以下工具';

  @override
  String get interactionPermissionFooterHint => '工具将在当前工作目录执行；可在输入区调整权限模式。';

  @override
  String get interactionReject => '拒绝';

  @override
  String get interactionApprove => '批准';

  @override
  String get interactionReasonLabel => '原因';

  @override
  String get interactionPlanConfirmTitle => '确认此计划？';

  @override
  String get interactionPlanConfirmSubtitle => '确认计划，或直接写下调整要求';

  @override
  String get interactionPlanReadyTitle => '实施计划已准备好';

  @override
  String get interactionPlanAwaitingConfirmation => '等待确认';

  @override
  String get interactionPlanViewDetails => '查看完整计划';

  @override
  String get interactionPlanDetailsTitle => '实施计划';

  @override
  String get interactionPlanComposerPausedHint => '普通消息输入已暂停，避免与计划反馈混淆。';

  @override
  String interactionPlanConfirmFooterHint(String mode) {
    return '确认后将在$mode模式进入文档编辑检查点。';
  }

  @override
  String get interactionPlanAdjust => '告诉糊来帮如何调整';

  @override
  String get interactionPlanConfirmAction => '确认并执行';

  @override
  String get interactionPlanAdjustHint => '输入要调整的要求…';

  @override
  String get interactionPlanAdjustSubmit => '提交修改';

  @override
  String get settingsProvidersTitle => '模型服务商';

  @override
  String get settingsProvidersSubtitle => '模型服务、凭据、模型和用量';

  @override
  String get settingsRefreshUsage => '刷新用量';

  @override
  String get settingsAddProvider => '添加模型服务商';

  @override
  String get settingsSearchProviders => '搜索模型服务商';

  @override
  String get settingsNoProvidersMatchTitle => '没有匹配的模型服务商';

  @override
  String get settingsNoProvidersMatchMessage => '清空搜索以查看所有已配置的模型服务商。';

  @override
  String get settingsNoProvidersTitle => '没有模型服务商';

  @override
  String get settingsNoProvidersMessage => '添加模型服务商后可配置凭据和模型。';

  @override
  String get settingsDefaultProvider => '默认模型服务商';

  @override
  String get settingsSetAsDefaultProvider => '设为默认';

  @override
  String get settingsOpenDetails => '打开详情';

  @override
  String get settingsProviderActions => '模型服务商操作';

  @override
  String get settingsEditProvider => '编辑模型服务商';

  @override
  String get settingsDeleteProvider => '删除模型服务商';

  @override
  String get settingsNoProviderSelected => '未选择模型服务商';

  @override
  String get settingsProviderTitle => '模型服务商信息';

  @override
  String get settingsProviderModelsTitle => '模型';

  @override
  String get settingsProviderConnectionTitle => '连接';

  @override
  String get settingsProviderDefaultModelsTitle => '默认模型';

  @override
  String get settingsProviderCustomModelsTitle => '自定义模型';

  @override
  String get settingsNewProvider => '新建模型服务商';

  @override
  String get settingsProviderKey => '服务商标识';

  @override
  String get settingsTemplate => '模板';

  @override
  String get settingsCustomProvider => '自定义模型服务商';

  @override
  String get settingsDefaultModel => '默认模型';

  @override
  String get settingsApiKey => 'API 密钥';

  @override
  String get settingsApiKeyKeepCurrent => 'API 密钥（留空以保留当前值）';

  @override
  String get settingsConfigured => '已配置';

  @override
  String get settingsMissing => '缺失';

  @override
  String get settingsDisplayName => '显示名称';

  @override
  String get settingsProtocolType => '协议类型';

  @override
  String get settingsBaseUrl => '基础 URL';

  @override
  String get settingsModelSlug => '模型标识';

  @override
  String get settingsReasoningEfforts => '思考强度';

  @override
  String get settingsEdit => '编辑';

  @override
  String get settingsCancel => '取消';

  @override
  String get settingsSave => '保存';

  @override
  String get settingsAddModel => '添加模型';

  @override
  String get settingsRemoveModel => '移除模型';

  @override
  String get settingsNoCustomModels => '没有自定义模型';

  @override
  String settingsBundledModels(int count) {
    return '$count 个内置模型';
  }

  @override
  String get settingsDefaultBadge => '默认';

  @override
  String get settingsReadyBadge => '就绪';

  @override
  String get settingsSetupBadge => '待配置';

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
  String get settingsUsageRefreshing => '正在刷新用量…';

  @override
  String get settingsUsageChecking => '正在检查用量…';

  @override
  String get settingsUsageCheckingShort => '正在检查用量';

  @override
  String get settingsUsageNotLoaded => '用量未加载';

  @override
  String get settingsUsageUnsupported => '不支持';

  @override
  String get settingsUsageNotSupported => '不支持用量查询';

  @override
  String get settingsUsageMissingKey => '缺少密钥';

  @override
  String get settingsUsageFailed => '用量查询失败';

  @override
  String get settingsUsageQueryFailed => '用量查询失败';

  @override
  String get settingsUsageApiKeyMissing => '未配置模型服务商 API 密钥';

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
  String get settingsUsageTools => '工具';

  @override
  String get settingsUsageToken => '词元用量';

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
    return '将于 $time 重置';
  }

  @override
  String get settingsInstructionsTitle => '指令';

  @override
  String get settingsInstructionsSubtitle => '注入到每轮对话；停止输入后自动保存。';

  @override
  String get settingsBaseInstructions => '基础指令';

  @override
  String get settingsDeveloperInstructions => '开发者指令';

  @override
  String get settingsUserContext => '用户上下文';

  @override
  String get settingsInstructionHint => '在这里添加项目指导';

  @override
  String get settingsSkillsTitle => '技能';

  @override
  String get settingsSkillsSubtitle => '禁用干扰较多的技能，或发现项目、用户与系统技能目录。';

  @override
  String get settingsDiscover => '发现';

  @override
  String get settingsDiscovering => '正在发现';

  @override
  String get settingsFilterSkills => '过滤技能';

  @override
  String get settingsOpenProjectToDiscoverSkills => '打开项目以发现技能';

  @override
  String get settingsNoSkillsMatchFilter => '没有匹配的技能';

  @override
  String get settingsSkillsDiscoverySources => '技能会从当前工作区以及已配置的用户/系统来源中发现。';

  @override
  String get settingsClearSearchOrDiscoverAgain => '清空搜索，或重新发现技能。';

  @override
  String get settingsNoSkillsTitle => '没有找到技能';

  @override
  String get settingsNoSkillsMessage => '换个过滤条件，或重新发现技能。';

  @override
  String get settingsRoleExplorerDescription => '探索代码并收集上下文。';

  @override
  String get settingsRolePlannerDescription => '理解需求、制定计划、协调子代理、整合与验证结果。';

  @override
  String get settingsRoleExecutorDescription => '落实修改并运行工具。';

  @override
  String get settingsRoleWorktreeExecutorDescription =>
      '在隔离的 Git 工作树中落实修改并运行工具。';

  @override
  String get settingsRoleReviewerDescription => '审查结果并验证风险。';

  @override
  String get settingsRoleFallbackDescription => '智能体角色';

  @override
  String get settingsModelField => '模型';

  @override
  String get settingsMcpTitle => 'MCP';

  @override
  String get settingsMcpSubtitle => 'MCP 服务器与内联端点。';

  @override
  String get settingsMcpRefresh => '刷新';

  @override
  String get settingsMcpReconnect => '重新连接';

  @override
  String get settingsMcpResetAll => '全部重置';

  @override
  String get settingsMcpResetConfirmTitle => '重置全部 MCP 服务器？';

  @override
  String get settingsMcpResetConfirmBody =>
      '将重新构建所有已配置的 MCP 连接；当前进行中的任务继续使用重置前的连接。';

  @override
  String get settingsMcpResetConfirmAction => '全部重置';

  @override
  String get settingsEndpoint => '端点';

  @override
  String get settingsMcpEmptyTitle => '没有 MCP 服务器';

  @override
  String get settingsMcpEmptyMessage => '已配置的 MCP 服务器会显示在这里。';

  @override
  String get settingsLspTitle => '语言服务器';

  @override
  String get settingsLspSubtitle => '显示项目最近一次语言服务器状态，可刷新、探测、修复或重置。';

  @override
  String get settingsLspRefresh => '刷新';

  @override
  String get settingsLspProbe => '探测';

  @override
  String get settingsLspRepair => '修复';

  @override
  String get settingsLspReset => '重置';

  @override
  String get settingsLspResetWorkspace => '重置工作区';

  @override
  String get settingsLspActivityIndexing => '正在索引';

  @override
  String get settingsLspActivityBusy => '忙碌';

  @override
  String get settingsLspEmptyTitle => '没有语言服务器';

  @override
  String get settingsLspEmptyMessage => '打开受支持的项目后，系统会自动关联相应的语言服务器。';

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
  String get settingsGeneralSubtitle => '界面偏好保存到本地设置存储。';

  @override
  String get settingsFollowActiveTurn => '跟随当前轮次';

  @override
  String get settingsFollowActiveTurnSubtitle => '让新的时间线输出始终停留在最新轮次。';

  @override
  String get settingsCompactTimeline => '紧凑时间线';

  @override
  String get settingsCompactTimelineSubtitle => '减少消息间距，适合更密集阅读。';

  @override
  String get settingsWebSearchTitle => '网页搜索';

  @override
  String get settingsWebSearchSubtitle => '通过符合条件的 OpenAI 账户执行搜索；修改从下一轮次起生效。';

  @override
  String get settingsWebSearchConfiguredMode => '已配置模式';

  @override
  String get settingsWebSearchEffectiveMode => '有效模式';

  @override
  String get settingsWebSearchProvider => 'OpenAI 模型服务商';

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
  String get settingsWebSearchAvailableNotSelected => '可用但未选中';

  @override
  String get settingsWebSearchDisabled => '已禁用';

  @override
  String get settingsWebSearchMissingCredential => '缺少凭证';

  @override
  String get settingsWebSearchUnsupportedProvider => '模型服务商不支持';

  @override
  String get settingsWebSearchUnsupportedModel => '模型不支持';

  @override
  String get settingsWebSearchMissingCredentialReason =>
      '没有从 OpenAI 预设创建且凭证有效的模型服务商，远程网页搜索已完全禁用。';

  @override
  String get settingsWebSearchUnsupportedProviderReason =>
      '当前模型服务商未提供这一路网页搜索后端。';

  @override
  String get settingsWebSearchUnsupportedModelReason => '当前路由无法提供函数工具或托管网页搜索。';

  @override
  String get settingsNotAvailable => '不可用';

  @override
  String get settingsSaveWebSearch => '保存网页搜索';

  @override
  String get settingsDeepSeekWebSearchTitle => 'DeepSeek 原生联网搜索';

  @override
  String get settingsDeepSeekWebSearchSubtitle =>
      '使用当前符合条件的 DeepSeek 接口执行联网搜索；当该接口不可用时，将回退到 OpenAI。';

  @override
  String get settingsDeepSeekWebSearchConfigured => '已配置';

  @override
  String get settingsDeepSeekWebSearchEffective => '当前有效';

  @override
  String get settingsDeepSeekWebSearchEnabled => '已启用';

  @override
  String get settingsStudioUpdateTitle => '糊来帮更新';

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
  String get settingsStudioUpdateBusy => '请先等待当前轮次或任务结束，再安装更新。';

  @override
  String get settingsStudioUpdateCheck => '检查更新';

  @override
  String get settingsStudioUpdateInstall => '下载并安装';

  @override
  String get settingsStudioUpdateReleaseNotes => '发行说明';

  @override
  String get timelineWebSearchTitle => '网页搜索';

  @override
  String get timelineWebSearchSearching => '正在搜索网页';

  @override
  String get timelineWebSearchOpening => '正在打开网页';

  @override
  String get timelineWebSearchFinding => '正在页内查找';

  @override
  String get timelineWebSearchResults => '结果链接';

  @override
  String get timelineLspQueryTitle => 'LSP 查询';

  @override
  String timelineLspQueryTitleWithDetail(String detail) {
    return 'LSP 查询 · $detail';
  }

  @override
  String get timelineLspCapabilitiesTitle => 'LSP 能力';

  @override
  String get modalityText => '文本';

  @override
  String get modalityImage => '视觉';

  @override
  String get modalityAudio => '音频';

  @override
  String get modalityVideo => '视频';

  @override
  String get modalityFile => '文件';

  @override
  String get composerAddUrlTitle => '添加 URL';

  @override
  String get composerAddUrlConfirm => '添加';

  @override
  String get composerAttachmentAddTooltip => '添加附件';

  @override
  String get composerAttachmentUnsupportedTooltip => '当前模型不支持附件';

  @override
  String get composerAttachmentPickLocal => '选择本地文件';

  @override
  String get composerAttachmentRemoveTooltip => '移除';

  @override
  String get composerClipboardImageUnsupported => '当前模型不支持粘贴图片。';

  @override
  String get composerClipboardReadFailed => '无法读取剪贴板图片。';

  @override
  String settingsModelOutputCapabilities(String capabilities) {
    return '输出：$capabilities';
  }

  @override
  String get settingsAgentsTitle => '智能体配置';

  @override
  String get settingsAgentsSubtitle =>
      '子代理可按用途配置和启用；目录模式仅限制内置工具的文件写入，不是操作系统沙箱。';

  @override
  String get settingsAgentsAddUserProfile => '添加用户智能体配置';

  @override
  String get settingsAgentsEditTooltip => '编辑';

  @override
  String get settingsAgentsRecoveryTitle => '恢复';

  @override
  String settingsWorktreeBase(String commit) {
    return '基线提交 $commit';
  }

  @override
  String settingsWorktreeHead(String commit) {
    return '当前提交 $commit';
  }

  @override
  String get settingsWorktreeHeadUnavailable => '当前提交暂不可用';

  @override
  String settingsWorktreeChangedFiles(String files) {
    return '变更文件：$files';
  }

  @override
  String get settingsWorktreeCleanup => '显式清理工作树与分支';

  @override
  String get settingsWorktreeOwnerSession => '会话工作树';

  @override
  String get settingsWorktreeOwnerChild => '子智能体工作树';

  @override
  String get settingsAgentProfileAddTitle => '添加用户智能体配置';

  @override
  String get settingsAgentProfileEditTitle => '编辑用户智能体配置';

  @override
  String get settingsAgentProfileIdField => '智能体 ID';

  @override
  String get settingsAgentProfileDisplayNameField => '显示名称';

  @override
  String get settingsAgentProfileDescriptionField => '介绍';

  @override
  String get settingsAgentProfileWhenToUseField => '适用任务';

  @override
  String get settingsAgentProfileInstructionsField => '系统指令';

  @override
  String get settingsAgentProfileProviderField => '模型服务商';

  @override
  String get settingsAgentProfileEffortDefault => '使用模型默认值';

  @override
  String get settingsAgentProfileWorkspaceModeField => '工作区模式';

  @override
  String get settingsAgentWorkspaceModeUnrestricted => '不受限';

  @override
  String get settingsAgentWorkspaceModeDirectory => '目录';

  @override
  String get settingsAgentWorkspaceModeWorktree => '工作树';

  @override
  String get settingsAgentProfileWorkspaceDirectoryHint =>
      '目录模式只约束项目内由内置工具执行的文件写入，并非操作系统级沙箱；命令行操作、Git 和 MCP 仍可能绕过此限制。';

  @override
  String get settingsAgentProfileEnabledTitle => '启用';

  @override
  String get settingsAgentProfileEnabledSubtitle =>
      '禁用后仍保留 TOML 文件，但不会出现在智能体工具目录中。';

  @override
  String get settingsAgentProfileSave => '原子保存 TOML';

  @override
  String get settingsAgentProfileRequired => '必填';

  @override
  String get settingsServiceCapabilitiesTitle => '服务能力';

  @override
  String get settingsCapabilitySourceField => '能力来源';

  @override
  String get settingsCapabilitySourcePreset => '跟随预设默认';

  @override
  String get settingsCapabilitySourceExplicit => '显式覆盖';

  @override
  String get settingsHostedWebSearchField => '托管网页搜索';

  @override
  String get settingsHostedWebSearchDialectField => '托管网页搜索方言';

  @override
  String get settingsStandaloneWebSearchField => '独立网页搜索';

  @override
  String get settingsProgrammaticToolCallingField => '程序化工具调用';

  @override
  String get settingsCapabilityEnabled => '已启用';

  @override
  String get settingsCapabilityDisabled => '已禁用';

  @override
  String get settingsDefaultConnectionField => '默认连接';

  @override
  String get settingsCurrentConnectionField => '当前连接';

  @override
  String get settingsSupportedConnectionsLabel => '支持的连接';

  @override
  String get settingsAgentRoutesTitle => '系统智能体模型路由';

  @override
  String get settingsStateChecking => '检查中';

  @override
  String get settingsStateAvailable => '可用';

  @override
  String get settingsStateUnavailable => '不可用';

  @override
  String get settingsStateDisabled => '已禁用';

  @override
  String get settingsMcpStateMissingCredential => '缺少凭据';

  @override
  String get settingsLspActivityIdle => '空闲';

  @override
  String get roleExplorer => '探索者';

  @override
  String get rolePlanner => '主智能体';

  @override
  String get roleExecutor => '执行者';

  @override
  String get roleWorktreeExecutor => '工作树执行者';

  @override
  String get roleReviewer => '审查者';

  @override
  String get roleEmpty => '智能体';

  @override
  String get settingsPricingEnabled => '估算词元费用';

  @override
  String get settingsPricingHelp => '仅控制应用内费用估算，词元与缓存统计照常。费用估算默认关闭。';

  @override
  String get settingsModelAdvanced => '模型可选设置';

  @override
  String get settingsContextBudget => '上下文预算（词元）';

  @override
  String get settingsOutputBudget => '最大输出（词元）';

  @override
  String get settingsPriceInput => '输入';

  @override
  String get settingsPriceOutput => '输出';

  @override
  String get settingsPriceCacheRead => '缓存读取';

  @override
  String get settingsPriceCacheWrite => '缓存写入';

  @override
  String get statusReportedUsageOnly => '仅显示已报告用量';

  @override
  String get settingsSystemAgentsGroup => '系统子代理';

  @override
  String get settingsUserAgentsGroup => '用户子代理';

  @override
  String get settingsAppearanceGroup => '界面与会话';

  @override
  String get settingsNetworkGroup => '搜索与联网';

  @override
  String get settingsUpdatesGroup => '应用更新';

  @override
  String get settingsModelsGroup => '模型与智能体';

  @override
  String get settingsExtensionsGroup => '工具与服务';

  @override
  String get settingsPreferencesGroup => '偏好设置';

  @override
  String get settingsFilterAgents => '筛选智能体';

  @override
  String get settingsPermissionRequestDescription => '工具操作需要授权时请求确认。';

  @override
  String get settingsPermissionReviewDescription => '通过自动审查评估工具操作。';

  @override
  String get settingsPermissionFullDescription => '在已配置的运行时策略下，以完整访问权限执行工具。';

  @override
  String get timelineToolArguments => '参数';

  @override
  String get timelineToolOutput => '输出';

  @override
  String get costPurposeMain => '主对话';

  @override
  String get costPurposeSummary => '摘要';

  @override
  String get costPurposeReview => '审查';

  @override
  String get costPurposeTitle => '标题';

  @override
  String get costPurposeUnknown => '未知用途';

  @override
  String get timelineRawRecord => '原始历史记录';

  @override
  String get sidebarSearch => '搜索项目与会话';

  @override
  String get sidebarAll => '全部';

  @override
  String get sidebarRunning => '进行中';

  @override
  String get sidebarAttention => '待处理';

  @override
  String get sidebarAddProject => '添加项目';

  @override
  String get sidebarLocalProject => '本地项目';

  @override
  String get sidebarLocalHint => '选择这台电脑上的文件夹';

  @override
  String get sidebarRemoteProject => '远程项目';

  @override
  String get sidebarRemoteHint => '通过 SSH 连接远程工作目录';

  @override
  String get sidebarContinue => '继续';

  @override
  String get sidebarBack => '返回';

  @override
  String get sidebarConnect => '连接并选择目录';

  @override
  String get sidebarSaveConnect => '保存并连接';

  @override
  String get sidebarSearchConnections => '搜索已配置连接';

  @override
  String get sidebarNoConnections => '还没有远程配置';

  @override
  String get sidebarNewConnection => '新建远程配置';

  @override
  String get sidebarNoResults => '没有匹配的项目或会话';

  @override
  String get sidebarEmptyProject => '还没有会话，点击项目右侧 ＋ 开始';

  @override
  String get sidebarEarlier => '查看更早会话';

  @override
  String get sidebarArchived => '已归档';

  @override
  String get sidebarRestore => '恢复会话';

  @override
  String get sidebarPin => '置顶';

  @override
  String get sidebarUnpin => '取消置顶';

  @override
  String get sidebarNavigation => '项目导航';

  @override
  String get sidebarResize => '调整侧栏宽度；方向键调整，Home 恢复默认';

  @override
  String get sidebarLocal => '本地';

  @override
  String get sidebarNotChecked => '未检测';

  @override
  String get sidebarConnectionSaved => '配置已保存；连接失败，可修改后重试';

  @override
  String get sidebarCopyPath => '复制路径';

  @override
  String get sidebarRetry => '重试';

  @override
  String get settingsSshRemoteHelper => '远程助手';

  @override
  String statusThroughputValue(String value) {
    return '$value 词元/秒';
  }

  @override
  String get statusThroughputUnavailable => '—';

  @override
  String get workflowStatePlanning => '规划';

  @override
  String get workflowStateEditingDocuments => '编辑文档';

  @override
  String get workflowStateWorking => '实施';

  @override
  String get workflowStateIntegrating => '集成';

  @override
  String get workflowStateReviewing => '审查';

  @override
  String get workflowStateCompleted => '已完成';

  @override
  String get workflowStateStopped => '已停止';

  @override
  String get workflowModeSimple => '简洁';

  @override
  String get workflowModeTask => '任务';

  @override
  String get attachmentFallback => '附件';

  @override
  String get timelineAgentTimedOut => '已超时';

  @override
  String timelineToolUnknownStatus(String name, String status) {
    return '$name · $status';
  }

  @override
  String get timelineTodoStatusUnknown => '未知';

  @override
  String get settingsApiKeyOptional => 'API 密钥（可选）';

  @override
  String get modelProtocolResponses => 'Responses（响应式接口）';

  @override
  String get modelProtocolChatCompletions => 'Chat Completions（对话补全）';

  @override
  String get modelConnectionWebSocket => 'WS（WebSocket 长连接）';

  @override
  String get modelConnectionHttp => 'HTTP';

  @override
  String get settingsProtocolChatCompletionsHttp =>
      'Chat Completions（对话补全，HTTP）';

  @override
  String get settingsProtocolResponsesHttp => 'Responses（响应式接口，HTTP）';

  @override
  String settingsAgentRouteUnavailable(String route) {
    return '$route（不可用）';
  }

  @override
  String settingsPriceTierUnit(String currency) {
    return '每 100 万词元 $currency';
  }

  @override
  String get threadStatusIdle => '空闲';

  @override
  String get threadStatusCancelling => '正在停止当前执行';

  @override
  String get threadStatusClosed => '已关闭';

  @override
  String get toolStatusAwaitingApproval => '等待授权';

  @override
  String get toolStatusFailed => '失败';

  @override
  String get toolStatusDenied => '已拒绝';

  @override
  String get toolStatusCancelled => '已取消';

  @override
  String get toolStatusCancelling => '正在取消';

  @override
  String get settingsProviderCatalogUnavailable => '无法获取模型服务商目录。';

  @override
  String get settingsProviderPresetUnavailable => '无法获取模型服务商预设。';

  @override
  String get settingsProviderMissingCredential => '缺少凭据';

  @override
  String get settingsSshStateDisconnected => '未连接';

  @override
  String get settingsSshStateConnecting => '连接中';

  @override
  String get settingsSshStateWaitingForInput => '等待输入';

  @override
  String get settingsSshStateReconnecting => '重新连接中';

  @override
  String get settingsSshStateFailed => '连接失败';

  @override
  String get settingsWorktreeStatePrepared => '已准备';

  @override
  String get settingsWorktreeStateActive => '使用中';

  @override
  String get settingsWorktreeStatePreserved => '已保留';

  @override
  String get settingsWorktreeStateCleanupRequested => '已请求清理';

  @override
  String get settingsWorktreeStateCleaned => '已清理';

  @override
  String get settingsWorktreeDirty => '有改动';

  @override
  String get startupPreparing => '正在准备应用…';

  @override
  String get startupStorage => '正在打开本地数据…';

  @override
  String get startupConfiguration => '正在读取配置…';

  @override
  String get startupProjects => '正在读取项目…';

  @override
  String get startupResources => '正在准备技能资源…';

  @override
  String get startupState => '正在加载工作区…';

  @override
  String get workspaceLoading => '正在加载会话历史…';

  @override
  String get recoveryChecking => '正在检查历史会话和工作区…';

  @override
  String get recoveryCheckFailed => '历史会话和工作区检查未完成';

  @override
  String get agentRoleRetired => '该子代理角色已停用，仅可查看历史记录。';
}
