# 第三方预置技能声明

本 crate 的部分预置系统技能由 `cargo xtask sync-skills` 从以下上游仓库的默认
分支最新提交同步而来,完全替换 `assets/skills/<name>/` 后提交进源码库;源码库即
canonical 内容,构建期不访问网络。每次同步后需人工核对本文件中的 revision 并随
变更一起提交。

## anthropics/skills

- 上游:<https://github.com/anthropics/skills>
- 最近同步 revision:`3b3fad96af16a10759d930941b4520ba0c40edae`
- 技能:`canvas-design`、`frontend-design`
- 许可:Apache License 2.0,Copyright Anthropic, PBC。各技能目录自带
  `LICENSE.txt`,同步拷贝时原样保留。
- 注意:同仓库的 `pdf`、`docx`、`pptx`、`xlsx` 使用 Anthropic 专有
  source-available 许可(禁止再分发),**不得**加入预置集合。

## NousResearch/hermes-agent

- 上游:<https://github.com/NousResearch/hermes-agent>
- 最近同步 revision:`cced6fa360a589ba50abfde687ef1bcba8ddaf2e`
- 技能:`docx`、`pdf`、`powerpoint`、`xlsx`(位于 `skills/productivity/`)
- 许可:MIT License,Copyright (c) Nous Research。按 MIT 条款在产物中随技能
  分发时保留本声明中的版权与许可指向;许可全文见上游仓库 `LICENSE`。
