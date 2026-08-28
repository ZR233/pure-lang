use pl_core::remote::{SshAuth, SshConnectionSnapshot, SshConnectionState, SshServerProfile};

use crate::api::studio::bridge_runtime::active_bridge;
use crate::api::studio::types::{
    BridgeError, ProjectDto, RemoteDirectoryEntryDto, RemoteDirectoryListingDto,
    SaveSshServerRequest, SshAuthKindDto, SshConnectionSnapshotDto, SshServerDto,
};

pub async fn list_ssh_servers() -> Result<Vec<SshServerDto>, BridgeError> {
    let bridge = active_bridge().await?;
    Ok(bridge
        .studio
        .list_ssh_servers()
        .await?
        .into_iter()
        .map(server_dto)
        .collect())
}

pub async fn save_ssh_server(request: SaveSshServerRequest) -> Result<SshServerDto, BridgeError> {
    let bridge = active_bridge().await?;
    let auth = match request.auth_kind {
        SshAuthKindDto::AgentOrKey => SshAuth::AgentOrKey {
            identity_file: request.identity_file.filter(|path| !path.trim().is_empty()),
        },
        SshAuthKindDto::Password => SshAuth::Password,
    };
    let profile = bridge
        .studio
        .save_ssh_server(
            SshServerProfile {
                id: request.id.unwrap_or_default(),
                name: request.name,
                host: request.host,
                port: request.port,
                username: request.username,
                auth,
            },
            request.password,
        )
        .await?;
    Ok(server_dto(profile))
}

pub async fn delete_ssh_server(server_id: String) -> Result<(), BridgeError> {
    active_bridge()
        .await?
        .studio
        .delete_ssh_server(&server_id)
        .await?;
    Ok(())
}

pub async fn test_ssh_connection(
    server_id: String,
) -> Result<SshConnectionSnapshotDto, BridgeError> {
    let snapshot = active_bridge()
        .await?
        .studio
        .test_ssh_connection(&server_id)
        .await?;
    Ok(connection_dto(snapshot))
}

pub async fn reconnect_ssh_server(
    server_id: String,
) -> Result<SshConnectionSnapshotDto, BridgeError> {
    let snapshot = active_bridge()
        .await?
        .studio
        .reconnect_ssh_server(&server_id)
        .await?;
    Ok(connection_dto(snapshot))
}

pub async fn browse_remote_directories(
    server_id: String,
    path: Option<String>,
) -> Result<RemoteDirectoryListingDto, BridgeError> {
    let listing = active_bridge()
        .await?
        .studio
        .browse_remote_directories(&server_id, path)
        .await?;
    Ok(RemoteDirectoryListingDto {
        path: listing.path,
        parent: listing.parent,
        entries: listing
            .entries
            .into_iter()
            .filter(|entry| entry.is_directory && !entry.is_symlink)
            .map(|entry| RemoteDirectoryEntryDto {
                name: entry.name,
                path: entry.path,
                is_directory: entry.is_directory,
            })
            .collect(),
    })
}

pub async fn open_remote_project(
    server_id: String,
    path: String,
) -> Result<ProjectDto, BridgeError> {
    Ok(active_bridge()
        .await?
        .studio
        .open_remote_project(&server_id, path)
        .await?
        .into())
}

fn server_dto(profile: SshServerProfile) -> SshServerDto {
    let (auth_kind, identity_file) = match profile.auth {
        SshAuth::AgentOrKey { identity_file } => (SshAuthKindDto::AgentOrKey, identity_file),
        SshAuth::Password => (SshAuthKindDto::Password, None),
    };
    SshServerDto {
        id: profile.id,
        name: profile.name,
        host: profile.host,
        port: profile.port,
        username: profile.username,
        auth_kind,
        identity_file,
    }
}

fn connection_dto(snapshot: SshConnectionSnapshot) -> SshConnectionSnapshotDto {
    let mut dto = SshConnectionSnapshotDto {
        server_id: snapshot.server_id,
        state: String::new(),
        helper_version: None,
        architecture: None,
        attempt: None,
        delay_seconds: None,
        error_code: None,
        error_message: None,
    };
    match snapshot.state {
        SshConnectionState::Disconnected => dto.state = "disconnected".to_string(),
        SshConnectionState::Connecting => dto.state = "connecting".to_string(),
        SshConnectionState::WaitingForInput => dto.state = "waitingForInput".to_string(),
        SshConnectionState::Ready {
            helper_version,
            architecture,
        } => {
            dto.state = "ready".to_string();
            dto.helper_version = Some(helper_version);
            dto.architecture = Some(architecture);
        }
        SshConnectionState::Reconnecting {
            attempt,
            delay_seconds,
        } => {
            dto.state = "reconnecting".to_string();
            dto.attempt = Some(attempt);
            dto.delay_seconds = Some(delay_seconds);
        }
        SshConnectionState::Failed { code, message } => {
            dto.state = "failed".to_string();
            dto.error_code = Some(code);
            dto.error_message = Some(message);
        }
    }
    dto
}
