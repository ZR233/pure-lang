import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

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
import 'plan_confirmation_dock.dart';
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
    return _PromptComposerPanel(
      composer: workspace.composer,
      permissionMode: workspace.permissionMode,
      enabled: enabled,
      isBusy: workspace.isBusy,
      onChanged: (value) =>
          controller.updateComposer(workspace.threadId, value),
      onSubmit: () => unawaited(controller.submitComposer(workspace.threadId)),
      onStop: () => unawaited(controller.stop(workspace.threadId)),
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
    this.onStop,
    this.selectorBar,
  });

  final ComposerThreadState composer;
  final PermissionMode permissionMode;
  final bool enabled;
  final bool isBusy;
  final ValueChanged<String> onChanged;
  final VoidCallback onSubmit;
  final VoidCallback? onStop;
  final Widget? selectorBar;

  @override
  State<_PromptComposerPanel> createState() => _PromptComposerPanelState();
}

class _PromptComposerPanelState extends State<_PromptComposerPanel> {
  late final TextEditingController _controller;

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
        composer.draft.trim().isNotEmpty &&
        !widget.isBusy &&
        !composer.isSubmissionPending;
    return StudioPanel(
      backgroundColor: colors.surfaceContainerLowest,
      borderColor: colors.outlineVariant.withValues(alpha: 0.86),
      radius: StudioRadii.lg,
      shadow: true,
      padding: const EdgeInsets.fromLTRB(12, 8, 10, 10),
      child: Column(
        mainAxisSize: MainAxisSize.min,
        children: [
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
      InteractionKind.planConfirmation => PlanConfirmationDock(
        threadId: workspace.threadId,
        interactionId: interaction.id,
        planContent: payload.planContent,
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
