//! Real worker bootstrap and process reaping through the public client.
#![cfg(target_os = "linux")]
use pl_remote_helper::client::{ManagedWorker, WorkerClientError};
use std::os::unix::fs::PermissionsExt;

// A concurrent fork can temporarily inherit another test's writable script fd
// before exec closes it, making that script fail to execute with ETXTBSY.
static PROBE_PROCESS_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[tokio::test]
async fn real_worker_probe_stops_before_launching_business_process() {
    let _process_guard = PROBE_PROCESS_LOCK.lock().await;
    ManagedWorker::probe(std::path::Path::new(env!("CARGO_BIN_EXE_pl-remote-helper")))
        .await
        .unwrap();
}

#[tokio::test]
async fn exited_worker_preserves_path_exit_status_and_bounded_stderr() {
    let _process_guard = PROBE_PROCESS_LOCK.lock().await;
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("worker");
    std::fs::write(&path, "#!/bin/sh\necho fixture-startup-error >&2\nexit 7\n").unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
    let error = ManagedWorker::probe(&path).await.unwrap_err();
    match error {
        WorkerClientError::Bootstrap {
            executable,
            stderr,
            source,
        } => {
            assert_eq!(executable, path);
            assert!(
                stderr.contains("fixture-startup-error"),
                "stderr={stderr:?}; source={source:?}"
            );
            assert!(
                matches!(*source, WorkerClientError::WorkerExit(status) if status.code() == Some(7))
            );
        }
        error => panic!("unexpected bootstrap diagnostic: {error}"),
    }
}

#[tokio::test]
async fn old_worker_protocol_is_distinct_from_readiness_timeout() {
    let _process_guard = PROBE_PROCESS_LOCK.lock().await;
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("worker");
    std::fs::write(&path, "#!/usr/bin/env python3\nimport os,sys,time\nos.write(int(sys.argv[2]), b'{\"type\":\"ready\",\"protocolVersion\":0}\\n')\ntime.sleep(60)\n").unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
    let error = ManagedWorker::probe(&path).await.unwrap_err();
    assert!(
        matches!(error, WorkerClientError::Bootstrap { source, .. } if matches!(*source, WorkerClientError::VersionMismatch { actual: 0, .. }))
    );
}

#[tokio::test]
async fn silent_worker_times_out_and_is_reaped_before_return() {
    let _process_guard = PROBE_PROCESS_LOCK.lock().await;
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("worker");
    std::fs::write(&path, "#!/bin/sh\necho $$ > \"$0.pid\"\nexec sleep 60\n").unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
    let error = ManagedWorker::probe(&path).await.unwrap_err();
    assert!(
        matches!(&error, WorkerClientError::Bootstrap { source, .. } if matches!(source.as_ref(), WorkerClientError::ReadinessTimeout)),
        "unexpected probe failure: {error:?}"
    );
    let pid = std::fs::read_to_string(path.with_extension("pid")).unwrap();
    assert!(
        !std::path::Path::new("/proc").join(pid.trim()).exists(),
        "probe child must be reaped"
    );
}
