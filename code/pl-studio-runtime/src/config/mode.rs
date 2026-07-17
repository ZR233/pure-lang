/// Pure Studio 的产品交互模式。
///
/// 模式只决定 Studio 选择的角色、指令、工具策略与完成方式；`pl-core`
/// 不感知这些产品语义。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum StudioMode {
    #[default]
    Simple,
    Task,
}

impl StudioMode {
    pub const fn instructions(self) -> &'static str {
        match self {
            Self::Simple => include_str!("../prompts/simple.md"),
            Self::Task => include_str!("../prompts/task.md"),
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Simple => "simple",
            Self::Task => "task",
        }
    }

    pub fn from_label(label: &str) -> Self {
        match label {
            "task" => Self::Task,
            "simple" => Self::Simple,
            _ => Self::Simple,
        }
    }
}
