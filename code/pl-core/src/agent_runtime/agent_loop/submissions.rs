use super::super::{AgentRuntimeHost, AgentRuntimeResult, AgentSubmissionPage};
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
        let history = &self.state.session.submissions;
        let limit = limit.max(1);
        let items = history
            .iter()
            .skip(offset)
            .take(limit)
            .cloned()
            .collect::<Vec<_>>();
        Ok(AgentSubmissionPage {
            has_more: offset.saturating_add(items.len()) < history.len(),
            items,
            offset,
            limit,
            total: history.len(),
        })
    }
}
