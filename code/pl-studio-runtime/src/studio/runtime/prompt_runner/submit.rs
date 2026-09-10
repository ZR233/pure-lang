//! 输入提交入口：内容校验、持久资源准备与 core 的原子路由受理。

use anyhow::{Result, bail};

use crate::studio::ThreadRecord;

use super::super::{
    StudioRuntime, StudioSubmitPromptOptions, StudioSubmitPromptRequest, StudioSubmitPromptResponse,
};

impl StudioRuntime {
    /// Accepts input that requires a new Turn.
    pub async fn start_turn(
        &self,
        thread_id: String,
        request: pl_protocol::studio::StartTurnRequest,
    ) -> Result<StudioSubmitPromptResponse> {
        self.submit_prompt(StudioSubmitPromptRequest {
            thread_id,
            input: request.input,
            options: StudioSubmitPromptOptions {
                turn_policy: pl_core::thread::input::InputPolicy::StartOnly,
                ..StudioSubmitPromptOptions::default()
            },
        })
        .await
    }

    /// Accepts steering input for the currently running Turn.
    pub async fn steer_turn(
        &self,
        thread_id: String,
        request: pl_protocol::studio::SteerTurnRequest,
    ) -> Result<StudioSubmitPromptResponse> {
        self.submit_prompt(StudioSubmitPromptRequest {
            thread_id,
            input: request.input,
            options: StudioSubmitPromptOptions {
                turn_policy: pl_core::thread::input::InputPolicy::SteerOnly,
                ..StudioSubmitPromptOptions::default()
            },
        })
        .await
    }

    pub async fn submit_prompt(
        &self,
        request: StudioSubmitPromptRequest,
    ) -> Result<StudioSubmitPromptResponse> {
        validate_prompt_content(&request.input)?;
        // Serialize turn registration with the updater's final idle check.
        let _lifecycle_guard = self.lifecycle_lock.lock().await;
        self.submit_prompt_with_lifecycle_lock(request).await
    }

    pub(in crate::studio::runtime) async fn submit_prompt_with_lifecycle_lock(
        &self,
        request: StudioSubmitPromptRequest,
    ) -> Result<StudioSubmitPromptResponse> {
        let thread_record = self.read_owned_thread(&request.thread_id).await?;
        self.submit_prompt_for_owned_thread_with_lifecycle_lock(request, thread_record)
            .await
    }

    pub(in crate::studio::runtime) async fn submit_prompt_for_owned_thread_with_lifecycle_lock(
        &self,
        request: StudioSubmitPromptRequest,
        thread_record: ThreadRecord,
    ) -> Result<StudioSubmitPromptResponse> {
        let StudioSubmitPromptRequest {
            thread_id,
            input,
            options,
        } = request;
        validate_prompt_content(&input)?;
        anyhow::ensure!(
            thread_record.visibility == crate::studio::ThreadVisibility::Active,
            "archived Thread cannot accept input"
        );
        let pl_protocol::studio::StudioPromptInput {
            text: prompt,
            attachment_draft_ids,
        } = input;
        anyhow::ensure!(
            thread_record.id == thread_id,
            "prompt Thread does not match its canonical owner"
        );
        self.ensure_prompt_runtime_ready().await?;
        let drafts = self
            .attachment_drafts
            .resolve(&attachment_draft_ids)
            .await?;
        let (route, config) = self.model_binding(&thread_id).await?;
        self.attachment_drafts
            .validate_for_model(&route.model, &drafts)?;
        let attachments = self
            .store
            .promote_attachment_drafts(&thread_id, &drafts)
            .await?;
        let resources = crate::resource_store::FileResourceStore::new(
            self.store.attachments_dir().join("thread-resources"),
        );
        let mut context = Vec::new();
        if !prompt.is_empty() {
            context.push(pl_core::context::ContextContent::Text {
                text: prompt.clone().into(),
            });
        }
        for attachment in &attachments {
            let reference = resources
                .retain_file(
                    std::path::Path::new(&attachment.storage_path),
                    &attachment.media_type,
                )
                .await?;
            anyhow::ensure!(
                reference.byte_len() == attachment.byte_size
                    && reference.content_digest()
                        == format!("sha256:{}", attachment.content_sha256),
                "attachment changed while preparing input"
            );
            if let Some(filename) = &attachment.filename {
                context.push(pl_core::context::ContextContent::Text {
                    text: format!("Attachment: {filename}").into(),
                });
            }
            let modality = match attachment.modality {
                pl_protocol::studio::StudioAttachmentModality::Image => {
                    pl_protocol::AttachmentModality::Image
                }
                pl_protocol::studio::StudioAttachmentModality::Video => {
                    pl_protocol::AttachmentModality::Video
                }
                pl_protocol::studio::StudioAttachmentModality::File => {
                    pl_protocol::AttachmentModality::File
                }
            };
            context.push(pl_model::runtime::attachment_content(reference, modality)?);
        }
        #[derive(serde::Serialize)]
        #[serde(rename_all = "camelCase")]
        struct PromptPayload<'a> {
            text: &'a str,
            presentation: pl_protocol::MessagePresentation,
            attachments: &'a [crate::studio::AttachmentRecord],
        }
        let input_id = crate::studio::new_id("input");
        let payload = pl_core::context::OpaquePayload::new(
            "pl.studio.prompt",
            1,
            serde_json::to_string(&PromptPayload {
                text: &prompt,
                presentation: options.presentation,
                attachments: &attachments,
            })?,
        )?;
        let thread = self.ensure_thread_owner(&thread_id).await?;
        self.queue_thread_model(&thread, &route, &config).await?;
        if let Some(suggestions) =
            self.thread_factory
                .skill_suggestions(&thread_id, &prompt, &thread.snapshot())
        {
            context.push(pl_core::context::ContextContent::Text {
                text: suggestions.into(),
            });
        }
        // Resource bytes are retained before admission; failures never delete user drafts or replay effects.
        self.agent_facility
            .product_events
            .record_attachments(attachments.clone())?;
        let accepted = thread
            .submit_input_with_policy(pl_core::thread::input::InputSubmission {
                input: pl_core::thread::input::ThreadInput {
                    id: input_id,
                    payload,
                    context,
                },
                policy: options.turn_policy,
                drive: Some(pl_core::thread::input::InputDriverOptions {
                    max_model_steps: std::num::NonZeroU32::new(64)
                        .expect("fixed positive model step limit"),
                }),
            })
            .await?;
        self.attachment_drafts.commit(&attachment_draft_ids).await;
        self.residency.touch(&thread_id).await;
        Ok(StudioSubmitPromptResponse {
            thread_id,
            input_id: accepted.input.id,
            cursor: accepted.accepted_sequence,
        })
    }

    pub(in crate::studio::runtime) async fn ensure_prompt_runtime_ready(&self) -> Result<()> {
        if !self.runtime_snapshot().await?.state.is_ready() {
            bail!("Studio runtime is not ready");
        }
        Ok(())
    }
}

pub(in crate::studio::runtime) fn validate_prompt_content(
    input: &pl_protocol::studio::StudioPromptInput,
) -> Result<()> {
    if input.text.trim().is_empty() && input.attachment_draft_ids.is_empty() {
        bail!("prompt is empty");
    }
    Ok(())
}
