import 'package:flutter/material.dart';

import '../../app/theme/studio_tokens.dart';
import '../../domain/models/studio_models.dart';
import '../../l10n/studio_l10n.dart';

class TodoPanel extends StatelessWidget {
  const TodoPanel({
    required this.todo,
    this.onClose,
    this.inDrawer = false,
    super.key,
  });

  final TimelineTodoListUpdate todo;
  final VoidCallback? onClose;
  final bool inDrawer;

  @override
  Widget build(BuildContext context) {
    final title = todo.explanation?.trim().isNotEmpty == true
        ? todo.explanation!.trim()
        : context.l10n.timelineTodoListFallback;
    return Material(
      color: context.studioPaper2,
      shape: inDrawer
          ? null
          : Border(left: BorderSide(color: context.studioLine)),
      child: SafeArea(
        left: false,
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            ListTile(
              dense: true,
              leading: const Icon(
                Icons.checklist_outlined,
                color: StudioColors.clay,
              ),
              title: Text(
                title,
                maxLines: 2,
                overflow: TextOverflow.ellipsis,
                style: Theme.of(context).textTheme.titleSmall
                    ?.copyWith(fontWeight: FontWeight.w700),
              ),
              trailing: onClose == null
                  ? null
                  : IconButton(
                      key: const ValueKey('todo-close-button'),
                      tooltip: MaterialLocalizations.of(context)
                          .closeButtonTooltip,
                      icon: const Icon(Icons.chevron_right),
                      onPressed: onClose,
                    ),
            ),
            Divider(height: 1, color: context.studioLine),
            Expanded(
              child: ListView.separated(
                padding: const EdgeInsets.symmetric(vertical: 6),
                itemCount: todo.items.length,
                separatorBuilder: (context, index) => const SizedBox(height: 1),
                itemBuilder: (context, index) =>
                    _TodoStepTile(item: todo.items[index]),
              ),
            ),
          ],
        ),
      ),
    );
  }
}

class _TodoStepTile extends StatelessWidget {
  const _TodoStepTile({required this.item});

  final TimelineTodoItem item;

  @override
  Widget build(BuildContext context) {
    final completed = item.status == 'completed';
    final inProgress = item.status == 'inProgress';
    final icon = completed
        ? Icons.check_circle_outline
        : inProgress
        ? Icons.radio_button_checked
        : Icons.radio_button_unchecked;
    final color = completed
        ? context.studioInkSoft
        : inProgress
        ? StudioColors.clay
        : context.studioInkSoft.withValues(alpha: 0.72);
    return ListTile(
      dense: true,
      visualDensity: VisualDensity.compact,
      minLeadingWidth: 24,
      leading: Icon(icon, size: 18, color: color),
      title: Text(
        item.step,
        style: Theme.of(context).textTheme.bodyMedium?.copyWith(
          color: completed ? context.studioInkSoft : context.studioInk,
          fontWeight: inProgress ? FontWeight.w600 : FontWeight.w400,
          decoration: completed ? TextDecoration.lineThrough : null,
          decorationColor: context.studioInkSoft,
        ),
      ),
    );
  }
}
