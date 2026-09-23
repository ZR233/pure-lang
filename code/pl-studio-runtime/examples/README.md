# 真实任务人工观察

从仓库根目录运行，artifact 目录必须尚不存在：

```bash
cargo run -p pl-studio-runtime --features live-tests --example model_observe -- \
  openai gpt-6-astra /absolute/task.txt /absolute/new-artifacts
```

读取 anywork 用户配置中选定供应商的完整配置与系统凭据，不改写配置或会话库。
任务文件是实际待处理要求，不要求固定回答。可选参数：

- `--image FILE`：图片 bytes。
- `--image-url URL`：远程图片，保存请求期快照。
- `--image-base64 FILE`：保存完整原始 Base64 的文本文件，默认 PNG。
- `--followup TASK_FILE`：在同一模型会话中继续任务，可重复。

`events.jsonl` 保存过程，`response-N.json` 保存响应和服务端用量，
`report.json` 分开记录 execution 与 review。execution 完成不是验收通过；
review 默认 pending，由观察者阅读过程、结果和产物后另行记录结论。
缺少配置、鉴权失败、超时与事件缺口必须保留，不能静默跳过。
不会断言回答文字、工具次数、缓存命中或业务终态。

完整工具/协作任务使用 `collaboration_observe`：
设置 `ANYWORK_OBSERVATION_PROMPT` 为任务文件，
`ANYWORK_WORKFLOW_ARTIFACT_DIR` 为新的证据目录，
`ANYWORK_OBSERVATION_SECONDS` 为观察时长。使用隔离 Studio 数据和工作目录，
读取用户配置并通过系统凭据库鉴权，过程结束回收 runtime。
设置 `ANYWORK_OBSERVATION_PROVIDER` 与 `ANYWORK_OBSERVATION_MODEL` 可在隔离副本中选择所有角色的模型；未选中的供应商不参与此次配置校验，原配置保持不变。
可用真实任务观察 patch、LSP、跨轮历史、压缩与协作，不再维护固定通过标记。

GUI 真实任务通过 `cargo xtask run-gui --driver` 启动，由观察者使用 Flutter Driver
提交任务、查看流、截图和产物，并人工记录结论。自动 Rust 测试仅保留在
`pl-core` 和 `pl-model` 两库；此处不再承诺配置迁移、恢复或
GUI 场景的自动断言；隔离模拟入口使用 `cargo xtask manual-gui`。
