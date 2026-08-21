use serde::{Deserialize, Serialize};

use super::{DesignProgress, DesignWorkspaceObservation, TaskGitFingerprint};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DesignUpdatingState {
    generation: u64,
    design: DesignProgress,
    baseline: Box<TaskGitFingerprint>,
    latest_observation: Box<DesignWorkspaceObservation>,
}

impl DesignUpdatingState {
    pub(crate) fn new(baseline: TaskGitFingerprint) -> Self {
        Self {
            generation: 0,
            design: DesignProgress::Updating,
            latest_observation: Box::new(DesignWorkspaceObservation::baseline(baseline.clone())),
            baseline: Box::new(baseline),
        }
    }

    pub(crate) const fn generation(&self) -> u64 {
        self.generation
    }

    pub(crate) const fn design(&self) -> &DesignProgress {
        &self.design
    }

    #[cfg(test)]
    pub(crate) const fn baseline(&self) -> &TaskGitFingerprint {
        &self.baseline
    }

    pub(crate) const fn latest_observation(&self) -> &DesignWorkspaceObservation {
        &self.latest_observation
    }

    pub(crate) fn observe(
        mut self,
        observation: DesignWorkspaceObservation,
    ) -> anyhow::Result<Self> {
        let expected_sequence = self
            .latest_observation
            .sequence
            .checked_add(1)
            .ok_or_else(|| anyhow::anyhow!("design observation sequence overflow"))?;
        if observation.sequence != expected_sequence {
            anyhow::bail!(
                "design observation sequence must be {expected_sequence}, got {}",
                observation.sequence
            );
        }
        self.latest_observation = Box::new(observation);
        Ok(self)
    }
}
