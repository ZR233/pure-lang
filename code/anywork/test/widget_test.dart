import 'dart:async';
import 'dart:convert';
import 'dart:ui';

import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:anywork/src/app/anywork_app.dart';
import 'package:anywork/src/app/theme/studio_tokens.dart';
import 'package:anywork/src/app/theme/material3_theme.dart';
import 'package:anywork/src/data/frb/studio_api.dart';
import 'package:anywork/src/data/repositories/studio_repository.dart';
import 'package:anywork/src/domain/models/studio_models.dart';
import 'package:anywork/src/features/settings/settings_page.dart';
import 'package:anywork/src/features/settings/settings_ssh_tab.dart';
import 'package:anywork/src/features/interaction/composer_dock.dart';

import 'dart:io' show File, Platform;

import 'package:anywork/src/platform/vscode_detection.dart';
import 'package:anywork/src/platform/vscode_launcher.dart';
import 'package:anywork/src/features/shell/studio_shell.dart';
import 'package:anywork/src/features/status/status_bar_item.dart';
import 'package:anywork/src/features/status/status_detail_popover.dart';
import 'package:anywork/src/features/status/context_usage_readout.dart';
import 'package:anywork/src/features/status/thread_status_bar.dart';
import 'package:anywork/src/features/update/studio_update_controller.dart';
import 'package:anywork/src/l10n/app_localizations.dart';
import 'package:anywork/src/platform/clipboard_image_reader.dart';
import 'package:anywork/src/shared/studio_driver_keys.dart';
import 'package:anywork/src/shared/studio_driver_state.dart';

part 'widget_test/controller_stream_tests.dart';
part 'widget_test/reducer_recovery_tests.dart';
part 'widget_test/snapshot_settings_tests.dart';
part 'widget_test/thread_stream_tests.dart';
part 'widget_test/agent_workspace_tests.dart';
part 'widget_test/status_accessibility_tests.dart';
part 'widget_test/shell_settings_tests.dart';
part 'widget_test/project_sidebar_tests.dart';
part 'widget_test/interaction_tests.dart';
part 'widget_test/skills_tests.dart';
part 'widget_test/fixture_helpers.dart';
part 'widget_test/bridge_event_helpers.dart';
part 'widget_test/state_fixtures.dart';
part 'widget_test/fake_studio_api.dart';
part 'widget_test/settings_helpers.dart';
part 'widget_test/studio_update_tests.dart';
part 'widget_test/app_lifecycle_tests.dart';
part 'widget_test/vscode_launcher_tests.dart';

void main() {
  registerControllerStreamTests();
  registerReducerRecoveryTests();
  registerSnapshotSettingsTests();
  registerThreadStreamTests();
  registerAgentWorkspaceTests();
  registerStatusAccessibilityTests();
  registerShellSettingsTests();
  registerProjectSidebarTests();
  registerInteractionTests();
  registerSkillsTests();
  registerStudioUpdateTests();
  registerAppLifecycleTests();
  registerVsCodeLauncherTests();
}
