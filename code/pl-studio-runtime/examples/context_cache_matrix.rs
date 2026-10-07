//! 与旧源码共同编译的真实缓存对照入口。只读取配置与既有凭据环境变量。
use anyhow::{Context, Result};
use pl_core::{
    context::{ContextContent, ContextRecord, ContextSource},
    model::{Model, ModelRequest, ToolCallMode},
    thread::{
        ContextReplacementReason, ModelStepLimit, ReplaceContext, RuntimeFact, ThreadHandle,
        TurnInput,
    },
};
use pl_model::{
    completion::ReasoningConfig,
    config::ProviderConfig,
    runtime::{ModelRuntime, ThreadCompactionOptions, ThreadCompactionStrategy, ThreadModel},
};
use serde_json::json;
use std::{path::PathBuf, sync::Arc};

fn text(value: impl Into<String>) -> ContextContent {
    ContextContent::Text {
        text: Arc::from(value.into()),
    }
}

fn instruction(revision: u64, append: bool, previous: Option<String>) -> Result<ContextRecord> {
    let content = format!(
        "只输出当前阶段名称及 INSTRUCTION_V{revision}，不调用工具，不请求批准。{}",
        "\n固定指令：依据最新宿主状态回答，压缩和重启不撤销批准。".repeat(256)
    );
    let hash = pl_core::context::content_hash(content.as_bytes());
    let source = if append {
        serde_json::from_value(
            json!({"kind":"instructionSnapshot","revision":revision,"contentHash":hash,"replaces":previous}),
        )?
    } else {
        ContextSource::Instruction
    };
    Ok(ContextRecord {
        id: format!("instructions:{revision}"),
        turn_id: None,
        source,
        content: vec![text(content)],
        tool_calls: vec![],
    })
}

#[tokio::main]
async fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let provider_id = args.next().context("provider required")?;
    let slug = args.next().context("model required")?;
    let output = PathBuf::from(args.next().context("new output required")?);
    tokio::fs::create_dir(&output).await?;
    let home =
        PathBuf::from(std::env::var("ANYWORK_HOME").context("isolated ANYWORK_HOME required")?);
    let config: toml::Value =
        toml::from_str(&tokio::fs::read_to_string(home.join("config.toml")).await?)?;
    let provider: ProviderConfig = config["models"]["providers"][&provider_id]
        .clone()
        .try_into()?;
    let model = provider
        .effective_models()?
        .into_iter()
        .find(|model| model.slug == slug)
        .context("model missing")?;
    let effort = model
        .parameters
        .iter()
        .find(|parameter| parameter.name == "effort")
        .and_then(|parameter| {
            parameter
                .candidates
                .iter()
                .find(|candidate| candidate.as_str() == "low")
                .or_else(|| parameter.candidates.first())
        })
        .cloned();
    let endpoint = provider.to_endpoint()?;
    let capabilities = serde_json::to_value(&model.capabilities)?;
    let endpoint_capabilities = serde_json::to_value(&endpoint.service_capabilities)?;
    let append = capabilities["instructionSnapshotOverrides"] == true
        && matches!(
            endpoint_capabilities["historyInstructionRole"].as_str(),
            Some("system" | "developer")
        );
    let protocol = model.binding.transport.clone();
    let factory = ThreadModel::new(
        ModelRuntime::new_with_provider_id(&provider_id, endpoint, model)?
            .with_pricing_mode(provider.pricing_mode),
        Some(ReasoningConfig {
            effort: effort.clone(),
            summary: None,
        }),
    );
    let thread = ThreadHandle::start("cache-matrix".into(), factory.open_session().await?).unwrap();
    let first = instruction(1, append, None)?;
    thread
        .replace_context(ReplaceContext {
            expected_revision: 0,
            reason: ContextReplacementReason::Rebuild,
            records: vec![first.clone()],
        })
        .await?;
    let stable = (0..512)
        .map(|line| format!("固定合成材料 {line}：此材料仅用于上下文缓存验收。\n"))
        .collect::<String>();
    let execution = async {
        for (index, phase) in ["warmup", "stable", "stateUpdate", "stableAfterStateUpdate", "instructionUpdate", "stableAfterInstructionUpdate", "afterSummary"].iter().enumerate() {
            if index == 4 {
                let snapshot = thread.snapshot();
                let previous = Some(pl_core::context::content_hash(first.content.iter().filter_map(|content| match content { ContextContent::Text { text } => Some(text.as_ref()), _ => None }).collect::<Vec<_>>().join("\n").as_bytes()));
                let current = instruction(2, append, previous)?;
                let mut records = snapshot.context.records.to_vec();
                if append { records.push(current); } else { records.retain(|record| record.source != ContextSource::Instruction); records.insert(0, current); }
                thread.replace_context(ReplaceContext { expected_revision: snapshot.context.revision, reason: ContextReplacementReason::Rebuild, records }).await?;
            }
            let state = if index < 2 { "已批准，当前阶段：实施" } else { "已批准，当前阶段：验证" };
            thread.update_facts(vec![
                RuntimeFact { source_id: "observation.material".into(), content: vec![text(stable.clone())] },
                RuntimeFact { source_id: "observation.state".into(), content: vec![text(state)] },
            ]).await?;
            if index == 6 {
                let snapshot = thread.snapshot();
                let compacted = factory.compact(ModelRequest {
                    thread_id: "cache-matrix".into(), turn_id: "auxiliary-summary".into(), attempt_id: "summary-matrix".into(),
                    context: snapshot.context.clone(), tools: Default::default(), tool_call_mode: ToolCallMode::Sequential,
                    solo_tool_ids: Default::default(), committed_private_context: snapshot.private_context.clone(), resources: None,
                    cancellation: Default::default(), progress: None,
                }, ThreadCompactionOptions { strategy: ThreadCompactionStrategy::TextSummary, instructions: "只生成简洁交接摘要，保留已批准状态、当前验证阶段和当前指令版本。禁止工具调用。".into(), requirement: "生成摘要。".into(), summary_prefix: "此前历史的压缩摘要：".into(), max_output_tokens: Some(256) }).await?;
                tokio::fs::write(output.join("auxiliary-summary.json"), serde_json::to_vec_pretty(&json!({"accounting":compacted.accounting,"binding":compacted.binding,"modelObservation":compacted.model_observation}))?).await?;
                thread.replace_context(compacted.replacement).await?;
            }
            let turn_id = format!("matrix-{index}");
            let result = tokio::time::timeout(std::time::Duration::from_secs(240), thread.run_turn(TurnInput {
                turn_id: turn_id.clone(), attempt_prefix: turn_id, content: vec![text("依据当前有效指令和最新宿主状态回答。")],
                max_model_steps: ModelStepLimit::Limited(1.try_into().unwrap()), cancellation: Default::default(),
            })).await;
            tokio::fs::write(output.join(format!("{index}-{phase}.json")), serde_json::to_vec_pretty(&json!({"phase":phase,"result":format!("{result:?}"),"snapshot":thread.snapshot()}))?).await?;
            result.context("model timed out")??;
        }
        anyhow::Ok(())
    }.await;
    let close = thread.close().await;
    tokio::fs::write(output.join("report.json"), serde_json::to_vec_pretty(&json!({"provider":provider_id,"model":slug,"protocol":protocol,"effort":effort,"appendInstructions":append,"execution":execution.as_ref().err().map(ToString::to_string),"close":format!("{close:?}"),"manualReview":"pending"}))?).await?;
    execution?;
    close?;
    Ok(())
}
