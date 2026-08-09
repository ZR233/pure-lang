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
import 'interaction_payload.dart';
import 'plan_confirmation_dock.dart';
import 'task_recovery_dialog.dart';
import 'tool_approval_dock.dart';
import 'user_input_dock.dart';

class ComposerDock extends ConsumerWidget {
  const ComposerDock({required this.workspace, super.key});

  final AgentWorkspaceView workspace;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    StudioDriverState.publishWorkspace(workspace);
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
                ? workspace.isTaskPaused
                      ? _TaskResumeDock(workspace: workspace)
                      : workspace.composerMode ==
                            AgentComposerMode.runtimeDriven
                      ? _RuntimeDrivenAgentDock(workspace: workspace)
                      : _PromptComposer(workspace: workspace)
                : _InteractionDock(
                    workspace: workspace,
                    interaction: interaction,
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
              style: Theme.of(
                context,
              ).textTheme.bodyMedium?.copyWith(color: context.studioInkSoft),
            ),
          ),
          if (workspace.isBusy) _StopButton(threadId: workspace.threadId),
        ],
      ),
    );
  }
}

class _TaskResumeDock extends ConsumerWidget {
  const _TaskResumeDock({required this.workspace});

  final AgentWorkspaceView workspace;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final composer = workspace.composer;
    final colors = Theme.of(context).colorScheme;
    return StudioPanel(
      key: StudioDriverKeys.taskPaused,
      backgroundColor: context.studioPaper2,
      borderColor: context.studioLine,
      radius: StudioRadii.lg,
      padding: const EdgeInsets.symmetric(horizontal: 14, vertical: 12),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Row(
            children: [
              Icon(
                Icons.pause_circle_outline,
                size: 20,
                color: context.studioInkSoft,
              ),
              const SizedBox(width: 10),
              Expanded(
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    Text(
                      context.l10n.taskResumeTitle,
                      style: Theme.of(context).textTheme.titleSmall,
                    ),
                    const SizedBox(height: 2),
                    Text(
                      context.l10n.taskResumeBody,
                      style: Theme.of(context).textTheme.bodySmall?.copyWith(
                        color: context.studioInkSoft,
                      ),
                    ),
                  ],
                ),
              ),
              const SizedBox(width: 12),
              FilledButton.icon(
                key: StudioDriverKeys.taskResume,
                onPressed: composer.isSubmissionPending
                    ? null
                    : () => unawaited(
                        showTaskRecoveryDialog(context, workspace.threadId),
                      ),
                icon: composer.isSubmissionPending
                    ? const SizedBox.square(
                        dimension: 16,
                        child: CircularProgressIndicator(strokeWidth: 2),
                      )
                    : const Icon(Icons.play_arrow),
                label: Text(context.l10n.taskResumeAction),
              ),
            ],
          ),
          if (composer.error case final error?)
            Padding(
              padding: const EdgeInsets.only(top: 8),
              child: Text(
                error,
                key: StudioDriverKeys.composerError,
                style: Theme.of(
                  context,
                ).textTheme.bodySmall?.copyWith(color: colors.error),
              ),
            ),
        ],
      ),
    );
  }
}

class _PromptComposer extends ConsumerStatefulWidget {
  const _PromptComposer({required this.workspace});

  final AgentWorkspaceView workspace;

  @override
  ConsumerState<_PromptComposer> createState() => _PromptComposerState();
}

class _PromptComposerState extends ConsumerState<_PromptComposer> {
  late final TextEditingController _controller;

  @override
  void initState() {
    super.initState();
    _controller = TextEditingController(text: widget.workspace.composer.draft);
  }

  @override
  void didUpdateWidget(covariant _PromptComposer oldWidget) {
    super.didUpdateWidget(oldWidget);
    final nextText = widget.workspace.composer.draft;
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
    final composer = widget.workspace.composer;
    final canSubmit =
        composer.draft.trim().isNotEmpty &&
        !widget.workspace.isBusy &&
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
            enabled: !composer.isSubmissionPending,
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
            onChanged: (value) => ref
                .read(studioControllerProvider.notifier)
                .updateComposer(widget.workspace.threadId, value),
            onSubmitted: (_) {
              if (canSubmit) {
                unawaited(
                  ref
                      .read(studioControllerProvider.notifier)
                      .submitComposer(widget.workspace.threadId),
                );
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
                  style: Theme.of(
                    context,
                  ).textTheme.bodySmall?.copyWith(color: colors.error),
                ),
              ),
            ),
          Row(
            children: [
              _PermissionSelector(mode: widget.workspace.permissionMode),
              const Spacer(),
              if (widget.workspace.isBusy)
                IconButton.filledTonal(
                  key: StudioDriverKeys.composerStop,
                  tooltip: context.l10n.composerStop,
                  icon: const Icon(Icons.stop),
                  onPressed: () => ref
                      .read(studioControllerProvider.notifier)
                      .stop(widget.workspace.threadId),
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
                  onPressed: canSubmit
                      ? () => unawaited(
                          ref
                              .read(studioControllerProvider.notifier)
                              .submitComposer(widget.workspace.threadId),
                        )
                      : null,
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
                style: Theme.of(
                  context,
                ).textTheme.labelMedium?.copyWith(color: context.studioInkSoft),
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
  const _InteractionDock({required this.workspace, required this.interaction});

  final AgentWorkspaceView workspace;
  final PendingInteraction interaction;

  @override
  Widget build(BuildContext context) {
    final payload = InteractionPayloadSnapshot.from(interaction);
    final trailing = workspace.isBusy
        ? _StopButton(threadId: workspace.threadId)
        : null;
    return switch (interaction.kind) {
      InteractionKind.toolApproval => ToolApprovalDock(
        threadId: workspace.threadId,
        payload: payload,
        trailing: trailing,
      ),
      InteractionKind.userInput => UserInputDock(
        threadId: workspace.threadId,
        interactionId: interaction.id,
        payload: payload,
        trailing: trailing,
      ),
      InteractionKind.planConfirmation => PlanConfirmationDock(
        threadId: workspace.threadId,
        planContent: payload.planContent,
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
