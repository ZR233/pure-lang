//! Instruction profiles, assembly, and model-context projection.

mod assembler;
mod profile;
mod snapshot;
mod types;

pub use assembler::InstructionAssembler;
pub use profile::InstructionProfile;
pub use types::{
    ExecutionInstructionProfile, InstructionAssemblyRequest, InstructionBlock, InstructionBundle,
    InstructionSnapshot, InstructionSource, InstructionSourceKind,
};

#[cfg(test)]
mod tests;
