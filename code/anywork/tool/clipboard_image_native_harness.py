#!/usr/bin/env python3
"""Linux X11 acceptance for real clipboard image paste and DeepSeek vision."""

import argparse
import ctypes
import json
import os
from pathlib import Path
import re
import shutil
import signal
import subprocess
import sys
import tempfile
import time
try:
    import tomllib
except ModuleNotFoundError:
    import tomli as tomllib


def create_fixture(path):
    from PIL import Image, ImageDraw, ImageFont

    image = Image.new('RGB', (1100, 700), 'white')
    draw = ImageDraw.Draw(image)
    draw.ellipse((80, 120, 420, 460), fill=(225, 45, 45))
    draw.polygon([(650, 460), (850, 110), (1030, 460)], fill=(40, 95, 225))
    font_path = '/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf'
    font = ImageFont.truetype(font_path, 72)
    label = 'PURE-VISION-7429'
    bounds = draw.textbbox((0, 0), label, font=font)
    x = (image.width - (bounds[2] - bounds[0])) // 2
    draw.text((x, 560), label, fill='black', font=font)
    image.save(path, format='PNG')


def run_clipboard_owner(image_path, ready_path):
    import gi

    gi.require_version('Gtk', '3.0')
    gi.require_version('Gdk', '3.0')
    gi.require_version('GdkPixbuf', '2.0')
    from gi.repository import Gdk, GdkPixbuf, Gtk

    pixbuf = GdkPixbuf.Pixbuf.new_from_file(str(image_path))
    clipboard = Gtk.Clipboard.get(Gdk.SELECTION_CLIPBOARD)
    clipboard.set_image(pixbuf)
    ready_path.write_text(
        json.dumps({'pid': os.getpid(), 'image': str(image_path)}),
        encoding='utf-8',
    )
    Gtk.main()


class X11Controller:
    class XErrorEvent(ctypes.Structure):
        _fields_ = [
            ('type', ctypes.c_int),
            ('display', ctypes.c_void_p),
            ('resourceid', ctypes.c_ulong),
            ('serial', ctypes.c_ulong),
            ('error_code', ctypes.c_ubyte),
            ('request_code', ctypes.c_ubyte),
            ('minor_code', ctypes.c_ubyte),
        ]

    class XWindowAttributes(ctypes.Structure):
        _fields_ = [
            ('x', ctypes.c_int),
            ('y', ctypes.c_int),
            ('width', ctypes.c_int),
            ('height', ctypes.c_int),
            ('border_width', ctypes.c_int),
            ('depth', ctypes.c_int),
            ('visual', ctypes.c_void_p),
            ('root', ctypes.c_ulong),
            ('window_class', ctypes.c_int),
            ('bit_gravity', ctypes.c_int),
            ('win_gravity', ctypes.c_int),
            ('backing_store', ctypes.c_int),
            ('backing_planes', ctypes.c_ulong),
            ('backing_pixel', ctypes.c_ulong),
            ('save_under', ctypes.c_int),
            ('colormap', ctypes.c_ulong),
            ('map_installed', ctypes.c_int),
            ('map_state', ctypes.c_int),
            ('all_event_masks', ctypes.c_long),
            ('your_event_mask', ctypes.c_long),
            ('do_not_propagate_mask', ctypes.c_long),
            ('override_redirect', ctypes.c_int),
            ('screen', ctypes.c_void_p),
        ]

    def __init__(self, display_name):
        self.x11 = ctypes.CDLL('libX11.so.6')
        self.xtst = ctypes.CDLL('libXtst.so.6')
        self.display_name = display_name
        self.ptr = ctypes.c_void_p
        self.window = ctypes.c_ulong
        self.x_error = False
        self.x_error_detail = None
        self.error_handler_type = ctypes.CFUNCTYPE(
            ctypes.c_int,
            self.ptr,
            self.ptr,
        )
        self.error_handler = self.error_handler_type(self._handle_x_error)
        self._configure()

    def _handle_x_error(self, _display, _event):
        self.x_error = True
        event = ctypes.cast(
            _event,
            ctypes.POINTER(self.XErrorEvent),
        ).contents
        self.x_error_detail = {
            'errorCode': int(event.error_code),
            'requestCode': int(event.request_code),
            'minorCode': int(event.minor_code),
            'resourceId': int(event.resourceid),
        }
        return 0

    def _configure(self):
        x = self.x11
        ptr, window = self.ptr, self.window
        x.XOpenDisplay.argtypes = [ctypes.c_char_p]
        x.XOpenDisplay.restype = ptr
        x.XDefaultRootWindow.argtypes = [ptr]
        x.XDefaultRootWindow.restype = window
        x.XQueryTree.argtypes = [
            ptr,
            window,
            ctypes.POINTER(window),
            ctypes.POINTER(window),
            ctypes.POINTER(ctypes.POINTER(window)),
            ctypes.POINTER(ctypes.c_uint),
        ]
        x.XFetchName.argtypes = [ptr, window, ctypes.POINTER(ptr)]
        x.XGetWindowAttributes.argtypes = [
            ptr,
            window,
            ctypes.POINTER(self.XWindowAttributes),
        ]
        x.XFree.argtypes = [ptr]
        x.XRaiseWindow.argtypes = [ptr, window]
        x.XMapRaised.argtypes = [ptr, window]
        x.XResizeWindow.argtypes = [ptr, window, ctypes.c_uint, ctypes.c_uint]
        x.XSetInputFocus.argtypes = [ptr, window, ctypes.c_int, ctypes.c_ulong]
        x.XGetInputFocus.argtypes = [
            ptr,
            ctypes.POINTER(window),
            ctypes.POINTER(ctypes.c_int),
        ]
        x.XKeysymToKeycode.argtypes = [ptr, ctypes.c_ulong]
        x.XKeysymToKeycode.restype = ctypes.c_ubyte
        x.XSync.argtypes = [ptr, ctypes.c_int]
        x.XCloseDisplay.argtypes = [ptr]
        x.XSetErrorHandler.argtypes = [self.error_handler_type]
        x.XSetErrorHandler(self.error_handler)
        self.xtst.XTestFakeKeyEvent.argtypes = [
            ptr,
            ctypes.c_uint,
            ctypes.c_int,
            ctypes.c_ulong,
        ]

    def _identity(self, connection, window):
        name = self.ptr()
        if self.x11.XFetchName(connection, window, ctypes.byref(name)) and name:
            try:
                return ctypes.string_at(name).decode('utf-8', errors='replace')
            finally:
                self.x11.XFree(name)
        return ''

    def _find_anywork(self, connection):
        root = self.x11.XDefaultRootWindow(connection)
        queue = [root]
        seen = []
        candidates = []
        while queue:
            current = queue.pop()
            identity = self._identity(connection, current)
            if identity:
                seen.append(identity)
            if 'anywork' in identity.lower() or '糊来帮' in identity:
                attributes = self.XWindowAttributes()
                if self.x11.XGetWindowAttributes(
                    connection,
                    current,
                    ctypes.byref(attributes),
                ):
                    candidates.append(
                        (
                            attributes.map_state == 2,
                            attributes.width * attributes.height,
                            current,
                        )
                    )
            returned_root = self.window()
            parent = self.window()
            children = ctypes.POINTER(self.window)()
            count = ctypes.c_uint()
            if self.x11.XQueryTree(
                connection,
                current,
                ctypes.byref(returned_root),
                ctypes.byref(parent),
                ctypes.byref(children),
                ctypes.byref(count),
            ):
                queue.extend(children[index] for index in range(count.value))
                if children:
                    self.x11.XFree(children)
        if candidates:
            candidates.sort(reverse=True)
            return candidates[0][2]
        raise RuntimeError(f'Anywork window was not found; titles={seen}')

    def focus_and_resize(self):
        connection = self.x11.XOpenDisplay(self.display_name.encode())
        if not connection:
            raise RuntimeError(f'cannot open X display {self.display_name}')
        try:
            window = self._find_anywork(connection)
            self.x11.XResizeWindow(connection, window, 1180, 900)
            self.x11.XMapRaised(connection, window)
            self.x11.XSync(connection, 0)
        finally:
            self.x11.XCloseDisplay(connection)

    def send_control_v(self):
        connection = self.x11.XOpenDisplay(self.display_name.encode())
        if not connection:
            raise RuntimeError(f'cannot open X display {self.display_name}')
        try:
            window = self._find_anywork(connection)
            self.x_error = False
            self.x_error_detail = None
            self.x11.XMapRaised(connection, window)
            self.x11.XSync(connection, 0)
            time.sleep(0.05)
            self.x11.XSetInputFocus(connection, window, 2, 0)
            focused = self.window()
            revert_to = ctypes.c_int()
            self.x11.XGetInputFocus(
                connection,
                ctypes.byref(focused),
                ctypes.byref(revert_to),
            )
            control = self.x11.XKeysymToKeycode(connection, 0xFFE3)
            key_v = self.x11.XKeysymToKeycode(connection, ord('v'))
            for keycode, pressed in (
                (control, 1),
                (key_v, 1),
                (key_v, 0),
                (control, 0),
            ):
                if not self.xtst.XTestFakeKeyEvent(
                    connection,
                    keycode,
                    pressed,
                    30,
                ):
                    raise RuntimeError('XTestFakeKeyEvent failed')
            self.x11.XSync(connection, 0)
            if self.x_error:
                raise RuntimeError(
                    'X11 rejected focus or keyboard injection: '
                    f'{self.x_error_detail}'
                )
            return {
                'targetWindow': int(window),
                'focusedWindow': int(focused.value),
                'revertTo': revert_to.value,
                'controlKeycode': int(control),
                'vKeycode': int(key_v),
            }
        finally:
            self.x11.XCloseDisplay(connection)


def wait_for_path(path, process, timeout, description):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        if path.exists() and path.stat().st_size > 0:
            return
        if process is not None and process.poll() is not None:
            raise RuntimeError(
                f'{description} exited with code {process.returncode}'
            )
        time.sleep(0.1)
    raise TimeoutError(f'timed out waiting for {description}')


def terminate(process, process_group=False):
    if process is None or process.poll() is not None:
        return
    try:
        if process_group:
            os.killpg(process.pid, signal.SIGTERM)
        else:
            process.terminate()
    except ProcessLookupError:
        return
    try:
        process.wait(timeout=15)
    except subprocess.TimeoutExpired:
        if process_group:
            os.killpg(process.pid, signal.SIGKILL)
        else:
            process.kill()
        process.wait(timeout=5)


def process_group_members(group_id):
    members = []
    for entry in Path('/proc').iterdir():
        if not entry.name.isdigit():
            continue
        try:
            if os.getpgid(int(entry.name)) == group_id:
                members.append(int(entry.name))
        except (ProcessLookupError, PermissionError):
            pass
    return members


def validate_config(config_path):
    with config_path.open('rb') as source:
        config = tomllib.load(source)
    provider = config.get('models', {}).get('providers', {}).get('deepseek')
    if not isinstance(provider, dict):
        raise RuntimeError('current config has no DeepSeek provider')
    for role in ('executor', 'explorer', 'planner', 'reviewer', 'worktree_executor'):
        route = config.get('models', {}).get('routes', {}).get(role, {})
        if route.get('provider') != 'deepseek' or route.get('model') != 'deepseek-flash':
            raise RuntimeError(
                f'current config route {role} is not deepseek/deepseek-flash'
            )


def normalize_temporary_config(config_path):
    content = config_path.read_text(encoding='utf-8')
    old = (
        '[models.providers.zhipu-coding-plan.catalog]\n'
        'source = "bundled"\n'
        'catalog = "zhipu"'
    )
    new = (
        '[models.providers.zhipu-coding-plan.catalog]\n'
        'source = "bundled"\n'
        'catalog = "zhipu-responses"'
    )
    if old not in content:
        return []
    config_path.write_text(content.replace(old, new, 1), encoding='utf-8')
    return [
        'temporary copy: zhipu-coding-plan catalog zhipu -> zhipu-responses'
    ]


def parse_vm_service(log_path):
    text = log_path.read_text(encoding='utf-8', errors='replace')
    matches = re.findall(
        r'A Dart VM Service .*?available at: (http://127\.0\.0\.1:\d+/[^\s]*)',
        text,
    )
    return matches[-1] if matches else None


def run(output_path):
    root = Path(__file__).resolve().parents[3]
    output = Path(output_path).resolve()
    output.mkdir(parents=True, exist_ok=False)
    source_config = Path.home() / '.anywork' / 'config.toml'
    validate_config(source_config)

    fixture = output / 'clipboard-image.png'
    create_fixture(fixture)
    workspace = Path(tempfile.mkdtemp(prefix='anywork-clipboard-project-'))
    home = Path(tempfile.mkdtemp(prefix='anywork-clipboard-home-'))
    temporary_config = home / 'config.toml'
    shutil.copy2(source_config, temporary_config)
    config_adjustments = normalize_temporary_config(temporary_config)
    (workspace / 'README.md').write_text(
        '# Temporary clipboard vision project\n',
        encoding='utf-8',
    )
    (output / 'fixture.json').write_text(
        json.dumps(
            {
                'workspace': str(workspace),
                'temporaryHome': str(home),
                'fixture': str(fixture),
                'marker': 'PURE-VISION-7429',
                'temporaryConfigAdjustments': config_adjustments,
            },
            ensure_ascii=False,
            indent=2,
        ),
        encoding='utf-8',
    )

    display_file = output / 'display.txt'
    auth_file = output / 'xauthority-path.txt'
    owner_ready = output / 'clipboard-owner-ready.json'
    paste_signal = output / 'paste-ready.json'
    native_events = output / 'native-events.jsonl'
    gui_log_path = output / 'gui.log'
    driver_log_path = output / 'driver.log'
    owner_log_path = output / 'clipboard-owner.log'
    gui = owner = driver = None
    original_error = None
    real_home = Path.home()
    env = dict(
        os.environ,
        ANYWORK_HOME=str(home),
        HOME=str(home),
        USERPROFILE=str(home),
        CARGO_HOME=os.environ.get('CARGO_HOME', str(real_home / '.cargo')),
        RUSTUP_HOME=os.environ.get('RUSTUP_HOME', str(real_home / '.rustup')),
        PUB_CACHE=os.environ.get('PUB_CACHE', str(real_home / '.pub-cache')),
        GDK_BACKEND='x11',
        WAYLAND_DISPLAY='',
        LIBGL_ALWAYS_SOFTWARE='1',
    )
    try:
        with gui_log_path.open('w', encoding='utf-8') as gui_log:
            gui = subprocess.Popen(
                [
                    'xvfb-run',
                    '-a',
                    '-s',
                    '-screen 0 1440x1000x24',
                    'bash',
                    '-c',
                    'printenv DISPLAY > "$1"; printenv XAUTHORITY > "$2"; '
                    'exec cargo xtask run-gui --driver --log-level info',
                    'clipboard-native',
                    str(display_file),
                    str(auth_file),
                ],
                cwd=root,
                env=env,
                stdout=gui_log,
                stderr=subprocess.STDOUT,
                start_new_session=True,
            )
            (output / 'process.json').write_text(
                json.dumps({'guiProcessGroup': gui.pid}),
                encoding='utf-8',
            )
            deadline = time.monotonic() + 600
            vm_service = None
            while time.monotonic() < deadline:
                if gui.poll() is not None:
                    raise RuntimeError(
                        f'GUI exited with code {gui.returncode}; see gui.log'
                    )
                vm_service = parse_vm_service(gui_log_path)
                if vm_service and display_file.exists() and auth_file.exists():
                    break
                time.sleep(0.2)
            if vm_service is None:
                raise TimeoutError('native GUI did not publish its VM service')

        display = display_file.read_text(encoding='utf-8').strip()
        xauthority = auth_file.read_text(encoding='utf-8').strip()
        child_env = dict(env, DISPLAY=display, XAUTHORITY=xauthority)
        os.environ['DISPLAY'] = display
        os.environ['XAUTHORITY'] = xauthority
        x11 = X11Controller(display)
        x11.focus_and_resize()
        with (output / 'xwininfo-tree.txt').open('w', encoding='utf-8') as tree:
            subprocess.run(
                ['xwininfo', '-root', '-tree'],
                env=child_env,
                stdout=tree,
                stderr=subprocess.STDOUT,
                check=True,
            )

        with owner_log_path.open('w', encoding='utf-8') as owner_log:
            owner = subprocess.Popen(
                [
                    sys.executable,
                    str(Path(__file__).resolve()),
                    '--clipboard-owner',
                    str(fixture),
                    str(owner_ready),
                ],
                env=child_env,
                stdout=owner_log,
                stderr=subprocess.STDOUT,
            )
        wait_for_path(owner_ready, owner, 30, 'GTK clipboard owner')

        driver_command = [
            'cargo',
            'dart',
            'run',
            'test_driver/multimodal_acceptance_driver.dart',
            '--vm-service-url',
            vm_service,
            '--workspace',
            str(workspace),
            '--snapshot-output',
            str(output / 'snapshots.jsonl'),
            '--model-screenshot-output',
            str(output / 'model.png'),
            '--preview-screenshot-output',
            str(output / 'pasted.png'),
            '--removed-screenshot-output',
            str(output / 'removed.png'),
            '--screenshot-output',
            str(output / 'completed.png'),
            '--provider-id',
            'deepseek',
            '--expected-model',
            'deepseek-flash',
            '--expected-marker',
            'PURE-VISION-7429',
            '--expected-answer-pattern',
            r'^(?=.*PURE-VISION-7429)(?=.*(?:圆|circle))(?=.*(?:三角|triangle)).*$',
            '--expected-filename',
            'clipboard-image.png',
            '--prompt',
            '识别图片中的文字和主要图形。请分别说明文字内容、图形形状和颜色。',
            '--turn-timeout-seconds',
            '900',
            '--paste-signal-output',
            str(paste_signal),
        ]
        with driver_log_path.open('w', encoding='utf-8') as driver_log:
            driver = subprocess.Popen(
                driver_command,
                cwd=root,
                env=child_env,
                stdout=driver_log,
                stderr=subprocess.STDOUT,
            )
            handled = 0
            deadline = time.monotonic() + 1200
            while driver.poll() is None:
                if time.monotonic() >= deadline:
                    raise TimeoutError('native clipboard Driver exceeded 1200 seconds')
                if paste_signal.exists():
                    try:
                        signal_state = json.loads(
                            paste_signal.read_text(encoding='utf-8')
                        )
                    except (json.JSONDecodeError, OSError):
                        signal_state = {}
                    sequence = signal_state.get('sequence')
                    if isinstance(sequence, int) and sequence > handled:
                        time.sleep(0.3)
                        injection = x11.send_control_v()
                        handled = sequence
                        with native_events.open('a', encoding='utf-8') as events:
                            events.write(
                                json.dumps(
                                    {
                                        'sequence': sequence,
                                        'action': 'XTest Ctrl+V',
                                        'capturedAt': time.time(),
                                        **injection,
                                    }
                                )
                                + '\n'
                            )
                time.sleep(0.1)
        if driver.returncode != 0:
            raise RuntimeError(
                f'clipboard Driver exited with code {driver.returncode}; '
                'see driver.log'
            )
        if handled != 2:
            raise RuntimeError(f'expected two native paste injections, got {handled}')

        runtime_log = gui_log_path.read_text(encoding='utf-8', errors='replace')
        for studio_logs in (
            home / 'studio' / 'logs',
            home / '.anywork' / 'studio' / 'logs',
        ):
            if studio_logs.is_dir():
                for log_path in studio_logs.glob('*.log'):
                    runtime_log += log_path.read_text(
                        encoding='utf-8',
                        errors='replace',
                    )
        if (
            'planned media request representation' not in runtime_log
            or 'representation="providerFile"' not in runtime_log
        ):
            raise RuntimeError(
                'runtime logs did not confirm providerFile media planning'
            )
    except BaseException as error:
        original_error = error
        raise
    finally:
        terminate(driver)
        terminate(owner)
        gui_group = gui.pid if gui is not None else None
        terminate(gui, process_group=True)
        remaining = []
        if gui_group is not None:
            deadline = time.monotonic() + 5
            while True:
                remaining = process_group_members(gui_group)
                if not remaining or time.monotonic() >= deadline:
                    break
                time.sleep(0.1)
        for studio_logs in (
            home / 'studio' / 'logs',
            home / '.anywork' / 'studio' / 'logs',
        ):
            if studio_logs.is_dir():
                shutil.copytree(
                    studio_logs,
                    output / 'studio-logs',
                    dirs_exist_ok=True,
                )
        shutil.rmtree(workspace, ignore_errors=True)
        shutil.rmtree(home, ignore_errors=True)
        cleanup = {
            'guiProcessGroup': gui_group,
            'remainingProcessGroupMembers': remaining,
            'temporaryWorkspaceExists': workspace.exists(),
            'temporaryHomeExists': home.exists(),
            'clipboardOwnerAlive': owner is not None and owner.poll() is None,
            'driverAlive': driver is not None and driver.poll() is None,
            'originalError': None if original_error is None else str(original_error),
        }
        (output / 'cleanup.json').write_text(
            json.dumps(cleanup, ensure_ascii=False, indent=2),
            encoding='utf-8',
        )
        if original_error is None and remaining:
            raise RuntimeError(f'GUI process group still alive: {remaining}')


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('output', nargs='?')
    parser.add_argument(
        '--clipboard-owner',
        nargs=2,
        metavar=('IMAGE', 'READY'),
    )
    args = parser.parse_args()
    if args.clipboard_owner:
        run_clipboard_owner(
            Path(args.clipboard_owner[0]),
            Path(args.clipboard_owner[1]),
        )
        return
    if not args.output:
        parser.error('OUTPUT is required')
    run(args.output)


if __name__ == '__main__':
    main()
