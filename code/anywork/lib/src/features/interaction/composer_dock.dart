import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:file_picker/file_picker.dart';
import 'package:desktop_drop/desktop_drop.dart';

import '../../app/theme/studio_tokens.dart';
import '../../data/repositories/studio_repository.dart';
import '../../domain/models/studio_models.dart';
import '../../l10n/studio_l10n.dart';
import '../../platform/clipboard_image_reader.dart';
import '../../shared/studio_chrome.dart';
import '../../shared/studio_driver_keys.dart';
import '../../shared/studio_driver_state.dart';
import '../../shared/upward_popup_menu.dart';
import '../status/session_selectors.dart';
import 'interaction_payload.dart';
import 'plan_confirmation_dock.dart';
import 'tool_approval_dock.dart';
import 'user_input_dock.dart';

part 'composer_attachments.dart';

class ComposerDock extends ConsumerWidget {
  const ComposerDock({
    required this.workspace,
    this.compact = false,
    super.key,
  });

  final AgentWorkspaceView workspace;

  /// 矮窗口紧凑布局：收起留白与输入行数，但发送/停止等操作保持不变。
  final bool compact;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
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
        padding: compact
            ? const EdgeInsets.fromLTRB(12, 4, 12, 6)
            : const EdgeInsets.fromLTRB(12, 7, 12, 12),
        child: Align(
          alignment: Alignment.center,
          child: ConstrainedBox(
            constraints: const BoxConstraints(
              maxWidth: StudioLayout.conversationWidth,
            ),
            child: workspace.thread.isRetiredAgent
                ? Text(
                    context.l10n.agentRoleRetired,
                    key: const ValueKey('retired-agent-notice'),
                  )
                : interaction == null
                ? workspace.composerMode == AgentComposerMode.runtimeDriven
                      ? _RuntimeDrivenAgentDock(workspace: workspace)
                      : _PromptComposer(
                          workspace: workspace,
                          enabled: true,
                          compact: compact,
                        )
                : _InteractionDock(
                    workspace: workspace,
                    interaction: interaction,
                    // 活动交互与消息提交仅依赖内存会话。
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
    final route = modeRouteFor(view.modeModelRoutes, view.mode);
    final model = route == null
        ? null
        : modelForRoute(view.providers, route.providerId, route.model);
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
              clipboard: ref.read(clipboardImageReaderProvider),
              selectorBar: Wrap(
                key: StudioDriverKeys.startPageSelectors,
                spacing: 6,
                runSpacing: 4,
                children: [
                  SessionModeSelector(
                    mode: view.mode,
                    onSelected: controller.setNewThreadMode,
                  ),
                  SessionWorkspaceModeSelector(
                    mode: view.workspaceMode,
                    onSelected: controller.setNewThreadWorkspaceMode,
                  ),
                  if (route != null) ...[
                    ModelRoleSelector(
                      providers: view.providers,
                      providerId: route.providerId,
                      model: route.model,
                      effort: route.effort,
                      onSelected: (providerId, model, effort) =>
                          controller.setModeModelRoute(
                            mode: view.mode,
                            providerId: providerId,
                            model: model,
                            effort: effort,
                          ),
                    ),
                    ReasoningEffortSelector(
                      providers: view.providers,
                      providerId: route.providerId,
                      model: route.model,
                      effort: route.effort,
                      onSelected: (providerId, model, effort) =>
                          controller.setModeModelRoute(
                            mode: view.mode,
                            providerId: providerId,
                            model: model,
                            effort: effort,
                          ),
                    ),
                  ],
                ],
              ),
              onChanged: controller.updateNewThreadComposer,
              onSubmit: () => unawaited(controller.submitNewThreadComposer()),
              inputCapabilities: model?.inputCapabilities ?? const [],
              onPasteImage: controller.addClipboardImage,
              onReportFailure: controller.reportComposerFailure,
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
      backgroundColor: context.colors.surfaceContainer,
      borderColor: context.colors.outlineVariant,
      radius: StudioRadii.lg,
      padding: const EdgeInsets.symmetric(horizontal: 14, vertical: 9),
      child: Row(
        children: [
          Icon(
            Icons.lock_clock_outlined,
            size: 18,
            color: context.colors.onSurfaceVariant,
          ),
          const SizedBox(width: 9),
          Expanded(
            child: Text(
              context.l10n.composerAgentRuntimeDriven,
              style: Theme.of(context).textTheme.bodyMedium
                  ?.copyWith(color: context.colors.onSurfaceVariant),
            ),
          ),
          if (workspace.isBusy) _StopButton(threadId: workspace.threadId),
        ],
      ),
    );
  }
}

class _PromptComposer extends ConsumerWidget {
  const _PromptComposer({
    required this.workspace,
    required this.enabled,
    this.compact = false,
  });

  final AgentWorkspaceView workspace;
  final bool enabled;
  final bool compact;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final controller = ref.read(studioControllerProvider.notifier);
    final route = workspace.runtime.modelRoute;
    final model = route == null
        ? null
        : modelForRoute(workspace.providers, route.providerId, route.model);
    return _PromptComposerPanel(
      composer: workspace.composer,
      permissionMode: workspace.permissionMode,
      enabled: enabled,
      isBusy: workspace.isBusy,
      compact: compact,
      clipboard: ref.read(clipboardImageReaderProvider),
      selectorBar: workspace.thread.isRoot
          ? Wrap(
              crossAxisAlignment: WrapCrossAlignment.center,
              spacing: 2,
              runSpacing: 4,
              children: [
                SessionModeSelector(
                  mode: workspace.thread.mode,
                  enabled:
                      !workspace.runtime.hasActiveWorkflow &&
                      workspace.thread.status == ThreadStatusView.idle &&
                      workspace.activeInteraction == null,
                  onSelected: controller.setThreadMode,
                ),
                if (route != null) ...[
                  ModelRoleSelector(
                    providers: workspace.providers,
                    providerId: route.providerId,
                    model: route.model,
                    effort: route.effort,
                    available: route.available,
                    onSelected: (providerId, model, effort) =>
                        controller.setThreadModelRoute(
                          providerId: providerId,
                          model: model,
                          effort: effort,
                        ),
                  ),
                  ReasoningEffortSelector(
                    providers: workspace.providers,
                    providerId: route.providerId,
                    model: route.model,
                    effort: route.effort,
                    onSelected: (providerId, model, effort) =>
                        controller.setThreadModelRoute(
                          providerId: providerId,
                          model: model,
                          effort: effort,
                        ),
                  ),
                ],
              ],
            )
          : null,
      onChanged: (value) =>
          controller.updateComposer(workspace.threadId, value),
      onSubmit: () => unawaited(controller.submitComposer(workspace.threadId)),
      onStop: () => unawaited(controller.stop(workspace.threadId)),
      inputCapabilities: model?.inputCapabilities ?? const [],
      onPasteImage: (bytes) =>
          controller.addClipboardImage(bytes, threadId: workspace.threadId),
      onReportFailure: (error) =>
          controller.reportComposerFailure(error, threadId: workspace.threadId),
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
    required this.clipboard,
    required this.onChanged,
    required this.onSubmit,
    required this.inputCapabilities,
    required this.onPasteImage,
    required this.onReportFailure,
    required this.onAddLocal,
    required this.onAddUrl,
    required this.onRemoveAttachment,
    this.onStop,
    this.selectorBar,
    this.compact = false,
  });

  final ComposerThreadState composer;
  final PermissionMode permissionMode;
  final bool enabled;
  final bool isBusy;
  final ClipboardImageReader clipboard;
  final ValueChanged<String> onChanged;
  final VoidCallback onSubmit;
  final List<ModelInputCapabilityView> inputCapabilities;
  final Future<void> Function(Uint8List pngBytes) onPasteImage;
  final ValueChanged<Object> onReportFailure;
  final Future<void> Function(List<String> paths) onAddLocal;
  final Future<void> Function(String url) onAddUrl;
  final Future<void> Function(String draftId) onRemoveAttachment;
  final VoidCallback? onStop;
  final Widget? selectorBar;

  /// 矮窗口紧凑布局：收起面板留白与输入最小行数，操作按钮保持不变。
  final bool compact;

  @override
  State<_PromptComposerPanel> createState() => _PromptComposerPanelState();
}

class _PromptComposerPanelState extends State<_PromptComposerPanel> {
  late final TextEditingController _controller;
  late final FocusNode _inputFocusNode;
  bool _dragging = false;
  bool _pasteInFlight = false;

  @override
  void initState() {
    super.initState();
    _controller = TextEditingController(text: widget.composer.draft);
    _inputFocusNode = FocusNode(debugLabel: 'composer-input');
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
    _inputFocusNode.dispose();
    _controller.dispose();
    super.dispose();
  }

  /// 主按钮与回车提交共享同一前置条件：启用、有内容且不在提交中。
  bool get _hasContent =>
      widget.composer.draft.trim().isNotEmpty ||
      widget.composer.attachments.isNotEmpty;

  bool get _canSubmit =>
      widget.enabled && _hasContent && !widget.composer.isSubmissionPending;

  bool get _supportsPastedImages => widget.inputCapabilities.any(
    (capability) =>
        capability.modality == ModelModalityView.image &&
        capability.supportsSource(ModelInputSourceView.local),
  );

  /// 回车提交当前草稿，语义与主按钮一致。
  ///
  /// Shift+Enter 与输入法组字过程保持原生行为（插入换行）；未满足提交前置条件
  /// 时不消费按键。返回 [KeyEventResult.handled] 让引擎不再把它当作文本输入。
  KeyEventResult _handleComposerKey(FocusNode node, KeyEvent event) {
    if (event is! KeyDownEvent) return KeyEventResult.ignored;
    final key = event.logicalKey;
    if (_isPasteShortcut(key)) {
      if (!_inputFocusNode.hasFocus ||
          !widget.enabled ||
          widget.composer.isSubmissionPending) {
        return KeyEventResult.ignored;
      }
      if (!_pasteInFlight) unawaited(_pasteClipboard());
      return KeyEventResult.handled;
    }
    if (key != LogicalKeyboardKey.enter &&
        key != LogicalKeyboardKey.numpadEnter) {
      return KeyEventResult.ignored;
    }
    if (HardwareKeyboard.instance.isShiftPressed ||
        _controller.value.composing.isValid) {
      return KeyEventResult.ignored;
    }
    if (!_canSubmit) return KeyEventResult.ignored;
    widget.onSubmit();
    return KeyEventResult.handled;
  }

  bool _isPasteShortcut(LogicalKeyboardKey key) {
    final keyboard = HardwareKeyboard.instance;
    return key == LogicalKeyboardKey.paste ||
        key == LogicalKeyboardKey.keyV &&
            (keyboard.isControlPressed || keyboard.isMetaPressed) ||
        key == LogicalKeyboardKey.insert && keyboard.isShiftPressed;
  }

  Future<void> _pasteClipboard() async {
    _pasteInFlight = true;
    try {
      final image = await widget.clipboard.readImage();
      if (!mounted) return;
      if (image != null) {
        if (!_supportsPastedImages) {
          widget.onReportFailure(
            StateError(context.l10n.composerClipboardImageUnsupported),
          );
          return;
        }
        await widget.onPasteImage(image);
        return;
      }
      final text = await widget.clipboard.readText();
      if (!mounted || text == null || text.isEmpty) return;
      _pasteText(text);
    } catch (_) {
      if (mounted) {
        widget.onReportFailure(
          StateError(context.l10n.composerClipboardReadFailed),
        );
      }
    } finally {
      _pasteInFlight = false;
    }
  }

  void _pasteText(String text) {
    final value = _controller.value;
    final selection = value.selection;
    if (!selection.isValid) return;
    final collapsed = value.copyWith(
      selection: TextSelection.collapsed(offset: selection.end),
      composing: TextRange.empty,
    );
    _controller.value = collapsed.replaced(selection, text);
    widget.onChanged(_controller.text);
  }

  @override
  Widget build(BuildContext context) {
    final colors = Theme.of(context).colorScheme;
    final composer = widget.composer;
    final showStop =
        widget.isBusy && !_hasContent && !composer.isSubmissionPending;
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
    final attachmentEnabled = widget.enabled && !composer.isSubmissionPending;
    final panel = StudioPanel(
      backgroundColor: colors.surfaceContainerLowest,
      borderColor: _dragging
          ? colors.primary
          : colors.outlineVariant.withValues(alpha: 0.86),
      radius: StudioRadii.lg,
      shadow: false,
      padding: widget.compact
          ? const EdgeInsets.fromLTRB(12, 6, 10, 6)
          : const EdgeInsets.fromLTRB(12, 8, 10, 10),
      child: Column(
        mainAxisSize: MainAxisSize.min,
        children: [
          if (composer.attachments.isNotEmpty)
            _AttachmentDraftRail(
              attachments: composer.attachments,
              enabled: attachmentEnabled,
              onRemove: (id) => unawaited(widget.onRemoveAttachment(id)),
            ),
          Focus(
            canRequestFocus: false,
            skipTraversal: true,
            onKeyEvent: _handleComposerKey,
            child: TextField(
              key: StudioDriverKeys.composerInput,
              controller: _controller,
              focusNode: _inputFocusNode,
              enabled: widget.enabled && !composer.isSubmissionPending,
              minLines: widget.compact ? 2 : 3,
              maxLines: 8,
              decoration: InputDecoration(
                hintText: context.l10n.composerHint,
                hintStyle: TextStyle(color: colors.onSurfaceVariant),
                isDense: true,
                filled: false,
                border: InputBorder.none,
                enabledBorder: InputBorder.none,
                focusedBorder: InputBorder.none,
                contentPadding: const EdgeInsets.symmetric(vertical: 8),
              ),
              onChanged: widget.onChanged,
              onSubmitted: (_) {
                if (_canSubmit) {
                  widget.onSubmit();
                }
              },
            ),
          ),
          if (widget.isBusy)
            Align(
              alignment: Alignment.centerLeft,
              child: Text(
                context.l10n.composerEscStopHint,
                style: Theme.of(context).textTheme.bodySmall,
              ),
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
              Expanded(
                child: Wrap(
                  alignment: WrapAlignment.end,
                  crossAxisAlignment: WrapCrossAlignment.center,
                  spacing: 2,
                  runSpacing: 4,
                  children: [
                    _PermissionSelector(mode: widget.permissionMode),
                    ?widget.selectorBar,
                  ],
                ),
              ),
              const SizedBox(width: 8),
              IconButton.filled(
                key: showStop
                    ? StudioDriverKeys.composerStop
                    : !_canSubmit
                    ? const ValueKey('composer-submit-disabled')
                    : StudioDriverKeys.composerSubmit,
                tooltip: showStop
                    ? context.l10n.composerStop
                    : widget.isBusy
                    ? context.l10n.composerSendAndContinue
                    : context.l10n.composerSend,
                style: IconButton.styleFrom(
                  fixedSize: const Size.square(40),
                  shape: const CircleBorder(),
                  backgroundColor: showStop
                      ? colors.surfaceContainerHighest
                      : colors.primary,
                  foregroundColor: showStop
                      ? colors.onSurface
                      : colors.onPrimary,
                  disabledBackgroundColor: colors.surfaceContainerHighest,
                  disabledForegroundColor: colors.onSurface.withValues(
                    alpha: 0.32,
                  ),
                ),
                icon: composer.isSubmissionPending
                    ? const SizedBox.square(
                        key: StudioDriverKeys.composerPending,
                        dimension: 18,
                        child: CircularProgressIndicator(strokeWidth: 2),
                      )
                    : Icon(
                        showStop
                            ? Icons.stop_rounded
                            : Icons.arrow_upward_rounded,
                      ),
                onPressed: showStop
                    ? widget.onStop
                    : _canSubmit
                    ? widget.onSubmit
                    : null,
              ),
            ],
          ),
        ],
      ),
    );
    return CallbackShortcuts(
      bindings: {
        if (widget.isBusy &&
            !composer.isSubmissionPending &&
            widget.onStop != null)
          const SingleActivator(LogicalKeyboardKey.escape): widget.onStop!,
      },
      child: DropTarget(
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
      ),
    );
  }

  Future<void> _pickLocalAttachments(
    List<ModelInputCapabilityView> capabilities,
  ) async {
    const driverFixture = String.fromEnvironment(
      'ANYWORK_DRIVER_ATTACHMENT_PATH',
    );
    if (const bool.fromEnvironment('ANYWORK_DRIVER') &&
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
        title: Text(context.l10n.composerAddUrlTitle),
        content: TextField(
          key: StudioDriverKeys.attachmentUrlInput,
          autofocus: true,
          onChanged: (value) => draft = value,
          decoration: const InputDecoration(hintText: 'https://…'),
        ),
        actions: [
          TextButton(
            onPressed: () => Navigator.pop(context),
            child: Text(context.l10n.commonCancel),
          ),
          FilledButton(
            key: StudioDriverKeys.attachmentUrlSubmit,
            onPressed: () => Navigator.pop(context, draft.trim()),
            child: Text(context.l10n.composerAddUrlConfirm),
          ),
        ],
      ),
    );
    if (url != null && url.isNotEmpty) await widget.onAddUrl(url);
  }
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
      child: StudioMenuLabel(label: context.permissionModeLabel(mode)),
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
    final trailing = workspace.isBusy
        ? _StopButton(threadId: workspace.threadId)
        : null;
    final plan = interaction.planConfirmation;
    if (plan != null) {
      return PlanConfirmationDock(
        threadId: workspace.threadId,
        plan: plan,
        enabled: enabled,
        trailing: trailing,
      );
    }
    final payload = InteractionPayloadSnapshot.from(interaction);
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
