use std::path::{Path, PathBuf};

fn rust_sources(root: &Path) -> Vec<PathBuf> {
    let mut pending = vec![root.to_path_buf()];
    let mut sources = Vec::new();
    while let Some(directory) = pending.pop() {
        for entry in std::fs::read_dir(directory).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                pending.push(path);
            } else if path.extension().is_some_and(|extension| extension == "rs") {
                sources.push(path);
            }
        }
    }
    sources
}

#[test]
fn model_crate_keeps_one_runtime_path_and_four_top_level_modules() {
    let source_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut top_level = std::fs::read_dir(&source_root)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    top_level.sort();
    assert_eq!(
        top_level,
        ["completion", "lib.rs", "model", "provider", "runtime"]
    );

    for forbidden in [
        "manager.rs",
        "protocol",
        "provider_info.rs",
        "transport_session.rs",
    ] {
        assert!(
            !source_root.join(forbidden).exists(),
            "obsolete model layer reappeared: {forbidden}"
        );
    }

    let production = rust_sources(&source_root)
        .into_iter()
        .map(|path| std::fs::read_to_string(path).unwrap())
        .collect::<Vec<_>>()
        .join("\n");
    for forbidden in [
        "trait ModelProvider",
        "SharedModelProvider",
        "OpenAiProvider",
        "ModelsManager",
        "create_provider",
        "decode_provider_stream",
        "#[path =",
    ] {
        assert!(
            !production.contains(forbidden),
            "obsolete model architecture symbol reappeared: {forbidden}"
        );
    }
}
