use super::*;

#[test]
fn detects_explicit_subagent_partition_requests() {
    assert!(prompt_requires_subagent_dispatch(
        "每个 crate 分一个 subagent 探索，然后介绍整个项目"
    ));
    assert!(prompt_requires_subagent_dispatch(
        "请分别用子代理探索前端和后端"
    ));
    assert!(!prompt_requires_subagent_dispatch("介绍整个项目"));
    assert!(!prompt_requires_subagent_dispatch(
        "用 bash 看一下每个 crate"
    ));
    assert!(!prompt_requires_subagent_dispatch(
        "读取 src/tool/subagent.rs，并总结每个模块的职责"
    ));
}

#[test]
fn subagent_dispatch_instructions_describe_structured_capacity_errors() {
    assert!(SUBAGENT_DISPATCH_CONSTRAINT.contains("结构化容量错误"));
    assert!(SUBAGENT_DISPATCH_CONSTRAINT.contains("停止继续创建"));
    assert!(SUBAGENT_FORCE_DISPATCH_INSTRUCTION.contains("结构化容量错误"));
}
