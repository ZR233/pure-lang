#!/usr/bin/env python3
"""Isolated Linux native acceptance for a real local Project + Thread chain.

Owner: W6-3 GUI/protocol/runtime stream acceptance. This is an opt-in, non-demo
entry point that drives the real FRB bridge and the real model adapter against a
scripted loopback provider. It is *evidence collection*, not a fixture that
pretends to be persistence:

* the Studio home is isolated under the output directory (`ANYWORK_HOME`), so the
  operator's `~/.anywork` is never read for state nor written;
* the workspace is a fresh `tempfile.mkdtemp` directory;
* the scripted provider is a local SSE server; every request is indexed in
  `wire-index.jsonl`, and bodies are kept in full except where long-session
  compaction replaces near-duplicate replays with a compact record;
* phase 1 creates a Project + Thread through real GUI interaction, sends prompts,
  navigates the bounded history window and captures the live full body;
* the GUI is cleanly shut down (`shutdown-await`), every process is reaped, and
  phase 2 restarts the same Studio home, reopens the saved Thread and inspects
  the persisted preview / full-body retrieval and recovery settle.

The Studio home is deliberately left in place so the operator can inspect the
canonical artifacts itself:

  <output>/studio-home/catalog.toml
  <output>/studio-home/sessions/<thread-storage-key>/state.toml
  <output>/studio-home/sessions/<thread-storage-key>/history.sqlite
  <output>/studio-home/calls/calls.sqlite

`<thread-storage-key>` is the lowercase hex SHA-256 of the Thread id
(`StudioPaths::thread_storage_key`); it is not the raw Thread id.

No automatic pass/fail is derived from a mock or a fabricated database.

Usage:

  python3 code/anywork/tool/project_session_native_harness.py OUTPUT_DIRECTORY

Requires `xvfb-run` and the repository's `cargo xtask run-gui --driver` support,
exactly like the existing timeline/sidebar native harnesses.

Coverage notes (honest gaps to hand back with the evidence):

* the default two-Turn path drives a shallow window (a handful of items): it
  asserts the budgets and both pagination ends but cannot reach past one window;
  deep pagination is covered by the opt-in `--long-session` mode, whose create
  phase hard-requires paging back to the oldest durable SQL page. Live window
  eviction raising `hasOlder`/`olderCursor` is production behaviour (reducer
  `_liveEvictionHistory`), not a recorded observation; the driver's `findings`
  channel remains a generic place for truthful limitations;
* the persistence queue-pressure panel (`persistence-queue-diagnostics`) renders
  only while a save is degraded/backlogged, which the scripted provider cannot
  induce; the canonical `persistence` snapshot is recorded instead;
* the persisted full body is read through the visible per-item affordance in
  both phases (load, or retry if a retrieval actually failed); a clean run
  records `affordance: load`, and `timeline-item-body-retry-*` is only exercised
  when a retrieval fails.
* during streaming the client keeps only a bounded tail preview of an oversized
  body (`kTimelineItemBodyBudget`), so a live preview may still contain the
  trailing sentinel; identification and the full-body proof therefore use the
  window's `previewedItemIds` / `loadedItemIds` identity lists instead of
  scanning rendered text.
* expanding the persisted full body makes one rendered row ~250 KiB tall, so the
  topmost visible row id necessarily changes while the reader stays anchored.
  Reading-position stability is therefore judged by `readingPositionStable`
  (`followingBottom` preserved, the target row still in the window, or the same
  anchor identity) in `reopen.json`; the raw `anchorItemStable` field is kept
  only as an observation and is expected to be `false` for that expansion.

Opt-in deep-history mode (default off, so the fast two-Turn path is unchanged):

  python3 code/anywork/tool/project_session_native_harness.py OUTPUT --long-session

`--long-session` submits `--turns` (default 132, minimum 130) short scripted
Turns through the same real GUI before the large-body Turn, so the durable
session holds well over one 500-item bounded history window. Both phases then
walk the real scroll/pagination path to the oldest page and back to the newest,
asserting that previously nonresident canonical items become reachable and that
the exact first/latest item and Turn identities appear at the window
boundaries, while the window/cache/live-tail budgets stay enforced at every
step. `session.json` / `reopen.json` carry the compact per-Turn progression,
the page transitions, the cold-reopen outcome and the provider request guard
that proves opening a saved Thread did not re-run a prior model. This mode
still collects evidence only: no pass/fail is derived from a mock or from a
fabricated database, and the operator inspects the SQLite/TOML artifacts.

Long sessions are expensive (one real GUI Turn each), so they never run by
default; `--driver-timeout` defaults to 5400 s per phase in that mode.
"""

import argparse
import http.server
import json
import os
from pathlib import Path
import re
import shutil
import signal
import subprocess
import sys
import tempfile
import threading
import time


USER_HOME_MARKER = Path(os.path.expanduser('~')) / '.anywork'
# Just above pl_protocol's TIMELINE_ITEM_PREVIEW_BYTES (256 KiB) so the item is
# previewed when a SQL page, not the live stream, produces the row.
LARGE_BODY_TARGET = 256 * 1024 + 4 * 1024
LARGE_BODY_SENTINEL = 'NATIVE_ACCEPT_LARGE_END'
# Long-session mode: at four items per Turn, 130 Turns already exceed the
# 500-item bounded window that the GUI keeps resident.
DEFAULT_LONG_TURNS = 132
MIN_LONG_TURNS = 130
# Full request bodies are only kept for this many requests in long-session mode;
# later requests keep a compact index entry instead of near-duplicate replays.
LONG_WIRE_BODY_LIMIT = 2
SHORT_DRIVER_TIMEOUT = 1200
LONG_DRIVER_TIMEOUT = 5400


def _large_body():
    """A single assistant body just above the per-item preview byte budget."""
    lines = []
    total = 0
    index = 0
    while total < LARGE_BODY_TARGET:
        line = f'{index:05d} native-accept large-body padding line\n'
        lines.append(line)
        total += len(line)
        index += 1
    lines.append(LARGE_BODY_SENTINEL + '\n')
    return ''.join(lines)


class Provider(http.server.BaseHTTPRequestHandler):
    """Scripted OpenAI-compatible provider used only by this acceptance run."""

    output = None
    index_path = None
    # None keeps every recorded request body in full (two-Turn path). A number
    # caps the full bodies so a 130+ Turn session does not write hundreds of
    # near-duplicate replayed conversations; the wire index still records every
    # request.
    full_body_limit = None
    sequence = 0
    lock = threading.Lock()

    def log_message(self, *args):
        pass

    def do_GET(self):
        if self.path == '/release':
            self.send_response(200)
            self.end_headers()
            self.wfile.write(b'ok')
        elif self.path == '/health':
            self.send_response(200)
            self.end_headers()
            self.wfile.write(b'ok')
        else:
            self.send_error(404)

    def do_POST(self):
        raw = self.rfile.read(int(self.headers['Content-Length']))
        body = json.loads(raw)
        with type(self).lock:
            type(self).sequence += 1
            seq = type(self).sequence
        responses = 'input' in body
        messages = body.get('input', []) if responses else body.get('messages', [])
        users = [
            message.get('content')
            for message in messages
            if isinstance(message, dict) and message.get('role') == 'user'
        ]
        # Only the newest user message decides the answer; earlier turns are replayed.
        prompt = json.dumps(users[-1], ensure_ascii=False) if users else ''
        large = 'NATIVE_ACCEPT_LARGE' in prompt
        self._record_request(seq, body, raw, users, prompt, large)

        self.send_response(200)
        self.send_header('Content-Type', 'text/event-stream')
        self.end_headers()
        text_parts = []

        def event(value):
            self.wfile.write(
                ('data: ' + json.dumps(value, ensure_ascii=False) + '\n\n').encode()
            )
            self.wfile.flush()

        def send(delta, finish=None):
            if not responses:
                event(
                    {
                        'id': f'fixture-{seq}',
                        'object': 'chat.completion.chunk',
                        'model': body.get('model'),
                        'choices': [
                            {
                                'index': 0,
                                'delta': delta,
                                'finish_reason': finish,
                            }
                        ],
                    }
                )
                return
            if 'content' in delta:
                if not text_parts:
                    event(
                        {
                            'type': 'response.output_item.added',
                            'item': {
                                'id': f'answer-{seq}',
                                'type': 'message',
                                'role': 'assistant',
                            },
                        }
                    )
                text_parts.append(delta['content'])
                event(
                    {
                        'type': 'response.output_text.delta',
                        'item_id': f'answer-{seq}',
                        'delta': delta['content'],
                    }
                )
            for call in delta.get('tool_calls', []):
                event(
                    {
                        'type': 'response.output_item.done',
                        'item': {
                            'id': call['id'],
                            'type': 'function_call',
                            'call_id': call['id'],
                            **call['function'],
                        },
                    }
                )
            if finish:
                if text_parts:
                    event(
                        {
                            'type': 'response.output_item.done',
                            'item': {
                                'id': f'answer-{seq}',
                                'type': 'message',
                                'role': 'assistant',
                                'content': [
                                    {
                                        'type': 'output_text',
                                        'text': ''.join(text_parts),
                                    }
                                ],
                            },
                        }
                    )
                event(
                    {
                        'type': 'response.completed',
                        'response': {
                            'id': f'fixture-{seq}',
                            'usage': {
                                'input_tokens': 5,
                                'output_tokens': 3,
                                'total_tokens': 8,
                            },
                        },
                    }
                )

        try:
            if large:
                # One delta keeps the deterministic huge body cheap to lay out.
                send({'content': _large_body()})
            else:
                send({'content': 'NATIVE_ACCEPT_TURN_ACK\n'})
            send({}, 'stop')
            self.wfile.write(b'data: [DONE]\n\n')
            self.wfile.flush()
        except (BrokenPipeError, ConnectionResetError):
            pass

    def _record_request(self, seq, body, raw, users, prompt, large):
        """Records bounded request evidence plus one wire-index line.

        The index is what a driver or operator can count without re-reading
        hundreds of replayed conversations; full bodies are only kept while
        `full_body_limit` allows it (and always for the large-body prompt).
        """
        cls = type(self)
        limit = cls.full_body_limit
        keep_body = limit is None or seq <= limit or large
        entry = {
            'seq': seq,
            'kind': 'large' if large else 'ack',
            'bytes': len(raw),
            'userMessages': len(users),
            'lastUserChars': len(prompt),
            'bodyWritten': keep_body,
        }
        with cls.lock:
            if cls.index_path is not None:
                with cls.index_path.open('a', encoding='utf-8') as index:
                    index.write(json.dumps(entry, ensure_ascii=False) + '\n')
            (cls.output / f'wire-{seq:04d}.json').write_text(
                json.dumps(body, ensure_ascii=False)
                if keep_body
                else json.dumps(entry, ensure_ascii=False),
                encoding='utf-8',
            )


def _config(url):
    lines = [
        'schema_version = 20',
        '[runtime]',
        'permission_mode = "full-access"',
        '[models.providers.deepseek]',
        'name = "Native acceptance scripted provider"',
        'preset = "deepseek"',
        f'base_url = "{url}"',
        '[models.providers.deepseek.catalog]',
        'source = "bundled"',
        'catalog = "deepseek"',
    ]
    for mode in ('mode.simple', 'mode.task'):
        lines += [
            f'[mode_model_routes."{mode}"]',
            'provider = "deepseek"',
            'model = "deepseek-flash"',
            'effort = "high"',
        ]
    for role in ('explorer', 'executor', 'worktree_executor', 'reviewer'):
        lines += [
            f'[models.routes.{role}]',
            'provider = "deepseek"',
            'model = "deepseek-flash"',
            'effort = "high"',
        ]
    return '\n'.join(lines) + '\n'


def _home_marker_state():
    try:
        stat = USER_HOME_MARKER.stat()
    except OSError:
        return {'path': str(USER_HOME_MARKER), 'exists': False}
    return {
        'path': str(USER_HOME_MARKER),
        'exists': True,
        'mtimeNs': stat.st_mtime_ns,
        'size': stat.st_size,
    }


def _cleanup_evidence(output):
    """Process-tree cleanup facts recorded by `_reap` for every phase."""
    evidence = {}
    for name in ('create', 'reopen'):
        candidate = output / f'cleanup-{name}.json'
        if candidate.exists():
            payload = json.loads(candidate.read_text())
            evidence[name] = {
                'processGroup': payload.get('processGroup'),
                'remaining': payload.get('remaining'),
                'released': not payload.get('remaining'),
            }
    return evidence


def _wire_index_summary(output):
    """Compact provider-request summary; never the replayed bodies themselves."""
    path = output / 'wire-index.jsonl'
    if not path.exists():
        return {'available': False}
    total = 0
    conversation = 0
    large = 0
    total_bytes = 0
    for line in path.read_text().splitlines():
        line = line.strip()
        if not line:
            continue
        try:
            entry = json.loads(line)
        except ValueError:
            # A partially flushed final line from a live provider is not evidence.
            continue
        if not isinstance(entry, dict):
            continue
        total += 1
        total_bytes += int(entry.get('bytes', 0))
        if int(entry.get('userMessages', 0)) > 0:
            conversation += 1
        if entry.get('kind') == 'large':
            large += 1
    return {
        'available': True,
        'requests': total,
        'conversationRequests': conversation,
        'largeBodyRequests': large,
        'requestBytes': total_bytes,
    }


def _await_vm(log_path, gui, deadline_seconds=900):
    deadline = time.monotonic() + deadline_seconds
    # The verbose driver log grows without bound; read it incrementally (keeping
    # a bounded tail for split lines) instead of re-reading the whole file.
    tail = ''
    with log_path.open('r', errors='replace') as stream:
        while time.monotonic() < deadline:
            chunk = stream.read()
            if chunk:
                combined = tail + chunk
                matches = re.findall(
                    r'A Dart VM Service .*?available at: '
                    r'(http://127\.0\.0\.1:\d+/[^\s]*)',
                    combined,
                )
                if matches:
                    return matches[-1]
                tail = combined[-262144:]
            if gui.poll() is not None:
                raise RuntimeError(
                    f'GUI exited with {gui.returncode}; see {log_path.name}'
                )
            time.sleep(0.2)
    raise TimeoutError(f'native GUI did not publish a VM service; see {log_path.name}')


def _launch(root, output, env, phase, size):
    log_path = output / f'gui-{phase}.log'
    log = log_path.open('w')
    gui = subprocess.Popen(
        [
            'xvfb-run', '-a', '-s', f'-screen 0 {size}',
            'cargo', 'xtask', 'run-gui', '--driver',
        ],
        cwd=root,
        env=env,
        stdout=log,
        stderr=subprocess.STDOUT,
        start_new_session=True,
    )
    (output / f'process-{phase}.json').write_text(
        json.dumps({'processGroup': gui.pid, 'phase': phase})
    )
    try:
        vm = _await_vm(log_path, gui)
    except BaseException:
        # The GUI never published a VM service (build failure, missing display,
        # ...): release the whole tree before surfacing the error.
        _reap(gui, log, output, phase)
        raise
    return gui, log, vm


def _reap(gui, log, output, phase):
    try:
        os.killpg(gui.pid, signal.SIGTERM)
    except ProcessLookupError:
        pass
    try:
        gui.wait(timeout=15)
    except subprocess.TimeoutExpired:
        os.killpg(gui.pid, signal.SIGKILL)
        gui.wait(timeout=5)
    remaining = []
    deadline = time.monotonic() + 5
    while True:
        remaining = []
        for proc in Path('/proc').iterdir():
            if not proc.name.isdigit():
                continue
            try:
                if os.getpgid(int(proc.name)) == gui.pid:
                    remaining.append(int(proc.name))
            except ProcessLookupError:
                pass
        if not remaining or time.monotonic() >= deadline:
            break
        time.sleep(0.1)
    log.close()
    (output / f'cleanup-{phase}.json').write_text(
        json.dumps({'processGroup': gui.pid, 'remaining': remaining})
    )
    if remaining:
        raise RuntimeError(f'GUI descendants survived cleanup after {phase}: {remaining}')


def _run_driver(root, env, output, phase, vm, workspace, url, timeout, turns=0):
    command = [
        'cargo', 'dart', 'run',
        'test_driver/project_session_acceptance_driver.dart',
        '--phase', phase,
        '--vm-service-url', vm,
        '--output', str(output),
        '--workspace', str(workspace),
        '--provider-url', url,
    ]
    if turns > 0:
        command += ['--turns', str(turns)]
    with (output / f'driver-{phase}.log').open('w') as log:
        subprocess.run(
            command,
            cwd=root,
            env=env,
            stdout=log,
            stderr=subprocess.STDOUT,
            check=True,
            timeout=timeout,
        )


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('output')
    parser.add_argument(
        '--display',
        default='1600x1000x24',
        help='isolated Xvfb screen spec handed to xvfb-run',
    )
    parser.add_argument(
        '--long-session',
        action='store_true',
        help=(
            'opt-in deep-history mode: submit enough short Turns to exceed one '
            '500-item bounded history window before walking the pages'
        ),
    )
    parser.add_argument(
        '--turns',
        type=int,
        default=DEFAULT_LONG_TURNS,
        help=(
            f'long-session Turn count (>= {MIN_LONG_TURNS}); '
            'requires --long-session'
        ),
    )
    parser.add_argument(
        '--driver-timeout',
        type=int,
        default=None,
        help=(
            'per-phase driver timeout in seconds '
            f'(defaults to {SHORT_DRIVER_TIMEOUT}, or {LONG_DRIVER_TIMEOUT} '
            'in long-session mode)'
        ),
    )
    args = parser.parse_args()
    if args.long_session:
        if args.turns < MIN_LONG_TURNS:
            parser.error(
                f'--turns must be >= {MIN_LONG_TURNS} in long-session mode'
            )
    elif args.turns != DEFAULT_LONG_TURNS:
        parser.error('--turns requires --long-session')
    turns = args.turns if args.long_session else 0
    timeout = args.driver_timeout or (
        LONG_DRIVER_TIMEOUT if args.long_session else SHORT_DRIVER_TIMEOUT
    )
    mode = 'long-session' if args.long_session else 'two-turn'

    root = Path(__file__).resolve().parents[3]
    output = Path(args.output).resolve()
    output.mkdir(parents=True, exist_ok=True)
    home = output / 'studio-home'
    # A fresh Studio home is required; the output directory itself may be reused.
    home.mkdir()
    workspace = Path(tempfile.mkdtemp(prefix='pure-native-project-workspace-'))
    user_home_before = _home_marker_state()
    (output / 'fixture.json').write_text(
        json.dumps(
            {
                'workspace': str(workspace),
                'studioHome': str(home),
                'preservedUserHome': str(USER_HOME_MARKER),
                'mode': mode,
                'requestedTurns': turns or 2,
                'driverTimeoutSeconds': timeout,
                'wireBodyLimit': (
                    LONG_WIRE_BODY_LIMIT if args.long_session else None
                ),
            }
        )
    )

    Provider.output = output
    # The wire index is written for both modes: the long-session driver uses it
    # to prove a cold reopen did not re-run a prior model conversation.
    Provider.index_path = output / 'wire-index.jsonl'
    Provider.index_path.write_text('')
    Provider.full_body_limit = (
        LONG_WIRE_BODY_LIMIT if args.long_session else None
    )
    server = http.server.ThreadingHTTPServer(('127.0.0.1', 0), Provider)
    serving = threading.Thread(target=server.serve_forever, daemon=True)
    serving.start()
    url = f'http://127.0.0.1:{server.server_port}'
    (home / 'config.toml').write_text(_config(url))
    env = dict(
        os.environ,
        ANYWORK_HOME=str(home),
        GDK_BACKEND='x11',
        WAYLAND_DISPLAY='',
        LIBGL_ALWAYS_SOFTWARE='1',
    )
    # Non-demo acceptance: never inherit a demo switch from the caller's shell.
    env.pop('ANYWORK_DEMO', None)

    phases = []
    failure = None
    try:
        for phase in ('create', 'reopen'):
            if phase == 'reopen':
                # Capture provider traffic before launching the GUI: the
                # selected Thread can open before the driver connects.
                wire = _wire_index_summary(output)
                (output / 'reopen-wire-baseline.json').write_text(
                    json.dumps({
                        'available': wire['available'],
                        'total': wire.get('requests', 0),
                        'conversation': wire.get('conversationRequests', 0),
                    })
                )
            gui, log, vm = _launch(root, output, env, phase, args.display)
            try:
                _run_driver(
                    root,
                    env,
                    output,
                    phase,
                    vm,
                    workspace,
                    url,
                    timeout=timeout,
                    turns=turns,
                )
            finally:
                _reap(gui, log, output, phase)
            phases.append(phase)
    except BaseException as error:
        failure = repr(error)
        raise
    finally:
        server.shutdown()
        server.server_close()
        serving.join(timeout=5)
        shutil.rmtree(workspace, ignore_errors=True)
        (output / 'home-preservation.json').write_text(
            json.dumps(
                {
                    'preservedUserHome': str(USER_HOME_MARKER),
                    'before': user_home_before,
                    'after': _home_marker_state(),
                }
            )
        )
        if failure is not None:
            (output / 'result.json').write_text(
                json.dumps(
                    {
                        'result': 'failed',
                        'error': failure,
                        'mode': mode,
                        'requestedTurns': turns or 2,
                        'phases': phases,
                        'studioHome': str(home),
                        'providerUrl': url,
                        'cleanup': _cleanup_evidence(output),
                        'wireIndex': _wire_index_summary(output),
                    },
                    ensure_ascii=False,
                )
            )

    session = {}
    for name in ('session.json', 'reopen.json'):
        candidate = output / name
        if candidate.exists():
            session[name] = json.loads(candidate.read_text())
    # Truthful limitations observed by the driver (never a mock pass/fail):
    # surfaced at the top level so a run cannot look clean while a finding exists.
    findings = [
        {'phase': name[:-5] if name.endswith('.json') else name, 'finding': finding}
        for name, payload in session.items()
        for finding in payload.get('findings', [])
    ]
    (output / 'result.json').write_text(
        json.dumps(
            {
                'result': 'completed',
                'mode': mode,
                'requestedTurns': turns or 2,
                'driverTimeoutSeconds': timeout,
                'phases': phases,
                'nativeFrb': True,
                'scriptedProvider': True,
                'studioHome': str(home),
                'workspaceRemoved': True,
                'providerUrl': url,
                'cleanup': _cleanup_evidence(output),
                'wireIndex': _wire_index_summary(output),
                'findings': findings,
                'evidence': session,
            },
            ensure_ascii=False,
        )
    )
    print(
        json.dumps(
            {
                'result': 'completed',
                'mode': mode,
                'requestedTurns': turns or 2,
                'output': str(output),
            }
        )
    )
    for entry in findings:
        print(
            f"finding[{entry['phase']}]: {entry['finding']}",
            file=sys.stderr,
        )


if __name__ == '__main__':
    main()
