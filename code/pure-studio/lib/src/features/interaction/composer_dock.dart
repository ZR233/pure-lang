import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:file_picker/file_picker.dart';
import 'package:desktop_drop/desktop_drop.dart';

import '../../app/theme/studio_tokens.dart';
import '../../data/repositories/studio_repository.dart';
import '../../domain/models/studio_models.dart';
import '../../l10n/studio_l10n.dart';
import '../../shared/studio_chrome.dart';
import '../../shared/studio_driver_keys.dart';
import '../../shared/studio_driver_state.dart';
import '../../shared/upward_popup_menu.dart';
import '../status/session_selectors.dart';
import 'interaction_payload.dart';
import 'tool_approval_dock.dart';
import 'user_input_dock.dart';

class ComposerDock extends ConsumerWidget {
  const ComposerDock({required this.workspace, super.key});

  final AgentWorkspaceView workspace;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final persistenceAllowsNewWork = ref.watch(
      studioControllerProvider.select(
        (state) => state.value?.persistenceState.acceptsNewWork ?? false,
      ),
    );
    // Driver 快照需要完整 timelineRows；controls 视图为省内存清空了行。
    final full = switch (ref.watch(selectedAgentWorkspaceProvider)) {
      AsyncData(:final value) => value,
      _ => null,
    };
    if (full != null) {
      StudioDriverState.publishWorkspace(full);
    }
    final interaction = workspace.activeInteraction;
    return SafeArea(
      top: false,
      child: Padding(
        padding: const EdgeInsets.fromLTRB(12, 7, 12, 12),
        child: Align(
          alignment: Alignment.center,
          child: ConstrainedBox(
            constraints: const BoxConstraints(
              maxWidth: StudioLayout.conversationWidth,
            ),
            child: interaction == null
                ? workspace.composerMode == AgentComposerMode.runtimeDriven
                      ? _RuntimeDrivenAgentDock(workspace: workspace)
                      : _PromptComposer(
                          workspace: workspace,
                          enabled: persistenceAllowsNewWork,
                        )
                : _InteractionDock(
                    workspace: workspace,
                    interaction: interaction,
                    // 活动交互属于当前 Turn 的收束；持久化降级只暂停新工作。
                    enabled: true,
                  ),
          ),
        ),
      ),
    );
  }
}

class StartPageComposerDock extends ConsumerWidget {
  const StartPageComposerDock({required this.view, super.key});

  final StartPageView view;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final controller = ref.read(studioControllerProvider.notifier);
    final model = modelFor(view.providers, view.roles, view.mode);
    return SafeArea(
      top: false,
      child: Padding(
        padding: const EdgeInsets.fromLTRB(12, 7, 12, 12),
        child: Align(
          alignment: Alignment.center,
          child: ConstrainedBox(
            constraints: const BoxConstraints(
              maxWidth: StudioLayout.conversationWidth,
            ),
            child: _PromptComposerPanel(
              composer: view.composer,
              permissionMode: view.permissionMode,
              enabled: view.canSubmit,
              isBusy: false,
              selectorBar: Wrap(
                key: StudioDriverKeys.startPageSelectors,
                spacing: 6,
                runSpacing: 4,
                children: [
                  SessionModeSelector(
                    mode: view.mode,
                    onSelected: controller.setNewThreadMode,
                  ),
                  ModelRoleSelector(
                    providers: view.providers,
                    roles: view.roles,
                    mode: view.mode,
                  ),
                  ReasoningEffortSelector(
                    providers: view.providers,
                    roles: view.roles,
                    mode: view.mode,
                  ),
                ],
              ),
              onChanged: controller.updateNewThreadComposer,
              onSubmit: () => unawaited(controller.submitNewThreadComposer()),
              inputCapabilities: model?.inputCapabilities ?? const [],
              onAddLocal: (paths) => controller.addLocalAttachments(paths),
              onAddUrl: (url) => controller.addRemoteAttachment(url),
              onRemoveAttachment: (id) => controller.removeAttachmentDraft(id),
            ),
          ),
        ),
      ),
    );
  }
}

class _RuntimeDrivenAgentDock extends StatelessWidget {
  const _RuntimeDrivenAgentDock({required this.workspace});

  final AgentWorkspaceView workspace;

  @override
  Widget build(BuildContext context) {
    return StudioPanel(
      backgroundColor: context.studioPaper2,
      borderColor: context.studioLine,
      radius: StudioRadii.lg,
      padding: const EdgeInsets.symmetric(horizontal: 14, vertical: 9),
      child: Row(
        children: [
          Icon(
            Icons.lock_clock_outlined,
            size: 18,
            color: context.studioInkSoft,
          ),
          const SizedBox(width: 9),
          Expanded(
            child: Text(
              context.l10n.composerAgentRuntimeDriven,
              style: Theme.of(context).textTheme.bodyMedium
                  ?.copyWith(color: context.studioInkSoft),
            ),
          ),
          if (workspace.isBusy) _StopButton(threadId: workspace.threadId),
        ],
      ),
    );
  }
}

class _PromptComposer extends ConsumerWidget {
  const _PromptComposer({required this.workspace, required this.enabled});

  final AgentWorkspaceView workspace;
  final bool enabled;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final controller = ref.read(studioControllerProvider.notifier);
    final model = modelFor(
      workspace.providers,
      workspace.roles,
      workspace.rootThread.mode,
    );
    return _PromptComposerPanel(
      composer: workspace.composer,
      permissionMode: workspace.permissionMode,
      enabled: enabled,
      isBusy: workspace.isBusy,
      onChanged: (value) =>
          controller.updateComposer(workspace.threadId, value),
      onSubmit: () => unawaited(controller.submitComposer(workspace.threadId)),
      onStop: () => unawaited(controller.stop(workspace.threadId)),
      inputCapabilities: model?.inputCapabilities ?? const [],
      onAddLocal: (paths) =>
          controller.addLocalAttachments(paths, threadId: workspace.threadId),
      onAddUrl: (url) =>
          controller.addRemoteAttachment(url, threadId: workspace.threadId),
      onRemoveAttachment: (id) =>
          controller.removeAttachmentDraft(id, threadId: workspace.threadId),
    );
  }
}

class _PromptComposerPanel extends StatefulWidget {
  const _PromptComposerPanel({
    required this.composer,
    required this.permissionMode,
    required this.enabled,
    required this.isBusy,
    required this.onChanged,
    required this.onSubmit,
    required this.inputCapabilities,
    required this.onAddLocal,
    required this.onAddUrl,
    required this.onRemoveAttachment,
    this.onStop,
    this.selectorBar,
  });

  final ComposerThreadState composer;
  final PermissionMode permissionMode;
  final bool enabled;
  final bool isBusy;
  final ValueChanged<String> onChanged;
  final VoidCallback onSubmit;
  final List<ModelInputCapabilityView> inputCapabilities;
  final Future<void> Function(List<String> paths) onAddLocal;
  final Future<void> Function(String url) onAddUrl;
  final Future<void> Function(String draftId) onRemoveAttachment;
  final VoidCallback? onStop;
  final Widget? selectorBar;

  @override
  State<_PromptComposerPanel> createState() => _PromptComposerPanelState();
}

class _PromptComposerPanelState extends State<_PromptComposerPanel> {
  late final TextEditingController _controller;
  bool _dragging = false;

  @override
  void initState() {
    super.initState();
    _controller = TextEditingController(text: widget.composer.draft);
  }

  @override
  void didUpdateWidget(covariant _PromptComposerPanel oldWidget) {
    super.didUpdateWidget(oldWidget);
    final nextText = widget.composer.draft;
    if (nextText != _controller.text) {
      _controller.value = TextEditingValue(
        text: nextText,
        selection: TextSelection.collapsed(offset: nextText.length),
      );
    }
  }

  @override
  void dispose() {
    _controller.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final colors = Theme.of(context).colorScheme;
    final composer = widget.composer;
    final canSubmit =
        widget.enabled &&
        (composer.draft.trim().isNotEmpty || composer.attachments.isNotEmpty) &&
        !widget.isBusy &&
        !composer.isSubmissionPending;
    final localCapabilities = widget.inputCapabilities
        .where(
          (capability) =>
              capability.modality != ModelModalityView.text &&
              capability.supportsSource(ModelInputSourceView.local),
        )
        .toList();
    final remoteCapabilities = widget.inputCapabilities
        .where(
          (capability) =>
              capability.modality != ModelModalityView.text &&
              capability.supportsSource(ModelInputSourceView.remoteUrl),
        )
        .toList();
    final attachmentEnabled =
        widget.enabled && !composer.isSubmissionPending && !widget.isBusy;
    final panel = StudioPanel(
      backgroundColor: colors.surfaceContainerLowest,
      borderColor: _dragging
          ? colors.primary
          : colors.outlineVariant.withValues(alpha: 0.86),
      radius: StudioRadii.lg,
      shadow: true,
      padding: const EdgeInsets.fromLTRB(12, 8, 10, 10),
      child: Column(
        mainAxisSize: MainAxisSize.min,
        children: [
          if (composer.attachments.isNotEmpty)
            _AttachmentDraftRail(
              attachments: composer.attachments,
              enabled: attachmentEnabled,
              onRemove: (id) => unawaited(widget.onRemoveAttachment(id)),
            ),
          TextField(
            key: StudioDriverKeys.composerInput,
            controller: _controller,
            enabled: widget.enabled && !composer.isSubmissionPending,
            minLines: 1,
            maxLines: 6,
            decoration: InputDecoration(
              hintText: context.l10n.composerHint,
              hintStyle: TextStyle(color: colors.onSurfaceVariant),
              isDense: true,
              filled: false,
              border: InputBorder.none,
              enabledBorder: InputBorder.none,
              focusedBorder: InputBorder.none,
              prefixIcon: Icon(
                Icons.edit_outlined,
                color: colors.onSurfaceVariant,
              ),
              contentPadding: const EdgeInsets.symmetric(vertical: 8),
            ),
            onChanged: widget.onChanged,
            onSubmitted: (_) {
              if (canSubmit) {
                widget.onSubmit();
              }
            },
          ),
          if (composer.error case final error?)
            Align(
              alignment: Alignment.centerLeft,
              child: Padding(
                padding: const EdgeInsets.fromLTRB(12, 2, 8, 6),
                child: Text(
                  error,
                  key: StudioDriverKeys.composerError,
                  style: Theme.of(context).textTheme.bodySmall
                      ?.copyWith(color: colors.error),
                ),
              ),
            ),
          if (widget.selectorBar case final selectorBar?)
            Padding(
              padding: const EdgeInsets.fromLTRB(2, 4, 0, 8),
              child: selectorBar,
            ),
          Row(
            children: [
              _AttachmentMenu(
                enabled: attachmentEnabled,
                localCapabilities: localCapabilities,
                remoteCapabilities: remoteCapabilities,
                onPickLocal: _pickLocalAttachments,
                onAddUrl: _showUrlDialog,
              ),
              const SizedBox(width: 6),
              _PermissionSelector(mode: widget.permissionMode),
              const Spacer(),
              if (widget.isBusy)
                IconButton.filledTonal(
                  key: StudioDriverKeys.composerStop,
                  tooltip: context.l10n.composerStop,
                  icon: const Icon(Icons.stop),
                  onPressed: widget.onStop,
                )
              else
                IconButton.filled(
                  key: StudioDriverKeys.composerSubmit,
                  tooltip: context.l10n.composerSend,
                  style: IconButton.styleFrom(
                    backgroundColor: StudioColors.clay,
                    foregroundColor: Colors.white,
                  ),
                  icon: composer.isSubmissionPending
                      ? const SizedBox.square(
                          key: StudioDriverKeys.composerPending,
                          dimension: 18,
                          child: CircularProgressIndicator(strokeWidth: 2),
                        )
                      : const Icon(Icons.arrow_upward),
                  onPressed: canSubmit ? widget.onSubmit : null,
                ),
            ],
          ),
        ],
      ),
    );
    return DropTarget(
      onDragEntered: attachmentEnabled && localCapabilities.isNotEmpty
          ? (_) => setState(() => _dragging = true)
          : null,
      onDragExited: (_) {
        if (_dragging) setState(() => _dragging = false);
      },
      onDragDone: attachmentEnabled && localCapabilities.isNotEmpty
          ? (event) {
              setState(() => _dragging = false);
              final paths = event.files
                  .map((file) => file.path)
                  .where((path) => path.isNotEmpty)
                  .toList();
              if (paths.isNotEmpty) unawaited(widget.onAddLocal(paths));
            }
          : null,
      child: panel,
    );
  }

  Future<void> _pickLocalAttachments(
    List<ModelInputCapabilityView> capabilities,
  ) async {
    const driverFixture = String.fromEnvironment(
      'PURE_STUDIO_DRIVER_ATTACHMENT_PATH',
    );
    if (const bool.fromEnvironment('PURE_STUDIO_DRIVER') &&
        driverFixture.isNotEmpty) {
      await widget.onAddLocal([driverFixture]);
      return;
    }
    final modalities = capabilities
        .map((capability) => capability.modality)
        .toSet();
    final acceptsFiles = modalities.contains(ModelModalityView.file);
    final result = await FilePicker.pickFiles(
      type: acceptsFiles ? FileType.any : FileType.custom,
      allowedExtensions: acceptsFiles
          ? null
          : [
              if (modalities.contains(ModelModalityView.image)) ...[
                'png',
                'jpg',
                'jpeg',
                'gif',
                'webp',
              ],
              if (modalities.contains(ModelModalityView.video)) ...[
                'mp4',
                'mov',
                'webm',
                'mkv',
              ],
            ],
    );
    final paths = result.map((file) => file.path).whereType<String>().toList();
    if (paths.isNotEmpty) await widget.onAddLocal(paths);
  }

  Future<void> _showUrlDialog() async {
    var draft = '';
    final url = await showDialog<String>(
      context: context,
      builder: (context) => AlertDialog(
        key: StudioDriverKeys.attachmentUrlDialog,
        title: const Text('添加 URL'),
        content: TextField(
          key: StudioDriverKeys.attachmentUrlInput,
          autofocus: true,
          onChanged: (value) => draft = value,
          decoration: const InputDecoration(hintText: 'https://…'),
        ),
        actions: [
          TextButton(
            onPressed: () => Navigator.pop(context),
            child: const Text('取消'),
          ),
          FilledButton(
            key: StudioDriverKeys.attachmentUrlSubmit,
            onPressed: () => Navigator.pop(context, draft.trim()),
            child: const Text('添加'),
          ),
        ],
      ),
    );
    if (url != null && url.isNotEmpty) await widget.onAddUrl(url);
  }
}

class _AttachmentMenu extends StatelessWidget {
  const _AttachmentMenu({
    required this.enabled,
    required this.localCapabilities,
    required this.remoteCapabilities,
    required this.onPickLocal,
    required this.onAddUrl,
  });

  final bool enabled;
  final List<ModelInputCapabilityView> localCapabilities;
  final List<ModelInputCapabilityView> remoteCapabilities;
  final Future<void> Function(List<ModelInputCapabilityView>) onPickLocal;
  final Future<void> Function() onAddUrl;

  @override
  Widget build(BuildContext context) {
    final hasAny =
        localCapabilities.isNotEmpty || remoteCapabilities.isNotEmpty;
    return PopupMenuButton<String>(
      key: StudioDriverKeys.attachmentEntry,
      tooltip: hasAny ? '添加附件' : '当前模型不支持附件',
      enabled: enabled && hasAny,
      icon: const Icon(Icons.attach_file),
      onSelected: (value) {
        if (value == 'local') unawaited(onPickLocal(localCapabilities));
        if (value == 'url') unawaited(onAddUrl());
      },
      itemBuilder: (context) => [
        if (localCapabilities.isNotEmpty)
          const PopupMenuItem(
            key: StudioDriverKeys.attachmentLocal,
            value: 'local',
            child: ListTile(
              leading: Icon(Icons.folder_open_outlined),
              title: Text('选择本地文件'),
            ),
          ),
        if (remoteCapabilities.isNotEmpty)
          const PopupMenuItem(
            key: StudioDriverKeys.attachmentUrl,
            value: 'url',
            child: ListTile(leading: Icon(Icons.link), title: Text('添加 URL')),
          ),
      ],
    );
  }
}

class _AttachmentDraftRail extends StatelessWidget {
  const _AttachmentDraftRail({
    required this.attachments,
    required this.enabled,
    required this.onRemove,
  });

  final List<AttachmentDraftView> attachments;
  final bool enabled;
  final ValueChanged<String> onRemove;

  @override
  Widget build(BuildContext context) {
    return SizedBox(
      key: StudioDriverKeys.attachmentDraftRail,
      height: 72,
      child: ListView.separated(
        scrollDirection: Axis.horizontal,
        itemCount: attachments.length,
        separatorBuilder: (_, _) => const SizedBox(width: 8),
        itemBuilder: (context, index) {
          final attachment = attachments[index];
          return Container(
            key: StudioDriverKeys.attachmentDraft(attachment.id),
            width: 210,
            padding: const EdgeInsets.all(7),
            decoration: BoxDecoration(
              color: Theme.of(context).colorScheme.surfaceContainerLow,
              borderRadius: BorderRadius.circular(10),
            ),
            child: Row(
              children: [
                SizedBox.square(
                  dimension: 48,
                  child:
                      attachment.modality == AttachmentModalityView.image &&
                          attachment.previewBytes?.isNotEmpty == true
                      ? ClipRRect(
                          borderRadius: BorderRadius.circular(7),
                          child: Image.memory(
                            attachment.previewBytes!,
                            fit: BoxFit.cover,
                          ),
                        )
                      : Icon(_attachmentIcon(attachment.modality)),
                ),
                const SizedBox(width: 8),
                Expanded(
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    mainAxisAlignment: MainAxisAlignment.center,
                    children: [
                      Text(
                        attachment.filename,
                        maxLines: 1,
                        overflow: TextOverflow.ellipsis,
                      ),
                      Text(
                        '${_attachmentModalityLabel(attachment.modality)} · ${_formatBytes(attachment.byteSize)}',
                        key: StudioDriverKeys.attachmentModality(attachment.id),
                        style: Theme.of(context).textTheme.labelSmall,
                      ),
                    ],
                  ),
                ),
                IconButton(
                  key: StudioDriverKeys.attachmentRemove(attachment.id),
                  visualDensity: VisualDensity.compact,
                  tooltip: '移除',
                  onPressed: enabled ? () => onRemove(attachment.id) : null,
                  icon: const Icon(Icons.close, size: 18),
                ),
              ],
            ),
          );
        },
      ),
    );
  }
}

IconData _attachmentIcon(AttachmentModalityView modality) => switch (modality) {
  AttachmentModalityView.image => Icons.image_outlined,
  AttachmentModalityView.video => Icons.movie_outlined,
  AttachmentModalityView.file => Icons.insert_drive_file_outlined,
};

String _attachmentModalityLabel(AttachmentModalityView modality) =>
    switch (modality) {
      AttachmentModalityView.image => '视觉',
      AttachmentModalityView.video => '视频',
      AttachmentModalityView.file => '文件',
    };

String _formatBytes(int bytes) {
  if (bytes < 1024) return '$bytes B';
  if (bytes < 1024 * 1024) return '${(bytes / 1024).toStringAsFixed(1)} KB';
  return '${(bytes / (1024 * 1024)).toStringAsFixed(1)} MB';
}

class _PermissionSelector extends ConsumerWidget {
  const _PermissionSelector({required this.mode});

  final PermissionMode mode;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    return UpwardPopupMenu<PermissionMode>(
      tooltip: context.l10n.permissionModeTooltip,
      initialValue: mode,
      onSelected: ref.read(studioControllerProvider.notifier).setPermissionMode,
      itemBuilder: (context) => [
        for (final option in PermissionMode.values)
          PopupMenuItem(
            value: option,
            child: SizedBox(
              width: 136,
              height: 36,
              child: Row(
                children: [
                  Icon(_permissionIcon(option), size: 18),
                  const SizedBox(width: 12),
                  Expanded(
                    child: Text(
                      context.permissionModeLabel(option),
                      overflow: TextOverflow.ellipsis,
                    ),
                  ),
                ],
              ),
            ),
          ),
      ],
      child: DecoratedBox(
        decoration: BoxDecoration(
          color: context.studioPaper2,
          border: Border.all(color: context.studioLine),
          borderRadius: BorderRadius.circular(StudioRadii.sm),
        ),
        child: Padding(
          padding: const EdgeInsets.symmetric(horizontal: 9, vertical: 6),
          child: Row(
            mainAxisSize: MainAxisSize.min,
            children: [
              Icon(
                _permissionIcon(mode),
                size: 17,
                color: context.studioInkSoft,
              ),
              const SizedBox(width: 6),
              Text(
                context.permissionModeLabel(mode),
                style: Theme.of(context).textTheme.labelMedium
                    ?.copyWith(color: context.studioInkSoft),
              ),
              const SizedBox(width: 4),
              Icon(
                Icons.keyboard_arrow_down,
                size: 16,
                color: context.studioInkSoft,
              ),
            ],
          ),
        ),
      ),
    );
  }

  IconData _permissionIcon(PermissionMode value) {
    return switch (value) {
      PermissionMode.requestApproval => Icons.verified_user_outlined,
      PermissionMode.autoReview => Icons.rule_folder_outlined,
      PermissionMode.fullAccess => Icons.lock_open_outlined,
    };
  }
}

class _InteractionDock extends StatelessWidget {
  const _InteractionDock({
    required this.workspace,
    required this.interaction,
    required this.enabled,
  });

  final AgentWorkspaceView workspace;
  final PendingInteraction interaction;
  final bool enabled;

  @override
  Widget build(BuildContext context) {
    final payload = InteractionPayloadSnapshot.from(interaction);
    final trailing = workspace.isBusy
        ? _StopButton(threadId: workspace.threadId)
        : null;
    return switch (interaction.kind) {
      InteractionKind.toolApproval => ToolApprovalDock(
        threadId: workspace.threadId,
        interactionId: interaction.id,
        payload: payload,
        enabled: enabled,
        trailing: trailing,
      ),
      InteractionKind.userInput => UserInputDock(
        threadId: workspace.threadId,
        interactionId: interaction.id,
        payload: payload,
        enabled: enabled,
        trailing: trailing,
      ),
    };
  }
}

class _StopButton extends ConsumerWidget {
  const _StopButton({required this.threadId});

  final String threadId;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    return IconButton.filledTonal(
      tooltip: context.l10n.composerStop,
      icon: const Icon(Icons.stop),
      onPressed: () =>
          ref.read(studioControllerProvider.notifier).stop(threadId),
    );
  }
}
