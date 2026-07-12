use std::collections::BTreeMap;

use anyhow::{Context, Result, bail};

use crate::studio::task_coordinator::MergeIndexStage;

pub(super) fn parse_unmerged_entries(
    bytes: &[u8],
) -> Result<BTreeMap<String, Vec<MergeIndexStage>>> {
    let mut grouped = BTreeMap::<String, Vec<MergeIndexStage>>::new();
    for record in bytes
        .split(|byte| *byte == 0)
        .filter(|record| !record.is_empty())
    {
        let tab = record
            .iter()
            .position(|byte| *byte == b'\t')
            .context("unmerged index record has no path separator")?;
        let metadata =
            std::str::from_utf8(&record[..tab]).context("unmerged index metadata is not UTF-8")?;
        let path = std::str::from_utf8(&record[tab + 1..])
            .context("unmerged index path is not UTF-8")?
            .replace('\\', "/");
        let mut fields = metadata.split_whitespace();
        let mode = fields.next().context("unmerged index mode is missing")?;
        let object_id = fields
            .next()
            .context("unmerged index object id is missing")?;
        let stage = fields
            .next()
            .context("unmerged index stage is missing")?
            .parse::<u8>()
            .context("unmerged index stage is invalid")?;
        if !matches!(stage, 1..=3) {
            bail!("invalid unmerged index stage {stage}");
        }
        grouped.entry(path).or_default().push(MergeIndexStage {
            stage,
            mode: mode.to_string(),
            object_id: object_id.to_string(),
        });
    }
    Ok(grouped)
}
