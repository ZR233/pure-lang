use std::path::{Path, PathBuf};
use std::str::FromStr;

use lsp_types::Uri;
use url::Url;

use crate::runtime::{LspResult, LspRuntimeError};

pub(crate) fn file_uri(path: &Path) -> LspResult<Uri> {
    parse_uri(&path_to_file_uri(path))
}

pub(crate) fn parse_uri(uri: &str) -> LspResult<Uri> {
    Uri::from_str(uri)
        .map_err(|error| LspRuntimeError::InvalidQuery(format!("invalid file URI: {error}")))
}

pub(crate) fn path_to_file_uri(path: &Path) -> String {
    debug_assert!(
        path.is_absolute(),
        "LSP file URI paths must already be absolute: {}",
        path.display()
    );
    let normalized = if cfg!(windows) {
        PathBuf::from(normalize_windows_verbatim_path(&path.to_string_lossy()))
    } else {
        path.to_path_buf()
    };
    Url::from_file_path(&normalized)
        .expect("absolute LSP paths must convert to file URLs")
        .into()
}

pub(crate) fn file_uri_to_path(uri: &str) -> PathBuf {
    Url::parse(uri)
        .ok()
        .filter(|url| url.scheme() == "file")
        .and_then(|url| {
            let path = url.to_file_path().ok()?;
            let uri_path = url.path().as_bytes();
            let is_windows_drive = uri_path.first() == Some(&b'/')
                && uri_path.get(1).is_some_and(u8::is_ascii_alphabetic)
                && uri_path.get(2) == Some(&b':');
            if is_windows_drive {
                Some(
                    path.strip_prefix(Path::new("/"))
                        .unwrap_or(&path)
                        .to_path_buf(),
                )
            } else {
                Some(path)
            }
        })
        .unwrap_or_else(|| PathBuf::from(uri))
}

pub(crate) fn uri_display_path(uri: &str, workspace_root: Option<&Path>) -> String {
    let path = file_uri_to_path(uri);
    if let Some(root) = workspace_root
        && let Ok(relative) = path.strip_prefix(root)
    {
        return normalize_separators(relative);
    }
    normalize_separators(&path)
}

pub(crate) fn normalize_separators(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn normalize_windows_verbatim_path(path: &str) -> String {
    if let Some(path) = path.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{path}")
    } else if let Some(path) = path.strip_prefix(r"\\?\") {
        path.to_string()
    } else {
        path.to_string()
    }
}
