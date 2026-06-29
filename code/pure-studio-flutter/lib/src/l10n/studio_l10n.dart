import 'package:flutter/widgets.dart';

import '../domain/models/studio_enums.dart';
import 'app_localizations.dart';

extension StudioLocalizationsX on BuildContext {
  AppLocalizations get l10n => AppLocalizations.of(this);

  String compileModeLabel(CompileMode mode) {
    return switch (mode) {
      CompileMode.auto => l10n.compileModeAuto,
      CompileMode.plan => l10n.compileModePlan,
    };
  }

  String permissionModeLabel(PermissionMode mode) {
    return switch (mode) {
      PermissionMode.requestApproval => l10n.permissionModeRequestApproval,
      PermissionMode.autoReview => l10n.permissionModeAutoReview,
      PermissionMode.fullAccess => l10n.permissionModeFullAccess,
    };
  }
}
