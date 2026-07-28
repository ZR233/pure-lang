import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../app/theme/studio_tokens.dart';
import '../../data/repositories/studio_repository.dart';
import '../../domain/models/studio_models.dart';
import '../../l10n/studio_l10n.dart';
import '../../shared/studio_chrome.dart';
import '../../shared/upward_popup_menu.dart';
import 'interaction_payload.dart';
import 'plan_confirmation_dock.dart';
import 'tool_approval_dock.dart';
import 'user_input_dock.dart';

class ComposerDock extends ConsumerWidget {
  const ComposerDock({required this.workspace, super.key});

  final AgentWorkspaceView workspace;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
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
          if (workspace.isBusy) _StopButton(sessionId: workspace.sessionId),
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
    _controller = TextEditingController(text: widget.workspace.composerText);
  }

  @override
  void didUpdateWidget(covariant _PromptComposer oldWidget) {
    super.didUpdateWidget(oldWidget);
    final nextText = widget.workspace.composerText;
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
    final canSubmit =
        widget.workspace.composerText.trim().isNotEmpty &&
        !widget.workspace.isBusy;
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
            controller: _controller,
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
                .updateComposer(widget.workspace.sessionId, value),
            onSubmitted: (_) {
              if (canSubmit) {
                ref
                    .read(studioControllerProvider.notifier)
                    .submitComposer(widget.workspace.sessionId);
              }
            },
          ),
          Row(
            children: [
              _PermissionSelector(mode: widget.workspace.permissionMode),
              const Spacer(),
              if (widget.workspace.isBusy)
                IconButton.filledTonal(
                  tooltip: context.l10n.composerStop,
                  icon: const Icon(Icons.stop),
                  onPressed: () => ref
                      .read(studioControllerProvider.notifier)
                      .stop(widget.workspace.sessionId),
                )
              else
                IconButton.filled(
                  tooltip: context.l10n.composerSend,
                  style: IconButton.styleFrom(
                    backgroundColor: StudioColors.clay,
                    foregroundColor: Colors.white,
                  ),
                  icon: const Icon(Icons.arrow_upward),
                  onPressed: canSubmit
                      ? () => ref
                            .read(studioControllerProvider.notifier)
                            .submitComposer(widget.workspace.sessionId)
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
        ? _StopButton(sessionId: workspace.sessionId)
        : null;
    return switch (interaction.kind) {
      InteractionKind.toolApproval => ToolApprovalDock(
        sessionId: workspace.sessionId,
        payload: payload,
        trailing: trailing,
      ),
      InteractionKind.userInput => UserInputDock(
        sessionId: workspace.sessionId,
        interactionId: interaction.id,
        payload: payload,
        trailing: trailing,
      ),
      InteractionKind.planConfirmation => PlanConfirmationDock(
        sessionId: workspace.sessionId,
        trailing: trailing,
      ),
    };
  }
}

class _StopButton extends ConsumerWidget {
  const _StopButton({required this.sessionId});

  final String sessionId;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    return IconButton.filledTonal(
      tooltip: context.l10n.composerStop,
      icon: const Icon(Icons.stop),
      onPressed: () =>
          ref.read(studioControllerProvider.notifier).stop(sessionId),
    );
  }
}
