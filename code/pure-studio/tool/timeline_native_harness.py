#!/usr/bin/env python3
"""Opt-in deterministic native GUI acceptance; all output is internal test evidence."""
import argparse
import http.server
import json
import os
from pathlib import Path
import re
import signal
import subprocess
import tempfile
import shutil
import threading
import time


class Provider(http.server.BaseHTTPRequestHandler):
    release = threading.Event()
    output = None
    sequence = 0
    lock = threading.Lock()

    def log_message(self, *args):
        pass

    def do_GET(self):
        if self.path == '/release':
            self.release.set()
            self.send_response(200)
            self.end_headers()
            self.wfile.write(b'ok')
        else:
            self.send_error(404)

    def do_POST(self):
        body = json.loads(self.rfile.read(int(self.headers['Content-Length'])))
        with self.lock:
            type(self).sequence += 1
            seq = self.sequence
        (self.output / f'wire-{seq:04d}.json').write_text(json.dumps(body, ensure_ascii=False))
        responses = 'input' in body
        messages = body.get('input', []) if responses else body.get('messages', [])
        users = [json.dumps(m.get('content'), ensure_ascii=False) for m in messages if m.get('role') == 'user']
        relevant = [text for text in users if re.search(r'timeline-(create-child|child|stream|seed-\d+)', text)]
        prompt = relevant[-1] if relevant else ''
        self.send_response(200)
        self.send_header('Content-Type', 'text/event-stream')
        self.end_headers()

        text_parts = []
        def event(value):
            self.wfile.write(('data: ' + json.dumps(value, ensure_ascii=False) + '\n\n').encode())
            self.wfile.flush()

        def send(delta, finish=None):
            if not responses:
                event({'id': f'fixture-{seq}', 'object': 'chat.completion.chunk', 'model': body.get('model'), 'choices': [{'index': 0, 'delta': delta, 'finish_reason': finish}]})
                return
            if 'content' in delta:
                if not text_parts:
                    event({'type': 'response.output_item.added', 'item': {'id': f'answer-{seq}', 'type': 'message', 'role': 'assistant'}})
                text_parts.append(delta['content'])
                event({'type': 'response.output_text.delta', 'item_id': f'answer-{seq}', 'delta': delta['content']})
            for call in delta.get('tool_calls', []):
                event({'type': 'response.output_item.done', 'item': {'id': call['id'], 'type': 'function_call', 'call_id': call['id'], **call['function']}})
            if finish:
                if text_parts:
                    event({'type': 'response.output_item.done', 'item': {'id': f'answer-{seq}', 'type': 'message', 'role': 'assistant', 'content': [{'type': 'output_text', 'text': ''.join(text_parts)}]}})
                event({'type': 'response.completed', 'response': {'id': f'fixture-{seq}', 'usage': {'input_tokens': 5, 'output_tokens': 3, 'total_tokens': 8}}})

        try:
            if not body.get('tools'):
                send({'content': 'Timeline fixture'})
            elif 'timeline-create-child' in prompt and not any(m.get('role') == 'tool' or m.get('type') == 'function_call_output' for m in messages):
                send({'tool_calls': [{'index': 0, 'id': 'timeline-child-call', 'type': 'function', 'function': {'name': 'spawn_agent', 'arguments': json.dumps({'profileId': 'explorer', 'message': 'timeline-child', 'forkTurns': 'none'})}}]})
                send({}, 'tool_calls')
                self.wfile.write(b'data: [DONE]\n\n')
                return
            elif 'timeline-stream' in prompt:
                self.release.clear()
                failed = 'timeline-stream-failed' in prompt
                cancelled = 'timeline-stream-cancelled' in prompt
                send({'content': '失败前片段\n' if failed else '取消前片段\n' if cancelled else '第一段\n'})
                if not self.release.wait(120):
                    raise TimeoutError('Driver did not release the controlled stream')
                if failed:
                    event({'type': 'response.failed', 'response': {'id': f'fixture-{seq}', 'error': {'code': 'invalid_request_error', 'message': 'deterministic fixture failure'}, 'status_code': 400}})
                    return
                if not cancelled:
                    send({'content': '第二段\n```text\nC:\\fixture\\n\n```\n'})
            elif 'timeline-child' in prompt:
                send({'content': 'CHILD 独立正文\n保持归属。'})
            else:
                marker = re.search(r'timeline-seed-\d+', prompt)
                send({'content': (marker.group() if marker else 'ROOT') + '\n完整正文，不截断。\n'})
            send({}, 'stop')
            self.wfile.write(b'data: [DONE]\n\n')
            self.wfile.flush()
        except (BrokenPipeError, ConnectionResetError):
            pass


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--output', required=True)
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[3]
    output = Path(args.output).resolve()
    output.mkdir(parents=True, exist_ok=False)
    home = output / 'studio-home'
    workspace = Path(tempfile.mkdtemp(prefix='pure-timeline-native-workspace-'))
    home.mkdir()
    (output / 'fixture.json').write_text(json.dumps({'workspace': str(workspace)}))
    Provider.output = output
    server = http.server.ThreadingHTTPServer(('127.0.0.1', 0), Provider)
    serving = threading.Thread(target=server.serve_forever, daemon=True)
    serving.start()
    url = f'http://127.0.0.1:{server.server_port}'
    config = 'schema_version = 18\n[runtime]\npermission_mode = "full-access"\n'
    config += f'[models.providers.deepseek]\nname = "Timeline scripted provider"\npreset = "deepseek"\nbase_url = "{url}"\n'
    config += '[models.providers.deepseek.catalog]\nsource = "bundled"\ncatalog = "deepseek"\n'
    for role in ('explorer', 'planner', 'executor', 'worktree_executor', 'reviewer'):
        config += f'[models.routes.{role}]\nprovider = "deepseek"\nmodel = "deepseek-flash"\neffort = "high"\n'
    (home / 'config.toml').write_text(config)
    env = dict(os.environ, PURE_STUDIO_HOME=str(home))
    gui = None
    try:
        with (output / 'gui.log').open('w') as log:
            gui = subprocess.Popen(['xvfb-run', '-a', '-s', '-screen 0 1280x720x24', 'cargo', 'xtask', 'run-gui', '--driver'], cwd=root, env=env, stdout=log, stderr=subprocess.STDOUT, start_new_session=True)
            (output / 'process.json').write_text(json.dumps({'processGroup': gui.pid}))
            deadline = time.monotonic() + 600
            vm = None
            while time.monotonic() < deadline:
                text = (output / 'gui.log').read_text()
                matches = re.findall(r'A Dart VM Service .*?available at: (http://127\.0\.0\.1:\d+/[^\s]*)', text)
                if matches:
                    vm = matches[-1]
                    break
                if gui.poll() is not None:
                    raise RuntimeError(f'GUI exited {gui.returncode}; see gui.log')
                time.sleep(0.2)
            if vm is None:
                raise TimeoutError('native GUI did not publish VM service')
            with (output / 'driver.log').open('w') as driver_log:
                subprocess.run(['cargo', 'dart', 'run', 'test_driver/timeline_acceptance_driver.dart', vm, str(output), url, str(workspace)], cwd=root, env=env, stdout=driver_log, stderr=subprocess.STDOUT, check=True, timeout=600)
    finally:
        Provider.release.set()
        try:
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
                remaining = []
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
                    time.sleep(0.05)
                (output / 'cleanup.json').write_text(json.dumps({'remainingProcessGroupMembers': remaining}))
                if remaining:
                    raise RuntimeError(f'GUI descendants survived cleanup: {remaining}')
        finally:
            server.shutdown()
            server.server_close()
            serving.join(timeout=5)
            shutil.rmtree(workspace)


if __name__ == '__main__':
    main()
