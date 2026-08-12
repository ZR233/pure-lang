use keyring::{Entry, Error as KeyringError};
use std::collections::BTreeMap;
use std::sync::Mutex;

use crate::{PureError, Result};

const CREDENTIAL_SERVICE: &str = "pure-studio";

pub(super) trait CredentialStore: Send + Sync {
    fn load(&self, provider_id: &str) -> Result<Option<String>>;
    fn save(&self, provider_id: &str, secret: &str) -> Result<()>;
    fn delete(&self, provider_id: &str) -> Result<()>;
}

#[derive(Debug, Default)]
pub(super) struct SystemCredentialStore;

impl CredentialStore for SystemCredentialStore {
    fn load(&self, provider_id: &str) -> Result<Option<String>> {
        match entry(provider_id)?.get_password() {
            Ok(secret) => Ok(Some(secret)),
            Err(KeyringError::NoEntry) => Ok(None),
            Err(error) => Err(credential_error("read", provider_id, error)),
        }
    }

    fn save(&self, provider_id: &str, secret: &str) -> Result<()> {
        entry(provider_id)?
            .set_password(secret)
            .map_err(|error| credential_error("write", provider_id, error))
    }

    fn delete(&self, provider_id: &str) -> Result<()> {
        match entry(provider_id)?.delete_credential() {
            Ok(()) | Err(KeyringError::NoEntry) => Ok(()),
            Err(error) => Err(credential_error("delete", provider_id, error)),
        }
    }
}

#[derive(Debug, Default)]
pub(super) struct MemoryCredentialStore {
    values: Mutex<BTreeMap<String, String>>,
}

impl CredentialStore for MemoryCredentialStore {
    fn load(&self, provider_id: &str) -> Result<Option<String>> {
        Ok(self.values.lock().unwrap().get(provider_id).cloned())
    }

    fn save(&self, provider_id: &str, secret: &str) -> Result<()> {
        self.values
            .lock()
            .unwrap()
            .insert(provider_id.to_string(), secret.to_string());
        Ok(())
    }

    fn delete(&self, provider_id: &str) -> Result<()> {
        self.values.lock().unwrap().remove(provider_id);
        Ok(())
    }
}

fn entry(provider_id: &str) -> Result<Entry> {
    Entry::new(CREDENTIAL_SERVICE, &format!("provider:{provider_id}"))
        .map_err(|error| credential_error("open", provider_id, error))
}

fn credential_error(action: &str, provider_id: &str, error: KeyringError) -> PureError {
    PureError::ConfigError(format!(
        "failed to {action} system credential for provider {provider_id}: {error}"
    ))
}
