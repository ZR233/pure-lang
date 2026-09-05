# 模型计量真实验收

从仓库根目录运行：

```bash
PURE_STUDIO_WIRE_CAPTURE_DIR="$PWD/target/model-accounting-live/wire" \
  cargo run -p pl-studio-runtime --features live-tests \
  --example model_accounting_live -- target/model-accounting-live/report.json
```

示例只读取现有配置的连接信息，从系统凭据库或配置指定的环境变量取得 Key；不改写用户配置。
每个已配置供应商执行短请求、最多三次固定前缀缓存验证，以及真实注册工具的多轮任务。
报告记录 endpoint、模型、请求时刻、最终计量和冻结费用；`usage-*.json` 只保存服务端数值计数及响应身份。
每次运行应使用新的 capture 目录，以保留此前证据。

- `--tools`：只执行完整工具任务，输出 `TurnResult.billing` 中的逐笔账单。
- `--native`：验证 DeepSeek 原生搜索与上下文回放，以及转发 OpenAI 的兼容 Responses。
- `--compatible`：验证显式兼容 Chat（DeepSeek endpoint）和 Responses（OpenAI endpoint）的工具往返。

鉴权、配额、缺少配置或未取得完整用量均不算通过。三次缓存验证仍未观察到命中也保留为未完成；
退出码非零，同时保留此前成功的账单和失败证据。未配置 MiMo 时，完整验收不能宣称全部通过。
OpenAI 的现有转发地址只证明该入口的行为，不能代替官方直连验收。

GUI 功能验收使用 `cargo xtask verify-gui --integration`。另可启动隔离数据目录的
`cargo xtask run-gui --driver`，再执行：

```bash
cargo dart run test_driver/provider_acceptance_driver.dart \
  <VM服务地址> <绝对路径的证据目录>
```

Driver 从设置页添加兼容供应商、保存、确认 canonical 模型选择，提交任务并检查界面的最终用量。
原生模式使用脚本内的本地 HTTP 供应商，只替换外部网络；模型执行、工具收尾、数据库及 FRB 全部是真实实现。
`run-gui --demo --driver` 配合脚本的 `--demo` 选项只用于界面交互验收。
