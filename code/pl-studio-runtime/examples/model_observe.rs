//! Explicit live observation using the selected installed provider, with no acceptance assertions.
use anyhow::{Context, Result};
use pl_model::{
    completion::{
        AttachmentInput, AttachmentSource, CompletionRequest, ContentPart, Message, MessageContent,
        MessageRole, ReasoningConfig,
    },
    config::ProviderConfig,
    runtime::{ModelInvocationContext, ModelRuntime, ModelSession},
};
use std::{collections::HashMap, path::PathBuf, time::Duration};

#[tokio::main]
async fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let provider_id = args.next().context("usage: model_observe PROVIDER MODEL TASK_FILE ARTIFACT_DIR [--image FILE | --image-url URL | --image-base64 FILE] [--followup TASK_FILE]")?;
    let model_slug = args.next().context("missing model")?;
    let mut tasks = vec![PathBuf::from(args.next().context("missing task file")?)];
    let artifacts = PathBuf::from(args.next().context("missing artifact directory")?);
    tokio::fs::create_dir(&artifacts)
        .await
        .context("use a new artifact directory for each observation")?;
    tokio::fs::write(
        artifacts.join("report.json"),
        br#"{"execution":"starting","review":"pending"}"#,
    )
    .await?;
    let result = observe(
        &provider_id,
        &model_slug,
        &mut tasks,
        args.collect(),
        &artifacts,
    )
    .await;
    if let Err(error) = &result {
        tokio::fs::write(
            artifacts.join("report.json"),
            serde_json::to_vec_pretty(&serde_json::json!({
                "execution":"error", "review":"pending", "detail":error.to_string()
            }))?,
        )
        .await?;
    }
    result
}

async fn observe(
    provider_id: &str,
    slug: &str,
    tasks: &mut Vec<PathBuf>,
    args: Vec<String>,
    artifacts: &std::path::Path,
) -> Result<()> {
    let mut attachments = Vec::new();
    let mut args = args.into_iter();
    while let Some(flag) = args.next() {
        let value = args.next().context("missing option value")?;
        let source = match flag.as_str() {
            "--followup" => {
                tasks.push(value.into());
                continue;
            }
            "--image" => AttachmentSource::Bytes {
                bytes: tokio::fs::read(&value).await?.into(),
            },
            "--image-url" => AttachmentSource::Url { url: value.clone() },
            "--image-base64" => AttachmentSource::Base64 {
                base64: tokio::fs::read_to_string(&value).await?.trim().into(),
            },
            _ => anyhow::bail!("unknown option {flag}"),
        };
        let media_type = match std::path::Path::new(&value)
            .extension()
            .and_then(|s| s.to_str())
        {
            Some("jpg" | "jpeg") => "image/jpeg",
            Some("gif") => "image/gif",
            Some("webp") => "image/webp",
            _ => "image/png",
        };
        attachments.push(AttachmentInput {
            attachment_id: format!("image-{}", attachments.len()),
            modality: pl_model::completion::AttachmentModality::Image,
            filename: None,
            media_type: media_type.into(),
            source,
        });
    }
    // Decode the selected provider only: unrelated obsolete configurations must not reset user data.
    let installed = pl_studio_runtime::ConfigStore::default_app()?;
    let config: toml::Value =
        toml::from_str(&tokio::fs::read_to_string(installed.paths().config_file()).await?)?;
    let selected = config
        .get("models")
        .and_then(|v| v.get("providers"))
        .and_then(|v| v.get(provider_id))
        .context("selected provider is not configured")?
        .clone();
    let mut provider: ProviderConfig = selected.try_into()?;
    let id = provider_id.to_owned();
    let stored = tokio::task::spawn_blocking(move || {
        match keyring::Entry::new("anywork", &format!("provider:{id}"))?.get_password() {
            Ok(secret) => Ok(Some(secret)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(error) => Err(error),
        }
    })
    .await??;
    provider.bearer_token = stored.or(provider.bearer_token);
    let model = provider
        .effective_models()?
        .into_iter()
        .find(|m| m.slug == slug)
        .context("model is not in the selected provider catalog")?;
    let effort = model
        .parameters
        .iter()
        .find(|p| p.name == "effort")
        .and_then(|p| p.candidates.first())
        .cloned();
    let runtime = ModelRuntime::new_with_provider_id(provider_id, provider.to_endpoint()?, model)?
        .with_pricing_mode(provider.pricing_mode);
    let session = ModelSession::default();
    let (tx, mut rx) = tokio::sync::broadcast::channel(4096);
    let events_path = artifacts.join("events.jsonl");
    let collector = tokio::spawn(async move {
        use tokio::io::AsyncWriteExt;
        let mut file = tokio::fs::File::create(events_path).await?;
        loop {
            match rx.recv().await {
                Ok(event) => {
                    if let pl_protocol::trace::AgentEvent::TracePartDelta { event } = &event
                        && let pl_protocol::trace::TraceDelta::Text { delta, .. } = &event.delta
                    {
                        use std::io::Write;
                        print!("{delta}");
                        std::io::stdout().flush()?;
                    }
                    let mut line = serde_json::to_vec(&event)?;
                    line.push(b'\n');
                    file.write_all(&line).await?;
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                Err(tokio::sync::broadcast::error::RecvError::Lagged(count)) => {
                    file.write_all(format!("{{\"observationGap\":{count}}}\n").as_bytes())
                        .await?;
                }
            }
        }
        file.flush().await?;
        anyhow::Ok(())
    });
    let execution = async {
        let mut messages = Vec::new();
        for (index, task) in tasks.iter().enumerate() {
            let text = tokio::fs::read_to_string(task).await?;
            let mut parts = vec![ContentPart::Text { text }];
            if index == 0 {
                parts.extend(attachments.iter().map(|a| ContentPart::Attachment {
                    attachment_id: a.attachment_id.clone(),
                    modality: a.modality,
                    media_type: a.media_type.clone(),
                    filename: a.filename.clone(),
                }));
            }
            messages.push(Message {
                presentation: Default::default(),
                role: MessageRole::User,
                content: MessageContent::new(parts),
                reasoning_content: None,
                tool_calls: None,
                tool_result: None,
                metadata: HashMap::new(),
            });
            let request = CompletionRequest::builder()
                .messages(messages.clone())
                .attachments(attachments.clone())
                .max_tokens(2048)
                .reasoning(Some(ReasoningConfig {
                    effort: effort.clone(),
                    summary: None,
                }))
                .build();
            println!("Observing {provider_id}/{slug}, turn {}", index + 1);
            let response = tokio::time::timeout(
                Duration::from_secs(180),
                runtime.complete(
                    request,
                    ModelInvocationContext::new(session.clone()).with_events(tx.clone()),
                ),
            )
            .await
            .context("observation timed out")??;
            tokio::fs::write(
                artifacts.join(format!("response-{index}.json")),
                serde_json::to_vec_pretty(&response)?,
            )
            .await?;
            println!("{}", response.content.as_deref().unwrap_or_default());
            messages.push(Message {
                presentation: Default::default(),
                role: MessageRole::Assistant,
                content: MessageContent::text(response.content.unwrap_or_default()),
                reasoning_content: response.reasoning_content,
                tool_calls: None,
                tool_result: None,
                metadata: HashMap::new(),
            });
        }
        anyhow::Ok(())
    }
    .await;
    drop(tx);
    let collected = collector.await?;
    let closed = session.close().await;
    execution?;
    collected?;
    closed?;
    tokio::fs::write(
        artifacts.join("report.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "provider":provider_id,"model":slug,"execution":"completed","review":"pending"
        }))?,
    )
    .await?;
    Ok(())
}
