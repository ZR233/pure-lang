import 'dart:async';
import 'dart:convert';
import 'dart:io';
import 'dart:ui';

import 'package:flutter/material.dart';
import 'package:flutter/rendering.dart';
import 'package:flutter/scheduler.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:pure_studio_flutter/src/app/theme/studio_tokens.dart';
import 'package:pure_studio_flutter/src/data/frb/studio_api.dart';
import 'package:pure_studio_flutter/src/data/repositories/studio_repository.dart';
import 'package:pure_studio_flutter/src/domain/models/studio_models.dart';
import 'package:pure_studio_flutter/src/features/settings/settings_page.dart';
import 'package:pure_studio_flutter/src/features/shell/studio_shell.dart';
import 'package:pure_studio_flutter/src/features/status/status_bar_item.dart';
import 'package:pure_studio_flutter/src/features/timeline/markdown_repair.dart';
import 'package:pure_studio_flutter/src/features/timeline/timeline_view.dart';
import 'package:pure_studio_flutter/src/l10n/app_localizations.dart';
import 'package:pure_studio_flutter/src/rust/api/studio.dart' as frb;
import 'package:pure_studio_flutter/src/shared/studio_chrome.dart';

import 'support/responsive_visual_fixture.dart';

part 'widget_test/controller_stream_tests.dart';
part 'widget_test/reducer_recovery_tests.dart';
part 'widget_test/timeline_model_tests.dart';
part 'widget_test/snapshot_json_tests.dart';
part 'widget_test/session_stream_tests.dart';
part 'widget_test/demo_project_tests.dart';
part 'widget_test/markdown_render_tests.dart';
part 'widget_test/timeline_tool_tests.dart';
part 'widget_test/timeline_scroll_tests.dart';
part 'widget_test/visual_foundation_tests.dart';
part 'widget_test/responsive_layout_tests.dart';
part 'widget_test/shell_settings_tests.dart';
part 'widget_test/interaction_tests.dart';
part 'widget_test/skills_tests.dart';
part 'widget_test/fixture_helpers.dart';
part 'widget_test/bridge_event_helpers.dart';
part 'widget_test/menu_scroll_helpers.dart';
part 'widget_test/state_fixtures.dart';
part 'widget_test/fake_studio_api.dart';
part 'widget_test/settings_helpers.dart';

void main() {
  registerControllerStreamTests();
  registerReducerRecoveryTests();
  registerTimelineModelTests();
  registerSnapshotJsonTests();
  registerSessionStreamTests();
  registerDemoProjectTests();
  registerMarkdownRenderTests();
  registerTimelineToolTests();
  registerTimelineScrollTests();
  registerVisualFoundationTests();
  registerResponsiveLayoutTests();
  registerShellSettingsTests();
  registerInteractionTests();
  registerSkillsTests();
}
