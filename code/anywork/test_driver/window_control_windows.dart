part of 'window_control.dart';

/// Uses client-area physical pixels, matching the X11 driver. Layout assertions
/// read Flutter's actual logical geometry separately, so restoring odd physical
/// dimensions never loses pixels through DPI rounding. All
/// native buffers belong to this synchronous call. EnumWindows invokes its
/// callback on the calling thread and retains neither callback nor buffers.
WindowResizeResult _resizeWindowsWindow(int guiPid, int width, int height) {
  if (guiPid <= 0 || width <= 0 || height <= 0) {
    return _refused('window PID and dimensions must be positive');
  }
  final diagnostics = <String>[];
  final previousDpi = win32.SetThreadDpiAwarenessContext(
    win32.DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
  );
  if (previousDpi == nullptr) {
    return _refused('could not establish physical window coordinates');
  }
  try {
    return ffi.using((arena) {
      final processId = arena<Uint32>();
      final client = arena<win32.RECT>();
      final frame = arena<win32.RECT>();
      final candidates = <win32.HWND>[];
      final callback = NativeCallable<win32.WNDENUMPROC>.isolateLocal((
        Pointer handle,
        int parameter,
      ) {
        final window = win32.HWND(handle);
        if (win32.GetWindowThreadProcessId(window, processId) != 0 &&
            processId.value == guiPid &&
            win32.IsWindowVisible(window) &&
            win32.GetClientRect(window, client).value &&
            client.ref.right > 100 &&
            client.ref.bottom > 100) {
          candidates.add(window);
        }
        return 1;
      }, exceptionalReturn: 0);
      try {
        final enumerated = win32.EnumWindows(
          callback.nativeFunction,
          const win32.LPARAM(0),
        );
        if (!enumerated.value) {
          return _refused('EnumWindows failed: ${enumerated.error}');
        }
      } finally {
        callback.close();
      }
      diagnostics.add(
        'pid $guiPid owns ${candidates.length} visible content windows',
      );
      if (candidates.length != 1) {
        return _refused(
          'GUI window ownership is not unique',
          diagnostics: diagnostics,
        );
      }
      final window = candidates.single;
      final id = '0x${window.address.toRadixString(16)}';
      if (win32.IsIconic(window) || win32.IsZoomed(window)) {
        return _refused(
          'GUI window is unavailable, minimized or maximized',
          windowId: id,
          diagnostics: diagnostics,
        );
      }
      WindowGeometry? geometry() {
        if (!win32.GetClientRect(window, client).value) return null;
        return WindowGeometry(
          windowId: id,
          width: client.ref.right - client.ref.left,
          height: client.ref.bottom - client.ref.top,
          mapped: win32.IsWindowVisible(window),
        );
      }

      final original = geometry();
      if (original == null || !win32.GetWindowRect(window, frame).value) {
        return _refused(
          'could not read original client and frame geometry',
          windowId: id,
          diagnostics: diagnostics,
        );
      }
      final frameWidth = frame.ref.right - frame.ref.left;
      final frameHeight = frame.ref.bottom - frame.ref.top;
      final borderWidth = frameWidth - (client.ref.right - client.ref.left);
      final borderHeight = frameHeight - (client.ref.bottom - client.ref.top);
      bool resize(int outerWidth, int outerHeight) {
        // Recheck ownership immediately before every mutation, including restore.
        if (win32.GetWindowThreadProcessId(window, processId) == 0 ||
            processId.value != guiPid) {
          return false;
        }
        final result = win32.SetWindowPos(
          window,
          null,
          0,
          0,
          outerWidth,
          outerHeight,
          win32.SWP_NOMOVE | win32.SWP_NOZORDER | win32.SWP_NOACTIVATE,
        );
        diagnostics.add(
          'SetWindowPos($id, ${outerWidth}x$outerHeight) '
          '-> ${result.value}, error=${result.error}',
        );
        return result.value;
      }

      var accepted = false;
      WindowGeometry? applied;
      WindowGeometry? awaitGeometry(int expectedWidth, int expectedHeight) {
        final deadline = DateTime.now().add(const Duration(seconds: 3));
        WindowGeometry? observed;
        do {
          observed = geometry();
          if (observed?.width == expectedWidth &&
              observed?.height == expectedHeight) {
            return observed;
          }
          sleep(const Duration(milliseconds: 25));
        } while (DateTime.now().isBefore(deadline));
        return observed;
      }

      try {
        if (!resize(width + borderWidth, height + borderHeight)) {
          return _refused(
            'SetWindowPos failed',
            windowId: id,
            original: original,
            diagnostics: diagnostics,
          );
        }
        applied = awaitGeometry(width, height);
        if (applied?.width == width && applied?.height == height) {
          accepted = true;
          return WindowResizeResult(
            resized: true,
            windowId: id,
            original: original,
            applied: applied,
            diagnostics: diagnostics,
          );
        }
        return _refused(
          'requested ${width}x$height but observed '
          '${applied?.width}x${applied?.height}',
          windowId: id,
          original: original,
          applied: applied,
          diagnostics: diagnostics,
        );
      } finally {
        if (!accepted) {
          final requested = resize(frameWidth, frameHeight);
          final restored = awaitGeometry(original.width, original.height);
          if (!requested ||
              restored?.width != original.width ||
              restored?.height != original.height) {
            throw StateError('failed to restore GUI window $id: $diagnostics');
          }
        }
      }
    });
  } finally {
    win32.SetThreadDpiAwarenessContext(previousDpi);
  }
}
