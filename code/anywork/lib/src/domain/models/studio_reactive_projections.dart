import 'package:flutter/foundation.dart' show listEquals;

import 'agent_workspace_view.dart';
import 'composer_models.dart';
import 'conversation_activity_models.dart';
import 'interaction_models.dart';
import 'model_catalog.dart';
import 'provider_models.dart';
import 'runtime_models.dart';
import 'settings_models.dart';
import 'studio_enums.dart';
import 'studio_projection_models.dart';
import 'studio_state.dart';
import 'thread_activity_models.dart';
import 'thread_directory_models.dart';
import 'turn_models.dart';

/// The runtime projection is the typed view of the selected Thread ordered
/// state. It deliberately contains no timeline rows, composer draft, or
/// settings collection, so high-frequency runtime changes cannot invalidate
/// those responsibilities.
class StudioRuntimeProjection {
  const StudioRuntimeProjection({
    required this.threadId,
    required this.runtime,
    required this.turn,
    required this.isBusy,
    required this.syncState,
    required this.loadError,
  });

  factory StudioRuntimeProjection.fromState(StudioState state) {
    final workspace = state.selectedWorkspace;
    return StudioRuntimeProjection(
      threadId: state.selectedThreadId,
      runtime: state.runtime,
      turn: workspace?.activeTurn,
      isBusy: state.isBusy,
      syncState: state.selectedWorkspaceUi.syncState,
      loadError: state.selectedWorkspaceUi.loadError,
    );
  }

  final String? threadId;
  final ThreadRuntimeView runtime;
  final StudioTurnView? turn;
  final bool isBusy;
  final AgentWorkspaceSyncState syncState;
  final String? loadError;

  @override
  bool operator ==(Object other) {
    return identical(this, other) ||
        other is StudioRuntimeProjection &&
            threadId == other.threadId &&
            runtime == other.runtime &&
            turn == other.turn &&
            isBusy == other.isBusy &&
            syncState == other.syncState &&
            loadError == other.loadError;
  }

  @override
  int get hashCode =>
      Object.hash(threadId, runtime, turn, isBusy, syncState, loadError);
}

/// Context values are split from [StudioRuntimeProjection] so the context
/// ring/detail does not rebuild for a throughput-only runtime update.
class StudioContextProjection {
  const StudioContextProjection({
    required this.threadId,
    required this.model,
    required this.contextTokens,
    required this.contextWindow,
    required this.totalTokens,
    required this.costLabel,
    required this.promptTokens,
    required this.completionTokens,
    required this.cachedPromptTokens,
    required this.cacheWriteTokens,
    required this.reasoningTokens,
    required this.inferenceCount,
    required this.cacheUsage,
    required this.estimatedCosts,
    required this.estimatedCacheSavings,
    required this.hasUnpricedUsage,
    required this.hasIncompleteUsage,
  });

  factory StudioContextProjection.fromState(StudioState state) {
    return StudioContextProjection.fromRuntime(
      state.selectedThreadId,
      state.runtime,
    );
  }

  factory StudioContextProjection.fromRuntime(
    String? threadId,
    ThreadRuntimeView runtime,
  ) {
    final live = runtime.liveUsage;
    return StudioContextProjection(
      threadId: threadId,
      model: runtime.model,
      contextTokens: live?.latestContextTokens ?? runtime.contextTokens,
      contextWindow: runtime.contextWindow,
      totalTokens: runtime.totalTokens,
      costLabel: runtime.costLabel,
      promptTokens: runtime.promptTokens,
      completionTokens: runtime.completionTokens,
      cachedPromptTokens: runtime.cachedPromptTokens,
      cacheWriteTokens: runtime.cacheWriteTokens,
      reasoningTokens: runtime.reasoningTokens,
      inferenceCount: runtime.inferenceCount,
      cacheUsage: runtime.cacheUsage,
      estimatedCosts: runtime.estimatedCosts,
      estimatedCacheSavings: runtime.estimatedCacheSavings,
      hasUnpricedUsage: runtime.hasUnpricedUsage,
      hasIncompleteUsage: runtime.hasIncompleteUsage,
    );
  }

  final String? threadId;
  final String model;
  final int contextTokens;
  final int contextWindow;
  final int totalTokens;
  final String costLabel;
  final int promptTokens;
  final int completionTokens;
  final int cachedPromptTokens;
  final int cacheWriteTokens;
  final int reasoningTokens;
  final int inferenceCount;
  final CacheUsageView cacheUsage;
  final List<RuntimeCostView> estimatedCosts;
  final List<RuntimeCostView> estimatedCacheSavings;
  final bool hasUnpricedUsage;
  final bool hasIncompleteUsage;

  bool get hasUsage =>
      inferenceCount > 0 || promptTokens > 0 || completionTokens > 0;

  @override
  bool operator ==(Object other) {
    return identical(this, other) ||
        other is StudioContextProjection &&
            threadId == other.threadId &&
            model == other.model &&
            contextTokens == other.contextTokens &&
            contextWindow == other.contextWindow &&
            totalTokens == other.totalTokens &&
            costLabel == other.costLabel &&
            promptTokens == other.promptTokens &&
            completionTokens == other.completionTokens &&
            cachedPromptTokens == other.cachedPromptTokens &&
            cacheWriteTokens == other.cacheWriteTokens &&
            reasoningTokens == other.reasoningTokens &&
            inferenceCount == other.inferenceCount &&
            cacheUsage == other.cacheUsage &&
            listEquals(estimatedCosts, other.estimatedCosts) &&
            listEquals(estimatedCacheSavings, other.estimatedCacheSavings) &&
            hasUnpricedUsage == other.hasUnpricedUsage &&
            hasIncompleteUsage == other.hasIncompleteUsage;
  }

  @override
  int get hashCode => Object.hash(
    threadId,
    model,
    contextTokens,
    contextWindow,
    totalTokens,
    costLabel,
    promptTokens,
    completionTokens,
    cachedPromptTokens,
    cacheWriteTokens,
    reasoningTokens,
    inferenceCount,
    cacheUsage,
    Object.hashAll(estimatedCosts),
    Object.hashAll(estimatedCacheSavings),
    hasUnpricedUsage,
    hasIncompleteUsage,
  );
}

class StudioThroughputProjection {
  const StudioThroughputProjection({
    required this.threadId,
    required this.completionTokens,
    required this.decodeMillis,
  });

  factory StudioThroughputProjection.fromState(StudioState state) {
    final runtime = state.runtime;
    final live = runtime.liveUsage;
    return StudioThroughputProjection(
      threadId: state.selectedThreadId,
      completionTokens: live?.completionTokens ?? runtime.turnCompletionTokens,
      decodeMillis: live?.decodeMillis ?? runtime.turnDecodeMillis,
    );
  }

  final String? threadId;
  final int completionTokens;
  final int decodeMillis;

  double? get tokensPerSecond =>
      decodeMillis > 0 ? completionTokens * 1000 / decodeMillis : null;

  @override
  bool operator ==(Object other) {
    return identical(this, other) ||
        other is StudioThroughputProjection &&
            threadId == other.threadId &&
            completionTokens == other.completionTokens &&
            decodeMillis == other.decodeMillis;
  }

  @override
  int get hashCode => Object.hash(threadId, completionTokens, decodeMillis);
}

/// Stable status inputs. Runtime/context/throughput are intentionally not part
/// of this value; status widgets subscribe to their own projections.
class StudioStatusProjection {
  const StudioStatusProjection({
    required this.thread,
    required this.permissionMode,
    required this.providers,
    required this.roles,
    required this.isBusy,
  });

  factory StudioStatusProjection.fromState(StudioState state) {
    return StudioStatusProjection(
      thread: state.selectedThread,
      permissionMode: state.permissionMode,
      providers: state.providers,
      roles: state.roles,
      isBusy: state.isBusy,
    );
  }

  final StudioThread? thread;
  final PermissionMode permissionMode;
  final List<ProviderSettingsView> providers;
  final List<RoleSettingsView> roles;
  final bool isBusy;

  @override
  bool operator ==(Object other) {
    return identical(this, other) ||
        other is StudioStatusProjection &&
            _threadEquals(thread, other.thread) &&
            permissionMode == other.permissionMode &&
            _listEqualsBy(providers, other.providers, _providerEquals) &&
            _listEqualsBy(roles, other.roles, _roleEquals) &&
            isBusy == other.isBusy;
  }

  @override
  int get hashCode => Object.hash(
    _threadHash(thread),
    permissionMode,
    Object.hashAll(providers.map(_providerHash)),
    Object.hashAll(roles.map(_roleHash)),
    isBusy,
  );
}

/// Start page projection with deep equality for provider/model-route lists.
/// `StudioState.providers` derives a fresh metadata list on each read, so a
/// record containing that list would rebuild the start page on unrelated state
/// updates even when every provider field is unchanged.
class StudioStartPageProjection {
  const StudioStartPageProjection({
    required this.isStartPage,
    required this.project,
    required this.composer,
    required this.permissionMode,
    required this.canSubmit,
    required this.mode,
    required this.workspaceMode,
    required this.targetDiagnostic,
    required this.providers,
    required this.modeModelRoutes,
    required this.modelRouteMutationPending,
  });

  final bool isStartPage;
  final StudioProject? project;
  final ComposerThreadState composer;
  final PermissionMode permissionMode;
  final bool canSubmit;
  final ThreadModeId mode;
  final ThreadWorkspaceMode workspaceMode;
  final ComposerTargetDiagnostic? targetDiagnostic;
  final List<ProviderSettingsView> providers;
  final List<ModeModelRouteView> modeModelRoutes;
  final bool modelRouteMutationPending;

  @override
  bool operator ==(Object other) {
    return identical(this, other) ||
        other is StudioStartPageProjection &&
            isStartPage == other.isStartPage &&
            _projectEquals(project, other.project) &&
            composer == other.composer &&
            permissionMode == other.permissionMode &&
            canSubmit == other.canSubmit &&
            mode == other.mode &&
            workspaceMode == other.workspaceMode &&
            targetDiagnostic == other.targetDiagnostic &&
            _listEqualsBy(providers, other.providers, _providerEquals) &&
            _listEqualsBy(
              modeModelRoutes,
              other.modeModelRoutes,
              _modeRouteEquals,
            ) &&
            modelRouteMutationPending == other.modelRouteMutationPending;
  }

  @override
  int get hashCode => Object.hash(
    isStartPage,
    _projectHash(project),
    composer,
    permissionMode,
    canSubmit,
    mode,
    workspaceMode,
    targetDiagnostic,
    Object.hashAll(providers.map(_providerHash)),
    Object.hashAll(modeModelRoutes.map(_modeRouteHash)),
    modelRouteMutationPending,
  );
}

/// Target diagnostics remain observable after a thread/workspace target is
/// terminated or removed from the directory. This projection is deliberately
/// independent from any selected workspace composer.
class StudioComposerDiagnosticsProjection {
  const StudioComposerDiagnosticsProjection({required this.diagnostics});

  factory StudioComposerDiagnosticsProjection.fromState(StudioState state) {
    final diagnostics = state.composerDiagnosticsByTarget.values.toList();
    diagnostics.sort(
      (left, right) => left.targetKey.compareTo(right.targetKey),
    );
    return StudioComposerDiagnosticsProjection(
      diagnostics: List.unmodifiable(diagnostics),
    );
  }

  final List<ComposerTargetDiagnostic> diagnostics;

  @override
  bool operator ==(Object other) {
    return identical(this, other) ||
        other is StudioComposerDiagnosticsProjection &&
            _listEqualsBy(
              diagnostics,
              other.diagnostics,
              (left, right) => left == right,
            );
  }

  @override
  int get hashCode => Object.hashAll(diagnostics);
}

/// Settings page projection keyed by settings/resource revisions and value
/// fields. The legacy SettingsPageView is retained for its existing callers,
/// while this wrapper prevents a live Thread runtime update from recreating the
/// settings page when none of its inputs changed.
class StudioSettingsProjection {
  const StudioSettingsProjection({
    required this.view,
    required this.settingsRevision,
    required this.modelCatalogRevision,
    required this.providerCatalogRevision,
    required this.providerCatalogSchemaVersion,
    required this.mcpRevision,
    required this.lspRevision,
    required this.skillsRevision,
    required this.skillsCatalogRevision,
    required this.selectedProjectId,
    required this.activeSkills,
    required this.runtimeBusy,
  });

  factory StudioSettingsProjection.fromState(StudioState state) {
    final projectId = state.selectedProjectId;
    final skills = projectId == null ? null : state.skillsByProject[projectId];
    return StudioSettingsProjection(
      view: SettingsPageView.fromState(state),
      settingsRevision: state.settingsRevision,
      modelCatalogRevision: state.settingsState.modelCatalogRevision,
      providerCatalogRevision: state.providerCatalog.revision,
      providerCatalogSchemaVersion: state.providerCatalog.schemaVersion,
      mcpRevision: state.mcpState.revision,
      lspRevision: state.lspState.revision,
      skillsRevision: skills?.revision ?? 0,
      skillsCatalogRevision: skills?.catalogRevision ?? 0,
      selectedProjectId: projectId,
      activeSkills: state.runtime.activeSkills,
      runtimeBusy: state.isBusy || state.runtime.hasActiveWorkflow,
    );
  }

  final SettingsPageView view;
  final int settingsRevision;
  final int modelCatalogRevision;
  final String providerCatalogRevision;
  final int providerCatalogSchemaVersion;
  final int mcpRevision;
  final int lspRevision;
  final int skillsRevision;
  final int skillsCatalogRevision;
  final String? selectedProjectId;
  final List<String> activeSkills;
  final bool runtimeBusy;

  @override
  bool operator ==(Object other) {
    return identical(this, other) ||
        other is StudioSettingsProjection &&
            settingsRevision == other.settingsRevision &&
            modelCatalogRevision == other.modelCatalogRevision &&
            providerCatalogRevision == other.providerCatalogRevision &&
            providerCatalogSchemaVersion ==
                other.providerCatalogSchemaVersion &&
            mcpRevision == other.mcpRevision &&
            lspRevision == other.lspRevision &&
            skillsRevision == other.skillsRevision &&
            skillsCatalogRevision == other.skillsCatalogRevision &&
            selectedProjectId == other.selectedProjectId &&
            listEquals(activeSkills, other.activeSkills) &&
            runtimeBusy == other.runtimeBusy;
  }

  @override
  int get hashCode => Object.hash(
    settingsRevision,
    modelCatalogRevision,
    providerCatalogRevision,
    providerCatalogSchemaVersion,
    mcpRevision,
    lspRevision,
    skillsRevision,
    skillsCatalogRevision,
    selectedProjectId,
    Object.hashAll(activeSkills),
    runtimeBusy,
  );
}

/// Provider usage has its own topic/revision. It must not be invalidated by a
/// Thread runtime event merely because both facts live in StudioState.
class StudioProviderUsageProjection {
  const StudioProviderUsageProjection({
    required this.revision,
    required this.configFingerprint,
    required this.usages,
  });

  factory StudioProviderUsageProjection.fromState(StudioState state) {
    final snapshot = state.providerUsageState;
    return StudioProviderUsageProjection(
      revision: snapshot.revision,
      configFingerprint: snapshot.configFingerprint,
      usages: snapshot.usages,
    );
  }

  final int revision;
  final String configFingerprint;
  final List<ProviderUsageView> usages;

  @override
  bool operator ==(Object other) {
    return identical(this, other) ||
        other is StudioProviderUsageProjection &&
            revision == other.revision &&
            configFingerprint == other.configFingerprint &&
            _listEqualsBy(usages, other.usages, _providerUsageEquals);
  }

  @override
  int get hashCode => Object.hash(
    revision,
    configFingerprint,
    Object.hashAll(usages.map(_providerUsageHash)),
  );
}

/// Composer-only projection used by the interaction dock and by the legacy
/// controls provider. It keeps stable route/workflow/turn facts but excludes
/// live usage counters, timeline rows and history state.
class StudioComposerProjection {
  const StudioComposerProjection({
    required this.thread,
    required this.rootThread,
    required this.syncState,
    required this.loadError,
    required this.composer,
    required this.composerMode,
    required this.permissionMode,
    required this.providers,
    required this.modeModelRoutes,
    required this.roles,
    required this.activeInteraction,
    required this.turn,
    required this.modelRoute,
    required this.workflow,
    required this.targetDiagnostic,
  });

  static StudioComposerProjection? fromState(StudioState state) {
    final thread = state.selectedThread;
    final root = state.selectedRootThread;
    if (thread == null || root == null) return null;
    final runtime = state.runtime;
    return StudioComposerProjection(
      thread: thread,
      rootThread: root,
      syncState: state.selectedWorkspaceUi.syncState,
      loadError: state.selectedWorkspaceUi.loadError,
      composer: state.composer,
      composerMode: thread.isAgent
          ? AgentComposerMode.runtimeDriven
          : AgentComposerMode.editable,
      permissionMode: state.permissionMode,
      providers: state.providers,
      modeModelRoutes: state.modeModelRoutes,
      roles: state.roles,
      activeInteraction: state.activeInteraction,
      turn: state.turn,
      modelRoute: runtime.modelRoute,
      workflow: runtime.workflow,
      targetDiagnostic: state.composerDiagnostic(thread.projectId, thread.id),
    );
  }

  static StudioComposerProjection fromWorkspace(AgentWorkspaceView workspace) {
    return StudioComposerProjection(
      thread: workspace.thread,
      rootThread: workspace.rootThread,
      syncState: workspace.syncState,
      loadError: workspace.loadError,
      composer: workspace.composer,
      composerMode: workspace.composerMode,
      permissionMode: workspace.permissionMode,
      providers: workspace.providers,
      modeModelRoutes: workspace.modeModelRoutes,
      roles: workspace.roles,
      activeInteraction: workspace.activeInteraction,
      turn: workspace.turn,
      modelRoute: workspace.runtime.modelRoute,
      workflow: workspace.runtime.workflow,
      targetDiagnostic: null,
    );
  }

  final StudioThread thread;
  final StudioThread rootThread;
  final AgentWorkspaceSyncState syncState;
  final String? loadError;
  final ComposerThreadState composer;
  final AgentComposerMode composerMode;
  final PermissionMode permissionMode;
  final List<ProviderSettingsView> providers;
  final List<ModeModelRouteView> modeModelRoutes;
  final List<RoleSettingsView> roles;
  final PendingInteraction? activeInteraction;
  final StudioTurnView? turn;
  final ThreadModelRouteView? modelRoute;
  final WorkflowRuntimeView? workflow;
  final ComposerTargetDiagnostic? targetDiagnostic;

  String get threadId => thread.id;
  bool get isBusy => turn?.state.isBusy ?? false;
  bool get hasActiveWorkflow => workflow?.isActive ?? false;
  bool get isRoot => thread.isRoot;

  /// Compatibility projection for callers that still accept
  /// [AgentWorkspaceView]. It has no timeline/history payload and a runtime
  /// containing only composer-relevant route/workflow fields.
  AgentWorkspaceView get legacyWorkspace {
    final thread = this.thread;
    final rootThread = this.rootThread;
    return AgentWorkspaceView(
      thread: thread,
      rootThread: rootThread,
      syncState: syncState,
      loadError: loadError,
      timelineRows: const [],
      todo: null,
      runtime: ThreadRuntimeView(
        model: modelRoute?.model ?? '',
        contextTokens: 0,
        contextWindow: 0,
        totalTokens: 0,
        costLabel: '',
        activeSkills: const [],
        activeMcpServers: const [],
        activeLspServers: const [],
        workflow: workflow,
        modelRoute: modelRoute,
      ),
      turn: turn,
      lastTurn: null,
      activeInteraction: activeInteraction,
      composer: composer,
      composerMode: composerMode,
      permissionMode: permissionMode,
      providers: providers,
      modeModelRoutes: modeModelRoutes,
      roles: roles,
      agents: const [],
    );
  }

  @override
  bool operator ==(Object other) {
    return identical(this, other) ||
        other is StudioComposerProjection &&
            _threadEquals(thread, other.thread) &&
            _threadEquals(rootThread, other.rootThread) &&
            syncState == other.syncState &&
            loadError == other.loadError &&
            composer == other.composer &&
            composerMode == other.composerMode &&
            permissionMode == other.permissionMode &&
            _listEqualsBy(providers, other.providers, _providerEquals) &&
            _listEqualsBy(
              modeModelRoutes,
              other.modeModelRoutes,
              _modeRouteEquals,
            ) &&
            _listEqualsBy(roles, other.roles, _roleEquals) &&
            _interactionEquals(activeInteraction, other.activeInteraction) &&
            turn == other.turn &&
            modelRoute == other.modelRoute &&
            workflow == other.workflow &&
            targetDiagnostic == other.targetDiagnostic;
  }

  @override
  int get hashCode => Object.hash(
    _threadHash(thread),
    _threadHash(rootThread),
    syncState,
    loadError,
    composer,
    composerMode,
    permissionMode,
    Object.hashAll(providers.map(_providerHash)),
    Object.hashAll(modeModelRoutes.map(_modeRouteHash)),
    Object.hashAll(roles.map(_roleHash)),
    _interactionHash(activeInteraction),
    turn,
    modelRoute,
    workflow,
    targetDiagnostic,
  );
}

class StudioInteractionProjection {
  const StudioInteractionProjection({
    required this.threadId,
    required this.interaction,
    required this.isBusy,
  });

  factory StudioInteractionProjection.fromState(StudioState state) {
    return StudioInteractionProjection(
      threadId: state.selectedThreadId,
      interaction: state.activeInteraction,
      isBusy: state.isBusy,
    );
  }

  final String? threadId;
  final PendingInteraction? interaction;
  final bool isBusy;

  @override
  bool operator ==(Object other) {
    return identical(this, other) ||
        other is StudioInteractionProjection &&
            threadId == other.threadId &&
            _interactionEquals(interaction, other.interaction) &&
            isBusy == other.isBusy;
  }

  @override
  int get hashCode =>
      Object.hash(threadId, _interactionHash(interaction), isBusy);
}

class StudioStorageProjection {
  const StudioStorageProjection({
    required this.threadId,
    required this.storage,
  });

  factory StudioStorageProjection.fromState(StudioState state) {
    return StudioStorageProjection(
      threadId: state.selectedThreadId,
      storage: state.selectedWorkspace?.storage,
    );
  }

  final String? threadId;
  final ThreadStorageStateView? storage;

  @override
  bool operator ==(Object other) {
    return identical(this, other) ||
        other is StudioStorageProjection &&
            threadId == other.threadId &&
            _storageEquals(storage, other.storage);
  }

  @override
  int get hashCode => Object.hash(threadId, _storageHash(storage));
}

class StudioActivityProjection {
  const StudioActivityProjection({required this.threadId, required this.view});

  final String? threadId;
  final ConversationActivityView? view;

  @override
  bool operator ==(Object other) {
    return identical(this, other) ||
        other is StudioActivityProjection &&
            threadId == other.threadId &&
            _conversationActivityEquals(view, other.view);
  }

  @override
  int get hashCode => Object.hash(threadId, _conversationActivityHash(view));
}

bool _listEqualsBy<T>(
  List<T> left,
  List<T> right,
  bool Function(T left, T right) equals,
) {
  if (left.length != right.length) return false;
  for (var index = 0; index < left.length; index++) {
    if (!equals(left[index], right[index])) return false;
  }
  return true;
}

bool _threadEquals(StudioThread? left, StudioThread? right) {
  if (identical(left, right)) return true;
  if (left == null || right == null) return false;
  return left.id == right.id &&
      left.projectId == right.projectId &&
      left.title == right.title &&
      left.mode == right.mode &&
      left.createdAt == right.createdAt &&
      left.updatedAt == right.updatedAt &&
      left.lastUserMessageAt == right.lastUserMessageAt &&
      left.parentThreadId == right.parentThreadId &&
      left.rootThreadId == right.rootThreadId &&
      left.agentPath == right.agentPath &&
      left.role == right.role &&
      left.status == right.status &&
      left.archived == right.archived &&
      left.workspaceMode == right.workspaceMode &&
      left.workspacePath == right.workspacePath;
}

bool _projectEquals(StudioProject? left, StudioProject? right) {
  if (identical(left, right)) return true;
  if (left == null || right == null) return false;
  return left.id == right.id &&
      left.name == right.name &&
      left.path == right.path &&
      left.sshAlias == right.sshAlias;
}

int? _projectHash(StudioProject? project) {
  if (project == null) return null;
  return Object.hash(project.id, project.name, project.path, project.sshAlias);
}

int? _threadHash(StudioThread? thread) {
  if (thread == null) return null;
  return Object.hash(
    thread.id,
    thread.projectId,
    thread.title,
    thread.mode,
    thread.createdAt,
    thread.updatedAt,
    thread.lastUserMessageAt,
    thread.parentThreadId,
    thread.rootThreadId,
    thread.agentPath,
    thread.role,
    thread.status,
    thread.archived,
    thread.workspaceMode,
    thread.workspacePath,
  );
}

bool _roleEquals(RoleSettingsView left, RoleSettingsView right) =>
    left.key == right.key &&
    left.providerId == right.providerId &&
    left.model == right.model &&
    left.effort == right.effort;

int _roleHash(RoleSettingsView role) =>
    Object.hash(role.key, role.providerId, role.model, role.effort);

bool _modeRouteEquals(ModeModelRouteView left, ModeModelRouteView right) =>
    left.modeId == right.modeId &&
    left.providerId == right.providerId &&
    left.model == right.model &&
    left.effort == right.effort;

int _modeRouteHash(ModeModelRouteView route) =>
    Object.hash(route.modeId, route.providerId, route.model, route.effort);

bool _providerEquals(ProviderSettingsView left, ProviderSettingsView right) {
  return left.pricingEnabled == right.pricingEnabled &&
      _catalogStatusEquals(left.modelCatalog, right.modelCatalog) &&
      left.id == right.id &&
      left.templateKind == right.templateKind &&
      left.name == right.name &&
      left.subtitle == right.subtitle &&
      left.baseUrl == right.baseUrl &&
      left.bearerToken == right.bearerToken &&
      left.hasBearerToken == right.hasBearerToken &&
      left.credentialRequired == right.credentialRequired &&
      left.defaultModel == right.defaultModel &&
      _listEqualsBy(left.models, right.models, _providerModelEquals) &&
      _listEqualsBy(
        left.defaultModels,
        right.defaultModels,
        _providerModelEquals,
      ) &&
      _listEqualsBy(
        left.customModels,
        right.customModels,
        _providerModelEquals,
      ) &&
      _stringMapEquals(left.modelConnectionModes, right.modelConnectionModes) &&
      _autoCompactMapEquals(left.autoCompactLimits, right.autoCompactLimits) &&
      left.status == right.status &&
      left.usageLabel == right.usageLabel &&
      left.modelCount == right.modelCount &&
      left.updatedAt == right.updatedAt &&
      left.catalogId == right.catalogId &&
      left.credentialLabel == right.credentialLabel &&
      left.credentialEnv == right.credentialEnv &&
      left.capabilitySource == right.capabilitySource &&
      left.hostedWebSearch == right.hostedWebSearch &&
      left.hostedWebSearchDialect == right.hostedWebSearchDialect &&
      left.standaloneWebSearch == right.standaloneWebSearch &&
      left.promptCacheDialect == right.promptCacheDialect &&
      left.responsesProgrammaticToolCalling ==
          right.responsesProgrammaticToolCalling &&
      left.iconKey == right.iconKey;
}

int _providerHash(ProviderSettingsView provider) => Object.hashAll([
  provider.pricingEnabled,
  _catalogStatusHash(provider.modelCatalog),
  provider.id,
  provider.templateKind,
  provider.name,
  provider.subtitle,
  provider.baseUrl,
  provider.bearerToken,
  provider.hasBearerToken,
  provider.credentialRequired,
  provider.defaultModel,
  Object.hashAll(provider.models.map(_providerModelHash)),
  Object.hashAll(provider.defaultModels.map(_providerModelHash)),
  Object.hashAll(provider.customModels.map(_providerModelHash)),
  _stringMapHash(provider.modelConnectionModes),
  _autoCompactMapHash(provider.autoCompactLimits),
  provider.status,
  provider.usageLabel,
  provider.modelCount,
  provider.updatedAt,
  provider.catalogId,
  provider.credentialLabel,
  provider.credentialEnv,
  provider.capabilitySource,
  provider.hostedWebSearch,
  provider.hostedWebSearchDialect,
  provider.standaloneWebSearch,
  provider.promptCacheDialect,
  provider.responsesProgrammaticToolCalling,
  provider.iconKey,
]);

bool _providerModelEquals(ProviderModelView left, ProviderModelView right) =>
    left.slug == right.slug &&
    left.displayName == right.displayName &&
    listEquals(left.reasoningEfforts, right.reasoningEfforts) &&
    left.description == right.description &&
    left.contextWindow == right.contextWindow &&
    left.maxContextWindow == right.maxContextWindow &&
    left.maxOutputTokens == right.maxOutputTokens &&
    _listEqualsBy(
      left.inputCapabilities,
      right.inputCapabilities,
      _inputCapabilityEquals,
    ) &&
    listEquals(left.outputModalities, right.outputModalities) &&
    listEquals(left.capabilities, right.capabilities) &&
    left.reasoningLabel == right.reasoningLabel &&
    left.defaultReasoningEffort == right.defaultReasoningEffort &&
    left.currency == right.currency &&
    _listEqualsBy(left.priceTiers, right.priceTiers, _priceTierEquals) &&
    left.baseInstructions == right.baseInstructions &&
    left.wireProtocol == right.wireProtocol &&
    listEquals(left.supportedConnectionModes, right.supportedConnectionModes) &&
    left.defaultConnectionMode == right.defaultConnectionMode &&
    left.connectionMode == right.connectionMode;

int _providerModelHash(ProviderModelView model) => Object.hash(
  model.slug,
  model.displayName,
  Object.hashAll(model.reasoningEfforts),
  model.description,
  model.contextWindow,
  model.maxContextWindow,
  model.maxOutputTokens,
  Object.hashAll(model.inputCapabilities.map(_inputCapabilityHash)),
  Object.hashAll(model.outputModalities),
  Object.hashAll(model.capabilities),
  model.reasoningLabel,
  model.defaultReasoningEffort,
  model.currency,
  Object.hashAll(model.priceTiers.map(_priceTierHash)),
  model.baseInstructions,
  model.wireProtocol,
  Object.hashAll(model.supportedConnectionModes),
  model.defaultConnectionMode,
  model.connectionMode,
);

bool _inputCapabilityEquals(
  ModelInputCapabilityView left,
  ModelInputCapabilityView right,
) =>
    left.modality == right.modality &&
    listEquals(left.sources, right.sources) &&
    left.maxCount == right.maxCount &&
    left.maxBytes == right.maxBytes &&
    left.maxTotalBytes == right.maxTotalBytes &&
    left.maxWidth == right.maxWidth &&
    left.maxHeight == right.maxHeight &&
    listEquals(left.mediaTypes, right.mediaTypes);

int _inputCapabilityHash(ModelInputCapabilityView capability) => Object.hash(
  capability.modality,
  Object.hashAll(capability.sources),
  capability.maxCount,
  capability.maxBytes,
  capability.maxTotalBytes,
  capability.maxWidth,
  capability.maxHeight,
  Object.hashAll(capability.mediaTypes),
);

bool _priceTierEquals(
  ProviderPriceTierView left,
  ProviderPriceTierView right,
) =>
    left.label == right.label &&
    left.input == right.input &&
    left.output == right.output &&
    left.cacheRead == right.cacheRead &&
    left.cacheWrite == right.cacheWrite;

int _priceTierHash(ProviderPriceTierView tier) => Object.hash(
  tier.label,
  tier.input,
  tier.output,
  tier.cacheRead,
  tier.cacheWrite,
);

bool _catalogStatusEquals(
  ModelCatalogStatusView left,
  ModelCatalogStatusView right,
) =>
    left.supported == right.supported &&
    left.source == right.source &&
    left.probing == right.probing &&
    left.lastSuccessAt == right.lastSuccessAt &&
    left.checkedAt == right.checkedAt &&
    left.cacheWarning == right.cacheWarning &&
    left.error?.kind == right.error?.kind &&
    left.error?.httpStatus == right.error?.httpStatus;

int _catalogStatusHash(ModelCatalogStatusView status) => Object.hash(
  status.supported,
  status.source,
  status.probing,
  status.lastSuccessAt,
  status.checkedAt,
  status.cacheWarning,
  status.error?.kind,
  status.error?.httpStatus,
);

bool _autoCompactEquals(
  ProviderModelAutoCompactView left,
  ProviderModelAutoCompactView right,
) =>
    left.slug == right.slug &&
    left.defaultLimit == right.defaultLimit &&
    left.overrideLimit == right.overrideLimit &&
    left.effectiveLimit == right.effectiveLimit &&
    left.safeLimit == right.safeLimit;

int _autoCompactHash(ProviderModelAutoCompactView value) => Object.hash(
  value.slug,
  value.defaultLimit,
  value.overrideLimit,
  value.effectiveLimit,
  value.safeLimit,
);

bool _stringMapEquals(Map<String, String> left, Map<String, String> right) {
  if (left.length != right.length) return false;
  for (final entry in left.entries) {
    if (right[entry.key] != entry.value) return false;
  }
  return true;
}

bool _autoCompactMapEquals(
  Map<String, ProviderModelAutoCompactView> left,
  Map<String, ProviderModelAutoCompactView> right,
) {
  if (left.length != right.length) return false;
  for (final entry in left.entries) {
    final other = right[entry.key];
    if (other == null || !_autoCompactEquals(entry.value, other)) return false;
  }
  return true;
}

int _stringMapHash(Map<String, String> value) =>
    Object.hashAll(_sortedStringEntries(value).map(_stringEntryHash));

int _autoCompactMapHash(Map<String, ProviderModelAutoCompactView> value) =>
    Object.hashAll(_sortedStringEntries(value).map(_autoCompactEntryHash));

List<MapEntry<String, T>> _sortedStringEntries<T>(Map<String, T> value) {
  final entries = value.entries.toList();
  entries.sort((left, right) => left.key.compareTo(right.key));
  return entries;
}

int _stringEntryHash(MapEntry<String, String> entry) =>
    Object.hash(entry.key, entry.value);

int _autoCompactEntryHash(
  MapEntry<String, ProviderModelAutoCompactView> entry,
) => Object.hash(entry.key, _autoCompactHash(entry.value));

bool _providerUsageEquals(ProviderUsageView left, ProviderUsageView right) =>
    left.providerId == right.providerId &&
    left.revision == right.revision &&
    left.updatedAt == right.updatedAt &&
    _providerUsageStateEquals(left.state, right.state);

int _providerUsageHash(ProviderUsageView usage) => Object.hash(
  usage.providerId,
  usage.revision,
  usage.updatedAt,
  _providerUsageStateHash(usage.state),
);

bool _providerUsageStateEquals(
  ProviderUsageStateView left,
  ProviderUsageStateView right,
) {
  if (left.runtimeType != right.runtimeType) return false;
  return switch ((left, right)) {
    (UnsupportedProviderUsageView(), UnsupportedProviderUsageView()) => true,
    (
      MissingCredentialProviderUsageView(message: final leftMessage),
      MissingCredentialProviderUsageView(message: final rightMessage),
    ) =>
      leftMessage == rightMessage,
    (
      FailedProviderUsageView(
        code: final leftCode,
        message: final leftMessage,
        retryable: final leftRetryable,
      ),
      FailedProviderUsageView(
        code: final rightCode,
        message: final rightMessage,
        retryable: final rightRetryable,
      ),
    ) =>
      leftCode == rightCode &&
          leftMessage == rightMessage &&
          leftRetryable == rightRetryable,
    (
      ReadyProviderUsageView(data: final leftData),
      ReadyProviderUsageView(data: final rightData),
    ) =>
      _providerUsageDataEquals(leftData, rightData),
    _ => false,
  };
}

int _providerUsageStateHash(ProviderUsageStateView state) {
  return switch (state) {
    UnsupportedProviderUsageView() => Object.hash(
      UnsupportedProviderUsageView,
      0,
    ),
    MissingCredentialProviderUsageView(message: final message) => Object.hash(
      MissingCredentialProviderUsageView,
      message,
    ),
    FailedProviderUsageView(
      code: final code,
      message: final message,
      retryable: final retryable,
    ) =>
      Object.hash(FailedProviderUsageView, code, message, retryable),
    ReadyProviderUsageView(data: final data) => Object.hash(
      ReadyProviderUsageView,
      _providerUsageDataHash(data),
    ),
  };
}

bool _providerUsageDataEquals(
  ProviderUsageDataView left,
  ProviderUsageDataView right,
) {
  if (left.runtimeType != right.runtimeType) return false;
  return switch ((left, right)) {
    (
      DeepSeekBalanceProviderUsageView(balance: final leftBalance),
      DeepSeekBalanceProviderUsageView(balance: final rightBalance),
    ) =>
      leftBalance.isAvailable == rightBalance.isAvailable &&
          _listEqualsBy(
            leftBalance.balances,
            rightBalance.balances,
            _deepSeekBalanceEquals,
          ),
    (
      ZhipuCodingPlanProviderUsageView(codingPlan: final leftPlan),
      ZhipuCodingPlanProviderUsageView(codingPlan: final rightPlan),
    ) =>
      leftPlan.level == rightPlan.level &&
          _listEqualsBy(leftPlan.limits, rightPlan.limits, _zhipuLimitEquals),
    _ => false,
  };
}

int _providerUsageDataHash(ProviderUsageDataView data) {
  return switch (data) {
    DeepSeekBalanceProviderUsageView(balance: final balance) => Object.hash(
      DeepSeekBalanceProviderUsageView,
      balance.isAvailable,
      Object.hashAll(balance.balances.map(_deepSeekBalanceHash)),
    ),
    ZhipuCodingPlanProviderUsageView(codingPlan: final plan) => Object.hash(
      ZhipuCodingPlanProviderUsageView,
      plan.level,
      Object.hashAll(plan.limits.map(_zhipuLimitHash)),
    ),
  };
}

bool _deepSeekBalanceEquals(
  DeepSeekBalanceInfoView left,
  DeepSeekBalanceInfoView right,
) =>
    left.currency == right.currency &&
    left.totalBalance == right.totalBalance &&
    left.grantedBalance == right.grantedBalance &&
    left.toppedUpBalance == right.toppedUpBalance;

int _deepSeekBalanceHash(DeepSeekBalanceInfoView balance) => Object.hash(
  balance.currency,
  balance.totalBalance,
  balance.grantedBalance,
  balance.toppedUpBalance,
);

bool _zhipuLimitEquals(ZhipuQuotaLimitView left, ZhipuQuotaLimitView right) =>
    left.window == right.window &&
    left.label == right.label &&
    left.percentage == right.percentage &&
    left.currentValue == right.currentValue &&
    left.total == right.total &&
    left.remaining == right.remaining &&
    left.nextResetAt == right.nextResetAt &&
    _listEqualsBy(left.usageDetails, right.usageDetails, _zhipuDetailEquals);

int _zhipuLimitHash(ZhipuQuotaLimitView limit) => Object.hash(
  limit.window,
  limit.label,
  limit.percentage,
  limit.currentValue,
  limit.total,
  limit.remaining,
  limit.nextResetAt,
  Object.hashAll(limit.usageDetails.map(_zhipuDetailHash)),
);

bool _zhipuDetailEquals(
  ZhipuToolUsageDetailView left,
  ZhipuToolUsageDetailView right,
) =>
    left.name == right.name &&
    left.currentValue == right.currentValue &&
    left.total == right.total &&
    left.percentage == right.percentage;

int _zhipuDetailHash(ZhipuToolUsageDetailView detail) => Object.hash(
  detail.name,
  detail.currentValue,
  detail.total,
  detail.percentage,
);

bool _interactionEquals(PendingInteraction? left, PendingInteraction? right) {
  if (identical(left, right)) return true;
  if (left == null || right == null) return false;
  return left.id == right.id &&
      left.threadId == right.threadId &&
      left.turnId == right.turnId &&
      left.kind == right.kind &&
      left.title == right.title &&
      left.body == right.body &&
      _interactionPayloadEquals(left.payload, right.payload);
}

int? _interactionHash(PendingInteraction? interaction) {
  if (interaction == null) return null;
  return Object.hash(
    interaction.id,
    interaction.threadId,
    interaction.turnId,
    interaction.kind,
    interaction.title,
    interaction.body,
    _interactionPayloadHash(interaction.payload),
  );
}

bool _interactionPayloadEquals(
  InteractionPayload left,
  InteractionPayload right,
) {
  if (left.runtimeType != right.runtimeType) return false;
  return switch ((left, right)) {
    (UnknownInteractionPayload(), UnknownInteractionPayload()) => true,
    (
      ToolApprovalInteractionPayload(
        :final toolName,
        :final arguments,
        :final workingDirectory,
        :final parentAgentId,
      ),
      ToolApprovalInteractionPayload(
        toolName: final otherToolName,
        arguments: final otherArguments,
        workingDirectory: final otherWorkingDirectory,
        parentAgentId: final otherParentAgentId,
      ),
    ) =>
      toolName == otherToolName &&
          _deepValueEquals(arguments, otherArguments) &&
          workingDirectory == otherWorkingDirectory &&
          parentAgentId == otherParentAgentId,
    (
      UserInputInteractionPayload(:final questions),
      UserInputInteractionPayload(questions: final otherQuestions),
    ) =>
      _listEqualsBy(questions, otherQuestions, _questionEquals),
    _ => false,
  };
}

int _interactionPayloadHash(InteractionPayload payload) {
  return switch (payload) {
    UnknownInteractionPayload() => 0,
    ToolApprovalInteractionPayload(
      :final toolName,
      :final arguments,
      :final workingDirectory,
      :final parentAgentId,
    ) =>
      Object.hash(
        toolName,
        _deepValueHash(arguments),
        workingDirectory,
        parentAgentId,
      ),
    UserInputInteractionPayload(:final questions) => Object.hashAll(
      questions.map(_questionHash),
    ),
  };
}

bool _questionEquals(UserQuestionView left, UserQuestionView right) =>
    left.id == right.id &&
    left.header == right.header &&
    left.question == right.question &&
    left.isOther == right.isOther &&
    left.isSecret == right.isSecret &&
    _listEqualsBy(left.options, right.options, _questionOptionEquals);

int _questionHash(UserQuestionView question) => Object.hash(
  question.id,
  question.header,
  question.question,
  question.isOther,
  question.isSecret,
  Object.hashAll(question.options.map(_questionOptionHash)),
);

bool _questionOptionEquals(
  UserQuestionOptionView left,
  UserQuestionOptionView right,
) => left.label == right.label && left.description == right.description;

int _questionOptionHash(UserQuestionOptionView option) =>
    Object.hash(option.label, option.description);

bool _deepValueEquals(Object? left, Object? right) {
  if (identical(left, right)) return true;
  if (left is Map && right is Map) {
    final leftEntries = _sortedJsonMapEntries(left);
    final rightEntries = _sortedJsonMapEntries(right);
    if (leftEntries.length != rightEntries.length) return false;
    for (var index = 0; index < leftEntries.length; index++) {
      final leftEntry = leftEntries[index];
      final rightEntry = rightEntries[index];
      if (leftEntry.key != rightEntry.key ||
          !_deepValueEquals(leftEntry.value, rightEntry.value)) {
        return false;
      }
    }
    return true;
  }
  if (left is Iterable && right is Iterable) {
    final leftValues = left.toList();
    final rightValues = right.toList();
    if (leftValues.length != rightValues.length) return false;
    for (var index = 0; index < leftValues.length; index++) {
      if (!_deepValueEquals(leftValues[index], rightValues[index])) {
        return false;
      }
    }
    return true;
  }
  return left == right;
}

int _deepValueHash(Object? value) {
  if (value is Map) {
    return Object.hashAll(
      _sortedJsonMapEntries(value)
          .map((entry) => Object.hash(entry.key, _deepValueHash(entry.value))),
    );
  }
  if (value is Iterable) return Object.hashAll(value.map(_deepValueHash));
  return value.hashCode;
}

/// Interaction arguments are decoded from typed JSON object values. JSON
/// object keys are strings; rejecting any other key type keeps this
/// equality/hash contract explicit instead of ordering equal Dart keys by
/// runtime type, `toString`, or their hash code.
List<MapEntry<String, Object?>> _sortedJsonMapEntries(Map value) {
  final entries = <MapEntry<String, Object?>>[];
  for (final entry in value.entries) {
    if (entry.key is! String) {
      throw StateError(
        'Typed JSON object keys must be String, got ${entry.key.runtimeType}.',
      );
    }
    entries.add(MapEntry(entry.key as String, entry.value));
  }
  entries.sort((left, right) => left.key.compareTo(right.key));
  return entries;
}

bool _storageEquals(
  ThreadStorageStateView? left,
  ThreadStorageStateView? right,
) {
  if (identical(left, right)) return true;
  if (left == null || right == null) return false;
  return left.fault == right.fault &&
      left.faultGeneration == right.faultGeneration &&
      left.acceptedSequence == right.acceptedSequence &&
      left.durableSequence == right.durableSequence &&
      left.execution == right.execution &&
      left.pressurePaused == right.pressurePaused &&
      left.resumeRequired == right.resumeRequired &&
      left.canResume == right.canResume &&
      left.lastError == right.lastError;
}

int? _storageHash(ThreadStorageStateView? storage) {
  if (storage == null) return null;
  return Object.hash(
    storage.fault,
    storage.faultGeneration,
    storage.acceptedSequence,
    storage.durableSequence,
    storage.execution,
    storage.pressurePaused,
    storage.resumeRequired,
    storage.canResume,
    storage.lastError,
  );
}

bool _conversationActivityEquals(
  ConversationActivityView? left,
  ConversationActivityView? right,
) {
  if (identical(left, right)) return true;
  if (left == null || right == null) return false;
  return left.identity == right.identity &&
      left.kind == right.kind &&
      left.summary == right.summary &&
      left.activeToolCount == right.activeToolCount &&
      left.backgroundToolCount == right.backgroundToolCount &&
      left.errorMessage == right.errorMessage &&
      _listEqualsBy(left.details, right.details, _activityDetailEquals) &&
      left.detailsLoading == right.detailsLoading &&
      left.detailsError == right.detailsError &&
      left.expandable == right.expandable &&
      _storageEquals(left.storage, right.storage);
}

int? _conversationActivityHash(ConversationActivityView? view) {
  if (view == null) return null;
  return Object.hash(
    view.identity,
    view.kind,
    view.summary,
    view.activeToolCount,
    view.backgroundToolCount,
    view.errorMessage,
    Object.hashAll(view.details.map(_activityDetailHash)),
    view.detailsLoading,
    view.detailsError,
    view.expandable,
    _storageHash(view.storage),
  );
}

bool _activityDetailEquals(
  ConversationActivityDetail left,
  ConversationActivityDetail right,
) =>
    left.id == right.id && left.title == right.title && left.body == right.body;

int _activityDetailHash(ConversationActivityDetail detail) =>
    Object.hash(detail.id, detail.title, detail.body);
