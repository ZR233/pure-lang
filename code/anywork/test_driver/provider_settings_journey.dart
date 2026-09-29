import 'dart:convert';
import 'dart:io';

import 'package:flutter_driver/flutter_driver.dart';

import 'flutter_driver_session.dart';

/// Manual Driver journey for the provider context-compaction threshold UI.
///
/// Run against a native GUI started by `cargo xtask manual-gui` (default `gui`
/// scenario), which prints the VM service URL. This journey never shuts the
/// application down; the operator types `done` in the manual-gui console to
/// capture evidence and shut down.
///
/// Usage:
///   dart run test_driver/provider_settings_journey.dart VM_URL OUTPUT_DIR
///
/// Covered: explicit fixture provider edit -> save; model/provider isolation;
/// input -> save; slider -> save; invalid input must not save; restore default;
/// custom-model field edit keeps its override; screenshots and snapshot JSON
/// per stage.
Future<void> main(List<String> args) async {
  if (args.length != 2) {
    stderr.writeln('usage: provider_settings_journey.dart VM_URL OUTPUT_DIR');
    exitCode = 64;
    return;
  }
  final output = Directory(args[1]);
  await output.create(recursive: true);
  final driver = await FlutterDriverSession.connect(vmServiceUrl: args[0]);
  try {
    final recorder = _Recorder(output, driver);
    await recorder.stage('connected');
    final baseline = await recorder.readSnapshot();
    final baselineIds = _providerIds(baseline);
    if (baselineIds.isEmpty) {
      throw StateError('no provider present to exercise the threshold editor');
    }
    await recorder.capture('baseline', baseline);

    // Navigate once into the settings providers tab; every later step works
    // from the list the shell shows after a successful save.
    await recorder.openProviders();

    // 0. Explicit fixture provider (no preset id) must open, edit and save.
    final fixtureId = baselineIds.first;
    final fixtureModel = _models(_provider(baseline, fixtureId)).firstOrNull;
    if (fixtureModel == null) {
      throw StateError('initial fixture provider exposes no model');
    }
    await recorder.openProviderEditor(fixtureId);
    await recorder.expandCustomAdvanced(0);
    await recorder.setCompactInput(fixtureId, fixtureModel, '18000');
    await recorder.capture('fixture-edited', await recorder.readSnapshot());
    await recorder.saveExpectEditorClosed();
    final afterFixture = await recorder.readSnapshot();
    _requireOverride(
      afterFixture,
      fixtureId,
      fixtureModel,
      expected: 18000,
      what: 'explicit fixture input then save',
    );
    await recorder.capture('fixture-saved', afterFixture);
    // Isolation reference: fixture now carries the override; later steps must
    // not change it again.
    final reference = afterFixture;

    // 1. A preset-backed provider exposes bundled models in the visible section.
    await recorder.tapAddProvider();
    await recorder.capture('create-provider', await recorder.readSnapshot());
    await recorder.saveExpectEditorClosed();
    final addedSnapshot = await recorder.readSnapshot();
    final addedId = _providerIds(addedSnapshot)
        .difference(baselineIds)
        .firstOrNull;
    if (addedId == null) {
      throw StateError('provider was not created by the add + save flow');
    }
    final added = _provider(addedSnapshot, addedId);
    final bundledModel = _models(added).firstOrNull;
    await recorder.capture('provider-created', addedSnapshot);
    if (bundledModel == null) {
      throw StateError('created provider exposes no model to configure');
    }
    _requireProviderUnchanged(addedSnapshot, reference, baselineIds);

    // 2. Input -> save: exact positive integer override is persisted.
    await recorder.openProviderEditor(addedId);
    await recorder.setCompactInput(addedId, bundledModel, '20000');
    await recorder.capture('input-edited', await recorder.readSnapshot());
    await recorder.saveExpectEditorClosed();
    final afterInput = await recorder.readSnapshot();
    _requireOverride(
      afterInput,
      addedId,
      bundledModel,
      expected: 20000,
      what: 'input then save',
    );
    _requireProviderUnchanged(afterInput, reference, baselineIds);
    await recorder.capture('input-saved', afterInput);

    // 3. Slider -> save: dragging the slider commits a different override.
    await recorder.openProviderEditor(addedId);
    await recorder.moveCompactSlider(addedId, bundledModel);
    await recorder.capture('slider-edited', await recorder.readSnapshot());
    await recorder.saveExpectEditorClosed();
    final afterSlider = await recorder.readSnapshot();
    final sliderOverride = _limit(
      afterSlider,
      addedId,
      bundledModel,
    )['override'];
    if (sliderOverride is! int || sliderOverride == 20000) {
      throw StateError(
        'slider move did not persist a new override: $sliderOverride',
      );
    }
    await recorder.capture('slider-saved', afterSlider);

    // 4. Invalid input must block save and keep the draft open.
    await recorder.openProviderEditor(addedId);
    await recorder.setCompactInput(addedId, bundledModel, 'not-a-number');
    await recorder.capture('invalid-edited', await recorder.readSnapshot());
    await recorder.saveExpectEditorOpen();
    final afterInvalid = await recorder.readSnapshot();
    if (_limit(afterInvalid, addedId, bundledModel)['override'] !=
        sliderOverride) {
      throw StateError('invalid input changed the persisted override');
    }
    await recorder.capture('invalid-blocked', afterInvalid);

    // 5. Restore default clears the override (and any invalid input).
    await recorder.tapCompactReset(addedId, bundledModel);
    await recorder.saveExpectEditorClosed();
    final afterReset = await recorder.readSnapshot();
    if (_limit(afterReset, addedId, bundledModel)['override'] != null) {
      throw StateError('restore default did not clear the override');
    }
    _requireProviderUnchanged(afterReset, reference, baselineIds);
    await recorder.capture('reset-saved', afterReset);

    // 6. Custom model: a field edit must keep that model's override.
    final existing = _models(_provider(afterReset, addedId));
    final customSlug = _nextCustomSlug(existing);
    await recorder.openProviderEditor(addedId);
    await recorder.tapAddCustomModel();
    await recorder.expandCustomAdvanced(0);
    await recorder.setCompactInput(addedId, customSlug, '30000');
    await recorder.setCustomDisplayName(0, 'Journey Custom');
    await recorder.capture('custom-edited', await recorder.readSnapshot());
    await recorder.saveExpectEditorClosed();
    final afterCustom = await recorder.readSnapshot();
    _requireOverride(
      afterCustom,
      addedId,
      customSlug,
      expected: 30000,
      what: 'custom model field edit preserves override',
    );
    if (_limit(afterCustom, addedId, bundledModel)['override'] != null) {
      throw StateError('custom model edit leaked into the bundled model');
    }
    _requireProviderUnchanged(afterCustom, reference, baselineIds);
    await recorder.capture('custom-saved', afterCustom);

    await recorder.writeSummary({
      'fixtureProviderId': fixtureId,
      'fixtureModel': fixtureModel,
      'addedProviderId': addedId,
      'bundledModel': bundledModel,
      'customModel': customSlug,
      'inputOverride': 20000,
      'sliderOverride': sliderOverride,
    });
    await recorder.stage('completed');
    stdout.writeln(
      'Provider settings journey completed; human verdict pending.',
    );
  } finally {
    try {
      await driver.close().timeout(const Duration(seconds: 5));
    } on Object {
      // Closing the observation connection must not mask a journey failure.
    }
  }
}

class _Recorder {
  _Recorder(this.output, this.driver);

  final Directory output;
  final FlutterDriverSession driver;
  int _captureIndex = 0;

  static const _scrollStep = -220.0;
  static const _scrollTimeout = Duration(seconds: 45);

  Future<void> stage(String name) =>
      File('${output.path}/provider-journey-stage.txt').writeAsString(name);

  Future<Map<String, dynamic>> readSnapshot() => driver.readSnapshot();

  Future<void> capture(String name, Map<String, dynamic> snapshot) async {
    final ordinal = _captureIndex++;
    await File('${output.path}/snapshot-$ordinal-$name.json').writeAsString(
      '${const JsonEncoder.withIndent('  ').convert(snapshot)}\n',
    );
    await File('${output.path}/screenshot-$ordinal-$name.png')
        .writeAsBytes(await driver.screenshot());
  }

  Future<void> writeSummary(Map<String, Object?> summary) => File(
    '${output.path}/provider-journey-summary.json',
  ).writeAsString('${const JsonEncoder.withIndent('  ').convert(summary)}\n');

  Future<void> openProviders() async {
    await driver.waitFor(
      find.byValueKey('settings-open'),
      timeout: const Duration(seconds: 60),
    );
    await driver.tap(find.byValueKey('settings-open'));
    await driver.waitFor(find.byValueKey('settings-page'));
    await driver.tap(find.byValueKey('settings-tab-providers'));
    await driver.waitFor(find.byValueKey('provider-add'));
  }

  Future<void> tapAddProvider() async {
    await driver.tap(find.byValueKey('provider-add'));
    await driver.waitFor(find.byValueKey('provider-save'));
  }

  Future<void> openProviderEditor(String providerId) async {
    await driver.waitFor(find.byValueKey('provider-add'));
    await driver.tap(find.byValueKey('provider-row-$providerId'));
    await driver.waitFor(find.byValueKey('provider-edit'));
    await driver.tap(find.byValueKey('provider-edit'));
    await driver.waitFor(find.byValueKey('provider-save'));
  }

  Future<void> saveExpectEditorClosed() async {
    await driver.tap(find.byValueKey('provider-save'));
    final deadline = DateTime.now().add(const Duration(seconds: 40));
    while (DateTime.now().isBefore(deadline)) {
      try {
        await driver.waitForAbsent(
          find.byValueKey('provider-save'),
          timeout: const Duration(seconds: 2),
        );
        return;
      } on Object {
        // Still open; a save error keeps the draft for correction.
      }
      await Future<void>.delayed(const Duration(milliseconds: 300));
    }
    throw StateError('provider editor did not close after save');
  }

  Future<void> saveExpectEditorOpen() async {
    await driver.tap(find.byValueKey('provider-save'));
    await Future<void>.delayed(const Duration(seconds: 2));
    await driver.waitFor(
      find.byValueKey('provider-save'),
      timeout: const Duration(seconds: 15),
    );
  }

  /// Drags the editor list downward ([_scrollStep] is negative) to reveal later
  /// content such as the custom-model section at the bottom.
  Future<void> _scrollTo(SerializableFinder finder) async {
    await driver.scrollUntilVisible(
      find.byValueKey('provider-editor-scroll'),
      finder,
      dyScroll: _scrollStep,
      timeout: _scrollTimeout,
    );
  }

  Future<void> setCompactInput(
    String providerId,
    String modelSlug,
    String value,
  ) async {
    final finder = find.byValueKey(
      'provider-$providerId-model-$modelSlug-auto-compact-input',
    );
    await _scrollTo(finder);
    await driver.tap(finder);
    await driver.enterText(value);
    await Future<void>.delayed(const Duration(milliseconds: 400));
  }

  Future<void> moveCompactSlider(String providerId, String modelSlug) async {
    final finder = find.byValueKey(
      'provider-$providerId-model-$modelSlug-auto-compact-slider',
    );
    await _scrollTo(finder);
    await driver.rawTap(finder);
    await Future<void>.delayed(const Duration(milliseconds: 400));
  }

  Future<void> tapCompactReset(String providerId, String modelSlug) async {
    final finder = find.byValueKey(
      'provider-$providerId-model-$modelSlug-auto-compact-reset',
    );
    await _scrollTo(finder);
    await driver.tap(finder);
    await Future<void>.delayed(const Duration(milliseconds: 300));
  }

  Future<void> tapAddCustomModel() async {
    final finder = find.byValueKey('provider-model-add');
    await _scrollTo(finder);
    await driver.tap(finder);
    await driver.waitFor(find.byValueKey('provider-model-0-id'));
  }

  Future<void> expandCustomAdvanced(int index) async {
    final finder = find.byValueKey('provider-model-$index-advanced');
    await _scrollTo(finder);
    await driver.tap(finder);
    await driver.waitFor(find.byValueKey('provider-model-$index-display-name'));
  }

  Future<void> setCustomDisplayName(int index, String value) async {
    final finder = find.byValueKey('provider-model-$index-display-name');
    await _scrollTo(finder);
    await driver.tap(finder);
    await driver.enterText(value);
    await Future<void>.delayed(const Duration(milliseconds: 300));
  }
}

List<Map<String, dynamic>> _providers(Map<String, dynamic> snapshot) {
  final settings = snapshot['settings'];
  final providers = settings is Map ? settings['providers'] : null;
  if (providers is! List) return const [];
  return [
    for (final provider in providers)
      if (provider is Map) provider.cast<String, dynamic>(),
  ];
}

Set<String> _providerIds(Map<String, dynamic> snapshot) => {
  for (final provider in _providers(snapshot)) '${provider['id']}',
};

Map<String, dynamic> _provider(Map<String, dynamic> snapshot, String id) {
  for (final provider in _providers(snapshot)) {
    if (provider['id'] == id) return provider;
  }
  throw StateError('provider $id missing from snapshot');
}

List<String> _models(Map<String, dynamic> provider) {
  final models = provider['models'];
  if (models is! List) return const [];
  return [for (final model in models) '$model'];
}

Map<String, dynamic> _limit(
  Map<String, dynamic> snapshot,
  String providerId,
  String modelSlug,
) {
  final limits = _provider(snapshot, providerId)['autoCompactLimits'];
  if (limits is List) {
    for (final limit in limits) {
      if (limit is Map && limit['slug'] == modelSlug) {
        return limit.cast<String, dynamic>();
      }
    }
  }
  throw StateError('model $modelSlug missing autoCompactLimits in snapshot');
}

void _requireOverride(
  Map<String, dynamic> snapshot,
  String providerId,
  String modelSlug, {
  required int expected,
  required String what,
}) {
  final override = _limit(snapshot, providerId, modelSlug)['override'];
  if (override != expected) {
    throw StateError('$what: expected override $expected, saw $override');
  }
}

/// Fails the journey if any provider present in [reference] changed.
void _requireProviderUnchanged(
  Map<String, dynamic> current,
  Map<String, dynamic> reference,
  Set<String> referenceIds,
) {
  for (final id in referenceIds) {
    final before = _provider(reference, id);
    final after = _provider(current, id);
    if (jsonEncode(before['autoCompactLimits']) !=
        jsonEncode(after['autoCompactLimits'])) {
      throw StateError('provider $id thresholds changed unexpectedly');
    }
  }
}

/// Mirrors the editor's `_addCustomModel` slug dedup for the added model.
String _nextCustomSlug(List<String> existing) {
  final taken = existing.toSet();
  var slug = 'custom-model';
  for (var index = 2; taken.contains(slug); index += 1) {
    slug = 'custom-model-$index';
  }
  return slug;
}
