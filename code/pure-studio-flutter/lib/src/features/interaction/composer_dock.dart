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
  const ComposerDock({required this.state, super.key});

  final StudioState state;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final interaction = state.activeInteraction;
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
                ? _PromptComposer(state: state)
                : _InteractionDock(state: state, interaction: interaction),
          ),
        ),
      ),
    );
  }
}

class _PromptComposer extends ConsumerStatefulWidget {
  const _PromptComposer({required this.state});

  final StudioState state;

  @override
  ConsumerState<_PromptComposer> createState() => _PromptComposerState();
}

class _PromptComposerState extends ConsumerState<_PromptComposer> {
  late final TextEditingController _controller;

  @override
  void initState() {
    super.initState();
    _controller = TextEditingController(text: widget.state.composerText);
  }

  @override
  void didUpdateWidget(covariant _PromptComposer oldWidget) {
    super.didUpdateWidget(oldWidget);
    final nextText = widget.state.composerText;
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
        widget.state.selectedSessionId != null &&
        widget.state.composerText.trim().isNotEmpty &&
        !widget.state.isBusy;
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
            onChanged: ref
                .read(studioControllerProvider.notifier)
                .updateComposer,
            onSubmitted: (_) {
              if (canSubmit) {
                ref.read(studioControllerProvider.notifier).submitComposer();
              }
            },
          ),
          Row(
            children: [
              _PermissionSelector(mode: widget.state.permissionMode),
              const Spacer(),
              if (widget.state.isBusy)
                IconButton.filledTonal(
                  tooltip: context.l10n.composerStop,
                  icon: const Icon(Icons.stop),
                  onPressed: ref.read(studioControllerProvider.notifier).stop,
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
                      ? ref
                            .read(studioControllerProvider.notifier)
                            .submitComposer
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
  const _InteractionDock({required this.state, required this.interaction});

  final StudioState state;
  final PendingInteraction interaction;

  @override
  Widget build(BuildContext context) {
    final payload = InteractionPayloadSnapshot.from(interaction);
    final trailing = state.isBusy ? const _StopButton() : null;
    return switch (interaction.kind) {
      InteractionKind.toolApproval => ToolApprovalDock(
        payload: payload,
        trailing: trailing,
      ),
      InteractionKind.userInput => UserInputDock(
        interactionId: interaction.id,
        payload: payload,
        trailing: trailing,
      ),
      InteractionKind.planConfirmation => PlanConfirmationDock(
        trailing: trailing,
      ),
    };
  }
}

class _StopButton extends ConsumerWidget {
  const _StopButton();

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    return IconButton.filledTonal(
      tooltip: context.l10n.composerStop,
      icon: const Icon(Icons.stop),
      onPressed: ref.read(studioControllerProvider.notifier).stop,
    );
  }
}
