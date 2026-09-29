import 'dart:ui' as ui;

import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

const hostAppsChannel = MethodChannel('io.github.zr233.anywork/host_apps');

enum HostAppIcon { vsCode, zed, terminal }

/// Native application artwork, cached independently of executable availability.
/// Missing or invalid artwork must never disable an otherwise usable launcher.
final hostAppIconProvider = FutureProvider.family<Uint8List?, HostAppIcon>((
  ref,
  app,
) async {
  ui.Codec? codec;
  try {
    final bytes = await hostAppsChannel.invokeMethod<Uint8List>(switch (app) {
      HostAppIcon.vsCode => 'vsCodeIcon',
      HostAppIcon.zed => 'zedIcon',
      HostAppIcon.terminal => 'terminalIcon',
    });
    if (bytes == null || bytes.isEmpty) return null;
    codec = await ui.instantiateImageCodec(bytes);
    return bytes;
  } on Object {
    return null;
  } finally {
    codec?.dispose();
  }
});
