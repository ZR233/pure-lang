use sha2::{Digest, Sha256};
use std::{
    env, fs,
    path::{Path, PathBuf},
};

fn files(root: &Path, directory: &Path, out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(directory).expect("read bundled Skills") {
        let entry = entry.expect("read bundled Skill entry");
        if entry.file_type().expect("read Skill type").is_dir() {
            files(root, &entry.path(), out);
        } else {
            out.push(
                entry
                    .path()
                    .strip_prefix(root)
                    .expect("relative Skill path")
                    .into(),
            );
        }
    }
}
fn main() {
    let root = Path::new("assets/skills");
    println!("cargo:rerun-if-changed=assets/skills");
    let mut paths = Vec::new();
    files(root, root, &mut paths);
    paths.sort();
    let mut digest = Sha256::new();
    for path in paths {
        let name = path.to_str().expect("UTF-8 Skill path").replace('\\', "/");
        let contents = fs::read(root.join(path)).expect("read bundled Skill");
        digest.update((name.len() as u64).to_le_bytes());
        digest.update(name);
        digest.update((contents.len() as u64).to_le_bytes());
        digest.update(contents);
    }
    let output = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR"));
    fs::write(
        output.join("system_skills_fingerprint"),
        hex::encode(digest.finalize()),
    )
    .expect("write Skill fingerprint");
}
