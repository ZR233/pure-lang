import 'package:flutter/foundation.dart';

abstract final class StudioDriverKeys {
  static const shell = ValueKey<String>('studio-shell');
  static const sidebar = ValueKey<String>('studio-sidebar');
  static const timeline = ValueKey<String>('timeline-scrollable');
  static const newSession = ValueKey<String>('sidebar-new-session');
  static const settingsOpen = ValueKey<String>('settings-open');
  static const settingsPage = ValueKey<String>('settings-page');
  static const settingsBack = ValueKey<String>('settings-back');
  static const sessionMode = ValueKey<String>('session-mode-selector');
  static const reasoningEffort = ValueKey<String>('reasoning-effort-selector');
  static const composerInput = ValueKey<String>('composer-input');
  static const composerSubmit = ValueKey<String>('composer-submit');
  static const composerStop = ValueKey<String>('composer-stop');
  static const composerPending = ValueKey<String>('composer-pending');
  static const composerError = ValueKey<String>('composer-error');
  static const agentSwitcher = ValueKey<String>('agent-switcher');
  static const providerEditor = ValueKey<String>('provider-editor');
  static const providerEdit = ValueKey<String>('provider-edit');
  static const providerSave = ValueKey<String>('provider-save');
  static const providerCancel = ValueKey<String>('provider-cancel');
  static const planImplement = ValueKey<String>('plan-implement');
  static const planContinue = ValueKey<String>('plan-continue');
  static const toolApprove = ValueKey<String>('tool-approve');
  static const toolDeny = ValueKey<String>('tool-deny');
  static const userInputSubmit = ValueKey<String>('user-input-submit');

  static ValueKey<String> settingsTab(String id) =>
      ValueKey<String>('settings-tab-$id');

  static ValueKey<String> projectRow(String id) =>
      ValueKey<String>('project-row-$id');

  static ValueKey<String> sessionRow(String id) =>
      ValueKey<String>('session-row-$id');

  static ValueKey<String> sessionModeOption(String mode) =>
      ValueKey<String>('session-mode-$mode');

  static ValueKey<String> userInputOption(String questionId, int optionIndex) =>
      ValueKey<String>('user-input-option-$questionId-$optionIndex');

  static ValueKey<String> agentRow(String id) =>
      ValueKey<String>('agent-session-$id');

  static ValueKey<String> providerRow(String id) =>
      ValueKey<String>('provider-row-$id');
}
