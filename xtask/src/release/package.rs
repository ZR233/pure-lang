use super::asset_name;
use crate::cli::BuildGuiOptions;
use crate::{flutter, paths, process};
use anyhow::{Context, Result, bail};
use semver::Version;
use std::env;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

pub(super) fn stage(workspace_root: &Path, release_dir: &Path, version: &Version) -> Result<()> {
    ensure_windows()?;
    reject_existing_release(release_dir)?;
    fs::create_dir_all(release_dir)
        .with_context(|| format!("failed to create {}", release_dir.display()))?;

    flutter::build_gui_release(
        BuildGuiOptions {
            demo: false,
            no_clean: false,
            check_generated: true,
        },
        &version.to_string(),
    )?;

    let bundle_dir = release_dir.join("bundle");
    copy_filtered_bundle(&paths::release_dist_dir(workspace_root), &bundle_dir)?;
    fs::copy(workspace_root.join("LICENSE"), bundle_dir.join("LICENSE"))
        .context("failed to add LICENSE to release bundle")?;
    fs::copy(
        workspace_root.join("THIRD_PARTY_NOTICES.md"),
        bundle_dir.join("THIRD_PARTY_NOTICES.md"),
    )
    .context("failed to add third-party notices to release bundle")?;

    sign_bundle_if_configured(&bundle_dir)?;
    create_portable_zip(release_dir, &bundle_dir, version)?;
    build_installer(workspace_root, release_dir, &bundle_dir, version)?;
    sign_installer_if_configured(release_dir, version)?;
    fs::remove_dir_all(&bundle_dir)
        .with_context(|| format!("failed to remove staging bundle {}", bundle_dir.display()))?;
    println!("Staged stable release: {}", release_dir.display());
    Ok(())
}

fn ensure_windows() -> Result<()> {
    if !cfg!(target_os = "windows") {
        bail!("release-gui stage currently supports Windows x64 only");
    }
    Ok(())
}

fn reject_existing_release(release_dir: &Path) -> Result<()> {
    if release_dir.exists() {
        bail!(
            "release staging directory already exists; refusing to overwrite {}",
            release_dir.display()
        );
    }
    Ok(())
}

fn copy_filtered_bundle(source: &Path, destination: &Path) -> Result<()> {
    if !source.is_dir() {
        bail!("GUI release output not found: {}", source.display());
    }
    fs::create_dir_all(destination)
        .with_context(|| format!("failed to create {}", destination.display()))?;
    copy_filtered_dir(source, destination)
}

fn copy_filtered_dir(source: &Path, destination: &Path) -> Result<()> {
    for entry in
        fs::read_dir(source).with_context(|| format!("failed to read {}", source.display()))?
    {
        let entry =
            entry.with_context(|| format!("failed to read entry in {}", source.display()))?;
        let source_path = entry.path();
        if is_excluded(&source_path) {
            continue;
        }
        let destination_path = destination.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            fs::create_dir_all(&destination_path)?;
            copy_filtered_dir(&source_path, &destination_path)?;
        } else {
            fs::copy(&source_path, &destination_path).with_context(|| {
                format!(
                    "failed to copy {} to {}",
                    source_path.display(),
                    destination_path.display()
                )
            })?;
        }
    }
    Ok(())
}

fn is_excluded(path: &Path) -> bool {
    path.extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("pdb"))
}

fn create_portable_zip(release_dir: &Path, bundle_dir: &Path, version: &Version) -> Result<()> {
    let output = release_dir.join(asset_name(version, "portable.zip"));
    let args = [
        OsString::from("-a"),
        OsString::from("-c"),
        OsString::from("-f"),
        output.as_os_str().to_owned(),
        OsString::from("-C"),
        bundle_dir.as_os_str().to_owned(),
        OsString::from("."),
    ];
    let display = process::display_command("tar", &args);
    let mut command = Command::new("tar");
    command.args(args);
    process::run_checked(&mut command, &display)
}

fn build_installer(
    workspace_root: &Path,
    release_dir: &Path,
    bundle_dir: &Path,
    version: &Version,
) -> Result<()> {
    let iscc = find_iscc()?;
    let script = paths::studio_app_dir(workspace_root)
        .join("windows")
        .join("installer")
        .join("pure_studio.iss");
    let output_base = asset_name(version, "setup");
    let args = [
        OsString::from(format!("/DMyAppVersion={version}")),
        OsString::from(format!("/DSourceDir={}", bundle_dir.display())),
        OsString::from(format!("/DOutputDir={}", release_dir.display())),
        OsString::from(format!("/DOutputBase={output_base}")),
        script.as_os_str().to_owned(),
    ];
    let display = process::display_command(&iscc.to_string_lossy(), &args);
    let mut command = Command::new(&iscc);
    command.args(args);
    process::run_checked(&mut command, &display)
}

fn find_iscc() -> Result<PathBuf> {
    if let Some(path) = env::var_os("ISCC_PATH").map(PathBuf::from) {
        if path.is_file() {
            return Ok(path);
        }
        bail!("ISCC_PATH does not point to a file: {}", path.display());
    }
    for path in [
        PathBuf::from(r"C:\Program Files (x86)\Inno Setup 6\ISCC.exe"),
        PathBuf::from(r"C:\Program Files\Inno Setup 6\ISCC.exe"),
    ] {
        if path.is_file() {
            return Ok(path);
        }
    }
    bail!("Inno Setup 6 compiler not found; install it or set ISCC_PATH")
}

fn sign_bundle_if_configured(bundle_dir: &Path) -> Result<()> {
    let Some(config) = authenticode_config()? else {
        println!("Authenticode certificate not configured; continuing with Minisign protection.");
        return Ok(());
    };
    let mut signable = vec![bundle_dir.join("pure_studio.exe")];
    signable.extend(
        fs::read_dir(bundle_dir)?
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| {
                path.extension().is_some_and(|extension| {
                    extension.eq_ignore_ascii_case("dll")
                        && path
                            .file_name()
                            .is_some_and(|name| name.to_string_lossy().starts_with("pl_"))
                })
            }),
    );
    for path in signable {
        sign_authenticode(&config, &path)?;
    }
    Ok(())
}

fn sign_installer_if_configured(release_dir: &Path, version: &Version) -> Result<()> {
    let Some(config) = authenticode_config()? else {
        return Ok(());
    };
    sign_authenticode(&config, &release_dir.join(asset_name(version, "setup.exe")))
}

struct AuthenticodeConfig {
    signtool: PathBuf,
    certificate: PathBuf,
    password: String,
}

fn authenticode_config() -> Result<Option<AuthenticodeConfig>> {
    let Some(certificate) = env::var_os("WINDOWS_CERTIFICATE_PATH").map(PathBuf::from) else {
        return Ok(None);
    };
    if !certificate.is_file() {
        bail!(
            "WINDOWS_CERTIFICATE_PATH does not point to a file: {}",
            certificate.display()
        );
    }
    let signtool = env::var_os("WINDOWS_SIGNTOOL_PATH")
        .map(PathBuf::from)
        .context("WINDOWS_SIGNTOOL_PATH is required with WINDOWS_CERTIFICATE_PATH")?;
    let password = env::var("WINDOWS_CERTIFICATE_PASSWORD")
        .context("WINDOWS_CERTIFICATE_PASSWORD is required with WINDOWS_CERTIFICATE_PATH")?;
    Ok(Some(AuthenticodeConfig {
        signtool,
        certificate,
        password,
    }))
}

fn sign_authenticode(config: &AuthenticodeConfig, path: &Path) -> Result<()> {
    if !path.is_file() {
        bail!("Authenticode input not found: {}", path.display());
    }
    let mut command = Command::new(&config.signtool);
    command.args([
        OsString::from("sign"),
        OsString::from("/fd"),
        OsString::from("SHA256"),
        OsString::from("/td"),
        OsString::from("SHA256"),
        OsString::from("/tr"),
        OsString::from("http://timestamp.digicert.com"),
        OsString::from("/f"),
        config.certificate.as_os_str().to_owned(),
        OsString::from("/p"),
        OsString::from(&config.password),
        path.as_os_str().to_owned(),
    ]);
    process::run_checked(&mut command, &format!("signtool sign {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn excludes_pdb_files_case_insensitively() {
        assert!(is_excluded(Path::new("app.pdb")));
        assert!(is_excluded(Path::new("APP.PDB")));
        assert!(!is_excluded(Path::new("app.dll")));
    }

    #[test]
    fn rejects_existing_release_directory() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "pure-studio-release-test-{}-{unique}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path).unwrap();
        assert!(reject_existing_release(&path).is_err());
        std::fs::remove_dir_all(path).unwrap();
    }
}
