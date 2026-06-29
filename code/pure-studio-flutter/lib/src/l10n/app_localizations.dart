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
  /// **'No session'**
  String get shellNoSession;

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

  /// No description provided for @compileModeAuto.
  ///
  /// In en, this message translates to:
  /// **'Auto'**
  String get compileModeAuto;

  /// No description provided for @compileModePlan.
  ///
  /// In en, this message translates to:
  /// **'Plan'**
  String get compileModePlan;

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

  /// No description provided for @statusCostDetailTitle.
  ///
  /// In en, this message translates to:
  /// **'Session cost'**
  String get statusCostDetailTitle;

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

  /// No description provided for @statusPlannerModel.
  ///
  /// In en, this message translates to:
  /// **'Planner model'**
  String get statusPlannerModel;

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

  /// No description provided for @statusAgentsCount.
  ///
  /// In en, this message translates to:
  /// **'{count, plural, =1{1 agent} other{{count} agents}}'**
  String statusAgentsCount(int count);

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
