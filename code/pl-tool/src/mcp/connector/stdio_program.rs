use std::ffi::OsString;
use std::io;
#[cfg(windows)]
use std::path::Path;
use std::path::PathBuf;

#[cfg(windows)]
use serde::Deserialize;

#[cfg(windows)]
const NPM_CLI_WINDOWS_HIDE_CHILDREN: &str = r#"
const childProcess = require('node:child_process');
for (const method of ['spawn', 'spawnSync']) {
  const original = childProcess[method];
  childProcess[method] = function(command, args, options) {
    if (!Array.isArray(args)) {
      options = args;
      args = [];
    }
    return original.call(
      this,
      command,
      args,
      Object.assign({}, options, { windowsHide: true }),
    );
  };
}
require(process.argv[1]);
"#;

/// CreateProcess target plus arguments required before the configured MCP arguments.
pub(super) struct ResolvedStdioProgram {
    pub executable: PathBuf,
    pub prefix_args: Vec<OsString>,
}

impl ResolvedStdioProgram {
    fn direct(executable: PathBuf) -> Self {
        Self {
            executable,
            prefix_args: Vec::new(),
        }
    }
}

/// Resolve a model-visible stdio command to a target accepted by CreateProcess.
///
/// Config stays platform-neutral (`npx`, not `npx.cmd`). On Windows, bare names
/// are resolved through PATH/PATHEXT while extensionless shell scripts such as
/// npm's POSIX `npx` shim are skipped. Standard npm launchers are unwrapped to
/// `node.exe + npx-cli.js`; the npm CLI is preloaded with `windowsHide` for its
/// package launcher, so neither the long-lived MCP process nor its npm shim owns
/// a visible console.
#[cfg(windows)]
pub(super) fn resolve(command: &str) -> io::Result<ResolvedStdioProgram> {
    resolve_windows(command)
}

#[cfg(not(windows))]
pub(super) fn resolve(command: &str) -> io::Result<ResolvedStdioProgram> {
    Ok(ResolvedStdioProgram::direct(PathBuf::from(command)))
}

#[cfg(windows)]
fn resolve_windows(command: &str) -> io::Result<ResolvedStdioProgram> {
    let path = Path::new(command);
    let executable = if has_explicit_path(path) || path.extension().is_some() {
        path.to_path_buf()
    } else {
        which::which_all(command)
            .map_err(io::Error::other)?
            .find(|candidate| is_create_process_target(candidate))
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::NotFound,
                    format!("stdio command was not found in PATH/PATHEXT: {command}"),
                )
            })?
    };

    Ok(resolve_standard_npm_cli(&executable)
        .unwrap_or_else(|| ResolvedStdioProgram::direct(executable)))
}

#[cfg(windows)]
fn resolve_standard_npm_cli(shim: &Path) -> Option<ResolvedStdioProgram> {
    let cli_name = shim
        .file_stem()
        .and_then(|name| name.to_str())
        .filter(|name| name.eq_ignore_ascii_case("npm") || name.eq_ignore_ascii_case("npx"))?;
    let extension = shim.extension().and_then(|extension| extension.to_str())?;
    if !extension.eq_ignore_ascii_case("cmd") && !extension.eq_ignore_ascii_case("bat") {
        return None;
    }

    let shim_directory = shim.parent()?;
    let adjacent_node = shim_directory.join("node.exe");
    let node = adjacent_node
        .is_file()
        .then_some(adjacent_node)
        .or_else(|| which::which("node.exe").ok())?;
    let node_directory = node.parent()?;

    let manifest_paths = [
        shim_directory.join("node_modules/npm/package.json"),
        node_directory.join("node_modules/npm/package.json"),
    ];
    let (manifest_path, manifest) = manifest_paths.iter().find_map(|path| {
        let content = std::fs::read_to_string(path).ok()?;
        serde_json::from_str::<NpmPackageManifest>(&content)
            .ok()
            .map(|manifest| (path, manifest))
    })?;
    let relative_cli = manifest.bin.resolve(cli_name)?;
    let cli = manifest_path.parent()?.join(relative_cli);
    if !cli.is_file() {
        return None;
    }

    Some(ResolvedStdioProgram {
        executable: node,
        prefix_args: vec![
            OsString::from("--eval"),
            OsString::from(NPM_CLI_WINDOWS_HIDE_CHILDREN),
            cli.into_os_string(),
        ],
    })
}

#[cfg(windows)]
#[derive(Deserialize)]
struct NpmPackageManifest {
    bin: NpmBinEntries,
}

#[cfg(windows)]
#[derive(Deserialize)]
#[serde(untagged)]
enum NpmBinEntries {
    Single(PathBuf),
    Named(std::collections::HashMap<String, PathBuf>),
}

#[cfg(windows)]
impl NpmBinEntries {
    fn resolve(&self, cli_name: &str) -> Option<&Path> {
        match self {
            Self::Single(path) => Some(path),
            Self::Named(entries) => entries.iter().find_map(|(name, path)| {
                name.eq_ignore_ascii_case(cli_name)
                    .then_some(path.as_path())
            }),
        }
    }
}

#[cfg(windows)]
fn has_explicit_path(path: &Path) -> bool {
    path.is_absolute()
        || path
            .parent()
            .is_some_and(|parent| !parent.as_os_str().is_empty())
}

#[cfg(windows)]
fn is_create_process_target(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            ["com", "exe", "bat", "cmd"]
                .iter()
                .any(|supported| extension.eq_ignore_ascii_case(supported))
        })
}
