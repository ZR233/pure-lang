use std::path::{Path, PathBuf};
use std::sync::Mutex;

use pl_patch::{PatchBackend, PatchError, PatchPathDisplay};
use pl_protocol::PureError;

use super::{TOOL_APPLY_SESSION_NOTE_PATCH, tool_error};

const SESSION_NOTE_PATH: &str = "session-note.md";

pub(super) async fn apply(content: Option<String>, patch: &str) -> Result<String, PureError> {
    if patch
        .lines()
        .any(|line| line.trim_start().starts_with("*** Move to:"))
    {
        return Err(tool_error(
            TOOL_APPLY_SESSION_NOTE_PATCH,
            "session note patches do not support moves",
        ));
    }
    let backend = SessionNotePatchBackend::new(content);
    pl_patch::apply_patch(patch, &backend)
        .await
        .map_err(|error| tool_error(TOOL_APPLY_SESSION_NOTE_PATCH, error.into_message()))?;
    Ok(backend.content().unwrap_or_default())
}

#[derive(Debug)]
struct SessionNotePatchBackend {
    content: Mutex<Option<String>>,
}

impl SessionNotePatchBackend {
    fn new(content: Option<String>) -> Self {
        Self {
            content: Mutex::new(content),
        }
    }

    fn content(&self) -> Option<String> {
        self.content
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }
}

impl PatchPathDisplay for SessionNotePatchBackend {
    fn display_path(&self, path: &Path) -> String {
        path.display().to_string()
    }
}

impl PatchBackend for SessionNotePatchBackend {
    async fn resolve_existing<'a>(&'a self, path: &'a str) -> Result<PathBuf, PatchError> {
        validate_path(path)?;
        if self.content().is_none() {
            return Err(PatchError::new("session-note.md does not exist"));
        }
        Ok(PathBuf::from(SESSION_NOTE_PATH))
    }

    async fn resolve_for_write<'a>(&'a self, path: &'a str) -> Result<PathBuf, PatchError> {
        validate_path(path)?;
        Ok(PathBuf::from(SESSION_NOTE_PATH))
    }

    async fn reject_symlink_write<'a>(&'a self, path: &'a Path) -> Result<(), PatchError> {
        validate_path_value(path)
    }

    async fn ensure_file<'a>(&'a self, path: &'a Path) -> Result<(), PatchError> {
        validate_path_value(path)?;
        if self.content().is_none() {
            return Err(PatchError::new("session-note.md does not exist"));
        }
        Ok(())
    }

    async fn read_to_string<'a>(&'a self, path: &'a Path) -> Result<String, PatchError> {
        validate_path_value(path)?;
        self.content()
            .ok_or_else(|| PatchError::new("session-note.md does not exist"))
    }

    async fn read_optional_text<'a>(
        &'a self,
        path: &'a Path,
    ) -> Result<Option<String>, PatchError> {
        validate_path_value(path)?;
        Ok(self.content())
    }

    async fn create_parent_dirs<'a>(&'a self, path: &'a Path) -> Result<(), PatchError> {
        validate_path_value(path)
    }

    async fn write_text<'a>(&'a self, path: &'a Path, content: &'a str) -> Result<(), PatchError> {
        validate_path_value(path)?;
        *self
            .content
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(content.to_string());
        Ok(())
    }

    async fn remove_file<'a>(&'a self, path: &'a Path) -> Result<(), PatchError> {
        validate_path_value(path)?;
        *self
            .content
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
        Ok(())
    }
}

fn validate_path(path: &str) -> Result<(), PatchError> {
    if path == SESSION_NOTE_PATH {
        Ok(())
    } else {
        Err(PatchError::new(format!(
            "session note patch path must be {SESSION_NOTE_PATH}"
        )))
    }
}

fn validate_path_value(path: &Path) -> Result<(), PatchError> {
    path.to_str()
        .ok_or_else(|| PatchError::new("session note patch path must be UTF-8"))
        .and_then(validate_path)
}
