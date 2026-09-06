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

  /// No description provided for @shutdownPhaseStoppingAgents.
  ///
  /// In en, this message translates to:
  /// **'Stopping collaborative agents'**
  String get shutdownPhaseStoppingAgents;

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

  /// No description provided for @sidebarRenameSession.
  ///
  /// In en, this message translates to:
  /// **'Rename session'**
  String get sidebarRenameSession;

  /// No description provided for @sidebarRenameSessionTitle.
  ///
  /// In en, this message translates to:
  /// **'Rename session'**
  String get sidebarRenameSessionTitle;

  /// No description provided for @sidebarRenameSessionInput.
  ///
  /// In en, this message translates to:
  /// **'Session title'**
  String get sidebarRenameSessionInput;

  /// No description provided for @sidebarRenameSessionEmpty.
  ///
  /// In en, this message translates to:
  /// **'Enter a session title.'**
  String get sidebarRenameSessionEmpty;

  /// No description provided for @sidebarRenameSessionTooLong.
  ///
  /// In en, this message translates to:
  /// **'Session titles can be at most 80 characters.'**
  String get sidebarRenameSessionTooLong;

  /// No description provided for @sidebarRenameSessionFailed.
  ///
  /// In en, this message translates to:
  /// **'Could not rename this session.'**
  String get sidebarRenameSessionFailed;

  /// No description provided for @commonCancel.
  ///
  /// In en, this message translates to:
  /// **'Cancel'**
  String get commonCancel;

  /// No description provided for @commonSave.
  ///
  /// In en, this message translates to:
  /// **'Save'**
  String get commonSave;

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

  /// No description provided for @configRecoveryMessage.
  ///
  /// In en, this message translates to:
  /// **'An incompatible configuration was backed up and replaced with current defaults.'**
  String get configRecoveryMessage;

  /// No description provided for @configRecoveryBackupPath.
  ///
  /// In en, this message translates to:
  /// **'Backup: {path}'**
  String configRecoveryBackupPath(String path);

  /// No description provided for @configRecoveryDismissTooltip.
  ///
  /// In en, this message translates to:
  /// **'Dismiss configuration recovery notice'**
  String get configRecoveryDismissTooltip;

  /// No description provided for @persistenceDegraded.
  ///
  /// In en, this message translates to:
  /// **'Saving is temporarily unavailable. {count} in-memory update(s) are waiting; you can continue the conversation.'**
  String persistenceDegraded(int count);

  /// No description provided for @persistenceRecovering.
  ///
  /// In en, this message translates to:
  /// **'Saving has recovered and is flushing {count} pending update(s). You can continue the conversation.'**
  String persistenceRecovering(int count);

  /// No description provided for @persistenceBlocked.
  ///
  /// In en, this message translates to:
  /// **'Saving is blocked with {count} pending update(s) and needs attention. You can continue the conversation.'**
  String persistenceBlocked(int count);

  /// No description provided for @persistenceRetry.
  ///
  /// In en, this message translates to:
  /// **'Retry saving'**
  String get persistenceRetry;

  /// No description provided for @recoveryGlobalWarning.
  ///
  /// In en, this message translates to:
  /// **'{count} recovery issue(s) need attention'**
  String recoveryGlobalWarning(int count);

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

  /// No description provided for @settingsAgentsTab.
  ///
  /// In en, this message translates to:
  /// **'Agents'**
  String get settingsAgentsTab;

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

  /// No description provided for @settingsStatisticsTab.
  ///
  /// In en, this message translates to:
  /// **'Statistics'**
  String get settingsStatisticsTab;

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

  /// No description provided for @settingsSshTab.
  ///
  /// In en, this message translates to:
  /// **'SSH'**
  String get settingsSshTab;

  /// No description provided for @settingsSshTitle.
  ///
  /// In en, this message translates to:
  /// **'Remote development'**
  String get settingsSshTitle;

  /// No description provided for @settingsSshSubtitle.
  ///
  /// In en, this message translates to:
  /// **'Manage SSH workspaces. Connections and helper lifecycle are owned by the local core.'**
  String get settingsSshSubtitle;

  /// No description provided for @settingsSshAdd.
  ///
  /// In en, this message translates to:
  /// **'Add server'**
  String get settingsSshAdd;

  /// No description provided for @settingsSshEmpty.
  ///
  /// In en, this message translates to:
  /// **'No SSH servers yet.'**
  String get settingsSshEmpty;

  /// No description provided for @settingsSshManagedByCore.
  ///
  /// In en, this message translates to:
  /// **'OpenSSH and the minimal remote helper are managed locally.'**
  String get settingsSshManagedByCore;

  /// No description provided for @settingsSshTest.
  ///
  /// In en, this message translates to:
  /// **'Test connection'**
  String get settingsSshTest;

  /// No description provided for @settingsSshReconnect.
  ///
  /// In en, this message translates to:
  /// **'Reconnect'**
  String get settingsSshReconnect;

  /// No description provided for @settingsSshOpenProject.
  ///
  /// In en, this message translates to:
  /// **'Open project'**
  String get settingsSshOpenProject;

  /// No description provided for @settingsSshEdit.
  ///
  /// In en, this message translates to:
  /// **'Edit'**
  String get settingsSshEdit;

  /// No description provided for @settingsSshDelete.
  ///
  /// In en, this message translates to:
  /// **'Delete'**
  String get settingsSshDelete;

  /// No description provided for @settingsSshReady.
  ///
  /// In en, this message translates to:
  /// **'Ready'**
  String get settingsSshReady;

  /// No description provided for @settingsSshDeleteTitle.
  ///
  /// In en, this message translates to:
  /// **'Delete SSH server?'**
  String get settingsSshDeleteTitle;

  /// No description provided for @settingsSshDeleteBody.
  ///
  /// In en, this message translates to:
  /// **'Delete {name}? Projects using this server must be removed first.'**
  String settingsSshDeleteBody(String name);

  /// No description provided for @settingsSshName.
  ///
  /// In en, this message translates to:
  /// **'Name'**
  String get settingsSshName;

  /// No description provided for @settingsSshHost.
  ///
  /// In en, this message translates to:
  /// **'Host'**
  String get settingsSshHost;

  /// No description provided for @settingsSshUsername.
  ///
  /// In en, this message translates to:
  /// **'Username'**
  String get settingsSshUsername;

  /// No description provided for @settingsSshPort.
  ///
  /// In en, this message translates to:
  /// **'Port'**
  String get settingsSshPort;

  /// No description provided for @settingsSshAuth.
  ///
  /// In en, this message translates to:
  /// **'Authentication'**
  String get settingsSshAuth;

  /// No description provided for @settingsSshAuthAgentOrKey.
  ///
  /// In en, this message translates to:
  /// **'SSH agent or key'**
  String get settingsSshAuthAgentOrKey;

  /// No description provided for @settingsSshAuthPassword.
  ///
  /// In en, this message translates to:
  /// **'Password'**
  String get settingsSshAuthPassword;

  /// No description provided for @settingsSshIdentityFile.
  ///
  /// In en, this message translates to:
  /// **'Identity file (optional)'**
  String get settingsSshIdentityFile;

  /// No description provided for @settingsSshPassword.
  ///
  /// In en, this message translates to:
  /// **'Password'**
  String get settingsSshPassword;

  /// No description provided for @settingsSshPasswordLease.
  ///
  /// In en, this message translates to:
  /// **'Kept in core memory for this app session only.'**
  String get settingsSshPasswordLease;

  /// No description provided for @settingsSshSave.
  ///
  /// In en, this message translates to:
  /// **'Save'**
  String get settingsSshSave;

  /// No description provided for @settingsSshNameRequired.
  ///
  /// In en, this message translates to:
  /// **'Enter a server name'**
  String get settingsSshNameRequired;

  /// No description provided for @settingsSshHostRequired.
  ///
  /// In en, this message translates to:
  /// **'Enter a host address'**
  String get settingsSshHostRequired;

  /// No description provided for @settingsSshUsernameRequired.
  ///
  /// In en, this message translates to:
  /// **'Enter a username'**
  String get settingsSshUsernameRequired;

  /// No description provided for @settingsSshPortInvalid.
  ///
  /// In en, this message translates to:
  /// **'Port must be a number from 1 to 65535'**
  String get settingsSshPortInvalid;

  /// No description provided for @settingsSshChooseDirectory.
  ///
  /// In en, this message translates to:
  /// **'Choose remote directory'**
  String get settingsSshChooseDirectory;

  /// No description provided for @settingsSshOpenThisDirectory.
  ///
  /// In en, this message translates to:
  /// **'Open this directory'**
  String get settingsSshOpenThisDirectory;

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
  /// **'Session mode cannot change while the session is running or a workflow is active'**
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

  /// No description provided for @sessionAllAgentsCostTooltip.
  ///
  /// In en, this message translates to:
  /// **'Cost for all agents in this session'**
  String get sessionAllAgentsCostTooltip;

  /// No description provided for @statusCurrentAgentTokenSpeed.
  ///
  /// In en, this message translates to:
  /// **'Current agent token speed'**
  String get statusCurrentAgentTokenSpeed;

  /// No description provided for @settingsStatisticsTitle.
  ///
  /// In en, this message translates to:
  /// **'Statistics'**
  String get settingsStatisticsTitle;

  /// No description provided for @settingsStatisticsSubtitle.
  ///
  /// In en, this message translates to:
  /// **'Recent successful model calls, grouped by provider instance and actual model.'**
  String get settingsStatisticsSubtitle;

  /// No description provided for @settingsStatisticsSummaryTitle.
  ///
  /// In en, this message translates to:
  /// **'Model performance'**
  String get settingsStatisticsSummaryTitle;

  /// No description provided for @settingsStatisticsHistoryTitle.
  ///
  /// In en, this message translates to:
  /// **'Call history'**
  String get settingsStatisticsHistoryTitle;

  /// No description provided for @settingsStatisticsAllModels.
  ///
  /// In en, this message translates to:
  /// **'All models'**
  String get settingsStatisticsAllModels;

  /// No description provided for @settingsStatisticsEmpty.
  ///
  /// In en, this message translates to:
  /// **'No complete performance samples yet.'**
  String get settingsStatisticsEmpty;

  /// No description provided for @statisticsModel.
  ///
  /// In en, this message translates to:
  /// **'Provider / model'**
  String get statisticsModel;

  /// No description provided for @statisticsSpeed.
  ///
  /// In en, this message translates to:
  /// **'Speed'**
  String get statisticsSpeed;

  /// No description provided for @statisticsSamples.
  ///
  /// In en, this message translates to:
  /// **'Samples'**
  String get statisticsSamples;

  /// No description provided for @statisticsOutputTokens.
  ///
  /// In en, this message translates to:
  /// **'Output tokens'**
  String get statisticsOutputTokens;

  /// No description provided for @statisticsAverageTtft.
  ///
  /// In en, this message translates to:
  /// **'Average TTFT'**
  String get statisticsAverageTtft;

  /// No description provided for @statisticsAverageResponse.
  ///
  /// In en, this message translates to:
  /// **'Average response'**
  String get statisticsAverageResponse;

  /// No description provided for @statisticsCompletedAt.
  ///
  /// In en, this message translates to:
  /// **'Completed'**
  String get statisticsCompletedAt;

  /// No description provided for @statisticsDecode.
  ///
  /// In en, this message translates to:
  /// **'Decode'**
  String get statisticsDecode;

  /// No description provided for @statisticsTotalResponse.
  ///
  /// In en, this message translates to:
  /// **'Total'**
  String get statisticsTotalResponse;

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

  /// No description provided for @timelineExternalLinkOpenFailed.
  ///
  /// In en, this message translates to:
  /// **'Unable to open this link.'**
  String get timelineExternalLinkOpenFailed;

  /// No description provided for @timelineAttachment.
  ///
  /// In en, this message translates to:
  /// **'Attachment'**
  String get timelineAttachment;

  /// No description provided for @timelineImageLoadFailed.
  ///
  /// In en, this message translates to:
  /// **'Unable to load this image.'**
  String get timelineImageLoadFailed;

  /// No description provided for @timelineImageRetry.
  ///
  /// In en, this message translates to:
  /// **'Retry'**
  String get timelineImageRetry;

  /// No description provided for @timelineImageClose.
  ///
  /// In en, this message translates to:
  /// **'Close image preview'**
  String get timelineImageClose;

  /// No description provided for @timelineRemoteImageSource.
  ///
  /// In en, this message translates to:
  /// **'External image from {host}'**
  String timelineRemoteImageSource(String host);

  /// No description provided for @timelineRemoteImageOpen.
  ///
  /// In en, this message translates to:
  /// **'Click to load and preview'**
  String get timelineRemoteImageOpen;

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

  /// No description provided for @timelineSkillActivated.
  ///
  /// In en, this message translates to:
  /// **'Activated skill · {name}'**
  String timelineSkillActivated(String name);

  /// No description provided for @timelineSkillAgentActivated.
  ///
  /// In en, this message translates to:
  /// **'Agent activated skill · {name}'**
  String timelineSkillAgentActivated(String name);

  /// No description provided for @timelineParentAgent.
  ///
  /// In en, this message translates to:
  /// **'Main agent'**
  String get timelineParentAgent;

  /// No description provided for @timelineSkillUserActivated.
  ///
  /// In en, this message translates to:
  /// **'User activated skill · {name}'**
  String timelineSkillUserActivated(String name);

  /// No description provided for @timelineAgentFallback.
  ///
  /// In en, this message translates to:
  /// **'Agent'**
  String get timelineAgentFallback;

  /// No description provided for @timelineViewImageRead.
  ///
  /// In en, this message translates to:
  /// **'Image read'**
  String get timelineViewImageRead;

  /// No description provided for @timelineViewImageReading.
  ///
  /// In en, this message translates to:
  /// **'Reading image'**
  String get timelineViewImageReading;

  /// No description provided for @timelineViewImageFailed.
  ///
  /// In en, this message translates to:
  /// **'Failed to read image'**
  String get timelineViewImageFailed;

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

  /// No description provided for @timelineToolCancelled.
  ///
  /// In en, this message translates to:
  /// **'{name} cancelled'**
  String timelineToolCancelled(String name);

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
  /// **'Confirm this plan?'**
  String get interactionPlanConfirmTitle;

  /// No description provided for @interactionPlanConfirmSubtitle.
  ///
  /// In en, this message translates to:
  /// **'Confirm it, or describe what should change.'**
  String get interactionPlanConfirmSubtitle;

  /// No description provided for @interactionPlanReadyTitle.
  ///
  /// In en, this message translates to:
  /// **'Implementation plan ready'**
  String get interactionPlanReadyTitle;

  /// No description provided for @interactionPlanAwaitingConfirmation.
  ///
  /// In en, this message translates to:
  /// **'Awaiting confirmation'**
  String get interactionPlanAwaitingConfirmation;

  /// No description provided for @interactionPlanViewDetails.
  ///
  /// In en, this message translates to:
  /// **'View full plan'**
  String get interactionPlanViewDetails;

  /// No description provided for @interactionPlanDetailsTitle.
  ///
  /// In en, this message translates to:
  /// **'Implementation plan'**
  String get interactionPlanDetailsTitle;

  /// No description provided for @interactionPlanComposerPausedHint.
  ///
  /// In en, this message translates to:
  /// **'Normal messages are paused to keep plan feedback unambiguous.'**
  String get interactionPlanComposerPausedHint;

  /// No description provided for @interactionPlanConfirmFooterHint.
  ///
  /// In en, this message translates to:
  /// **'Confirming starts the document-editing checkpoint in {mode} mode.'**
  String interactionPlanConfirmFooterHint(String mode);

  /// No description provided for @interactionPlanAdjust.
  ///
  /// In en, this message translates to:
  /// **'Tell Pure how to adjust'**
  String get interactionPlanAdjust;

  /// No description provided for @interactionPlanConfirmAction.
  ///
  /// In en, this message translates to:
  /// **'Confirm and execute'**
  String get interactionPlanConfirmAction;

  /// No description provided for @interactionPlanAdjustHint.
  ///
  /// In en, this message translates to:
  /// **'Describe what should change...'**
  String get interactionPlanAdjustHint;

  /// No description provided for @interactionPlanAdjustSubmit.
  ///
  /// In en, this message translates to:
  /// **'Submit changes'**
  String get interactionPlanAdjustSubmit;

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

  /// No description provided for @settingsRoleWorktreeExecutorDescription.
  ///
  /// In en, this message translates to:
  /// **'Apply edits and run tools in an isolated Git worktree.'**
  String get settingsRoleWorktreeExecutorDescription;

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

  /// No description provided for @settingsWebSearchAvailableNotSelected.
  ///
  /// In en, this message translates to:
  /// **'Available, not selected'**
  String get settingsWebSearchAvailableNotSelected;

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

  /// No description provided for @settingsWebSearchUnsupportedProvider.
  ///
  /// In en, this message translates to:
  /// **'Unsupported provider'**
  String get settingsWebSearchUnsupportedProvider;

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

  /// No description provided for @settingsWebSearchUnsupportedProviderReason.
  ///
  /// In en, this message translates to:
  /// **'The current provider does not expose this web search backend.'**
  String get settingsWebSearchUnsupportedProviderReason;

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

  /// No description provided for @settingsDeepSeekWebSearchTitle.
  ///
  /// In en, this message translates to:
  /// **'DeepSeek native web search'**
  String get settingsDeepSeekWebSearchTitle;

  /// No description provided for @settingsDeepSeekWebSearchSubtitle.
  ///
  /// In en, this message translates to:
  /// **'Let the current eligible DeepSeek Responses model search the web. It takes priority over the OpenAI fallback.'**
  String get settingsDeepSeekWebSearchSubtitle;

  /// No description provided for @settingsDeepSeekWebSearchConfigured.
  ///
  /// In en, this message translates to:
  /// **'Configured'**
  String get settingsDeepSeekWebSearchConfigured;

  /// No description provided for @settingsDeepSeekWebSearchEffective.
  ///
  /// In en, this message translates to:
  /// **'Effective'**
  String get settingsDeepSeekWebSearchEffective;

  /// No description provided for @settingsDeepSeekWebSearchEnabled.
  ///
  /// In en, this message translates to:
  /// **'Enabled'**
  String get settingsDeepSeekWebSearchEnabled;

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

  /// No description provided for @modalityText.
  ///
  /// In en, this message translates to:
  /// **'Text'**
  String get modalityText;

  /// No description provided for @modalityImage.
  ///
  /// In en, this message translates to:
  /// **'Vision'**
  String get modalityImage;

  /// No description provided for @modalityAudio.
  ///
  /// In en, this message translates to:
  /// **'Audio'**
  String get modalityAudio;

  /// No description provided for @modalityVideo.
  ///
  /// In en, this message translates to:
  /// **'Video'**
  String get modalityVideo;

  /// No description provided for @modalityFile.
  ///
  /// In en, this message translates to:
  /// **'File'**
  String get modalityFile;

  /// No description provided for @composerAddUrlTitle.
  ///
  /// In en, this message translates to:
  /// **'Add URL'**
  String get composerAddUrlTitle;

  /// No description provided for @composerAddUrlConfirm.
  ///
  /// In en, this message translates to:
  /// **'Add'**
  String get composerAddUrlConfirm;

  /// No description provided for @composerAttachmentAddTooltip.
  ///
  /// In en, this message translates to:
  /// **'Add attachment'**
  String get composerAttachmentAddTooltip;

  /// No description provided for @composerAttachmentUnsupportedTooltip.
  ///
  /// In en, this message translates to:
  /// **'The current model does not support attachments'**
  String get composerAttachmentUnsupportedTooltip;

  /// No description provided for @composerAttachmentPickLocal.
  ///
  /// In en, this message translates to:
  /// **'Choose local files'**
  String get composerAttachmentPickLocal;

  /// No description provided for @composerAttachmentRemoveTooltip.
  ///
  /// In en, this message translates to:
  /// **'Remove'**
  String get composerAttachmentRemoveTooltip;

  /// No description provided for @settingsModelOutputCapabilities.
  ///
  /// In en, this message translates to:
  /// **'Outputs: {capabilities}'**
  String settingsModelOutputCapabilities(String capabilities);

  /// No description provided for @settingsAgentsTitle.
  ///
  /// In en, this message translates to:
  /// **'Agent Profiles'**
  String get settingsAgentsTitle;

  /// No description provided for @settingsAgentsSubtitle.
  ///
  /// In en, this message translates to:
  /// **'System profiles have a fixed purpose and workspace mode; enablement and model routing stay configurable. Directory only constrains Pure\'s built-in file-write tools — shell, Git, and MCP can bypass it.'**
  String get settingsAgentsSubtitle;

  /// No description provided for @settingsAgentsAddUserProfile.
  ///
  /// In en, this message translates to:
  /// **'Add user profile'**
  String get settingsAgentsAddUserProfile;

  /// No description provided for @settingsAgentsEditTooltip.
  ///
  /// In en, this message translates to:
  /// **'Edit'**
  String get settingsAgentsEditTooltip;

  /// No description provided for @settingsAgentsRecoveryTitle.
  ///
  /// In en, this message translates to:
  /// **'Recovery'**
  String get settingsAgentsRecoveryTitle;

  /// No description provided for @settingsWorktreeBase.
  ///
  /// In en, this message translates to:
  /// **'base {commit}'**
  String settingsWorktreeBase(String commit);

  /// No description provided for @settingsWorktreeHead.
  ///
  /// In en, this message translates to:
  /// **'head {commit}'**
  String settingsWorktreeHead(String commit);

  /// No description provided for @settingsWorktreeHeadUnavailable.
  ///
  /// In en, this message translates to:
  /// **'head unavailable'**
  String get settingsWorktreeHeadUnavailable;

  /// No description provided for @settingsWorktreeChangedFiles.
  ///
  /// In en, this message translates to:
  /// **'Changed files: {files}'**
  String settingsWorktreeChangedFiles(String files);

  /// No description provided for @settingsWorktreeCleanup.
  ///
  /// In en, this message translates to:
  /// **'Clean up worktree and branch'**
  String get settingsWorktreeCleanup;

  /// No description provided for @settingsAgentProfileAddTitle.
  ///
  /// In en, this message translates to:
  /// **'Add user agent profile'**
  String get settingsAgentProfileAddTitle;

  /// No description provided for @settingsAgentProfileEditTitle.
  ///
  /// In en, this message translates to:
  /// **'Edit user agent profile'**
  String get settingsAgentProfileEditTitle;

  /// No description provided for @settingsAgentProfileIdField.
  ///
  /// In en, this message translates to:
  /// **'Agent ID'**
  String get settingsAgentProfileIdField;

  /// No description provided for @settingsAgentProfileDisplayNameField.
  ///
  /// In en, this message translates to:
  /// **'Display name'**
  String get settingsAgentProfileDisplayNameField;

  /// No description provided for @settingsAgentProfileDescriptionField.
  ///
  /// In en, this message translates to:
  /// **'Description'**
  String get settingsAgentProfileDescriptionField;

  /// No description provided for @settingsAgentProfileWhenToUseField.
  ///
  /// In en, this message translates to:
  /// **'Best for'**
  String get settingsAgentProfileWhenToUseField;

  /// No description provided for @settingsAgentProfileInstructionsField.
  ///
  /// In en, this message translates to:
  /// **'System instructions'**
  String get settingsAgentProfileInstructionsField;

  /// No description provided for @settingsAgentProfileProviderField.
  ///
  /// In en, this message translates to:
  /// **'Provider'**
  String get settingsAgentProfileProviderField;

  /// No description provided for @settingsAgentProfileEffortDefault.
  ///
  /// In en, this message translates to:
  /// **'Use model default'**
  String get settingsAgentProfileEffortDefault;

  /// No description provided for @settingsAgentProfileWorkspaceModeField.
  ///
  /// In en, this message translates to:
  /// **'Workspace mode'**
  String get settingsAgentProfileWorkspaceModeField;

  /// No description provided for @settingsAgentWorkspaceModeUnrestricted.
  ///
  /// In en, this message translates to:
  /// **'Unrestricted'**
  String get settingsAgentWorkspaceModeUnrestricted;

  /// No description provided for @settingsAgentWorkspaceModeDirectory.
  ///
  /// In en, this message translates to:
  /// **'Directory'**
  String get settingsAgentWorkspaceModeDirectory;

  /// No description provided for @settingsAgentWorkspaceModeWorktree.
  ///
  /// In en, this message translates to:
  /// **'Worktree'**
  String get settingsAgentWorkspaceModeWorktree;

  /// No description provided for @settingsAgentProfileWorkspaceDirectoryHint.
  ///
  /// In en, this message translates to:
  /// **'Directory is a cooperative file-tool boundary, not an OS sandbox; shell, Git, and MCP can bypass it.'**
  String get settingsAgentProfileWorkspaceDirectoryHint;

  /// No description provided for @settingsAgentProfileEnabledTitle.
  ///
  /// In en, this message translates to:
  /// **'Enabled'**
  String get settingsAgentProfileEnabledTitle;

  /// No description provided for @settingsAgentProfileEnabledSubtitle.
  ///
  /// In en, this message translates to:
  /// **'Disabled profiles keep their TOML file but no longer appear in the agent tool catalog.'**
  String get settingsAgentProfileEnabledSubtitle;

  /// No description provided for @settingsAgentProfileSave.
  ///
  /// In en, this message translates to:
  /// **'Save TOML atomically'**
  String get settingsAgentProfileSave;

  /// No description provided for @settingsAgentProfileRequired.
  ///
  /// In en, this message translates to:
  /// **'Required'**
  String get settingsAgentProfileRequired;

  /// No description provided for @settingsServiceCapabilitiesTitle.
  ///
  /// In en, this message translates to:
  /// **'Service capabilities'**
  String get settingsServiceCapabilitiesTitle;

  /// No description provided for @settingsCapabilitySourceField.
  ///
  /// In en, this message translates to:
  /// **'Capability source'**
  String get settingsCapabilitySourceField;

  /// No description provided for @settingsCapabilitySourcePreset.
  ///
  /// In en, this message translates to:
  /// **'Follow preset defaults'**
  String get settingsCapabilitySourcePreset;

  /// No description provided for @settingsCapabilitySourceExplicit.
  ///
  /// In en, this message translates to:
  /// **'Explicit override'**
  String get settingsCapabilitySourceExplicit;

  /// No description provided for @settingsHostedWebSearchField.
  ///
  /// In en, this message translates to:
  /// **'Hosted Web Search'**
  String get settingsHostedWebSearchField;

  /// No description provided for @settingsHostedWebSearchDialectField.
  ///
  /// In en, this message translates to:
  /// **'Hosted Web Search dialect'**
  String get settingsHostedWebSearchDialectField;

  /// No description provided for @settingsStandaloneWebSearchField.
  ///
  /// In en, this message translates to:
  /// **'Standalone Web Search'**
  String get settingsStandaloneWebSearchField;

  /// No description provided for @settingsProgrammaticToolCallingField.
  ///
  /// In en, this message translates to:
  /// **'Programmatic Tool Calling'**
  String get settingsProgrammaticToolCallingField;

  /// No description provided for @settingsCapabilityEnabled.
  ///
  /// In en, this message translates to:
  /// **'Enabled'**
  String get settingsCapabilityEnabled;

  /// No description provided for @settingsCapabilityDisabled.
  ///
  /// In en, this message translates to:
  /// **'Disabled'**
  String get settingsCapabilityDisabled;

  /// No description provided for @settingsDefaultConnectionField.
  ///
  /// In en, this message translates to:
  /// **'Default connection'**
  String get settingsDefaultConnectionField;

  /// No description provided for @settingsCurrentConnectionField.
  ///
  /// In en, this message translates to:
  /// **'Current connection'**
  String get settingsCurrentConnectionField;

  /// No description provided for @settingsSupportedConnectionsLabel.
  ///
  /// In en, this message translates to:
  /// **'Supported connections'**
  String get settingsSupportedConnectionsLabel;

  /// No description provided for @settingsAgentRoutesTitle.
  ///
  /// In en, this message translates to:
  /// **'System agent model routing'**
  String get settingsAgentRoutesTitle;

  /// No description provided for @settingsStateChecking.
  ///
  /// In en, this message translates to:
  /// **'Checking'**
  String get settingsStateChecking;

  /// No description provided for @settingsStateAvailable.
  ///
  /// In en, this message translates to:
  /// **'Available'**
  String get settingsStateAvailable;

  /// No description provided for @settingsStateUnavailable.
  ///
  /// In en, this message translates to:
  /// **'Unavailable'**
  String get settingsStateUnavailable;

  /// No description provided for @settingsStateDisabled.
  ///
  /// In en, this message translates to:
  /// **'Disabled'**
  String get settingsStateDisabled;

  /// No description provided for @settingsMcpStateMissingCredential.
  ///
  /// In en, this message translates to:
  /// **'Missing credential'**
  String get settingsMcpStateMissingCredential;

  /// No description provided for @settingsLspActivityIdle.
  ///
  /// In en, this message translates to:
  /// **'Idle'**
  String get settingsLspActivityIdle;

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

  /// No description provided for @roleWorktreeExecutor.
  ///
  /// In en, this message translates to:
  /// **'Worktree executor'**
  String get roleWorktreeExecutor;

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

  /// No description provided for @settingsPricingEnabled.
  ///
  /// In en, this message translates to:
  /// **'Estimate token cost'**
  String get settingsPricingEnabled;

  /// No description provided for @settingsPricingHelp.
  ///
  /// In en, this message translates to:
  /// **'Controls in-app cost estimates; token and cache statistics remain available. Plans default to off.'**
  String get settingsPricingHelp;

  /// No description provided for @settingsModelAdvanced.
  ///
  /// In en, this message translates to:
  /// **'Optional model settings'**
  String get settingsModelAdvanced;

  /// No description provided for @settingsContextBudget.
  ///
  /// In en, this message translates to:
  /// **'Context budget (tokens)'**
  String get settingsContextBudget;

  /// No description provided for @settingsOutputBudget.
  ///
  /// In en, this message translates to:
  /// **'Maximum output (tokens)'**
  String get settingsOutputBudget;

  /// No description provided for @settingsPriceInput.
  ///
  /// In en, this message translates to:
  /// **'Input'**
  String get settingsPriceInput;

  /// No description provided for @settingsPriceOutput.
  ///
  /// In en, this message translates to:
  /// **'Output'**
  String get settingsPriceOutput;

  /// No description provided for @settingsPriceCacheRead.
  ///
  /// In en, this message translates to:
  /// **'Cache read'**
  String get settingsPriceCacheRead;

  /// No description provided for @settingsPriceCacheWrite.
  ///
  /// In en, this message translates to:
  /// **'Cache write'**
  String get settingsPriceCacheWrite;

  /// No description provided for @statusReportedUsageOnly.
  ///
  /// In en, this message translates to:
  /// **'reported'**
  String get statusReportedUsageOnly;
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
