//! SSH 项目技能目录进入 Settings catalog 的真实 localhost 验收。
//!
//! 需要本机 sshd 与到 `localhost` 的免密公钥认证，以及
//! `--features embedded-remote-helpers` 提供的 helper 资产；仅手动执行：
//! `cargo test -p pl-studio-runtime --features embedded-remote-helpers \
//!   --test ssh_skills_acceptance -- --ignored --nocapture`

use std::fs;
use std::path::PathBuf;

use pl_core::remote::{SshAuth, SshServerProfile};
use pl_studio_runtime::{StudioHostKind, StudioRuntime, StudioRuntimeOptions};

fn remote_workspace() -> PathBuf {
    let dir = std::env::temp_dir().join("pure-ssh-skills-acceptance");
    fs::create_dir_all(dir.join("skills").join("accept-remote-skill")).unwrap();
    fs::write(
        dir.join("skills").join("accept-remote-skill").join("SKILL.md"),
        "---\nname: accept-remote-skill\ndescription: Remote project skill for SSH acceptance\n---\nBody\n",
    )
    .unwrap();
    dir
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires local sshd with passwordless localhost and embedded remote helpers"]
async fn ssh_project_skills_catalog_reaches_settings() -> anyhow::Result<()> {
    let home = tempfile::tempdir()?;
    let workspace = remote_workspace();
    let runtime = StudioRuntime::with_options(StudioRuntimeOptions {
        studio_home: Some(home.path().to_path_buf()),
        host: StudioHostKind::Test,
    })
    .await?;
    runtime.start_runtime().await?;

    let identity = format!("{}/.ssh/id_ed25519", std::env::var("HOME")?);
    let profile = runtime
        .save_ssh_server(
            SshServerProfile {
                id: "accept-ssh".to_string(),
                name: "accept-ssh".to_string(),
                host: "localhost".to_string(),
                port: 22,
                username: whoami(),
                auth: SshAuth::AgentOrKey {
                    identity_file: Some(identity),
                },
            },
            None,
        )
        .await?;

    let project = runtime
        .open_remote_project(&profile.id, workspace.to_string_lossy().into_owned())
        .await?;
    println!("ACCEPT remote project: {} -> {}", project.id, project.path);

    // 修复点 1：activate_project 的 SSH 分支现在触发远端技能发现。
    runtime.activate_project(&project.id).await?;
    let activated = runtime.read_skills_state(&project.id).await;
    println!(
        "ACCEPT after activate: state={:?}, skills={:?}",
        activated.state.kind(),
        activated.state.value().map(|data| data
            .catalog
            .skills
            .iter()
            .map(|skill| (skill.name.clone(), skill.source))
            .collect::<Vec<_>>())
    );

    // 修复点 2：显式 discover_skills 不再返回空缓存。
    let discovered = runtime.discover_skills(&project.id).await?;
    let catalog = discovered
        .state
        .value()
        .expect("SSH discovery must publish a catalog");
    let names: Vec<(String, String)> = catalog
        .catalog
        .snapshot()
        .skills
        .iter()
        .map(|skill| (skill.name.clone(), format!("{:?}", skill.source)))
        .collect();
    println!("ACCEPT discovered skills: {names:?}");

    let has = |name: &str| {
        catalog
            .catalog
            .snapshot()
            .skills
            .iter()
            .any(|skill| skill.name == name)
    };
    assert!(
        has("canvas-design"),
        "system skill canvas-design must appear"
    );
    assert!(has("docx"), "system skill docx must appear");
    assert!(
        has("skill-creator"),
        "system skill skill-creator must appear"
    );
    assert!(
        has("accept-remote-skill"),
        "remote project skill must appear"
    );
    // Thread Mode 目录是进程级投影，经 `read_thread_mode_catalog` 读取，
    // 不随 Project Skills catalog 返回。
    let modes = runtime.read_thread_mode_catalog();
    println!(
        "ACCEPT modes: {:?}",
        modes
            .modes
            .iter()
            .map(|m| m.id.as_str())
            .collect::<Vec<_>>()
    );
    assert!(
        modes
            .modes
            .iter()
            .any(|mode| mode.id.as_str() == pl_protocol::ThreadModeId::SIMPLE),
        "builtin mode must appear"
    );

    let search = runtime
        .search_skills(&project.id, "canvas design", 10)
        .await?;
    assert!(
        search
            .matches
            .iter()
            .any(|skill| skill.name == "canvas-design"),
        "search must find the system skill"
    );

    runtime.shutdown().await;
    fs::remove_dir_all(workspace).ok();
    println!("ACCEPT ALL PASS");
    Ok(())
}

fn whoami() -> String {
    std::env::var("USER")
        .or_else(|_| std::env::var("LOGNAME"))
        .unwrap_or_else(|_| "zhourui".to_string())
}
