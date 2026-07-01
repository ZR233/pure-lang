use std::path::{Path, PathBuf};

pub(crate) fn path_to_file_uri(path: &Path) -> String {
    debug_assert!(
        path.is_absolute(),
        "LSP file URI paths must already be absolute: {}",
        path.display()
    );
    let absolute = path.to_path_buf();
    if cfg!(windows) {
        let path = normalize_windows_verbatim_path(&absolute.to_string_lossy()).replace('\\', "/");
        if let Some(unc_path) = path.strip_prefix("//") {
            let encoded = encode_path(unc_path);
            format!("file://{encoded}")
        } else {
            let encoded = encode_path(&path);
            format!("file:///{encoded}")
        }
    } else {
        let path = absolute.to_string_lossy().replace('\\', "/");
        let encoded = encode_path(&path);
        format!("file://{encoded}")
    }
}

pub(crate) fn file_uri_to_path(uri: &str) -> PathBuf {
    let mut value = uri.strip_prefix("file://").unwrap_or(uri).to_string();
    if cfg!(windows) && value.starts_with('/') && value.as_bytes().get(2) == Some(&b':') {
        value.remove(0);
    } else if cfg!(windows) && !value.starts_with('/') && !value.is_empty() {
        value = format!("//{value}");
    }
    PathBuf::from(decode_path(&value))
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

fn encode_path(path: &str) -> String {
    let mut output = String::with_capacity(path.len());
    for byte in path.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'/' | b':' | b'-' | b'_' | b'.' | b'~' => {
                output.push(byte as char)
            }
            _ => output.push_str(&format!("%{byte:02X}")),
        }
    }
    output
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

fn decode_path(path: &str) -> String {
    let mut output = Vec::with_capacity(path.len());
    let bytes = path.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%'
            && index + 2 < bytes.len()
            && let Ok(hex) = std::str::from_utf8(&bytes[index + 1..index + 3])
            && let Ok(value) = u8::from_str_radix(hex, 16)
        {
            output.push(value);
            index += 3;
            continue;
        }
        output.push(bytes[index]);
        index += 1;
    }
    String::from_utf8(output).unwrap_or_else(|_| path.to_string())
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
