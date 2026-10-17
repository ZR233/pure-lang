import '../../models.dart';
import 'stores/domain_sync.dart';

/// Display intent deliberately has no resource revision. A pending selection
/// must never be mistaken for an observed Thread model-route resource.
final class ModelRouteDisplay {
  const ModelRouteDisplay({
    required this.providerId,
    required this.model,
    required this.effort,
    required this.pending,
    required this.available,
    this.unavailableReason,
  });

  final String providerId;
  final String model;
  final String? effort;
  final bool pending;
  final bool available;
  final String? unavailableReason;

  @override
  bool operator ==(Object other) =>
      other is ModelRouteDisplay &&
      providerId == other.providerId &&
      model == other.model &&
      effort == other.effort &&
      pending == other.pending &&
      available == other.available &&
      unavailableReason == other.unavailableReason;

  @override
  int get hashCode => Object.hash(
    providerId,
    model,
    effort,
    pending,
    available,
    unavailableReason,
  );
}

final class StartRouteQuery {
  const StartRouteQuery({
    required this.canonical,
    required this.lane,
    required this.display,
    required this.pending,
    required this.failure,
  });

  final ModeModelRouteView? canonical;
  final ModeModelRouteView? lane;
  final ModeModelRouteView? display;
  final bool pending;
  final String? failure;
}

final class RoleRouteQuery {
  const RoleRouteQuery({
    required this.providerId,
    required this.model,
    required this.effort,
    required this.descriptor,
    required this.hasConfiguredRoute,
  });

  final String providerId;
  final String model;
  final String? effort;
  final ProviderModelView? descriptor;
  final bool hasConfiguredRoute;

  bool get unavailable => hasConfiguredRoute && descriptor == null;
}

final class ThreadStatusQuery {
  const ThreadStatusQuery({required this.thread, required this.runtime});
  final StudioThread? thread;
  final ThreadRuntimeView? runtime;
}

/// Route resolution shared by Client consumers. These queries preserve the
/// existing mode/default inheritance rules without publishing invented facts.
abstract final class ModelRouteQueries {
  static ModeModelRouteView? modeRoute(
    List<ModeModelRouteView> routes,
    ThreadModeId mode,
  ) =>
      routes.where((route) => route.modeId == mode).firstOrNull ??
      routes.where((route) => route.modeId == ThreadModeId.simple).firstOrNull;

  static ProviderModelView? model(
    List<ProviderSettingsView> providers,
    String providerId,
    String modelId,
  ) {
    final provider = providers
        .where((candidate) => candidate.id == providerId)
        .firstOrNull;
    final models = provider?.models;
    return models?.where((candidate) => candidate.slug == modelId).firstOrNull;
  }

  /// A model selection keeps a supported effort, then uses the declared model
  /// default when it is still a candidate, then the first supported effort.
  static String? effortAfterModelSelection(
    ProviderModelView? model,
    String? candidate,
  ) {
    if (model?.reasoningEfforts.contains(candidate) == true) return candidate;
    final declared = model?.defaultReasoningEffort;
    if (declared != null &&
        declared.isNotEmpty &&
        model!.reasoningEfforts.contains(declared)) {
      return declared;
    }
    return model?.reasoningEfforts.firstOrNull;
  }

  /// New Agent-profile drafts previously admitted only a supported default.
  static String? agentProfileEffort(
    ProviderModelView? model,
    String? candidate,
  ) {
    if (model?.reasoningEfforts.contains(candidate) == true) return candidate;
    final declared = model?.defaultReasoningEffort;
    if (declared != null &&
        declared.isNotEmpty &&
        model!.reasoningEfforts.contains(declared)) {
      return declared;
    }
    return model?.reasoningEfforts.firstOrNull;
  }

  static StartRouteQuery startRoute({
    required List<ModeModelRouteView> routes,
    required Map<String, CommandStatus> commands,
    required StartSessionDraft draft,
    required ThreadModeId mode,
  }) {
    final canonical = modeRoute(routes, mode);
    final model = commands['new-thread-route:model:${mode.id}'];
    final effort = commands['new-thread-route:effort:${mode.id}'];
    var lane = canonical;
    if (model?.isPending == true &&
        model!.target.providerId != null &&
        model.target.model != null) {
      lane = ModeModelRouteView(
        modeId: mode,
        providerId: model.target.providerId!,
        model: model.target.model!,
        effort: model.target.effort ?? canonical?.effort ?? '',
      );
    } else if (effort?.isPending == true && canonical != null) {
      lane = ModeModelRouteView(
        modeId: mode,
        providerId: canonical.providerId,
        model: canonical.model,
        effort: effort!.target.effort ?? canonical.effort,
      );
    }
    final failure = [model, effort]
        .where((status) => status?.phase == CommandPhase.failed)
        .map((status) => status?.error?.message)
        .firstOrNull;
    return StartRouteQuery(
      canonical: canonical,
      lane: lane,
      display: overlayStartSessionRoute(
        mode: mode,
        laneRoute: lane,
        modelIntent: draft.modelIntentFor(mode),
        effortIntent: draft.effortIntentFor(mode),
      ),
      pending: model?.isPending == true || effort?.isPending == true,
      failure: failure,
    );
  }

  static ModelRouteDisplay? threadRoute({
    required ThreadModelRouteView? canonical,
    required CommandStatus? command,
    required List<ProviderSettingsView> providers,
  }) {
    final target = command?.isPending == true ? command?.target : null;
    final pending = target?.providerId != null && target?.model != null;
    final providerId = pending ? target!.providerId : canonical?.providerId;
    final modelId = pending ? target!.model : canonical?.model;
    if (providerId == null || modelId == null) return null;
    return ModelRouteDisplay(
      providerId: providerId,
      model: modelId,
      effort: pending ? target!.effort ?? canonical?.effort : canonical?.effort,
      pending: pending,
      available: pending
          ? model(providers, providerId, modelId) != null
          : canonical!.available,
      unavailableReason: pending ? null : canonical?.unavailableReason,
    );
  }

  static RoleRouteQuery roleRoute({
    required List<ProviderSettingsView> providers,
    required List<RoleSettingsView> roles,
    required String role,
  }) {
    final configured = roles.where((value) => value.key == role).firstOrNull;
    final hasRoute =
        configured != null &&
        (configured.providerId.isNotEmpty || configured.model.isNotEmpty);
    if (hasRoute) {
      return RoleRouteQuery(
        providerId: configured.providerId,
        model: configured.model,
        effort: configured.effort.isEmpty ? null : configured.effort,
        descriptor: model(providers, configured.providerId, configured.model),
        hasConfiguredRoute: true,
      );
    }
    for (final provider in providers) {
      final descriptor = provider.models
          .where((candidate) => candidate.slug.isNotEmpty)
          .firstOrNull;
      if (descriptor == null) continue;
      return RoleRouteQuery(
        providerId: provider.id,
        model: descriptor.slug,
        effort: effortAfterModelSelection(descriptor, null),
        descriptor: descriptor,
        hasConfiguredRoute: false,
      );
    }
    return const RoleRouteQuery(
      providerId: '',
      model: '',
      effort: null,
      descriptor: null,
      hasConfiguredRoute: false,
    );
  }

  static RoleRouteQuery newAgentProfileRoute(
    List<ProviderSettingsView> providers,
  ) {
    final provider = providers.firstOrNull;
    final descriptor = provider?.models.firstOrNull;
    return RoleRouteQuery(
      providerId: provider?.id ?? '',
      model: descriptor?.slug ?? '',
      effort: agentProfileEffort(descriptor, null),
      descriptor: descriptor,
      hasConfiguredRoute: false,
    );
  }

  static StudioThread? threadIdentity({
    required String threadId,
    required List<StudioThread> directory,
  }) => directory.where((thread) => thread.id == threadId).firstOrNull;

  static ThreadStatusQuery threadStatus({
    required String threadId,
    required List<StudioThread> directory,
    required ThreadRuntimeView? runtime,
  }) => ThreadStatusQuery(
    thread: threadIdentity(threadId: threadId, directory: directory),
    runtime: runtime,
  );

  /// A partial live sample is unknown; it cannot borrow the previous Turn's
  /// missing half. All status fields are resolved from this same runtime frame.
  static double? throughput(ThreadRuntimeView runtime) {
    final live = runtime.liveUsage;
    if (live == null) return runtime.turnTokensPerSecond;
    final tokens = live.completionTokens;
    final millis = live.decodeMillis;
    return tokens != null && tokens > 0 && millis != null && millis > 0
        ? tokens * 1000 / millis
        : null;
  }

  static bool? requiresFunctionCalling(
    ThreadModeCatalogView? catalog,
    ThreadModeId mode,
  ) => catalog?.modes
      .where((descriptor) => descriptor.id == mode.id)
      .firstOrNull
      ?.hasWorkflow;

  static String defaultModelAfterRemoval({
    required String current,
    required String removed,
    required List<ProviderModelView> remaining,
  }) => current == removed ? remaining.firstOrNull?.slug ?? '' : current;

  static String defaultModelForCustomProvider(ProviderSettingsView provider) =>
      provider.customModels.firstOrNull?.slug ?? '';

  static String defaultModelAfterAddition(String current, String added) =>
      current.isEmpty ? added : current;

  static String? providerAfterRemoval({
    required List<ProviderSettingsView> providers,
    required String? currentDefault,
    required String removed,
  }) => currentDefault == removed
      ? providers.where((provider) => provider.id != removed).firstOrNull?.id
      : currentDefault;
}
