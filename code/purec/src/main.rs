use clap::Parser;
use clap::Subcommand;
use pl_core::{CompileMode, ConfigStore, CoreSession, ModelRole, PureCore, PureError, TurnRequest};

#[tokio::main]
async fn main() -> pl_core::Result<()> {
    let PurecArgs {
        command,
        plan,
        auto,
        prompt,
    } = PurecArgs::parse();
    match command {
        Some(command) => run_command(command),
        None => {
            run_prompt(PurecArgs {
                command: None,
                plan,
                auto,
                prompt,
            })
            .await
        }
    }
}

async fn run_prompt(args: PurecArgs) -> pl_core::Result<()> {
    ensure_prompt(&args)?;
    let request = TurnRequest::new(args.prompt_text(), args.compile_mode());
    let store = ConfigStore::default_app()?;
    let config = store.load_or_default()?;
    let core = PureCore::from_config(&config, ModelRole::Planner)?;
    let mut session = CoreSession::new();
    let (event_tx, _event_rx) = tokio::sync::broadcast::channel(256);
    let result = core.run_turn(&mut session, request, event_tx).await?;

    if !result.content.trim().is_empty() {
        println!("{}", result.content.trim_end());
    }

    Ok(())
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
#[command(
    name = "purec",
    about = "Pure-Lang 命令行编译器前端",
    version,
    arg_required_else_help = true
)]
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

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> PurecArgs {
        PurecArgs::try_parse_from(args).unwrap()
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
}
