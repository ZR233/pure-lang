import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../data/repositories/studio_repository.dart';
import '../../shared/studio_chrome.dart';
import 'interaction_widgets.dart';

class PlanConfirmationDock extends ConsumerStatefulWidget {
  const PlanConfirmationDock({this.trailing, super.key});

  final Widget? trailing;

  @override
  ConsumerState<PlanConfirmationDock> createState() =>
      _PlanConfirmationDockState();
}

class _PlanConfirmationDockState extends ConsumerState<PlanConfirmationDock> {
  late final TextEditingController _adjustmentController;
  bool _editingAdjustment = false;

  @override
  void initState() {
    super.initState();
    _adjustmentController = TextEditingController();
  }

  @override
  void dispose() {
    _adjustmentController.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final canSubmitAdjustment = _adjustmentController.text.trim().isNotEmpty;
    return InteractionDockShell(
      kind: InteractionDockKind.plan,
      trailing: widget.trailing,
      title: '实施此计划？',
      subtitle: '计划正文保留在上方 timeline 卡片中',
      footerHint: _editingAdjustment
          ? '只会发送你的调整说明，不会回传计划正文。'
          : '选择实施会切回 Auto 模式并提交后台实施 prompt。',
      footer: DockActions(
        children: [
          TextButton.icon(
            icon: const Icon(Icons.close),
            label: const Text('忽略'),
            onPressed: _resolveDismiss,
          ),
          if (!_editingAdjustment)
            OutlinedButton.icon(
              icon: const Icon(Icons.edit_note),
              label: const Text('告诉 Pure 如何调整'),
              onPressed: () => setState(() => _editingAdjustment = true),
            ),
          FilledButton.icon(
            icon: Icon(_editingAdjustment ? Icons.check : Icons.play_arrow),
            label: Text(_editingAdjustment ? '提交调整' : '实施此计划'),
            onPressed: _editingAdjustment
                ? (canSubmitAdjustment ? _resolveContinuePlanning : null)
                : _resolveImplement,
          ),
        ],
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        mainAxisSize: MainAxisSize.min,
        children: [
          StudioNotice(
            icon: _editingAdjustment
                ? Icons.edit_note_outlined
                : Icons.info_outline,
            message: _editingAdjustment
                ? '继续规划：只提交你的调整说明。'
                : '计划内容不会在这里编辑；请在 timeline 中查看完整计划。',
            tone: StudioNoticeTone.warning,
            iconSize: 17,
            padding: const EdgeInsets.fromLTRB(12, 9, 12, 9),
          ),
          if (_editingAdjustment)
            Padding(
              padding: const EdgeInsets.only(top: 12),
              child: TextField(
                controller: _adjustmentController,
                autofocus: true,
                minLines: 2,
                maxLines: 5,
                decoration: const InputDecoration(
                  hintText: '告诉 Pure 需要怎样调整计划...',
                  alignLabelWithHint: true,
                  prefixIcon: Icon(Icons.edit_note),
                ),
                onChanged: (_) => setState(() {}),
                onSubmitted: (_) {
                  if (canSubmitAdjustment) {
                    _resolveContinuePlanning();
                  }
                },
              ),
            ),
        ],
      ),
    );
  }

  void _resolveImplement() {
    ref.read(studioControllerProvider.notifier).resolveActiveInteraction({
      'type': 'planConfirmation',
      'decision': 'implementFreshContext',
    });
  }

  void _resolveContinuePlanning() {
    final content = _adjustmentController.text.trim();
    if (content.isEmpty) {
      return;
    }
    ref.read(studioControllerProvider.notifier).resolveActiveInteraction({
      'type': 'planConfirmation',
      'decision': 'continuePlanning',
      'content': content,
      'reason': 'continue planning',
    });
  }

  void _resolveDismiss() {
    ref.read(studioControllerProvider.notifier).resolveActiveInteraction({
      'type': 'planConfirmation',
      'decision': 'dismiss',
      'reason': 'dismissed',
    });
  }
}
