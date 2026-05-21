use pl_model::TokenUsage;

/// 编译请求的执行模式。
///
/// `Plan` 只要求核心流程产出规划与解释；`Auto` 允许模型生成更主动的
/// 编译步骤，但当前版本不会执行命令、写文件或调用沙箱。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum CompileMode {
    #[default]
    Plan,
    Auto,
}

impl CompileMode {
    pub fn instructions(self) -> &'static str {
        match self {
            Self::Plan => {
                "你是 Pure-Lang 的核心编译器。请把用户的自然语言需求整理成清晰的编译计划，说明目标、步骤和需要确认的风险。不要执行命令或修改文件。"
            }
            Self::Auto => {
                "你是 Pure-Lang 的核心编译器。请根据用户的自然语言需求生成可执行导向的编译方案和下一步动作建议，但当前前端不会执行命令、修改文件或调用沙箱。"
            }
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Plan => "plan",
            Self::Auto => "auto",
        }
    }
}

/// 单轮核心编译请求。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TurnRequest {
    pub prompt: String,
    pub mode: CompileMode,
}

impl TurnRequest {
    pub fn new(prompt: impl Into<String>, mode: CompileMode) -> Self {
        Self {
            prompt: prompt.into(),
            mode,
        }
    }
}

/// 单轮核心编译结果。
#[derive(Debug, Clone)]
pub struct TurnResult {
    pub content: String,
    pub reasoning_content: Option<String>,
    pub model: String,
    pub usage: TokenUsage,
    pub mode: CompileMode,
    pub session_message_count: usize,
}
