use super::super::{
    AgentRuntimeError, AgentRuntimeHost, AgentRuntimeResult, AgentSubmissionPage, ThreadRepository,
};
use super::AgentLoop;

impl<H> AgentLoop<H>
where
    H: AgentRuntimeHost,
{
    /// 读取本 agent 的 durable 阶段提交历史（含已关闭状态后的全量记录）。
    ///
    /// 主代理通过协作工具 `read_agent_submissions` 主动 pull，按提交顺序分页返回，
    /// detail 全文不截断。
    pub(super) async fn read_submissions(
        &self,
        offset: usize,
        limit: usize,
    ) -> AgentRuntimeResult<AgentSubmissionPage> {
        self.host
            .repository()
            .list_submissions(&self.state.snapshot.identity.id, offset, limit)
            .await
            .map_err(|error| AgentRuntimeError::Repository(error.to_string()))
    }
}
