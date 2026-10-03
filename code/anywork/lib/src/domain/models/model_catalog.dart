enum ModelCatalogSource { defaultDefinition, cached, online }

enum ModelCatalogFailure {
  unsupported,
  configuration,
  timeout,
  transport,
  http,
  tooLarge,
  protocol,
  cacheIdentity,
  unexpectedNotModified,
  cacheWrite,
  closing,
  stale,
}

enum ModelCatalogCacheWarning { read, schema, identity, declaration }

class ModelCatalogErrorView {
  const ModelCatalogErrorView(this.kind, {this.httpStatus});
  final ModelCatalogFailure kind;
  final int? httpStatus;
}

class ModelCatalogStatusView {
  const ModelCatalogStatusView({
    this.supported = false,
    this.source = ModelCatalogSource.defaultDefinition,
    this.probing = false,
    this.lastSuccessAt,
    this.checkedAt,
    this.error,
    this.cacheWarning,
  });
  final bool supported;
  final ModelCatalogSource source;
  final bool probing;
  final int? lastSuccessAt;
  final int? checkedAt;
  final ModelCatalogErrorView? error;
  final ModelCatalogCacheWarning? cacheWarning;
}
