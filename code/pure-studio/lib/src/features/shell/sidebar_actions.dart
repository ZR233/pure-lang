part of 'studio_shell.dart';

class _SidebarActions extends ConsumerWidget {
  const _SidebarActions({required this.state, required this.compact});

  final SidebarView state;
  final bool compact;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final horizontalPadding = compact ? 8.0 : 14.0;
    final hasUpdate = ref.watch(
      studioUpdateControllerProvider.select((state) => state.hasUpdate),
    );
    return Padding(
      padding: EdgeInsets.fromLTRB(
        horizontalPadding,
        11,
        horizontalPadding,
        12,
      ),
      child: compact
          ? Column(
              mainAxisSize: MainAxisSize.min,
              children: [
                _SidebarActionButton(
                  key: StudioDriverKeys.openProject,
                  tooltip: context.l10n.sidebarOpenProject,
                  icon: Icons.create_new_folder,
                  onPressed: () => _openProject(context, ref),
                ),
                const SizedBox(height: 4),
                _SidebarActionButton(
                  key: StudioDriverKeys.settingsOpen,
                  tooltip: context.l10n.sidebarSettings,
                  icon: Icons.settings,
                  showIndicator: hasUpdate,
                  onPressed: () => context.go('/settings'),
                ),
              ],
            )
          : Column(
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: [
                _SidebarActionButton(
                  showLabel: true,
                  key: StudioDriverKeys.openProject,
                  icon: Icons.create_new_folder,
                  tooltip: context.l10n.sidebarOpenProject,
                  onPressed: () => _openProject(context, ref),
                ),
                const SizedBox(height: 4),
                _SidebarActionButton(
                  showLabel: true,
                  key: StudioDriverKeys.settingsOpen,
                  tooltip: context.l10n.sidebarSettings,
                  icon: Icons.settings,
                  showIndicator: hasUpdate,
                  onPressed: () => context.go('/settings'),
                ),
              ],
            ),
    );
  }

  Future<void> _openProject(BuildContext context, WidgetRef ref) async {
    final path = await ref.read(projectDirectoryPickerProvider)(context);
    if (path == null || path.isEmpty) {
      return;
    }
    await ref.read(studioControllerProvider.notifier).openProject(path);
  }
}

class _DriverProjectPathDialog extends StatefulWidget {
  const _DriverProjectPathDialog();

  @override
  State<_DriverProjectPathDialog> createState() =>
      _DriverProjectPathDialogState();
}

class _DriverProjectPathDialogState extends State<_DriverProjectPathDialog> {
  final _controller = TextEditingController();

  String get _path => _controller.text.trim();

  @override
  void dispose() {
    _controller.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    return AlertDialog(
      key: StudioDriverKeys.projectPathDialog,
      title: Text(context.l10n.sidebarOpenProject),
      content: TextField(
        key: StudioDriverKeys.projectPathInput,
        controller: _controller,
        autofocus: true,
        decoration: InputDecoration(
          labelText: context.l10n.agentDetailPathLabel,
        ),
        onChanged: (_) => setState(() {}),
        onSubmitted: (_) => _submit(),
      ),
      actions: [
        TextButton(
          onPressed: () => Navigator.of(context).pop(),
          child: Text(context.l10n.settingsCancel),
        ),
        FilledButton(
          key: StudioDriverKeys.projectPathSubmit,
          onPressed: _path.isEmpty ? null : _submit,
          child: Text(context.l10n.sidebarOpen),
        ),
      ],
    );
  }

  void _submit() {
    final path = _path;
    if (path.isNotEmpty) {
      Navigator.of(context).pop(path);
    }
  }
}

class _SidebarActionButton extends StatelessWidget {
  const _SidebarActionButton({
    required this.icon,
    required this.tooltip,
    required this.onPressed,
    this.showIndicator = false,
    this.showLabel = false,
    super.key,
  });

  final IconData icon;
  final String tooltip;
  final VoidCallback? onPressed;
  final bool showIndicator;
  final bool showLabel;

  @override
  Widget build(BuildContext context) {
    final colors = Theme.of(context).colorScheme;
    final iconWidget = Stack(
      clipBehavior: Clip.none,
      children: [
        Icon(icon),
        if (showIndicator)
          Positioned(
            key: const ValueKey('studio-update-indicator'),
            right: -3,
            top: -3,
            child: DecoratedBox(
              decoration: BoxDecoration(
                color: context.statusColors.activeIndicator,
                shape: BoxShape.circle,
                border: Border.all(color: context.colors.surface, width: 1.5),
              ),
              child: const SizedBox.square(dimension: 8),
            ),
          ),
      ],
    );
    if (showLabel) {
      return TextButton.icon(
        onPressed: onPressed,
        icon: iconWidget,
        label: Text(tooltip),
        style: TextButton.styleFrom(
          alignment: Alignment.centerLeft,
          foregroundColor: colors.onSurfaceVariant,
          padding: const EdgeInsets.symmetric(horizontal: 10, vertical: 14),
        ),
      );
    }
    return IconButton(
      tooltip: tooltip,
      icon: iconWidget,
      style: IconButton.styleFrom(
        fixedSize: const Size.square(40),
        iconSize: 18,
        tapTargetSize: MaterialTapTargetSize.shrinkWrap,
        foregroundColor: colors.onSurfaceVariant,
        disabledForegroundColor: colors.onSurfaceVariant.withValues(
          alpha: 0.38,
        ),
        hoverColor: context.colors.surface.withValues(alpha: 0.76),
        focusColor: context.colors.surface.withValues(alpha: 0.76),
        shape: RoundedRectangleBorder(
          borderRadius: BorderRadius.circular(StudioRadii.sm),
        ),
      ),
      onPressed: onPressed,
    );
  }
}
