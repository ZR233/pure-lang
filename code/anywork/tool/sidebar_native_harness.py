"""Isolated Linux Flutter Driver sidebar acceptance; owns and reaps the GUI tree."""
import ctypes
import json
import os
from pathlib import Path
import re
import signal
import subprocess
import sys
import time


def resize(display):
    x = ctypes.CDLL('libX11.so.6')
    ptr, win = ctypes.c_void_p, ctypes.c_ulong
    x.XOpenDisplay.argtypes = [ctypes.c_char_p]
    x.XOpenDisplay.restype = ptr
    x.XDefaultRootWindow.argtypes = [ptr]
    x.XDefaultRootWindow.restype = win
    x.XQueryTree.argtypes = [ptr, win, ctypes.POINTER(win), ctypes.POINTER(win), ctypes.POINTER(ctypes.POINTER(win)), ctypes.POINTER(ctypes.c_uint)]
    x.XFetchName.argtypes = [ptr, win, ctypes.POINTER(ptr)]
    class ClassHint(ctypes.Structure):
        _fields_ = [('name', ptr), ('klass', ptr)]
    x.XGetClassHint.argtypes = [ptr, win, ctypes.POINTER(ClassHint)]
    x.XFree.argtypes = [ptr]
    x.XResizeWindow.argtypes = [ptr, win, ctypes.c_uint, ctypes.c_uint]
    x.XSync.argtypes = [ptr, ctypes.c_int]
    x.XCloseDisplay.argtypes = [ptr]
    connection = x.XOpenDisplay(display.encode())
    if not connection:
        raise RuntimeError('Cannot open isolated X display')
    try:
        queue = [x.XDefaultRootWindow(connection)]
        seen = []
        while queue:
            window = queue.pop()
            hint = ClassHint()
            identity = ''
            if x.XGetClassHint(connection, window, ctypes.byref(hint)):
                for value in [hint.name, hint.klass]:
                    if value:
                        identity += ctypes.string_at(value).decode('utf8', errors='replace') + ' '
                        x.XFree(value)
            name = ptr()
            if x.XFetchName(connection, window, ctypes.byref(name)) and name:
                title = ctypes.string_at(name).decode('utf8', errors='replace')
                x.XFree(name)
                identity += title
            seen.append(identity)
            if 'anywork' in identity.lower() or '糊来帮' in identity:
                x.XResizeWindow(connection, window, 700, 800)
                x.XSync(connection, 0)
                return
            root, parent, children, count = win(), win(), ctypes.POINTER(win)(), ctypes.c_uint()
            if x.XQueryTree(connection, window, ctypes.byref(root), ctypes.byref(parent), ctypes.byref(children), ctypes.byref(count)):
                queue.extend(children[i] for i in range(count.value))
                if children:
                    x.XFree(children)
        raise RuntimeError(f'No Anywork window on isolated display: {seen}')
    finally:
        x.XCloseDisplay(connection)


def run(output):
    root = Path(__file__).resolve().parents[3]
    output = Path(output).resolve()
    output.mkdir(parents=True, exist_ok=True)
    display_file = output / 'display.txt'
    auth_file = output / 'xauthority-path.txt'
    gui = None
    try:
        with (output / 'gui.log').open('w') as log:
            gui = subprocess.Popen(['xvfb-run', '-a', '-s', '-screen 0 1600x1000x24', 'bash', '-c', 'printenv DISPLAY > "$1"; printenv XAUTHORITY > "$2"; exec cargo xtask run-gui --demo --driver', 'sidebar-driver', str(display_file), str(auth_file)], cwd=root, env=dict(os.environ, GDK_BACKEND='x11', WAYLAND_DISPLAY='', LIBGL_ALWAYS_SOFTWARE='1'), stdout=log, stderr=subprocess.STDOUT, start_new_session=True)
            deadline = time.monotonic() + 600
            while time.monotonic() < deadline:
                matches = re.findall(r'A Dart VM Service .*?available at: (http://127\.0\.0\.1:\d+/[^\s]*)', (output / 'gui.log').read_text())
                if matches:
                    break
                if gui.poll() is not None:
                    raise RuntimeError('GUI exited; see gui.log')
                time.sleep(.2)
            else:
                raise TimeoutError('GUI did not publish VM service')
            with (output / 'driver.log').open('w') as driver_log:
                subprocess.run(['cargo', 'dart', 'run', 'test_driver/project_sidebar_acceptance_driver.dart', matches[-1], str(output), display_file.read_text().strip()], cwd=root, env=dict(os.environ, DISPLAY=display_file.read_text().strip(), XAUTHORITY=auth_file.read_text().strip()), stdout=driver_log, stderr=subprocess.STDOUT, check=True, timeout=300)
    finally:
        if gui is not None:
            try:
                os.killpg(gui.pid, signal.SIGTERM)
            except ProcessLookupError:
                pass
            try:
                gui.wait(timeout=15)
            except subprocess.TimeoutExpired:
                os.killpg(gui.pid, signal.SIGKILL)
                gui.wait(timeout=5)
            deadline = time.monotonic() + 5
            while True:
                remaining = []
                for proc in Path('/proc').iterdir():
                    if proc.name.isdigit():
                        try:
                            if os.getpgid(int(proc.name)) == gui.pid:
                                remaining.append(int(proc.name))
                        except ProcessLookupError:
                            pass
                if not remaining or time.monotonic() >= deadline:
                    break
                time.sleep(.1)
            (output / 'cleanup.json').write_text(json.dumps({'processGroup': gui.pid, 'remaining': remaining}))
            if remaining:
                raise RuntimeError(f'GUI process group still alive: {remaining}')


if __name__ == '__main__':
    if len(sys.argv) == 3 and sys.argv[1] == '--resize':
        resize(sys.argv[2])
    elif len(sys.argv) == 2:
        run(sys.argv[1])
    else:
        raise SystemExit('usage: python3 code/anywork/tool/sidebar_native_harness.py OUTPUT_DIRECTORY')
