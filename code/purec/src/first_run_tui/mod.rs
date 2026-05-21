mod state;

use std::io;
use std::time::Duration;

use crossterm::cursor::Show;
use crossterm::event::{self, Event, KeyEventKind};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use pl_core::{ConfigStore, FirstRunProviderDraft, ModelConfig, PureConfig, Result};
use ratatui::Frame;
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};

use self::state::{FirstRunTuiState, ModelField, ModelForm, ProviderField, Screen, TuiCommand};

type TuiTerminal = Terminal<CrosstermBackend<io::Stdout>>;

pub(crate) fn run(store: &ConfigStore) -> Result<Option<PureConfig>> {
    let mut terminal = init_terminal()?;
    let _guard = TerminalRestoreGuard;
    let mut state = FirstRunTuiState::new();

    loop {
        terminal.draw(|frame| render(frame, &state))?;

        if !event::poll(Duration::from_millis(250))? {
            continue;
        }

        let Event::Key(key) = event::read()? else {
            continue;
        };
        if key.kind == KeyEventKind::Release {
            continue;
        }

        match state.handle_key(key) {
            TuiCommand::None => {}
            TuiCommand::Cancel => return Ok(None),
            TuiCommand::Save => match state
                .prepare_save()
                .and_then(|_| state.to_config())
                .and_then(|config| {
                    store.save(&config)?;
                    Ok(config)
                }) {
                Ok(config) => return Ok(Some(config)),
                Err(error) => state.set_error(error.to_string()),
            },
        }
    }
}

fn init_terminal() -> io::Result<TuiTerminal> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let mut terminal = Terminal::new(CrosstermBackend::new(stdout))?;
    terminal.clear()?;
    Ok(terminal)
}

struct TerminalRestoreGuard;

impl Drop for TerminalRestoreGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let mut stdout = io::stdout();
        let _ = execute!(stdout, LeaveAlternateScreen, Show);
    }
}

fn render(frame: &mut Frame<'_>, state: &FirstRunTuiState) {
    let area = frame.area();
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(8),
            Constraint::Length(4),
        ])
        .split(area);

    render_header(frame, chunks[0], state);
    match &state.screen {
        Screen::Providers => render_provider_list(frame, chunks[1], state),
        Screen::ProviderEdit { field } => render_provider_edit(frame, chunks[1], state, *field),
        Screen::Models { selected_model } => {
            render_model_list(frame, chunks[1], state, *selected_model);
        }
        Screen::ModelEdit { form, .. } => render_model_edit(frame, chunks[1], form),
    }
    render_footer(frame, chunks[2], state);
}

fn render_header(frame: &mut Frame<'_>, area: Rect, state: &FirstRunTuiState) {
    let provider_count = state.draft.providers.len();
    let default_provider = state.draft.default_provider.as_str();
    let text = vec![
        Line::from(vec![Span::styled(
            "purec first run config",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )]),
        Line::from(format!(
            "providers: {provider_count}  default: {default_provider}"
        )),
    ];
    frame.render_widget(
        Paragraph::new(text).block(Block::default().borders(Borders::BOTTOM)),
        area,
    );
}

fn render_provider_list(frame: &mut Frame<'_>, area: Rect, state: &FirstRunTuiState) {
    let [list_area, detail_area] =
        Layout::horizontal([Constraint::Percentage(46), Constraint::Percentage(54)]).areas(area);

    let mut lines = Vec::new();
    for (index, provider) in state.draft.providers.iter().enumerate() {
        let selected = index == state.selected_provider;
        let default_marker = if state.draft.default_provider == provider.key {
            "*"
        } else {
            " "
        };
        let marker = if selected { ">" } else { " " };
        let line = format!(
            "{marker}{default_marker} {} ({})",
            provider.key,
            provider.kind.display_name()
        );
        lines.push(styled_line(line, selected));
    }

    frame.render_widget(
        Paragraph::new(lines)
            .block(Block::default().title("Providers").borders(Borders::ALL))
            .wrap(Wrap { trim: false }),
        list_area,
    );

    let detail = state
        .selected_provider()
        .map(provider_detail_lines)
        .unwrap_or_else(|| vec![Line::from("No provider")]);
    frame.render_widget(
        Paragraph::new(detail)
            .block(Block::default().title("Details").borders(Borders::ALL))
            .wrap(Wrap { trim: false }),
        detail_area,
    );
}

fn provider_detail_lines(provider: &FirstRunProviderDraft) -> Vec<Line<'static>> {
    vec![
        Line::from(format!("key: {}", provider.key)),
        Line::from(format!("kind: {}", provider.kind.display_name())),
        Line::from(format!("name: {}", provider.name)),
        Line::from(format!(
            "base_url: {}",
            provider.base_url.as_deref().unwrap_or("")
        )),
        Line::from(format!("api_key: {}", mask_secret(&provider.bearer_token))),
        Line::from(format!("default_model: {}", provider.default_model)),
        Line::from(format!("custom_models: {}", provider.models.len())),
    ]
}

fn render_provider_edit(
    frame: &mut Frame<'_>,
    area: Rect,
    state: &FirstRunTuiState,
    active: ProviderField,
) {
    let Some(provider) = state.selected_provider() else {
        return;
    };
    let lines = vec![
        provider_field_line(ProviderField::Key, active, &provider.key),
        provider_field_line(ProviderField::Name, active, &provider.name),
        provider_field_line(
            ProviderField::BaseUrl,
            active,
            provider.base_url.as_deref().unwrap_or(""),
        ),
        provider_field_line(
            ProviderField::ApiKey,
            active,
            &mask_secret(&provider.bearer_token),
        ),
        Line::from(""),
        Line::from(format!(
            "kind: {}  default_model: {}",
            provider.kind.display_name(),
            provider.default_model
        )),
        Line::from("Press Ctrl+T to toggle DeepSeek/OpenAI template."),
    ];
    frame.render_widget(
        Paragraph::new(lines)
            .block(
                Block::default()
                    .title("Edit Provider")
                    .borders(Borders::ALL),
            )
            .wrap(Wrap { trim: false }),
        area,
    );
}

fn provider_field_line(field: ProviderField, active: ProviderField, value: &str) -> Line<'static> {
    field_line(field.label(), field == active, value)
}

fn render_model_list(
    frame: &mut Frame<'_>,
    area: Rect,
    state: &FirstRunTuiState,
    selected_model: usize,
) {
    let Some(provider) = state.selected_provider() else {
        return;
    };
    let models = provider.all_models().unwrap_or_default();
    let mut lines = Vec::new();
    for (index, model) in models.iter().enumerate() {
        let selected = index == selected_model;
        let default_marker = if provider.default_model == model.slug {
            "*"
        } else {
            " "
        };
        let template_marker = if index == 0 { "template" } else { "custom" };
        let marker = if selected { ">" } else { " " };
        let line = format!(
            "{marker}{default_marker} {} ({template_marker})",
            model.slug
        );
        lines.push(styled_line(line, selected));
    }

    frame.render_widget(
        Paragraph::new(lines)
            .block(Block::default().title("Models").borders(Borders::ALL))
            .wrap(Wrap { trim: false }),
        area,
    );
}

fn render_model_edit(frame: &mut Frame<'_>, area: Rect, form: &ModelForm) {
    let fields = [
        ModelField::Slug,
        ModelField::DisplayName,
        ModelField::Description,
        ModelField::ContextWindow,
        ModelField::MaxContextWindow,
        ModelField::AutoCompactTokenLimit,
        ModelField::DefaultTemperature,
        ModelField::MaxOutputTokens,
        ModelField::ReasoningEfforts,
        ModelField::Capabilities,
        ModelField::InputModalities,
        ModelField::TruncationMode,
        ModelField::TruncationLimit,
        ModelField::BaseInstructions,
    ];
    let lines = fields
        .into_iter()
        .map(|field| {
            field_line(
                field.label(),
                form.field == field,
                form.value_for_field(field),
            )
        })
        .collect::<Vec<_>>();
    frame.render_widget(
        Paragraph::new(lines)
            .block(
                Block::default()
                    .title("Edit Custom Model")
                    .borders(Borders::ALL),
            )
            .wrap(Wrap { trim: false }),
        area,
    );
}

fn render_footer(frame: &mut Frame<'_>, area: Rect, state: &FirstRunTuiState) {
    let help = match state.screen {
        Screen::Providers => {
            "Up/Down select  n add DeepSeek  o add OpenAI  e edit  m models  Space default  d delete  s save  q cancel"
        }
        Screen::ProviderEdit { .. } => {
            "Type to edit  Tab/Up/Down move field  Ctrl+T template  Enter/Esc back  Ctrl+S save"
        }
        Screen::Models { .. } => {
            "Up/Down select  a add custom  e edit custom  Space default model  d delete custom  b back  Ctrl+S save"
        }
        Screen::ModelEdit { .. } => {
            "Type to edit  Tab/Up/Down move field  Enter apply  Esc discard  Ctrl+S save"
        }
    };
    let mut lines = vec![Line::from(help)];
    if let Some(error) = &state.error {
        lines.push(Line::from(Span::styled(
            error.clone(),
            Style::default().fg(Color::Red),
        )));
    } else {
        lines.push(Line::from(
            "API keys are saved as plaintext bearer_token in ~/.pure/config.toml.",
        ));
    }
    frame.render_widget(
        Paragraph::new(lines)
            .block(Block::default().borders(Borders::TOP))
            .wrap(Wrap { trim: false }),
        area,
    );
}

fn field_line(label: &str, active: bool, value: &str) -> Line<'static> {
    let prefix = if active { "> " } else { "  " };
    styled_line(format!("{prefix}{label}: {value}"), active)
}

fn styled_line(text: String, active: bool) -> Line<'static> {
    if active {
        Line::from(Span::styled(
            text,
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ))
    } else {
        Line::from(text)
    }
}

fn mask_secret(value: &str) -> String {
    if value.is_empty() {
        return String::new();
    }
    "*".repeat(value.chars().count().min(24))
}

#[allow(dead_code)]
fn model_summary(model: &ModelConfig) -> String {
    format!("{} ({})", model.slug, model.display_name)
}
