## 审查范围

Delivery

本轮必须审查当前 Completion 的完整精确 diff。其他 WorkUnit 只是延后集成的上下文：不得把尚未合并的 sibling 文件、跨 WorkUnit 集成或任务整体完整性报告为当前 executor 的 Delivery finding；这些问题属于合并后的 Integrated review。

### 目标 WorkUnit

```json
{{TARGET_FOCUS_JSON}}
```

### 其他 WorkUnit（仅延后集成上下文）

```json
{{SIBLING_FOCUS_JSON}}
```

### Completion

```json
{{COMPLETION_JSON}}
```

### 冻结 changed-files

```json
{{CHANGED_FILES_JSON}}
```

### 精确 Completion diff

```diff
{{DIFF}}
```
