//! Instruction profiles, assembly, and model-context projection.

mod assembler;
mod profile;
mod snapshot;
mod types;

pub use assembler::InstructionAssembler;
pub use profile::InstructionProfile;
pub use types::{
    ExecutionInstructionProfile, InstructionAssemblyRequest, InstructionBlock, InstructionSnapshot,
    InstructionSource, InstructionSourceKind, SkillSuggestionRequest,
};

mod bundle;
pub use bundle::InstructionBundle;
