use clap::Parser;
use pl_core::{CompileMode, CoreSession, PureCore, TurnRequest};

#[tokio::main]
async fn main() -> pl_core::Result<()> {
    let args = PurecArgs::parse();
    let request = TurnRequest::new(args.prompt_text(), args.compile_mode());
    let core = PureCore::default_provider()?;
    let mut session = CoreSession::new();
    let (event_tx, _event_rx) = tokio::sync::broadcast::channel(256);
    let result = core.run_turn(&mut session, request, event_tx).await?;

    if !result.content.trim().is_empty() {
        println!("{}", result.content.trim_end());
    }

    Ok(())
}

#[derive(Debug, Parser)]
#[command(name = "purec", about = "Pure-Lang 命令行编译器前端", version)]
struct PurecArgs {
    #[arg(
        long,
        conflicts_with = "auto",
        help = "只生成编译计划，不执行命令或修改文件"
    )]
    plan: bool,

    #[arg(long, help = "生成自动执行导向的编译方案，但当前版本仍不会执行命令")]
    auto: bool,

    #[arg(value_name = "PROMPT", required = true, num_args = 1..)]
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
        self.prompt.join(" ")
    }
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
}
