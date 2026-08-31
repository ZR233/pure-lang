// ignore: unused_import
import 'package:intl/intl.dart' as intl;

import 'app_localizations.dart';

// ignore_for_file: type=lint

/// The translations for English (`en`).
class AppLocalizationsEn extends AppLocalizations {
  AppLocalizationsEn([String locale = 'en']) : super(locale);

  @override
  String get appTitle => 'Pure Studio';

  @override
  String get sidebarProjects => 'Projects';

  @override
  String get sidebarSessions => 'Sessions';

  @override
  String get sidebarLoadingMore => 'Loading more sessions…';

  @override
  String get sidebarLoadError => 'Failed to load more sessions';

  @override
  String get shutdownTitle => 'Shutting down safely';

  @override
  String get shutdownPhaseStoppingSubscriptions => 'Stopping subscriptions';

  @override
  String get shutdownPhaseCancellingTurns => 'Stopping active turns';

  @override
  String get shutdownPhaseFlushingPersistence => 'Saving sessions';

  @override
  String get shutdownPhaseStoppingAgents => 'Stopping collaborative agents';

  @override
  String get shutdownPhaseStoppingMcp => 'Stopping MCP servers';

  @override
  String get shutdownPhaseStoppingLsp => 'Stopping language servers';

  @override
  String get shutdownPhaseStopped => 'Shutdown complete';

  @override
  String shutdownPendingCommits(int count) {
    return '$count commits pending';
  }

  @override
  String get sidebarCloseProject => 'Close project';

  @override
  String get sidebarArchiveSession => 'Archive session';

  @override
  String get sidebarArchiveSessionFailed =>
      'Could not archive this session. It may still be running.';

  @override
  String get sidebarNewSession => 'New session';

  @override
  String get sidebarOpenProject => 'Open project';

  @override
  String get sidebarSettings => 'Settings';

  @override
  String get runtimeFatalTitle => 'Pure Studio could not start';

  @override
  String get runtimeFatalRetry => 'Retry';

  @override
  String get configRecoveryMessage =>
      'An incompatible configuration was backed up and replaced with current defaults.';

  @override
  String configRecoveryBackupPath(String path) {
    return 'Backup: $path';
  }

  @override
  String get configRecoveryDismissTooltip =>
      'Dismiss configuration recovery notice';

  @override
  String persistenceDegraded(int count) {
    return 'Saving is temporarily unavailable. $count in-memory update(s) are waiting; new work is paused.';
  }

  @override
  String persistenceRecovering(int count) {
    return 'Saving has recovered and is flushing $count pending update(s). New work remains paused.';
  }

  @override
  String persistenceBlocked(int count) {
    return 'Saving is blocked with $count pending update(s) and needs attention. New work is paused.';
  }

  @override
  String get persistenceRetry => 'Retry saving';

  @override
  String recoveryGlobalWarning(int count) {
    return '$count recovery issue(s) need attention';
  }

  @override
  String get sidebarNew => 'New';

  @override
  String get sidebarOpen => 'Open';

  @override
  String get shellNoSession => 'New session';

  @override
  String get startPageWelcome => 'What can I help you build?';

  @override
  String startPageProject(String project) {
    return 'Working in $project';
  }

  @override
  String get startPageOpenProjectTitle => 'Open a project to get started';

  @override
  String get startPageOpenProjectBody =>
      'Choose a project from the sidebar before sending your first message.';

  @override
  String shellSessionUpdated(String mode, String time) {
    return '$mode · updated $time';
  }

  @override
  String get settingsBack => 'Back';

  @override
  String get settingsBackToChat => 'Back to chat';

  @override
  String get settingsWorkspaceGroup => 'Workspace';

  @override
  String get settingsSystemGroup => 'System';

  @override
  String get settingsProvidersTab => 'Providers';

  @override
  String get settingsInstructionsTab => 'Instructions';

  @override
  String get settingsSkillsTab => 'Skills';

  @override
  String get settingsRolesTab => 'Roles';

  @override
  String get settingsAgentsTab => 'Agents';

  @override
  String get settingsMcpTab => 'MCP';

  @override
  String get settingsLspTab => 'LSP';

  @override
  String get settingsStatisticsTab => 'Statistics';

  @override
  String get settingsSecurityTab => 'Security';

  @override
  String get settingsGeneralTab => 'General';

  @override
  String get settingsSshTab => 'SSH';

  @override
  String get settingsSshTitle => 'Remote development';

  @override
  String get settingsSshSubtitle =>
      'Manage SSH workspaces. Connections and helper lifecycle are owned by the local core.';

  @override
  String get settingsSshAdd => 'Add server';

  @override
  String get settingsSshEmpty => 'No SSH servers yet.';

  @override
  String get settingsSshManagedByCore =>
      'OpenSSH and the minimal remote helper are managed locally.';

  @override
  String get settingsSshTest => 'Test connection';

  @override
  String get settingsSshReconnect => 'Reconnect';

  @override
  String get settingsSshOpenProject => 'Open project';

  @override
  String get settingsSshEdit => 'Edit';

  @override
  String get settingsSshDelete => 'Delete';

  @override
  String get settingsSshReady => 'Ready';

  @override
  String get settingsSshDeleteTitle => 'Delete SSH server?';

  @override
  String settingsSshDeleteBody(String name) {
    return 'Delete $name? Projects using this server must be removed first.';
  }

  @override
  String get settingsSshName => 'Name';

  @override
  String get settingsSshHost => 'Host';

  @override
  String get settingsSshUsername => 'Username';

  @override
  String get settingsSshPort => 'Port';

  @override
  String get settingsSshAuth => 'Authentication';

  @override
  String get settingsSshAuthAgentOrKey => 'SSH agent or key';

  @override
  String get settingsSshAuthPassword => 'Password';

  @override
  String get settingsSshIdentityFile => 'Identity file (optional)';

  @override
  String get settingsSshPassword => 'Password';

  @override
  String get settingsSshPasswordLease =>
      'Kept in core memory for this app session only.';

  @override
  String get settingsSshSave => 'Save';

  @override
  String get settingsSshNameRequired => 'Enter a server name';

  @override
  String get settingsSshHostRequired => 'Enter a host address';

  @override
  String get settingsSshUsernameRequired => 'Enter a username';

  @override
  String get settingsSshPortInvalid => 'Port must be a number from 1 to 65535';

  @override
  String get settingsSshChooseDirectory => 'Choose remote directory';

  @override
  String get settingsSshOpenThisDirectory => 'Open this directory';

  @override
  String get composerHint => 'Describe what you need...';

  @override
  String get composerSend => 'Send';

  @override
  String get composerStop => 'Stop';

  @override
  String get permissionModeTooltip => 'Permission mode';

  @override
  String get compileModeSimple => 'Simple';

  @override
  String get compileModeTask => 'Task';

  @override
  String get permissionModeRequestApproval => 'Request';

  @override
  String get permissionModeAutoReview => 'Review';

  @override
  String get permissionModeFullAccess => 'Full';

  @override
  String get statusCost => 'Cost';

  @override
  String get statusTotalTokensLabel => 'Total tokens';

  @override
  String get statusModelLabel => 'Model';

  @override
  String get statusCapabilitiesTitle => 'Active capabilities';

  @override
  String get statusSessionMode => 'Session mode';

  @override
  String get statusSessionModeLocked =>
      'Session mode cannot change while the session is running or a workflow is active';

  @override
  String get statusPlannerModel => 'Planner model';

  @override
  String get statusExecutorModel => 'Executor model';

  @override
  String get statusReasoningEffort => 'Reasoning effort';

  @override
  String get statusContextLabel => 'Context';

  @override
  String get statusCacheLabel => 'Cache';

  @override
  String get statusCacheHitTokensLabel => 'Cache read';

  @override
  String get statusCacheMissTokensLabel => 'Cache miss';

  @override
  String get statusCacheWriteTokensLabel => 'Cache write';

  @override
  String get statusReasoningTokensLabel => 'Reasoning tokens';

  @override
  String get statusInferenceCountLabel => 'Inferences';

  @override
  String get statusCacheSavingsLabel => 'Cache savings';

  @override
  String get statusUnpricedUsageLabel => 'Partially unpriced';

  @override
  String get sessionAllAgentsCostTooltip =>
      'Cost for all agents in this session';

  @override
  String get statusCurrentAgentTokenSpeed => 'Current agent token speed';

  @override
  String get settingsStatisticsTitle => 'Statistics';

  @override
  String get settingsStatisticsSubtitle =>
      'Recent successful model calls, grouped by provider instance and actual model.';

  @override
  String get settingsStatisticsSummaryTitle => 'Model performance';

  @override
  String get settingsStatisticsHistoryTitle => 'Call history';

  @override
  String get settingsStatisticsAllModels => 'All models';

  @override
  String get settingsStatisticsEmpty => 'No complete performance samples yet.';

  @override
  String get statisticsModel => 'Provider / model';

  @override
  String get statisticsSpeed => 'Speed';

  @override
  String get statisticsSamples => 'Samples';

  @override
  String get statisticsOutputTokens => 'Output tokens';

  @override
  String get statisticsAverageTtft => 'Average TTFT';

  @override
  String get statisticsAverageResponse => 'Average response';

  @override
  String get statisticsCompletedAt => 'Completed';

  @override
  String get statisticsDecode => 'Decode';

  @override
  String get statisticsTotalResponse => 'Total';

  @override
  String get statusTurnQueued => 'Queued';

  @override
  String get statusTurnPreparing => 'Preparing context';

  @override
  String get statusTurnResponding => 'Responding';

  @override
  String get statusTurnPlanning => 'Planning';

  @override
  String get statusTurnRunningTool => 'Running tool';

  @override
  String get statusTurnWaitingForApproval => 'Waiting for tool approval';

  @override
  String get statusTurnWaitingForUserInput => 'Waiting for input';

  @override
  String get statusTurnPersisting => 'Saving turn';

  @override
  String get statusInteractionToolApproval => 'Waiting for tool approval';

  @override
  String get statusInteractionUserInput => 'Waiting for input';

  @override
  String statusContextTooltip(
    int contextTokens,
    int contextWindow,
    int percent,
    int totalTokens,
    String model,
  ) {
    return 'Context: $contextTokens/$contextWindow ($percent%)\n\nTotal tokens: $totalTokens\n\nModel: $model';
  }

  @override
  String statusContextTooltipNoModel(
    int contextTokens,
    int contextWindow,
    int percent,
    int totalTokens,
  ) {
    return 'Context: $contextTokens/$contextWindow ($percent%)\n\nTotal tokens: $totalTokens';
  }

  @override
  String statusSkillsCount(int count) {
    String _temp0 = intl.Intl.pluralLogic(
      count,
      locale: localeName,
      other: '$count skills',
      one: '1 skill',
    );
    return '$_temp0';
  }

  @override
  String statusMcpCount(int count) {
    return '$count MCP';
  }

  @override
  String statusLspCount(int count) {
    return '$count LSP';
  }

  @override
  String get statusLspIndexing => 'Indexing';

  @override
  String get statusLspBusy => 'Working';

  @override
  String statusLspActivityPercentage(int percentage) {
    return '$percentage%';
  }

  @override
  String statusAgentsCount(int count) {
    String _temp0 = intl.Intl.pluralLogic(
      count,
      locale: localeName,
      other: '$count agents',
      one: '1 agent',
    );
    return '$_temp0';
  }

  @override
  String get composerAgentRuntimeDriven =>
      'This agent session is driven by the runtime';

  @override
  String get statusSkillsSection => 'Skills';

  @override
  String get statusMcpSection => 'MCP';

  @override
  String get statusLspSection => 'LSP';

  @override
  String get statusSubagentsSection => 'Subagents';

  @override
  String get statusAgentChipTooltip => 'Subagent status';

  @override
  String get agentDetailTitle => 'Subagents';

  @override
  String agentDetailSummary(int count, int running) {
    String _temp0 = intl.Intl.pluralLogic(
      count,
      locale: localeName,
      other: '$count agents · $running running',
      one: '1 agent · $running running',
    );
    return '$_temp0';
  }

  @override
  String get agentDetailEmpty => 'No subagents';

  @override
  String get agentDetailStatusQueued => 'Queued';

  @override
  String get agentDetailStatusRunning => 'Running';

  @override
  String get agentDetailStatusWaiting => 'Waiting';

  @override
  String get agentDetailStatusCompleted => 'Completed';

  @override
  String get agentDetailStatusErrored => 'Errored';

  @override
  String get agentDetailStatusInterrupted => 'Interrupted';

  @override
  String get agentDetailStatusShutdown => 'Shutdown';

  @override
  String get agentDetailStatusNotFound => 'Not found';

  @override
  String get agentDetailSummaryLabel => 'Summary';

  @override
  String get agentDetailErrorLabel => 'Error';

  @override
  String get agentDetailReasonLabel => 'Reason';

  @override
  String get agentDetailPathLabel => 'Path';

  @override
  String get timelineEmptyTitle => 'No messages yet';

  @override
  String get timelineEmptyMessage =>
      'Open a project or start a session to begin.';

  @override
  String get workflowHistoryTitle => 'History';

  @override
  String get timelineExternalLinkOpenFailed => 'Unable to open this link.';

  @override
  String get timelineAttachment => 'Attachment';

  @override
  String get timelineImageFallback => 'Image';

  @override
  String get timelineImageLoadFailed => 'Unable to load this image.';

  @override
  String get timelineImageRetry => 'Retry';

  @override
  String get timelineImageClose => 'Close image preview';

  @override
  String timelineRemoteImageSource(String host) {
    return 'External image from $host';
  }

  @override
  String get timelineRemoteImageOpen => 'Click to load and preview';

  @override
  String get timelineJumpToLatest => 'Jump to latest';

  @override
  String get timelineNew => 'New';

  @override
  String get timelineReasoningFallback => 'Thinking';

  @override
  String get timelineReasoningActive => 'Thinking';

  @override
  String get timelineReasoningCompleted => 'Thought';

  @override
  String get timelineReasoningEmpty => 'No reasoning text was provided.';

  @override
  String get timelineToolFallback => 'Tool';

  @override
  String get timelineToolGroupTitle => 'Tool activity';

  @override
  String timelineToolGroupSummary(int count) {
    return '$count tools';
  }

  @override
  String timelineToolGroupSummaryRunning(int count, int runningCount) {
    return '$count tools, $runningCount running';
  }

  @override
  String timelineToolGroupSummaryIssues(int count, int issueCount) {
    return '$count tools, $issueCount need attention';
  }

  @override
  String timelineToolGroupSummaryRunningWithIssues(
    int count,
    int runningCount,
    int issueCount,
  ) {
    return '$count tools, $runningCount running, $issueCount need attention';
  }

  @override
  String timelineSkillActivated(String name) {
    return 'Activated skill · $name';
  }

  @override
  String timelineSkillAgentActivated(String name) {
    return 'Agent activated skill · $name';
  }

  @override
  String timelineSkillUserActivated(String name) {
    return 'User activated skill · $name';
  }

  @override
  String get timelineAgentFallback => 'Agent';

  @override
  String get timelineViewImageRead => 'Image read';

  @override
  String get timelineViewImageReading => 'Reading image';

  @override
  String get timelineViewImageFailed => 'Failed to read image';

  @override
  String timelineToolCompleted(String name) {
    return '$name completed';
  }

  @override
  String timelineToolFailed(String name) {
    return '$name failed';
  }

  @override
  String timelineToolDenied(String name) {
    return '$name denied';
  }

  @override
  String timelineToolCancelled(String name) {
    return '$name cancelled';
  }

  @override
  String timelineToolAwaitingApproval(String name) {
    return '$name awaiting approval';
  }

  @override
  String timelineToolRunning(String name) {
    return '$name running';
  }

  @override
  String timelineToolExitCode(int code) {
    return 'exit code $code';
  }

  @override
  String get timelineToolTimedOut => 'timed out';

  @override
  String get timelineAgentSubagent => 'Subagent';

  @override
  String get timelineAgentSubagentMessage => 'Subagent message';

  @override
  String get timelineAgentWaiting => 'Waiting for subagents';

  @override
  String get timelineAgentClose => 'Close subagent';

  @override
  String get timelineTodoListFallback => 'Todo list';

  @override
  String get timelineTodoPending => 'Pending';

  @override
  String get timelineTodoInProgress => 'In progress';

  @override
  String get timelineTodoCompleted => 'Completed';

  @override
  String get interactionQuestionsTitle => 'A few questions';

  @override
  String get interactionLastQuestion => 'Last question';

  @override
  String get interactionContinueAfterAnswer => 'Continue after answering';

  @override
  String get timelineRolledBack => 'Rolled back from active context';

  @override
  String get interactionSubmitEmptyAnswersHint =>
      'Unanswered questions are submitted as empty arrays.';

  @override
  String interactionAnsweredPendingHint(int answeredCount, int pendingCount) {
    return '$answeredCount answered · $pendingCount pending';
  }

  @override
  String get interactionPreviousQuestion => 'Previous';

  @override
  String get interactionNextQuestion => 'Next';

  @override
  String get interactionSubmitAnswers => 'Submit answers';

  @override
  String get interactionNeedInputTitle => 'Input needed';

  @override
  String get interactionAnswerHint =>
      'Pure will use this answer to continue the current question.';

  @override
  String get interactionAnswerButton => 'Answer';

  @override
  String get interactionAnswerLabel => 'Answer';

  @override
  String interactionQuestionProgress(int current, int total) {
    return 'Question $current / $total';
  }

  @override
  String interactionAnsweredCount(int count) {
    return '$count answered';
  }

  @override
  String interactionQuestionTooltip(int index) {
    return 'Question $index';
  }

  @override
  String get interactionQuestionFallback => 'Question';

  @override
  String get interactionOtherLabel => 'Other';

  @override
  String get interactionSecretHint => 'Enter secret answer';

  @override
  String get interactionTextHint => 'Enter your answer...';

  @override
  String get interactionPermissionTitle => 'Permission needed';

  @override
  String get interactionToolApprovalFallback => 'Tool approval';

  @override
  String get interactionPermissionSubtitle => 'Pure wants to run a tool call';

  @override
  String get interactionPermissionFooterHint =>
      'The tool runs in the current working directory; adjust permission mode in the composer.';

  @override
  String get interactionReject => 'Reject';

  @override
  String get interactionApprove => 'Approve';

  @override
  String get interactionReasonLabel => 'Reason';

  @override
  String get interactionPlanConfirmTitle => 'Confirm this plan?';

  @override
  String get interactionPlanConfirmSubtitle =>
      'Confirm it, or describe what should change.';

  @override
  String interactionPlanConfirmFooterHint(String mode) {
    return 'Confirming starts the document-editing checkpoint in $mode mode.';
  }

  @override
  String get interactionPlanAdjust => 'Tell Pure how to adjust';

  @override
  String get interactionPlanConfirmAction => 'Confirm plan';

  @override
  String get interactionPlanAdjustHint => 'Describe what should change...';

  @override
  String get interactionPlanAdjustSubmit => 'Submit adjustment';

  @override
  String get settingsProvidersTitle => 'Providers';

  @override
  String get settingsProvidersSubtitle =>
      'Model providers, credentials, models, and usage';

  @override
  String get settingsRefreshUsage => 'Refresh usage';

  @override
  String get settingsAddProvider => 'Add provider';

  @override
  String get settingsSearchProviders => 'Search providers';

  @override
  String get settingsNoProvidersMatchTitle => 'No providers match this filter';

  @override
  String get settingsNoProvidersMatchMessage =>
      'Clear the search to see all configured providers.';

  @override
  String get settingsNoProvidersTitle => 'No providers found';

  @override
  String get settingsNoProvidersMessage =>
      'Add a provider to configure credentials and models.';

  @override
  String get settingsDefaultProvider => 'Default provider';

  @override
  String get settingsSetAsDefaultProvider => 'Set as default';

  @override
  String get settingsOpenDetails => 'Open details';

  @override
  String get settingsProviderActions => 'Provider actions';

  @override
  String get settingsEditProvider => 'Edit provider';

  @override
  String get settingsDeleteProvider => 'Delete provider';

  @override
  String get settingsNoProviderSelected => 'No provider selected';

  @override
  String get settingsProviderCatalogUnavailable =>
      'Provider catalog is unavailable.';

  @override
  String get settingsProviderPresetUnavailable =>
      'Provider preset is unavailable.';

  @override
  String get settingsProviderTitle => 'Provider';

  @override
  String get settingsProviderModelsTitle => 'Models';

  @override
  String get settingsProviderConnectionTitle => 'Connection';

  @override
  String get settingsProviderDefaultModelsTitle => 'Default models';

  @override
  String get settingsProviderCustomModelsTitle => 'Custom models';

  @override
  String get settingsProviderCapabilitiesTitle => 'Service capabilities';

  @override
  String get settingsProviderCapabilitySource => 'Capability source';

  @override
  String get settingsProviderCapabilityPresetDefaults =>
      'Follow preset defaults';

  @override
  String get settingsProviderCapabilityExplicit => 'Explicit override';

  @override
  String get settingsProviderHostedWebSearch => 'Hosted web search';

  @override
  String get settingsProviderHostedWebSearchDialect =>
      'Hosted web search dialect';

  @override
  String get settingsProviderStandaloneWebSearch => 'Standalone web search';

  @override
  String get settingsProviderProgrammaticToolCalling =>
      'Programmatic tool calling';

  @override
  String get settingsProviderDialectOpenAiResponses => 'OpenAI Responses';

  @override
  String get settingsProviderDialectDeepSeekResponses => 'DeepSeek Responses';

  @override
  String get settingsEnabled => 'Enabled';

  @override
  String get settingsDisabled => 'Disabled';

  @override
  String get settingsProtocolResponses => 'Responses';

  @override
  String get settingsProtocolChatCompletions => 'Chat Completions';

  @override
  String get settingsConnectionDefault => 'Default connection';

  @override
  String get settingsConnectionsSupported => 'Supported connections';

  @override
  String get settingsConnectionCurrent => 'Current connection';

  @override
  String get settingsConnectionWebSocket => 'WS';

  @override
  String get settingsConnectionHttp => 'HTTP';

  @override
  String get settingsNewProvider => 'New provider';

  @override
  String get settingsProviderKey => 'Provider key';

  @override
  String get settingsTemplate => 'Template';

  @override
  String get settingsCustomProvider => 'Custom provider';

  @override
  String get settingsDefaultModel => 'Default model';

  @override
  String get settingsApiKey => 'API key';

  @override
  String get settingsApiKeyKeepCurrent =>
      'API key (leave blank to keep current)';

  @override
  String get settingsConfigured => 'configured';

  @override
  String get settingsMissing => 'missing';

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
  String get settingsEdit => 'Edit';

  @override
  String get settingsCancel => 'Cancel';

  @override
  String get settingsSave => 'Save';

  @override
  String get settingsAddModel => 'Add model';

  @override
  String get settingsCustomModelName => 'Custom model';

  @override
  String get settingsRemoveModel => 'Remove model';

  @override
  String get settingsNoCustomModels => 'No custom models';

  @override
  String settingsBundledModels(int count) {
    return '$count bundled';
  }

  @override
  String get settingsDefaultBadge => 'default';

  @override
  String get settingsReadyBadge => 'ready';

  @override
  String get settingsSetupBadge => 'setup';

  @override
  String get settingsUsageTitle => 'Usage';

  @override
  String settingsUsageUpdated(String updatedAt) {
    return 'Updated $updatedAt';
  }

  @override
  String get settingsUsageAvailableBalance => 'Available balance';

  @override
  String get settingsUsageBalanceUnavailable => 'Balance unavailable';

  @override
  String settingsUsageGranted(String amount) {
    return 'Granted $amount';
  }

  @override
  String settingsUsageToppedUp(String amount) {
    return 'Topped up $amount';
  }

  @override
  String get settingsUsageRefreshing => 'Refreshing usage...';

  @override
  String get settingsUsageChecking => 'Checking usage...';

  @override
  String get settingsUsageCheckingShort => 'Checking usage';

  @override
  String get settingsUsageNotLoaded => 'Usage not loaded';

  @override
  String get settingsUsageUnsupported => 'Unsupported';

  @override
  String get settingsUsageNotSupported => 'Usage not supported';

  @override
  String get settingsUsageMissingKey => 'Missing key';

  @override
  String get settingsUsageFailed => 'Usage failed';

  @override
  String get settingsUsageQueryFailed => 'Usage query failed';

  @override
  String get settingsUsageApiKeyMissing => 'Provider API key is not configured';

  @override
  String settingsUsageUnsupportedForProvider(String providerName) {
    return 'Usage is not supported for $providerName';
  }

  @override
  String get settingsUsageNotChecked => 'Not checked';

  @override
  String get settingsUsageUnavailable => 'Usage is unavailable';

  @override
  String get settingsUsageError => 'Could not load usage';

  @override
  String get settingsUsageNoQuota => 'No quota details returned.';

  @override
  String get settingsUsageTools => 'Tools';

  @override
  String get settingsUsageToken => 'Token usage';

  @override
  String get settingsUsageSpend => 'Spend';

  @override
  String get settingsUsageRemaining => 'Remaining';

  @override
  String get settingsUsageUsed => 'Used';

  @override
  String get settingsUsageFiveHourQuota => '5 hour quota';

  @override
  String get settingsUsageWeeklyQuota => 'Weekly quota';

  @override
  String get settingsUsageMcpQuota => 'MCP quota';

  @override
  String get settingsUsageQuota => 'Quota';

  @override
  String settingsUsageQuotaRemaining(String remaining, String total) {
    return '$remaining of $total remaining';
  }

  @override
  String settingsUsageQuotaUsed(String current, String total) {
    return '$current of $total used';
  }

  @override
  String settingsUsagePercentRemaining(String percent) {
    return '$percent remaining';
  }

  @override
  String settingsUsageReset(String time) {
    return 'Reset $time';
  }

  @override
  String get settingsInstructionsTitle => 'Instructions';

  @override
  String get settingsInstructionsSubtitle =>
      'Injected into each turn; changes save after typing stops.';

  @override
  String get settingsBaseInstructions => 'Base instructions';

  @override
  String get settingsDeveloperInstructions => 'Developer instructions';

  @override
  String get settingsUserContext => 'User context';

  @override
  String get settingsInstructionHint => 'Add project guidance here';

  @override
  String get settingsSkillsTitle => 'Skills';

  @override
  String get settingsSkillsSubtitle =>
      'Disable noisy skills or discover project/user/system catalogs.';

  @override
  String get settingsDiscover => 'Discover';

  @override
  String get settingsDiscovering => 'Discovering';

  @override
  String get settingsFilterSkills => 'Filter skills';

  @override
  String get settingsSkillDisabled => 'Disabled for this workspace';

  @override
  String get settingsSkillEnabled => 'Enabled';

  @override
  String get settingsOpenProjectToDiscoverSkills =>
      'Open a project to discover skills';

  @override
  String get settingsNoSkillsMatchFilter => 'No skills match this filter';

  @override
  String get settingsSkillsDiscoverySources =>
      'Skills are discovered from the selected workspace and configured user/system sources.';

  @override
  String get settingsClearSearchOrDiscoverAgain =>
      'Clear the search or run discovery again.';

  @override
  String get settingsNoSkillsTitle => 'No skills found';

  @override
  String get settingsNoSkillsMessage =>
      'Try another filter or discover skills for this project.';

  @override
  String get settingsRolesTitle => 'Roles';

  @override
  String get settingsRolesSubtitle =>
      'Choose provider/model defaults for each fixed agent role.';

  @override
  String get settingsAgentsTitle => 'Agent profiles';

  @override
  String get settingsAgentsSubtitle =>
      'System profiles are read-only; user profiles are stored in ~/.pure/agents/*.toml.';

  @override
  String get settingsAgentsAdd => 'Add user profile';

  @override
  String get settingsAgentsBuiltinReadonly => 'Built-in · read-only';

  @override
  String get settingsAgentsEditTooltip => 'Edit';

  @override
  String get settingsAgentsDialogAddTitle => 'Add user agent profile';

  @override
  String get settingsAgentsDialogEditTitle => 'Edit user agent profile';

  @override
  String get settingsAgentsFieldId => 'Agent ID';

  @override
  String get settingsAgentsFieldDisplayName => 'Display name';

  @override
  String get settingsAgentsFieldDescription => 'Description';

  @override
  String get settingsAgentsFieldWhenToUse => 'Applicable tasks';

  @override
  String get settingsAgentsFieldInstructions => 'System instructions';

  @override
  String get settingsAgentsFieldProvider => 'Provider ID';

  @override
  String get settingsAgentsFieldModel => 'Model';

  @override
  String get settingsAgentsFieldEffort => 'Effort (optional)';

  @override
  String get settingsAgentsEnabled => 'Enabled';

  @override
  String get settingsAgentsEnabledSubtitle =>
      'Disabled profiles remain in TOML but are removed from the Agent tool catalog.';

  @override
  String get settingsAgentsCancel => 'Cancel';

  @override
  String get settingsAgentsSave => 'Save TOML atomically';

  @override
  String get settingsRequired => 'Required';

  @override
  String get settingsRoleExplorerDescription =>
      'Explore code and collect context.';

  @override
  String get settingsRolePlannerDescription =>
      'Draft plans and structure intent.';

  @override
  String get settingsRoleExecutorDescription => 'Apply edits and run tools.';

  @override
  String get settingsRoleReviewerDescription =>
      'Review results and verify risk.';

  @override
  String get settingsRoleFallbackDescription => 'Studio role';

  @override
  String get settingsModelField => 'Model';

  @override
  String get settingsMcpTitle => 'MCP';

  @override
  String get settingsMcpSubtitle =>
      'Model Context Protocol servers and inline endpoints.';

  @override
  String get settingsMcpRefresh => 'Refresh';

  @override
  String get settingsMcpReconnect => 'Reconnect';

  @override
  String get settingsMcpResetAll => 'Reset all';

  @override
  String get settingsMcpResetConfirmTitle => 'Reset all MCP servers?';

  @override
  String get settingsMcpResetConfirmBody =>
      'Every configured MCP connection will be rebuilt. Active turns keep their current leased generation.';

  @override
  String get settingsMcpResetConfirmAction => 'Reset all';

  @override
  String get settingsEndpoint => 'Endpoint';

  @override
  String get settingsMcpStatusDisabled => 'disabled';

  @override
  String get settingsMcpStatusMissingCredential => 'missingCredential';

  @override
  String get settingsMcpStatusChecking => 'checking';

  @override
  String get settingsMcpStatusAvailable => 'available';

  @override
  String get settingsMcpStatusUnavailable => 'unavailable';

  @override
  String get settingsMcpHealthCheckPending => 'MCP health check is pending';

  @override
  String get settingsMcpDisabledMessage =>
      'MCP server is disabled in configuration';

  @override
  String get settingsMcpMissingCredentialMessage =>
      'MCP server credential is not configured';

  @override
  String get settingsMcpEmptyTitle => 'No MCP servers';

  @override
  String get settingsMcpEmptyMessage =>
      'Configured MCP servers will appear here.';

  @override
  String get settingsLspTitle => 'Language servers';

  @override
  String get settingsLspSubtitle =>
      'Last-known project language server status and explicit lifecycle commands.';

  @override
  String get settingsLspRefresh => 'Refresh';

  @override
  String get settingsLspProbe => 'Probe';

  @override
  String get settingsLspRepair => 'Repair';

  @override
  String get settingsLspReset => 'Reset';

  @override
  String get settingsLspResetWorkspace => 'Reset workspace';

  @override
  String get settingsLspActivityIndexing => 'Indexing';

  @override
  String get settingsLspActivityBusy => 'Busy';

  @override
  String get settingsLspStatusChecking => 'checking';

  @override
  String get settingsLspStatusAvailable => 'available';

  @override
  String get settingsLspStatusUnavailable => 'unavailable';

  @override
  String get settingsLspStatusDisabled => 'disabled';

  @override
  String get settingsLspEmptyTitle => 'No language servers';

  @override
  String get settingsLspEmptyMessage =>
      'Activate a supported project to create language server membership.';

  @override
  String get settingsSecurityTitle => 'Security';

  @override
  String get settingsSecuritySubtitle =>
      'Choose the default approval posture for this workspace.';

  @override
  String get settingsSecurityModeSubtitle =>
      'Tool execution permission mode; changes apply immediately.';

  @override
  String settingsCurrentMode(String mode) {
    return 'Current: $mode';
  }

  @override
  String get settingsWorkspaceBoundary =>
      'Workspace boundary policy remains unchanged.';

  @override
  String get settingsGeneralTitle => 'General';

  @override
  String get settingsGeneralSubtitle =>
      'Interface preferences saved into the Studio store.';

  @override
  String get settingsFollowSystemTheme => 'Follow system theme';

  @override
  String get settingsFollowSystemThemeSubtitle =>
      'Switch light and dark mode with the OS.';

  @override
  String get settingsFollowActiveTurn => 'Follow active turn';

  @override
  String get settingsFollowActiveTurnSubtitle =>
      'Keep new timeline output pinned to the latest turn.';

  @override
  String get settingsCompactTimeline => 'Compact timeline';

  @override
  String get settingsCompactTimelineSubtitle =>
      'Reduce message spacing for denser reading.';

  @override
  String get settingsWebSearchTitle => 'Web search';

  @override
  String get settingsWebSearchSubtitle =>
      'Search through an eligible OpenAI account. Changes apply from the next turn.';

  @override
  String get settingsWebSearchConfiguredMode => 'Configured mode';

  @override
  String get settingsWebSearchEffectiveMode => 'Effective mode';

  @override
  String get settingsWebSearchProvider => 'OpenAI provider';

  @override
  String get settingsWebSearchModel => 'Search model';

  @override
  String get settingsWebSearchMode => 'Mode';

  @override
  String get settingsWebSearchModeDisabled => 'Disabled';

  @override
  String get settingsWebSearchModeCached => 'Cached';

  @override
  String get settingsWebSearchModeIndexed => 'Indexed';

  @override
  String get settingsWebSearchModeLive => 'Live';

  @override
  String get settingsWebSearchContextSize => 'Context size';

  @override
  String get settingsWebSearchContextLow => 'Low';

  @override
  String get settingsWebSearchContextMedium => 'Medium';

  @override
  String get settingsWebSearchContextHigh => 'High';

  @override
  String get settingsServiceDefault => 'Service default';

  @override
  String get settingsWebSearchAllowedDomains => 'Allowed domains';

  @override
  String get settingsWebSearchDomainsHint => 'example.com, docs.example.com';

  @override
  String get settingsWebSearchCountry => 'Country';

  @override
  String get settingsWebSearchRegion => 'Region';

  @override
  String get settingsWebSearchCity => 'City';

  @override
  String get settingsWebSearchTimezone => 'Timezone';

  @override
  String get settingsWebSearchAvailable => 'Available';

  @override
  String get settingsWebSearchAvailableNotSelected => 'Available, not selected';

  @override
  String get settingsWebSearchDisabled => 'Disabled';

  @override
  String get settingsWebSearchMissingCredential => 'Missing credential';

  @override
  String get settingsWebSearchUnsupportedProvider => 'Unsupported provider';

  @override
  String get settingsWebSearchUnsupportedModel => 'Unsupported model';

  @override
  String get settingsWebSearchMissingCredentialReason =>
      'No credentialed provider originating from the OpenAI preset is available. Remote web search is fully disabled.';

  @override
  String get settingsWebSearchUnsupportedProviderReason =>
      'The current provider does not expose this web search backend.';

  @override
  String get settingsWebSearchUnsupportedModelReason =>
      'The current route cannot expose either the function tool or hosted web search.';

  @override
  String get settingsNotAvailable => 'Not available';

  @override
  String get settingsSaveWebSearch => 'Save web search';

  @override
  String get settingsDeepSeekWebSearchTitle => 'DeepSeek native web search';

  @override
  String get settingsDeepSeekWebSearchSubtitle =>
      'Let the current eligible DeepSeek Responses model search the web. It takes priority over the OpenAI fallback.';

  @override
  String get settingsDeepSeekWebSearchConfigured => 'Configured';

  @override
  String get settingsDeepSeekWebSearchEffective => 'Effective';

  @override
  String get settingsDeepSeekWebSearchEnabled => 'Enabled';

  @override
  String get settingsStudioUpdateTitle => 'Pure Studio update';

  @override
  String settingsStudioUpdateDisabled(String version) {
    return 'Version $version. Automatic checks run only in Windows release builds.';
  }

  @override
  String settingsStudioUpdateCurrent(String version) {
    return 'Current version: $version';
  }

  @override
  String settingsStudioUpdateChecking(String version) {
    return 'Version $version. Checking for updates...';
  }

  @override
  String settingsStudioUpdateLatest(String version) {
    return 'Version $version is up to date.';
  }

  @override
  String settingsStudioUpdateAvailable(String current, String latest) {
    return 'Version $current is installed. Version $latest is available.';
  }

  @override
  String settingsStudioUpdateDownloading(String version, int progress) {
    return 'Downloading version $version: $progress%';
  }

  @override
  String settingsStudioUpdateVerifying(String version) {
    return 'Verifying version $version...';
  }

  @override
  String settingsStudioUpdateInstallerLaunched(String version) {
    return 'Installer for version $version started.';
  }

  @override
  String settingsStudioUpdateFailed(String error) {
    return 'Update failed: $error';
  }

  @override
  String get settingsStudioUpdateBusy =>
      'Finish the active turn or task before installing.';

  @override
  String get settingsStudioUpdateCheck => 'Check for updates';

  @override
  String get settingsStudioUpdateInstall => 'Download and install';

  @override
  String get settingsStudioUpdateReleaseNotes => 'Release notes';

  @override
  String get timelineWebSearchTitle => 'Web search';

  @override
  String get timelineWebSearchSearching => 'Searching the web';

  @override
  String get timelineWebSearchOpening => 'Opening a web page';

  @override
  String get timelineWebSearchFinding => 'Finding text on a page';

  @override
  String get timelineWebSearchResults => 'Result links';

  @override
  String get timelineLspQueryTitle => 'LSP query';

  @override
  String timelineLspQueryTitleWithDetail(String detail) {
    return 'LSP query · $detail';
  }

  @override
  String get timelineLspCapabilitiesTitle => 'LSP capabilities';

  @override
  String get roleExplorer => 'Explorer';

  @override
  String get rolePlanner => 'Planner';

  @override
  String get roleExecutor => 'Executor';

  @override
  String get roleReviewer => 'Reviewer';

  @override
  String get roleEmpty => 'Agent';
}
