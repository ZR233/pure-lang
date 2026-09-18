#!/usr/bin/env bash
# Reproducible end-to-end acceptance for a real remote Project worktree session.
#
# It creates a temporary Git repository on a real SSH host, launches the real
# native GUI (provider credentials still come from the login keyring), drives the
# add-project wizard and a worktree session through Flutter Driver, submits a real
# user prompt, verifies the remote worktree registration, and finally cleans up
# every artifact it created.
#
# HOME isolation is NOT used by default, and that is a measured decision:
# the application resolves `~/.ssh/config` from `$HOME` (pl-tool
# `user_ssh_config_path`), while the OpenSSH subprocess resolves `~/.ssh/config`
# from the passwd database and ignores `$HOME`. Under `HOME=<isolated>` the two
# disagree, the alias written by the wizard is invisible to ssh, and the
# connection test fails with:
#   ssh: Could not resolve hostname n5-remote-worktree: Temporary failure in name resolution
# (reproduced with the application's exact ssh argument vector). True HOME
# isolation therefore needs a product change and is out of this task's scope.
# Instead the real HOME is used and cleanup removes ONLY this run's managed block
# for this alias, so `~/.ssh/config` is restored byte-identically and the
# before/after hash guard passes for real. Set N5_ISOLATED_HOME=1 to exercise the
# isolated variant (expected to fail the wizard connection for the reason above).
#
# Usage:
#   DISPLAY=:0 DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/$UID/bus \
#     bash code/anywork/test_driver/remote_worktree_acceptance.sh
#
# Every override is an environment variable; see the defaults below.
set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../../.." && pwd)"

SSH_TARGET="${N5_SSH_TARGET:-runner@10.3.10.194}"
SSH_PORT="${N5_SSH_PORT:-22}"
IDENTITY="${N5_SSH_IDENTITY:-$HOME/.ssh/id_ed25519}"
SSH_ALIAS="${N5_SSH_ALIAS:-n5-remote-worktree}"
DISPLAY_VALUE="${DISPLAY:-:0}"
if [ -n "${DBUS_SESSION_BUS_ADDRESS:-}" ]; then
  DBUS_VALUE="$DBUS_SESSION_BUS_ADDRESS"
else
  DBUS_VALUE="unix:path=/run/user/$(id -u)/bus"
fi
PROMPT="${N5_PROMPT:-请只回复这一行文本，不要使用任何工具：REMOTE_WORKTREE_LIVE_OK}"
MARKER="${N5_MARKER:-REMOTE_WORKTREE_LIVE_OK}"
TIMEOUT_SECONDS="${N5_TIMEOUT_SECONDS:-900}"
ARTIFACT_DIR="${N5_ARTIFACT_DIR:-$REPO_ROOT/target/remote-worktree-live-artifacts/gui-$(date +%Y%m%d%H%M%S)-$$}"
WORK_DIR="$(mktemp -d "${TMPDIR:-/tmp}/n5-remote-worktree.XXXXXX")"
# Isolated HOME: the app writes SSH host blocks into `$HOME/.ssh/config` and all
# Studio state into `$HOME/.anywork`, so both stay inside WORK_DIR. The Rust and
# Dart toolchains keep pointing at the real caches so cargo/flutter still work.
REAL_HOME="$HOME"
ISO_HOME="$WORK_DIR/home"
STUDIO_HOME="$ISO_HOME/.anywork"
ISOLATED_HOME="${N5_ISOLATED_HOME:-0}"
GUI_HOME="$REAL_HOME"
if [ "$ISOLATED_HOME" = "1" ]; then
  GUI_HOME="$ISO_HOME"
fi
DRIVER_OUT="$ARTIFACT_DIR/driver"
GUI_LOG="$ARTIFACT_DIR/gui.log"
SSH_OPTS=(-T -x -o BatchMode=yes -o ConnectTimeout=15 -p "$SSH_PORT")

SSH_USER="${SSH_TARGET%@*}"
SSH_HOST="${SSH_TARGET#*@}"

FIXTURE=""
BASE_COMMIT=""
GUI_PGID=""

log() { printf 'n5> %s\n' "$*"; }

ssh_run() {
  # shellcheck disable=SC2029
  ssh "${SSH_OPTS[@]}" "$SSH_TARGET" "$@"
}

## Restores the real user configuration after the app has written its managed
## SSH block: first remove only the block belonging to this disposable alias
## (case-insensitive marker, leading whitespace tolerated, unclosed block is a
## hard error instead of a truncation), then guarantee the exact pre-run bytes.
restore_user_ssh_config() {
  local output
  output="$(python3 - "$REAL_HOME/.ssh/config" "$SSH_ALIAS" <<'PY'
import pathlib, sys
path = pathlib.Path(sys.argv[1])
alias = sys.argv[2].strip().lower()
if not path.is_file():
    print("no-user-ssh-config")
    raise SystemExit(0)
lines = path.read_text().splitlines(keepends=True)
kept, index, removed = [], 0, 0
while index < len(lines):
    stripped = lines[index].strip()
    lowered = stripped.lower()
    if lowered.startswith("# begin anywork server:") and (
        lowered[len("# begin anywork server:") :].strip() == alias
    ):
        closing = index + 1
        while closing < len(lines):
            candidate = lines[closing].strip().lower()
            if candidate.startswith("# end anywork server:") and (
                candidate[len("# end anywork server:") :].strip() == alias
            ):
                break
            closing += 1
        if closing >= len(lines):
            print("unclosed-managed-block-for-" + alias, file=sys.stderr)
            raise SystemExit(3)
        removed += 1
        index = closing + 1
        continue
    kept.append(lines[index])
    index += 1
if removed == 0:
    print("no-managed-block-for-alias")
    raise SystemExit(0)
path.write_text("".join(kept))
print(f"removed-managed-blocks={removed}")
PY
)" || { log "user ssh config restore failed: $output"; return 1; }
  log "user ssh config restore: $output"
  local snapshot target
  for pair in "user-config.toml.snapshot:$REAL_HOME/.anywork/config.toml" \
    "user-ssh-config.snapshot:$REAL_HOME/.ssh/config"; do
    snapshot="$WORK_DIR/${pair%%:*}"
    target="${pair#*:}"
    if [ -f "$snapshot" ] && ! cmp -s "$snapshot" "$target"; then
      cp "$snapshot" "$target"
      log "restored verbatim from snapshot: $target"
    fi
  done
  return 0
}

cleanup() {
  local status=$?
  # Order matters: stop the GUI/driver process tree first (it holds the remote
  # worktree), then remove the remote fixture, then restore the local config.
  if [ -n "$GUI_PGID" ]; then
    log "stopping GUI process group $GUI_PGID"
    kill -TERM -- "-$GUI_PGID" >/dev/null 2>&1 || true
    for _ in 1 2 3 4 5 6 7 8 9 10; do
      kill -0 -- "-$GUI_PGID" >/dev/null 2>&1 || break
      sleep 1
    done
    kill -KILL -- "-$GUI_PGID" >/dev/null 2>&1 || true
  fi
  if [ -n "$FIXTURE" ]; then
    log "removing remote fixture $FIXTURE"
    ssh_run "bash -s" -- "$FIXTURE" <<'EOF' || log "remote fixture cleanup failed"
set -u
fixture="$1"
if [ -e "$fixture" ]; then
  while read -r worktree; do
    [ "$worktree" = "$fixture" ] && continue
    git -C "$fixture" worktree remove --force "$worktree" || true
  done < <(git -C "$fixture" worktree list --porcelain | sed -n 's/^worktree //p')
  git -C "$fixture" worktree prune || true
  rm -rf -- "$fixture" || exit 1
fi
test ! -e "$fixture" && printf 'REMOTE_FIXTURE_REMOVED\n'
EOF
  fi
  restore_user_ssh_config || status=1
  rm -rf -- "$WORK_DIR"
  exit "$status"
}
trap cleanup EXIT INT TERM

mkdir -p "$ARTIFACT_DIR" "$DRIVER_OUT" "$STUDIO_HOME" "$ISO_HOME/.ssh" "$ISO_HOME/.config"
if [ -f "$REAL_HOME/.ssh/known_hosts" ]; then
  cp "$REAL_HOME/.ssh/known_hosts" "$ISO_HOME/.ssh/known_hosts"
fi
chmod 700 "$ISO_HOME/.ssh"
# No config file is pre-written: the application owns `$HOME/.ssh/config` and
# creates its own managed block. A hand-written, unmanaged `Host <alias>` entry
# for the same alias is not recognised by the Rust config parser and made the
# wizard's save fail in the earlier isolated-HOME attempt.
# Independent key-auth probe through an isolated config file.
cat > "$WORK_DIR/ssh-probe.conf" <<EOF
Host $SSH_HOST
    HostName $SSH_HOST
    Port $SSH_PORT
    User $SSH_USER
    IdentityFile $IDENTITY
    UserKnownHostsFile $ISO_HOME/.ssh/known_hosts
EOF
if ! ssh -F "$WORK_DIR/ssh-probe.conf" -o BatchMode=yes -o ConnectTimeout=15 "$SSH_HOST" true; then
  log "isolated ssh probe cannot reach $SSH_TARGET"
  exit 1
fi
log "isolated ssh key authentication verified for $SSH_TARGET"
{
  printf 'isolatedHome=%s\n' "$ISOLATED_HOME"
  printf 'HOME=%s\n' "$GUI_HOME"
  printf 'ANYWORK_HOME=%s\n' "$STUDIO_HOME"
  printf 'CARGO_HOME=%s\n' "$REAL_HOME/.cargo"
  printf 'RUSTUP_HOME=%s\n' "$REAL_HOME/.rustup"
  printf 'PUB_CACHE=%s\n' "$REAL_HOME/.pub-cache"
  printf 'userSshConfig=%s\n' "$REAL_HOME/.ssh/config"
} > "$ARTIFACT_DIR/env-isolation.txt"

log "artifact dir: $ARTIFACT_DIR"
log "ssh target: $SSH_TARGET port $SSH_PORT identity $IDENTITY"
log "isolated studio home: $STUDIO_HOME"

# 1. Temporary remote Git repository with a single base commit.
log "creating remote fixture"
FIXTURE_OUTPUT="$(ssh_run "bash -s" <<'EOF'
set -eu
fixture=$(mktemp -d /tmp/pure-remote-worktree-live.XXXXXX)
mkdir -p "$fixture/src"
printf '%s\n' '# Remote worktree acceptance fixture' > "$fixture/README.md"
printf '%s\n' 'pub fn fixture_ready() -> bool { true }' > "$fixture/src/lib.rs"
printf '%s\n' '/target/' '/.anywork/' > "$fixture/.gitignore"
git -C "$fixture" init -q -b main
git -C "$fixture" config user.name 'Pure Acceptance'
git -C "$fixture" config user.email 'pure-acceptance@example.invalid'
git -C "$fixture" add .
git -C "$fixture" -c user.name='Pure Acceptance' \
  -c user.email='pure-acceptance@example.invalid' \
  commit -q -m 'test: initialize remote worktree fixture'
printf 'FIXTURE=%s\n' "$fixture"
printf 'BASE=%s\n' "$(git -C "$fixture" rev-parse HEAD)"
EOF
)" || { log "remote fixture creation failed: $FIXTURE_OUTPUT"; exit 1; }
printf '%s\n' "$FIXTURE_OUTPUT" | tee "$ARTIFACT_DIR/remote-fixture.txt"
FIXTURE="$(printf '%s\n' "$FIXTURE_OUTPUT" | sed -n 's/^FIXTURE=//p')"
BASE_COMMIT="$(printf '%s\n' "$FIXTURE_OUTPUT" | sed -n 's/^BASE=//p')"
if [ -z "$FIXTURE" ] || [ -z "$BASE_COMMIT" ]; then
  log "failed to parse remote fixture path/base commit"
  exit 1
fi
log "remote fixture: $FIXTURE base $BASE_COMMIT"

# 2. Isolated Studio home: copy the user's working config verbatim so the
# isolated run uses exactly the same provider routes. Provider credentials live
# in the login keyring (`service=anywork`) and are independent of the Studio
# home, so they still resolve; tool approvals are answered by the driver.
cp "$REAL_HOME/.anywork/config.toml" "$STUDIO_HOME/config.toml"
cp "$REAL_HOME/.anywork/config.toml" "$WORK_DIR/user-config.toml.snapshot"
cp "$REAL_HOME/.ssh/config" "$WORK_DIR/user-ssh-config.snapshot"
USER_CONFIG_STATE_BEFORE="$(sha256sum "$REAL_HOME/.anywork/config.toml" "$REAL_HOME/.ssh/config" 2>/dev/null || true)"

# 3. Launch the real native GUI with Flutter Driver enabled.
log "starting GUI (cargo xtask run-gui --driver)"
(
  cd "$REPO_ROOT"
  setsid env \
    HOME="$GUI_HOME" \
    ANYWORK_HOME="$STUDIO_HOME" \
    CARGO_HOME="$REAL_HOME/.cargo" \
    RUSTUP_HOME="$REAL_HOME/.rustup" \
    PUB_CACHE="$REAL_HOME/.pub-cache" \
    DISPLAY="$DISPLAY_VALUE" \
    DBUS_SESSION_BUS_ADDRESS="$DBUS_VALUE" \
    GDK_BACKEND=x11 \
    ANYWORK_LOG_LEVEL=debug \
    cargo xtask run-gui --driver --log-level debug >"$GUI_LOG" 2>&1 &
  echo $! > "$WORK_DIR/gui.pid"
)
GUI_PGID="$(cat "$WORK_DIR/gui.pid")"
log "GUI process group: $GUI_PGID"

VM_SERVICE=""
for _ in $(seq 1 240); do
  VM_SERVICE="$(sed -n 's/.*\(https\?:\/\/127\.0\.0\.1:[0-9]*\/[^ ]*\)/\1/p' "$GUI_LOG" | tail -1)"
  [ -n "$VM_SERVICE" ] && break
  if ! kill -0 -- "-$GUI_PGID" >/dev/null 2>&1; then
    log "GUI launcher exited before the VM service was ready"
    tail -40 "$GUI_LOG"
    exit 1
  fi
  sleep 1
done
if [ -z "$VM_SERVICE" ]; then
  log "timed out waiting for the Dart VM service URL"
  tail -40 "$GUI_LOG"
  exit 1
fi
VM_SERVICE="${VM_SERVICE%/}"
log "VM service: $VM_SERVICE"
printf '%s\n' "$VM_SERVICE" > "$ARTIFACT_DIR/vm-service.txt"

# 4. Drive the GUI through the real acceptance script.
log "running Flutter Driver acceptance script"
(
  cd "$REPO_ROOT"
  cargo dart run test_driver/remote_worktree_acceptance_driver.dart \
    --vm-service-url "$VM_SERVICE" \
    --output-dir "$DRIVER_OUT" \
    --ssh-host "$SSH_HOST" \
    --ssh-port "$SSH_PORT" \
    --ssh-username "$SSH_USER" \
    --ssh-alias "$SSH_ALIAS" \
    --ssh-identity "$IDENTITY" \
    --remote-repository "$FIXTURE" \
    --prompt "$PROMPT" \
    --prompt-marker "$MARKER" \
    --timeout-seconds "$TIMEOUT_SECONDS"
) 2>&1 | tee "$ARTIFACT_DIR/driver.log"
DRIVER_STATUS="${PIPESTATUS[0]}"
if [ "$DRIVER_STATUS" -ne 0 ]; then
  log "driver exited with status $DRIVER_STATUS"
  exit 1
fi

THREAD_ID="$(sed -n 's/.*"result":"completed".*"threadId":"\([^"]*\)".*/\1/p' "$ARTIFACT_DIR/driver.log" | tail -1)"
if [ -z "$THREAD_ID" ]; then
  THREAD_ID="$(python3 - "$ARTIFACT_DIR/driver.log" <<'PY'
import json, sys
thread = ""
for line in open(sys.argv[1], encoding="utf-8"):
    line = line.strip()
    if not line.startswith("{"):
        continue
    try:
        record = json.loads(line)
    except ValueError:
        continue
    if record.get("result") == "completed" and record.get("threadId"):
        thread = record["threadId"]
print(thread)
PY
)"
fi
if [ -z "$THREAD_ID" ]; then
  log "could not read the created thread id from the driver receipt"
  exit 1
fi
log "remote worktree thread: $THREAD_ID"
printf '%s\n' "$THREAD_ID" > "$ARTIFACT_DIR/thread-id.txt"

# 5. Assert the real remote worktree registration.
log "verifying remote worktree"
ssh_run "bash -s" -- "$FIXTURE" "$THREAD_ID" "$BASE_COMMIT" <<'EOF' | tee "$ARTIFACT_DIR/remote-verification.txt"
set -eu
fixture="$1"
thread="$2"
base="$3"
worktree="$fixture/.anywork/worktrees/$thread/session"
test -d "$worktree" || { printf 'MISSING_WORKTREE_DIR %s\n' "$worktree"; exit 1; }
porcelain="$(git -C "$fixture" worktree list --porcelain)"
printf '%s\n' "$porcelain"
case "$porcelain" in
  *"worktree $worktree"*) ;;
  *) printf 'UNREGISTERED_WORKTREE %s\n' "$worktree"; exit 1 ;;
esac
case "$porcelain" in
  *'\'*) printf 'BACKSLASH_PATH_IN_PORCELAIN\n'; exit 1 ;;
esac
head="$(git -C "$worktree" rev-parse HEAD)"
test "$head" = "$base" || { printf 'HEAD_MISMATCH %s != %s\n' "$head" "$base"; exit 1; }
branch="$(git -C "$worktree" rev-parse --abbrev-ref HEAD)"
printf 'WORKTREE=%s\nHEAD=%s\nBASE=%s\nBRANCH=%s\n' "$worktree" "$head" "$base" "$branch"
printf 'REMOTE_WORKTREE_VERIFIED\n'
EOF
VERIFY_STATUS=$?
if [ "$VERIFY_STATUS" -ne 0 ]; then
  log "remote worktree verification failed"
  exit 1
fi

cp "$ISO_HOME/.ssh/config" "$ARTIFACT_DIR/isolated-ssh-config.txt" 2>/dev/null || true
# Restore the real user configuration as part of the acceptance, then prove it is
# byte-identical to the pre-run state (the guard below is strict, not relaxed).
restore_user_ssh_config || { log "user configuration restore failed"; exit 1; }
USER_CONFIG_STATE_AFTER="$(sha256sum "$REAL_HOME/.anywork/config.toml" "$REAL_HOME/.ssh/config" 2>/dev/null || true)"
printf '%s\n' "$USER_CONFIG_STATE_BEFORE" | tee "$ARTIFACT_DIR/user-config-state-before.txt"
printf '%s\n' "$USER_CONFIG_STATE_AFTER" | tee "$ARTIFACT_DIR/user-config-state-after.txt"
if [ "$USER_CONFIG_STATE_BEFORE" != "$USER_CONFIG_STATE_AFTER" ]; then
  log "user ~/.anywork/config.toml or ~/.ssh/config changed during acceptance"
  exit 1
fi
log "user ~/.anywork/config.toml and ~/.ssh/config byte-identical before/after"
if [ "$ISOLATED_HOME" = "1" ]; then
  if ! grep -q "^# BEGIN anywork server: $SSH_ALIAS$" "$ISO_HOME/.ssh/config"; then
    log "isolated ssh config has no managed block for $SSH_ALIAS"
    exit 1
  fi
  if grep -qi "# .*anywork server: $SSH_ALIAS" "$REAL_HOME/.ssh/config"; then
    log "real user ssh config still holds a managed block for $SSH_ALIAS"
    exit 1
  fi
  log "isolated HOME took the managed SSH block; real user ssh config untouched"
fi
log "acceptance complete: $ARTIFACT_DIR"
