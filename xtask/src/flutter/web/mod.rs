use crate::process;
use anyhow::{Context, Result, bail};
use std::fs;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

const DRIVER_READY_TIMEOUT: Duration = Duration::from_secs(5);
const DRIVER_EXIT_TIMEOUT: Duration = Duration::from_secs(3);
const DRIVER_POLL_INTERVAL: Duration = Duration::from_millis(50);
const DRIVER_START_ATTEMPTS: usize = 3;

#[derive(Debug, Clone)]
pub(super) struct WebDriverEnvironment {
    browser: PathBuf,
    chromedriver: PathBuf,
}

impl WebDriverEnvironment {
    pub(super) fn discover() -> Result<Self> {
        let browser = discover_browser()?;
        let chromedriver = which::which("chromedriver").ok();
        if browser.is_none() || chromedriver.is_none() {
            bail!(
                "{}",
                missing_webdriver_tools_diagnostic(browser.is_none(), chromedriver.is_none())
            );
        }
        let browser_launcher = browser.context("Chrome/Chromium disappeared during discovery")?;
        let chromedriver = chromedriver.context("ChromeDriver disappeared during discovery")?;
        let browser_version = read_version(&browser_launcher, "Chrome/Chromium")?;
        let driver_version = read_version(&chromedriver, "ChromeDriver")?;
        ensure_matching_major_versions(&browser_version, &driver_version)?;
        let browser = resolve_browser_for_driver(&browser_launcher)?;
        println!("Flutter Web Driver tools:");
        if browser == browser_launcher {
            println!("  Browser: {} ({browser_version})", browser.display());
        } else {
            println!(
                "  Browser: {} -> {} ({browser_version})",
                browser_launcher.display(),
                browser.display()
            );
        }
        println!(
            "  ChromeDriver: {} ({driver_version})",
            chromedriver.display()
        );
        Ok(Self {
            browser,
            chromedriver,
        })
    }

    pub(super) fn browser(&self) -> &Path {
        &self.browser
    }

    pub(super) fn start(&self, artifacts_dir: &Path) -> Result<RunningChromeDriver> {
        process::own_current_process_tree().context("failed to own ChromeDriver process tree")?;
        for attempt in 1..=DRIVER_START_ATTEMPTS {
            let port = reserve_local_port()?;
            match self.start_once(port, artifacts_dir) {
                Err(error)
                    if attempt < DRIVER_START_ATTEMPTS
                        && address_conflict(&format!("{error:#}")) =>
                {
                    eprintln!(
                        "ChromeDriver port {port} was claimed concurrently; retrying ({attempt}/{DRIVER_START_ATTEMPTS})."
                    );
                }
                result => return result,
            }
        }
        bail!("ChromeDriver startup exhausted all retry attempts")
    }

    fn start_once(&self, port: u16, artifacts_dir: &Path) -> Result<RunningChromeDriver> {
        let log_path = artifacts_dir.join(format!("chromedriver-{port}.log"));
        remove_stale_log(&log_path)?;
        let mut command = Command::new(&self.chromedriver);
        command
            .arg(format!("--port={port}"))
            .arg(format!("--log-path={}", log_path.display()))
            .arg("--log-level=INFO")
            .stdin(Stdio::null())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit());
        configure_process_group(&mut command);
        process::configure_background_command(&mut command);
        let child = command.spawn().with_context(|| {
            format!(
                "failed to start ChromeDriver: {}",
                self.chromedriver.display()
            )
        })?;
        wait_until_ready(child, port, log_path)
    }
}

pub(super) struct RunningChromeDriver {
    child: Option<Child>,
    port: u16,
    log_path: PathBuf,
}

impl RunningChromeDriver {
    pub(super) fn port(&self) -> u16 {
        self.port
    }

    pub(super) fn stop(mut self, retain_log: bool) -> Result<String> {
        let mut child = self
            .child
            .take()
            .context("ChromeDriver process was already released")?;
        terminate_process_tree(&mut child)?;
        let log = read_driver_log(&self.log_path)?;
        if !retain_log {
            remove_stale_log(&self.log_path)?;
        }
        Ok(log)
    }
}

impl Drop for RunningChromeDriver {
    fn drop(&mut self) {
        if let Some(child) = self.child.as_mut() {
            let _ = terminate_process_tree(child);
        }
    }
}

fn reserve_local_port() -> Result<u16> {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .context("failed to reserve a local ChromeDriver port")?;
    listener
        .local_addr()
        .context("failed to inspect reserved ChromeDriver port")
        .map(|address| address.port())
}

fn wait_until_ready(mut child: Child, port: u16, log_path: PathBuf) -> Result<RunningChromeDriver> {
    let address = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port);
    let deadline = Instant::now() + DRIVER_READY_TIMEOUT;
    while Instant::now() < deadline {
        if let Some(status) = child
            .try_wait()
            .context("failed to inspect ChromeDriver startup")?
        {
            bail!(
                "ChromeDriver exited before becoming ready with {status}.\n原始输出已透传；ChromeDriver 日志:\n{}",
                read_driver_log(&log_path)?
            );
        }
        if TcpStream::connect_timeout(&address, DRIVER_POLL_INTERVAL).is_ok() {
            println!("ChromeDriver ready on 127.0.0.1:{port}.");
            return Ok(RunningChromeDriver {
                child: Some(child),
                port,
                log_path,
            });
        }
        thread::sleep(DRIVER_POLL_INTERVAL);
    }

    terminate_process_tree(&mut child)?;
    bail!(
        "ChromeDriver did not become ready within {} seconds.\n原始输出已透传；ChromeDriver 日志:\n{}",
        DRIVER_READY_TIMEOUT.as_secs(),
        read_driver_log(&log_path)?
    );
}

fn remove_stale_log(path: &Path) -> Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error).with_context(|| format!("failed to remove {}", path.display())),
    }
}

fn discover_browser() -> Result<Option<PathBuf>> {
    match std::env::var_os("CHROME_EXECUTABLE") {
        Some(value) if value.is_empty() => {
            bail!("CHROME_EXECUTABLE is set but empty")
        }
        Some(value) => {
            let browser = which::which(&value).with_context(|| {
                format!(
                    "CHROME_EXECUTABLE does not resolve to an executable: {}",
                    PathBuf::from(value).display()
                )
            })?;
            return Ok(Some(browser));
        }
        None => {}
    }

    for candidate in [
        "google-chrome",
        "google-chrome-stable",
        "chromium",
        "chromium-browser",
        "chrome",
    ] {
        if let Ok(browser) = which::which(candidate) {
            return Ok(Some(browser));
        }
    }
    Ok(None)
}

fn resolve_browser_for_driver(launcher: &Path) -> Result<PathBuf> {
    let Some(application) = snap_application_name(launcher) else {
        return Ok(launcher.to_owned());
    };
    let snap_root = Path::new("/snap").join(application).join("current");
    find_browser_payload(&snap_root)?.with_context(|| {
        format!(
            "Snap browser launcher {} cannot be handed back to its confined ChromeDriver, and no executable browser payload was found under {}. Reinstall the matching browser/driver package or set CHROME_EXECUTABLE to a directly executable Chrome/Chromium binary.",
            launcher.display(),
            snap_root.display()
        )
    })
}

fn snap_application_name(launcher: &Path) -> Option<&std::ffi::OsStr> {
    let relative = launcher.strip_prefix("/snap/bin").ok()?;
    (relative.components().count() == 1)
        .then(|| relative.file_name())
        .flatten()
}

fn find_browser_payload(root: &Path) -> Result<Option<PathBuf>> {
    let mut candidates = Vec::new();
    collect_browser_payloads(root, 0, &mut candidates)?;
    candidates.sort_by(|left, right| left.1.cmp(&right.1).then_with(|| right.0.cmp(&left.0)));
    Ok(candidates.pop().map(|(path, _)| path))
}

fn collect_browser_payloads(
    directory: &Path,
    depth: usize,
    candidates: &mut Vec<(PathBuf, u64)>,
) -> Result<()> {
    const MAX_PAYLOAD_DEPTH: usize = 6;
    if depth > MAX_PAYLOAD_DEPTH {
        return Ok(());
    }
    for entry in fs::read_dir(directory).with_context(|| {
        format!(
            "failed to inspect browser package at {}",
            directory.display()
        )
    })? {
        let entry =
            entry.with_context(|| format!("failed to read entry in {}", directory.display()))?;
        let path = entry.path();
        let metadata = entry
            .metadata()
            .with_context(|| format!("failed to inspect {}", path.display()))?;
        if metadata.is_dir() {
            collect_browser_payloads(&path, depth + 1, candidates)?;
        } else if metadata.is_file()
            && is_browser_payload_name(&entry.file_name())
            && is_executable(&metadata)
        {
            candidates.push((path, metadata.len()));
        }
    }
    Ok(())
}

fn is_browser_payload_name(name: &std::ffi::OsStr) -> bool {
    matches!(
        name.to_str(),
        Some("chrome" | "chromium" | "chromium-browser")
    )
}

#[cfg(unix)]
fn is_executable(metadata: &fs::Metadata) -> bool {
    use std::os::unix::fs::PermissionsExt;

    metadata.permissions().mode() & 0o111 != 0
}

#[cfg(not(unix))]
fn is_executable(_metadata: &fs::Metadata) -> bool {
    true
}

fn read_version(program: &Path, label: &str) -> Result<String> {
    let output = Command::new(program)
        .arg("--version")
        .output()
        .with_context(|| format!("failed to start {label}: {}", program.display()))?;
    let raw_output = combined_output(&output.stdout, &output.stderr);
    if !output.status.success() {
        bail!(
            "{label} version probe failed: {} --version\n原始输出:\n{raw_output}",
            program.display()
        );
    }
    Ok(raw_output.trim().to_owned())
}

fn ensure_matching_major_versions(browser: &str, driver: &str) -> Result<()> {
    let browser_major = major_version(browser)
        .with_context(|| format!("cannot parse Chrome/Chromium version from: {browser}"))?;
    let driver_major = major_version(driver)
        .with_context(|| format!("cannot parse ChromeDriver version from: {driver}"))?;
    if browser_major != driver_major {
        bail!(
            "Chrome/Chromium 与 ChromeDriver 主版本不匹配：browser={browser_major}, driver={driver_major}。请安装版本匹配的浏览器与 chromedriver。\nBrowser: {browser}\nChromeDriver: {driver}"
        );
    }
    Ok(())
}

fn major_version(version: &str) -> Option<u32> {
    version
        .split(|character: char| !(character.is_ascii_digit() || character == '.'))
        .filter(|candidate| candidate.contains('.'))
        .find_map(|candidate| candidate.split('.').next()?.parse().ok())
}

fn read_driver_log(path: &Path) -> Result<String> {
    match fs::read_to_string(path) {
        Ok(log) if log.trim().is_empty() => Ok("(ChromeDriver 日志为空)".to_owned()),
        Ok(log) => Ok(log),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            Ok("(ChromeDriver 未生成日志)".to_owned())
        }
        Err(error) => Err(error).with_context(|| format!("failed to read {}", path.display())),
    }
}

fn combined_output(stdout: &[u8], stderr: &[u8]) -> String {
    let stdout = String::from_utf8_lossy(stdout);
    let stderr = String::from_utf8_lossy(stderr);
    match (stdout.trim().is_empty(), stderr.trim().is_empty()) {
        (true, true) => "(命令没有输出)".to_owned(),
        (false, true) => stdout.into_owned(),
        (true, false) => stderr.into_owned(),
        (false, false) => format!("{stdout}\n{stderr}"),
    }
}

fn missing_webdriver_tools_diagnostic(browser_missing: bool, driver_missing: bool) -> String {
    let mut missing = Vec::new();
    if browser_missing {
        missing.push("Chrome/Chromium browser");
    }
    if driver_missing {
        missing.push("matching chromedriver");
    }
    format!(
        "Flutter Web 远程验收缺少: {}。\n请安装版本匹配的 Chrome/Chromium 与 ChromeDriver，将 chromedriver 加入 PATH；浏览器不在 PATH 时设置 CHROME_EXECUTABLE。安装后重试 'cargo xtask verify-gui --web-integration'。xtask 会自行选择空闲端口并托管 ChromeDriver 生命周期。",
        missing.join(", ")
    )
}

fn address_conflict(error: &str) -> bool {
    let error = error.to_ascii_lowercase();
    error.contains("address already in use")
        || error.contains("only one usage of each socket address")
}

#[cfg(unix)]
fn configure_process_group(command: &mut Command) {
    use std::os::unix::process::CommandExt;

    command.process_group(0);
}

#[cfg(not(unix))]
fn configure_process_group(command: &mut Command) {
    let _ = command;
}

fn terminate_process_tree(child: &mut Child) -> Result<()> {
    if child
        .try_wait()
        .context("failed to inspect ChromeDriver before cleanup")?
        .is_some()
    {
        return Ok(());
    }

    #[cfg(unix)]
    terminate_unix_process_group(child)?;
    #[cfg(not(unix))]
    {
        child
            .kill()
            .context("failed to stop ChromeDriver after Web integration")?;
        child
            .wait()
            .context("failed to wait for ChromeDriver after Web integration")?;
    }
    Ok(())
}

#[cfg(unix)]
fn terminate_unix_process_group(child: &mut Child) -> Result<()> {
    signal_process_group(child, libc::SIGTERM)?;
    let deadline = Instant::now() + DRIVER_EXIT_TIMEOUT;
    while Instant::now() < deadline {
        if child
            .try_wait()
            .context("failed to wait for ChromeDriver process group")?
            .is_some()
        {
            return Ok(());
        }
        thread::sleep(DRIVER_POLL_INTERVAL);
    }

    signal_process_group(child, libc::SIGKILL)?;
    child
        .wait()
        .context("failed to reap ChromeDriver process group after SIGKILL")?;
    Ok(())
}

#[cfg(unix)]
fn signal_process_group(child: &Child, signal: libc::c_int) -> Result<()> {
    let process_group = i32::try_from(child.id()).context("ChromeDriver PID exceeds pid_t")?;
    // SAFETY: start_once places ChromeDriver in a process group whose id is its child PID.
    // A negative pid targets only that owned group; the signal value is a libc constant.
    let result = unsafe { libc::kill(-process_group, signal) };
    if result == 0 {
        return Ok(());
    }
    let error = std::io::Error::last_os_error();
    if error.raw_os_error() == Some(libc::ESRCH) {
        Ok(())
    } else {
        Err(error).context("failed to signal ChromeDriver process group")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn missing_browser_and_driver_are_reported_together() {
        let diagnostic = missing_webdriver_tools_diagnostic(true, true);

        assert!(diagnostic.contains("Chrome/Chromium browser, matching chromedriver"));
        assert!(diagnostic.contains("CHROME_EXECUTABLE"));
        assert!(diagnostic.contains("自行选择空闲端口"));
    }

    #[test]
    fn webdriver_output_preserves_both_streams() {
        assert_eq!(
            combined_output(b"driver stdout\n", b"driver stderr\n"),
            "driver stdout\n\ndriver stderr\n"
        );
    }

    #[test]
    fn diagnostic_can_name_only_the_missing_component() {
        assert!(
            missing_webdriver_tools_diagnostic(false, true).contains("缺少: matching chromedriver")
        );
        assert!(
            missing_webdriver_tools_diagnostic(true, false)
                .contains("缺少: Chrome/Chromium browser")
        );
    }

    #[test]
    fn matching_browser_and_driver_major_versions_are_required() -> Result<()> {
        ensure_matching_major_versions(
            "Chromium 151.0.7922.108 snap",
            "ChromeDriver 151.0.7922.108 (revision)",
        )?;
        let error = ensure_matching_major_versions("Google Chrome 150.0.1", "ChromeDriver 151.0.2")
            .expect_err("mismatched browser and driver must fail before integration");

        assert!(error.to_string().contains("browser=150, driver=151"));
        Ok(())
    }

    #[test]
    fn address_conflict_is_the_only_retryable_startup_failure() {
        assert!(address_conflict("bind() failed: Address already in use"));
        assert!(address_conflict(
            "Only one usage of each socket address is normally permitted"
        ));
        assert!(!address_conflict("ChromeDriver executable is incompatible"));
    }

    #[test]
    fn snap_launcher_is_mapped_to_its_application_name() {
        assert_eq!(
            snap_application_name(Path::new("/snap/bin/chromium")),
            Some(std::ffi::OsStr::new("chromium"))
        );
        assert_eq!(snap_application_name(Path::new("/usr/bin/chromium")), None);
    }

    #[cfg(unix)]
    #[test]
    fn largest_executable_browser_payload_is_selected() -> Result<()> {
        let package = tempfile::tempdir()?;
        let shallow = package.path().join("chrome");
        let payload_dir = package.path().join("usr/lib/browser");
        fs::create_dir_all(&payload_dir)?;
        let payload = payload_dir.join("chrome");
        fs::write(&shallow, b"wrapper")?;
        fs::write(&payload, vec![0_u8; 128])?;
        for path in [&shallow, &payload] {
            let mut permissions = fs::metadata(path)?.permissions();
            permissions.set_mode(0o755);
            fs::set_permissions(path, permissions)?;
        }

        assert_eq!(find_browser_payload(package.path())?, Some(payload));
        Ok(())
    }
}
