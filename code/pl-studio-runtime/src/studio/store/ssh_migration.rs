//! v20→v21 产品迁移：`ssh_servers` 行迁出产品库，成为用户 `~/.ssh/config` 管理块。
//!
//! 步骤幂等可重试：先写配置文件（同别名管理块按内容替换，别名分配只避开手写条目），
//! 再在单事务内以当前 schema 的 canonical DDL 重建 `projects`（`ssh_server_id` UUID 重写为
//! `ssh_alias` 别名）、删除旧表并推进版本；提交前执行外键校验。

use std::collections::HashSet;

use anyhow::{Context, Result};
use sea_orm::{
    ConnectOptions, ConnectionTrait, Database, DatabaseBackend, DatabaseConnection, Statement,
    TransactionTrait,
};
use serde::Deserialize;

use pl_tool::remote::{SshConfigFile, SshServerProfile, allocate_alias, sanitize_alias_base};

use crate::studio::store_support;

/// 迁移一行旧 `ssh_servers` 记录所需的非敏感事实。
struct LegacySshServerRow {
    id: String,
    name: String,
    host: String,
    port: u16,
    username: String,
    auth_json: String,
}

/// 仅在迁移边界解码的历史认证结构；运行时不再消费该格式。
#[derive(Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
enum LegacySshAuth {
    AgentOrKey { identity_file: Option<String> },
    Password,
}

/// 把 v20 产品库的 SSH 服务器迁移为 `ssh_config` 指向文件中的管理块。
///
/// v21 库没有 `ssh_servers` 表，此时为无操作；文件与数据库都写入成功才返回。
pub(in crate::studio) async fn migrate_ssh_servers_to_user_config(
    db: &DatabaseConnection,
    ssh_config: &SshConfigFile,
) -> Result<()> {
    if !ssh_servers_table_exists(db).await? {
        return Ok(());
    }
    let rows = read_legacy_rows(db).await?;
    let entries = ssh_config
        .read()
        .await
        .map_err(|error| anyhow::anyhow!(error))
        .context("failed to read the user ssh config during migration")?;
    // 只避开手写别名；本迁移已写入的管理块按内容替换，保证重试幂等。
    let mut taken: HashSet<String> = entries
        .iter()
        .filter(|entry| !entry.managed)
        .map(|entry| entry.profile.alias.clone())
        .collect();
    let mut profiles = Vec::with_capacity(rows.len());
    let mut alias_by_id = Vec::with_capacity(rows.len());
    for row in rows {
        let identity_file = match serde_json::from_str::<LegacySshAuth>(&row.auth_json) {
            Ok(LegacySshAuth::AgentOrKey { identity_file }) => {
                identity_file.filter(|path| !path.trim().is_empty())
            }
            _ => None,
        };
        let alias = allocate_alias(&sanitize_alias_base(&row.name), &taken);
        taken.insert(alias.clone());
        alias_by_id.push((row.id, alias.clone()));
        profiles.push(SshServerProfile {
            alias,
            host_name: row.host,
            port: row.port,
            username: row.username,
            identity_file,
        });
    }
    ssh_config
        .upsert_managed(&profiles)
        .await
        .map_err(|error| anyhow::anyhow!(error))
        .context("failed to write anywork server blocks to the user ssh config")?;
    rewrite_projects_table(db, &alias_by_id).await
}

async fn ssh_servers_table_exists(db: &DatabaseConnection) -> Result<bool> {
    let row = db
        .query_one_raw(Statement::from_string(
            DatabaseBackend::Sqlite,
            "SELECT COUNT(*) AS count FROM sqlite_schema WHERE type = 'table' AND name = 'ssh_servers'"
                .to_owned(),
        ))
        .await?
        .context("missing sqlite_schema response")?;
    Ok(row.try_get::<i64>("", "count")? > 0)
}

async fn read_legacy_rows(db: &DatabaseConnection) -> Result<Vec<LegacySshServerRow>> {
    let raw = db
        .query_all_raw(Statement::from_string(
            DatabaseBackend::Sqlite,
            "SELECT id, name, host, port, username, auth_json FROM ssh_servers ORDER BY created_at, id"
                .to_owned(),
        ))
        .await
        .context("failed to read legacy ssh_servers rows")?;
    raw.into_iter()
        .map(|row| {
            let port = row.try_get::<i64>("", "port")?;
            anyhow::ensure!(
                (1..=i64::from(u16::MAX)).contains(&port),
                "legacy SSH server {} has an invalid port",
                row.try_get::<String>("", "id")?
            );
            Ok(LegacySshServerRow {
                id: row.try_get("", "id")?,
                name: row.try_get("", "name")?,
                host: row.try_get("", "host")?,
                port: u16::try_from(port)?,
                username: row.try_get("", "username")?,
                auth_json: row.try_get("", "auth_json")?,
            })
        })
        .collect()
}

async fn rewrite_projects_table(
    db: &DatabaseConnection,
    alias_by_id: &[(String, String)],
) -> Result<()> {
    let (projects_ddl, index_ddls) = canonical_projects_ddl().await?;
    let migrated_ddl = rename_created_table(&projects_ddl, "projects_migration")
        .context("canonical projects DDL does not name its table")?;
    // 外键约束必须在线程外关闭；重建期间 projects 短暂不存在。
    db.execute_unprepared("PRAGMA foreign_keys=OFF").await?;
    let transaction = db.begin().await?;
    let result = async {
        transaction
            .execute_unprepared(
                "CREATE TEMP TABLE ssh_alias_map(uuid TEXT PRIMARY KEY, alias TEXT NOT NULL);",
            )
            .await?;
        for (uuid, alias) in alias_by_id {
            transaction
                .execute_raw(Statement::from_sql_and_values(
                    DatabaseBackend::Sqlite,
                    "INSERT OR REPLACE INTO ssh_alias_map(uuid, alias) VALUES (?, ?)",
                    [uuid.as_str().into(), alias.as_str().into()],
                ))
                .await?;
        }
        transaction.execute_unprepared(&migrated_ddl).await?;
        transaction
            .execute_unprepared(
                "INSERT INTO projects_migration
                    (id, name, path, ssh_alias, created_at, updated_at, last_opened_at, closed)
                 SELECT p.id, p.name, p.path, m.alias, p.created_at, p.updated_at,
                        p.last_opened_at, p.closed
                 FROM projects p
                 LEFT JOIN ssh_alias_map m ON m.uuid = p.ssh_server_id;",
            )
            .await?;
        transaction
            .execute_unprepared("DROP TABLE projects;")
            .await?;
        transaction
            .execute_unprepared("ALTER TABLE projects_migration RENAME TO projects;")
            .await?;
        for ddl in &index_ddls {
            transaction.execute_unprepared(ddl).await?;
        }
        transaction
            .execute_unprepared("DROP TABLE ssh_servers;")
            .await?;
        transaction
            .execute_unprepared("DROP TABLE ssh_alias_map;")
            .await?;
        transaction
            .execute_unprepared("PRAGMA user_version=21;")
            .await?;
        Ok::<_, anyhow::Error>(())
    }
    .await;
    match result {
        Ok(()) => transaction.commit().await?,
        Err(error) => {
            let rollback = transaction.rollback().await;
            restore_foreign_keys(db).await;
            return Err(error).context(match rollback {
                Ok(()) => "failed to rewrite projects for ssh aliases".to_string(),
                Err(rollback_error) => format!(
                    "failed to rewrite projects for ssh aliases; rollback also failed: {rollback_error}"
                ),
            });
        }
    }
    restore_foreign_keys(db).await;
    let violations = db
        .query_all_raw(Statement::from_string(
            DatabaseBackend::Sqlite,
            "PRAGMA foreign_key_check".to_owned(),
        ))
        .await?;
    anyhow::ensure!(
        violations.is_empty(),
        "ssh alias migration left foreign key violations"
    );
    Ok(())
}

async fn restore_foreign_keys(db: &DatabaseConnection) {
    if let Err(error) = db.execute_unprepared("PRAGMA foreign_keys=ON").await {
        tracing::warn!(%error, "failed to restore SQLite foreign keys after migration");
    }
}

/// 当前 schema 中 `projects` 表与索引的 canonical DDL，迁移重建必须与其逐字一致。
async fn canonical_projects_ddl() -> Result<(String, Vec<String>)> {
    let mut options = ConnectOptions::new("sqlite::memory:");
    options.max_connections(1).min_connections(1);
    let db = Database::connect(options).await?;
    let result = async {
        store_support::initialize_studio_schema(&db).await?;
        let rows = db
            .query_all_raw(Statement::from_string(
                DatabaseBackend::Sqlite,
                "SELECT name, sql FROM sqlite_schema
                 WHERE tbl_name = 'projects' AND type IN ('table', 'index') AND sql IS NOT NULL"
                    .to_owned(),
            ))
            .await?;
        let mut table = None;
        let mut indexes = Vec::new();
        for row in rows {
            let name: String = row.try_get("", "name")?;
            let ddl: String = row.try_get("", "sql")?;
            if name == "projects" {
                table = Some(ddl);
            } else {
                indexes.push(ddl);
            }
        }
        Ok::<_, anyhow::Error>((table, indexes))
    }
    .await;
    let close = db.close().await;
    let (table, indexes) = result?;
    close.map_err(|error| anyhow::anyhow!("failed to close canonical schema probe: {error}"))?;
    let table = table.context("canonical schema has no projects table")?;
    Ok((table, indexes))
}

fn rename_created_table(ddl: &str, replacement: &str) -> Option<String> {
    let lower = ddl.to_ascii_lowercase();
    let marker = "create table ";
    let mut cursor = lower.find(marker)? + marker.len();
    loop {
        cursor += ddl[cursor..].len() - ddl[cursor..].trim_start().len();
        if lower[cursor..].starts_with("if not exists") {
            cursor += "if not exists".len();
            continue;
        }
        break;
    }
    let quoted = ddl.as_bytes().get(cursor) == Some(&b'"');
    let name_start = cursor + usize::from(quoted);
    let name_end = if quoted {
        name_start + ddl[name_start..].find('"')?
    } else {
        name_start + ddl[name_start..].find(|c: char| c.is_whitespace() || c == '(')?
    };
    Some(format!(
        "{}{}{}",
        &ddl[..name_start],
        replacement,
        &ddl[name_end..]
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    /// 从当前 schema 降级构造完整 v20 库：`projects` 回到 `ssh_server_id` 并补回 `ssh_servers`。
    async fn legacy_database(path: &std::path::Path) {
        let create = format!(
            "sqlite://{}?mode=rwc",
            path.to_str().expect("non-UTF8 database path")
        );
        let mut options = ConnectOptions::new(create);
        options.max_connections(1).min_connections(1);
        let db = Database::connect(options).await.unwrap();
        store_support::initialize_studio_schema(&db).await.unwrap();
        let (projects_ddl, index_ddls) = canonical_projects_ddl().await.unwrap();
        let v20_projects = rename_created_table(
            &projects_ddl.replace("ssh_alias", "ssh_server_id"),
            "projects_v20",
        )
        .expect("canonical projects DDL names its table");
        db.execute_unprepared("PRAGMA foreign_keys=OFF;")
            .await
            .unwrap();
        let transaction = db.begin().await.unwrap();
        let result: Result<()> = async {
            transaction
                .execute_unprepared("DROP TABLE projects;")
                .await?;
            transaction.execute_unprepared(&v20_projects).await?;
            transaction
                .execute_unprepared(
                    "CREATE TABLE ssh_servers (
                        id TEXT PRIMARY KEY NOT NULL,
                        name TEXT NOT NULL,
                        host TEXT NOT NULL,
                        port INTEGER NOT NULL,
                        username TEXT NOT NULL,
                        auth_json TEXT NOT NULL,
                        created_at INTEGER NOT NULL,
                        updated_at INTEGER NOT NULL
                    );
                    INSERT INTO ssh_servers VALUES
                        ('ssh-server-ci', 'CI Box', 'ci.example.test', 2222, 'dev',
                         '{\"kind\":\"agentOrKey\",\"identityFile\":\"/keys/ci\"}', 10, 10),
                        ('ssh-server-manual', 'CI Box', 'manual.example.test', 22, 'ops',
                         '{\"kind\":\"password\"}', 11, 11);
                    INSERT INTO projects_v20 VALUES
                        ('project-1', 'Local', '/home/dev/local', NULL, 5, 5, 5, 0),
                        ('project-2', 'CI', '/srv/app', 'ssh-server-ci', 6, 6, 6, 0),
                        ('project-3', 'Manual', '/srv/ops', 'ssh-server-manual', 7, 7, 7, 0);
                    ALTER TABLE projects_v20 RENAME TO projects;",
                )
                .await?;
            for ddl in &index_ddls {
                transaction
                    .execute_unprepared(&ddl.replace("ssh_alias", "ssh_server_id"))
                    .await?;
            }
            transaction
                .execute_unprepared("PRAGMA user_version=20;")
                .await?;
            Ok(())
        }
        .await;
        match result {
            Ok(()) => transaction.commit().await.unwrap(),
            Err(error) => {
                transaction.rollback().await.unwrap();
                panic!("failed to build the v20 fixture: {error:#}");
            }
        }
        db.execute_unprepared("PRAGMA foreign_keys=ON;")
            .await
            .unwrap();
        db.close().await.unwrap();
    }

    fn connect(path: &std::path::Path) -> ConnectOptions {
        let mut options = ConnectOptions::new(format!(
            "sqlite://{}",
            path.to_str().expect("non-UTF8 database path")
        ));
        options.max_connections(1).min_connections(1);
        options
    }

    async fn aliases(db: &DatabaseConnection) -> Vec<(String, Option<String>)> {
        let rows = db
            .query_all_raw(Statement::from_string(
                DatabaseBackend::Sqlite,
                "SELECT id, ssh_alias FROM projects ORDER BY id".to_owned(),
            ))
            .await
            .unwrap();
        rows.into_iter()
            .map(|row| {
                (
                    row.try_get::<String>("", "id").unwrap(),
                    row.try_get::<Option<String>>("", "ssh_alias").unwrap(),
                )
            })
            .collect()
    }

    #[tokio::test]
    async fn migration_moves_rows_to_config_blocks_and_rewrites_projects() {
        let directory = tempfile::tempdir().unwrap();
        let database_path = directory.path().join("studio.sqlite");
        legacy_database(&database_path).await;
        let ssh_config = SshConfigFile::at(directory.path().join("ssh").join("config"));
        std::fs::create_dir_all(ssh_config.path().parent().unwrap()).unwrap();
        std::fs::write(
            ssh_config.path(),
            "Host handwritten\n    HostName example.test\n",
        )
        .unwrap();

        let db = Database::connect(connect(&database_path)).await.unwrap();
        migrate_ssh_servers_to_user_config(&db, &ssh_config)
            .await
            .unwrap();

        let entries = ssh_config.read().await.unwrap();
        let ci = entries
            .iter()
            .find(|entry| entry.profile.alias == "CI-Box")
            .expect("migrated CI entry");
        assert_eq!(ci.profile.host_name, "ci.example.test");
        assert_eq!(ci.profile.port, 2222);
        assert_eq!(ci.profile.identity_file.as_deref(), Some("/keys/ci"));
        assert!(ci.managed);
        // 同名第二台服务器分配不冲突后缀；密码认证行不携带私钥。
        let manual = entries
            .iter()
            .find(|entry| entry.profile.alias == "CI-Box-2")
            .expect("suffixed manual entry");
        assert_eq!(manual.profile.host_name, "manual.example.test");
        assert_eq!(manual.profile.identity_file, None);

        assert_eq!(
            aliases(&db).await,
            vec![
                ("project-1".into(), None),
                ("project-2".into(), Some("CI-Box".into())),
                ("project-3".into(), Some("CI-Box-2".into())),
            ]
        );
        let version = db
            .query_one_raw(Statement::from_string(
                DatabaseBackend::Sqlite,
                "PRAGMA user_version".to_owned(),
            ))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(version.try_get::<i64>("", "user_version").unwrap(), 21);
        let table = db
            .query_one_raw(Statement::from_string(
                DatabaseBackend::Sqlite,
                "SELECT COUNT(*) AS count FROM sqlite_schema WHERE name = 'ssh_servers'".to_owned(),
            ))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(table.try_get::<i64>("", "count").unwrap(), 0);
        db.close().await.unwrap();

        // 迁移后的库必须能以当前 schema 直接打开（含指纹校验）。
        crate::studio::store::StudioStore::open(&database_path)
            .await
            .expect("migrated database opens with the current schema");
    }

    #[tokio::test]
    async fn migration_is_idempotent_across_retries() {
        let directory = tempfile::tempdir().unwrap();
        let database_path = directory.path().join("studio.sqlite");
        legacy_database(&database_path).await;
        let ssh_config = SshConfigFile::at(directory.path().join("config"));

        // 模拟文件写入后、数据库提交前中断：重新执行整个迁移。
        let db = Database::connect(connect(&database_path)).await.unwrap();
        migrate_ssh_servers_to_user_config(&db, &ssh_config)
            .await
            .unwrap();
        let first = std::fs::read_to_string(ssh_config.path()).unwrap();
        migrate_ssh_servers_to_user_config(&db, &ssh_config)
            .await
            .unwrap();
        assert_eq!(std::fs::read_to_string(ssh_config.path()).unwrap(), first);
        assert_eq!(aliases(&db).await.len(), 3);
        db.close().await.unwrap();
    }

    #[test]
    fn table_rename_handles_quoted_and_bare_names() {
        assert_eq!(
            rename_created_table(r#"create table "projects" (id TEXT)"#, "renamed").as_deref(),
            Some(r#"create table "renamed" (id TEXT)"#)
        );
        assert_eq!(
            rename_created_table("CREATE TABLE IF NOT EXISTS projects (id TEXT)", "renamed")
                .as_deref(),
            Some("CREATE TABLE IF NOT EXISTS renamed (id TEXT)")
        );
        assert_eq!(
            rename_created_table("CREATE INDEX x ON projects(id)", "renamed"),
            None
        );
    }
}
