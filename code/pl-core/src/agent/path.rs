use std::fmt;
use std::ops::Deref;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

/// Canonical multi-agent path.
///
/// Paths are absolute and rooted at `/root`. Child names use lowercase
/// letters, digits, and underscores so model-facing references remain stable.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct AgentPath(String);

impl AgentPath {
    pub const ROOT: &str = "/root";

    pub fn root() -> Self {
        Self(Self::ROOT.to_string())
    }

    pub fn from_string(path: String) -> Result<Self, String> {
        validate_absolute_path(&path)?;
        Ok(Self(path))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn is_root(&self) -> bool {
        self.as_str() == Self::ROOT
    }

    pub fn join(&self, agent_name: &str) -> Result<Self, String> {
        validate_agent_name(agent_name)?;
        Self::from_string(format!("{self}/{agent_name}"))
    }

    pub fn resolve(&self, reference: &str) -> Result<Self, String> {
        let reference = reference.trim();
        if reference.is_empty() {
            return Err("agent path must not be empty".to_string());
        }
        if reference == Self::ROOT {
            return Ok(Self::root());
        }
        if reference.starts_with('/') {
            return Self::try_from(reference);
        }
        for segment in reference.split('/') {
            validate_agent_name(segment)?;
        }
        Self::from_string(format!("{self}/{reference}"))
    }
}

impl TryFrom<String> for AgentPath {
    type Error = String;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::from_string(value)
    }
}

impl TryFrom<&str> for AgentPath {
    type Error = String;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        Self::from_string(value.to_string())
    }
}

impl From<AgentPath> for String {
    fn from(value: AgentPath) -> Self {
        value.0
    }
}

impl FromStr for AgentPath {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::try_from(value)
    }
}

impl AsRef<str> for AgentPath {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl Deref for AgentPath {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        self.as_str()
    }
}

impl fmt::Display for AgentPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

fn validate_absolute_path(path: &str) -> Result<(), String> {
    let Some(stripped) = path.strip_prefix('/') else {
        return Err("agent path must start with `/root`".to_string());
    };
    let mut segments = stripped.split('/');
    match segments.next() {
        Some("root") => {}
        _ => return Err("agent path must start with `/root`".to_string()),
    }
    if stripped.ends_with('/') {
        return Err("agent path must not end with `/`".to_string());
    }
    for segment in segments {
        validate_agent_name(segment)?;
    }
    Ok(())
}

fn validate_agent_name(agent_name: &str) -> Result<(), String> {
    if agent_name.is_empty() {
        return Err("agent_name must not be empty".to_string());
    }
    if agent_name == "root" || agent_name == "." || agent_name == ".." {
        return Err(format!("agent_name `{agent_name}` is reserved"));
    }
    if agent_name.contains('/') {
        return Err("agent_name must not contain `/`".to_string());
    }
    if !agent_name
        .chars()
        .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '_')
    {
        return Err(
            "agent_name must use only lowercase letters, digits, and underscores".to_string(),
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn validates_agent_paths() {
        let root = AgentPath::root();
        assert_eq!(root.join("worker").unwrap().as_str(), "/root/worker");
        assert_eq!(
            root.join("worker")
                .unwrap()
                .resolve("child")
                .unwrap()
                .as_str(),
            "/root/worker/child"
        );
        assert!(AgentPath::try_from("/nope").is_err());
        assert!(root.join("BadName").is_err());
        assert!(root.join("root").is_err());
    }
}
