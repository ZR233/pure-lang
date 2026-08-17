use std::path::{Path, PathBuf};

use url::Url;

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

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    #[test]
    fn windows_drive_file_uri_round_trips() {
        let path = PathBuf::from(r"C:\work\pure lang\src\lib.rs");
        let uri = if cfg!(windows) {
            path_to_file_uri(&path)
        } else {
            "file:///C:/work/pure%20lang/src/lib.rs".to_string()
        };

        assert_eq!(
            file_uri_to_path(&uri),
            PathBuf::from("C:/work/pure lang/src/lib.rs")
        );
    }

    #[cfg(not(windows))]
    #[test]
    fn unix_path_with_colon_stays_absolute() {
        assert_eq!(
            file_uri_to_path("file:///tmp/a:/lib.rs"),
            PathBuf::from("/tmp/a:/lib.rs")
        );
    }

    #[test]
    fn encodes_spaces_in_file_uri() {
        let uri = path_to_file_uri(&std::env::temp_dir().join("pure lang/lib.rs"));

        assert!(uri.contains("pure%20lang"));
    }

    #[test]
    fn strips_windows_verbatim_drive_prefix_before_file_uri_encoding() {
        let normalized = normalize_windows_verbatim_path(r"\\?\C:\work\pure lang\src\lib.rs");

        assert_eq!(normalized, r"C:\work\pure lang\src\lib.rs");
        if cfg!(windows) {
            let uri = path_to_file_uri(Path::new(r"\\?\C:\work\pure lang\src\lib.rs"));
            assert_eq!(uri, "file:///C:/work/pure%20lang/src/lib.rs");
        }
    }

    #[test]
    fn strips_windows_verbatim_unc_prefix_before_file_uri_encoding() {
        let normalized =
            normalize_windows_verbatim_path(r"\\?\UNC\server\share\pure lang\src\lib.rs");

        assert_eq!(normalized, r"\\server\share\pure lang\src\lib.rs");
        if cfg!(windows) {
            let uri = path_to_file_uri(Path::new(r"\\?\UNC\server\share\pure lang\src\lib.rs"));
            assert_eq!(uri, "file://server/share/pure%20lang/src/lib.rs");
        }
    }
}
