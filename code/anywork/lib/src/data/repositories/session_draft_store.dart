import '../../domain/models/studio_models.dart';

/// Application owned drafts for a new session.
///
/// These values are local presentation intent.  A Bridge settings snapshot is
/// never allowed to replace them, and disposing a route does not dispose this
/// store.  The immutable maps exposed here are only projections used by
/// [StudioState]; the store remains the owner of draft values.
final class SessionDraftStore {
  final Map<String, ThreadModeId> _modes = {};
  final Map<String, ThreadWorkspaceMode> _workspaceModes = {};
  final Map<String, ComposerThreadState> _composers = {};

  ThreadModeId modeFor(String projectId) =>
      _modes[projectId] ?? ThreadModeId.simple;

  ThreadWorkspaceMode workspaceModeFor(String projectId) =>
      _workspaceModes[projectId] ?? ThreadWorkspaceMode.local;

  ComposerThreadState composerFor(String projectId) =>
      _composers[projectId] ?? const ComposerThreadState.idle();

  void setMode(String projectId, ThreadModeId mode) => _modes[projectId] = mode;

  void setWorkspaceMode(String projectId, ThreadWorkspaceMode mode) =>
      _workspaceModes[projectId] = mode;

  void setComposer(String projectId, ComposerThreadState composer) =>
      _composers[projectId] = composer;

  void pruneTo(Iterable<String> projectIds) {
    final ids = projectIds.toSet();
    _modes.removeWhere((id, _) => !ids.contains(id));
    _workspaceModes.removeWhere((id, _) => !ids.contains(id));
    _composers.removeWhere((id, _) => !ids.contains(id));
  }

  Map<String, ThreadModeId> get modes => Map.unmodifiable(_modes);

  Map<String, ThreadWorkspaceMode> get workspaceModes =>
      Map.unmodifiable(_workspaceModes);

  Map<String, ComposerThreadState> get composers =>
      Map.unmodifiable(_composers);
}
