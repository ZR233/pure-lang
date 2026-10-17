//! Optional discovery instances for directed native GUI observation.
use anyhow::{Context, Result, bail, ensure};
use pl_model::config::{
    ModelCatalogId, ProviderId, ProviderModelCatalogConfig, builtin_provider_catalog,
};
use pl_model::provider::ProviderConnectionMode;
use pl_provider_fixture::{
    MIMO_DISCOVERY_EXCLUDED_SLUGS, MODEL_CATALOG_CONTROL_FILE_NAME, ModelCatalogControl,
};
use pl_studio_runtime::config::StudioConfig;
use serde_json::Value;
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

/// Loopback discovery instances written into the isolated `config.toml`.
pub(super) const INSTANCES: [(&str, &str, &str); 4] = [
    ("catalog-openai", "openai", "openai"),
    ("catalog-openai-second", "openai", "openai-second"),
    ("catalog-deepseek", "deepseek", "deepseek"),
    ("catalog-mimo", "mimo-api", "mimo"),
];

pub(super) fn configure(home: &Path, base_url: &str) -> Result<()> {
    // Accept only an explicit IPv4 loopback endpoint; never consult real credentials.
    let remainder = base_url
        .strip_prefix("http://127.0.0.1:")
        .ok_or_else(|| anyhow::anyhow!("model catalog fixture must use IPv4 loopback HTTP"))?;
    let port = remainder.split('/').next().unwrap_or_default();
    ensure!(
        port.parse::<u16>().is_ok_and(|port| port != 0),
        "invalid model catalog fixture port"
    );
    let file = home.join("config.toml");
    let mut config: StudioConfig = toml::from_str(&fs::read_to_string(&file)?)?;
    let catalog = builtin_provider_catalog()?;
    for (id, preset, suffix) in INSTANCES {
        let preset = catalog
            .presets
            .iter()
            .find(|candidate| candidate.id.as_str() == preset)
            .ok_or_else(|| anyhow::anyhow!("model catalog fixture preset is missing"))?;
        let mut provider = preset.provider.clone();
        provider.name = id.to_owned();
        provider.base_url = format!("{}/{suffix}", base_url.trim_end_matches('/'));
        provider.bearer_token_env = None;
        provider.bearer_token = None;
        if id == "catalog-openai"
            && let ProviderModelCatalogConfig::Bundled {
                connection_overrides,
                auto_compact_overrides,
                ..
            } = &mut provider.catalog
        {
            connection_overrides.insert("fixture-model-old".into(), ProviderConnectionMode::Http);
            auto_compact_overrides.insert("fixture-model-old".into(), 12_000);
        }
        config
            .models
            .providers
            .insert(ProviderId::new(id)?, provider);
    }
    let openai = ProviderId::new("catalog-openai")?;
    for route in config
        .models
        .routes
        .values_mut()
        .chain(config.mode_model_routes.values_mut())
    {
        route.provider = openai.clone();
        route.model = "fixture-model-old".into();
        route.effort = None;
    }
    ensure_mimo_exclusions(&catalog)?;
    // Discovery instances keep withdrawn-slug routes and overrides before the
    // first successful observation; strict `validate` would reject them.
    config.validate_declarations()?;
    fs::write(file, toml::to_string_pretty(&config)?)?;
    Ok(())
}

fn ensure_mimo_exclusions(catalog: &pl_model::config::ProviderCatalogRegistry) -> Result<()> {
    let mimo = catalog
        .model_catalog(&ModelCatalogId::new("mimo")?)
        .ok_or_else(|| anyhow::anyhow!("mimo catalog is missing"))?;
    let product = mimo
        .discovery
        .as_ref()
        .map(|discovery| discovery.excluded_slugs.as_slice())
        .unwrap_or(&[]);
    ensure!(
        product == MIMO_DISCOVERY_EXCLUDED_SLUGS,
        "fixture MiMo excluded slugs {:?} diverge from product {:?}",
        MIMO_DISCOVERY_EXCLUDED_SLUGS,
        product
    );
    Ok(())
}

/// Materializes a named inventory or copies an operator-supplied control file.
pub(super) fn materialize_control(
    destination: &Path,
    inventory: Option<&str>,
    source: Option<&Path>,
) -> Result<ModelCatalogControl> {
    ensure!(
        inventory.is_some() || source.is_some(),
        "model catalog control requires --model-catalog-inventory or --model-catalog-control"
    );
    let control = match (inventory, source) {
        (None, Some(path)) => ModelCatalogControl::load(path)?,
        (Some(name), None) => ModelCatalogControl::named(name)?,
        (None, None) => {
            bail!(
                "model catalog control requires --model-catalog-inventory or --model-catalog-control"
            )
        }
        (Some(_), Some(_)) => {
            bail!("--model-catalog-inventory cannot be combined with --model-catalog-control")
        }
    };
    control.write(destination)?;
    Ok(control)
}

pub(super) fn default_control_path(working: &Path) -> std::path::PathBuf {
    working.join(MODEL_CATALOG_CONTROL_FILE_NAME)
}

/// Human-readable restart-driven checklist written next to screenshots/logs.
pub(super) fn write_runbook(
    output: &Path,
    home: &Path,
    control_path: &Path,
    control: &ModelCatalogControl,
    fixture_base_url: &str,
) -> Result<()> {
    let cache_dir = home.join("v2/model-catalogs");
    let config = home.join("config.toml");
    let runbook = format!(
        r#"# 在线目录发现人工验收 Runbook

同步只在启动初始化执行一次。改库存必须重启 GUI（以及读取控制文件的 fixture 进程）。不要热加载。

## 环境

- 隔离 home: `{home}`（临时目录，运行结束即删；`--scenario gui` 与 catalog 重启旅程可加 `--retain-environment` 保留整个隔离环境）
- config.toml: `{config}`（随隔离 home 删除；结束时脱敏副本写入证据目录 `{output}/config.toml`，重启旅程为 `{output}/config-phase-a.toml` / `{output}/config-phase-b.toml`）
- 控制文件（启动时读取一次）: `{control}`；证据副本 `{output}/model-catalog.json`
- 成功观察缓存: `{cache}`（随隔离 home 删除；协调器把 slug/`checkedAt`/`cachePath` 写进证据目录的 phase JSON）
- fixture `/v1` 前缀: `{base}`
- 证据目录: `{output}`（截图/快照/请求/日志沿用既有产物）
- 发现实例: `catalog-openai`（`{base}/openai/models`）、`catalog-openai-second`（`{base}/openai-second/models`）、`catalog-deepseek`（`{base}/deepseek/models`）、`catalog-mimo`（`{base}/mimo/models`，Chat 传输语义，仅需 `/models`）
- MiMo 排除 slug（产品 decode 后过滤，不由 fixture 删除）: {excluded}
- 当前控制文件库存:
{inventory}

命名库存用 `--model-catalog-inventory a|b|empty|reject-all|reject-second`；自定义文件用 `--model-catalog-control PATH`。每次运行都是全新隔离 home，改库存请重跑一次命令，不要热重启。

JSON 字段（每个 provider：`openai` / `openai-second` / `deepseek` / `mimo`）:

```json
{{
  "openai": {{ "models": ["gpt-6-astra"], "reject": false, "status": 401, "protocolError": false, "hang": false }}
}}
```

- `models`: 任意 slug 列表；空名单是合法成功替换。
- 失败开关互斥，判定顺序 **`hang` > `protocolError` > `status`/`reject`**：同时给出时只生效最先匹配的一项，不会静默组合。
- `hang: true`: 挂起直到进程停止（超时隔离）。
- `protocolError: true`: HTTP 200 + error envelope，验证协议失败回退。
- `status`: 非 2xx 验证 HTTP 失败回退。
- `reject: true`: 未给 `status` 时 HTTP 401。

## 1. 旧模型移除

一条命令完成第 1 项和第 5 项（同 home、同端口，必须重启 fixture + GUI）：

```text
cargo xtask manual-gui --scenario gui --model-catalog-inventory a --model-catalog-restart
```

状态机：库存 A 启动 → 协调器断言 `catalog-openai` 缓存 slug 集合含 `fixture-model-old`、`gpt-6-astra`、`gpt-6.1-sol` → 停 GUI（保留 home）→ 停 fixture（shutdown 后复制 `<evidence>/model-catalog-requests-a.json`）→ 写库存 B（去掉 `fixture-model-old` 与内置 slug `gpt-6.1-sol`，保留 `gpt-6-astra`；`openai-second` 改为 `gpt-6-sol`）→ **同一端口**重启 fixture → 同一 home 重启 GUI → 协调器断言 old 与 `gpt-6.1-sol` 消失、`gpt-6-astra` 仍在、`checkedAt` 前进，并记录被选中的 `cachePath`。阶段 A 把 `catalog-openai` 路由设为 `fixture-model-old` 并写入连接/压缩覆盖；阶段 B 断言 `config.toml` 仍保留该路由与覆盖。

协调器断言 = 缓存 slug 集合（present/absent）+ `checkedAt` 前进 + 路由与覆盖保留。`origin=online` / `source=online` 由第 6 项（`--model-catalog-inventory a` 的成功在线观测）的人工观察与截图确认（缓存文件无法直接断言 origin/source）。`--model-catalog-restart` 只产出协调器断言与缓存/日志证据，**不产出 GUI 截图**；要看来源徽标/`source=online` 需另跑一次 `--scenario gui --model-catalog-inventory a`。协调器失败即非零退出并打印缓存片段。改库存只刷新界面不够。

阶段 B 证据：`<evidence>/model-catalog-requests-b.json`（对应 fixture shutdown 后）、`catalog-phase-b-gui.log`、`fixture-restart.log`、`requests-restart.json`、`model-catalog-phase-a.json` / `model-catalog-phase-b.json`、脱敏 `config-phase-a.toml` / `config-phase-b.toml`。

## 2. 无观察回退

本工具每次运行都是全新隔离 home，并自带 fixture 子进程。不要在同一会话里改控制文件后热重启。

```text
cargo xtask manual-gui --scenario gui --model-catalog-inventory reject-all
```

1. 全新隔离 home（无既有 `{cache}` 成功观察）。
2. fixture 四实例均 `reject: true`（HTTP 401）。
3. 启动 GUI 后打开发现实例设置页。
4. 应观察：provider `status.source=default`（尚未同步），模型级 `origin=bundled`；bundled 默认模型重新出现。失败时 `error` 为类型化 Http/Protocol/Timeout，不是自由文本。
5. 证据：设置页截图、runtime 日志、本目录 `fixture.log`、`<evidence>/model-catalog-requests.json`（fixture 停止后复制）。

也可用自定义控制文件把该实例设为 `protocolError: true` 后同样重跑一次，验证协议失败回退。

## 3. 手动模型区分与删除

1. 在某一发现实例上新增手动模型（不与在线 slug 冲突）。
2. 应观察：GUI 「手动模型」分组 + 徽标；`{config}` 的 `additional_models` 写入该 slug。
3. 详情卡删除该手动模型。
4. 应观察：列表与 `additional_models` 同时消失；该 slug 的压缩阈值/连接覆盖一并清除。
5. 证据：设置页截图；结束时证据目录的脱敏 `{output}/config.toml`（`additional_models` 与该 slug 的连接/压缩覆盖应已消失）。删除前状态请自行另存一份，或加 `--retain-environment` 复核隔离 home 里的 `{config}`。

## 4. 计费不丢失

发现实例已挂 `/{{suffix}}/responses` 与 `/{{suffix}}/chat/completions`，剥前缀后委托既有严格脚本 `handle`（不复制推理实现）。已知在线 slug（`gpt-6-astra`）应显示内置价格行；未知在线 slug（`mimo-v2.6-pro`、`fixture-model-old`）显示未计价/空行。对已知 slug 发调用应产生 `estimated_costs`；对未知 slug 发调用应 `has_unpriced_usage=true`。历史会话费用在重启前后不变。

## 5. 路由不可用

由 `--model-catalog-restart` 同一旅程完成。阶段 A 将 `catalog-openai` 全部角色/模式路由设为 `fixture-model-old`，并写入该 slug 的连接覆盖与压缩阈值。阶段 B 断言 `config.toml` 路由与覆盖未被改写；有效目录不再含该 slug，选择以 unavailable 保留。对照证据目录的脱敏 `config-phase-a.toml` / `config-phase-b.toml`。

## 6. MiMo 发现

```text
cargo xtask manual-gui --scenario gui --model-catalog-inventory a
```

1. 控制文件 MiMo 段包含 `mimo-v2.5-pro`、`mimo-v2.5`、`mimo-v2.6-pro` 以及 4 个排除 slug。
2. 启动后打开 `catalog-mimo`。
3. 应观察：TTS/ASR 四个 slug 不出现；其余在线 slug 出现。排除由产品 catalog decode 完成，fixture 仍原样返回它们。
4. 证据：MiMo 列表截图、`<evidence>/model-catalog-requests.json`（path/status/provider/hits/rejected；`provider` 为实例名 `openai`/`openai-second`/`deepseek`/`mimo`）、`{cache}` 下 `model.json`、studio 日志。不要使用严格脚本的 `requests.json`。

## 7. 失败隔离

本工具每次运行都是全新隔离 home。验证失败隔离请单独重跑，不要依赖上一趟库存 A 的进程：

```text
cargo xtask manual-gui --scenario gui --model-catalog-inventory reject-second
```

1. `catalog-openai` 走库存 A（含 `gpt-6-astra`）；`catalog-openai-second` 返回 401。
2. 启动后打开两个实例。
3. 应观察：仅 `catalog-openai-second` 显示错误状态；`catalog-openai` / DeepSeek / MiMo 正常；首屏不受影响。
4. 证据：两个实例设置页截图、首屏截图、`gui.log`、`<evidence>/model-catalog-requests.json`。

超时隔离可用自定义控制文件把该实例设为 `hang: true` 后同样重跑一次。

完成后在本目录记录 `verdict.json` 与 `operations.json`，保持既有截图/快照/请求/日志机制。
"#,
        home = home.display(),
        config = config.display(),
        control = control_path.display(),
        cache = cache_dir.display(),
        base = fixture_base_url.trim_end_matches('/'),
        output = output.display(),
        excluded = MIMO_DISCOVERY_EXCLUDED_SLUGS.join(", "),
        inventory = inventory_summary(control),
    );
    fs::write(output.join("model-catalog-runbook.md"), runbook)
        .context("failed to write model catalog runbook")?;
    control
        .write(output.join(MODEL_CATALOG_CONTROL_FILE_NAME))
        .context("failed to copy model catalog control into evidence")?;
    Ok(())
}

fn inventory_summary(control: &ModelCatalogControl) -> String {
    format!(
        "- openai: models={:?} reject={} status={:?} protocolError={} hang={}\n- openai-second: models={:?} reject={} status={:?} protocolError={} hang={}\n- deepseek: models={:?} reject={} status={:?} protocolError={} hang={}\n- mimo: models={:?} reject={} status={:?} protocolError={} hang={}",
        control.openai.models,
        control.openai.reject,
        control.openai.status,
        control.openai.protocol_error,
        control.openai.hang,
        control.openai_second.models,
        control.openai_second.reject,
        control.openai_second.status,
        control.openai_second.protocol_error,
        control.openai_second.hang,
        control.deepseek.models,
        control.deepseek.reject,
        control.deepseek.status,
        control.deepseek.protocol_error,
        control.deepseek.hang,
        control.mimo.models,
        control.mimo.reject,
        control.mimo.status,
        control.mimo.protocol_error,
        control.mimo.hang,
    )
}

pub(super) const WITHDRAWN_SLUG: &str = "fixture-model-old";
pub(super) const KEPT_BUNDLED_SLUG: &str = "gpt-6-astra";
pub(super) const WITHDRAWN_BUNDLED_SLUG: &str = "gpt-6.1-sol";

#[derive(Debug, Clone, Copy)]
pub(super) struct CatalogPhaseExpectation {
    pub present: &'static [&'static str],
    pub absent: &'static [&'static str],
}

pub(super) fn phase_a_expectation() -> CatalogPhaseExpectation {
    CatalogPhaseExpectation {
        present: &[WITHDRAWN_SLUG, KEPT_BUNDLED_SLUG, WITHDRAWN_BUNDLED_SLUG],
        absent: &[],
    }
}

pub(super) fn phase_b_expectation() -> CatalogPhaseExpectation {
    CatalogPhaseExpectation {
        present: &[KEPT_BUNDLED_SLUG],
        absent: &[WITHDRAWN_SLUG, WITHDRAWN_BUNDLED_SLUG],
    }
}

pub(super) fn rewrite_inventory_b(path: &Path) -> Result<()> {
    ModelCatalogControl::inventory_b().write(path)
}

pub(super) fn bind_port(base_url: &str) -> Result<u16> {
    let remainder = base_url
        .strip_prefix("http://127.0.0.1:")
        .context("fixture base_url must use IPv4 loopback")?;
    let port = remainder
        .split('/')
        .next()
        .context("fixture base_url missing port")?;
    port.parse().context("fixture base_url port is not u16")
}

pub(super) struct CatalogCacheAssertion {
    pub checked_at: i64,
    pub cache_path: String,
}

pub(super) fn wait_for_online_cache(
    home: &Path,
    expected: CatalogPhaseExpectation,
    previous_checked_at: Option<i64>,
) -> Result<CatalogCacheAssertion> {
    let deadline = Instant::now() + Duration::from_secs(45);
    let mut last = String::from("(no cache yet)");
    while Instant::now() < deadline {
        match read_openai_cache(home) {
            Ok((path, cache)) => {
                last = serde_json::to_string(&cache).unwrap_or_else(|_| cache.to_string());
                if let Ok(checked) = assert_openai_cache(&cache, expected, previous_checked_at) {
                    return Ok(CatalogCacheAssertion {
                        checked_at: checked,
                        cache_path: path.display().to_string(),
                    });
                }
            }
            Err(error) => last = error.to_string(),
        }
        thread::sleep(Duration::from_millis(250));
    }
    bail!("catalog cache assertion failed; last fragment: {last}")
}

fn read_openai_cache(home: &Path) -> Result<(PathBuf, Value)> {
    let root = home.join("v2/model-catalogs");
    let mut matching = Vec::new();
    collect_openai_caches(&root, &mut matching)?;
    matching
        .into_iter()
        .max_by_key(|(modified, _, _)| *modified)
        .map(|(_, path, cache)| (path, cache))
        .context("no catalog-openai model.json under v2/model-catalogs")
}

fn collect_openai_caches(
    dir: &Path,
    matching: &mut Vec<(std::time::SystemTime, PathBuf, Value)>,
) -> Result<()> {
    if !dir.is_dir() {
        return Ok(());
    }
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_openai_caches(&path, matching)?;
            continue;
        }
        if path.file_name().and_then(|name| name.to_str()) != Some("model.json") {
            continue;
        }
        let cache: Value = serde_json::from_slice(&fs::read(&path)?)?;
        // catalog-openai uniquely lists gpt-6-astra; openai-second uses gpt-6-sol.
        if cache_slugs(&cache)
            .iter()
            .any(|slug| slug == KEPT_BUNDLED_SLUG)
        {
            matching.push((entry.metadata()?.modified()?, path, cache));
        }
    }
    Ok(())
}

fn assert_openai_cache(
    cache: &Value,
    expected: CatalogPhaseExpectation,
    previous_checked_at: Option<i64>,
) -> Result<i64> {
    let slugs = cache_slugs(cache);
    for slug in expected.present {
        ensure!(
            slugs.iter().any(|value| value == slug),
            "expected {slug} in cache, had {slugs:?}"
        );
    }
    for slug in expected.absent {
        ensure!(
            !slugs.iter().any(|value| value == slug),
            "did not expect {slug} in cache, had {slugs:?}"
        );
    }
    let checked = cache
        .get("checkedAt")
        .and_then(Value::as_i64)
        .context("cache missing checkedAt")?;
    if let Some(previous) = previous_checked_at {
        ensure!(
            checked > previous,
            "checkedAt {checked} did not advance past {previous}"
        );
    }
    Ok(checked)
}

fn cache_slugs(cache: &Value) -> Vec<String> {
    cache
        .get("models")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|model| {
            model
                .get("slug")
                .or_else(|| model.get("id"))
                .and_then(Value::as_str)
                .map(str::to_owned)
        })
        .collect()
}

pub(super) fn assert_route_preserved(home: &Path) -> Result<()> {
    let config: StudioConfig = toml::from_str(&fs::read_to_string(home.join("config.toml"))?)?;
    let openai = config
        .models
        .providers
        .get(&ProviderId::new("catalog-openai")?)
        .context("catalog-openai missing after restart")?;
    ensure!(
        config
            .models
            .routes
            .values()
            .chain(config.mode_model_routes.values())
            .all(|route| route.provider.as_str() == "catalog-openai"
                && route.model == WITHDRAWN_SLUG),
        "routes were rewritten across catalog restart"
    );
    match &openai.catalog {
        ProviderModelCatalogConfig::Bundled {
            connection_overrides,
            auto_compact_overrides,
            ..
        } => {
            ensure!(
                connection_overrides.get(WITHDRAWN_SLUG) == Some(&ProviderConnectionMode::Http),
                "connection override for {WITHDRAWN_SLUG} was removed"
            );
            ensure!(
                auto_compact_overrides.get(WITHDRAWN_SLUG) == Some(&12_000),
                "auto-compact override for {WITHDRAWN_SLUG} was removed"
            );
        }
        _ => bail!("catalog-openai is no longer a bundled catalog"),
    }
    Ok(())
}

pub(super) fn copy_hits(source: &Path, dest: &Path, required: bool) -> Result<()> {
    if source.is_file() {
        fs::copy(source, dest)?;
        return Ok(());
    }
    eprintln!(
        "catalog hits missing after fixture shutdown: {}",
        source.display()
    );
    ensure!(
        !required,
        "catalog hits missing after fixture shutdown: {}",
        source.display()
    );
    Ok(())
}

/// Non-interactive coordinator check: bind, rewrite inventory B, rebind the same port.
pub(super) fn restart_bind_check(executable: &Path, control_path: &Path) -> Result<()> {
    let first = spawn_bind_fixture(executable, control_path, None)?;
    let port = first.port;
    stop_child(first.child);
    rewrite_inventory_b(control_path)?;
    let second = spawn_bind_fixture(executable, control_path, Some(port))?;
    ensure!(
        second.port == port,
        "restart rebound {port} as {}",
        second.port
    );
    stop_child(second.child);
    Ok(())
}

struct BindFixture {
    child: std::process::Child,
    port: u16,
}

fn spawn_bind_fixture(
    executable: &Path,
    control_path: &Path,
    port: Option<u16>,
) -> Result<BindFixture> {
    let ready = control_path.with_extension("ready.json");
    let _ = fs::remove_file(&ready);
    let mut command = Command::new(executable);
    command
        .arg("--scenario")
        .arg("gui")
        .arg("--ready-file")
        .arg(&ready)
        .arg("--model-catalog-control")
        .arg(control_path)
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    if let Some(port) = port {
        command.arg("--bind-port").arg(port.to_string());
    }
    let mut child = command
        .spawn()
        .context("failed to start catalog bind-check fixture")?;
    let deadline = Instant::now() + Duration::from_secs(20);
    while Instant::now() < deadline {
        if ready.is_file() {
            let ready: Value = serde_json::from_slice(&fs::read(&ready)?)?;
            let url = ready
                .get("base_url")
                .and_then(Value::as_str)
                .context("ready file missing base_url")?;
            return Ok(BindFixture {
                child,
                port: bind_port(url)?,
            });
        }
        if child.try_wait()?.is_some() {
            bail!("catalog bind-check fixture exited before ready-file");
        }
        thread::sleep(Duration::from_millis(50));
    }
    stop_child(child);
    bail!("catalog bind-check fixture did not publish ready-file")
}

fn stop_child(mut child: std::process::Child) {
    let _ = child.kill();
    let _ = child.wait();
}
