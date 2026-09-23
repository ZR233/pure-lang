//! 冷热 keyset 分页的共用合并核心。
//!
//! "内存热集合覆盖 SQLite 冷页"是会话目录与 Turn 历史共用的分页语义：
//! 相同标识热条目胜出（可用 `refine` 让冷事实补充热条目）、cursor 键排重、
//! 条目按调用方约定的顺序保留。本模块只做纯合并，不执行查询。

use std::collections::HashMap;

/// 参与冷热合并分页的条目。
pub(crate) trait HotColdEntry {
    type Key: Ord;
    /// keyset 排序键（分页方向为降序）。
    fn page_key(&self) -> Self::Key;
    /// 跨冷热去重的唯一标识。
    fn entry_id(&self) -> &str;
}

/// 把一页冷条目叠加进已积累的合并序列。
///
/// `merged` 通常以热条目为初始输入（已完成 cursor 过滤），随后逐页追加冷条目：
/// - 已存在同 id 条目时保留既有条目，并调用 `refine(cold, hot)` 让冷事实补充
///   热条目（例如 rolled-back disposition）；
/// - key 不小于 cursor 的冷条目属于更早的页面，直接排除——这覆盖了"热 cursor
///   尚未落库、冷查询从最新端开始"的场景；
/// - 其余冷条目按到达顺序追加。
///
/// 本函数不排序；调用方负责输入顺序（或最后统一排序）。
pub(crate) fn overlay_cold_page<E, F>(
    mut merged: Vec<E>,
    cold: Vec<E>,
    cursor: Option<&E::Key>,
    mut refine: F,
) -> Vec<E>
where
    E: HotColdEntry,
    F: FnMut(&E, &mut E),
{
    if let Some(cursor) = cursor {
        merged.retain(|entry| entry.page_key() < *cursor);
    }
    let mut positions = merged
        .iter()
        .enumerate()
        .map(|(index, entry)| (entry.entry_id().to_string(), index))
        .collect::<HashMap<String, usize>>();
    for entry in cold {
        if let Some(position) = positions.get(entry.entry_id()).copied() {
            refine(&entry, &mut merged[position]);
            continue;
        }
        if cursor.is_some_and(|cursor| entry.page_key() >= *cursor) {
            continue;
        }
        positions.insert(entry.entry_id().to_string(), merged.len());
        merged.push(entry);
    }
    merged
}

/// 一次性合并热集合与冷页并按 key 降序归位。
///
/// 适用于热条目可散落在任意 key 位置的场景（目录分页）；Turn 历史等
/// "热窗口是最新后缀"的场景直接用 [`overlay_cold_page`] 保留追加顺序。
pub(crate) fn merge_page_desc<E>(hot: Vec<E>, cold: Vec<E>, cursor: Option<&E::Key>) -> Vec<E>
where
    E: HotColdEntry,
{
    let mut merged = overlay_cold_page(hot, cold, cursor, |_, _| {});
    merged.sort_by_key(|entry| std::cmp::Reverse(entry.page_key()));
    merged
}
