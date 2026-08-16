/// 工具对运行环境产生的副作用类别。
///
/// effect 由每个 `Tool` 实现显式声明；core 不按工具名查表推断。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ToolEffect {
    Read,
    WorkspaceWrite,
    Process,
    AgentControl,
    BranchControl,
}
