use std::path::Path;
use std::path::PathBuf;

use clap::CommandFactory;
use clap::Parser;
use clap::Subcommand;
use pl_core::{
    CompileMode, ConfigStore, CoreSession, ModelRole, PureConfig, PureCore, PureError, TurnRequest,
};

mod first_run_tui;

#[tokio::main]
async fn main() -> pl_core::Result<()> {
    let PurecArgs {
        command,
        plan,
        auto,
        workspace,
        prompt,
    } = PurecArgs::parse();
    if command.is_none() && prompt.is_empty() {
        return run_without_args();
    }

    match command {
        Some(command) => run_command(command),
        None => {
            run_prompt(PurecArgs {
                command: None,
                plan,
                auto,
                workspace,
                prompt,
            })
            .await
        }
    }
}

fn run_without_args() -> pl_core::Result<()> {
    let store = ConfigStore::default_app()?;
    if should_run_first_run_tui(&None, store.config_exists()) {
        let _ = first_run_tui::run(&store)?;
    } else {
        PurecArgs::command().print_help()?;
        println!();
    }
    Ok(())
}

async fn run_prompt(args: PurecArgs) -> pl_core::Result<()> {
    ensure_prompt(&args)?;
    let mut request = TurnRequest::new(args.prompt_text(), args.compile_mode());

    if let Some(workspace_dir) = &args.workspace {
        let instructions = load_workspace_instructions(workspace_dir)?;
        request = request.with_workspace_instructions(instructions);
    }

    let store = ConfigStore::default_app()?;
    let Some(config) = load_prompt_config(&args, &store)? else {
        return Ok(());
    };
    let core = PureCore::from_config(&config, ModelRole::Planner)?;
    let mut session = CoreSession::new();
    let (event_tx, _event_rx) = tokio::sync::broadcast::channel(256);
    let result = core.run_turn(&mut session, request, event_tx).await?;

    if !result.content.trim().is_empty() {
        println!("{}", result.content.trim_end());
    }

    Ok(())
}

fn load_prompt_config(
    args: &PurecArgs,
    store: &ConfigStore,
) -> pl_core::Result<Option<PureConfig>> {
    if !should_run_first_run_tui(&args.command, store.config_exists()) {
        return store.load().map(Some);
    }

    match first_run_tui::run(store)? {
        Some(_) => store.load().map(Some),
        None => Ok(None),
    }
}

fn run_command(command: PurecCommand) -> pl_core::Result<()> {
    match command {
        PurecCommand::Config { command } => run_config_command(command),
    }
}

fn run_config_command(command: ConfigCommand) -> pl_core::Result<()> {
    let store = ConfigStore::default_app()?;
    match command {
        ConfigCommand::Path => {
            println!("{}", store.paths().config_file().display());
        }
        ConfigCommand::Show => {
            let config = store.load_or_default()?;
            print!("{}", config.to_toml_pretty()?);
        }
        ConfigCommand::Init => {
            store.init_default()?;
            println!("{}", store.paths().config_file().display());
        }
    }
    Ok(())
}

#[derive(Debug, Parser)]
#[command(name = "purec", about = "Pure-Lang 命令行编译器前端", version)]
struct PurecArgs {
    #[command(subcommand)]
    command: Option<PurecCommand>,

    #[arg(
        long,
        conflicts_with = "auto",
        help = "只生成编译计划，不执行命令或修改文件"
    )]
    plan: bool,

    #[arg(long, help = "生成自动执行导向的编译方案，但当前版本仍不会执行命令")]
    auto: bool,

    #[arg(
        short,
        long,
        value_name = "DIR",
        help = "工作区目录，读取 DIR/Agents.md 作为项目记忆"
    )]
    workspace: Option<PathBuf>,

    #[arg(value_name = "PROMPT", num_args = 1..)]
    prompt: Vec<String>,
}

impl PurecArgs {
    fn compile_mode(&self) -> CompileMode {
        match (self.plan, self.auto) {
            (_, true) => CompileMode::Auto,
            (true, false) | (false, false) => CompileMode::Plan,
        }
    }

    fn prompt_text(&self) -> String {
        if self.prompt.is_empty() {
            return String::new();
        }
        self.prompt.join(" ")
    }
}

#[derive(Debug, Subcommand)]
enum PurecCommand {
    Config {
        #[command(subcommand)]
        command: ConfigCommand,
    },
}

#[derive(Debug, Subcommand)]
enum ConfigCommand {
    Path,
    Show,
    Init,
}

fn ensure_prompt(args: &PurecArgs) -> pl_core::Result<()> {
    if args.prompt.is_empty() {
        return Err(PureError::ConfigError("prompt is required".to_string()));
    }
    Ok(())
}

fn load_workspace_instructions(workspace_dir: &Path) -> pl_core::Result<String> {
    let agents_file = workspace_dir.join("Agents.md");
    match std::fs::read_to_string(&agents_file) {
        Ok(content) => Ok(content),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            // 区分"目录不存在"和"目录存在但无 Agents.md"
            if workspace_dir.is_dir() {
                Ok(String::new())
            } else {
                Err(PureError::ConfigError(format!(
                    "workspace directory not found: {}",
                    workspace_dir.display()
                )))
            }
        }
        Err(e) => Err(PureError::ConfigError(format!(
            "failed to read workspace instructions: {e}"
        ))),
    }
}

fn should_run_first_run_tui(command: &Option<PurecCommand>, config_exists: bool) -> bool {
    command.is_none() && !config_exists
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> PurecArgs {
        PurecArgs::try_parse_from(args).unwrap()
    }

    #[test]
    fn parses_no_args_for_top_level_help_or_first_run() {
        let args = parse(&["purec"]);

        assert!(args.command.is_none());
        assert!(args.prompt.is_empty());
    }

    #[test]
    fn parses_plain_prompt_as_plan_mode() {
        let args = parse(&["purec", "创建 HTTP 服务器"]);

        assert_eq!(args.prompt_text(), "创建 HTTP 服务器");
        assert_eq!(args.compile_mode(), CompileMode::Plan);
    }

    #[test]
    fn parses_plan_flag() {
        let args = parse(&["purec", "--plan", "创建 HTTP 服务器"]);

        assert_eq!(args.compile_mode(), CompileMode::Plan);
    }

    #[test]
    fn parses_auto_flag() {
        let args = parse(&["purec", "--auto", "创建 HTTP 服务器"]);

        assert_eq!(args.compile_mode(), CompileMode::Auto);
    }

    #[test]
    fn rejects_plan_and_auto_together() {
        let result = PurecArgs::try_parse_from(["purec", "--plan", "--auto", "创建 HTTP 服务器"]);

        assert!(result.is_err());
    }

    #[test]
    fn parses_config_path_subcommand() {
        let args = parse(&["purec", "config", "path"]);

        assert!(matches!(
            args.command,
            Some(PurecCommand::Config {
                command: ConfigCommand::Path
            })
        ));
    }

    #[test]
    fn parses_config_show_subcommand() {
        let args = parse(&["purec", "config", "show"]);

        assert!(matches!(
            args.command,
            Some(PurecCommand::Config {
                command: ConfigCommand::Show
            })
        ));
    }

    #[test]
    fn parses_config_init_subcommand() {
        let args = parse(&["purec", "config", "init"]);

        assert!(matches!(
            args.command,
            Some(PurecCommand::Config {
                command: ConfigCommand::Init
            })
        ));
    }

    #[test]
    fn first_run_tui_triggers_only_for_prompt_path_without_config() {
        let prompt_args = parse(&["purec", "创建 HTTP 服务器"]);
        let config_args = parse(&["purec", "config", "path"]);

        assert!(should_run_first_run_tui(&prompt_args.command, false));
        assert!(!should_run_first_run_tui(&prompt_args.command, true));
        assert!(!should_run_first_run_tui(&config_args.command, false));
    }

    #[test]
    fn parses_workspace_short_flag() {
        let args = parse(&["purec", "-w", "/tmp/project", "build app"]);

        assert_eq!(args.workspace.as_deref(), Some(Path::new("/tmp/project")));
        assert_eq!(args.prompt_text(), "build app");
    }

    #[test]
    fn parses_workspace_long_flag() {
        let args = parse(&["purec", "--workspace", "/tmp/project", "build app"]);

        assert_eq!(args.workspace.as_deref(), Some(Path::new("/tmp/project")));
    }

    #[test]
    fn workspace_defaults_to_none() {
        let args = parse(&["purec", "build app"]);

        assert!(args.workspace.is_none());
    }

    #[test]
    fn load_workspace_rejects_missing_directory() {
        let result = load_workspace_instructions(Path::new("/nonexistent/dir/abc123"));

        assert!(result.is_err());
    }

    #[test]
    fn load_workspace_returns_empty_for_missing_agents_md() {
        let dir = std::env::temp_dir().join("purec-test-no-agents");
        std::fs::create_dir_all(&dir).unwrap();

        let result = load_workspace_instructions(&dir).unwrap();

        assert!(result.is_empty());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn load_workspace_reads_agents_md_content() {
        let dir = std::env::temp_dir().join("purec-test-with-agents");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("Agents.md"), "# Test Project\nRules here").unwrap();

        let result = load_workspace_instructions(&dir).unwrap();

        assert_eq!(result, "# Test Project\nRules here");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
