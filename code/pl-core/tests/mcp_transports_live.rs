#![cfg(windows)]

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use pl_core::*;
use serde_json::json;

#[tokio::test]
async fn rmcp_stdio_transport_hides_unmanaged_descendant_console_and_cleans_tree() {
    let temp = fixture_temp_dir("stdio");
    tokio::fs::create_dir_all(&temp).await.unwrap();
    let pid_file = temp.join("server.pid");
    let console_window_file = temp.join("console-window.txt");
    let command_file = temp.join("stdio-server.cmd");
    tokio::fs::write(
        &command_file,
        format!("@echo off\r\n\"{}\" %*\r\n", fixture_executable().display()),
    )
    .await
    .unwrap();
    let request = connect_request(
        "stdio-fixture",
        McpServerConfig {
            transport: McpServerTransport::Stdio,
            command: Some(command_file.to_string_lossy().into_owned()),
            args: vec![
                "--spawn-stdio-child".to_string(),
                "--pid-file".to_string(),
                pid_file.to_string_lossy().into_owned(),
                "--console-window-file".to_string(),
                console_window_file.to_string_lossy().into_owned(),
            ],
            ..McpServerConfig::default()
        },
    );

    let connection = McpConnector::default().connect(request).await.unwrap();
    assert_real_call(&connection, "stdio").await;
    let pid = wait_for_pid(&pid_file).await;
    assert_eq!(wait_for_file(&console_window_file).await, "none");
    assert!(
        process_exists(pid),
        "stdio fixture process should be running"
    );

    connection.close().await;
    wait_for_process_exit(pid).await;
    let _ = tokio::fs::remove_dir_all(temp).await;
}

#[tokio::test]
async fn rmcp_streamable_http_transport_calls_tool() {
    assert_http_transport("--http", "streamableHttp").await;
}

#[tokio::test]
async fn rmcp_streamable_http_transport_connects_legacy_initialize_server() {
    assert_http_transport("--legacy-http", "legacyHttp").await;
}

async fn assert_http_transport(server_mode: &str, expected_transport: &str) {
    let temp = fixture_temp_dir("http");
    tokio::fs::create_dir_all(&temp).await.unwrap();
    let pid_file = temp.join("server.pid");
    let address = reserve_loopback_address();
    let mut command = tokio::process::Command::new(fixture_executable());
    command
        .args([
            server_mode,
            &address.to_string(),
            "--pid-file",
            &pid_file.to_string_lossy(),
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .kill_on_drop(true)
        .creation_flags(windows::Win32::System::Threading::CREATE_NO_WINDOW.0);
    let mut child = command.spawn().unwrap();
    wait_for_listener(address).await;

    let request = connect_request(
        "http-fixture",
        McpServerConfig {
            transport: McpServerTransport::StreamableHttp,
            url: Some(format!("http://{address}/mcp")),
            ..McpServerConfig::default()
        },
    );
    let connection = McpConnector::default().connect(request).await.unwrap();
    assert_real_call(&connection, expected_transport).await;
    connection.close().await;

    child.kill().await.unwrap();
    child.wait().await.unwrap();
    let _ = tokio::fs::remove_dir_all(temp).await;
}

async fn assert_real_call(connection: &pl_core::ConnectedMcp, transport: &str) {
    let tools = connection.list_tools().await.unwrap();
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0].name.as_ref(), "lookup");
    assert!(tools[0].output_schema.is_some());
    let result = connection
        .call_tool("lookup".to_string(), json!({ "query": "rmcp live" }))
        .await
        .unwrap();
    let value = serde_json::to_value(result).unwrap();
    assert_eq!(value["structuredContent"]["transport"], transport);
    assert_eq!(
        value["structuredContent"]["arguments"]["query"],
        "rmcp live"
    );
}

fn connect_request(server_id: &str, config: McpServerConfig) -> McpConnectRequest {
    McpConnectRequest {
        server_id: server_id.to_string(),
        server: EffectiveMcpServerConfig {
            id: server_id.to_string(),
            config,
            source_kind: McpServerSourceKind::User,
            source_label: "rmcp live fixture".to_string(),
            source_detail: None,
            status_kind: McpServerStatusKind::Enabled,
            status_message: None,
            mutation_policy: McpServerMutationPolicy::UserEditable,
            bearer_token: None,
            tool_effect: None,
        },
    }
}

fn fixture_executable() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_pl-mcp-test-server"))
}

fn fixture_temp_dir(transport: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("pl-rmcp-{transport}-{nonce}"))
}

fn reserve_loopback_address() -> std::net::SocketAddr {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.local_addr().unwrap()
}

async fn wait_for_listener(address: std::net::SocketAddr) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if tokio::net::TcpStream::connect(address).await.is_ok() {
            return;
        }
        assert!(Instant::now() < deadline, "HTTP fixture did not start");
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}

async fn wait_for_pid(path: &Path) -> u32 {
    wait_for_file(path).await.parse().unwrap()
}

async fn wait_for_file(path: &Path) -> String {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Ok(value) = tokio::fs::read_to_string(path).await {
            return value.trim().to_string();
        }
        assert!(Instant::now() < deadline, "fixture did not write {path:?}");
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}

fn process_exists(pid: u32) -> bool {
    let script = format!(
        "if (Get-Process -Id {pid} -ErrorAction SilentlyContinue) {{ exit 0 }} else {{ exit 1 }}"
    );
    std::process::Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

async fn wait_for_process_exit(pid: u32) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while process_exists(pid) {
        assert!(
            Instant::now() < deadline,
            "stdio fixture process {pid} survived connection close"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}
