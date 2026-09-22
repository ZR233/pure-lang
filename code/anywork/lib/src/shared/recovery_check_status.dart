import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../app/theme/studio_tokens.dart';
import '../data/repositories/studio_repository.dart';
import '../domain/models/studio_models.dart';
import '../l10n/studio_l10n.dart';

class RecoveryCheckStatus extends ConsumerWidget {
  const RecoveryCheckStatus({this.showChecking = true, super.key});

  final bool showChecking;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final resource = ref.watch(
      studioControllerProvider.select(
        (value) => value.value?.recoveryState.state,
      ),
    );
    final checking =
        resource is LoadingObservedResource ||
        resource is RefreshingObservedResource;
    final error = switch (resource) {
      FailedObservedResource(:final error) ||
      DegradedObservedResource(:final error) => error,
      _ => null,
    };
    if ((!checking || !showChecking) && error == null) {
      return const SizedBox.shrink();
    }
    return Material(
      key: const ValueKey('recovery-check-status'),
      color: context.colors.surfaceContainerLow,
      child: Padding(
        padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 10),
        child: Row(
          children: [
            if (checking)
              SizedBox.square(
                dimension: 18,
                child: CircularProgressIndicator(
                  strokeWidth: 2,
                  color: context.statusColors.activeIndicator,
                ),
              )
            else
              Icon(Icons.error_outline, size: 18, color: context.colors.error),
            const SizedBox(width: 12),
            Expanded(
              child: Semantics(
                liveRegion: true,
                child: Text(
                  checking
                      ? context.l10n.recoveryChecking
                      : '${context.l10n.recoveryCheckFailed}\n${error!.message}',
                ),
              ),
            ),
            if (!checking && error?.retryable == true)
              TextButton(
                key: const ValueKey('recovery-check-retry'),
                onPressed: () async {
                  try {
                    await ref
                        .read(studioControllerProvider.notifier)
                        .retryRecovery();
                  } catch (error) {
                    if (context.mounted) {
                      ScaffoldMessenger.of(
                        context,
                      ).showSnackBar(SnackBar(content: Text(error.toString())));
                    }
                  }
                },
                child: Text(context.l10n.runtimeFatalRetry),
              ),
          ],
        ),
      ),
    );
  }
}
