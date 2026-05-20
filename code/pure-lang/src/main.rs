use pl_model::{ModelProvider, ProviderInfo, create_provider};

#[tokio::main]
async fn main() {
    let info = ProviderInfo::openai(None);
    println!("Provider: {}", info.name);
    println!("Wire API: {}", info.wire_api);

    let provider = create_provider(info).expect("failed to create provider");
    println!("Default model: {}", provider.default_model());

    let model_info = provider.model_info("gpt-4o");
    println!("Model: {} ({})", model_info.slug, model_info.display_name);
    println!(
        "Context window: {}",
        model_info
            .context_window
            .map(|w| w.to_string())
            .unwrap_or_else(|| "unknown".into())
    );
}
