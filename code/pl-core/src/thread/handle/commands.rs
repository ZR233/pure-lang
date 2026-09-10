//! Serialized owner commands and exhaustive rejection when the owner closes.
use super::*;

#[derive(Debug)]
pub(in crate::thread) enum Command {
    QueuedTurn(
        input::QueuedTurn,
        oneshot::Sender<Result<Option<TurnCompletion>, ThreadError>>,
    ),
    ContextPreparation(
        Option<context_preparation::ContextPreparer>,
        oneshot::Sender<Result<(), ThreadError>>,
    ),
    ReplaceModel(
        crate::model::ModelFactory,
        Option<context_preparation::ContextPreparer>,
        oneshot::Sender<Result<(), ThreadError>>,
    ),
    Reveal(Vec<String>, oneshot::Sender<Result<(), ThreadError>>),
    RequestInteraction(
        interactions::InteractionRequest,
        oneshot::Sender<Result<interactions::InteractionRecord, ThreadError>>,
    ),
    CancelInteraction(
        interactions::InteractionCancellation,
        oneshot::Sender<Result<interactions::InteractionRecord, ThreadError>>,
    ),
    ResolveInteraction(
        interactions::InteractionResolution,
        oneshot::Sender<Result<interactions::InteractionRecord, ThreadError>>,
    ),
    Application(
        extensions::ApplicationUpdate,
        oneshot::Sender<Result<ThreadSnapshot, ThreadError>>,
    ),
    Extensions(
        Vec<extensions::ExtensionMutation>,
        oneshot::Sender<
            Result<std::collections::BTreeMap<String, extensions::ExtensionRecord>, ThreadError>,
        >,
    ),
    Resources(
        crate::context::ResourceAccess,
        oneshot::Sender<Result<(), ThreadError>>,
    ),
    Retry {
        source: String,
        attempt: String,
        cancellation: CancellationToken,
        reply: oneshot::Sender<Result<ModelStepOutput, ThreadError>>,
    },
    AttachCold(
        cold::ColdStoreHandle,
        oneshot::Sender<Result<(), ThreadError>>,
    ),
    Flush(oneshot::Sender<Result<(), ThreadError>>),
    Turn(
        TurnInput,
        oneshot::Sender<Result<TurnCompletion, ThreadError>>,
    ),
    Capacity(ContextCapacity, oneshot::Sender<Result<(), ThreadError>>),
    PatchRuntimeFacts(
        Vec<RuntimeFact>,
        oneshot::Sender<Result<ContextSnapshot, ThreadError>>,
    ),
    UpdateFacts(
        Vec<RuntimeFact>,
        oneshot::Sender<Result<ContextSnapshot, ThreadError>>,
    ),
    ReplaceContext(
        ReplaceContext,
        oneshot::Sender<Result<ContextSnapshot, ThreadError>>,
    ),
    Execute(
        String,
        CancellationToken,
        oneshot::Sender<Result<ToolDispatch, ThreadError>>,
    ),
    Step(
        StepInput,
        oneshot::Sender<Result<ModelStepOutput, ThreadError>>,
    ),
    Close(oneshot::Sender<Result<(), ThreadError>>),
}

impl Command {
    pub(in crate::thread) fn reject(self) {
        fn closed<T>(reply: oneshot::Sender<Result<T, ThreadError>>) {
            let _ = reply.send(Err(ThreadError::Closed));
        }
        match self {
            Self::ReplaceModel(_, _, reply) => closed(reply),
            Self::ContextPreparation(_, reply) => closed(reply),
            Self::Reveal(_, reply) => closed(reply),
            Self::RequestInteraction(_, reply) => closed(reply),
            Self::ResolveInteraction(_, reply) => closed(reply),
            Self::CancelInteraction(_, reply) => closed(reply),
            Self::Application(_, reply) => closed(reply),
            Self::Extensions(_, reply) => closed(reply),
            Self::Resources(_, reply) => closed(reply),
            Self::Retry { reply, .. } => closed(reply),
            Self::AttachCold(_, reply) => closed(reply),
            Self::Turn(_, reply) => closed(reply),
            Self::QueuedTurn(_, reply) => closed(reply),
            Self::Capacity(_, reply) => closed(reply),
            Self::UpdateFacts(_, reply) | Self::PatchRuntimeFacts(_, reply) => closed(reply),
            Self::ReplaceContext(_, reply) => closed(reply),
            Self::Execute(_, _, reply) => closed(reply),
            Self::Step(_, reply) => closed(reply),
            Self::Flush(reply) | Self::Close(reply) => closed(reply),
        }
    }
}
