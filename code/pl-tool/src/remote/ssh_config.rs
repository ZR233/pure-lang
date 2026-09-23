//! `~/.ssh/config` 作为 SSH 服务器配置唯一事实源的解析与保全写入。
//!
//! 解析遵循 ssh 的首次匹配语义（每个关键字取块内首次出现值，HostName 缺省回退别名）；
//! 含通配、多模式或 Include 的条目不进入可选列表。写回只改动 anywork 管理块
//! （`# BEGIN/END anywork server: <alias>` 标记包裹的 Host 块），未触及内容逐字节保留；
//! 用户手写条目只读，别名被手写条目占用时拒绝写入。

use std::collections::{BTreeMap, HashSet};
use std::io::Write;
use std::path::{Path, PathBuf};

use super::client::RemoteClientError;
use super::manager::SshServerProfile;

const BEGIN_MARKER: &str = "# BEGIN anywork server:";
const END_MARKER: &str = "# END anywork server:";
const DEFAULT_ALIAS_BASE: &str = "server";

/// 解析出的一个可选 Host 条目；`managed` 表示位于 anywork 管理块内。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SshConfigEntry {
    pub profile: SshServerProfile,
    pub managed: bool,
}

/// 指向用户 ssh config 的读写句柄；只携带路径，可自由克隆。
#[derive(Debug, Clone)]
pub struct SshConfigFile {
    path: PathBuf,
    /// 显式指定的路径会让 ssh 子进程用 `-F` 读取该文件而不是默认位置。
    explicit: bool,
}

impl SshConfigFile {
    /// 用户默认配置文件（Unix `~/.ssh/config`，Windows `%USERPROFILE%\.ssh\config`）。
    pub fn user_default() -> Result<Self, RemoteClientError> {
        Ok(Self {
            path: user_ssh_config_path()?,
            explicit: false,
        })
    }

    /// 指向显式路径；测试与迁移使用，连接时经 `-F` 生效。
    pub fn at(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            explicit: true,
        }
    }

    /// 用户默认位置语义的路径句柄；home 探测失败时的回退使用。
    pub(crate) fn default_location(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            explicit: false,
        }
    }

    /// 是否为显式指定路径（非用户默认位置）。
    pub fn is_explicit(&self) -> bool {
        self.explicit
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// 解析全部可选 Host 条目；文件缺失视为没有配置。
    pub async fn read(&self) -> Result<Vec<SshConfigEntry>, RemoteClientError> {
        let bytes = self.read_bytes().await?;
        Ok(parse_entries(&bytes))
    }

    /// 幂等写入或替换一组管理块；别名被手写条目占用时失败。
    pub async fn upsert_managed(
        &self,
        profiles: &[SshServerProfile],
    ) -> Result<(), RemoteClientError> {
        for profile in profiles {
            validate_profile_shape(profile)?;
        }
        let bytes = self.read_bytes().await?;
        let updated = upsert_bytes(&bytes, profiles)?;
        self.write_atomic(updated).await
    }

    /// 删除一个管理块；别名不存在或属于手写条目时失败。
    pub async fn remove_managed(&self, alias: &str) -> Result<(), RemoteClientError> {
        let bytes = self.read_bytes().await?;
        let updated = remove_bytes(&bytes, alias)?;
        self.write_atomic(updated).await
    }

    async fn read_bytes(&self) -> Result<Vec<u8>, RemoteClientError> {
        let path = self.path.clone();
        match tokio::task::spawn_blocking(move || std::fs::read(path)).await {
            Ok(Ok(bytes)) => Ok(bytes),
            Ok(Err(error)) if error.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
            Ok(Err(error)) => Err(error.into()),
            Err(error) => Err(RemoteClientError::Protocol(error.to_string())),
        }
    }

    async fn write_atomic(&self, contents: Vec<u8>) -> Result<(), RemoteClientError> {
        let path = self.path.clone();
        let result = tokio::task::spawn_blocking(move || write_atomic(&path, &contents))
            .await
            .map_err(|error| RemoteClientError::Protocol(error.to_string()))?;
        result.map_err(RemoteClientError::from)
    }
}

/// Host 别名必须是可单 token 引用的名字：非空、无空白与控制字符、
/// 不含通配/否定/注释/引号字符，也不以 `-` 开头。
pub fn validate_alias(alias: &str) -> Result<(), RemoteClientError> {
    if alias.is_empty()
        || alias.starts_with('-')
        || alias.chars().any(|c| {
            c.is_whitespace()
                || c.is_control()
                || matches!(c, '*' | '?' | '[' | ']' | '!' | '#' | '"' | '\'' | '\\')
        })
    {
        return Err(RemoteClientError::Protocol(format!(
            "SSH alias '{alias}' is invalid"
        )));
    }
    Ok(())
}

fn validate_profile_shape(profile: &SshServerProfile) -> Result<(), RemoteClientError> {
    validate_alias(&profile.alias)?;
    if profile.host_name.trim().is_empty() || profile.username.trim().is_empty() {
        return Err(RemoteClientError::Protocol(format!(
            "SSH server {} is missing a host name or username",
            profile.alias
        )));
    }
    if profile.port == 0 {
        return Err(RemoteClientError::Protocol(
            "SSH server port must be greater than zero".to_string(),
        ));
    }
    Ok(())
}

/// 把任意展示名规范成可用作 Host 别名前缀的 token；连续分隔折叠为单个 `-`。
pub fn sanitize_alias_base(name: &str) -> String {
    let mut sanitized = String::with_capacity(name.len());
    let mut pending_dash = false;
    for c in name.trim().chars() {
        if c.is_whitespace()
            || c.is_control()
            || matches!(
                c,
                '*' | '?' | '[' | ']' | '!' | '#' | '"' | '\'' | '\\' | '-'
            )
        {
            if !sanitized.is_empty() {
                pending_dash = true;
            }
        } else {
            if pending_dash {
                sanitized.push('-');
                pending_dash = false;
            }
            sanitized.push(c);
        }
    }
    if sanitized.is_empty() {
        DEFAULT_ALIAS_BASE.to_string()
    } else {
        sanitized
    }
}

/// 在已占用别名集合之外分配唯一别名；冲突时追加 `-2`、`-3` …。
pub fn allocate_alias(base: &str, taken: &HashSet<String>) -> String {
    if validate_alias(base).is_ok() && !taken.contains(base) {
        return base.to_string();
    }
    for suffix in 2.. {
        let candidate = format!("{base}-{suffix}");
        if !taken.contains(&candidate) {
            return candidate;
        }
    }
    unreachable!("alias suffix space is infinite")
}

fn user_ssh_config_path() -> Result<PathBuf, RemoteClientError> {
    #[cfg(windows)]
    const HOME_VARS: &[&str] = &["USERPROFILE", "HOME"];
    #[cfg(not(windows))]
    const HOME_VARS: &[&str] = &["HOME", "USERPROFILE"];
    HOME_VARS
        .iter()
        .filter_map(std::env::var_os)
        .map(PathBuf::from)
        .find(|path| !path.as_os_str().is_empty())
        .map(|home| home.join(".ssh").join("config"))
        .ok_or_else(|| {
            RemoteClientError::Protocol("could not resolve user home directory".to_string())
        })
}

struct ManagedSpan {
    alias: String,
    start: usize,
    end: usize,
}

#[derive(Default)]
struct HostBlock {
    alias: Option<String>,
    host_offset: usize,
    hostname: Option<String>,
    port: Option<u16>,
    user: Option<String>,
    identity_file: Option<String>,
}

struct ParsedConfig {
    entries: Vec<SshConfigEntry>,
    managed_spans: Vec<ManagedSpan>,
}

fn parse_entries(bytes: &[u8]) -> Vec<SshConfigEntry> {
    parse(bytes).entries
}

fn parse(bytes: &[u8]) -> ParsedConfig {
    let mut entries = Vec::new();
    let mut entry_offsets = Vec::new();
    let mut managed_spans = Vec::new();
    let mut block = HostBlock::default();
    let mut managed_open: Option<(String, usize)> = None;

    for line in Lines::new(bytes) {
        let trimmed = trim_leading_whitespace(line.text);
        if let Some(rest) = marker_alias(trimmed, BEGIN_MARKER) {
            if managed_open.is_none() {
                managed_open = Some((rest.to_string(), line.start));
            }
            continue;
        }
        if let Some(_rest) = marker_alias(trimmed, END_MARKER) {
            if let Some((alias, start)) = managed_open.take() {
                managed_spans.push(ManagedSpan {
                    alias,
                    start,
                    end: line.end,
                });
            }
            continue;
        }
        if trimmed.is_empty() || trimmed.first() == Some(&b'#') {
            continue;
        }
        let (keyword, value) = split_keyword(trimmed);
        // ssh 关键字大小写不敏感。
        if keyword.eq_ignore_ascii_case(b"host") {
            finalize_block(&mut block, &mut entries, &mut entry_offsets);
            if let Some(alias) = selectable_pattern(value) {
                block.alias = Some(alias.to_string());
                block.host_offset = line.start;
            }
        } else if keyword.eq_ignore_ascii_case(b"match") {
            finalize_block(&mut block, &mut entries, &mut entry_offsets);
        } else if block.alias.is_some() {
            if keyword.eq_ignore_ascii_case(b"hostname") && block.hostname.is_none() {
                block.hostname = Some(token_value(value));
            } else if keyword.eq_ignore_ascii_case(b"port") && block.port.is_none() {
                block.port = token_value(value)
                    .parse::<u16>()
                    .ok()
                    .filter(|port| *port != 0);
            } else if keyword.eq_ignore_ascii_case(b"user") && block.user.is_none() {
                block.user = Some(token_value(value));
            } else if keyword.eq_ignore_ascii_case(b"identityfile") && block.identity_file.is_none()
            {
                block.identity_file = Some(token_value(value));
            }
        }
    }
    finalize_block(&mut block, &mut entries, &mut entry_offsets);
    // 只有 Host 行落在成对 BEGIN/END 标记内的条目才是 anywork 管理块；
    // 未闭合标记中的条目按手写只读处理。
    for (entry, offset) in entries.iter_mut().zip(entry_offsets) {
        entry.managed = managed_spans
            .iter()
            .any(|span| span.start <= offset && offset < span.end);
    }
    ParsedConfig {
        entries,
        managed_spans,
    }
}

fn finalize_block(
    block: &mut HostBlock,
    entries: &mut Vec<SshConfigEntry>,
    entry_offsets: &mut Vec<usize>,
) {
    let Some(alias) = block.alias.take() else {
        return;
    };
    // HostName 缺省时 ssh 以别名作为连接主机名。
    let host_name = block.hostname.take().unwrap_or_else(|| alias.clone());
    entries.push(SshConfigEntry {
        profile: SshServerProfile {
            alias,
            host_name,
            port: block.port.take().unwrap_or(22),
            username: block.user.take().unwrap_or_default(),
            identity_file: block.identity_file.take(),
        },
        managed: false,
    });
    entry_offsets.push(block.host_offset);
}

struct Line<'a> {
    text: &'a [u8],
    start: usize,
    end: usize,
}

struct Lines<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> Lines<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }
}

impl<'a> Iterator for Lines<'a> {
    type Item = Line<'a>;

    fn next(&mut self) -> Option<Line<'a>> {
        if self.offset >= self.bytes.len() {
            return None;
        }
        let start = self.offset;
        let end = self.bytes[start..]
            .iter()
            .position(|byte| *byte == b'\n')
            .map_or(self.bytes.len(), |index| start + index + 1);
        self.offset = end;
        let mut text = &self.bytes[start..end];
        if text.last() == Some(&b'\n') {
            text = &text[..text.len() - 1];
        }
        if text.last() == Some(&b'\r') {
            text = &text[..text.len() - 1];
        }
        Some(Line { text, start, end })
    }
}

fn trim_leading_whitespace(text: &[u8]) -> &[u8] {
    let start = text
        .iter()
        .position(|byte| !matches!(byte, b' ' | b'\t'))
        .unwrap_or(text.len());
    &text[start..]
}

/// 命中标记注释时返回标记后的别名原文。
fn marker_alias<'a>(line: &'a [u8], marker: &str) -> Option<&'a str> {
    let marker = marker.as_bytes();
    if line.len() < marker.len() || !line[..marker.len()].eq_ignore_ascii_case(marker) {
        return None;
    }
    let rest = trim_leading_whitespace(&line[marker.len()..]);
    if rest.is_empty() {
        return None;
    }
    std::str::from_utf8(rest).ok().map(str::trim)
}

fn split_keyword(line: &[u8]) -> (&[u8], &[u8]) {
    let split = line
        .iter()
        .position(|byte| byte.is_ascii_whitespace())
        .unwrap_or(line.len());
    let (keyword, rest) = line.split_at(split);
    (keyword, trim_leading_whitespace(rest))
}

/// Host 行只有一个非通配模式时才可作为服务器别名引用。
fn selectable_pattern(value: &[u8]) -> Option<&str> {
    let mut patterns = value
        .split(|byte| byte.is_ascii_whitespace())
        .filter(|p| !p.is_empty());
    let pattern = std::str::from_utf8(patterns.next()?).ok()?;
    if patterns.next().is_some() {
        return None;
    }
    if validate_alias(pattern).is_err() {
        return None;
    }
    Some(pattern)
}

fn token_value(value: &[u8]) -> String {
    let text = String::from_utf8_lossy(value).trim().to_string();
    if text.len() >= 2
        && ((text.starts_with('"') && text.ends_with('"'))
            || (text.starts_with('\'') && text.ends_with('\'')))
    {
        text[1..text.len() - 1].to_string()
    } else {
        text
    }
}

fn render_block(profile: &SshServerProfile, newline: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let line = |out: &mut Vec<u8>, text: &str| {
        out.extend_from_slice(text.as_bytes());
        out.extend_from_slice(newline);
    };
    line(&mut out, &format!("{BEGIN_MARKER} {}", profile.alias));
    line(&mut out, &format!("Host {}", profile.alias));
    line(&mut out, &format!("    HostName {}", profile.host_name));
    line(&mut out, &format!("    Port {}", profile.port));
    line(&mut out, &format!("    User {}", profile.username));
    if let Some(identity_file) = profile
        .identity_file
        .as_deref()
        .filter(|path| !path.trim().is_empty())
    {
        line(&mut out, &format!("    IdentityFile {identity_file}"));
    }
    line(&mut out, &format!("{END_MARKER} {}", profile.alias));
    out
}

fn newline_style(bytes: &[u8]) -> &'static [u8] {
    if bytes.windows(2).any(|window| window == b"\r\n") {
        b"\r\n"
    } else {
        b"\n"
    }
}

fn upsert_bytes(bytes: &[u8], profiles: &[SshServerProfile]) -> Result<Vec<u8>, RemoteClientError> {
    let parsed = parse(bytes);
    let managed_aliases: HashSet<&str> = parsed
        .managed_spans
        .iter()
        .map(|span| span.alias.as_str())
        .collect();
    for profile in profiles {
        // 只有完整管理块可被替换；别名被任何其他条目（手写或未闭合标记）占用时拒绝。
        if managed_aliases.contains(profile.alias.as_str()) {
            continue;
        }
        if parsed
            .entries
            .iter()
            .any(|entry| entry.profile.alias == profile.alias)
        {
            return Err(RemoteClientError::Protocol(format!(
                "SSH alias '{}' is already used by a hand-written entry",
                profile.alias
            )));
        }
    }

    let mut replacements: BTreeMap<usize, (usize, Vec<u8>)> = BTreeMap::new();
    let mut appended: Vec<&SshServerProfile> = Vec::new();
    let newline = newline_style(bytes);
    for profile in profiles {
        match parsed
            .managed_spans
            .iter()
            .find(|span| span.alias == profile.alias)
        {
            Some(span) => {
                replacements.insert(span.start, (span.end, render_block(profile, newline)));
            }
            None => appended.push(profile),
        }
    }

    let mut out = Vec::with_capacity(bytes.len() + 256);
    let mut cursor = 0;
    for (start, (end, block)) in replacements {
        out.extend_from_slice(&bytes[cursor..start]);
        out.extend_from_slice(&block);
        cursor = end;
    }
    out.extend_from_slice(&bytes[cursor.min(bytes.len())..]);
    if !appended.is_empty() {
        if !out.is_empty() && !out.ends_with(b"\n") {
            out.extend_from_slice(newline);
        }
        for profile in appended {
            out.extend_from_slice(&render_block(profile, newline));
        }
    }
    Ok(out)
}

fn remove_bytes(bytes: &[u8], alias: &str) -> Result<Vec<u8>, RemoteClientError> {
    let parsed = parse(bytes);
    if let Some(span) = parsed.managed_spans.iter().find(|span| span.alias == alias) {
        let mut out = Vec::with_capacity(bytes.len());
        out.extend_from_slice(&bytes[..span.start]);
        out.extend_from_slice(&bytes[span.end..]);
        return Ok(out);
    }
    if parsed
        .entries
        .iter()
        .any(|entry| entry.profile.alias == alias)
    {
        return Err(RemoteClientError::Protocol(format!(
            "SSH alias '{alias}' belongs to a hand-written entry"
        )));
    }
    Err(RemoteClientError::Protocol(format!(
        "unknown SSH alias '{alias}'"
    )))
}

fn write_atomic(path: &Path, contents: &[u8]) -> std::io::Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    // 仅本次创建目录时收紧权限，不改动已存在的用户目录；Windows 无此语义。
    #[cfg(unix)]
    let parent_created = !parent.exists();
    std::fs::create_dir_all(parent)?;
    #[cfg(unix)]
    if parent_created {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700))?;
    }
    let mut temp = tempfile::NamedTempFile::new_in(parent)?;
    temp.write_all(contents)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        temp.as_file()
            .set_permissions(std::fs::Permissions::from_mode(0o600))?;
    }
    temp.as_file().sync_all()?;
    temp.persist(path)?;
    #[cfg(unix)]
    std::fs::File::open(parent)?.sync_all()?;
    Ok(())
}
