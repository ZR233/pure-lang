import 'dart:convert';
import 'dart:ffi';
import 'dart:io';

/// Driver-side window control for the native GUI running on this host.
///
/// There is no product window-management API, so acceptance resizes the GUI
/// window from the driver script process. The target is provable in two ways:
/// the display is the GUI's own display, read from the GUI process through
/// `/proc/<pid>/environ` for the pid the Flutter Driver extension reports, and
/// the window is matched by `_NET_WM_PID` rather than by a title or a WM_CLASS.
/// Every `xprop`, `xwininfo` and Xlib call is pointed at that display.
///
/// A missing, unreadable or non-X11 display, a missing property, zero or
/// ambiguous matches, or unreadable geometry is refused with a reason plus the
/// sanitized command outcomes, so the journey records pending instead of a pass.
///
/// A GUI process can own more than one top-level window: GTK maps tiny hidden
/// helper windows (observed 10x10) that inherit the same `_NET_WM_PID`. The
/// target is therefore the *unique* window of that pid that is really viewable
/// (`xwininfo` `Map State: IsViewable`) and large enough to be the content
/// window; anything else is refused rather than guessed. No global resolution
/// and no unrelated window is touched.

class WindowGeometry {
  const WindowGeometry({
    required this.windowId,
    required this.width,
    required this.height,
    this.mapped = false,
  });

  final String windowId;
  final int width;
  final int height;

  /// `xwininfo` map state: `IsViewable` means the window is really mapped.
  final bool mapped;

  Map<String, Object?> toJson() => <String, Object?>{
    'windowId': windowId,
    'width': width,
    'height': height,
    'mapped': mapped,
  };
}

class WindowResizeResult {
  const WindowResizeResult({
    required this.resized,
    this.windowId,
    this.reason,
    this.original,
    this.applied,
    this.diagnostics = const <String>[],
  });

  /// Whether the GUI-owned window was located, resized and observed at the
  /// requested size.
  final bool resized;

  /// The X11 window id (`0x…`) proven to belong to the GUI process.
  final String? windowId;

  /// Why the resize was refused or did not take effect; null on success.
  final String? reason;

  /// Real geometry observed before the request, so the caller can restore it.
  final WindowGeometry? original;

  /// Real geometry observed after a successful request.
  final WindowGeometry? applied;

  /// Sanitized one-line outcomes of the display queries and Xlib calls, so a
  /// refused or ineffective resize is diagnosable instead of swallowed.
  final List<String> diagnostics;
}

/// Resizes the GUI window owned by [guiPid] and reports the real geometry.
WindowResizeResult resizeOwnedWindow({
  required int guiPid,
  required int width,
  required int height,
}) {
  if (!Platform.isLinux) {
    return _refused(
      'driver window resize is only implemented for the Linux/X11 runner',
    );
  }
  final diagnostics = <String>[];
  final gui = _readGuiProcessEnvironment(guiPid);
  final driverDisplay = _nonEmpty(Platform.environment['DISPLAY']);
  diagnostics.add('gui pid $guiPid -> ${gui.summary(driverDisplay)}');
  if (gui.readable && !gui.hasX11) {
    final wayland = gui.waylandDisplay;
    final waylandNote = wayland == null
        ? ''
        : ' and reports WAYLAND_DISPLAY=$wayland';
    return _refused(
      'the GUI process has no X11 DISPLAY$waylandNote, so it owns no X11 '
      'window the driver could resize',
      diagnostics: diagnostics,
    );
  }
  final display = gui.hasX11 ? gui.display! : driverDisplay;
  if (display == null) {
    return _refused(
      'neither the GUI process environment nor the driver has a DISPLAY, so '
      'the GUI window cannot be located',
      diagnostics: diagnostics,
    );
  }
  final environment = <String, String>{
    'DISPLAY': display,
    if (gui.xauthority != null) 'XAUTHORITY': gui.xauthority!,
  };
  final located = _locateOwnedWindow(guiPid, display, environment, diagnostics);
  final windowId = located.windowId;
  if (windowId == null) {
    return _refused(located.reason!, diagnostics: diagnostics);
  }
  // Only a window that answered a geometry query is resized, so the request is
  // never sent to a window id that has already gone away.
  final original = _readGeometry(windowId, environment, diagnostics);
  if (original == null) {
    return _refused(
      'the current geometry of $windowId on $display could not be read, so the '
      'original window size cannot be restored',
      windowId: windowId,
      diagnostics: diagnostics,
    );
  }
  final numericId = int.parse(windowId.substring(2), radix: 16);
  final failure = _x11Resize(
    numericId,
    width,
    height,
    display,
    driverDisplay,
    diagnostics,
  );
  if (failure != null) {
    return _refused(
      failure,
      windowId: windowId,
      original: original,
      diagnostics: diagnostics,
    );
  }
  final applied = _awaitGeometry(
    windowId,
    width,
    height,
    environment,
    diagnostics,
  );
  final observedSize = applied == null
      ? 'unreadable'
      : '${applied.width}x${applied.height}';
  _record(
    diagnostics,
    'geometry of $windowId after ${width}x$height -> $observedSize',
  );
  if (applied == null || applied.width != width || applied.height != height) {
    // Put the window back so an environment-limited resize does not silently
    // change the run's layout, then report exactly what was observed.
    _x11Resize(
      numericId,
      original.width,
      original.height,
      display,
      driverDisplay,
      diagnostics,
    );
    final observed = applied == null
        ? 'no new geometry was reported'
        : 'the window is ${applied.width}x${applied.height}';
    return _refused(
      'requested ${width}x$height for $windowId on $display but $observed; the '
      'window was put back to ${original.width}x${original.height}',
      windowId: windowId,
      original: original,
      applied: applied,
      diagnostics: diagnostics,
    );
  }
  return WindowResizeResult(
    resized: true,
    windowId: windowId,
    original: original,
    applied: applied,
    diagnostics: diagnostics,
  );
}

WindowResizeResult _refused(
  String reason, {
  String? windowId,
  WindowGeometry? original,
  WindowGeometry? applied,
  List<String> diagnostics = const <String>[],
}) => WindowResizeResult(
  resized: false,
  reason: reason,
  windowId: windowId,
  original: original,
  applied: applied,
  diagnostics: diagnostics,
);

/// Environment facts of the GUI process, read from its own `/proc` entry.
class _GuiProcessEnvironment {
  const _GuiProcessEnvironment({
    this.readable = false,
    this.display,
    this.waylandDisplay,
    this.xauthority,
  });

  final bool readable;
  final String? display;
  final String? waylandDisplay;
  final String? xauthority;

  bool get hasX11 => display != null;

  String summary(String? driverDisplay) {
    if (!readable) {
      return 'environment unreadable; driver display '
          '${_describe(driverDisplay)}';
    }
    final xauthorityState = xauthority == null ? 'unset' : 'set';
    return 'display ${_describe(display)}, WAYLAND_DISPLAY '
        '${_describe(waylandDisplay)}, XAUTHORITY $xauthorityState';
  }
}

_GuiProcessEnvironment _readGuiProcessEnvironment(int pid) {
  try {
    final entries = <String, String>{};
    var start = 0;
    final bytes = File('/proc/$pid/environ').readAsBytesSync();
    for (var index = 0; index <= bytes.length; index++) {
      if (index != bytes.length && bytes[index] != 0) continue;
      if (index > start) {
        final text = String.fromCharCodes(bytes.sublist(start, index));
        final separator = text.indexOf('=');
        if (separator > 0) {
          entries[text.substring(0, separator)] = text.substring(separator + 1);
        }
      }
      start = index + 1;
    }
    return _GuiProcessEnvironment(
      readable: true,
      display: _nonEmpty(entries['DISPLAY']),
      waylandDisplay: _nonEmpty(entries['WAYLAND_DISPLAY']),
      xauthority: _nonEmpty(entries['XAUTHORITY']),
    );
  } on Object {
    return const _GuiProcessEnvironment();
  }
}

String _describe(String? value) => value == null ? 'unset' : '"$value"';

String? _nonEmpty(String? value) =>
    value == null || value.isEmpty ? null : value;

class _OwnedWindowLookup {
  const _OwnedWindowLookup(this.windowId, this.reason);

  final String? windowId;
  final String? reason;
}

/// One candidate window that reports the GUI pid, with its observed geometry.
class _OwnedWindow {
  const _OwnedWindow(this.id, this.geometry, this.windowClass);

  final String id;
  final WindowGeometry? geometry;
  final String? windowClass;

  /// Whether this window can be the real content window: really mapped and large
  /// enough that a hidden 10x10 GTK helper cannot pass.
  bool get isRealContentWindow =>
      geometry != null &&
      geometry!.mapped &&
      geometry!.width >= _minVisibleExtent &&
      geometry!.height >= _minVisibleExtent;

  String describe() {
    final observed = geometry;
    if (observed == null) return '$id geometry unreadable';
    final windowClass = this.windowClass;
    return '$id ${observed.width}x${observed.height} '
        'mapped=${observed.mapped}'
        '${windowClass == null ? '' : ' class ${_sanitize(windowClass)}'}';
  }
}

/// A window narrower or shorter than this cannot be the content window; GTK's
/// hidden helper toplevels are far smaller (observed 10x10).
const _minVisibleExtent = 100;

/// Finds the single client window that reports [guiPid] as its `_NET_WM_PID` and
/// is really the mapped content window, ignoring the process's hidden helpers.
_OwnedWindowLookup _locateOwnedWindow(
  int guiPid,
  String display,
  Map<String, String> environment,
  List<String> diagnostics,
) {
  final candidates = _candidateWindowIds(environment, diagnostics);
  if (candidates.isEmpty) {
    return _OwnedWindowLookup(
      null,
      'no candidate client windows could be listed on GUI display $display; '
      'the recorded diagnostics hold the raw command outcomes',
    );
  }
  final matches = <_OwnedWindow>[];
  var unreadable = 0;
  for (final id in candidates) {
    final pid = _windowPid(id, environment, diagnostics);
    if (pid == null) {
      unreadable++;
      continue;
    }
    if (pid != guiPid) continue;
    matches.add(
      _OwnedWindow(
        id,
        _readGeometry(id, environment, diagnostics, quiet: true),
        _windowClass(id, environment, diagnostics),
      ),
    );
  }
  if (matches.isEmpty) {
    return _OwnedWindowLookup(
      null,
      'no window on GUI display $display reports _NET_WM_PID $guiPid (checked '
      '${candidates.length} client windows, $unreadable without a readable pid), '
      'so the GUI window could not be proven to belong to this run',
    );
  }
  final real = matches
      .where((match) => match.isRealContentWindow)
      .toList(growable: false);
  final described = matches.map((match) => match.describe()).join('; ');
  if (real.length == 1) {
    _record(
      diagnostics,
      'owned window ${real.single.describe()} is the unique viewable '
      '${_minVisibleExtent}px+ top-level of pid $guiPid on $display',
    );
    return _OwnedWindowLookup(real.single.id, null);
  }
  return _OwnedWindowLookup(
    null,
    '${matches.length} window(s) on GUI display $display report _NET_WM_PID '
    '$guiPid ($described) but ${real.isEmpty ? 'none is' : '${real.length} are'} '
    'a viewable ${_minVisibleExtent}px+ top-level, so the resize target is '
    'ambiguous or only a hidden helper is owned',
  );
}

/// Client toplevels on the GUI display, from the EWMH property when present.
List<String> _candidateWindowIds(
  Map<String, String> environment,
  List<String> diagnostics,
) {
  final managed = _listWindowIds(
    'xprop',
    const ['-root', '_NET_CLIENT_LIST'],
    RegExp(r'(0x[0-9a-fA-F]+)'),
    environment,
    diagnostics,
  );
  if (managed.isNotEmpty) return managed;
  // A window manager that is absent or not EWMH still exposes the X11 tree.
  return _listWindowIds(
    'xwininfo',
    const ['-root', '-tree'],
    RegExp(r'^\s*(0x[0-9a-fA-F]+)\s', multiLine: true),
    environment,
    diagnostics,
  );
}

List<String> _listWindowIds(
  String program,
  List<String> arguments,
  RegExp pattern,
  Map<String, String> environment,
  List<String> diagnostics,
) {
  final outcome = _runX11Command(program, arguments, environment, diagnostics);
  if (outcome == null || outcome.exitCode != 0) return const [];
  final ids = <String>[];
  for (final match in pattern.allMatches(outcome.stdout.toString())) {
    final id = match.group(1)!;
    if (!ids.contains(id)) ids.add(id);
  }
  return ids;
}

/// Reads one window's `_NET_WM_PID` through `xprop`, or null when unreadable.
int? _windowPid(
  String windowId,
  Map<String, String> environment,
  List<String> diagnostics,
) {
  final outcome = _runX11Command(
    'xprop',
    ['-id', windowId, '_NET_WM_PID'],
    environment,
    diagnostics,
    quiet: true,
  );
  if (outcome == null || outcome.exitCode != 0) return null;
  final match = RegExp(r'_NET_WM_PID\([^)]*\)\s*=\s*(\d+)')
      .firstMatch(outcome.stdout.toString());
  return match == null ? null : int.tryParse(match.group(1)!);
}

/// Reads the window's live geometry from `xwininfo`, or null when unavailable.
WindowGeometry? _readGeometry(
  String windowId,
  Map<String, String> environment,
  List<String> diagnostics, {
  bool quiet = false,
}) {
  final outcome = _runX11Command(
    'xwininfo',
    ['-id', windowId],
    environment,
    diagnostics,
    quiet: quiet,
  );
  if (outcome == null || outcome.exitCode != 0) return null;
  final output = outcome.stdout.toString();
  final width = _dimension(output, 'Width');
  final height = _dimension(output, 'Height');
  if (width == null || height == null) return null;
  return WindowGeometry(
    windowId: windowId,
    width: width,
    height: height,
    mapped: _mapStateIsViewable(output),
  );
}

/// Reads the window's `WM_CLASS` through `xprop`, for evidence only.
///
/// The class is never the ownership proof: `_NET_WM_PID` is. It is recorded so a
/// reviewer can confirm which real window was chosen.
String? _windowClass(
  String windowId,
  Map<String, String> environment,
  List<String> diagnostics,
) {
  final outcome = _runX11Command(
    'xprop',
    ['-id', windowId, 'WM_CLASS'],
    environment,
    diagnostics,
    quiet: true,
  );
  if (outcome == null || outcome.exitCode != 0) return null;
  final text = outcome.stdout.toString().trim();
  return text.isEmpty ? null : text;
}

/// `xwininfo`'s `Map State: IsViewable` is the process-independent fact that the
/// window is really mapped; `IsUnMapped`/`IsUnviewable` are hidden helpers.
bool _mapStateIsViewable(String output) => RegExp(
  r'^\s*Map State:\s*IsViewable\s*$',
  multiLine: true,
).hasMatch(output);

int? _dimension(String output, String label) {
  final match = RegExp(
    '^\\s*$label:\\s*(\\d+)\\s*\$',
    multiLine: true,
  ).firstMatch(output);
  return match == null ? null : int.parse(match.group(1)!);
}

/// Polls the real geometry until the requested size is observed, bounded so a
/// window manager that ignores the request is reported instead of hanging.
WindowGeometry? _awaitGeometry(
  String windowId,
  int width,
  int height,
  Map<String, String> environment,
  List<String> diagnostics,
) {
  final deadline = DateTime.now().add(const Duration(seconds: 5));
  WindowGeometry? observed;
  while (true) {
    observed = _readGeometry(windowId, environment, diagnostics, quiet: true);
    if (observed != null &&
        observed.width == width &&
        observed.height == height) {
      return observed;
    }
    if (!DateTime.now().isBefore(deadline)) return observed;
    sleep(const Duration(milliseconds: 100));
  }
}

/// Runs [program] with the GUI's own DISPLAY/XAUTHORITY.
///
/// Every outcome is sanitized into [diagnostics] so a refused resize carries the
/// raw command result instead of a swallowed failure. `xprop` and `xwininfo` are
/// separate processes, so a stale window id only makes them exit non-zero
/// instead of raising an Xlib protocol error inside the driver.
ProcessResult? _runX11Command(
  String program,
  List<String> arguments,
  Map<String, String> environment,
  List<String> diagnostics, {
  bool quiet = false,
}) {
  final command = '$program ${arguments.join(' ')}';
  try {
    final result = Process.runSync(
      program,
      arguments,
      environment: environment,
      includeParentEnvironment: true,
    );
    if (result.exitCode != 0) {
      _record(
        diagnostics,
        '$command -> exit ${result.exitCode}; stderr: '
        '${_sanitize(result.stderr.toString())}; stdout: '
        '${_sanitize(result.stdout.toString())}',
      );
    } else if (!quiet) {
      _record(
        diagnostics,
        '$command -> exit 0; stdout: ${_sanitize(result.stdout.toString())}',
      );
    }
    return result;
  } on Object catch (error) {
    _record(diagnostics, '$command -> could not run: ${_sanitize('$error')}');
    return null;
  }
}

void _record(List<String> diagnostics, String line) {
  const limit = 10;
  if (diagnostics.length < limit) {
    diagnostics.add(line);
  } else if (diagnostics.length == limit) {
    diagnostics.add('further display query outcomes omitted');
  }
}

/// Collapses whitespace, redacts path-like tokens and truncates one command
/// outcome so evidence stays readable without carrying local paths.
String _sanitize(String value) {
  final collapsed = value.replaceAll(RegExp(r'\s+'), ' ').trim();
  final redacted = collapsed.replaceAll(RegExp(r'\S*/[^\s]*'), '<path>');
  return redacted.length <= 200 ? redacted : '${redacted.substring(0, 200)}…';
}

/// Requests the resize through Xlib and returns a reason when it fails.
///
/// Xlib is pointed at [display] explicitly when it differs from the driver's own
/// display, so the request targets the GUI's display rather than whatever the
/// driver process happens to use.
String? _x11Resize(
  int windowId,
  int width,
  int height,
  String display,
  String? driverDisplay,
  List<String> diagnostics,
) {
  late final DynamicLibrary lib;
  try {
    lib = DynamicLibrary.open('libX11.so.6');
  } on Object catch (error) {
    return 'libX11 is unavailable: $error';
  }
  Pointer<Uint8>? name;
  if (display != driverDisplay) {
    name = _cString(display);
    if (name == null) {
      _record(
        diagnostics,
        'display name $display -> could not be copied to native memory',
      );
      return 'the GUI display $display could not be passed to Xlib';
    }
  }
  try {
    final openDisplay = lib.lookupFunction<_XOpenDisplayNative, _XOpenDisplay>(
      'XOpenDisplay',
    );
    final handle = openDisplay(name == null ? nullptr : name.cast<Char>());
    if (handle == nullptr) {
      _record(diagnostics, 'XOpenDisplay($display) -> null');
      return 'XOpenDisplay($display) returned null, so the driver cannot open '
          'the GUI display to request the resize';
    }
    try {
      final resize = lib.lookupFunction<_XResizeWindowNative, _XResizeWindow>(
        'XResizeWindow',
      );
      final flush = lib.lookupFunction<_XFlushNative, _XFlush>('XFlush');
      resize(handle, windowId, width, height);
      flush(handle);
      _record(
        diagnostics,
        'XResizeWindow($windowId, ${width}x$height) on $display -> requested',
      );
    } on Object catch (error) {
      return 'XResizeWindow could not run: $error';
    } finally {
      lib.lookupFunction<_XCloseDisplayNative, _XCloseDisplay>('XCloseDisplay')(
        handle,
      );
    }
  } finally {
    if (name != null) _freeCString(name);
  }
  return null;
}

/// Copies [value] into native C memory for Xlib.
///
/// Only the display name needs this, so there is deliberately no general
/// allocator here: `dart:ffi` ships the allocator in `package:ffi`, which is not
/// a declared dependency, so the loaded C library is used directly.
Pointer<Uint8>? _cString(String value) {
  final bytes = utf8.encode(value);
  for (final library in _cLibraries()) {
    try {
      final malloc = library.lookupFunction<_MallocNative, _Malloc>('malloc');
      final buffer = malloc(bytes.length + 1);
      if (buffer == nullptr) return null;
      final view = buffer.asTypedList(bytes.length + 1);
      view.setRange(0, bytes.length, bytes);
      view[bytes.length] = 0;
      return buffer;
    } on Object {
      continue;
    }
  }
  return null;
}

void _freeCString(Pointer<Uint8> pointer) {
  for (final library in _cLibraries()) {
    try {
      library.lookupFunction<_FreeNative, _Free>('free')(pointer);
      return;
    } on Object {
      continue;
    }
  }
}

List<DynamicLibrary> _cLibraries() {
  final libraries = <DynamicLibrary>[DynamicLibrary.process()];
  try {
    libraries.add(DynamicLibrary.open('libc.so.6'));
  } on Object {
    // The C library is only a fallback; the process table is preferred.
  }
  return libraries;
}

typedef _XOpenDisplayNative = Pointer<Void> Function(Pointer<Char>);
typedef _XOpenDisplay = Pointer<Void> Function(Pointer<Char>);
typedef _XResizeWindowNative = Int32 Function(
  Pointer<Void>,
  UintPtr,
  Uint32,
  Uint32,
);
typedef _XResizeWindow = int Function(Pointer<Void>, int, int, int);
typedef _XFlushNative = Int32 Function(Pointer<Void>);
typedef _XFlush = int Function(Pointer<Void>);
typedef _XCloseDisplayNative = Int32 Function(Pointer<Void>);
typedef _XCloseDisplay = int Function(Pointer<Void>);
typedef _MallocNative = Pointer<Uint8> Function(IntPtr);
typedef _Malloc = Pointer<Uint8> Function(int);
typedef _FreeNative = Void Function(Pointer<Uint8>);
typedef _Free = void Function(Pointer<Uint8>);
