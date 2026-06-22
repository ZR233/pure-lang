import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../data/repositories/studio_repository.dart';
import 'interaction_payload.dart';
import 'interaction_widgets.dart';

class ToolApprovalDock extends ConsumerStatefulWidget {
  const ToolApprovalDock({required this.payload, this.trailing, super.key});

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
      header: const DockTitle(
        icon: Icons.shield_outlined,
        label: 'Permission required',
      ),
      footer: DockActions(
        children: [
          OutlinedButton.icon(
            icon: const Icon(Icons.close),
            label: const Text('Deny'),
            onPressed: () => _resolve('denied'),
          ),
          FilledButton.icon(
            icon: const Icon(Icons.check),
            label: const Text('Approve'),
            onPressed: () => _resolve('approved'),
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
            decoration: const InputDecoration(
              labelText: 'Reason',
              prefixIcon: Icon(Icons.chat_bubble_outline),
            ),
            onChanged: (_) => setState(() {}),
          ),
        ],
      ),
    );
  }

  void _resolve(String decision) {
    final reason = _reasonController.text.trim();
    ref.read(studioControllerProvider.notifier).resolveActiveInteraction({
      'type': 'toolApproval',
      'decision': decision,
      if (reason.isNotEmpty) 'reason': reason,
    });
  }
}
