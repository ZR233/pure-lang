import 'dart:async';

import 'package:flutter/foundation.dart';
import 'package:flutter/widgets.dart';
import 'package:flutter_localizations/flutter_localizations.dart';
import 'package:intl/intl.dart' as intl;

import 'app_localizations_en.dart';
import 'app_localizations_zh.dart';

// ignore_for_file: type=lint

/// Callers can lookup localized strings with an instance of AppLocalizations
/// returned by `AppLocalizations.of(context)`.
///
/// Applications need to include `AppLocalizations.delegate()` in their app's
/// `localizationDelegates` list, and the locales they support in the app's
/// `supportedLocales` list. For example:
///
/// ```dart
/// import 'l10n/app_localizations.dart';
///
/// return MaterialApp(
///   localizationsDelegates: AppLocalizations.localizationsDelegates,
///   supportedLocales: AppLocalizations.supportedLocales,
///   home: MyApplicationHome(),
/// );
/// ```
///
/// ## Update pubspec.yaml
///
/// Please make sure to update your pubspec.yaml to include the following
/// packages:
///
/// ```yaml
/// dependencies:
///   # Internationalization support.
///   flutter_localizations:
///     sdk: flutter
///   intl: any # Use the pinned version from flutter_localizations
///
///   # Rest of dependencies
/// ```
///
/// ## iOS Applications
///
/// iOS applications define key application metadata, including supported
/// locales, in an Info.plist file that is built into the application bundle.
/// To configure the locales supported by your app, you’ll need to edit this
/// file.
///
/// First, open your project’s ios/Runner.xcworkspace Xcode workspace file.
/// Then, in the Project Navigator, open the Info.plist file under the Runner
/// project’s Runner folder.
///
/// Next, select the Information Property List item, select Add Item from the
/// Editor menu, then select Localizations from the pop-up menu.
///
/// Select and expand the newly-created Localizations item then, for each
/// locale your application supports, add a new item and select the locale
/// you wish to add from the pop-up menu in the Value field. This list should
/// be consistent with the languages listed in the AppLocalizations.supportedLocales
/// property.
abstract class AppLocalizations {
  AppLocalizations(String locale)
    : localeName = intl.Intl.canonicalizedLocale(locale.toString());

  final String localeName;

  static AppLocalizations of(BuildContext context) {
    return Localizations.of<AppLocalizations>(context, AppLocalizations)!;
  }

  static const LocalizationsDelegate<AppLocalizations> delegate =
      _AppLocalizationsDelegate();

  /// A list of this localizations delegate along with the default localizations
  /// delegates.
  ///
  /// Returns a list of localizations delegates containing this delegate along with
  /// GlobalMaterialLocalizations.delegate, GlobalCupertinoLocalizations.delegate,
  /// and GlobalWidgetsLocalizations.delegate.
  ///
  /// Additional delegates can be added by appending to this list in
  /// MaterialApp. This list does not have to be used at all if a custom list
  /// of delegates is preferred or required.
  static const List<LocalizationsDelegate<dynamic>> localizationsDelegates =
      <LocalizationsDelegate<dynamic>>[
        delegate,
        GlobalMaterialLocalizations.delegate,
        GlobalCupertinoLocalizations.delegate,
        GlobalWidgetsLocalizations.delegate,
      ];

  /// A list of this localizations delegate's supported locales.
  static const List<Locale> supportedLocales = <Locale>[
    Locale('en'),
    Locale('zh'),
    Locale.fromSubtags(languageCode: 'zh', scriptCode: 'Hans'),
  ];

  /// No description provided for @appTitle.
  ///
  /// In en, this message translates to:
  /// **'Pure Studio'**
  String get appTitle;

  /// No description provided for @sidebarProjects.
  ///
  /// In en, this message translates to:
  /// **'Projects'**
  String get sidebarProjects;

  /// No description provided for @sidebarSessions.
  ///
  /// In en, this message translates to:
  /// **'Sessions'**
  String get sidebarSessions;

  /// No description provided for @sidebarLoadingMore.
  ///
  /// In en, this message translates to:
  /// **'Loading more sessions…'**
  String get sidebarLoadingMore;

  /// No description provided for @sidebarLoadError.
  ///
  /// In en, this message translates to:
  /// **'Failed to load more sessions'**
  String get sidebarLoadError;

  /// No description provided for @shutdownTitle.
  ///
  /// In en, this message translates to:
  /// **'Shutting down safely'**
  String get shutdownTitle;

  /// No description provided for @shutdownPhaseStoppingSubscriptions.
  ///
  /// In en, this message translates to:
  /// **'Stopping subscriptions'**
  String get shutdownPhaseStoppingSubscriptions;

  /// No description provided for @shutdownPhaseCancellingTurns.
  ///
  /// In en, this message translates to:
  /// **'Stopping active turns'**
  String get shutdownPhaseCancellingTurns;

  /// No description provided for @shutdownPhaseFlushingPersistence.
  ///
  /// In en, this message translates to:
  /// **'Saving sessions'**
  String get shutdownPhaseFlushingPersistence;

  /// No description provided for @shutdownPhaseSuspendingTasks.
  ///
  /// In en, this message translates to:
  /// **'Suspending background tasks'**
  String get shutdownPhaseSuspendingTasks;

  /// No description provided for @shutdownPhaseStoppingMcp.
  ///
  /// In en, this message translates to:
  /// **'Stopping MCP servers'**
  String get shutdownPhaseStoppingMcp;

  /// No description provided for @shutdownPhaseStoppingLsp.
  ///
  /// In en, this message translates to:
  /// **'Stopping language servers'**
  String get shutdownPhaseStoppingLsp;

  /// No description provided for @shutdownPhaseStopped.
  ///
  /// In en, this message translates to:
  /// **'Shutdown complete'**
  String get shutdownPhaseStopped;

  /// No description provided for @shutdownPendingCommits.
  ///
  /// In en, this message translates to:
  /// **'{count} commits pending'**
  String shutdownPendingCommits(int count);

  /// No description provided for @sidebarCloseProject.
  ///
  /// In en, this message translates to:
  /// **'Close project'**
  String get sidebarCloseProject;

  /// No description provided for @sidebarArchiveSession.
  ///
  /// In en, this message translates to:
  /// **'Archive session'**
  String get sidebarArchiveSession;

  /// No description provided for @sidebarArchiveSessionFailed.
  ///
  /// In en, this message translates to:
  /// **'Could not archive this session. It may still be running.'**
  String get sidebarArchiveSessionFailed;

  /// No description provided for @sidebarNewSession.
  ///
  /// In en, this message translates to:
  /// **'New session'**
  String get sidebarNewSession;

  /// No description provided for @sidebarOpenProject.
  ///
  /// In en, this message translates to:
  /// **'Open project'**
  String get sidebarOpenProject;

  /// No description provided for @sidebarSettings.
  ///
  /// In en, this message translates to:
  /// **'Settings'**
  String get sidebarSettings;

  /// No description provided for @runtimeFatalTitle.
  ///
  /// In en, this message translates to:
  /// **'Pure Studio could not start'**
  String get runtimeFatalTitle;

  /// No description provided for @runtimeFatalRetry.
  ///
  /// In en, this message translates to:
  /// **'Retry'**
  String get runtimeFatalRetry;

  /// No description provided for @recoveryGlobalWarning.
  ///
  /// In en, this message translates to:
  /// **'{count} recovery issue(s) need attention'**
  String recoveryGlobalWarning(int count);

  /// No description provided for @recoveryCleanupTooltip.
  ///
  /// In en, this message translates to:
  /// **'Review safe cleanup'**
  String get recoveryCleanupTooltip;

  /// No description provided for @recoveryRetryTooltip.
  ///
  /// In en, this message translates to:
  /// **'Continue merge recovery'**
  String get recoveryRetryTooltip;

  /// No description provided for @recoveryRetryFailed.
  ///
  /// In en, this message translates to:
  /// **'Recovery retry failed: {error}'**
  String recoveryRetryFailed(String error);

  /// No description provided for @recoveryCleanupTitle.
  ///
  /// In en, this message translates to:
  /// **'Clean up recovery issue?'**
  String get recoveryCleanupTitle;

  /// No description provided for @recoveryCleanupBody.
  ///
  /// In en, this message translates to:
  /// **'Pure Studio will only remove its own task resources. Review any work that may be lost before continuing.'**
  String get recoveryCleanupBody;

  /// No description provided for @recoveryCleanupNoResources.
  ///
  /// In en, this message translates to:
  /// **'No remaining task resources were found.'**
  String get recoveryCleanupNoResources;

  /// No description provided for @recoveryCleanupPresenceAbsent.
  ///
  /// In en, this message translates to:
  /// **'Missing'**
  String get recoveryCleanupPresenceAbsent;

  /// No description provided for @recoveryCleanupPresenceComplete.
  ///
  /// In en, this message translates to:
  /// **'Complete'**
  String get recoveryCleanupPresenceComplete;

  /// No description provided for @recoveryCleanupPresencePartial.
  ///
  /// In en, this message translates to:
  /// **'Partially missing'**
  String get recoveryCleanupPresencePartial;

  /// No description provided for @recoveryCleanupDirty.
  ///
  /// In en, this message translates to:
  /// **'Uncommitted changes'**
  String get recoveryCleanupDirty;

  /// No description provided for @recoveryCleanupAhead.
  ///
  /// In en, this message translates to:
  /// **'{count} unmerged commit(s)'**
  String recoveryCleanupAhead(int count);

  /// No description provided for @recoveryCleanupChangedFiles.
  ///
  /// In en, this message translates to:
  /// **'{count} changed file(s)'**
  String recoveryCleanupChangedFiles(int count);

  /// No description provided for @recoveryCleanupCancel.
  ///
  /// In en, this message translates to:
  /// **'Cancel'**
  String get recoveryCleanupCancel;

  /// No description provided for @recoveryCleanupConfirm.
  ///
  /// In en, this message translates to:
  /// **'Clean up'**
  String get recoveryCleanupConfirm;

  /// No description provided for @recoveryCleanupRefreshPreview.
  ///
  /// In en, this message translates to:
  /// **'Refresh preview'**
  String get recoveryCleanupRefreshPreview;

  /// No description provided for @recoveryCleanupFailed.
  ///
  /// In en, this message translates to:
  /// **'Cleanup failed: {error}'**
  String recoveryCleanupFailed(String error);

  /// No description provided for @projectCleanupTitle.
  ///
  /// In en, this message translates to:
  /// **'Remove project and clean up Pure worktrees?'**
  String get projectCleanupTitle;

  /// No description provided for @projectCleanupBody.
  ///
  /// In en, this message translates to:
  /// **'This removes the project from Studio and permanently discards uncommitted changes and unmerged commits in every Pure-owned task worktree. The main workspace will not be deleted or modified.'**
  String get projectCleanupBody;

  /// No description provided for @projectCleanupConfirm.
  ///
  /// In en, this message translates to:
  /// **'Remove project and clean up'**
  String get projectCleanupConfirm;

  /// No description provided for @sidebarNew.
  ///
  /// In en, this message translates to:
  /// **'New'**
  String get sidebarNew;

  /// No description provided for @sidebarOpen.
  ///
  /// In en, this message translates to:
  /// **'Open'**
  String get sidebarOpen;

  /// No description provided for @shellNoSession.
  ///
  /// In en, this message translates to:
  /// **'New session'**
  String get shellNoSession;

  /// No description provided for @startPageWelcome.
  ///
  /// In en, this message translates to:
  /// **'What can I help you build?'**
  String get startPageWelcome;

  /// No description provided for @startPageProject.
  ///
  /// In en, this message translates to:
  /// **'Working in {project}'**
  String startPageProject(String project);

  /// No description provided for @startPageOpenProjectTitle.
  ///
  /// In en, this message translates to:
  /// **'Open a project to get started'**
  String get startPageOpenProjectTitle;

  /// No description provided for @startPageOpenProjectBody.
  ///
  /// In en, this message translates to:
  /// **'Choose a project from the sidebar before sending your first message.'**
  String get startPageOpenProjectBody;

  /// No description provided for @shellSessionUpdated.
  ///
  /// In en, this message translates to:
  /// **'{mode} · updated {time}'**
  String shellSessionUpdated(String mode, String time);

  /// No description provided for @settingsBack.
  ///
  /// In en, this message translates to:
  /// **'Back'**
  String get settingsBack;

  /// No description provided for @settingsBackToChat.
  ///
  /// In en, this message translates to:
  /// **'Back to chat'**
  String get settingsBackToChat;

  /// No description provided for @settingsWorkspaceGroup.
  ///
  /// In en, this message translates to:
  /// **'Workspace'**
  String get settingsWorkspaceGroup;

  /// No description provided for @settingsSystemGroup.
  ///
  /// In en, this message translates to:
  /// **'System'**
  String get settingsSystemGroup;

  /// No description provided for @settingsProvidersTab.
  ///
  /// In en, this message translates to:
  /// **'Providers'**
  String get settingsProvidersTab;

  /// No description provided for @settingsInstructionsTab.
  ///
  /// In en, this message translates to:
  /// **'Instructions'**
  String get settingsInstructionsTab;

  /// No description provided for @settingsSkillsTab.
  ///
  /// In en, this message translates to:
  /// **'Skills'**
  String get settingsSkillsTab;

  /// No description provided for @settingsRolesTab.
  ///
  /// In en, this message translates to:
  /// **'Roles'**
  String get settingsRolesTab;

  /// No description provided for @settingsMcpTab.
  ///
  /// In en, this message translates to:
  /// **'MCP'**
  String get settingsMcpTab;

  /// No description provided for @settingsLspTab.
  ///
  /// In en, this message translates to:
  /// **'LSP'**
  String get settingsLspTab;

  /// No description provided for @settingsSecurityTab.
  ///
  /// In en, this message translates to:
  /// **'Security'**
  String get settingsSecurityTab;

  /// No description provided for @settingsGeneralTab.
  ///
  /// In en, this message translates to:
  /// **'General'**
  String get settingsGeneralTab;

  /// No description provided for @composerHint.
  ///
  /// In en, this message translates to:
  /// **'Describe what you need...'**
  String get composerHint;

  /// No description provided for @composerSend.
  ///
  /// In en, this message translates to:
  /// **'Send'**
  String get composerSend;

  /// No description provided for @composerStop.
  ///
  /// In en, this message translates to:
  /// **'Stop'**
  String get composerStop;

  /// No description provided for @permissionModeTooltip.
  ///
  /// In en, this message translates to:
  /// **'Permission mode'**
  String get permissionModeTooltip;

  /// No description provided for @compileModeSimple.
  ///
  /// In en, this message translates to:
  /// **'Simple'**
  String get compileModeSimple;

  /// No description provided for @compileModeTask.
  ///
  /// In en, this message translates to:
  /// **'Task'**
  String get compileModeTask;

  /// No description provided for @permissionModeRequestApproval.
  ///
  /// In en, this message translates to:
  /// **'Request'**
  String get permissionModeRequestApproval;

  /// No description provided for @permissionModeAutoReview.
  ///
  /// In en, this message translates to:
  /// **'Review'**
  String get permissionModeAutoReview;

  /// No description provided for @permissionModeFullAccess.
  ///
  /// In en, this message translates to:
  /// **'Full'**
  String get permissionModeFullAccess;

  /// No description provided for @statusCost.
  ///
  /// In en, this message translates to:
  /// **'Cost'**
  String get statusCost;

  /// No description provided for @statusTotalTokensLabel.
  ///
  /// In en, this message translates to:
  /// **'Total tokens'**
  String get statusTotalTokensLabel;

  /// No description provided for @statusModelLabel.
  ///
  /// In en, this message translates to:
  /// **'Model'**
  String get statusModelLabel;

  /// No description provided for @statusCapabilitiesTitle.
  ///
  /// In en, this message translates to:
  /// **'Active capabilities'**
  String get statusCapabilitiesTitle;

  /// No description provided for @statusSessionMode.
  ///
  /// In en, this message translates to:
  /// **'Session mode'**
  String get statusSessionMode;

  /// No description provided for @statusSessionModeLocked.
  ///
  /// In en, this message translates to:
  /// **'Session mode cannot change while the session is running or a Task is active'**
  String get statusSessionModeLocked;

  /// No description provided for @statusPlannerModel.
  ///
  /// In en, this message translates to:
  /// **'Planner model'**
  String get statusPlannerModel;

  /// No description provided for @statusExecutorModel.
  ///
  /// In en, this message translates to:
  /// **'Executor model'**
  String get statusExecutorModel;

  /// No description provided for @statusReasoningEffort.
  ///
  /// In en, this message translates to:
  /// **'Reasoning effort'**
  String get statusReasoningEffort;

  /// No description provided for @statusContextLabel.
  ///
  /// In en, this message translates to:
  /// **'Context'**
  String get statusContextLabel;

  /// No description provided for @statusCacheLabel.
  ///
  /// In en, this message translates to:
  /// **'Cache'**
  String get statusCacheLabel;

  /// No description provided for @statusCacheHitTokensLabel.
  ///
  /// In en, this message translates to:
  /// **'Cache read'**
  String get statusCacheHitTokensLabel;

  /// No description provided for @statusCacheMissTokensLabel.
  ///
  /// In en, this message translates to:
  /// **'Cache miss'**
  String get statusCacheMissTokensLabel;

  /// No description provided for @statusCacheWriteTokensLabel.
  ///
  /// In en, this message translates to:
  /// **'Cache write'**
  String get statusCacheWriteTokensLabel;

  /// No description provided for @statusReasoningTokensLabel.
  ///
  /// In en, this message translates to:
  /// **'Reasoning tokens'**
  String get statusReasoningTokensLabel;

  /// No description provided for @statusInferenceCountLabel.
  ///
  /// In en, this message translates to:
  /// **'Inferences'**
  String get statusInferenceCountLabel;

  /// No description provided for @statusCacheSavingsLabel.
  ///
  /// In en, this message translates to:
  /// **'Cache savings'**
  String get statusCacheSavingsLabel;

  /// No description provided for @statusUnpricedUsageLabel.
  ///
  /// In en, this message translates to:
  /// **'Partially unpriced'**
  String get statusUnpricedUsageLabel;

  /// No description provided for @statusTurnQueued.
  ///
  /// In en, this message translates to:
  /// **'Queued'**
  String get statusTurnQueued;

  /// No description provided for @statusTurnPreparing.
  ///
  /// In en, this message translates to:
  /// **'Preparing context'**
  String get statusTurnPreparing;

  /// No description provided for @statusTurnResponding.
  ///
  /// In en, this message translates to:
  /// **'Responding'**
  String get statusTurnResponding;

  /// No description provided for @statusTurnPlanning.
  ///
  /// In en, this message translates to:
  /// **'Planning'**
  String get statusTurnPlanning;

  /// No description provided for @statusTurnRunningTool.
  ///
  /// In en, this message translates to:
  /// **'Running tool'**
  String get statusTurnRunningTool;

  /// No description provided for @statusTurnWaitingForApproval.
  ///
  /// In en, this message translates to:
  /// **'Waiting for tool approval'**
  String get statusTurnWaitingForApproval;

  /// No description provided for @statusTurnWaitingForUserInput.
  ///
  /// In en, this message translates to:
  /// **'Waiting for input'**
  String get statusTurnWaitingForUserInput;

  /// No description provided for @statusTurnWaitingForPlanConfirmation.
  ///
  /// In en, this message translates to:
  /// **'Waiting for plan confirmation'**
  String get statusTurnWaitingForPlanConfirmation;

  /// No description provided for @statusTurnPersisting.
  ///
  /// In en, this message translates to:
  /// **'Saving turn'**
  String get statusTurnPersisting;

  /// No description provided for @statusInteractionToolApproval.
  ///
  /// In en, this message translates to:
  /// **'Waiting for tool approval'**
  String get statusInteractionToolApproval;

  /// No description provided for @statusInteractionUserInput.
  ///
  /// In en, this message translates to:
  /// **'Waiting for input'**
  String get statusInteractionUserInput;

  /// No description provided for @statusInteractionPlanConfirmation.
  ///
  /// In en, this message translates to:
  /// **'Waiting for plan confirmation'**
  String get statusInteractionPlanConfirmation;

  /// No description provided for @statusContextTooltip.
  ///
  /// In en, this message translates to:
  /// **'Context: {contextTokens}/{contextWindow} ({percent}%)\n\nTotal tokens: {totalTokens}\n\nModel: {model}'**
  String statusContextTooltip(
    int contextTokens,
    int contextWindow,
    int percent,
    int totalTokens,
    String model,
  );

  /// No description provided for @statusContextTooltipNoModel.
  ///
  /// In en, this message translates to:
  /// **'Context: {contextTokens}/{contextWindow} ({percent}%)\n\nTotal tokens: {totalTokens}'**
  String statusContextTooltipNoModel(
    int contextTokens,
    int contextWindow,
    int percent,
    int totalTokens,
  );

  /// No description provided for @statusSkillsCount.
  ///
  /// In en, this message translates to:
  /// **'{count, plural, =1{1 skill} other{{count} skills}}'**
  String statusSkillsCount(int count);

  /// No description provided for @statusMcpCount.
  ///
  /// In en, this message translates to:
  /// **'{count} MCP'**
  String statusMcpCount(int count);

  /// No description provided for @statusLspCount.
  ///
  /// In en, this message translates to:
  /// **'{count} LSP'**
  String statusLspCount(int count);

  /// No description provided for @statusLspIndexing.
  ///
  /// In en, this message translates to:
  /// **'Indexing'**
  String get statusLspIndexing;

  /// No description provided for @statusLspBusy.
  ///
  /// In en, this message translates to:
  /// **'Working'**
  String get statusLspBusy;

  /// No description provided for @statusLspActivityPercentage.
  ///
  /// In en, this message translates to:
  /// **'{percentage}%'**
  String statusLspActivityPercentage(int percentage);

  /// No description provided for @statusAgentsCount.
  ///
  /// In en, this message translates to:
  /// **'{count, plural, =1{1 agent} other{{count} agents}}'**
  String statusAgentsCount(int count);

  /// No description provided for @composerAgentRuntimeDriven.
  ///
  /// In en, this message translates to:
  /// **'This agent session is driven by the runtime'**
  String get composerAgentRuntimeDriven;

  /// No description provided for @statusTaskSection.
  ///
  /// In en, this message translates to:
  /// **'Task coordinator'**
  String get statusTaskSection;

  /// No description provided for @statusTaskBranch.
  ///
  /// In en, this message translates to:
  /// **'Branch'**
  String get statusTaskBranch;

  /// No description provided for @statusTaskHead.
  ///
  /// In en, this message translates to:
  /// **'HEAD'**
  String get statusTaskHead;

  /// No description provided for @statusTaskWorkUnits.
  ///
  /// In en, this message translates to:
  /// **'Work units'**
  String get statusTaskWorkUnits;

  /// No description provided for @statusTaskAgents.
  ///
  /// In en, this message translates to:
  /// **'Agents'**
  String get statusTaskAgents;

  /// No description provided for @statusTaskCompletions.
  ///
  /// In en, this message translates to:
  /// **'Completions'**
  String get statusTaskCompletions;

  /// No description provided for @statusTaskMerges.
  ///
  /// In en, this message translates to:
  /// **'Merge records'**
  String get statusTaskMerges;

  /// No description provided for @statusTaskReviews.
  ///
  /// In en, this message translates to:
  /// **'Reviews'**
  String get statusTaskReviews;

  /// No description provided for @statusTaskWorktree.
  ///
  /// In en, this message translates to:
  /// **'Worktree'**
  String get statusTaskWorktree;

  /// No description provided for @statusTaskCommit.
  ///
  /// In en, this message translates to:
  /// **'Commit'**
  String get statusTaskCommit;

  /// No description provided for @statusTaskSource.
  ///
  /// In en, this message translates to:
  /// **'Source'**
  String get statusTaskSource;

  /// No description provided for @statusTaskPreviousHead.
  ///
  /// In en, this message translates to:
  /// **'Previous HEAD'**
  String get statusTaskPreviousHead;

  /// No description provided for @statusTaskDeliveryHead.
  ///
  /// In en, this message translates to:
  /// **'Delivery HEAD'**
  String get statusTaskDeliveryHead;

  /// No description provided for @statusTaskResultingHead.
  ///
  /// In en, this message translates to:
  /// **'Resulting HEAD'**
  String get statusTaskResultingHead;

  /// No description provided for @statusTaskCleanup.
  ///
  /// In en, this message translates to:
  /// **'Cleanup'**
  String get statusTaskCleanup;

  /// No description provided for @statusTaskRequest.
  ///
  /// In en, this message translates to:
  /// **'Request'**
  String get statusTaskRequest;

  /// No description provided for @statusTaskSummary.
  ///
  /// In en, this message translates to:
  /// **'Summary'**
  String get statusTaskSummary;

  /// No description provided for @statusTaskStage.
  ///
  /// In en, this message translates to:
  /// **'Stage'**
  String get statusTaskStage;

  /// No description provided for @statusTaskNextStep.
  ///
  /// In en, this message translates to:
  /// **'Next step'**
  String get statusTaskNextStep;

  /// No description provided for @statusTaskSummaryAge.
  ///
  /// In en, this message translates to:
  /// **'Summary age'**
  String get statusTaskSummaryAge;

  /// No description provided for @statusTaskVerification.
  ///
  /// In en, this message translates to:
  /// **'Verification'**
  String get statusTaskVerification;

  /// No description provided for @statusTaskScope.
  ///
  /// In en, this message translates to:
  /// **'Scope'**
  String get statusTaskScope;

  /// No description provided for @statusTaskCompletionRevision.
  ///
  /// In en, this message translates to:
  /// **'Completion revision'**
  String get statusTaskCompletionRevision;

  /// No description provided for @statusTaskFindings.
  ///
  /// In en, this message translates to:
  /// **'Findings'**
  String get statusTaskFindings;

  /// No description provided for @statusTaskRecommendation.
  ///
  /// In en, this message translates to:
  /// **'How to fix'**
  String get statusTaskRecommendation;

  /// No description provided for @statusTaskError.
  ///
  /// In en, this message translates to:
  /// **'Error'**
  String get statusTaskError;

  /// No description provided for @statusTaskFailed.
  ///
  /// In en, this message translates to:
  /// **'Task failed'**
  String get statusTaskFailed;

  /// No description provided for @statusTaskRecoverable.
  ///
  /// In en, this message translates to:
  /// **'Can continue'**
  String get statusTaskRecoverable;

  /// No description provided for @statusTaskFailures.
  ///
  /// In en, this message translates to:
  /// **'Task failures'**
  String get statusTaskFailures;

  /// No description provided for @statusTaskFatalHint.
  ///
  /// In en, this message translates to:
  /// **'Fix the provider or configuration, then start a new task.'**
  String get statusTaskFatalHint;

  /// No description provided for @statusTaskRecoverableHint.
  ///
  /// In en, this message translates to:
  /// **'This task can continue after the cause is corrected.'**
  String get statusTaskRecoverableHint;

  /// No description provided for @statusTaskExecution.
  ///
  /// In en, this message translates to:
  /// **'Execution'**
  String get statusTaskExecution;

  /// No description provided for @statusTaskBudget.
  ///
  /// In en, this message translates to:
  /// **'Budget limit'**
  String get statusTaskBudget;

  /// No description provided for @statusTaskBudgetSlice.
  ///
  /// In en, this message translates to:
  /// **'Budget slice'**
  String get statusTaskBudgetSlice;

  /// No description provided for @statusTaskBudgetSliceValue.
  ///
  /// In en, this message translates to:
  /// **'{current}/{limit}'**
  String statusTaskBudgetSliceValue(int current, int limit);

  /// No description provided for @statusTaskBudgetUsage.
  ///
  /// In en, this message translates to:
  /// **'{modelSteps} model · {toolCalls} tools · {waitCalls} waits · {elapsedMs} ms'**
  String statusTaskBudgetUsage(
    int modelSteps,
    int toolCalls,
    int waitCalls,
    String elapsedMs,
  );

  /// No description provided for @statusTaskContinuation.
  ///
  /// In en, this message translates to:
  /// **'Continuation'**
  String get statusTaskContinuation;

  /// No description provided for @statusTaskBudgetModelStep.
  ///
  /// In en, this message translates to:
  /// **'Model-step limit'**
  String get statusTaskBudgetModelStep;

  /// No description provided for @statusTaskBudgetToolCall.
  ///
  /// In en, this message translates to:
  /// **'Tool-call limit'**
  String get statusTaskBudgetToolCall;

  /// No description provided for @statusTaskBudgetWait.
  ///
  /// In en, this message translates to:
  /// **'Wait-call limit'**
  String get statusTaskBudgetWait;

  /// No description provided for @statusTaskBudgetWallClock.
  ///
  /// In en, this message translates to:
  /// **'Wall-clock limit'**
  String get statusTaskBudgetWallClock;

  /// No description provided for @statusTaskBudgetAgentCount.
  ///
  /// In en, this message translates to:
  /// **'Agent-count limit'**
  String get statusTaskBudgetAgentCount;

  /// No description provided for @statusTaskBudgetAgentDepth.
  ///
  /// In en, this message translates to:
  /// **'Agent-depth limit'**
  String get statusTaskBudgetAgentDepth;

  /// No description provided for @statusTaskBudgetFinalization.
  ///
  /// In en, this message translates to:
  /// **'Finalization limit'**
  String get statusTaskBudgetFinalization;

  /// No description provided for @statusTaskContinuationNone.
  ///
  /// In en, this message translates to:
  /// **'None'**
  String get statusTaskContinuationNone;

  /// No description provided for @statusTaskContinuationCompacting.
  ///
  /// In en, this message translates to:
  /// **'Compacting context'**
  String get statusTaskContinuationCompacting;

  /// No description provided for @statusTaskContinuationPendingStart.
  ///
  /// In en, this message translates to:
  /// **'Starting next slice'**
  String get statusTaskContinuationPendingStart;

  /// No description provided for @statusTaskContinuationNeedsAttention.
  ///
  /// In en, this message translates to:
  /// **'Needs attention'**
  String get statusTaskContinuationNeedsAttention;

  /// No description provided for @statusTaskPhaseDesignUpdating.
  ///
  /// In en, this message translates to:
  /// **'Updating design'**
  String get statusTaskPhaseDesignUpdating;

  /// No description provided for @statusTaskPhaseImplementing.
  ///
  /// In en, this message translates to:
  /// **'Implementing'**
  String get statusTaskPhaseImplementing;

  /// No description provided for @statusTaskPhaseMerging.
  ///
  /// In en, this message translates to:
  /// **'Merging'**
  String get statusTaskPhaseMerging;

  /// No description provided for @statusTaskPhaseReviewing.
  ///
  /// In en, this message translates to:
  /// **'Reviewing'**
  String get statusTaskPhaseReviewing;

  /// No description provided for @statusTaskPhaseReworking.
  ///
  /// In en, this message translates to:
  /// **'Reworking'**
  String get statusTaskPhaseReworking;

  /// No description provided for @statusTaskPhaseStopping.
  ///
  /// In en, this message translates to:
  /// **'Stopping'**
  String get statusTaskPhaseStopping;

  /// No description provided for @statusTaskPhaseCompleted.
  ///
  /// In en, this message translates to:
  /// **'Task completed'**
  String get statusTaskPhaseCompleted;

  /// No description provided for @statusTaskPhaseBlocked.
  ///
  /// In en, this message translates to:
  /// **'Task blocked'**
  String get statusTaskPhaseBlocked;

  /// No description provided for @statusTaskPhaseFailed.
  ///
  /// In en, this message translates to:
  /// **'Task failed'**
  String get statusTaskPhaseFailed;

  /// No description provided for @statusTaskPhaseCancelled.
  ///
  /// In en, this message translates to:
  /// **'Task cancelled'**
  String get statusTaskPhaseCancelled;

  /// No description provided for @statusTaskStatusPending.
  ///
  /// In en, this message translates to:
  /// **'Pending'**
  String get statusTaskStatusPending;

  /// No description provided for @statusTaskStatusQueued.
  ///
  /// In en, this message translates to:
  /// **'Queued'**
  String get statusTaskStatusQueued;

  /// No description provided for @statusTaskStatusRunning.
  ///
  /// In en, this message translates to:
  /// **'Running'**
  String get statusTaskStatusRunning;

  /// No description provided for @statusTaskStatusAwaitingCompletion.
  ///
  /// In en, this message translates to:
  /// **'Awaiting completion'**
  String get statusTaskStatusAwaitingCompletion;

  /// No description provided for @statusTaskStatusReadyForReview.
  ///
  /// In en, this message translates to:
  /// **'Ready for review'**
  String get statusTaskStatusReadyForReview;

  /// No description provided for @statusTaskStatusReviewing.
  ///
  /// In en, this message translates to:
  /// **'Reviewing'**
  String get statusTaskStatusReviewing;

  /// No description provided for @statusTaskStatusChangesRequested.
  ///
  /// In en, this message translates to:
  /// **'Changes requested'**
  String get statusTaskStatusChangesRequested;

  /// No description provided for @statusTaskStatusApproved.
  ///
  /// In en, this message translates to:
  /// **'Approved'**
  String get statusTaskStatusApproved;

  /// No description provided for @statusTaskStatusMerged.
  ///
  /// In en, this message translates to:
  /// **'Merged'**
  String get statusTaskStatusMerged;

  /// No description provided for @statusTaskStatusNoDelivery.
  ///
  /// In en, this message translates to:
  /// **'No delivery'**
  String get statusTaskStatusNoDelivery;

  /// No description provided for @statusTaskStatusCompleted.
  ///
  /// In en, this message translates to:
  /// **'Completed'**
  String get statusTaskStatusCompleted;

  /// No description provided for @statusTaskStatusBudgetLimited.
  ///
  /// In en, this message translates to:
  /// **'Budget limited'**
  String get statusTaskStatusBudgetLimited;

  /// No description provided for @statusTaskStatusNeedsAttention.
  ///
  /// In en, this message translates to:
  /// **'Needs attention'**
  String get statusTaskStatusNeedsAttention;

  /// No description provided for @statusTaskStatusFailed.
  ///
  /// In en, this message translates to:
  /// **'Failed'**
  String get statusTaskStatusFailed;

  /// No description provided for @statusTaskStatusCancelled.
  ///
  /// In en, this message translates to:
  /// **'Cancelled'**
  String get statusTaskStatusCancelled;

  /// No description provided for @statusTaskStatusPass.
  ///
  /// In en, this message translates to:
  /// **'Passed'**
  String get statusTaskStatusPass;

  /// No description provided for @statusTaskStatusChangesRequired.
  ///
  /// In en, this message translates to:
  /// **'Changes required'**
  String get statusTaskStatusChangesRequired;

  /// No description provided for @statusTaskStatusBlocked.
  ///
  /// In en, this message translates to:
  /// **'Blocked'**
  String get statusTaskStatusBlocked;

  /// No description provided for @statusSkillsSection.
  ///
  /// In en, this message translates to:
  /// **'Skills'**
  String get statusSkillsSection;

  /// No description provided for @statusMcpSection.
  ///
  /// In en, this message translates to:
  /// **'MCP'**
  String get statusMcpSection;

  /// No description provided for @statusLspSection.
  ///
  /// In en, this message translates to:
  /// **'LSP'**
  String get statusLspSection;

  /// No description provided for @statusSubagentsSection.
  ///
  /// In en, this message translates to:
  /// **'Subagents'**
  String get statusSubagentsSection;

  /// No description provided for @statusAgentChipTooltip.
  ///
  /// In en, this message translates to:
  /// **'Subagent status'**
  String get statusAgentChipTooltip;

  /// No description provided for @agentDetailTitle.
  ///
  /// In en, this message translates to:
  /// **'Subagents'**
  String get agentDetailTitle;

  /// No description provided for @agentDetailSummary.
  ///
  /// In en, this message translates to:
  /// **'{count, plural, =1{1 agent · {running} running} other{{count} agents · {running} running}}'**
  String agentDetailSummary(int count, int running);

  /// No description provided for @agentDetailEmpty.
  ///
  /// In en, this message translates to:
  /// **'No subagents'**
  String get agentDetailEmpty;

  /// No description provided for @agentDetailStatusQueued.
  ///
  /// In en, this message translates to:
  /// **'Queued'**
  String get agentDetailStatusQueued;

  /// No description provided for @agentDetailStatusRunning.
  ///
  /// In en, this message translates to:
  /// **'Running'**
  String get agentDetailStatusRunning;

  /// No description provided for @agentDetailStatusWaiting.
  ///
  /// In en, this message translates to:
  /// **'Waiting'**
  String get agentDetailStatusWaiting;

  /// No description provided for @agentDetailStatusCompleted.
  ///
  /// In en, this message translates to:
  /// **'Completed'**
  String get agentDetailStatusCompleted;

  /// No description provided for @agentDetailStatusErrored.
  ///
  /// In en, this message translates to:
  /// **'Errored'**
  String get agentDetailStatusErrored;

  /// No description provided for @agentDetailStatusInterrupted.
  ///
  /// In en, this message translates to:
  /// **'Interrupted'**
  String get agentDetailStatusInterrupted;

  /// No description provided for @agentDetailStatusShutdown.
  ///
  /// In en, this message translates to:
  /// **'Shutdown'**
  String get agentDetailStatusShutdown;

  /// No description provided for @agentDetailStatusNotFound.
  ///
  /// In en, this message translates to:
  /// **'Not found'**
  String get agentDetailStatusNotFound;

  /// No description provided for @agentDetailSummaryLabel.
  ///
  /// In en, this message translates to:
  /// **'Summary'**
  String get agentDetailSummaryLabel;

  /// No description provided for @agentDetailErrorLabel.
  ///
  /// In en, this message translates to:
  /// **'Error'**
  String get agentDetailErrorLabel;

  /// No description provided for @agentDetailReasonLabel.
  ///
  /// In en, this message translates to:
  /// **'Reason'**
  String get agentDetailReasonLabel;

  /// No description provided for @agentDetailPathLabel.
  ///
  /// In en, this message translates to:
  /// **'Path'**
  String get agentDetailPathLabel;

  /// No description provided for @timelineEmptyTitle.
  ///
  /// In en, this message translates to:
  /// **'No messages yet'**
  String get timelineEmptyTitle;

  /// No description provided for @timelineEmptyMessage.
  ///
  /// In en, this message translates to:
  /// **'Open a project or start a session to begin.'**
  String get timelineEmptyMessage;

  /// No description provided for @timelineJumpToLatest.
  ///
  /// In en, this message translates to:
  /// **'Jump to latest'**
  String get timelineJumpToLatest;

  /// No description provided for @timelineNew.
  ///
  /// In en, this message translates to:
  /// **'New'**
  String get timelineNew;

  /// No description provided for @timelineReasoningFallback.
  ///
  /// In en, this message translates to:
  /// **'Thinking'**
  String get timelineReasoningFallback;

  /// No description provided for @timelineReasoningActive.
  ///
  /// In en, this message translates to:
  /// **'Thinking'**
  String get timelineReasoningActive;

  /// No description provided for @timelineReasoningCompleted.
  ///
  /// In en, this message translates to:
  /// **'Thought'**
  String get timelineReasoningCompleted;

  /// No description provided for @timelineReasoningEmpty.
  ///
  /// In en, this message translates to:
  /// **'No reasoning text was provided.'**
  String get timelineReasoningEmpty;

  /// No description provided for @timelineToolFallback.
  ///
  /// In en, this message translates to:
  /// **'Tool'**
  String get timelineToolFallback;

  /// No description provided for @timelineToolGroupTitle.
  ///
  /// In en, this message translates to:
  /// **'Tool activity'**
  String get timelineToolGroupTitle;

  /// No description provided for @timelineToolGroupSummary.
  ///
  /// In en, this message translates to:
  /// **'{count} tools'**
  String timelineToolGroupSummary(int count);

  /// No description provided for @timelineToolGroupSummaryRunning.
  ///
  /// In en, this message translates to:
  /// **'{count} tools, {runningCount} running'**
  String timelineToolGroupSummaryRunning(int count, int runningCount);

  /// No description provided for @timelineToolGroupSummaryIssues.
  ///
  /// In en, this message translates to:
  /// **'{count} tools, {issueCount} need attention'**
  String timelineToolGroupSummaryIssues(int count, int issueCount);

  /// No description provided for @timelineToolGroupSummaryRunningWithIssues.
  ///
  /// In en, this message translates to:
  /// **'{count} tools, {runningCount} running, {issueCount} need attention'**
  String timelineToolGroupSummaryRunningWithIssues(
    int count,
    int runningCount,
    int issueCount,
  );

  /// No description provided for @timelinePlanFallback.
  ///
  /// In en, this message translates to:
  /// **'Plan'**
  String get timelinePlanFallback;

  /// No description provided for @timelineAgentFallback.
  ///
  /// In en, this message translates to:
  /// **'Agent'**
  String get timelineAgentFallback;

  /// No description provided for @timelineToolCompleted.
  ///
  /// In en, this message translates to:
  /// **'{name} completed'**
  String timelineToolCompleted(String name);

  /// No description provided for @timelineToolFailed.
  ///
  /// In en, this message translates to:
  /// **'{name} failed'**
  String timelineToolFailed(String name);

  /// No description provided for @timelineToolDenied.
  ///
  /// In en, this message translates to:
  /// **'{name} denied'**
  String timelineToolDenied(String name);

  /// No description provided for @timelineToolAwaitingApproval.
  ///
  /// In en, this message translates to:
  /// **'{name} awaiting approval'**
  String timelineToolAwaitingApproval(String name);

  /// No description provided for @timelineToolRunning.
  ///
  /// In en, this message translates to:
  /// **'{name} running'**
  String timelineToolRunning(String name);

  /// No description provided for @timelineToolExitCode.
  ///
  /// In en, this message translates to:
  /// **'exit code {code}'**
  String timelineToolExitCode(int code);

  /// No description provided for @timelineToolTimedOut.
  ///
  /// In en, this message translates to:
  /// **'timed out'**
  String get timelineToolTimedOut;

  /// No description provided for @timelineAgentSubagent.
  ///
  /// In en, this message translates to:
  /// **'Subagent'**
  String get timelineAgentSubagent;

  /// No description provided for @timelineAgentSubagentMessage.
  ///
  /// In en, this message translates to:
  /// **'Subagent message'**
  String get timelineAgentSubagentMessage;

  /// No description provided for @timelineAgentWaiting.
  ///
  /// In en, this message translates to:
  /// **'Waiting for subagents'**
  String get timelineAgentWaiting;

  /// No description provided for @timelineAgentClose.
  ///
  /// In en, this message translates to:
  /// **'Close subagent'**
  String get timelineAgentClose;

  /// No description provided for @timelineTodoListFallback.
  ///
  /// In en, this message translates to:
  /// **'Todo list'**
  String get timelineTodoListFallback;

  /// No description provided for @timelineTodoPending.
  ///
  /// In en, this message translates to:
  /// **'Pending'**
  String get timelineTodoPending;

  /// No description provided for @timelineTodoInProgress.
  ///
  /// In en, this message translates to:
  /// **'In progress'**
  String get timelineTodoInProgress;

  /// No description provided for @timelineTodoCompleted.
  ///
  /// In en, this message translates to:
  /// **'Completed'**
  String get timelineTodoCompleted;

  /// No description provided for @interactionQuestionsTitle.
  ///
  /// In en, this message translates to:
  /// **'A few questions'**
  String get interactionQuestionsTitle;

  /// No description provided for @interactionLastQuestion.
  ///
  /// In en, this message translates to:
  /// **'Last question'**
  String get interactionLastQuestion;

  /// No description provided for @interactionContinueAfterAnswer.
  ///
  /// In en, this message translates to:
  /// **'Continue after answering'**
  String get interactionContinueAfterAnswer;

  /// No description provided for @taskResumeTitle.
  ///
  /// In en, this message translates to:
  /// **'Task paused after restart'**
  String get taskResumeTitle;

  /// No description provided for @taskResumeBody.
  ///
  /// In en, this message translates to:
  /// **'Continue from canonical Task and agent state without adding a new prompt.'**
  String get taskResumeBody;

  /// No description provided for @taskResumeAction.
  ///
  /// In en, this message translates to:
  /// **'Continue task'**
  String get taskResumeAction;

  /// No description provided for @taskRecoveryDialogTitle.
  ///
  /// In en, this message translates to:
  /// **'Recover and continue'**
  String get taskRecoveryDialogTitle;

  /// No description provided for @taskRecoveryDialogBody.
  ///
  /// In en, this message translates to:
  /// **'Only the selected Thread model context is rolled back. Task, WorkUnit, workspace, commits, usage, and audit history are preserved.'**
  String get taskRecoveryDialogBody;

  /// No description provided for @taskRecoveryTargetLabel.
  ///
  /// In en, this message translates to:
  /// **'Recovery target'**
  String get taskRecoveryTargetLabel;

  /// No description provided for @taskRecoveryTargetPlanner.
  ///
  /// In en, this message translates to:
  /// **'Planner'**
  String get taskRecoveryTargetPlanner;

  /// No description provided for @taskRecoveryTargetExecutor.
  ///
  /// In en, this message translates to:
  /// **'Executor'**
  String get taskRecoveryTargetExecutor;

  /// No description provided for @taskRecoveryTurnSuffixLabel.
  ///
  /// In en, this message translates to:
  /// **'Contiguous tail Turns to roll back'**
  String get taskRecoveryTurnSuffixLabel;

  /// No description provided for @taskRecoveryModeLabel.
  ///
  /// In en, this message translates to:
  /// **'Recovery mode'**
  String get taskRecoveryModeLabel;

  /// No description provided for @taskRecoveryModeRewind.
  ///
  /// In en, this message translates to:
  /// **'Rewind conversation tail'**
  String get taskRecoveryModeRewind;

  /// No description provided for @taskRecoveryModeRebuild.
  ///
  /// In en, this message translates to:
  /// **'Rebuild this Thread context'**
  String get taskRecoveryModeRebuild;

  /// No description provided for @taskRecoveryGitPreserved.
  ///
  /// In en, this message translates to:
  /// **'Git and workspace state will not be reset or cleaned. Their fingerprint must exactly match this preview when recovery is applied.'**
  String get taskRecoveryGitPreserved;

  /// No description provided for @taskRecoveryRebuildWarning.
  ///
  /// In en, this message translates to:
  /// **'When no safe prefix can be preserved, only this Thread\'s ordinary transcript is cleared. Handoff, evidence, session notes, and external state remain.'**
  String get taskRecoveryRebuildWarning;

  /// No description provided for @taskRecoveryFirstConfirm.
  ///
  /// In en, this message translates to:
  /// **'Review recovery impact'**
  String get taskRecoveryFirstConfirm;

  /// No description provided for @taskRecoveryFinalConfirm.
  ///
  /// In en, this message translates to:
  /// **'Confirm recovery and continue'**
  String get taskRecoveryFinalConfirm;

  /// No description provided for @taskRecoveryApplying.
  ///
  /// In en, this message translates to:
  /// **'Recovering…'**
  String get taskRecoveryApplying;

  /// No description provided for @taskRecoveryItems.
  ///
  /// In en, this message translates to:
  /// **'items'**
  String get taskRecoveryItems;

  /// No description provided for @taskRecoveryInputs.
  ///
  /// In en, this message translates to:
  /// **'inputs'**
  String get taskRecoveryInputs;

  /// No description provided for @taskRecoveryTools.
  ///
  /// In en, this message translates to:
  /// **'tools'**
  String get taskRecoveryTools;

  /// No description provided for @timelineRolledBack.
  ///
  /// In en, this message translates to:
  /// **'Rolled back from active context'**
  String get timelineRolledBack;

  /// No description provided for @interactionSubmitEmptyAnswersHint.
  ///
  /// In en, this message translates to:
  /// **'Unanswered questions are submitted as empty arrays.'**
  String get interactionSubmitEmptyAnswersHint;

  /// No description provided for @interactionAnsweredPendingHint.
  ///
  /// In en, this message translates to:
  /// **'{answeredCount} answered · {pendingCount} pending'**
  String interactionAnsweredPendingHint(int answeredCount, int pendingCount);

  /// No description provided for @interactionPreviousQuestion.
  ///
  /// In en, this message translates to:
  /// **'Previous'**
  String get interactionPreviousQuestion;

  /// No description provided for @interactionNextQuestion.
  ///
  /// In en, this message translates to:
  /// **'Next'**
  String get interactionNextQuestion;

  /// No description provided for @interactionSubmitAnswers.
  ///
  /// In en, this message translates to:
  /// **'Submit answers'**
  String get interactionSubmitAnswers;

  /// No description provided for @interactionNeedInputTitle.
  ///
  /// In en, this message translates to:
  /// **'Input needed'**
  String get interactionNeedInputTitle;

  /// No description provided for @interactionAnswerHint.
  ///
  /// In en, this message translates to:
  /// **'Pure will use this answer to continue the current question.'**
  String get interactionAnswerHint;

  /// No description provided for @interactionAnswerButton.
  ///
  /// In en, this message translates to:
  /// **'Answer'**
  String get interactionAnswerButton;

  /// No description provided for @interactionAnswerLabel.
  ///
  /// In en, this message translates to:
  /// **'Answer'**
  String get interactionAnswerLabel;

  /// No description provided for @interactionQuestionProgress.
  ///
  /// In en, this message translates to:
  /// **'Question {current} / {total}'**
  String interactionQuestionProgress(int current, int total);

  /// No description provided for @interactionAnsweredCount.
  ///
  /// In en, this message translates to:
  /// **'{count} answered'**
  String interactionAnsweredCount(int count);

  /// No description provided for @interactionQuestionTooltip.
  ///
  /// In en, this message translates to:
  /// **'Question {index}'**
  String interactionQuestionTooltip(int index);

  /// No description provided for @interactionQuestionFallback.
  ///
  /// In en, this message translates to:
  /// **'Question'**
  String get interactionQuestionFallback;

  /// No description provided for @interactionOtherLabel.
  ///
  /// In en, this message translates to:
  /// **'Other'**
  String get interactionOtherLabel;

  /// No description provided for @interactionSecretHint.
  ///
  /// In en, this message translates to:
  /// **'Enter secret answer'**
  String get interactionSecretHint;

  /// No description provided for @interactionTextHint.
  ///
  /// In en, this message translates to:
  /// **'Enter your answer...'**
  String get interactionTextHint;

  /// No description provided for @interactionPermissionTitle.
  ///
  /// In en, this message translates to:
  /// **'Permission needed'**
  String get interactionPermissionTitle;

  /// No description provided for @interactionPermissionSubtitle.
  ///
  /// In en, this message translates to:
  /// **'Pure wants to run a tool call'**
  String get interactionPermissionSubtitle;

  /// No description provided for @interactionPermissionFooterHint.
  ///
  /// In en, this message translates to:
  /// **'The tool runs in the current working directory; adjust permission mode in the composer.'**
  String get interactionPermissionFooterHint;

  /// No description provided for @interactionReject.
  ///
  /// In en, this message translates to:
  /// **'Reject'**
  String get interactionReject;

  /// No description provided for @interactionApprove.
  ///
  /// In en, this message translates to:
  /// **'Approve'**
  String get interactionApprove;

  /// No description provided for @interactionReasonLabel.
  ///
  /// In en, this message translates to:
  /// **'Reason'**
  String get interactionReasonLabel;

  /// No description provided for @interactionPlanConfirmTitle.
  ///
  /// In en, this message translates to:
  /// **'Implement this plan?'**
  String get interactionPlanConfirmTitle;

  /// No description provided for @interactionPlanConfirmSubtitle.
  ///
  /// In en, this message translates to:
  /// **'Implement it, or describe what should change.'**
  String get interactionPlanConfirmSubtitle;

  /// No description provided for @interactionPlanEditingFooterHint.
  ///
  /// In en, this message translates to:
  /// **'Only your adjustment will be sent; the plan body is not returned.'**
  String get interactionPlanEditingFooterHint;

  /// No description provided for @interactionPlanImplementFooterHint.
  ///
  /// In en, this message translates to:
  /// **'Implementing switches back to {mode} mode and starts execution.'**
  String interactionPlanImplementFooterHint(String mode);

  /// No description provided for @interactionPlanIgnore.
  ///
  /// In en, this message translates to:
  /// **'Ignore'**
  String get interactionPlanIgnore;

  /// No description provided for @interactionPlanAdjust.
  ///
  /// In en, this message translates to:
  /// **'Tell Pure how to adjust'**
  String get interactionPlanAdjust;

  /// No description provided for @interactionPlanImplement.
  ///
  /// In en, this message translates to:
  /// **'Implement this plan'**
  String get interactionPlanImplement;

  /// No description provided for @interactionPlanAdjustHint.
  ///
  /// In en, this message translates to:
  /// **'Describe what should change...'**
  String get interactionPlanAdjustHint;

  /// No description provided for @interactionPlanAdjustSubmit.
  ///
  /// In en, this message translates to:
  /// **'Submit adjustment'**
  String get interactionPlanAdjustSubmit;

  /// No description provided for @interactionPlanEditingNotice.
  ///
  /// In en, this message translates to:
  /// **'Continue planning: only your adjustment will be submitted.'**
  String get interactionPlanEditingNotice;

  /// No description provided for @interactionPlanViewNotice.
  ///
  /// In en, this message translates to:
  /// **'Plan content is not edited here; review the full plan in the timeline.'**
  String get interactionPlanViewNotice;

  /// No description provided for @interactionPlanContinueReason.
  ///
  /// In en, this message translates to:
  /// **'continue planning'**
  String get interactionPlanContinueReason;

  /// No description provided for @settingsProvidersTitle.
  ///
  /// In en, this message translates to:
  /// **'Providers'**
  String get settingsProvidersTitle;

  /// No description provided for @settingsProvidersSubtitle.
  ///
  /// In en, this message translates to:
  /// **'Model providers, credentials, models, and usage'**
  String get settingsProvidersSubtitle;

  /// No description provided for @settingsRefreshUsage.
  ///
  /// In en, this message translates to:
  /// **'Refresh usage'**
  String get settingsRefreshUsage;

  /// No description provided for @settingsAddProvider.
  ///
  /// In en, this message translates to:
  /// **'Add provider'**
  String get settingsAddProvider;

  /// No description provided for @settingsSearchProviders.
  ///
  /// In en, this message translates to:
  /// **'Search providers'**
  String get settingsSearchProviders;

  /// No description provided for @settingsNoProvidersMatchTitle.
  ///
  /// In en, this message translates to:
  /// **'No providers match this filter'**
  String get settingsNoProvidersMatchTitle;

  /// No description provided for @settingsNoProvidersMatchMessage.
  ///
  /// In en, this message translates to:
  /// **'Clear the search to see all configured providers.'**
  String get settingsNoProvidersMatchMessage;

  /// No description provided for @settingsNoProvidersTitle.
  ///
  /// In en, this message translates to:
  /// **'No providers found'**
  String get settingsNoProvidersTitle;

  /// No description provided for @settingsNoProvidersMessage.
  ///
  /// In en, this message translates to:
  /// **'Add a provider to configure credentials and models.'**
  String get settingsNoProvidersMessage;

  /// No description provided for @settingsDefaultProvider.
  ///
  /// In en, this message translates to:
  /// **'Default provider'**
  String get settingsDefaultProvider;

  /// No description provided for @settingsSetAsDefaultProvider.
  ///
  /// In en, this message translates to:
  /// **'Set as default'**
  String get settingsSetAsDefaultProvider;

  /// No description provided for @settingsOpenDetails.
  ///
  /// In en, this message translates to:
  /// **'Open details'**
  String get settingsOpenDetails;

  /// No description provided for @settingsProviderActions.
  ///
  /// In en, this message translates to:
  /// **'Provider actions'**
  String get settingsProviderActions;

  /// No description provided for @settingsEditProvider.
  ///
  /// In en, this message translates to:
  /// **'Edit provider'**
  String get settingsEditProvider;

  /// No description provided for @settingsDeleteProvider.
  ///
  /// In en, this message translates to:
  /// **'Delete provider'**
  String get settingsDeleteProvider;

  /// No description provided for @settingsNoProviderSelected.
  ///
  /// In en, this message translates to:
  /// **'No provider selected'**
  String get settingsNoProviderSelected;

  /// No description provided for @settingsProviderTitle.
  ///
  /// In en, this message translates to:
  /// **'Provider'**
  String get settingsProviderTitle;

  /// No description provided for @settingsProviderModelsTitle.
  ///
  /// In en, this message translates to:
  /// **'Models'**
  String get settingsProviderModelsTitle;

  /// No description provided for @settingsProviderConnectionTitle.
  ///
  /// In en, this message translates to:
  /// **'Connection'**
  String get settingsProviderConnectionTitle;

  /// No description provided for @settingsProviderDefaultModelsTitle.
  ///
  /// In en, this message translates to:
  /// **'Default models'**
  String get settingsProviderDefaultModelsTitle;

  /// No description provided for @settingsProviderCustomModelsTitle.
  ///
  /// In en, this message translates to:
  /// **'Custom models'**
  String get settingsProviderCustomModelsTitle;

  /// No description provided for @settingsNewProvider.
  ///
  /// In en, this message translates to:
  /// **'New provider'**
  String get settingsNewProvider;

  /// No description provided for @settingsProviderKey.
  ///
  /// In en, this message translates to:
  /// **'Provider key'**
  String get settingsProviderKey;

  /// No description provided for @settingsTemplate.
  ///
  /// In en, this message translates to:
  /// **'Template'**
  String get settingsTemplate;

  /// No description provided for @settingsCustomProvider.
  ///
  /// In en, this message translates to:
  /// **'Custom provider'**
  String get settingsCustomProvider;

  /// No description provided for @settingsDefaultModel.
  ///
  /// In en, this message translates to:
  /// **'Default model'**
  String get settingsDefaultModel;

  /// No description provided for @settingsApiKey.
  ///
  /// In en, this message translates to:
  /// **'API key'**
  String get settingsApiKey;

  /// No description provided for @settingsApiKeyKeepCurrent.
  ///
  /// In en, this message translates to:
  /// **'API key (leave blank to keep current)'**
  String get settingsApiKeyKeepCurrent;

  /// No description provided for @settingsConfigured.
  ///
  /// In en, this message translates to:
  /// **'configured'**
  String get settingsConfigured;

  /// No description provided for @settingsMissing.
  ///
  /// In en, this message translates to:
  /// **'missing'**
  String get settingsMissing;

  /// No description provided for @settingsDisplayName.
  ///
  /// In en, this message translates to:
  /// **'Display name'**
  String get settingsDisplayName;

  /// No description provided for @settingsProtocolType.
  ///
  /// In en, this message translates to:
  /// **'Protocol type'**
  String get settingsProtocolType;

  /// No description provided for @settingsBaseUrl.
  ///
  /// In en, this message translates to:
  /// **'Base URL'**
  String get settingsBaseUrl;

  /// No description provided for @settingsModelSlug.
  ///
  /// In en, this message translates to:
  /// **'Model slug'**
  String get settingsModelSlug;

  /// No description provided for @settingsReasoningEfforts.
  ///
  /// In en, this message translates to:
  /// **'Reasoning efforts'**
  String get settingsReasoningEfforts;

  /// No description provided for @settingsEdit.
  ///
  /// In en, this message translates to:
  /// **'Edit'**
  String get settingsEdit;

  /// No description provided for @settingsCancel.
  ///
  /// In en, this message translates to:
  /// **'Cancel'**
  String get settingsCancel;

  /// No description provided for @settingsSave.
  ///
  /// In en, this message translates to:
  /// **'Save'**
  String get settingsSave;

  /// No description provided for @settingsAddModel.
  ///
  /// In en, this message translates to:
  /// **'Add model'**
  String get settingsAddModel;

  /// No description provided for @settingsRemoveModel.
  ///
  /// In en, this message translates to:
  /// **'Remove model'**
  String get settingsRemoveModel;

  /// No description provided for @settingsNoCustomModels.
  ///
  /// In en, this message translates to:
  /// **'No custom models'**
  String get settingsNoCustomModels;

  /// No description provided for @settingsBundledModels.
  ///
  /// In en, this message translates to:
  /// **'{count} bundled'**
  String settingsBundledModels(int count);

  /// No description provided for @settingsDefaultBadge.
  ///
  /// In en, this message translates to:
  /// **'default'**
  String get settingsDefaultBadge;

  /// No description provided for @settingsReadyBadge.
  ///
  /// In en, this message translates to:
  /// **'ready'**
  String get settingsReadyBadge;

  /// No description provided for @settingsSetupBadge.
  ///
  /// In en, this message translates to:
  /// **'setup'**
  String get settingsSetupBadge;

  /// No description provided for @settingsUsageTitle.
  ///
  /// In en, this message translates to:
  /// **'Usage'**
  String get settingsUsageTitle;

  /// No description provided for @settingsUsageUpdated.
  ///
  /// In en, this message translates to:
  /// **'Updated {updatedAt}'**
  String settingsUsageUpdated(String updatedAt);

  /// No description provided for @settingsUsageAvailableBalance.
  ///
  /// In en, this message translates to:
  /// **'Available balance'**
  String get settingsUsageAvailableBalance;

  /// No description provided for @settingsUsageBalanceUnavailable.
  ///
  /// In en, this message translates to:
  /// **'Balance unavailable'**
  String get settingsUsageBalanceUnavailable;

  /// No description provided for @settingsUsageGranted.
  ///
  /// In en, this message translates to:
  /// **'Granted {amount}'**
  String settingsUsageGranted(String amount);

  /// No description provided for @settingsUsageToppedUp.
  ///
  /// In en, this message translates to:
  /// **'Topped up {amount}'**
  String settingsUsageToppedUp(String amount);

  /// No description provided for @settingsUsageRefreshing.
  ///
  /// In en, this message translates to:
  /// **'Refreshing usage...'**
  String get settingsUsageRefreshing;

  /// No description provided for @settingsUsageChecking.
  ///
  /// In en, this message translates to:
  /// **'Checking usage...'**
  String get settingsUsageChecking;

  /// No description provided for @settingsUsageCheckingShort.
  ///
  /// In en, this message translates to:
  /// **'Checking usage'**
  String get settingsUsageCheckingShort;

  /// No description provided for @settingsUsageNotLoaded.
  ///
  /// In en, this message translates to:
  /// **'Usage not loaded'**
  String get settingsUsageNotLoaded;

  /// No description provided for @settingsUsageUnsupported.
  ///
  /// In en, this message translates to:
  /// **'Unsupported'**
  String get settingsUsageUnsupported;

  /// No description provided for @settingsUsageNotSupported.
  ///
  /// In en, this message translates to:
  /// **'Usage not supported'**
  String get settingsUsageNotSupported;

  /// No description provided for @settingsUsageMissingKey.
  ///
  /// In en, this message translates to:
  /// **'Missing key'**
  String get settingsUsageMissingKey;

  /// No description provided for @settingsUsageFailed.
  ///
  /// In en, this message translates to:
  /// **'Usage failed'**
  String get settingsUsageFailed;

  /// No description provided for @settingsUsageQueryFailed.
  ///
  /// In en, this message translates to:
  /// **'Usage query failed'**
  String get settingsUsageQueryFailed;

  /// No description provided for @settingsUsageApiKeyMissing.
  ///
  /// In en, this message translates to:
  /// **'Provider API key is not configured'**
  String get settingsUsageApiKeyMissing;

  /// No description provided for @settingsUsageUnsupportedForProvider.
  ///
  /// In en, this message translates to:
  /// **'Usage is not supported for {providerName}'**
  String settingsUsageUnsupportedForProvider(String providerName);

  /// No description provided for @settingsUsageNotChecked.
  ///
  /// In en, this message translates to:
  /// **'Not checked'**
  String get settingsUsageNotChecked;

  /// No description provided for @settingsUsageUnavailable.
  ///
  /// In en, this message translates to:
  /// **'Usage is unavailable'**
  String get settingsUsageUnavailable;

  /// No description provided for @settingsUsageError.
  ///
  /// In en, this message translates to:
  /// **'Could not load usage'**
  String get settingsUsageError;

  /// No description provided for @settingsUsageNoQuota.
  ///
  /// In en, this message translates to:
  /// **'No quota details returned.'**
  String get settingsUsageNoQuota;

  /// No description provided for @settingsUsageTools.
  ///
  /// In en, this message translates to:
  /// **'Tools'**
  String get settingsUsageTools;

  /// No description provided for @settingsUsageToken.
  ///
  /// In en, this message translates to:
  /// **'Token usage'**
  String get settingsUsageToken;

  /// No description provided for @settingsUsageSpend.
  ///
  /// In en, this message translates to:
  /// **'Spend'**
  String get settingsUsageSpend;

  /// No description provided for @settingsUsageRemaining.
  ///
  /// In en, this message translates to:
  /// **'Remaining'**
  String get settingsUsageRemaining;

  /// No description provided for @settingsUsageUsed.
  ///
  /// In en, this message translates to:
  /// **'Used'**
  String get settingsUsageUsed;

  /// No description provided for @settingsUsageFiveHourQuota.
  ///
  /// In en, this message translates to:
  /// **'5 hour quota'**
  String get settingsUsageFiveHourQuota;

  /// No description provided for @settingsUsageWeeklyQuota.
  ///
  /// In en, this message translates to:
  /// **'Weekly quota'**
  String get settingsUsageWeeklyQuota;

  /// No description provided for @settingsUsageMcpQuota.
  ///
  /// In en, this message translates to:
  /// **'MCP quota'**
  String get settingsUsageMcpQuota;

  /// No description provided for @settingsUsageQuota.
  ///
  /// In en, this message translates to:
  /// **'Quota'**
  String get settingsUsageQuota;

  /// No description provided for @settingsUsageQuotaRemaining.
  ///
  /// In en, this message translates to:
  /// **'{remaining} of {total} remaining'**
  String settingsUsageQuotaRemaining(String remaining, String total);

  /// No description provided for @settingsUsageQuotaUsed.
  ///
  /// In en, this message translates to:
  /// **'{current} of {total} used'**
  String settingsUsageQuotaUsed(String current, String total);

  /// No description provided for @settingsUsagePercentRemaining.
  ///
  /// In en, this message translates to:
  /// **'{percent} remaining'**
  String settingsUsagePercentRemaining(String percent);

  /// No description provided for @settingsUsageReset.
  ///
  /// In en, this message translates to:
  /// **'Reset {time}'**
  String settingsUsageReset(String time);

  /// No description provided for @settingsInstructionsTitle.
  ///
  /// In en, this message translates to:
  /// **'Instructions'**
  String get settingsInstructionsTitle;

  /// No description provided for @settingsInstructionsSubtitle.
  ///
  /// In en, this message translates to:
  /// **'Injected into each turn; changes save after typing stops.'**
  String get settingsInstructionsSubtitle;

  /// No description provided for @settingsBaseInstructions.
  ///
  /// In en, this message translates to:
  /// **'Base instructions'**
  String get settingsBaseInstructions;

  /// No description provided for @settingsDeveloperInstructions.
  ///
  /// In en, this message translates to:
  /// **'Developer instructions'**
  String get settingsDeveloperInstructions;

  /// No description provided for @settingsUserContext.
  ///
  /// In en, this message translates to:
  /// **'User context'**
  String get settingsUserContext;

  /// No description provided for @settingsInstructionHint.
  ///
  /// In en, this message translates to:
  /// **'Add project guidance here'**
  String get settingsInstructionHint;

  /// No description provided for @settingsSkillsTitle.
  ///
  /// In en, this message translates to:
  /// **'Skills'**
  String get settingsSkillsTitle;

  /// No description provided for @settingsSkillsSubtitle.
  ///
  /// In en, this message translates to:
  /// **'Disable noisy skills or discover project/user/system catalogs.'**
  String get settingsSkillsSubtitle;

  /// No description provided for @settingsDiscover.
  ///
  /// In en, this message translates to:
  /// **'Discover'**
  String get settingsDiscover;

  /// No description provided for @settingsDiscovering.
  ///
  /// In en, this message translates to:
  /// **'Discovering'**
  String get settingsDiscovering;

  /// No description provided for @settingsFilterSkills.
  ///
  /// In en, this message translates to:
  /// **'Filter skills'**
  String get settingsFilterSkills;

  /// No description provided for @settingsSkillDisabled.
  ///
  /// In en, this message translates to:
  /// **'Disabled for this workspace'**
  String get settingsSkillDisabled;

  /// No description provided for @settingsSkillEnabled.
  ///
  /// In en, this message translates to:
  /// **'Enabled'**
  String get settingsSkillEnabled;

  /// No description provided for @settingsOpenProjectToDiscoverSkills.
  ///
  /// In en, this message translates to:
  /// **'Open a project to discover skills'**
  String get settingsOpenProjectToDiscoverSkills;

  /// No description provided for @settingsNoSkillsMatchFilter.
  ///
  /// In en, this message translates to:
  /// **'No skills match this filter'**
  String get settingsNoSkillsMatchFilter;

  /// No description provided for @settingsSkillsDiscoverySources.
  ///
  /// In en, this message translates to:
  /// **'Skills are discovered from the selected workspace and configured user/system sources.'**
  String get settingsSkillsDiscoverySources;

  /// No description provided for @settingsClearSearchOrDiscoverAgain.
  ///
  /// In en, this message translates to:
  /// **'Clear the search or run discovery again.'**
  String get settingsClearSearchOrDiscoverAgain;

  /// No description provided for @settingsNoSkillsTitle.
  ///
  /// In en, this message translates to:
  /// **'No skills found'**
  String get settingsNoSkillsTitle;

  /// No description provided for @settingsNoSkillsMessage.
  ///
  /// In en, this message translates to:
  /// **'Try another filter or discover skills for this project.'**
  String get settingsNoSkillsMessage;

  /// No description provided for @settingsRolesTitle.
  ///
  /// In en, this message translates to:
  /// **'Roles'**
  String get settingsRolesTitle;

  /// No description provided for @settingsRolesSubtitle.
  ///
  /// In en, this message translates to:
  /// **'Choose provider/model defaults for each fixed agent role.'**
  String get settingsRolesSubtitle;

  /// No description provided for @settingsRoleExplorerDescription.
  ///
  /// In en, this message translates to:
  /// **'Explore code and collect context.'**
  String get settingsRoleExplorerDescription;

  /// No description provided for @settingsRolePlannerDescription.
  ///
  /// In en, this message translates to:
  /// **'Draft plans and structure intent.'**
  String get settingsRolePlannerDescription;

  /// No description provided for @settingsRoleExecutorDescription.
  ///
  /// In en, this message translates to:
  /// **'Apply edits and run tools.'**
  String get settingsRoleExecutorDescription;

  /// No description provided for @settingsRoleReviewerDescription.
  ///
  /// In en, this message translates to:
  /// **'Review results and verify risk.'**
  String get settingsRoleReviewerDescription;

  /// No description provided for @settingsRoleFallbackDescription.
  ///
  /// In en, this message translates to:
  /// **'Studio role'**
  String get settingsRoleFallbackDescription;

  /// No description provided for @settingsModelField.
  ///
  /// In en, this message translates to:
  /// **'Model'**
  String get settingsModelField;

  /// No description provided for @settingsMcpTitle.
  ///
  /// In en, this message translates to:
  /// **'MCP'**
  String get settingsMcpTitle;

  /// No description provided for @settingsMcpSubtitle.
  ///
  /// In en, this message translates to:
  /// **'Model Context Protocol servers and inline endpoints.'**
  String get settingsMcpSubtitle;

  /// No description provided for @settingsMcpRefresh.
  ///
  /// In en, this message translates to:
  /// **'Refresh'**
  String get settingsMcpRefresh;

  /// No description provided for @settingsMcpReconnect.
  ///
  /// In en, this message translates to:
  /// **'Reconnect'**
  String get settingsMcpReconnect;

  /// No description provided for @settingsMcpResetAll.
  ///
  /// In en, this message translates to:
  /// **'Reset all'**
  String get settingsMcpResetAll;

  /// No description provided for @settingsMcpResetConfirmTitle.
  ///
  /// In en, this message translates to:
  /// **'Reset all MCP servers?'**
  String get settingsMcpResetConfirmTitle;

  /// No description provided for @settingsMcpResetConfirmBody.
  ///
  /// In en, this message translates to:
  /// **'Every configured MCP connection will be rebuilt. Active turns keep their current leased generation.'**
  String get settingsMcpResetConfirmBody;

  /// No description provided for @settingsMcpResetConfirmAction.
  ///
  /// In en, this message translates to:
  /// **'Reset all'**
  String get settingsMcpResetConfirmAction;

  /// No description provided for @settingsEndpoint.
  ///
  /// In en, this message translates to:
  /// **'Endpoint'**
  String get settingsEndpoint;

  /// No description provided for @settingsMcpEmptyTitle.
  ///
  /// In en, this message translates to:
  /// **'No MCP servers'**
  String get settingsMcpEmptyTitle;

  /// No description provided for @settingsMcpEmptyMessage.
  ///
  /// In en, this message translates to:
  /// **'Configured MCP servers will appear here.'**
  String get settingsMcpEmptyMessage;

  /// No description provided for @settingsLspTitle.
  ///
  /// In en, this message translates to:
  /// **'Language servers'**
  String get settingsLspTitle;

  /// No description provided for @settingsLspSubtitle.
  ///
  /// In en, this message translates to:
  /// **'Last-known project language server status and explicit lifecycle commands.'**
  String get settingsLspSubtitle;

  /// No description provided for @settingsLspRefresh.
  ///
  /// In en, this message translates to:
  /// **'Refresh'**
  String get settingsLspRefresh;

  /// No description provided for @settingsLspProbe.
  ///
  /// In en, this message translates to:
  /// **'Probe'**
  String get settingsLspProbe;

  /// No description provided for @settingsLspRepair.
  ///
  /// In en, this message translates to:
  /// **'Repair'**
  String get settingsLspRepair;

  /// No description provided for @settingsLspReset.
  ///
  /// In en, this message translates to:
  /// **'Reset'**
  String get settingsLspReset;

  /// No description provided for @settingsLspResetWorkspace.
  ///
  /// In en, this message translates to:
  /// **'Reset workspace'**
  String get settingsLspResetWorkspace;

  /// No description provided for @settingsLspActivityIndexing.
  ///
  /// In en, this message translates to:
  /// **'Indexing'**
  String get settingsLspActivityIndexing;

  /// No description provided for @settingsLspActivityBusy.
  ///
  /// In en, this message translates to:
  /// **'Busy'**
  String get settingsLspActivityBusy;

  /// No description provided for @settingsLspEmptyTitle.
  ///
  /// In en, this message translates to:
  /// **'No language servers'**
  String get settingsLspEmptyTitle;

  /// No description provided for @settingsLspEmptyMessage.
  ///
  /// In en, this message translates to:
  /// **'Activate a supported project to create language server membership.'**
  String get settingsLspEmptyMessage;

  /// No description provided for @settingsSecurityTitle.
  ///
  /// In en, this message translates to:
  /// **'Security'**
  String get settingsSecurityTitle;

  /// No description provided for @settingsSecuritySubtitle.
  ///
  /// In en, this message translates to:
  /// **'Choose the default approval posture for this workspace.'**
  String get settingsSecuritySubtitle;

  /// No description provided for @settingsSecurityModeSubtitle.
  ///
  /// In en, this message translates to:
  /// **'Tool execution permission mode; changes apply immediately.'**
  String get settingsSecurityModeSubtitle;

  /// No description provided for @settingsCurrentMode.
  ///
  /// In en, this message translates to:
  /// **'Current: {mode}'**
  String settingsCurrentMode(String mode);

  /// No description provided for @settingsWorkspaceBoundary.
  ///
  /// In en, this message translates to:
  /// **'Workspace boundary policy remains unchanged.'**
  String get settingsWorkspaceBoundary;

  /// No description provided for @settingsGeneralTitle.
  ///
  /// In en, this message translates to:
  /// **'General'**
  String get settingsGeneralTitle;

  /// No description provided for @settingsGeneralSubtitle.
  ///
  /// In en, this message translates to:
  /// **'Interface preferences saved into the Studio store.'**
  String get settingsGeneralSubtitle;

  /// No description provided for @settingsFollowSystemTheme.
  ///
  /// In en, this message translates to:
  /// **'Follow system theme'**
  String get settingsFollowSystemTheme;

  /// No description provided for @settingsFollowSystemThemeSubtitle.
  ///
  /// In en, this message translates to:
  /// **'Switch light and dark mode with the OS.'**
  String get settingsFollowSystemThemeSubtitle;

  /// No description provided for @settingsFollowActiveTurn.
  ///
  /// In en, this message translates to:
  /// **'Follow active turn'**
  String get settingsFollowActiveTurn;

  /// No description provided for @settingsFollowActiveTurnSubtitle.
  ///
  /// In en, this message translates to:
  /// **'Keep new timeline output pinned to the latest turn.'**
  String get settingsFollowActiveTurnSubtitle;

  /// No description provided for @settingsCompactTimeline.
  ///
  /// In en, this message translates to:
  /// **'Compact timeline'**
  String get settingsCompactTimeline;

  /// No description provided for @settingsCompactTimelineSubtitle.
  ///
  /// In en, this message translates to:
  /// **'Reduce message spacing for denser reading.'**
  String get settingsCompactTimelineSubtitle;

  /// No description provided for @settingsWebSearchTitle.
  ///
  /// In en, this message translates to:
  /// **'Web search'**
  String get settingsWebSearchTitle;

  /// No description provided for @settingsWebSearchSubtitle.
  ///
  /// In en, this message translates to:
  /// **'Search through an eligible OpenAI account. Changes apply from the next turn.'**
  String get settingsWebSearchSubtitle;

  /// No description provided for @settingsWebSearchConfiguredMode.
  ///
  /// In en, this message translates to:
  /// **'Configured mode'**
  String get settingsWebSearchConfiguredMode;

  /// No description provided for @settingsWebSearchEffectiveMode.
  ///
  /// In en, this message translates to:
  /// **'Effective mode'**
  String get settingsWebSearchEffectiveMode;

  /// No description provided for @settingsWebSearchProvider.
  ///
  /// In en, this message translates to:
  /// **'OpenAI provider'**
  String get settingsWebSearchProvider;

  /// No description provided for @settingsWebSearchModel.
  ///
  /// In en, this message translates to:
  /// **'Search model'**
  String get settingsWebSearchModel;

  /// No description provided for @settingsWebSearchMode.
  ///
  /// In en, this message translates to:
  /// **'Mode'**
  String get settingsWebSearchMode;

  /// No description provided for @settingsWebSearchModeDisabled.
  ///
  /// In en, this message translates to:
  /// **'Disabled'**
  String get settingsWebSearchModeDisabled;

  /// No description provided for @settingsWebSearchModeCached.
  ///
  /// In en, this message translates to:
  /// **'Cached'**
  String get settingsWebSearchModeCached;

  /// No description provided for @settingsWebSearchModeIndexed.
  ///
  /// In en, this message translates to:
  /// **'Indexed'**
  String get settingsWebSearchModeIndexed;

  /// No description provided for @settingsWebSearchModeLive.
  ///
  /// In en, this message translates to:
  /// **'Live'**
  String get settingsWebSearchModeLive;

  /// No description provided for @settingsWebSearchContextSize.
  ///
  /// In en, this message translates to:
  /// **'Context size'**
  String get settingsWebSearchContextSize;

  /// No description provided for @settingsWebSearchContextLow.
  ///
  /// In en, this message translates to:
  /// **'Low'**
  String get settingsWebSearchContextLow;

  /// No description provided for @settingsWebSearchContextMedium.
  ///
  /// In en, this message translates to:
  /// **'Medium'**
  String get settingsWebSearchContextMedium;

  /// No description provided for @settingsWebSearchContextHigh.
  ///
  /// In en, this message translates to:
  /// **'High'**
  String get settingsWebSearchContextHigh;

  /// No description provided for @settingsServiceDefault.
  ///
  /// In en, this message translates to:
  /// **'Service default'**
  String get settingsServiceDefault;

  /// No description provided for @settingsWebSearchAllowedDomains.
  ///
  /// In en, this message translates to:
  /// **'Allowed domains'**
  String get settingsWebSearchAllowedDomains;

  /// No description provided for @settingsWebSearchDomainsHint.
  ///
  /// In en, this message translates to:
  /// **'example.com, docs.example.com'**
  String get settingsWebSearchDomainsHint;

  /// No description provided for @settingsWebSearchCountry.
  ///
  /// In en, this message translates to:
  /// **'Country'**
  String get settingsWebSearchCountry;

  /// No description provided for @settingsWebSearchRegion.
  ///
  /// In en, this message translates to:
  /// **'Region'**
  String get settingsWebSearchRegion;

  /// No description provided for @settingsWebSearchCity.
  ///
  /// In en, this message translates to:
  /// **'City'**
  String get settingsWebSearchCity;

  /// No description provided for @settingsWebSearchTimezone.
  ///
  /// In en, this message translates to:
  /// **'Timezone'**
  String get settingsWebSearchTimezone;

  /// No description provided for @settingsWebSearchAvailable.
  ///
  /// In en, this message translates to:
  /// **'Available'**
  String get settingsWebSearchAvailable;

  /// No description provided for @settingsWebSearchDisabled.
  ///
  /// In en, this message translates to:
  /// **'Disabled'**
  String get settingsWebSearchDisabled;

  /// No description provided for @settingsWebSearchMissingCredential.
  ///
  /// In en, this message translates to:
  /// **'Missing credential'**
  String get settingsWebSearchMissingCredential;

  /// No description provided for @settingsWebSearchUnsupportedModel.
  ///
  /// In en, this message translates to:
  /// **'Unsupported model'**
  String get settingsWebSearchUnsupportedModel;

  /// No description provided for @settingsWebSearchMissingCredentialReason.
  ///
  /// In en, this message translates to:
  /// **'No credentialed provider originating from the OpenAI preset is available. Remote web search is fully disabled.'**
  String get settingsWebSearchMissingCredentialReason;

  /// No description provided for @settingsWebSearchUnsupportedModelReason.
  ///
  /// In en, this message translates to:
  /// **'The current route cannot expose either the function tool or hosted web search.'**
  String get settingsWebSearchUnsupportedModelReason;

  /// No description provided for @settingsNotAvailable.
  ///
  /// In en, this message translates to:
  /// **'Not available'**
  String get settingsNotAvailable;

  /// No description provided for @settingsSaveWebSearch.
  ///
  /// In en, this message translates to:
  /// **'Save web search'**
  String get settingsSaveWebSearch;

  /// No description provided for @settingsStudioUpdateTitle.
  ///
  /// In en, this message translates to:
  /// **'Pure Studio update'**
  String get settingsStudioUpdateTitle;

  /// No description provided for @settingsStudioUpdateDisabled.
  ///
  /// In en, this message translates to:
  /// **'Version {version}. Automatic checks run only in Windows release builds.'**
  String settingsStudioUpdateDisabled(String version);

  /// No description provided for @settingsStudioUpdateCurrent.
  ///
  /// In en, this message translates to:
  /// **'Current version: {version}'**
  String settingsStudioUpdateCurrent(String version);

  /// No description provided for @settingsStudioUpdateChecking.
  ///
  /// In en, this message translates to:
  /// **'Version {version}. Checking for updates...'**
  String settingsStudioUpdateChecking(String version);

  /// No description provided for @settingsStudioUpdateLatest.
  ///
  /// In en, this message translates to:
  /// **'Version {version} is up to date.'**
  String settingsStudioUpdateLatest(String version);

  /// No description provided for @settingsStudioUpdateAvailable.
  ///
  /// In en, this message translates to:
  /// **'Version {current} is installed. Version {latest} is available.'**
  String settingsStudioUpdateAvailable(String current, String latest);

  /// No description provided for @settingsStudioUpdateDownloading.
  ///
  /// In en, this message translates to:
  /// **'Downloading version {version}: {progress}%'**
  String settingsStudioUpdateDownloading(String version, int progress);

  /// No description provided for @settingsStudioUpdateVerifying.
  ///
  /// In en, this message translates to:
  /// **'Verifying version {version}...'**
  String settingsStudioUpdateVerifying(String version);

  /// No description provided for @settingsStudioUpdateInstallerLaunched.
  ///
  /// In en, this message translates to:
  /// **'Installer for version {version} started.'**
  String settingsStudioUpdateInstallerLaunched(String version);

  /// No description provided for @settingsStudioUpdateFailed.
  ///
  /// In en, this message translates to:
  /// **'Update failed: {error}'**
  String settingsStudioUpdateFailed(String error);

  /// No description provided for @settingsStudioUpdateBusy.
  ///
  /// In en, this message translates to:
  /// **'Finish the active turn or task before installing.'**
  String get settingsStudioUpdateBusy;

  /// No description provided for @settingsStudioUpdateCheck.
  ///
  /// In en, this message translates to:
  /// **'Check for updates'**
  String get settingsStudioUpdateCheck;

  /// No description provided for @settingsStudioUpdateInstall.
  ///
  /// In en, this message translates to:
  /// **'Download and install'**
  String get settingsStudioUpdateInstall;

  /// No description provided for @settingsStudioUpdateReleaseNotes.
  ///
  /// In en, this message translates to:
  /// **'Release notes'**
  String get settingsStudioUpdateReleaseNotes;

  /// No description provided for @timelineWebSearchTitle.
  ///
  /// In en, this message translates to:
  /// **'Web search'**
  String get timelineWebSearchTitle;

  /// No description provided for @timelineWebSearchSearching.
  ///
  /// In en, this message translates to:
  /// **'Searching the web'**
  String get timelineWebSearchSearching;

  /// No description provided for @timelineWebSearchOpening.
  ///
  /// In en, this message translates to:
  /// **'Opening a web page'**
  String get timelineWebSearchOpening;

  /// No description provided for @timelineWebSearchFinding.
  ///
  /// In en, this message translates to:
  /// **'Finding text on a page'**
  String get timelineWebSearchFinding;

  /// No description provided for @timelineWebSearchResults.
  ///
  /// In en, this message translates to:
  /// **'Result links'**
  String get timelineWebSearchResults;

  /// No description provided for @timelineToolSearchTitle.
  ///
  /// In en, this message translates to:
  /// **'Tool search'**
  String get timelineToolSearchTitle;

  /// No description provided for @timelineToolSearchLoadedTools.
  ///
  /// In en, this message translates to:
  /// **'{count} loaded tools'**
  String timelineToolSearchLoadedTools(int count);

  /// No description provided for @timelineLspQueryTitle.
  ///
  /// In en, this message translates to:
  /// **'LSP query'**
  String get timelineLspQueryTitle;

  /// No description provided for @timelineLspQueryTitleWithDetail.
  ///
  /// In en, this message translates to:
  /// **'LSP query · {detail}'**
  String timelineLspQueryTitleWithDetail(String detail);

  /// No description provided for @timelineLspCapabilitiesTitle.
  ///
  /// In en, this message translates to:
  /// **'LSP capabilities'**
  String get timelineLspCapabilitiesTitle;

  /// No description provided for @roleExplorer.
  ///
  /// In en, this message translates to:
  /// **'Explorer'**
  String get roleExplorer;

  /// No description provided for @rolePlanner.
  ///
  /// In en, this message translates to:
  /// **'Planner'**
  String get rolePlanner;

  /// No description provided for @roleExecutor.
  ///
  /// In en, this message translates to:
  /// **'Executor'**
  String get roleExecutor;

  /// No description provided for @roleReviewer.
  ///
  /// In en, this message translates to:
  /// **'Reviewer'**
  String get roleReviewer;

  /// No description provided for @roleEmpty.
  ///
  /// In en, this message translates to:
  /// **'Agent'**
  String get roleEmpty;
}

class _AppLocalizationsDelegate
    extends LocalizationsDelegate<AppLocalizations> {
  const _AppLocalizationsDelegate();

  @override
  Future<AppLocalizations> load(Locale locale) {
    return SynchronousFuture<AppLocalizations>(lookupAppLocalizations(locale));
  }

  @override
  bool isSupported(Locale locale) =>
      <String>['en', 'zh'].contains(locale.languageCode);

  @override
  bool shouldReload(_AppLocalizationsDelegate old) => false;
}

AppLocalizations lookupAppLocalizations(Locale locale) {
  // Lookup logic when language+script codes are specified.
  switch (locale.languageCode) {
    case 'zh':
      {
        switch (locale.scriptCode) {
          case 'Hans':
            return AppLocalizationsZhHans();
        }
        break;
      }
  }

  // Lookup logic when only language code is specified.
  switch (locale.languageCode) {
    case 'en':
      return AppLocalizationsEn();
    case 'zh':
      return AppLocalizationsZh();
  }

  throw FlutterError(
    'AppLocalizations.delegate failed to load unsupported locale "$locale". This is likely '
    'an issue with the localizations generation tool. Please file an issue '
    'on GitHub with a reproducible sample app and the gen-l10n configuration '
    'that was used.',
  );
}
