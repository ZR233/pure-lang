import 'package:flutter/material.dart';

/// Small status accents that have no equivalent Material color role.
@immutable
class StudioSemanticColors extends ThemeExtension<StudioSemanticColors> {
  const StudioSemanticColors({
    required this.eyeAccent,
    required this.activeIndicator,
    required this.success,
    required this.successContainer,
    required this.warning,
    required this.warningContainer,
  });

  final Color eyeAccent;
  final Color activeIndicator;
  final Color success;
  final Color successContainer;
  final Color warning;
  final Color warningContainer;

  @override
  StudioSemanticColors copyWith({
    Color? eyeAccent,
    Color? activeIndicator,
    Color? success,
    Color? successContainer,
    Color? warning,
    Color? warningContainer,
  }) => StudioSemanticColors(
    eyeAccent: eyeAccent ?? this.eyeAccent,
    activeIndicator: activeIndicator ?? this.activeIndicator,
    success: success ?? this.success,
    successContainer: successContainer ?? this.successContainer,
    warning: warning ?? this.warning,
    warningContainer: warningContainer ?? this.warningContainer,
  );

  @override
  StudioSemanticColors lerp(StudioSemanticColors? other, double t) {
    if (other == null) return this;
    return StudioSemanticColors(
      eyeAccent: Color.lerp(eyeAccent, other.eyeAccent, t)!,
      activeIndicator: Color.lerp(activeIndicator, other.activeIndicator, t)!,
      success: Color.lerp(success, other.success, t)!,
      successContainer: Color.lerp(
        successContainer,
        other.successContainer,
        t,
      )!,
      warning: Color.lerp(warning, other.warning, t)!,
      warningContainer: Color.lerp(
        warningContainer,
        other.warningContainer,
        t,
      )!,
    );
  }
}

/// Presentation intent for shared badges; business state stays with the caller.
enum StudioTone { neutral, brand, active, success, warning, error }

extension StudioToneColors on StudioTone {
  Color foreground(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    final status = Theme.of(context).extension<StudioSemanticColors>()!;
    return switch (this) {
      StudioTone.neutral => scheme.onSurfaceVariant,
      StudioTone.brand => scheme.onPrimaryContainer,
      StudioTone.active => scheme.onSurface,
      StudioTone.success => status.success,
      StudioTone.warning => status.warning,
      StudioTone.error => scheme.onErrorContainer,
    };
  }

  Color background(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    final status = Theme.of(context).extension<StudioSemanticColors>()!;
    return switch (this) {
      StudioTone.neutral || StudioTone.active => scheme.surfaceContainerLow,
      StudioTone.brand => scheme.primaryContainer,
      StudioTone.success => status.successContainer,
      StudioTone.warning => status.warningContainer,
      StudioTone.error => scheme.errorContainer,
    };
  }

  Color indicator(BuildContext context) => this == StudioTone.active
      ? Theme.of(context).extension<StudioSemanticColors>()!.activeIndicator
      : foreground(context);
}
