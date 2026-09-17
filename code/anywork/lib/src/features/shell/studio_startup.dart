part of 'studio_shell.dart';

class _StudioStartup extends StatelessWidget {
  const _StudioStartup();

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      backgroundColor: context.colors.surface,
      body: SafeArea(
        child: Center(
          child: SingleChildScrollView(
            padding: const EdgeInsets.all(32),
            child: Column(
              mainAxisSize: MainAxisSize.min,
              children: [
                Image.asset(
                  'assets/branding/anywork-app-icon.png',
                  width: 160,
                  height: 160,
                  fit: BoxFit.contain,
                  filterQuality: FilterQuality.high,
                  excludeFromSemantics: true,
                ),
                const SizedBox(height: 24),
                Text(
                  context.l10n.appTitle,
                  style: context.text.headlineSmall?.copyWith(
                    fontWeight: FontWeight.w600,
                  ),
                  textAlign: TextAlign.center,
                ),
                const SizedBox(height: 16),
                ValueListenableBuilder<StudioStartupPhase>(
                  valueListenable: FrbStudioApi.startupProgress,
                  builder: (context, phase, _) => Semantics(
                    liveRegion: true,
                    child: Text(
                      switch (phase) {
                        StudioStartupPhase.loadingBridge =>
                          context.l10n.startupPreparing,
                        StudioStartupPhase.openingStorage =>
                          context.l10n.startupStorage,
                        StudioStartupPhase.loadingConfiguration =>
                          context.l10n.startupConfiguration,
                        StudioStartupPhase.readingProjects =>
                          context.l10n.startupProjects,
                        StudioStartupPhase.preparingResources =>
                          context.l10n.startupResources,
                        StudioStartupPhase.readingState ||
                        StudioStartupPhase.ready => context.l10n.startupState,
                        StudioStartupPhase.failed =>
                          context.l10n.runtimeFatalTitle,
                      },
                      textAlign: TextAlign.center,
                      style: context.text.bodyMedium,
                    ),
                  ),
                ),
                const SizedBox(height: 24),
                SizedBox.square(
                  dimension: 24,
                  child: CircularProgressIndicator(
                    strokeWidth: 2.4,
                    color: context.statusColors.activeIndicator,
                  ),
                ),
              ],
            ),
          ),
        ),
      ),
    );
  }
}
