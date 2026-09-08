import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../data/repositories/studio_repository.dart';
import '../../domain/models/studio_models.dart';
import '../../l10n/studio_l10n.dart';
import 'settings_common.dart';

class WorktreeRecoverySection extends ConsumerStatefulWidget {
  const WorktreeRecoverySection({super.key, required this.issue});

  final StudioRecoveryIssue issue;

  @override
  ConsumerState<WorktreeRecoverySection> createState() =>
      _WorktreeRecoverySectionState();
}

class _WorktreeRecoverySectionState
    extends ConsumerState<WorktreeRecoverySection> {
  bool _cleaning = false;

  @override
  Widget build(BuildContext context) {
    final worktree = widget.issue.worktree!;
    final l10n = context.l10n;
    final head = worktree.headCommit;
    return SettingsResourceRow(
      key: ValueKey('worktree-recovery-${worktree.childId}'),
      title: worktree.branch,
      icon: Icons.account_tree_outlined,
      status: Text([worktree.state, if (worktree.dirty) 'dirty'].join(' · ')),
      actions: [
        TextButton.icon(
          key: ValueKey('worktree-cleanup-${worktree.childId}'),
          onPressed: _cleaning ? null : () => _cleanup(worktree),
          icon: _cleaning
              ? const SizedBox.square(
                  dimension: 16,
                  child: CircularProgressIndicator(strokeWidth: 2),
                )
              : const Icon(Icons.delete_sweep_outlined, size: 18),
          label: Text(context.l10n.settingsWorktreeCleanup),
        ),
      ],
      children: [
        SelectableText(
          [
            l10n.settingsWorktreeBase(worktree.baseCommit),
            head == null
                ? l10n.settingsWorktreeHeadUnavailable
                : l10n.settingsWorktreeHead(head),
            worktree.path,
          ].join('\n'),
        ),
        if (worktree.changedFiles.isNotEmpty) ...[
          const SizedBox(height: 8),
          Text(
            context.l10n.settingsWorktreeChangedFiles(
              worktree.changedFiles.join(', '),
            ),
          ),
        ],
        const Divider(height: 24),
      ],
    );
  }

  Future<void> _cleanup(WorktreeRecoveryPreview worktree) async {
    setState(() => _cleaning = true);
    try {
      await ref
          .read(studioControllerProvider.notifier)
          .cleanupPreservedWorktree(worktree);
    } finally {
      if (mounted) setState(() => _cleaning = false);
    }
  }
}
