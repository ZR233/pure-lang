import 'dart:async';

import 'package:flutter/foundation.dart' show clampDouble;
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../app/theme/studio_tokens.dart';
import '../../data/repositories/studio_repository.dart';
import '../../domain/models/studio_models.dart';
import '../../l10n/studio_l10n.dart';
import '../../shared/studio_driver_keys.dart';
import 'settings_common.dart';

class RemoteDirectoryDialog extends ConsumerStatefulWidget {
  const RemoteDirectoryDialog({super.key, required this.server});

  final SshServer server;

  @override
  ConsumerState<RemoteDirectoryDialog> createState() =>
      _RemoteDirectoryDialogState();
}

class _RemoteDirectoryDialogState extends ConsumerState<RemoteDirectoryDialog> {
  late final TextEditingController _pathController;
  RemoteDirectoryListing? _listing;
  String? _error;
  bool _browsing = false;
  bool _opening = false;

  bool get _busy => _browsing || _opening;

  bool get _canOpen =>
      !_busy &&
      _listing != null &&
      _pathController.text.trim() == _listing!.path;

  @override
  void initState() {
    super.initState();
    _pathController = TextEditingController();
    unawaited(_load(null));
  }

  @override
  void dispose() {
    _pathController.dispose();
    super.dispose();
  }

  void _go() {
    if (_busy) return;
    final raw = _pathController.text.trim();
    if (raw.isEmpty) {
      setState(() => _error = context.l10n.settingsSshPathRequired);
      return;
    }
    if (!raw.startsWith('/')) {
      setState(() => _error = context.l10n.settingsSshPathAbsolute);
      return;
    }
    unawaited(_load(raw));
  }

  Future<void> _load(String? path) async {
    // 入口级串行 guard：pending 期间拒绝任何重复/冲突请求，
    // 不依赖下一帧的按钮禁用状态。
    if (_busy) return;
    setState(() {
      // 新请求开始后，旧 listing 不再是当前输入的已验证结果；即使请求
      // 失败，也不能让旧目录重新启用 Open。
      _listing = null;
      _browsing = true;
      _error = null;
      if (path != null) _pathController.text = path;
    });
    try {
      final listing = await ref
          .read(studioApiProvider)
          .browseRemoteDirectories(widget.server.id, path: path);
      if (!mounted) return;
      if (!listing.path.startsWith('/')) {
        setState(() {
          _browsing = false;
          _error = context.l10n.settingsSshPathAbsolute;
        });
        return;
      }
      setState(() {
        _listing = listing;
        _browsing = false;
        _pathController.text = listing.path;
      });
    } catch (error) {
      if (!mounted) return;
      setState(() {
        _browsing = false;
        _error = error.toString();
      });
    }
  }

  Future<void> _open() async {
    if (!_canOpen) return;
    final path = _listing?.path;
    if (path == null) return;
    setState(() {
      _opening = true;
      _error = null;
    });
    var opened = false;
    try {
      opened = await ref
          .read(studioControllerProvider.notifier)
          .openRemoteProject(widget.server.id, path);
    } on Object {
      // Controller failures are normally converted to false; retain the same
      // retryable dialog state if provider access or another boundary fails.
    }
    if (!mounted) return;
    if (opened) {
      setState(() => _opening = false);
      Navigator.of(context).pop(path);
    } else {
      setState(() {
        _opening = false;
        _error = context.l10n.settingsSshOpenFailed;
      });
    }
  }

  /// Cancel 关闭入口：在执行瞬间检查 `_opening`，即使拿到的是重建前的旧
  /// onPressed closure 也安全——返回/遮罩/Escape 走 PopScope 回调、Cancel 走这里，
  /// 都依赖同步字段而非下一次 rebuild。
  void _cancel() {
    if (_opening) return;
    Navigator.of(context).pop();
  }

  @override
  Widget build(BuildContext context) {
    final listing = _listing;
    final busy = _busy;
    return PopScope(
      canPop: false,
      onPopInvokedWithResult: (didPop, result) {
        if (didPop || _opening) return;
        Navigator.of(context).pop();
      },
      child: AlertDialog(
        key: StudioDriverKeys.sshDirectoryDialog,
        scrollable: true,
        title: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Text(context.l10n.settingsSshChooseDirectory),
            const SizedBox(height: 4),
            Text(
              '${widget.server.username}@${widget.server.host}:${widget.server.port}',
              style: context.text.bodySmall?.copyWith(
                color: context.colors.onSurfaceVariant,
                fontFamily: 'monospace',
              ),
              maxLines: 1,
              overflow: TextOverflow.ellipsis,
            ),
          ],
        ),
        content: SizedBox(
          width: _dialogContentWidth(context),
          child: Column(
            mainAxisSize: MainAxisSize.min,
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              _buildPathFieldRow(context, busy),
              if (busy && listing != null) ...[
                const SizedBox(height: 12),
                const LinearProgressIndicator(),
              ],
              const SizedBox(height: 12),
              _buildCurrentPathRow(context, listing, busy),
              const Divider(),
              _buildListingBody(listing, busy),
            ],
          ),
        ),
        actions: [
          TextButton(
            onPressed: _opening ? null : _cancel,
            child: Text(context.l10n.settingsCancel),
          ),
          FilledButton.icon(
            key: StudioDriverKeys.sshOpenCurrentDirectory,
            onPressed: _canOpen ? () => unawaited(_open()) : null,
            icon: _opening
                ? const SizedBox.square(
                    dimension: 17,
                    child: CircularProgressIndicator(strokeWidth: 2),
                  )
                : const Icon(Icons.folder_open_outlined, size: 17),
            label: Text(context.l10n.settingsSshOpenThisDirectory),
          ),
        ],
      ),
    );
  }

  Widget _buildListingBody(RemoteDirectoryListing? listing, bool busy) {
    if (_error != null) {
      return SettingsInlineError(
        key: StudioDriverKeys.sshDirectoryError,
        message: _error!,
      );
    }
    if (listing == null) {
      return const Center(child: CircularProgressIndicator());
    }
    if (listing.entries.isEmpty) {
      return SettingsEmptyMessage(
        key: StudioDriverKeys.sshDirectoryEmpty,
        icon: Icons.folder_open_outlined,
        title: context.l10n.settingsSshDirectoryEmpty,
        body: context.l10n.settingsSshDirectoryEmptyHint,
      );
    }
    // 目录浏览只在 body 内滚动，整体溢出由 AlertDialog(scrollable: true)
    // 处理；使用 box-backed scrolling content 以兼容 AlertDialog 的 intrinsic 测量。
    return ConstrainedBox(
      constraints: BoxConstraints(maxHeight: _directoryBodyMaxHeight(context)),
      child: SingleChildScrollView(
        key: StudioDriverKeys.sshDirectoryList,
        child: Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            for (final entry in listing.entries)
              ListTile(
                key: StudioDriverKeys.sshDirectoryEntry(entry.path),
                leading: const Icon(Icons.folder_outlined),
                title: Tooltip(
                  message: entry.path,
                  child: Text(
                    entry.name,
                    maxLines: 1,
                    overflow: TextOverflow.ellipsis,
                  ),
                ),
                dense: true,
                shape: RoundedRectangleBorder(
                  borderRadius: BorderRadius.circular(6),
                ),
                onTap: busy ? null : () => _load(entry.path),
              ),
          ],
        ),
      ),
    );
  }

  /// 路径输入与 Go 在窄视口/放大文本下可换行堆叠，宽视口保持单行。
  Widget _buildPathFieldRow(BuildContext context, bool busy) {
    // `AlertDialog(scrollable: true)` measures its content intrinsically;
    // LayoutBuilder cannot participate in that pass on Flutter 3.47.1.
    // The dialog width is derived from the same viewport constraint, so it is
    // also sufficient to select the narrow layout without a second layout
    // pass.
    final stack = _dialogContentWidth(context) < 420;
    final field = TextField(
      key: StudioDriverKeys.sshDirectoryPathInput,
      controller: _pathController,
      decoration: InputDecoration(
        labelText: context.l10n.settingsSshDirectoryPathLabel,
        hintText: context.l10n.settingsSshDirectoryPathHint,
      ),
      textInputAction: TextInputAction.go,
      onChanged: (_) {
        // Browse failures keep their retry prompt while no canonical
        // listing exists; an existing listing can safely clear stale
        // open/validation errors as the input changes.
        setState(() {
          if (_listing != null) _error = null;
        });
      },
      onSubmitted: (_) => _go(),
      enabled: !busy,
    );
    final go = TextButton(
      key: StudioDriverKeys.sshDirectoryGo,
      onPressed: busy ? null : _go,
      child: Text(context.l10n.settingsSshGo),
    );
    if (stack) {
      return Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          field,
          const SizedBox(height: 8),
          Align(alignment: Alignment.centerRight, child: go),
        ],
      );
    }
    return Row(
      children: [
        Expanded(child: field),
        const SizedBox(width: 8),
        go,
      ],
    );
  }

  Widget _buildCurrentPathRow(
    BuildContext context,
    RemoteDirectoryListing? listing,
    bool busy,
  ) {
    return Row(
      children: [
        IconButton(
          key: StudioDriverKeys.sshDirectoryUp,
          tooltip: context.l10n.settingsSshUp,
          onPressed: (busy || listing?.parent == null)
              ? null
              : () => _load(listing!.parent),
          icon: const Icon(Icons.arrow_upward),
        ),
        const SizedBox(width: 4),
        Expanded(
          child: SelectableText(
            listing?.path ?? '…',
            key: listing == null
                ? null
                : StudioDriverKeys.sshDirectoryCurrent(listing.path),
          ),
        ),
      ],
    );
  }

  /// 目录浏览视口高度上限：只约束列表本身，不约束内容总高度。整体溢出交给
  /// `AlertDialog(scrollable: true)` 滚动；actions 固定在底部，不会与 path 重叠。
  double _directoryBodyMaxHeight(BuildContext context) {
    final viewportHeight = MediaQuery.sizeOf(context).height;
    return clampDouble(viewportHeight * 0.42, 150.0, 340.0);
  }

  /// 内容宽度受 viewport 约束：窄窗口收缩，宽桌面保持合理宽度。
  double _dialogContentWidth(BuildContext context) {
    final viewport = MediaQuery.sizeOf(context);
    // AlertDialog 默认左右 inset 各 40px；按可用宽度计算，避免窄窗口中
    // SizedBox 的固定宽度突破 dialog 约束。这里不设置固定最小宽度，
    // 让极窄 viewport 仍由父约束决定可用宽度。
    return clampDouble(viewport.width - 80.0, 0.0, 640.0);
  }
}
