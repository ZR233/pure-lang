#!/usr/bin/env python3
"""Long-history GUI/database scale acceptance for the W6-3 storage contract.

Owner: W6-3 GUI/protocol/runtime stream acceptance. This entry point answers a
different question than `project_session_native_harness.py`: that harness proves
a real Project + Thread chain end to end, while this one fixes the workspace and
config and scales only the *durable history* of one already-accepted session, so
an operator can compare "one 1000-item history" against "one 1000000-item
history" on the same build, host and display.

The base argument must be a completed run of the native harness: an output
directory that still holds the real `session.json` and `studio-home/`. Nothing
in that base directory is written; it is only read and copied.

For every requested size this harness:

* copies the base `studio-home` into an isolated fixture directory
  (`<output>/fixtures/<size>/studio-home`) and injects older, self-consistent
  history rows there. The real last Turn and the real oversized assistant body
  stay the newest items, and `history_meta.applied_write_seq` is never moved
  below the checkpoint fence that `state.toml` already requires;
* reports the constructed row counts, the declared indexes and the
  `EXPLAIN QUERY PLAN` of the queries the runtime actually issues;
  each plan is judged against that query's own shape: the ordered rowid scan
  SQLite uses for the unpredicated first page is canonical (it only has to
  avoid a TEMP B-TREE), while a lost keyset bound, a missing index on the
  kind/lifecycle/Turn predicates or a sort is a fixture failure;
* opens each fixture with the real Linux native Studio (`cargo xtask run-gui
  --driver`), observes the selected Thread opening automatically at startup,
  waits for the bounded first window and records its identity, then polls only
  the leaf GUI process' `/proc/<pid>/smaps_rollup` (falling back to
  `/proc/<pid>/status` VmRSS/VmHWM) for the whole run.

Honest limits this harness does not hide:

* it measures storage scale only. Nothing here proves model/provider behaviour,
  GUI hot-update performance, or a production latency budget; the comparison
  section is data, not a verdict, and no "passed" flag is inferred from it;
* the first-window latency is observed by the Dart driver over the Flutter
  Driver transport, and the memory samples are external `/proc` reads of the
  GUI leaf process. Both are wall-clock observations on one host; concurrent
  GUI or build work on the same machine can shift them, so sizes are repeated
  and interleaved instead of measured once;
* the fixtures are SQLite-level injections derived from real payloads. They are
  ordered, indexed, watermarked and checkpoint-consistent, but they are not
  produced by replaying real Turns through the product writer.

Usage:

  python3 code/anywork/tool/project_session_scale_harness.py \
    BASE_OUTPUT_DIRECTORY --output OUTPUT_DIRECTORY

Optional: `--sizes 1000,1000000`, `--repeats 2`, `--display 1600x1000x24`,
`--driver-timeout 3600`, `--sample-interval-ms 250`, `--rebuild`,
`--skip-identity-scan`, `--fixtures-only`.

The entry point is fail-closed: exit 0 only when every constructed fixture and
every GUI run verified. A fixture whose schema shape, index set, watermark,
identity, Turn boundary or query plan report is wrong stops the run before any
GUI is launched (exit 2), and a GUI run that fails to open, to sample, to clean
up or to preserve the operator Studio home fails the run (exit 1). `result.json`
lists the exact reasons under `problems`.

The injected payloads are rewritten with SQLite's JSON functions, so the
interpreter's bundled SQLite must provide `json_valid`/`json_extract`.

Requires the same environment as the native harness: `xvfb-run`, a Linux native
Flutter desktop toolchain and the repository's `cargo xtask run-gui --driver`.
"""

import argparse
import hashlib
import http.server
import json
import os
from pathlib import Path
import re
import shutil
import sqlite3
import subprocess
import sys
import threading
import time

# Reused only for the isolated launch/cleanup helpers and the scripted loopback
# provider; the two-Turn and long-session behaviour of that harness is untouched.
import project_session_native_harness as native


DEFAULT_SIZES = '1000,1000000'
DEFAULT_REPEATS = 2
DEFAULT_DISPLAY = '1600x1000x24'
DEFAULT_DRIVER_TIMEOUT = 3600
DEFAULT_SAMPLE_INTERVAL_MS = 250

# The synthetic history is modelled from the real small `text`/`turn` payloads
# only: they carry the ordinary message/terminal shapes the timeline renders,
# while the real oversized body is never duplicated across the fixture.
TEMPLATE_KINDS = ('text', 'turn')
MAX_TEMPLATE_PAYLOAD_BYTES = 64 * 1024
INSERT_BATCH = 20000

# Indexes the history schema declares; the fixture must still carry exactly
# these (a fixture may add nothing and must drop nothing).
REQUIRED_INDEXES = (
    'history_items_by_turn',
    'history_items_by_kind_lifecycle',
    'history_items_by_kind',
    'history_turns_by_last_ordinal',
)

# SQL history page limit; live overlay entries are counted separately.
SQL_HISTORY_WINDOW_LIMIT = 500


def _size_label(size):
    if size % 1000000 == 0:
        return f'{size // 1000000}M'
    if size % 1000 == 0:
        return f'{size // 1000}k'
    return str(size)


def _sha256(path):
    digest = hashlib.sha256()
    with Path(path).open('rb') as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b''):
            digest.update(chunk)
    return digest.hexdigest()


def _thread_storage_key(thread_id):
    """StudioPaths::thread_storage_key: lowercase hex SHA-256 of the Thread id."""
    return hashlib.sha256(thread_id.encode('utf-8')).hexdigest()


class BaseLayout:
    """Read-only description of the completed native acceptance run to scale."""

    def __init__(self, base):
        self.base = base
        self.session_path = base / 'session.json'
        self.home = base / 'studio-home'
        self.catalog_path = self.home / 'catalog.toml'
        self.config_path = self.home / 'config.toml'
        self.session = self._read_session()
        self.thread_id = self.session.get('threadId')
        if not isinstance(self.thread_id, str) or not self.thread_id:
            raise SystemExit(f'{self.session_path} has no Thread id')
        self.storage_key = _thread_storage_key(self.thread_id)
        self.session_dir = self.home / 'sessions' / self.storage_key
        self.state_path = self.session_dir / 'state.toml'
        self.history_path = self.session_dir / 'history.sqlite'
        for path in (self.home, self.catalog_path, self.state_path, self.history_path):
            if not path.exists():
                raise SystemExit(
                    f'{path} is missing; BASE must be a completed '
                    'project_session_native_harness.py output directory'
                )
        self.workspace = self.session.get('workspace')

    def _read_session(self):
        if not self.session_path.exists():
            raise SystemExit(
                f'{self.session_path} is missing; BASE must hold the real '
                'session.json from the native acceptance run'
            )
        return json.loads(self.session_path.read_text(encoding='utf-8'))

    def fingerprint(self):
        return {
            'sessionJson': _sha256(self.session_path),
            'historySqlite': _sha256(self.history_path),
        }


def _ensure_workspace(workspace):
    """The base run removes its temporary workspace; recreate it if it is gone.

    The fixture keeps the real workspace path (no config rewrite), so the
    directory the copied `catalog.toml`/`state.toml` name has to exist. A path
    that is already present is never touched, so a still-running session at the
    same path is unaffected.
    """
    if not workspace:
        return {'path': None, 'state': 'not-recorded'}
    path = Path(workspace)
    if path.exists():
        return {
            'path': str(path),
            'state': 'pre-existing' if path.is_dir() else 'pre-existing-not-directory',
        }
    path.mkdir(parents=True, exist_ok=True)
    return {'path': str(path), 'state': 'created'}


# --------------------------------------------------------------- fixture build


def _turn_template(real_turns):
    for row in reversed(real_turns):
        candidate = json.loads(row[4])
        if isinstance(candidate, dict) and isinstance(candidate.get('turn'), dict):
            return candidate
    raise RuntimeError('the base history has no Turn payload to template from')


def _item_payload(template, item_id, thread_id, turn_id, ordinal, created_at, revision):
    """Rewrites only the identity fields of one real item payload.

    `state` (channel/text/lifecycle or the Turn state machine) is reused verbatim
    except for the fields that would otherwise dangle: a synthetic Turn carries
    no input association, and terminal content timestamps follow the synthetic
    created time so the row stays self-consistent.
    """
    payload = template['payload']
    payload['id'] = item_id
    payload['threadId'] = thread_id
    payload['turnId'] = turn_id
    payload['ordinal'] = ordinal
    payload['revision'] = revision
    payload['createdAt'] = created_at
    payload['updatedAt'] = created_at
    state = payload.get('state')
    if isinstance(state, dict):
        data = state.get('data')
        if isinstance(data, dict):
            if state.get('kind') == 'turn':
                data.pop('inputId', None)
                # The Turn item wraps its own TurnState under `state`; align its
                # terminal timestamp with the synthetic row time when present.
                turn_state = data.get('state')
                if isinstance(turn_state, dict):
                    turn_terminal = turn_state.get('data')
                    if isinstance(turn_terminal, dict):
                        for key in ('completedAt', 'failedAt', 'cancelledAt'):
                            if key in turn_terminal:
                                turn_terminal[key] = created_at
                                break
            lifecycle = data.get('lifecycle')
            if isinstance(lifecycle, dict):
                terminal = lifecycle.get('data')
                if isinstance(terminal, dict):
                    for key in ('completedAt', 'failedAt', 'cancelledAt'):
                        if key in terminal:
                            terminal[key] = created_at
                            break
    return json.dumps(payload, separators=(',', ':'), ensure_ascii=False)


def _turn_payload(template, turn_id, thread_id, last_item_id, created_at):
    payload = template
    turn = payload['turn']
    turn['id'] = turn_id
    turn['threadId'] = thread_id
    turn['updatedAt'] = created_at
    turn.pop('inputId', None)
    payload['lastItemId'] = last_item_id
    payload['contextDisposition'] = 'active'
    return json.dumps(payload, separators=(',', ':'), ensure_ascii=False)


def _shift_real_item(payload, ordinal):
    rewritten = json.loads(payload)
    rewritten['ordinal'] = ordinal
    return json.dumps(rewritten, separators=(',', ':'), ensure_ascii=False)


def _template_role(template):
    """Identity-free shape of one template: its kind plus its text channel.

    A real long session contributes one `turn` item and a handful of message
    items per Turn; the fixture only needs each *shape* once per synthetic Turn.
    """
    payload = template['payload']
    state = payload.get('state') if isinstance(payload, dict) else None
    data = state.get('data') if isinstance(state, dict) else None
    channel = data.get('channel') if isinstance(data, dict) else None
    return (template['kind'], channel if isinstance(channel, str) else '')


def build_fixture(base, fixture_root, size, skip_identity_scan=False):
    """Copies the base Studio home and injects a self-consistent history of `size` items."""
    home = fixture_root / 'studio-home'
    if home.exists():
        raise RuntimeError(f'{home} already exists; pass --rebuild to replace it')
    shutil.copytree(base.home, home, symlinks=True)
    history_path = home / 'sessions' / base.storage_key / 'history.sqlite'
    layout = {
        'label': _size_label(size),
        'targetItems': size,
        'fixtureHome': str(home),
        'historySqlite': str(history_path),
        'threadId': base.thread_id,
        'storageKey': base.storage_key,
    }
    layout.update(_populate(history_path, size, base.thread_id, skip_identity_scan))
    return home, layout


def _populate(history_path, size, thread_id, skip_identity_scan=False):
    connection = sqlite3.connect(str(history_path), isolation_level=None, timeout=120.0)
    try:
        cursor = connection.cursor()
        # Fold any leftover write-ahead log into the main file so the fixture is
        # one self-contained database before it is copied per run.
        cursor.execute('PRAGMA wal_checkpoint(TRUNCATE)')
        meta = cursor.execute(
            'SELECT schema_version,database_id,thread_id,applied_write_seq '
            'FROM history_meta WHERE id=1'
        ).fetchone()
        if meta is None:
            raise RuntimeError('the base history has no identity row')
        schema_version, database_id, owner, applied_write_seq = meta
        if owner != thread_id:
            raise RuntimeError(
                f'the base history belongs to {owner}, not {thread_id}'
            )
        real_items = cursor.execute(
            'SELECT ordinal,item_id,turn_id,kind,revision,lifecycle,created_at,'
            'updated_at,payload FROM history_items ORDER BY ordinal'
        ).fetchall()
        real_turns = cursor.execute(
            'SELECT turn_id,first_ordinal,last_ordinal,revision,payload '
            'FROM history_turns ORDER BY last_ordinal'
        ).fetchall()
        reservations = cursor.execute(
            'SELECT item_id,ordinal FROM history_ordinals'
        ).fetchall()
        if not real_items:
            raise RuntimeError('the base history has no items to template from')

        templates = []
        for row in real_items:
            kind = row[3]
            payload = row[8]
            if kind not in TEMPLATE_KINDS:
                continue
            if len(payload.encode('utf-8')) > MAX_TEMPLATE_PAYLOAD_BYTES:
                continue
            templates.append({
                'kind': kind,
                'revision': row[4],
                'lifecycle': row[5],
                'payload': json.loads(payload),
            })
        if not templates:
            raise RuntimeError(
                'the base history has no small text/Turn item to template from'
            )
        # One synthetic Turn = its ordinary items + exactly one Turn item.
        #
        # A real long session contributes one `turn` item per Turn, and the
        # derived Turn identity `turn:{len}:{turn_id}` is unique per Turn, so
        # more than one Turn template inside one synthetic Turn would collide on
        # `history_items.item_id`. Ordinary templates are deduplicated by role
        # for the same reason: the 132 Turns of a real long session repeat the
        # same handful of message shapes, and replaying every copy inside each
        # synthetic Turn would multiply the Turn size without adding history.
        turn_items = [
            template for template in templates if template['kind'] == 'turn'
        ]
        ordinary = []
        seen_roles = set()
        for template in templates:
            if template['kind'] == 'turn':
                continue
            role = _template_role(template)
            if role in seen_roles:
                continue
            seen_roles.add(role)
            ordinary.append(template)
        # Prefer the newest terminal Turn entry as the one Turn item per Turn.
        turn_item = None
        for template in reversed(turn_items):
            if template['lifecycle'] == 'terminal':
                turn_item = template
                break
        if turn_item is None and turn_items:
            turn_item = turn_items[-1]
        # The Turn entry is emitted last so history_turns.last_item_id always
        # names the end of its own Turn, exactly like the sequence it summarises.
        composition = ordinary + ([turn_item] if turn_item is not None else [])
        if not composition:
            raise RuntimeError(
                'the base history has no item shape to build synthetic Turns from'
            )
        turn_template = _turn_template(real_turns)

        real_count = len(real_items)
        synthetic = size - real_count
        per_turn = len(composition)
        if synthetic < per_turn:
            raise RuntimeError(
                f'target size {size} leaves only {synthetic} synthetic rows; '
                f'more than the {real_count} real items are required'
            )
        blocks = (synthetic + per_turn - 1) // per_turn
        base_time = min(row[6] for row in real_items)
        # Synthetic Turns walk backwards from the oldest real item in 1 s steps,
        # clamped so the earliest injected timestamp stays positive at any size.
        step_ms = max(1, min(1000, base_time // (blocks + 1))) if base_time > 0 else 1
        turn_revision = int(turn_template['turn'].get('revision', 1)) or 1

        connection.execute('BEGIN IMMEDIATE')
        cursor.execute('DELETE FROM history_items')
        cursor.executemany(
            'INSERT INTO history_items(ordinal,item_id,turn_id,kind,revision,'
            'lifecycle,created_at,updated_at,payload,last_write_seq) '
            'VALUES(?,?,?,?,?,?,?,?,?,?)',
            [
                (
                    row[0] + synthetic,
                    row[1],
                    row[2],
                    row[3],
                    row[4],
                    row[5],
                    row[6],
                    row[7],
                    _shift_real_item(row[8], row[0] + synthetic),
                    applied_write_seq,
                )
                for row in real_items
            ],
        )
        cursor.executemany(
            'UPDATE history_turns SET first_ordinal=?,last_ordinal=? WHERE turn_id=?',
            [
                (row[1] + synthetic, row[2] + synthetic, row[0])
                for row in real_turns
            ],
        )
        cursor.executemany(
            'UPDATE history_ordinals SET ordinal=? WHERE item_id=?',
            [(row[1] + synthetic, row[0]) for row in reservations],
        )
        # The durable input-identity index denormalises the item ordinal; keep it
        # aligned with the shifted `history_items.ordinal` instead of leaving a
        # second, stale position behind.
        cursor.execute(
            'UPDATE history_input_identities SET ordinal = ordinal + ?', (synthetic,)
        )

        items_batch = []
        ordinals_batch = []
        ordinal = 0
        turn_index = 0
        remaining = synthetic
        while remaining > 0:
            take = min(per_turn, remaining)
            turn_index += 1
            turn_id = f'scale-turn:{size}:{turn_index}'
            created_at = base_time - (blocks - turn_index + 1) * step_ms
            if take >= per_turn:
                selected = composition
            elif turn_item is not None:
                # A short tail still carries its own Turn fact: ordinary shapes
                # are dropped from the end, never the unique Turn item.
                selected = composition[: take - 1] + [turn_item]
            else:
                selected = composition[:take]
            last_item_id = None
            for index, template in enumerate(selected):
                ordinal += 1
                if template['kind'] == 'turn':
                    item_id = f'turn:{len(turn_id)}:{turn_id}'
                else:
                    item_id = f'scale-item:{size}:{turn_index}:{index}'
                last_item_id = item_id
                items_batch.append((
                    ordinal,
                    item_id,
                    turn_id,
                    template['kind'],
                    template['revision'],
                    template['lifecycle'],
                    created_at,
                    created_at,
                    _item_payload(
                        template,
                        item_id,
                        thread_id,
                        turn_id,
                        ordinal,
                        created_at,
                        template['revision'],
                    ),
                    applied_write_seq,
                ))
                ordinals_batch.append((item_id, ordinal))
            cursor.execute(
                'INSERT INTO history_turns(turn_id,first_ordinal,last_ordinal,'
                'revision,payload,last_write_seq) VALUES(?,?,?,?,?,?)',
                (
                    turn_id,
                    ordinal - take + 1,
                    ordinal,
                    turn_revision,
                    _turn_payload(
                        turn_template,
                        turn_id,
                        thread_id,
                        last_item_id,
                        created_at,
                    ),
                    applied_write_seq,
                ),
            )
            remaining -= take
            if len(items_batch) >= INSERT_BATCH or remaining == 0:
                cursor.executemany(
                    'INSERT INTO history_items(ordinal,item_id,turn_id,kind,'
                    'revision,lifecycle,created_at,updated_at,payload,'
                    'last_write_seq) VALUES(?,?,?,?,?,?,?,?,?,?)',
                    items_batch,
                )
                cursor.executemany(
                    'INSERT INTO history_ordinals(item_id,ordinal) VALUES(?,?) '
                    'ON CONFLICT(item_id) DO NOTHING',
                    ordinals_batch,
                )
                items_batch = []
                ordinals_batch = []
        connection.execute('COMMIT')
        cursor.execute('PRAGMA wal_checkpoint(TRUNCATE)')

        report = {
            'label': _size_label(size),
            'targetItems': size,
            'templateKinds': list(TEMPLATE_KINDS),
            'templateItems': per_turn,
            'realItems': real_count,
            'syntheticItems': synthetic,
            'syntheticTurns': turn_index,
            'composition': {
                'ordinaryRoles': [
                    ':'.join(part for part in _template_role(template) if part)
                    for template in ordinary
                ],
                'ordinaryItems': len(ordinary),
                'hasTurnItem': turn_item is not None,
                'realTurnItemTemplates': len(turn_items),
                'perTurn': per_turn,
            },
            'historyMeta': {
                'schemaVersion': schema_version,
                'databaseId': database_id,
                'threadId': owner,
                'appliedWriteSeq': applied_write_seq,
            },
            'state': _state_facts(history_path),
        }
        report['counts'] = _counts(cursor)
        report['indexes'] = _indexes(cursor)
        report['integrity'] = _integrity(cursor, skip_identity_scan)
        report['checkpoint'] = _checkpoint(
            report['state'], applied_write_seq, thread_id
        )
        return report
    finally:
        connection.close()


def _state_facts(history_path):
    """Minimal read-only facts from the sibling `state.toml` checkpoint."""
    state_path = history_path.parent / 'state.toml'
    if not state_path.exists():
        return {'available': False}
    text = state_path.read_text(encoding='utf-8')
    facts = {'available': True, 'path': str(state_path)}
    for key in ('threadId',):
        match = re.search(rf'(?m)^{key}\s*=\s*"([^"]*)"', text)
        facts[key] = match.group(1) if match else None
    for key in ('historyFence', 'stateRevision'):
        match = re.search(rf'(?m)^{key}\s*=\s*(\d+)', text)
        facts[key] = int(match.group(1)) if match else None
    match = re.search(r'(?m)^commitSequence\s*=\s*(\d+)', text)
    facts['commitSequence'] = int(match.group(1)) if match else None
    return facts


def _checkpoint(state, applied_write_seq, thread_id):
    """The durable-history preconditions the runtime re-checks on every read.

    `StudioRuntime::ensure_timeline_history` refuses a Thread whose history
    watermark is below its checkpoint fence, and `ThreadStore::publish` requires
    `history_fence <= state_revision == commit_sequence`. Injecting rows must not
    break either relation, so they are reported explicitly.
    """
    fence = state.get('historyFence')
    state_revision = state.get('stateRevision')
    commit_sequence = state.get('commitSequence')
    state_thread_id = state.get('threadId')
    covers_fence = (
        fence is not None and applied_write_seq is not None
        and applied_write_seq >= fence
    )
    fence_consistent = (
        fence is not None
        and state_revision is not None
        and commit_sequence is not None
        and fence <= state_revision
        and state_revision == commit_sequence
    )
    thread_id_matches = state_thread_id is not None and state_thread_id == thread_id
    return {
        'appliedWriteSeq': applied_write_seq,
        'metaThreadId': thread_id,
        'stateThreadId': state_thread_id,
        'historyFence': fence,
        'stateRevision': state_revision,
        'commitSequence': commit_sequence,
        'watermarkCoversFence': covers_fence,
        'fenceConsistentWithState': fence_consistent,
        'threadIdMatchesState': thread_id_matches,
    }


def _counts(cursor):
    counts = {}
    for label, sql in (
        ('history_items', 'SELECT COUNT(*) FROM history_items'),
        ('history_turns', 'SELECT COUNT(*) FROM history_turns'),
        ('history_ordinals', 'SELECT COUNT(*) FROM history_ordinals'),
        ('history_input_identities', 'SELECT COUNT(*) FROM history_input_identities'),
        (
            'ordinalBounds',
            'SELECT MIN(ordinal),MAX(ordinal) FROM history_items',
        ),
    ):
        row = cursor.execute(sql).fetchone()
        counts[label] = list(row) if len(row) > 1 else row[0]
    return counts


def _indexes(cursor):
    rows = cursor.execute(
        "SELECT name,sql FROM sqlite_master WHERE type='index' "
        "AND name NOT LIKE 'sqlite_%' ORDER BY name"
    ).fetchall()
    declared = {name: sql for name, sql in rows}
    return {
        'declared': sorted(declared),
        'required': {name: name in declared for name in REQUIRED_INDEXES},
        'sql': declared,
    }


# Every check below must report 0 bad rows; the verdict enumerates the same
# names so a report that silently omits one cannot pass.
INTEGRITY_CHECKS = (
    (
        'duplicateOrdinals',
        'SELECT COUNT(*) FROM (SELECT ordinal FROM history_items '
        'GROUP BY ordinal HAVING COUNT(*) > 1)',
    ),
    (
        'duplicateItemIds',
        'SELECT COUNT(*) FROM (SELECT item_id FROM history_items '
        'GROUP BY item_id HAVING COUNT(*) > 1)',
    ),
    (
        'invalidPayload',
        'SELECT COUNT(*) FROM history_items WHERE json_valid(payload) = 0',
    ),
    (
        'turnBoundaryFirst',
        'SELECT COUNT(*) FROM history_turns t WHERE t.first_ordinal <> '
        '(SELECT MIN(i.ordinal) FROM history_items i WHERE i.turn_id = t.turn_id)',
    ),
    (
        'turnBoundaryLast',
        'SELECT COUNT(*) FROM history_turns t WHERE t.last_ordinal <> '
        '(SELECT MAX(i.ordinal) FROM history_items i WHERE i.turn_id = t.turn_id)',
    ),
    (
        'itemsWithoutTurn',
        'SELECT COUNT(*) FROM history_items i WHERE i.turn_id <> \'\' AND '
        'NOT EXISTS (SELECT 1 FROM history_turns t WHERE t.turn_id = i.turn_id)',
    ),
    (
        'writesAboveWatermark',
        'SELECT COUNT(*) FROM history_items WHERE last_write_seq > '
        '(SELECT applied_write_seq FROM history_meta WHERE id=1)',
    ),
    (
        'reservationMismatch',
        'SELECT COUNT(*) FROM history_ordinals o JOIN history_items i '
        'ON i.item_id = o.item_id WHERE o.ordinal <> i.ordinal',
    ),
    (
        'turnPayloadIdentity',
        "SELECT COUNT(*) FROM history_turns WHERE json_valid(payload) = 0 OR "
        "json_extract(payload,'$.turn.id') IS NOT turn_id OR "
        "json_extract(payload,'$.turn.threadId') IS NOT "
        '(SELECT thread_id FROM history_meta WHERE id=1)',
    ),
    (
        'negativeOrdinals',
        'SELECT COUNT(*) FROM history_items WHERE ordinal <= 0',
    ),
    # Each synthetic Turn must carry exactly one Turn item: its identity
    # `turn:{len}:{turn_id}` is unique per Turn, so a second one in the same
    # Turn is a construction bug (and a duplicate `item_id`).
    (
        'syntheticTurnItemCount',
        'SELECT COUNT(*) FROM (SELECT turn_id FROM history_items '
        "WHERE turn_id LIKE 'scale-turn:%' AND kind = 'turn' "
        'GROUP BY turn_id HAVING COUNT(*) <> 1)',
    ),
)

# The full-payload scan is the strongest check and is only skipped on explicit
# operator request, which then makes the fixture unverified on purpose.
# `history_items` has no `thread_id` column: the row's Thread identity is the
# database identity in `history_meta`, so the payload is compared against that
# authority instead of an invented column.
IDENTITY_MISMATCH_CHECK = (
    'identityMismatch',
    "SELECT COUNT(*) FROM history_items WHERE json_extract(payload,'$.id') "
    'IS NOT item_id OR json_extract(payload,\'$.ordinal\') IS NOT ordinal '
    "OR json_extract(payload,'$.turnId') IS NOT turn_id OR "
    "json_extract(payload,'$.threadId') IS NOT "
    '(SELECT thread_id FROM history_meta WHERE id=1) OR '
    "json_extract(payload,'$.revision') IS NOT revision",
)


def _integrity(cursor, skip_identity_scan=False):
    checks = dict(INTEGRITY_CHECKS)
    if not skip_identity_scan:
        checks[IDENTITY_MISMATCH_CHECK[0]] = IDENTITY_MISMATCH_CHECK[1]
    report = {}
    for label, sql in checks.items():
        report[label] = cursor.execute(sql).fetchone()[0]
    report['identityScanSkipped'] = skip_identity_scan
    return report


def _explain_queries(max_ordinal, sample_turn_id):
    """One row per runtime query: SQL, human expectation, and plan semantics.

    The last field is the *shape* of the query, because a plan can only be
    judged against what the query actually is:

    * `rowid-page` — no predicate, ordered by `ordinal` (the INTEGER PRIMARY
      KEY) with a LIMIT. SQLite satisfies this with an ordered scan of the
      rowid; in several versions that step is reported as a bare
      `SCAN history_items` with no `USING`, which is correct and efficient here.
      The absence of a `TEMP B-TREE` is what proves the order came from the
      primary key instead of a sort.
    * `integer-primary-key` — a bounded keyset/range on `ordinal`; losing the
      bound shows up as a real full scan and must fail.
    * `index:<name>` — a kind/lifecycle/Turn predicate that must use its index.
    * `indexed` — a point read that must use some index or primary key.
    """
    return (
        (
            'latest-item-page',
            'SELECT payload FROM history_items ORDER BY ordinal DESC LIMIT 101',
            'history_items INTEGER PRIMARY KEY ordered first page, no TEMP B-TREE',
            'rowid-page',
        ),
        (
            'older-item-page',
            f'SELECT payload FROM history_items WHERE ordinal < {max_ordinal} '
            'ORDER BY ordinal DESC LIMIT 100',
            'history_items INTEGER PRIMARY KEY',
            'integer-primary-key',
        ),
        (
            'newer-item-page',
            "SELECT payload FROM history_items WHERE ordinal > 1 "
            'ORDER BY ordinal ASC LIMIT 100',
            'history_items INTEGER PRIMARY KEY',
            'integer-primary-key',
        ),
        (
            'watermark',
            'SELECT applied_write_seq FROM history_meta WHERE id=1',
            'history_meta primary-key point read',
            'indexed',
        ),
        (
            'turn-latest-page',
            'SELECT turn_id,first_ordinal,last_ordinal FROM history_turns '
            'ORDER BY last_ordinal DESC LIMIT 201',
            'history_turns_by_last_ordinal',
            'index:history_turns_by_last_ordinal',
        ),
        (
            'turn-older-page',
            f'SELECT turn_id,first_ordinal,last_ordinal FROM history_turns '
            f'WHERE last_ordinal < {max_ordinal} ORDER BY last_ordinal DESC LIMIT 200',
            'history_turns_by_last_ordinal',
            'index:history_turns_by_last_ordinal',
        ),
        (
            'latest-terminal-turn',
            "SELECT payload FROM history_items WHERE kind='turn' AND "
            "lifecycle='terminal' ORDER BY ordinal DESC LIMIT 1",
            'history_items_by_kind_lifecycle',
            'index:history_items_by_kind_lifecycle',
        ),
        (
            'terminal-turns-after',
            "SELECT payload FROM history_items WHERE kind='turn' AND "
            f"lifecycle='terminal' AND ordinal > 1 ORDER BY ordinal ASC LIMIT 100",
            'history_items_by_kind_lifecycle',
            'index:history_items_by_kind_lifecycle',
        ),
        (
            'sparse-text-filter',
            f"SELECT payload FROM history_items WHERE ordinal <= {max_ordinal} "
            "AND kind='text' ORDER BY ordinal DESC LIMIT 100",
            'history_items_by_kind',
            'index:history_items_by_kind',
        ),
        (
            'turn-by-id',
            f"SELECT payload FROM history_turns WHERE turn_id='{sample_turn_id}'",
            'history_turns primary-key point read',
            'indexed',
        ),
        (
            'oldest-boundary-probe',
            f'SELECT 1 FROM history_items WHERE ordinal < {max_ordinal} LIMIT 1',
            'history_items INTEGER PRIMARY KEY range probe',
            'integer-primary-key',
        ),
    )


PLAN_REQUIREMENT_NOTES = {
    'rowid-page': (
        'no predicate, ordered by the INTEGER PRIMARY KEY with a LIMIT: an '
        'ordered rowid scan (bare `SCAN history_items` is canonical here) is '
        'accepted, and a TEMP B-TREE means the order was sorted instead'
    ),
    'integer-primary-key': (
        'bounded by a predicate on `ordinal`: the plan must read through the '
        'INTEGER PRIMARY KEY, so a bare scan means the bound was lost'
    ),
    'indexed': 'a point read: the plan must use an index or a primary key',
}

_PLAN_ACCESS_PATTERN = re.compile(
    r'(?P<verb>SCAN|SEARCH)\s+(?P<table>[A-Za-z_][A-Za-z0-9_]*)(?P<rest>.*)$',
    re.IGNORECASE,
)
_PLAN_USING_PATTERN = re.compile(
    r'USING\s+(?P<target>[^()]+?)\s*(?:\(|$)', re.IGNORECASE
)


def _requirement_note(requirement):
    if requirement in PLAN_REQUIREMENT_NOTES:
        return PLAN_REQUIREMENT_NOTES[requirement]
    if requirement.startswith('index:'):
        name = requirement.split(':', 1)[1]
        return (
            f'filtered by kind/lifecycle/Turn and ordered by ordinal: the plan '
            f'must use the `{name}` index'
        )
    return requirement


def _plan_accesses(plan):
    """Every table access step of one `EXPLAIN QUERY PLAN`, with its access path."""
    accesses = []
    for step in plan:
        match = _PLAN_ACCESS_PATTERN.search(step)
        if match is None:
            continue
        using = _PLAN_USING_PATTERN.search(step)
        accesses.append({
            'step': step,
            'verb': match.group('verb').upper(),
            'table': match.group('table'),
            'using': using.group('target').strip() if using else None,
        })
    return accesses


def _plan_problems(requirement, plan):
    """Per-query plan violations, judged against that query's own semantics."""
    problems = []
    joined = '\n'.join(plan)
    if 'TEMP B-TREE' in joined.upper():
        problems.append('the plan materialises a TEMP B-TREE')
    accesses = _plan_accesses(plan)
    if not accesses:
        problems.append('the plan reports no table access step')
        return problems

    if requirement == 'rowid-page':
        # A bare ordered scan is the canonical plan for this shape; the check
        # above already rejected a sort, so the order can only come from the
        # INTEGER PRIMARY KEY. An explicit access path must still be the PK.
        for access in accesses:
            using = (access['using'] or '').upper()
            if access['using'] is None:
                continue
            if 'INTEGER PRIMARY KEY' not in using:
                problems.append(
                    f"{access['table']} is not read through its INTEGER PRIMARY "
                    f"KEY: {access['step']}"
                )
        return problems

    if requirement == 'integer-primary-key':
        wanted = 'INTEGER PRIMARY KEY'
    elif requirement == 'indexed':
        wanted = None
    elif requirement.startswith('index:'):
        wanted = requirement.split(':', 1)[1]
    else:
        problems.append(f'unknown plan requirement {requirement!r}')
        return problems

    for access in accesses:
        using = access['using'] or ''
        if wanted is None:
            if access['using'] is None:
                problems.append(
                    f"{access['table']} is read without an index or primary "
                    f"key: {access['step']}"
                )
            continue
        if re.search(rf'\b{re.escape(wanted)}\b', using, re.IGNORECASE) is None:
            problems.append(
                f"{access['table']} is not read through {wanted}: {access['step']}"
            )
    return problems


def _explain(history_path):
    connection = sqlite3.connect(str(history_path), isolation_level=None, timeout=60.0)
    try:
        cursor = connection.cursor()
        max_ordinal = cursor.execute(
            'SELECT MAX(ordinal) FROM history_items'
        ).fetchone()[0] or 1
        sample = cursor.execute(
            'SELECT turn_id FROM history_turns ORDER BY last_ordinal DESC LIMIT 1'
        ).fetchone()
        sample_turn_id = sample[0] if sample else 'missing'
        report = []
        for label, sql, expected, requirement in _explain_queries(
            max_ordinal, sample_turn_id
        ):
            rows = cursor.execute('EXPLAIN QUERY PLAN ' + sql).fetchall()
            plan = [str(row[-1]) for row in rows]
            accesses = _plan_accesses(plan)
            report.append({
                'label': label,
                'sql': sql,
                'expected': expected,
                'requirement': requirement,
                'requirementNote': _requirement_note(requirement),
                'plan': plan,
                'accesses': accesses,
                # Raw evidence, not a verdict: the ordered first page is
                # expected to appear as a bare scan of the primary key.
                'unindexedScans': [
                    access['step'] for access in accesses if access['using'] is None
                ],
                'usesTemporaryBTree': 'TEMP B-TREE' in '\n'.join(plan).upper(),
                'problems': _plan_problems(requirement, plan),
            })
        return {'maxOrdinal': max_ordinal, 'queries': report}
    finally:
        connection.close()


# ------------------------------------------------------------ process sampling


def _descends_from(pid, ancestor_pid, depth=32):
    """Walks the parent chain so a re-grouped GUI is still recognised as ours."""
    seen = set()
    current = pid
    for _ in range(depth):
        if current <= 0 or current in seen:
            return False
        seen.add(current)
        if current == ancestor_pid:
            return True
        try:
            stat = Path(f'/proc/{current}/stat').read_text(
                encoding='utf-8', errors='replace'
            )
        except OSError:
            return False
        fields = stat.rsplit(')', 1)[-1].split()
        if len(fields) < 2:
            return False
        current = int(fields[1])
    return False


def _leaf_gui_pids(group_pid):
    """`anywork` GUI processes that belong to one launched Studio run."""
    matches = []
    for entry in Path('/proc').iterdir():
        if not entry.name.isdigit():
            continue
        pid = int(entry.name)
        try:
            exe = os.readlink(entry / 'exe')
        except OSError:
            continue
        if os.path.basename(exe) != 'anywork':
            continue
        try:
            owned = os.getpgid(pid) == group_pid or _descends_from(pid, group_pid)
        except OSError:
            continue
        if owned:
            matches.append(pid)
    return sorted(matches)


def _kb(line):
    match = re.search(r'(\d+)', line.split(':', 1)[1])
    return int(match.group(1)) if match else None


def _sample_process(pid):
    rollup = Path(f'/proc/{pid}/smaps_rollup')
    try:
        text = rollup.read_text(encoding='utf-8', errors='replace')
    except OSError:
        text = ''
    if text:
        pss = None
        rss = None
        for line in text.splitlines():
            if line.startswith('Pss:'):
                pss = _kb(line)
            elif line.startswith('Rss:'):
                rss = _kb(line)
        if pss is not None:
            return {'source': 'smaps_rollup', 'pssKb': pss, 'rssKb': rss}
    try:
        text = Path(f'/proc/{pid}/status').read_text(encoding='utf-8', errors='replace')
    except OSError:
        return None
    vm_rss = None
    vm_hwm = None
    for line in text.splitlines():
        if line.startswith('VmRSS:'):
            vm_rss = _kb(line)
        elif line.startswith('VmHWM:'):
            vm_hwm = _kb(line)
    if vm_rss is None and vm_hwm is None:
        return None
    return {'source': 'status', 'vmRssKb': vm_rss, 'vmHwmKb': vm_hwm}


class LeafProcessSampler:
    """Samples only the GUI leaf process, never the whole process group."""

    def __init__(self, group_pid, interval_seconds, path):
        self._group_pid = group_pid
        self._interval = interval_seconds
        self._path = path
        self._stop = threading.Event()
        self._thread = threading.Thread(target=self._loop, daemon=True)
        self.pid = None
        self.matches = []
        self.source = None
        self.samples = []

    def start(self):
        self._thread.start()

    def stop(self):
        self._stop.set()
        self._thread.join(timeout=30)
        return {
            'leafPid': self.pid,
            'leafMatches': self.matches,
            'source': self.source,
            'sampleCount': len(self.samples),
        }

    def _loop(self):
        with self._path.open('w', encoding='utf-8') as stream:
            while not self._stop.is_set():
                if self.pid is None:
                    matches = _leaf_gui_pids(self._group_pid)
                    self.matches = matches
                    # One GUI per run; the highest pid is the most recently
                    # started leaf if the scan ever sees more than one.
                    self.pid = matches[-1] if matches else None
                sample = _sample_process(self.pid) if self.pid is not None else None
                if sample is None:
                    # The leaf exited or was replaced; keep looking for it.
                    self.pid = None
                else:
                    self.source = sample['source']
                    record = {'tMs': int(time.time() * 1000), **sample}
                    self.samples.append(record)
                    stream.write(json.dumps(record) + '\n')
                    stream.flush()
                self._stop.wait(self._interval)


def _phase_summary(samples, start_ms, end_ms, fields):
    selected = [s for s in samples if start_ms <= s['tMs'] <= end_ms]
    summary = {'sampleCount': len(selected)}
    for field in fields:
        values = [s[field] for s in selected if s.get(field) is not None]
        if values:
            summary[field] = {
                'first': values[0],
                'last': values[-1],
                'max': max(values),
                'min': min(values),
                'delta': values[-1] - values[0],
            }
    return summary


# ------------------------------------------------------------------- GUI runs


def _rewrite_provider_url(home, url):
    config = home / 'config.toml'
    if not config.exists():
        return {'rewritten': False, 'reason': 'config.toml is missing'}
    text = config.read_text(encoding='utf-8')
    updated, count = re.subn(
        r'(?m)^(\s*base_url\s*=\s*)"[^"]*"',
        lambda match: f'{match.group(1)}"{url}"',
        text,
    )
    if count == 0:
        return {'rewritten': False, 'reason': 'config.toml declares no base_url'}
    config.write_text(updated, encoding='utf-8')
    return {'rewritten': True, 'replacements': count, 'providerUrl': url}


def _run_scale_driver(root, env, run_dir, vm, label, thread_id, expected, timeout):
    command = [
        'cargo', 'dart', 'run',
        'test_driver/project_session_scale_driver.dart',
        '--vm-service-url', vm,
        '--output', str(run_dir),
        '--fixture', label,
        '--thread-id', thread_id,
    ]
    for name, value in expected.items():
        if value:
            command += [f'--{name}', str(value)]
    with (run_dir / 'driver.log').open('w') as log:
        subprocess.run(
            command,
            cwd=root,
            env=env,
            stdout=log,
            stderr=subprocess.STDOUT,
            check=True,
            timeout=timeout,
        )


def _read_json(path):
    if not path.exists():
        return None
    return json.loads(path.read_text(encoding='utf-8'))


def _run_once(root, base, fixture, label, repeat, output, args, user_home_before):
    run_dir = output / 'runs' / f'{label}-r{repeat}'
    run_dir.mkdir(parents=True, exist_ok=True)
    home = run_dir / 'studio-home'
    if home.exists():
        shutil.rmtree(home)
    # Each run gets its own copy so repeats never share mutated Studio state and
    # the built fixture stays reusable.
    shutil.copytree(fixture['home'], home, symlinks=True)

    provider = native.Provider
    provider.output = run_dir
    provider.index_path = run_dir / 'wire-index.jsonl'
    provider.index_path.write_text('')
    provider.full_body_limit = 0
    server = http.server.ThreadingHTTPServer(('127.0.0.1', 0), provider)
    serving = threading.Thread(target=server.serve_forever, daemon=True)
    serving.start()
    url = f'http://127.0.0.1:{server.server_port}'

    env = dict(
        os.environ,
        ANYWORK_HOME=str(home),
        GDK_BACKEND='x11',
        WAYLAND_DISPLAY='',
        LIBGL_ALWAYS_SOFTWARE='1',
    )
    env.pop('ANYWORK_DEMO', None)

    phase = f'{label}-r{repeat}'
    launch_started_ms = int(time.time() * 1000)
    summary = {
        'label': label,
        'repeat': repeat,
        'runDirectory': str(run_dir),
        'result': 'completed',
    }
    sampler = None
    try:
        summary['provider'] = _rewrite_provider_url(home, url)
        try:
            gui, log, vm = native._launch(root, run_dir, env, phase, args.display)
        except BaseException as error:  # noqa: BLE001 - evidence must survive
            # `_launch` already released the process tree it started.
            summary['result'] = 'launch-failed'
            summary['error'] = repr(error)
            gui = log = None
        else:
            summary['vmServiceReadyMs'] = int(time.time() * 1000) - launch_started_ms
            sampler = LeafProcessSampler(
                gui.pid,
                args.sample_interval_ms / 1000.0,
                run_dir / 'mem-samples.jsonl',
            )
            sampler.start()
        driver_error = None
        if gui is not None:
            try:
                _run_scale_driver(
                    root,
                    env,
                    run_dir,
                    vm,
                    label,
                    base.thread_id,
                    {
                        'expected-last-turn-id': base.session.get('lastTurnId'),
                        'expected-latest-item-id': base.session.get('latestItemId'),
                        'expected-large-item-id': base.session.get('largeItemId'),
                    },
                    timeout=args.driver_timeout,
                )
            except BaseException as error:  # noqa: BLE001 - evidence must survive
                driver_error = repr(error)
            finally:
                if sampler is not None:
                    summary['sampling'] = sampler.stop()
                native._reap(gui, log, run_dir, phase)
        if driver_error is not None:
            summary['result'] = 'driver-failed'
            summary['error'] = driver_error
    finally:
        server.shutdown()
        server.server_close()
        serving.join(timeout=5)

    scale = _read_json(run_dir / 'scale.json') or {}
    timeline = _read_json(run_dir / 'timeline.json') or {}
    summary.update({
        'providerUrl': url,
        'launchStartedAtMs': launch_started_ms,
        'timeline': timeline,
        'scale': scale,
        'cleanup': _read_json(run_dir / f'cleanup-{phase}.json'),
        'shutdown': _read_json(run_dir / 'shutdown.json'),
        'wireIndex': native._wire_index_summary(run_dir),
        'userHomePreserved': {
            'before': user_home_before,
            'after': native._home_marker_state(),
        },
    })
    if timeline:
        summary['stages'] = {
            'startupToFirstScreenMs': (
                timeline.get('startupAtMs', 0) - launch_started_ms
            ),
            'launchToFirstWindowMs': (
                timeline.get('firstWindowAtMs', 0) - launch_started_ms
            ),
        }
        if sampler:
            summary['memory'] = {
                'startup': _phase_summary(
                    sampler.samples,
                    launch_started_ms,
                    timeline.get('firstWindowAtMs') or launch_started_ms,
                    ('pssKb', 'rssKb', 'vmRssKb', 'vmHwmKb'),
                ),
                'opened': _phase_summary(
                    sampler.samples,
                    timeline.get('firstWindowAtMs') or launch_started_ms,
                    timeline.get('openedObservedEndAtMs')
                    or int(time.time() * 1000),
                    ('pssKb', 'rssKb', 'vmRssKb', 'vmHwmKb'),
                ),
            }
    (run_dir / 'run.json').write_text(
        json.dumps(summary, ensure_ascii=False, indent=2), encoding='utf-8'
    )
    return summary


# ------------------------------------------------------------------ reporting


def _comparison(runs):
    by_label = {}
    for run in runs:
        by_label.setdefault(run['label'], []).append(run)
    data = {}
    for label, entries in by_label.items():
        opened_window = [
            (e.get('scale', {}).get('window') or {}).get('historyCount')
            for e in entries
        ]
        data[label] = {
            'repeats': len(entries),
            'launchToFirstWindowMs': [
                e.get('stages', {}).get('launchToFirstWindowMs') for e in entries
            ],
            'startupToFirstScreenMs': [
                e.get('stages', {}).get('startupToFirstScreenMs') for e in entries
            ],
            'openedWindowItems': opened_window,
            'openedPssKb': [
                ((e.get('memory') or {}).get('opened') or {})
                .get('pssKb', {})
                .get('last')
                for e in entries
            ],
            'openedVmHwmKb': [
                ((e.get('memory') or {}).get('opened') or {})
                .get('vmHwmKb', {})
                .get('max')
                for e in entries
            ],
        }
    labels = list(data)
    return {
        'perSize': data,
        'labels': labels,
        'notes': [
            'data only: no pass/fail verdict is derived from these numbers',
            'compare like-for-like: same build, host, display and provider',
            'storage scale only; this does not measure provider or GUI hot-update cost',
        ],
    }


def _fixture_verdict(report):
    """Fail-closed verdict for one constructed fixture.

    A fixture whose schema shape, watermark, identity, Turn boundary or index
    report is wrong must stop the acceptance *before* any GUI is launched; the
    returned list is the exact reason set, never a summary judgement.
    """
    problems = []
    counts = report.get('counts') or {}
    target = report.get('targetItems')
    if not target:
        problems.append('the fixture report carries no target size')
    elif counts.get('history_items') != target:
        problems.append(
            f"history_items holds {counts.get('history_items')} rows, not {target}"
        )
    if (counts.get('history_turns') or 0) <= 0:
        problems.append('the fixture declares no Turn rows')
    bounds = counts.get('ordinalBounds') or [None, None]
    if len(bounds) < 2 or bounds[0] != 1:
        problems.append(f'history does not start at ordinal 1: {bounds}')

    indexes = report.get('indexes') or {}
    required = indexes.get('required')
    if not indexes.get('declared'):
        problems.append('the fixture report lists no declared index')
    if not isinstance(required, dict) or not required:
        # An absent verdict map must never read as "nothing was missing".
        problems.append('the fixture report carries no required-index verdicts')
    else:
        missing = [
            name for name in REQUIRED_INDEXES if required.get(name) is not True
        ]
        if missing:
            problems.append(f'declared history indexes are missing: {missing}')
        unknown = sorted(name for name in required if name not in REQUIRED_INDEXES)
        if unknown:
            problems.append(f'unexpected required-index entries: {unknown}')

    integrity = report.get('integrity') or {}
    if not integrity:
        problems.append('the integrity report is missing')
    identity_skipped = bool(integrity.get('identityScanSkipped'))
    if identity_skipped:
        problems.append(
            'the full payload identity scan was skipped; the fixture is '
            'unverified (--skip-identity-scan is diagnostic only)'
        )
    # Every check is enumerated by name: a report that silently omits one can
    # never be mistaken for "all checks passed".
    expected_checks = [name for name, _ in INTEGRITY_CHECKS]
    if not identity_skipped:
        expected_checks.append(IDENTITY_MISMATCH_CHECK[0])
    for name in expected_checks:
        if name not in integrity:
            problems.append(f'integrity check {name} is missing from the report')
            continue
        value = integrity[name]
        if isinstance(value, bool) or not isinstance(value, int):
            problems.append(f'integrity check {name} is not a row count: {value!r}')
        elif value != 0:
            problems.append(f'integrity check {name} reported {value} bad rows')
    unexpected = sorted(
        set(integrity) - set(expected_checks) - {'identityScanSkipped'}
    )
    if unexpected:
        problems.append(f'unexpected integrity checks in the report: {unexpected}')

    checkpoint = report.get('checkpoint') or {}
    for key in (
        'watermarkCoversFence',
        'fenceConsistentWithState',
        'threadIdMatchesState',
    ):
        if checkpoint.get(key) is not True:
            problems.append(f'checkpoint check {key} failed: {checkpoint}')

    queries = (report.get('explain') or {}).get('queries')
    if not queries:
        problems.append('the EXPLAIN QUERY PLAN report is missing')
    else:
        by_label = {
            query.get('label'): query
            for query in queries
            if isinstance(query, dict)
        }
        for label, _, _, _ in _explain_queries(1, 'sample'):
            query = by_label.get(label)
            if query is None:
                problems.append(f'{label}: the EXPLAIN report has no plan')
                continue
            if not query.get('plan'):
                problems.append(f'{label}: the EXPLAIN report carries no plan text')
            if 'problems' not in query or not isinstance(query['problems'], list):
                problems.append(f'{label}: the EXPLAIN report carries no verdict')
                continue
            for plan_problem in query['problems']:
                problems.append(f'{label}: {plan_problem}')
            if query.get('usesTemporaryBTree') and not any(
                'TEMP B-TREE' in str(plan_problem)
                for plan_problem in query['problems']
            ):
                problems.append(f'{label}: the plan materialises a TEMP B-TREE')
    return problems


def _run_verdict(summary):
    """Fail-closed verdict for one GUI run, derived from its recorded artifacts.

    The Dart driver already refuses a bad open, but the harness must not depend
    on that: a missing artifact, an unsampled GUI, an unclean process tree or a
    changed operator Studio home is a failure of this run by itself.
    """
    problems = []
    if summary.get('result') != 'completed':
        problems.append(
            f"run status is {summary.get('result')}: {summary.get('error')}"
        )

    scale = summary.get('scale') or {}
    if not scale:
        problems.append('scale.json is missing, so the open was never judged')
    else:
        window = scale.get('window') or {}
        history_count = window.get('historyCount')
        if not isinstance(history_count, int) or not (
            1 <= history_count <= SQL_HISTORY_WINDOW_LIMIT
        ):
            problems.append(
                'the first SQL history count is not a bounded page: '
                f'{history_count}'
            )
        if window.get('hasOlder') is not True:
            problems.append('the first window reports no older history')
        opened = scale.get('open') or {}
        if opened.get('workspaceBusy') is not False:
            problems.append('opening the Thread did not leave an idle workspace')
        if opened.get('latestItemMatched') is not True:
            problems.append(
                'the real latest item is not the newest item of the first window'
            )
        guard = scale.get('modelRequestGuard') or {}
        if guard.get('conversationDelta') != 0:
            problems.append(f'model requests were issued while opening: {guard}')

    shutdown = summary.get('shutdown') or {}
    if ((shutdown.get('reply') or {}).get('shutdown')) != 'completed':
        problems.append('Studio did not report a completed shutdown')

    cleanup = summary.get('cleanup') or {}
    if not cleanup:
        problems.append('process cleanup evidence is missing')
    elif cleanup.get('remaining'):
        problems.append(
            f"GUI descendants survived cleanup: {cleanup.get('remaining')}"
        )

    sampling = summary.get('sampling') or {}
    if (sampling.get('sampleCount') or 0) <= 0:
        problems.append('no leaf GUI process memory samples were collected')

    latency = (summary.get('stages') or {}).get('launchToFirstWindowMs')
    if not isinstance(latency, int) or latency < 0:
        problems.append('launch-to-first-window timing evidence is missing')

    if (summary.get('wireIndex') or {}).get('available') is not True:
        problems.append('the provider wire index is unavailable')

    preserved = summary.get('userHomePreserved') or {}
    if preserved.get('before') != preserved.get('after'):
        problems.append('the operator Studio home changed during this run')

    return problems


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument(
        'base',
        help='completed project_session_native_harness.py output directory',
    )
    parser.add_argument(
        '--output', required=True, help='scale acceptance output directory'
    )
    parser.add_argument(
        '--sizes',
        default=DEFAULT_SIZES,
        help=f'comma-separated total history item counts (default {DEFAULT_SIZES})',
    )
    parser.add_argument('--repeats', type=int, default=DEFAULT_REPEATS)
    parser.add_argument('--display', default=DEFAULT_DISPLAY)
    parser.add_argument(
        '--driver-timeout', type=int, default=DEFAULT_DRIVER_TIMEOUT
    )
    parser.add_argument(
        '--sample-interval-ms', type=int, default=DEFAULT_SAMPLE_INTERVAL_MS
    )
    parser.add_argument(
        '--rebuild', action='store_true', help='rebuild fixtures even if they exist'
    )
    parser.add_argument(
        '--skip-identity-scan',
        action='store_true',
        help=(
            'diagnostic only: skip the full-payload identity scan of every '
            'injected row; the fixture is then reported as unverified and the '
            'run fails closed'
        ),
    )
    parser.add_argument(
        '--fixtures-only',
        action='store_true',
        help='build and report the fixtures without launching the GUI',
    )
    args = parser.parse_args()
    sizes = [int(part) for part in args.sizes.split(',') if part.strip()]
    if len(sizes) < 2:
        parser.error('--sizes needs at least two sizes to compare')
    if args.repeats < 2 and not args.fixtures_only:
        print(
            'note: --repeats < 2 cannot interleave sizes; the comparison will be '
            'a single observation per size',
            file=sys.stderr,
        )

    root = Path(__file__).resolve().parents[3]
    base = BaseLayout(Path(args.base).resolve())
    output = Path(args.output).resolve()
    output.mkdir(parents=True, exist_ok=True)
    user_home_before = native._home_marker_state()
    base_before = base.fingerprint()
    workspace = _ensure_workspace(base.workspace)

    fixtures = {}
    fixture_problems = {}
    for size in sizes:
        label = _size_label(size)
        fixture_root = output / 'fixtures' / label
        home = fixture_root / 'studio-home'
        history_path = (
            fixture_root / 'studio-home' / 'sessions' / base.storage_key
            / 'history.sqlite'
        )
        if args.rebuild and fixture_root.exists():
            shutil.rmtree(fixture_root)
        if home.exists():
            report = _read_json(fixture_root / 'fixture.json')
            if report is None:
                raise SystemExit(
                    f'{fixture_root} exists without fixture.json; pass --rebuild'
                )
        else:
            fixture_root.mkdir(parents=True, exist_ok=True)
            home, report = build_fixture(
                base, fixture_root, size, args.skip_identity_scan
            )
        # Query plans are cheap to re-read and their judgement is part of the
        # verdict, so a reused fixture is always re-planned instead of trusting
        # a report shape written by an older revision of this tool.
        report['explain'] = _explain(history_path)
        (fixture_root / 'fixture.json').write_text(
            json.dumps(report, ensure_ascii=False, indent=2), encoding='utf-8'
        )
        problems = _fixture_verdict(report)
        fixtures[label] = {
            'home': home,
            'root': fixture_root,
            'report': report,
            'problems': problems,
        }
        fixture_problems[label] = problems
        required = (report.get('indexes') or {}).get('required') or {}
        indexes_ok = bool(required) and all(required.values())
        counts = report.get('counts') or {}
        print(
            f'fixture {label}: items={counts.get("history_items")} '
            f'turns={counts.get("history_turns")} '
            f'indexes={"ok" if indexes_ok else "missing"} '
            f'problems={len(problems)}'
        )
        for problem in problems:
            print(f'  fixture[{label}]: {problem}', file=sys.stderr)

    runs = []
    # A fixture that violates its own construction contract must stop the run
    # before any GUI is launched: an invalid fixture can only produce invalid
    # GUI evidence.
    if any(fixture_problems.values()):
        print(
            'fixture verification failed; the GUI phases were not launched',
            file=sys.stderr,
        )
    elif args.fixtures_only:
        print('--fixtures-only: the GUI phases were not requested')
    else:
        for repeat in range(1, args.repeats + 1):
            # Alternating order keeps one size from always paying the cold
            # build/cache cost of the pair.
            order = sizes if repeat % 2 else list(reversed(sizes))
            for size in order:
                label = _size_label(size)
                runs.append(
                    _run_once(
                        root,
                        base,
                        fixtures[label],
                        label,
                        repeat,
                        output,
                        args,
                        user_home_before,
                    )
                )

    run_problems = {
        f"{run['label']}-r{run['repeat']}": _run_verdict(run) for run in runs
    }
    for name, problems in run_problems.items():
        for problem in problems:
            print(f'  run[{name}]: {problem}', file=sys.stderr)
    fixture_failed = any(fixture_problems.values())
    runs_failed = any(run_problems.values())
    ok = not fixture_failed and not runs_failed
    exit_code = 2 if fixture_failed else (1 if runs_failed else 0)
    result = {
        'result': 'completed' if ok else 'failed',
        'exitCode': exit_code,
        'base': {
            'path': str(base.base),
            'threadId': base.thread_id,
            'storageKey': base.storage_key,
            'workspace': workspace,
            'fingerprintBefore': base_before,
            'fingerprintAfter': base.fingerprint(),
            'unchanged': base_before == base.fingerprint(),
        },
        'fixtures': {label: entry['report'] for label, entry in fixtures.items()},
        'fixtureVerdicts': fixture_problems,
        'runs': runs,
        'runVerdicts': run_problems,
        'comparison': _comparison(runs),
        'findings': [
            finding
            for run in runs
            for finding in (run.get('scale', {}) or {}).get('findings', [])
        ],
        'problems': [
            f'fixture[{label}]: {problem}'
            for label, problems in fixture_problems.items()
            for problem in problems
        ] + [
            f'run[{name}]: {problem}'
            for name, problems in run_problems.items()
            for problem in problems
        ],
        'notes': [
            'storage-scale acceptance: history rows, first window identity, memory',
            'no model request is expected during open; verified from wire-index.jsonl',
            'the base output directory and the operator Studio home are only read',
            'fail-closed: exit 0 only when every fixture and GUI run verified',
        ],
    }
    (output / 'result.json').write_text(
        json.dumps(result, ensure_ascii=False, indent=2), encoding='utf-8'
    )
    (output / 'home-preservation.json').write_text(
        json.dumps(
            {'before': user_home_before, 'after': native._home_marker_state()},
            ensure_ascii=False,
        ),
        encoding='utf-8',
    )
    print(
        json.dumps(
            {
                'result': result['result'],
                'exitCode': exit_code,
                'problems': len(result['problems']),
                'output': str(output),
            }
        )
    )
    return exit_code


if __name__ == '__main__':
    sys.exit(main())
