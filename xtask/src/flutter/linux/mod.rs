use crate::process;
use anyhow::{Context, Result, bail};
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const INSTALL_HINT: &str = "请安装完整且彼此匹配的 Flutter Linux 原生构建依赖。\
\nDebian/Ubuntu 示例: sudo apt-get install -y clang cmake ninja-build pkg-config libgtk-3-dev build-essential\
\n其他发行版请安装等价的 Clang、CMake、Ninja、pkg-config、GTK 3 开发包和 C++ 标准库开发包；不要在仓库中写死编译器版本或系统库路径。";

const PROBE_CMAKE: &str = r#"cmake_minimum_required(VERSION 3.13)
project(pure_studio_linux_preflight LANGUAGES CXX)
find_package(PkgConfig REQUIRED)
pkg_check_modules(GTK REQUIRED IMPORTED_TARGET gtk+-3.0)
add_executable(pure_studio_linux_preflight main.cc)
target_compile_features(pure_studio_linux_preflight PRIVATE cxx_std_17)
target_link_libraries(pure_studio_linux_preflight PRIVATE PkgConfig::GTK)
"#;

const PROBE_SOURCE: &str = r#"#include <gtk/gtk.h>
#include <type_traits>

int main() {
  static_assert(std::is_integral_v<int>);
  return gtk_get_major_version() > 0 ? 0 : 1;
}
"#;

#[derive(Debug)]
struct NativeTools {
    cmake: PathBuf,
    ninja: PathBuf,
    pkg_config: PathBuf,
    clang: PathBuf,
    clangxx: PathBuf,
}

impl NativeTools {
    fn discover() -> Result<Self> {
        let required = ["cmake", "ninja", "pkg-config", "clang", "clang++"];
        let missing = required
            .iter()
            .copied()
            .filter(|program| find_program(program).is_none())
            .collect::<Vec<_>>();
        if !missing.is_empty() {
            bail!("{}", missing_programs_diagnostic(&missing));
        }

        Ok(Self {
            cmake: required_program("cmake")?,
            ninja: required_program("ninja")?,
            pkg_config: required_program("pkg-config")?,
            clang: required_program("clang")?,
            clangxx: required_program("clang++")?,
        })
    }

    fn print(&self) {
        println!("Linux native GUI preflight tools:");
        println!("  CMake: {}", self.cmake.display());
        println!("  Ninja: {}", self.ninja.display());
        println!("  pkg-config: {}", self.pkg_config.display());
        println!("  Clang: {}", self.clang.display());
        println!("  Clang++: {}", self.clangxx.display());
    }
}

pub(super) fn ensure_native_build_environment() -> Result<()> {
    let tools = NativeTools::discover()?;
    tools.print();
    probe_version(&tools.cmake, "--version", "CMake 可执行性")?;
    probe_version(&tools.ninja, "--version", "Ninja 可执行性")?;
    probe_version(&tools.pkg_config, "--version", "pkg-config 可执行性")?;
    probe_version(&tools.clang, "--version", "Clang 可执行性")?;
    probe_version(&tools.clangxx, "--version", "Clang++ 可执行性")?;
    probe_gtk(&tools.pkg_config)?;
    probe_cmake_toolchain(&tools)?;
    println!("Linux native GUI preflight passed.");
    Ok(())
}

fn probe_version(program: &Path, argument: &str, stage: &str) -> Result<()> {
    let mut command = Command::new(program);
    command.arg(argument);
    run_probe(&mut command, stage)
}

fn probe_gtk(pkg_config: &Path) -> Result<()> {
    let mut command = Command::new(pkg_config);
    command.args(["--print-errors", "--exists", "gtk+-3.0"]);
    run_probe(&mut command, "GTK 3 开发包探测")
}

fn probe_cmake_toolchain(tools: &NativeTools) -> Result<()> {
    let temp = tempfile::Builder::new()
        .prefix("pure-studio-linux-preflight-")
        .tempdir()
        .context("failed to create isolated Linux GUI preflight directory")?;
    let source_dir = temp.path().join("source");
    let build_dir = temp.path().join("build");
    fs::create_dir(&source_dir)
        .with_context(|| format!("failed to create {}", source_dir.display()))?;
    fs::write(source_dir.join("CMakeLists.txt"), PROBE_CMAKE)
        .context("failed to write isolated CMake probe")?;
    fs::write(source_dir.join("main.cc"), PROBE_SOURCE)
        .context("failed to write isolated C++ probe")?;

    let mut configure = Command::new(&tools.cmake);
    configure
        .arg("-S")
        .arg(&source_dir)
        .arg("-B")
        .arg(&build_dir)
        .args(["-G", "Ninja"])
        .arg(format!("-DCMAKE_MAKE_PROGRAM={}", tools.ninja.display()))
        .arg(format!(
            "-DPKG_CONFIG_EXECUTABLE={}",
            tools.pkg_config.display()
        ))
        .env("CC", &tools.clang)
        .env("CXX", &tools.clangxx);
    run_probe(&mut configure, "CMake/Clang 配置")?;

    let mut build = Command::new(&tools.cmake);
    build.arg("--build").arg(&build_dir).arg("--verbose");
    run_probe(&mut build, "C++ 标准库与 GTK 编译链接")
}

fn run_probe(command: &mut Command, stage: &str) -> Result<()> {
    let display = command_display(command);
    let output = command.output().map_err(|error| {
        anyhow::anyhow!(probe_failure_diagnostic(
            stage,
            &display,
            &format!("failed to start command: {error}"),
        ))
    })?;
    if output.status.success() {
        return Ok(());
    }

    let raw_output = raw_output(&output.stdout, &output.stderr);
    bail!("{}", probe_failure_diagnostic(stage, &display, &raw_output));
}

fn command_display(command: &Command) -> String {
    let program = command.get_program().to_string_lossy();
    let args = command.get_args().map(OsString::from).collect::<Vec<_>>();
    process::display_command(&program, &args)
}

fn raw_output(stdout: &[u8], stderr: &[u8]) -> String {
    let stdout = String::from_utf8_lossy(stdout);
    let stderr = String::from_utf8_lossy(stderr);
    match (stdout.trim().is_empty(), stderr.trim().is_empty()) {
        (true, true) => "(命令没有输出)".to_owned(),
        (false, true) => stdout.into_owned(),
        (true, false) => stderr.into_owned(),
        (false, false) => format!("{stdout}\n{stderr}"),
    }
}

fn missing_programs_diagnostic(missing: &[&str]) -> String {
    format!(
        "Linux 原生 GUI 构建环境预检失败。\n阶段: 必需命令发现\nPATH 中缺少: {}\n修复建议:\n{INSTALL_HINT}",
        missing.join(", ")
    )
}

fn probe_failure_diagnostic(stage: &str, command: &str, raw_output: &str) -> String {
    format!(
        "Linux 原生 GUI 构建环境预检失败。\n阶段: {stage}\n命令: {command}\n原始输出:\n{raw_output}\n修复建议:\n{INSTALL_HINT}"
    )
}

fn required_program(program: &str) -> Result<PathBuf> {
    find_program(program).with_context(|| format!("{program} disappeared from PATH during probe"))
}

fn find_program(program: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|directory| directory.join(program))
        .find(|candidate| is_executable(candidate))
}

fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;

    path.metadata()
        .is_ok_and(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsStr;

    #[test]
    fn missing_programs_are_reported_together_without_machine_paths() {
        let diagnostic = missing_programs_diagnostic(&["cmake", "clang++"]);

        assert!(diagnostic.contains("PATH 中缺少: cmake, clang++"));
        assert!(diagnostic.contains("sudo apt-get install -y clang cmake ninja-build"));
        assert!(!diagnostic.contains("/usr/lib/gcc"));
        assert!(!diagnostic.contains("CPLUS_INCLUDE_PATH"));
    }

    #[test]
    fn compiler_failure_preserves_raw_output_and_actionable_context() {
        let raw = "clang++: error: unable to find library -lstdc++\n";
        let diagnostic = probe_failure_diagnostic(
            "C++ 标准库与 GTK 编译链接",
            "/usr/bin/cmake --build /tmp/probe --verbose",
            raw,
        );

        assert!(diagnostic.contains("阶段: C++ 标准库与 GTK 编译链接"));
        assert!(diagnostic.contains("命令: /usr/bin/cmake --build /tmp/probe --verbose"));
        assert!(diagnostic.contains(raw));
        assert!(diagnostic.contains("C++ 标准库开发包"));
    }

    #[test]
    fn mixed_stdout_and_stderr_are_kept_in_order() {
        assert_eq!(
            raw_output(b"configure output\n", b"compiler detail\n"),
            "configure output\n\ncompiler detail\n"
        );
    }

    #[test]
    fn command_display_quotes_paths_and_arguments() {
        let mut command = Command::new("/tmp/tool path/cmake");
        command.args([OsStr::new("--build"), OsStr::new("/tmp/build dir")]);

        assert_eq!(
            command_display(&command),
            "\"/tmp/tool path/cmake\" --build \"/tmp/build dir\""
        );
    }
}
