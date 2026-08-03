use std::path::Path;

use anyhow::Result;

use super::super::git::checked_git;
use super::super::verifier::{MergeVerifier, ProductionMergeVerifier};
use super::resolve::reject_conflict_markers;
use crate::studio::task_coordinator::{
    ConflictVerificationOutput, MergeVerificationRequest, RecordConflictVerification,
    TaskCoordinator,
};

impl TaskCoordinator {
    pub(crate) async fn verify_active_conflict(
        &self,
        session_id: &str,
        merge_id: &str,
    ) -> Result<ConflictVerificationOutput> {
        let guard = self.lock_branch_mutation().await;
        self.ensure_branch_mutation_guard(&guard)?;
        let (scope, unmerged) = self
            .load_active_conflict_scope(session_id, merge_id)
            .await?;
        let manifest = scope
            .merge
            .evidence
            .as_ref()
            .and_then(|evidence| evidence.conflict_manifest.as_ref())
            .ok_or_else(|| anyhow::anyhow!("conflicted merge has no durable manifest"))?;
        let workspace = Path::new(&scope.run.workspace_root);
        let mut diagnostics = unmerged
            .keys()
            .map(|path| format!("unresolved conflict: {path}"))
            .collect::<Vec<_>>();
        if unmerged.is_empty() {
            for conflict in manifest.conflicts.iter().filter(|entry| !entry.binary) {
                if !workspace.join(&conflict.path).exists() {
                    continue;
                }
                if let Err(error) = reject_conflict_markers(workspace, &conflict.path).await {
                    diagnostics.push(format!("{}: {error:#}", conflict.path));
                }
            }
        }
        let verification =
            if diagnostics.is_empty() {
                match ProductionMergeVerifier
                    .verify(MergeVerificationRequest {
                        workspace_root: scope.run.workspace_root.clone(),
                        changed_files: scope.delivery.changed_files.clone(),
                    })
                    .await
                {
                    Ok(steps) => {
                        diagnostics.extend(steps.iter().filter(|step| !step.success).map(|step| {
                            format!("verification failed: {}", step.command.join(" "))
                        }));
                        steps
                    }
                    Err(error) => {
                        diagnostics.push(format!("verification runner failed: {error:#}"));
                        Vec::new()
                    }
                }
            } else {
                Vec::new()
            };
        let success = diagnostics.is_empty();
        let index_tree = if success {
            Some(checked_git(workspace, vec!["write-tree".into()]).await?)
        } else {
            None
        };
        let record = self
            .store
            .record_conflict_verification(RecordConflictVerification {
                merge_id: scope.merge.id.clone(),
                expected_head: scope.run.expected_head.clone(),
                success,
                index_tree,
                steps: verification.clone(),
                diagnostic: (!diagnostics.is_empty()).then(|| diagnostics.join("; ")),
            })
            .await?;
        Ok(ConflictVerificationOutput {
            merge_id: scope.merge.id,
            attempt: record.attempt,
            success,
            diagnostics,
            verification,
            aborted: false,
        })
    }
}
