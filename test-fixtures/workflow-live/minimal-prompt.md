在当前项目创建 hello.txt，内容严格为 ASCII hello 加单个 LF（6 字节）。只新增这个交付文件，不联网、不提交 Git。按任务模式完成全部阶段；计划不超过 8 行。用 python3 实际断言 read_bytes() == b"hello\n" 并打印 PURE_MINIMAL_VERIFY_OK。独立 reviewer 成功终态后读取 canonical submissions ，关闭本任务创建的子代理后收尾。最终用三个短段落报告结果、验证、剩余问题，段落使用真实换行。

验收环境是新建的空项目，无既有设计文档；简单确认后直接处理，不假设 design 目录存在。workflow_transition.completion 仅填写 reason、summary、evidence 三个字段，按工具 schema 原样提交。
