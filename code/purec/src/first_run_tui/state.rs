use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use pl_core::{
    FirstRunConfigDraft, FirstRunModelDraft, FirstRunProviderDraft, InputModality,
    ModelCapabilityConfig, ModelConfig, ProviderTemplateKind, PureConfig, PureError, Result,
    TruncationMode, TruncationPolicyConfig,
};

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct FirstRunTuiState {
    pub(crate) draft: FirstRunConfigDraft,
    pub(crate) screen: Screen,
    pub(crate) selected_provider: usize,
    pub(crate) error: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Screen {
    Providers,
    ProviderEdit {
        field: ProviderField,
    },
    Models {
        selected_model: usize,
    },
    ModelEdit {
        model_index: usize,
        form: Box<ModelForm>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ProviderField {
    Key,
    Name,
    BaseUrl,
    ApiKey,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ModelField {
    Slug,
    DisplayName,
    Description,
    ContextWindow,
    MaxContextWindow,
    AutoCompactTokenLimit,
    DefaultTemperature,
    MaxOutputTokens,
    ReasoningEfforts,
    Capabilities,
    InputModalities,
    TruncationMode,
    TruncationLimit,
    BaseInstructions,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TuiCommand {
    None,
    Save,
    Cancel,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ModelForm {
    pub(crate) field: ModelField,
    pub(crate) slug: String,
    pub(crate) display_name: String,
    pub(crate) description: String,
    pub(crate) context_window: String,
    pub(crate) max_context_window: String,
    pub(crate) auto_compact_token_limit: String,
    pub(crate) default_temperature: String,
    pub(crate) max_output_tokens: String,
    pub(crate) reasoning_efforts: String,
    pub(crate) capabilities: String,
    pub(crate) input_modalities: String,
    pub(crate) truncation_mode: String,
    pub(crate) truncation_limit: String,
    pub(crate) base_instructions: String,
}

impl FirstRunTuiState {
    pub(crate) fn new() -> Self {
        Self {
            draft: FirstRunConfigDraft::new_default(),
            screen: Screen::Providers,
            selected_provider: 0,
            error: None,
        }
    }

    pub(crate) fn handle_key(&mut self, key: KeyEvent) -> TuiCommand {
        if key.modifiers.contains(KeyModifiers::CONTROL) {
            match key.code {
                KeyCode::Char('c') => return TuiCommand::Cancel,
                KeyCode::Char('s') => return TuiCommand::Save,
                _ => {}
            }
        }

        self.error = None;
        match self.screen.clone() {
            Screen::Providers => self.handle_provider_list_key(key),
            Screen::ProviderEdit { field } => self.handle_provider_edit_key(key, field),
            Screen::Models { selected_model } => self.handle_model_list_key(key, selected_model),
            Screen::ModelEdit { model_index, form } => {
                self.handle_model_edit_key(key, model_index, *form)
            }
        }
    }

    pub(crate) fn to_config(&self) -> Result<PureConfig> {
        self.draft.to_config()
    }

    pub(crate) fn prepare_save(&mut self) -> Result<()> {
        let Screen::ModelEdit { model_index, form } = self.screen.clone() else {
            return Ok(());
        };
        let config = form.to_model_config()?;
        if let Some(provider) = self.selected_provider_mut()
            && let Some(model) = provider.models.get_mut(model_index)
        {
            model.config = config;
        }
        self.screen = Screen::Models {
            selected_model: model_index + 1,
        };
        Ok(())
    }

    pub(crate) fn set_error(&mut self, error: impl Into<String>) {
        self.error = Some(error.into());
    }

    pub(crate) fn selected_provider(&self) -> Option<&FirstRunProviderDraft> {
        self.draft.providers.get(self.selected_provider)
    }

    pub(crate) fn selected_provider_mut(&mut self) -> Option<&mut FirstRunProviderDraft> {
        self.draft.providers.get_mut(self.selected_provider)
    }

    fn handle_provider_list_key(&mut self, key: KeyEvent) -> TuiCommand {
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => TuiCommand::Cancel,
            KeyCode::Char('s') => TuiCommand::Save,
            KeyCode::Up => {
                self.select_previous_provider();
                TuiCommand::None
            }
            KeyCode::Down => {
                self.select_next_provider();
                TuiCommand::None
            }
            KeyCode::Char('n') => {
                self.add_provider(ProviderTemplateKind::DeepSeek);
                TuiCommand::None
            }
            KeyCode::Char('o') => {
                self.add_provider(ProviderTemplateKind::OpenAi);
                TuiCommand::None
            }
            KeyCode::Char('e') | KeyCode::Enter => {
                self.screen = Screen::ProviderEdit {
                    field: ProviderField::Key,
                };
                TuiCommand::None
            }
            KeyCode::Char('m') => {
                self.screen = Screen::Models { selected_model: 0 };
                TuiCommand::None
            }
            KeyCode::Char('d') => {
                self.delete_selected_provider();
                TuiCommand::None
            }
            KeyCode::Char(' ') => {
                if let Some(provider) = self.selected_provider() {
                    self.draft.default_provider = provider.key.clone();
                }
                TuiCommand::None
            }
            _ => TuiCommand::None,
        }
    }

    fn handle_provider_edit_key(&mut self, key: KeyEvent, field: ProviderField) -> TuiCommand {
        match key.code {
            KeyCode::Esc | KeyCode::Enter => {
                self.screen = Screen::Providers;
                TuiCommand::None
            }
            KeyCode::Tab | KeyCode::Down => {
                self.screen = Screen::ProviderEdit {
                    field: field.next(),
                };
                TuiCommand::None
            }
            KeyCode::BackTab | KeyCode::Up => {
                self.screen = Screen::ProviderEdit {
                    field: field.previous(),
                };
                TuiCommand::None
            }
            KeyCode::Char('t') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.toggle_selected_provider_kind();
                TuiCommand::None
            }
            KeyCode::Char(ch) => {
                self.edit_provider_text(field, TextEdit::Push(ch));
                TuiCommand::None
            }
            KeyCode::Backspace => {
                self.edit_provider_text(field, TextEdit::Backspace);
                TuiCommand::None
            }
            KeyCode::Delete => {
                self.edit_provider_text(field, TextEdit::Clear);
                TuiCommand::None
            }
            _ => TuiCommand::None,
        }
    }

    fn handle_model_list_key(&mut self, key: KeyEvent, mut selected_model: usize) -> TuiCommand {
        let model_count = self.selected_provider_model_count();
        match key.code {
            KeyCode::Esc | KeyCode::Char('b') => {
                self.screen = Screen::Providers;
                TuiCommand::None
            }
            KeyCode::Up => {
                selected_model = selected_model.saturating_sub(1);
                self.screen = Screen::Models { selected_model };
                TuiCommand::None
            }
            KeyCode::Down => {
                selected_model = (selected_model + 1).min(model_count.saturating_sub(1));
                self.screen = Screen::Models { selected_model };
                TuiCommand::None
            }
            KeyCode::Char('a') => {
                let slug = self.suggest_model_slug();
                if let Some(provider) = self.selected_provider_mut() {
                    provider.models.push(FirstRunModelDraft::fallback(slug));
                    let model_index = provider.models.len() - 1;
                    let form = ModelForm::from_model(&provider.models[model_index].config);
                    self.screen = Screen::ModelEdit {
                        model_index,
                        form: Box::new(form),
                    };
                }
                TuiCommand::None
            }
            KeyCode::Char('e') | KeyCode::Enter => {
                if selected_model == 0 {
                    self.error = Some("模板默认模型不可编辑；可新增自定义模型覆盖需求".to_string());
                } else if let Some(provider) = self.selected_provider() {
                    let model_index = selected_model - 1;
                    if let Some(model) = provider.models.get(model_index) {
                        self.screen = Screen::ModelEdit {
                            model_index,
                            form: Box::new(ModelForm::from_model(&model.config)),
                        };
                    }
                }
                TuiCommand::None
            }
            KeyCode::Char('d') => {
                if selected_model == 0 {
                    self.error = Some("模板默认模型不可删除".to_string());
                } else if let Some(provider) = self.selected_provider_mut() {
                    let model_index = selected_model - 1;
                    if let Some(model) = provider.models.get(model_index)
                        && provider.default_model == model.config.slug
                    {
                        provider.default_model = provider
                            .template_model()
                            .map(|model| model.slug)
                            .unwrap_or_else(|_| provider.default_model.clone());
                    }
                    provider.models.remove(model_index);
                    let selected_model =
                        selected_model.min(self.selected_provider_model_count() - 1);
                    self.screen = Screen::Models { selected_model };
                }
                TuiCommand::None
            }
            KeyCode::Char(' ') => {
                if let Some(slug) = self.model_slug_for_list_index(selected_model)
                    && let Some(provider) = self.selected_provider_mut()
                {
                    provider.default_model = slug;
                }
                TuiCommand::None
            }
            _ => TuiCommand::None,
        }
    }

    fn handle_model_edit_key(
        &mut self,
        key: KeyEvent,
        model_index: usize,
        mut form: ModelForm,
    ) -> TuiCommand {
        match key.code {
            KeyCode::Esc => {
                self.screen = Screen::Models {
                    selected_model: model_index + 1,
                };
                TuiCommand::None
            }
            KeyCode::Enter => {
                match form.to_model_config() {
                    Ok(config) => {
                        if let Some(provider) = self.selected_provider_mut()
                            && let Some(model) = provider.models.get_mut(model_index)
                        {
                            model.config = config;
                        }
                        self.screen = Screen::Models {
                            selected_model: model_index + 1,
                        };
                    }
                    Err(error) => self.error = Some(error.to_string()),
                }
                TuiCommand::None
            }
            KeyCode::Tab | KeyCode::Down => {
                form.field = form.field.next();
                self.screen = Screen::ModelEdit {
                    model_index,
                    form: Box::new(form),
                };
                TuiCommand::None
            }
            KeyCode::BackTab | KeyCode::Up => {
                form.field = form.field.previous();
                self.screen = Screen::ModelEdit {
                    model_index,
                    form: Box::new(form),
                };
                TuiCommand::None
            }
            KeyCode::Char(ch) => {
                form.edit(TextEdit::Push(ch));
                self.screen = Screen::ModelEdit {
                    model_index,
                    form: Box::new(form),
                };
                TuiCommand::None
            }
            KeyCode::Backspace => {
                form.edit(TextEdit::Backspace);
                self.screen = Screen::ModelEdit {
                    model_index,
                    form: Box::new(form),
                };
                TuiCommand::None
            }
            KeyCode::Delete => {
                form.edit(TextEdit::Clear);
                self.screen = Screen::ModelEdit {
                    model_index,
                    form: Box::new(form),
                };
                TuiCommand::None
            }
            _ => {
                self.screen = Screen::ModelEdit {
                    model_index,
                    form: Box::new(form),
                };
                TuiCommand::None
            }
        }
    }

    fn add_provider(&mut self, kind: ProviderTemplateKind) {
        self.draft.add_provider(kind);
        self.selected_provider = self.draft.providers.len() - 1;
        self.screen = Screen::ProviderEdit {
            field: ProviderField::Key,
        };
    }

    fn delete_selected_provider(&mut self) {
        if self.draft.providers.len() <= 1 {
            self.error = Some("至少需要保留一个 provider".to_string());
            return;
        }
        let removed = self.draft.providers.remove(self.selected_provider);
        self.selected_provider = self
            .selected_provider
            .min(self.draft.providers.len().saturating_sub(1));
        if self.draft.default_provider == removed.key
            && let Some(provider) = self.draft.providers.first()
        {
            self.draft.default_provider = provider.key.clone();
        }
    }

    fn select_previous_provider(&mut self) {
        self.selected_provider = self.selected_provider.saturating_sub(1);
    }

    fn select_next_provider(&mut self) {
        self.selected_provider =
            (self.selected_provider + 1).min(self.draft.providers.len().saturating_sub(1));
    }

    fn toggle_selected_provider_kind(&mut self) {
        let Some(provider) = self.selected_provider() else {
            return;
        };
        let old_key = provider.key.clone();
        let was_default = self.draft.default_provider == old_key;
        let new_kind = match provider.kind {
            ProviderTemplateKind::DeepSeek => ProviderTemplateKind::OpenAi,
            ProviderTemplateKind::OpenAi => ProviderTemplateKind::DeepSeek,
        };
        let new_key = self.suggest_provider_key_excluding(new_kind, self.selected_provider);
        let replacement = FirstRunProviderDraft::from_template(new_key, new_kind);

        if let Some(provider) = self.selected_provider_mut() {
            provider.key = replacement.key;
            provider.kind = replacement.kind;
            provider.name = replacement.name;
            provider.base_url = replacement.base_url;
            provider.default_model = replacement.default_model;
            provider.models.clear();
        }
        if was_default && let Some(provider) = self.selected_provider() {
            self.draft.default_provider = provider.key.clone();
        }
    }

    fn suggest_provider_key_excluding(
        &self,
        kind: ProviderTemplateKind,
        excluded_index: usize,
    ) -> String {
        let prefix = kind.key_prefix();
        if !self
            .draft
            .providers
            .iter()
            .enumerate()
            .any(|(index, provider)| index != excluded_index && provider.key == prefix)
        {
            return prefix.to_string();
        }

        for index in 2.. {
            let candidate = format!("{prefix}-{index}");
            if !self
                .draft
                .providers
                .iter()
                .enumerate()
                .any(|(provider_index, provider)| {
                    provider_index != excluded_index && provider.key == candidate
                })
            {
                return candidate;
            }
        }

        unreachable!("unbounded provider key suggestion should always return")
    }

    fn edit_provider_text(&mut self, field: ProviderField, edit: TextEdit) {
        if let Some(provider) = self.selected_provider_mut() {
            match field {
                ProviderField::Key => edit.apply(&mut provider.key),
                ProviderField::Name => edit.apply(&mut provider.name),
                ProviderField::BaseUrl => {
                    let mut value = provider.base_url.clone().unwrap_or_default();
                    edit.apply(&mut value);
                    provider.base_url = (!value.is_empty()).then_some(value);
                }
                ProviderField::ApiKey => edit.apply(&mut provider.bearer_token),
            }
        }
    }

    fn selected_provider_model_count(&self) -> usize {
        self.selected_provider()
            .map(|provider| provider.models.len() + 1)
            .unwrap_or(1)
    }

    fn model_slug_for_list_index(&self, index: usize) -> Option<String> {
        let provider = self.selected_provider()?;
        if index == 0 {
            return provider.template_model().ok().map(|model| model.slug);
        }
        provider
            .models
            .get(index - 1)
            .map(|model| model.config.slug.clone())
    }

    fn suggest_model_slug(&self) -> String {
        let prefix = "custom-model";
        let Some(provider) = self.selected_provider() else {
            return prefix.to_string();
        };
        let existing = provider
            .all_models()
            .unwrap_or_default()
            .into_iter()
            .map(|model| model.slug)
            .collect::<Vec<_>>();
        if !existing.iter().any(|slug| slug == prefix) {
            return prefix.to_string();
        }
        for index in 2.. {
            let candidate = format!("{prefix}-{index}");
            if !existing.iter().any(|slug| slug == &candidate) {
                return candidate;
            }
        }
        unreachable!("unbounded model slug suggestion should always return")
    }
}

impl ProviderField {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Key => "provider key",
            Self::Name => "name",
            Self::BaseUrl => "base url",
            Self::ApiKey => "api key",
        }
    }

    fn next(self) -> Self {
        match self {
            Self::Key => Self::Name,
            Self::Name => Self::BaseUrl,
            Self::BaseUrl => Self::ApiKey,
            Self::ApiKey => Self::Key,
        }
    }

    fn previous(self) -> Self {
        match self {
            Self::Key => Self::ApiKey,
            Self::Name => Self::Key,
            Self::BaseUrl => Self::Name,
            Self::ApiKey => Self::BaseUrl,
        }
    }
}

impl ModelField {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Slug => "slug",
            Self::DisplayName => "display name",
            Self::Description => "description",
            Self::ContextWindow => "context window",
            Self::MaxContextWindow => "max context window",
            Self::AutoCompactTokenLimit => "auto compact token limit",
            Self::DefaultTemperature => "default temperature",
            Self::MaxOutputTokens => "max output tokens",
            Self::ReasoningEfforts => "reasoning efforts",
            Self::Capabilities => "capabilities",
            Self::InputModalities => "input modalities",
            Self::TruncationMode => "truncation mode",
            Self::TruncationLimit => "truncation limit",
            Self::BaseInstructions => "base instructions",
        }
    }

    fn next(self) -> Self {
        match self {
            Self::Slug => Self::DisplayName,
            Self::DisplayName => Self::Description,
            Self::Description => Self::ContextWindow,
            Self::ContextWindow => Self::MaxContextWindow,
            Self::MaxContextWindow => Self::AutoCompactTokenLimit,
            Self::AutoCompactTokenLimit => Self::DefaultTemperature,
            Self::DefaultTemperature => Self::MaxOutputTokens,
            Self::MaxOutputTokens => Self::ReasoningEfforts,
            Self::ReasoningEfforts => Self::Capabilities,
            Self::Capabilities => Self::InputModalities,
            Self::InputModalities => Self::TruncationMode,
            Self::TruncationMode => Self::TruncationLimit,
            Self::TruncationLimit => Self::BaseInstructions,
            Self::BaseInstructions => Self::Slug,
        }
    }

    fn previous(self) -> Self {
        match self {
            Self::Slug => Self::BaseInstructions,
            Self::DisplayName => Self::Slug,
            Self::Description => Self::DisplayName,
            Self::ContextWindow => Self::Description,
            Self::MaxContextWindow => Self::ContextWindow,
            Self::AutoCompactTokenLimit => Self::MaxContextWindow,
            Self::DefaultTemperature => Self::AutoCompactTokenLimit,
            Self::MaxOutputTokens => Self::DefaultTemperature,
            Self::ReasoningEfforts => Self::MaxOutputTokens,
            Self::Capabilities => Self::ReasoningEfforts,
            Self::InputModalities => Self::Capabilities,
            Self::TruncationMode => Self::InputModalities,
            Self::TruncationLimit => Self::TruncationMode,
            Self::BaseInstructions => Self::TruncationLimit,
        }
    }
}

impl ModelForm {
    fn from_model(model: &ModelConfig) -> Self {
        Self {
            field: ModelField::Slug,
            slug: model.slug.clone(),
            display_name: model.display_name.clone(),
            description: model.description.clone().unwrap_or_default(),
            context_window: option_to_string(model.context_window),
            max_context_window: option_to_string(model.max_context_window),
            auto_compact_token_limit: option_to_string(model.auto_compact_token_limit),
            default_temperature: option_to_string(model.default_temperature),
            max_output_tokens: option_to_string(model.max_output_tokens),
            reasoning_efforts: model.reasoning_efforts.join(","),
            capabilities: model
                .capabilities
                .iter()
                .map(|capability| match capability {
                    ModelCapabilityConfig::Streaming => "streaming",
                    ModelCapabilityConfig::FunctionCalling => "function_calling",
                    ModelCapabilityConfig::Vision => "vision",
                    ModelCapabilityConfig::ParallelToolCalls => "parallel_tool_calls",
                    ModelCapabilityConfig::Reasoning => "reasoning",
                    ModelCapabilityConfig::WebSearch => "web_search",
                })
                .collect::<Vec<_>>()
                .join(","),
            input_modalities: model
                .input_modalities
                .iter()
                .map(|modality| match modality {
                    InputModality::Text => "text",
                    InputModality::Image => "image",
                    InputModality::Audio => "audio",
                })
                .collect::<Vec<_>>()
                .join(","),
            truncation_mode: match model.truncation_policy.mode {
                TruncationMode::Bytes => "bytes",
                TruncationMode::Tokens => "tokens",
            }
            .to_string(),
            truncation_limit: model.truncation_policy.limit.to_string(),
            base_instructions: model.base_instructions.clone(),
        }
    }

    pub(crate) fn value_for_field(&self, field: ModelField) -> &str {
        match field {
            ModelField::Slug => &self.slug,
            ModelField::DisplayName => &self.display_name,
            ModelField::Description => &self.description,
            ModelField::ContextWindow => &self.context_window,
            ModelField::MaxContextWindow => &self.max_context_window,
            ModelField::AutoCompactTokenLimit => &self.auto_compact_token_limit,
            ModelField::DefaultTemperature => &self.default_temperature,
            ModelField::MaxOutputTokens => &self.max_output_tokens,
            ModelField::ReasoningEfforts => &self.reasoning_efforts,
            ModelField::Capabilities => &self.capabilities,
            ModelField::InputModalities => &self.input_modalities,
            ModelField::TruncationMode => &self.truncation_mode,
            ModelField::TruncationLimit => &self.truncation_limit,
            ModelField::BaseInstructions => &self.base_instructions,
        }
    }

    fn edit(&mut self, edit: TextEdit) {
        let value = match self.field {
            ModelField::Slug => &mut self.slug,
            ModelField::DisplayName => &mut self.display_name,
            ModelField::Description => &mut self.description,
            ModelField::ContextWindow => &mut self.context_window,
            ModelField::MaxContextWindow => &mut self.max_context_window,
            ModelField::AutoCompactTokenLimit => &mut self.auto_compact_token_limit,
            ModelField::DefaultTemperature => &mut self.default_temperature,
            ModelField::MaxOutputTokens => &mut self.max_output_tokens,
            ModelField::ReasoningEfforts => &mut self.reasoning_efforts,
            ModelField::Capabilities => &mut self.capabilities,
            ModelField::InputModalities => &mut self.input_modalities,
            ModelField::TruncationMode => &mut self.truncation_mode,
            ModelField::TruncationLimit => &mut self.truncation_limit,
            ModelField::BaseInstructions => &mut self.base_instructions,
        };
        edit.apply(value);
    }

    fn to_model_config(&self) -> Result<ModelConfig> {
        let slug = required(&self.slug, "model slug")?;
        let display_name = required(&self.display_name, "model display_name")?;
        let truncation_limit = parse_required_u64(&self.truncation_limit, "truncation limit")?;
        let reasoning_efforts = parse_string_list(&self.reasoning_efforts);
        if reasoning_efforts.is_empty() {
            return Err(PureError::ConfigError(
                "reasoning efforts must not be empty".to_string(),
            ));
        }
        let input_modalities = parse_modalities(&self.input_modalities)?;
        if input_modalities.is_empty() {
            return Err(PureError::ConfigError(
                "input modalities must not be empty".to_string(),
            ));
        }

        Ok(ModelConfig {
            slug,
            display_name,
            description: optional_string(&self.description),
            context_window: parse_optional_u64(&self.context_window, "context window")?,
            max_context_window: parse_optional_u64(&self.max_context_window, "max context window")?,
            auto_compact_token_limit: parse_optional_u64(
                &self.auto_compact_token_limit,
                "auto compact token limit",
            )?,
            default_temperature: parse_optional_f32(
                &self.default_temperature,
                "default temperature",
            )?,
            max_output_tokens: parse_optional_u64(&self.max_output_tokens, "max output tokens")?,
            reasoning_efforts,
            capabilities: parse_capabilities(&self.capabilities)?,
            input_modalities,
            truncation_policy: TruncationPolicyConfig {
                mode: parse_truncation_mode(&self.truncation_mode)?,
                limit: truncation_limit,
            },
            base_instructions: self.base_instructions.clone(),
        })
    }
}

#[derive(Debug, Clone, Copy)]
enum TextEdit {
    Push(char),
    Backspace,
    Clear,
}

impl TextEdit {
    fn apply(self, value: &mut String) {
        match self {
            Self::Push(ch) => value.push(ch),
            Self::Backspace => {
                value.pop();
            }
            Self::Clear => value.clear(),
        }
    }
}

fn option_to_string<T: ToString>(value: Option<T>) -> String {
    value.map(|value| value.to_string()).unwrap_or_default()
}

fn required(value: &str, name: &str) -> Result<String> {
    let value = value.trim();
    if value.is_empty() {
        return Err(PureError::ConfigError(format!("{name} must not be empty")));
    }
    Ok(value.to_string())
}

fn optional_string(value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_string())
}

fn parse_string_list(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

fn parse_optional_u64(value: &str, name: &str) -> Result<Option<u64>> {
    let value = value.trim();
    if value.is_empty() {
        return Ok(None);
    }
    value.parse::<u64>().map(Some).map_err(|error| {
        PureError::ConfigError(format!("{name} must be an unsigned integer: {error}"))
    })
}

fn parse_required_u64(value: &str, name: &str) -> Result<u64> {
    parse_optional_u64(value, name)?
        .ok_or_else(|| PureError::ConfigError(format!("{name} must be an unsigned integer")))
}

fn parse_optional_f32(value: &str, name: &str) -> Result<Option<f32>> {
    let value = value.trim();
    if value.is_empty() {
        return Ok(None);
    }
    value
        .parse::<f32>()
        .map(Some)
        .map_err(|error| PureError::ConfigError(format!("{name} must be a number: {error}")))
}

fn parse_capabilities(value: &str) -> Result<Vec<ModelCapabilityConfig>> {
    parse_string_list(value)
        .into_iter()
        .map(|item| match item.as_str() {
            "streaming" => Ok(ModelCapabilityConfig::Streaming),
            "function_calling" => Ok(ModelCapabilityConfig::FunctionCalling),
            "vision" => Ok(ModelCapabilityConfig::Vision),
            "parallel_tool_calls" => Ok(ModelCapabilityConfig::ParallelToolCalls),
            "reasoning" => Ok(ModelCapabilityConfig::Reasoning),
            "web_search" => Ok(ModelCapabilityConfig::WebSearch),
            _ => Err(PureError::ConfigError(format!(
                "unsupported capability: {item}"
            ))),
        })
        .collect()
}

fn parse_modalities(value: &str) -> Result<Vec<InputModality>> {
    parse_string_list(value)
        .into_iter()
        .map(|item| match item.as_str() {
            "text" => Ok(InputModality::Text),
            "image" => Ok(InputModality::Image),
            "audio" => Ok(InputModality::Audio),
            _ => Err(PureError::ConfigError(format!(
                "unsupported input modality: {item}"
            ))),
        })
        .collect()
}

fn parse_truncation_mode(value: &str) -> Result<TruncationMode> {
    match value.trim() {
        "bytes" => Ok(TruncationMode::Bytes),
        "tokens" => Ok(TruncationMode::Tokens),
        other => Err(PureError::ConfigError(format!(
            "unsupported truncation mode: {other}"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    #[test]
    fn adds_repeated_providers_with_unique_keys() {
        let mut state = FirstRunTuiState::new();

        state.handle_key(key(KeyCode::Char('n')));
        state.screen = Screen::Providers;
        state.handle_key(key(KeyCode::Char('o')));

        let keys = state
            .draft
            .providers
            .iter()
            .map(|provider| provider.key.as_str())
            .collect::<Vec<_>>();

        assert_eq!(keys, vec!["deepseek", "deepseek-2", "openai"]);
    }

    #[test]
    fn space_sets_default_provider() {
        let mut state = FirstRunTuiState::new();
        state.handle_key(key(KeyCode::Char('o')));
        state.screen = Screen::Providers;

        state.handle_key(key(KeyCode::Char(' ')));

        assert_eq!(state.draft.default_provider, "openai");
    }

    #[test]
    fn cancel_command_is_reported() {
        let mut state = FirstRunTuiState::new();

        let command = state.handle_key(key(KeyCode::Esc));

        assert_eq!(command, TuiCommand::Cancel);
    }

    #[test]
    fn save_command_is_reported() {
        let mut state = FirstRunTuiState::new();

        let command = state.handle_key(key(KeyCode::Char('s')));

        assert_eq!(command, TuiCommand::Save);
    }

    #[test]
    fn model_form_parses_full_model_config() {
        let model = FirstRunModelDraft::fallback("custom-model").config;
        let mut form = ModelForm::from_model(&model);
        form.context_window = "200000".to_string();
        form.max_output_tokens = "8192".to_string();
        form.reasoning_efforts = "low,high".to_string();
        form.capabilities = "streaming,reasoning".to_string();
        form.input_modalities = "text,image".to_string();
        form.truncation_mode = "tokens".to_string();
        form.truncation_limit = "12000".to_string();

        let parsed = form.to_model_config().unwrap();

        assert_eq!(parsed.context_window, Some(200000));
        assert_eq!(parsed.max_output_tokens, Some(8192));
        assert_eq!(
            parsed.reasoning_efforts,
            vec!["low".to_string(), "high".to_string()]
        );
        assert_eq!(parsed.truncation_policy.limit, 12000);
        assert_eq!(
            parsed.input_modalities,
            vec![InputModality::Text, InputModality::Image]
        );
    }

    #[test]
    fn prepare_save_applies_active_model_form() {
        let mut state = FirstRunTuiState::new();
        state.screen = Screen::Models { selected_model: 0 };
        state.handle_key(key(KeyCode::Char('a')));
        let Screen::ModelEdit {
            model_index,
            mut form,
        } = state.screen.clone()
        else {
            panic!("expected model edit screen");
        };
        form.display_name = "Edited Model".to_string();
        state.screen = Screen::ModelEdit { model_index, form };

        state.prepare_save().unwrap();

        assert_eq!(
            state.draft.providers[0].models[model_index]
                .config
                .display_name,
            "Edited Model"
        );
    }
}
