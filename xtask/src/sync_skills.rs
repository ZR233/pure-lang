//! 手动同步上游预置系统技能到 `pl-studio-runtime` 的源码资产目录。
//!
//! 命令浅拉取各上游仓库默认分支的最新提交（大仓库使用 blobless partial clone 加
//! sparse checkout），把选中技能目录完全替换到
//! `code/pl-studio-runtime/assets/skills/` 下并校验 frontmatter；同步结果提交进
//! 源码库，源码库即 canonical 内容，构建期不再访问网络。上游来源、revision 与许可
//! 记录在 `code/pl-studio-runtime/THIRD_PARTY_NOTICES.md`。

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};

use crate::paths::workspace_root;
use crate::process::configure_background_command;

struct PresetSkillSource {
    /// 缓存目录名，位于 `target/xtask-sync-skills/` 下。
    cache_name: &'static str,
    repo_url: &'static str,
    /// 仓库内保存选中技能的目录。
    skills_path: &'static str,
    skills: &'static [&'static str],
    /// 大仓库使用 blobless partial clone 加 sparse checkout 缩小下载。
    sparse: bool,
}

struct SourceCheckout {
    cache_dir: PathBuf,
    revision: String,
}

const PRESET_SKILL_SOURCES: &[PresetSkillSource] = &[
    PresetSkillSource {
        cache_name: "anthropics-skills",
        repo_url: "https://github.com/anthropics/skills.git",
        skills_path: "skills",
        skills: &["canvas-design", "frontend-design"],
        sparse: false,
    },
    PresetSkillSource {
        cache_name: "hermes-agent",
        repo_url: "https://github.com/NousResearch/hermes-agent.git",
        skills_path: "skills/productivity",
        skills: &["docx", "pdf", "powerpoint", "xlsx"],
        sparse: true,
    },
];

pub(crate) fn run() -> Result<()> {
    let root = workspace_root()?;
    let cache_root = root.join("target").join("xtask-sync-skills");
    let assets_dir = root
        .join("code")
        .join("pl-studio-runtime")
        .join("assets")
        .join("skills");

    for source in PRESET_SKILL_SOURCES {
        let checkout = fetch_latest(source, &cache_root)?;
        replace_skills(source, &checkout, &assets_dir)?;
        println!(
            "synced {} from {} at {}",
            source.skills.join(", "),
            source.repo_url,
            checkout.revision
        );
    }
    println!(
        "review the diff, update revisions in \
         code/pl-studio-runtime/THIRD_PARTY_NOTICES.md, and commit the refreshed Skills"
    );
    Ok(())
}

fn fetch_latest(source: &PresetSkillSource, cache_root: &Path) -> Result<SourceCheckout> {
    let cache_dir = cache_root.join(source.cache_name);
    if !cache_dir.join(".git").exists() {
        discard_directory(&cache_dir)?;
        fs::create_dir_all(&cache_dir)
            .with_context(|| format!("failed to create Skill cache '{}'", cache_dir.display()))?;
        git(&cache_dir, &["init", "--quiet"])?;
        git(&cache_dir, &["remote", "add", "origin", source.repo_url])?;
        if source.sparse {
            git(&cache_dir, &["config", "remote.origin.promisor", "true"])?;
            git(
                &cache_dir,
                &["config", "remote.origin.partialclonefilter", "blob:none"],
            )?;
            let sparse_dirs: Vec<String> = source
                .skills
                .iter()
                .map(|skill| format!("{}/{}", source.skills_path, skill))
                .collect();
            let mut sparse_args: Vec<&str> = vec!["sparse-checkout", "set"];
            sparse_args.extend(sparse_dirs.iter().map(String::as_str));
            git(&cache_dir, &sparse_args)?;
        }
    }

    eprintln!("fetching latest {} ...", source.repo_url);
    let mut fetch_args: Vec<&str> = vec!["fetch", "--depth", "1"];
    if source.sparse {
        fetch_args.push("--filter=blob:none");
    }
    // 显式拉取远程默认分支；`git remote add` 不配置 fetch refspec，无 refspec 的
    // fetch 会拉取所有分支并让 FETCH_HEAD 指向字母序第一的分支头。
    fetch_args.extend(["origin", "HEAD"]);
    git(&cache_dir, &fetch_args)?;
    git(&cache_dir, &["checkout", "--detach", "FETCH_HEAD"])?;
    let revision = git(&cache_dir, &["rev-parse", "HEAD"])?;
    Ok(SourceCheckout {
        cache_dir,
        revision,
    })
}

fn replace_skills(
    source: &PresetSkillSource,
    checkout: &SourceCheckout,
    assets_dir: &Path,
) -> Result<()> {
    for skill in source.skills {
        let from = checkout.cache_dir.join(source.skills_path).join(skill);
        let document = from.join("SKILL.md");
        if !document.is_file() {
            bail!(
                "preset Skill '{skill}' from {} at {} has no SKILL.md at '{}'",
                source.repo_url,
                checkout.revision,
                document.display()
            );
        }
        validate_skill_document(&document, skill)?;
        let to = assets_dir.join(skill);
        discard_directory(&to)?;
        copy_directory(&from, &to).with_context(|| {
            format!(
                "failed to copy preset Skill '{skill}' from '{}' to '{}'",
                from.display(),
                to.display()
            )
        })?;
    }
    Ok(())
}

/// 提前执行与启动刷新一致的最低限度校验，避免把格式损坏的技能提交进源码库。
fn validate_skill_document(document: &Path, expected_name: &str) -> Result<()> {
    let content = fs::read_to_string(document)
        .with_context(|| format!("failed to read '{}'", document.display()))?;
    let parsed = pl_skill_core::parse_skill_document(&content)
        .with_context(|| format!("invalid Skill frontmatter: '{}'", document.display()))?;
    if !parsed.frontmatter.name.eq_ignore_ascii_case(expected_name) {
        bail!(
            "Skill frontmatter name '{}' does not match directory '{expected_name}' in '{}'",
            parsed.frontmatter.name,
            document.display()
        );
    }
    if parsed.frontmatter.description.trim().is_empty() {
        bail!("Skill description is empty in '{}'", document.display());
    }
    Ok(())
}

fn discard_directory(path: &Path) -> Result<()> {
    match fs::remove_dir_all(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => {
            Err(anyhow::Error::new(error).context(format!("failed to remove '{}'", path.display())))
        }
    }
}

fn copy_directory(from: &Path, to: &Path) -> std::io::Result<()> {
    fs::create_dir_all(to)?;
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_directory(&entry.path(), &target)?;
        } else {
            // 跟随 symlink，保证嵌入资产始终是真实文件。
            fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}

fn git(working_dir: &Path, args: &[&str]) -> Result<String> {
    let mut command = Command::new("git");
    command
        .arg("-c")
        .arg("core.autocrlf=false")
        .arg("-C")
        .arg(working_dir)
        .args(args);
    configure_background_command(&mut command);
    let output = command
        .output()
        .with_context(|| format!("failed to spawn git {:?}", args))?;
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    if !output.status.success() {
        bail!(
            "git {:?} failed in '{}' with {}.\nstdout: {stdout}\nstderr: {stderr}",
            args,
            working_dir.display(),
            output.status
        );
    }
    Ok(stdout)
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    #[test]
    fn skill_names_are_unique_across_sources() {
        let mut names = PRESET_SKILL_SOURCES
            .iter()
            .flat_map(|source| source.skills.iter().copied())
            .collect::<Vec<_>>();
        names.sort_unstable();
        let count = names.len();
        names.dedup();
        assert_eq!(names.len(), count, "preset Skill names must be unique");
    }

    #[test]
    fn copy_directory_replaces_nested_content_entirely() {
        let root = tempfile::tempdir().unwrap();
        let from = root.path().join("from");
        let to = root.path().join("to");
        fs::create_dir_all(from.join("scripts")).unwrap();
        fs::write(from.join("SKILL.md"), "skill").unwrap();
        fs::write(from.join("scripts").join("helper.py"), "helper").unwrap();

        fs::create_dir_all(to.join("obsolete")).unwrap();
        fs::write(to.join("SKILL.md"), "stale").unwrap();
        fs::write(to.join("obsolete").join("stale.txt"), "stale").unwrap();

        discard_directory(&to).unwrap();
        copy_directory(&from, &to).unwrap();

        assert_eq!(fs::read_to_string(to.join("SKILL.md")).unwrap(), "skill");
        assert_eq!(
            fs::read_to_string(to.join("scripts").join("helper.py")).unwrap(),
            "helper"
        );
        assert!(!to.join("obsolete").exists());
    }
}
