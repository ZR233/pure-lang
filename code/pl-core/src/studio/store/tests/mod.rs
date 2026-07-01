use super::StudioStore;
use super::attachment::{
    MAX_BASE64_IMAGE_BYTES, MAX_IMAGE_SIDE, base64_encoded_len, normalize_image_attachment,
};
use crate::CompileMode;
use pl_protocol::{
    StudioEventEnvelope, StudioEventKind, StudioMessage, StudioMessageRole, StudioMessageStatus,
    StudioPart, StudioPartDelta, StudioPartDeltaField, StudioPartStatus, StudioPartType,
    StudioTextChannel, StudioToolPart,
};
use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};

mod attachment;
mod event_guards;
mod message_part_projection;
mod message_projection;
mod migration;
