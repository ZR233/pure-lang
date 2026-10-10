import '../../domain/models/studio_models.dart';
import '../frb/studio_api.dart';
import 'studio_state_reducer.dart';

/// Owns the pure thread-state projection boundary.
///
/// The controller coordinates Bridge calls and lifecycle, while this small
/// repository is the only place it asks to apply thread snapshots, stream
/// updates, and bounded timeline pages.  Keeping these operations behind one
/// immutable reducer facade prevents settings/catalog responses from becoming
/// an accidental second owner of thread state.
final class StudioThreadRepository {
  const StudioThreadRepository();

  StudioState applySnapshot(StudioState current, ThreadWorkspace snapshot) =>
      applyThreadSnapshot(current, snapshot);

  StudioReduceResult applyUpdate(
    StudioState current, {
    required String threadId,
    required int revision,
    required ThreadWorkspaceUpdate update,
    int? baseRevision,
  }) => applyThreadUpdate(
    current,
    threadId: threadId,
    revision: revision,
    update: update,
    baseRevision: baseRevision,
  );

  StudioState applyTimeline(
    StudioState current,
    String threadId,
    TimelinePage page,
    TimelineDirection direction, {
    bool replaceWindow = false,
    bool followBottom = false,
  }) => applyTimelinePage(
    current,
    threadId,
    page,
    direction,
    replaceWindow: replaceWindow,
    followBottom: followBottom,
  );

  StudioState releaseHistory(StudioState current, String threadId) =>
      releaseThreadHistoryPayload(current, threadId);
}
