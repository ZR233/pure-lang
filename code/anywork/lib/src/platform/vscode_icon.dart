import 'dart:ui' as ui;

import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

/// 宿主系统图标通道名；宿主按 `vscode` URL 协议默认处理应用解析图标。
const MethodChannel vsCodeIconChannel = MethodChannel(
  'io.github.zr233.anywork/application_icon',
);

/// 读取宿主系统为 `vscode` 协议默认处理应用解析的图标（PNG 图片字节）。
///
/// 宿主无该协议关联、缺图或查询失败时返回 null；仅在字节可解码为图片时
/// 返回，调用方据此决定是否使用图标按钮。通道异常一律在 Dart 侧吞掉并返回
/// null，不影响 GUI。
Future<Uint8List?> loadVsCodeIcon() async {
  final Uint8List? bytes;
  try {
    bytes = await vsCodeIconChannel.invokeMethod<Uint8List>('vsCodeIcon');
  } on Object {
    return null;
  }
  if (bytes == null || bytes.isEmpty) {
    return null;
  }
  final decodable = await _isDecodableImage(bytes);
  return decodable ? bytes : null;
}

/// 验证字节是否是可解码图片；不保留解码结果，避免持有原生图像资源。
Future<bool> _isDecodableImage(Uint8List bytes) async {
  ui.Codec? codec;
  try {
    codec = await ui.instantiateImageCodec(bytes);
    return true;
  } on Object {
    return false;
  } finally {
    codec?.dispose();
  }
}

/// 进程内缓存一次 VS Code 图标查询；demo 与 widget 测试可覆写注入。
final vsCodeIconProvider = FutureProvider<Uint8List?>((ref) async {
  return loadVsCodeIcon();
});
