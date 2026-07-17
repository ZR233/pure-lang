use anyhow::{Context, Result, bail};

pub(super) struct PorcelainEntry {
    pub(super) status: String,
    pub(super) path: String,
    pub(super) original_path: Option<String>,
}

pub(super) fn parse_porcelain_entries(bytes: &[u8]) -> Result<Vec<PorcelainEntry>> {
    let fields = bytes
        .split(|byte| *byte == 0)
        .filter(|field| !field.is_empty())
        .collect::<Vec<_>>();
    let mut entries = Vec::new();
    let mut index = 0;
    while index < fields.len() {
        let field = fields[index];
        index += 1;
        if field.len() < 4 || field[2] != b' ' {
            bail!("invalid Git conflict status entry");
        }
        let status = std::str::from_utf8(&field[..2])?.to_string();
        let path = std::str::from_utf8(&field[3..])?.replace('\\', "/");
        let renamed = matches!(field[0], b'R' | b'C') || matches!(field[1], b'R' | b'C');
        let original_path = if renamed {
            let original = fields
                .get(index)
                .context("rename status entry has no original path")?;
            index += 1;
            Some(std::str::from_utf8(original)?.replace('\\', "/"))
        } else {
            None
        };
        entries.push(PorcelainEntry {
            status,
            path,
            original_path,
        });
    }
    Ok(entries)
}
