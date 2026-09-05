use pl_core::AgentProgressStage;

pub(crate) const fn progress_stage_label(stage: AgentProgressStage) -> &'static str {
    match stage {
        AgentProgressStage::Exploring => "exploring",
        AgentProgressStage::Implementing => "implementing",
        AgentProgressStage::Verifying => "verifying",
        AgentProgressStage::Blocked => "blocked",
        AgentProgressStage::ReadyForCompletion => "readyForCompletion",
        AgentProgressStage::ReadyForReview => "readyForReview",
    }
}

pub(crate) fn progress_stage_from_label(label: &str) -> AgentProgressStage {
    match label {
        "exploring" => AgentProgressStage::Exploring,
        "implementing" => AgentProgressStage::Implementing,
        "verifying" => AgentProgressStage::Verifying,
        "blocked" => AgentProgressStage::Blocked,
        "readyForCompletion" => AgentProgressStage::ReadyForCompletion,
        "readyForReview" => AgentProgressStage::ReadyForReview,
        _ => AgentProgressStage::Exploring,
    }
}
