part of 'studio_shell.dart';

class _SidebarActions extends ConsumerWidget {
  const _SidebarActions({
    required this.archived,
    required this.onToggleArchived,
  });
  final bool archived;
  final VoidCallback onToggleArchived;
  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final hasUpdate = ref.watch(
      studioUpdateControllerProvider.select((state) => state.hasUpdate),
    );
    return Padding(
      padding: const EdgeInsets.fromLTRB(10, 0, 10, 10),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          const Padding(
            padding: EdgeInsets.symmetric(horizontal: 10),
            child: Divider(height: 17),
          ),
          _SidebarActionButton(
            key: const ValueKey('sidebar-archived'),
            tooltip: archived
                ? context.l10n.sidebarProjects
                : context.l10n.sidebarArchived,
            icon: archived
                ? Icons.folder_open_outlined
                : Icons.archive_outlined,
            selected: archived,
            onPressed: onToggleArchived,
          ),
          const SizedBox(height: 4),
          _SidebarActionButton(
            key: StudioDriverKeys.settingsOpen,
            tooltip: context.l10n.sidebarSettings,
            icon: Icons.settings_outlined,
            showIndicator: hasUpdate,
            onPressed: () => context.go('/settings'),
          ),
        ],
      ),
    );
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
    this.selected = false,
    super.key,
  });

  final IconData icon;
  final String tooltip;
  final VoidCallback? onPressed;
  final bool showIndicator;
  final bool selected;

  @override
  Widget build(BuildContext context) {
    final colors = Theme.of(context).colorScheme;
    final iconWidget = Stack(
      clipBehavior: Clip.none,
      children: [
        Icon(icon, size: 18),
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
    return Semantics(
      selected: selected,
      child: TextButton(
        onPressed: onPressed,
        style: TextButton.styleFrom(
          alignment: Alignment.centerLeft,
          foregroundColor: selected
              ? colors.onSurface
              : colors.onSurfaceVariant,
          backgroundColor: selected ? colors.surfaceContainerHigh : null,
          minimumSize: const Size(0, 44),
          visualDensity: VisualDensity.standard,
          tapTargetSize: MaterialTapTargetSize.shrinkWrap,
          padding: const EdgeInsets.symmetric(horizontal: 10, vertical: 10),
          textStyle: context.text.labelLarge,
          shape: RoundedRectangleBorder(
            borderRadius: BorderRadius.circular(StudioRadii.sm),
          ),
        ),
        child: Row(
          children: [
            iconWidget,
            const SizedBox(width: 10),
            Expanded(child: Text(tooltip)),
          ],
        ),
      ),
    );
  }
}
