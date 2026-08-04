import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../data/repositories/studio_repository.dart';
import '../../domain/models/studio_models.dart';
import '../../l10n/studio_l10n.dart';
import '../../shared/studio_driver_keys.dart';
import 'interaction_payload.dart';
import 'interaction_widgets.dart';

class ToolApprovalDock extends ConsumerStatefulWidget {
  const ToolApprovalDock({
    required this.threadId,
    required this.payload,
    this.trailing,
    super.key,
  });

  final String threadId;
  final InteractionPayloadSnapshot payload;
  final Widget? trailing;

  @override
  ConsumerState<ToolApprovalDock> createState() => _ToolApprovalDockState();
}

class _ToolApprovalDockState extends ConsumerState<ToolApprovalDock> {
  late final TextEditingController _reasonController;

  @override
  void initState() {
    super.initState();
    _reasonController = TextEditingController();
  }

  @override
  void dispose() {
    _reasonController.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final payload = widget.payload;
    final arguments = payload.formattedArguments;
    return InteractionDockShell(
      kind: InteractionDockKind.permission,
      trailing: widget.trailing,
      title: context.l10n.interactionPermissionTitle,
      subtitle: context.l10n.interactionPermissionSubtitle,
      footerHint: context.l10n.interactionPermissionFooterHint,
      footer: DockActions(
        children: [
          OutlinedButton.icon(
            key: StudioDriverKeys.toolDeny,
            icon: const Icon(Icons.close),
            label: Text(context.l10n.interactionReject),
            onPressed: () => _resolve(ToolApprovalDecision.denied),
          ),
          FilledButton.icon(
            key: StudioDriverKeys.toolApprove,
            icon: const Icon(Icons.check),
            label: Text(context.l10n.interactionApprove),
            onPressed: () => _resolve(ToolApprovalDecision.approved),
          ),
        ],
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        mainAxisSize: MainAxisSize.min,
        children: [
          Wrap(
            spacing: 8,
            runSpacing: 8,
            children: [
              InfoChip(icon: Icons.build_outlined, label: payload.toolName),
              if (payload.workingDirectory.isNotEmpty)
                InfoChip(
                  icon: Icons.folder_outlined,
                  label: payload.workingDirectory,
                ),
            ],
          ),
          if (arguments.isNotEmpty) ...[
            const SizedBox(height: 10),
            InteractionCodeBlock(text: arguments),
          ],
          const SizedBox(height: 10),
          TextField(
            controller: _reasonController,
            minLines: 1,
            maxLines: 3,
            decoration: InputDecoration(
              labelText: context.l10n.interactionReasonLabel,
              prefixIcon: const Icon(Icons.chat_bubble_outline),
            ),
            onChanged: (_) => setState(() {}),
          ),
        ],
      ),
    );
  }

  void _resolve(ToolApprovalDecision decision) {
    final reason = _reasonController.text.trim();
    ref
        .read(studioControllerProvider.notifier)
        .resolveActiveInteraction(
          widget.threadId,
          ToolApprovalResolutionCommand(
            decision: decision,
            reason: reason.isEmpty ? null : reason,
          ),
        );
  }
}
