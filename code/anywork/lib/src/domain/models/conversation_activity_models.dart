import 'dart:convert';

import 'interaction_models.dart';
import 'studio_enums.dart';
import 'thread_activity_models.dart';

/// 固定活动条（输入框上方单行）唯一的展示阶段。
///
/// 它只由后端 typed 活动投影与 typed 存储状态派生：等待 API 与已开始 reasoning 都
/// 落到 [thinking]（“思考中”），工具落到 [runningTool]（“执行中”+完整命令/目标），
/// 保存被明确阻塞时落到 [storageBlocked]（显示准确原因而不是“思考中”）。这里不
/// 从 Timeline 行、可见性窗口或错误字符串推断任何阶段。
enum ConversationActivityKind {
  /// Turn 运行中但还没有运行中的 attempt。
  preparing,

  /// 思考中：等待 API 返回，或已开始产出 reasoning。
  thinking,

  /// 回复中：正在产出正文。
  responding,

  /// 规划中。
  planning,

  /// 执行中：工具正在运行（可能并行多条）。
  runningTool,

  /// 等待工具审批。
  awaitingApproval,

  /// 等待用户输入。
  awaitingInput,

  /// 正在打断当前 Turn。
  stopping,

  /// 保存被明确阻塞（故障/压力暂停/已暂停执行）。
  storageBlocked,
}

/// 展开详情中的一条：工具、推理或交互的完整内容。
///
/// [body] 始终是完整内容，展示层只做视觉截断（ellipsis）/有界滚动，不改变文本。
class ConversationActivityDetail {
  const ConversationActivityDetail({
    required this.id,
    required this.body,
    this.title,
  });

  /// 稳定身份（工具/条目标识），用于同一活动内多条详情去重与重建。
  final String id;

  /// 可选标题（例如工具名）；为空时展示层只渲染正文。
  final String? title;

  /// 完整详情正文。
  final String body;

  bool get hasTitle => title?.trim().isNotEmpty ?? false;
}

/// 固定活动条的完整输入。
class ConversationActivityView {
  const ConversationActivityView({
    required this.identity,
    required this.kind,
    this.summary,
    this.activeToolCount = 0,
    this.backgroundToolCount = 0,
    this.errorMessage,
    this.details = const [],
    this.detailsLoading = false,
    this.detailsError,
    this.expandable = false,
    this.storage,
  });

  /// 稳定活动身份：展开态以它为准，身份变化即重置。
  final String identity;

  /// 准确阶段，决定标签与图标。
  final ConversationActivityKind kind;

  /// 单行摘要；展示层只做视觉截断，不改变内容。
  final String? summary;

  /// 并行工具数量；`<= 1` 表示没有并行工具，不显示数量。
  final int activeToolCount;

  /// 其中处于后台的并行工具数量；仅用于调整“执行中”的呈现，不改变标签。
  final int backgroundToolCount;

  /// 准确的故障文案（例如保存故障）；展示层优先于 [summary]。
  final String? errorMessage;

  /// 已按需读取到的完整详情；为空但 [expandable] 为真时活动条仍提供展开并触发读取。
  final List<ConversationActivityDetail> details;

  /// 详情请求是否在途。
  final bool detailsLoading;

  /// 详情读取失败的准确错误；为空表示没有失败。
  final String? detailsError;

  /// 该活动是否存在可展开的完整内容（工具/推理/输出）。
  final bool expandable;

  /// 派生 `storageBlocked` 阶段的 typed 存储事实；其它阶段为空。
  final ThreadStorageStateView? storage;

  bool get hasDetails => expandable || details.isNotEmpty;

  /// 是否以“等待”脉冲呈现：准备/等待模型/思考/执行/审批/输入/停止。
  bool get waits =>
      kind == ConversationActivityKind.preparing ||
      kind == ConversationActivityKind.thinking ||
      kind == ConversationActivityKind.runningTool ||
      kind == ConversationActivityKind.awaitingApproval ||
      kind == ConversationActivityKind.awaitingInput ||
      kind == ConversationActivityKind.stopping;
}

/// 从后端 typed 活动、typed 存储状态与待处理交互合成固定活动条输入。
///
/// 这是唯一的事实入口：阶段/摘要/并行数量只来自 typed 活动，完整详情由调用方在展开时
/// 按活动身份读取后传入。它不读窗口可见行、不做 SQL、不解析错误字符串，也不填默认阶段。
ConversationActivityView? projectConversationActivity({
  required ThreadActivityView? activity,
  ThreadStorageStateView? storage,
  PendingInteraction? interaction,
  ThreadActivityDetail? detail,
  bool detailsLoading = false,
  String? detailsError,
}) {
  if (interaction != null) {
    return _interactionActivity(interaction);
  }
  if (storage != null && storage.blocksContinuation) {
    return _storageActivity(storage);
  }
  if (activity == null) return null;
  return _typedActivity(
    activity,
    detail: detail,
    detailsLoading: detailsLoading,
    detailsError: detailsError,
  );
}

ConversationActivityView _typedActivity(
  ThreadActivityView activity, {
  ThreadActivityDetail? detail,
  required bool detailsLoading,
  String? detailsError,
}) {
  final kind = switch (activity.kind) {
    // 等待 API 与已开始 reasoning 都是“思考中”：单行标签一致，摘要取最新非空逻辑行。
    ThreadActivityKind.preparing => ConversationActivityKind.preparing,
    ThreadActivityKind.waitingApi => ConversationActivityKind.thinking,
    ThreadActivityKind.thinking => ConversationActivityKind.thinking,
    ThreadActivityKind.responding => ConversationActivityKind.responding,
    ThreadActivityKind.planning => ConversationActivityKind.planning,
    ThreadActivityKind.runningTool => ConversationActivityKind.runningTool,
    ThreadActivityKind.awaitingApproval =>
      ConversationActivityKind.awaitingApproval,
    ThreadActivityKind.awaitingInput => ConversationActivityKind.awaitingInput,
    ThreadActivityKind.stopping => ConversationActivityKind.stopping,
  };
  final summary = _singleLine(activity.summary);
  final tools = activity.tools;
  final currentDetail =
      detail is CurrentThreadActivityDetail &&
          detail.activity.identity == activity.identity
      ? detail
      : null;
  final details = currentDetail == null
      ? const <ConversationActivityDetail>[]
      : _activityDetails(currentDetail);
  // 后端已明确该身份不再是当前活动（`superseded` / `ended`）时，没有可展开的当前
  // 内容：不提供展开入口，也不显示空的详情面板；新的身份会作为新活动重新提供详情。
  final detailResolved = detail == null || currentDetail != null;
  return ConversationActivityView(
    identity: activity.identity,
    kind: kind,
    summary: summary.isEmpty ? null : summary,
    activeToolCount: tools.count,
    backgroundToolCount: tools.background,
    details: details,
    detailsLoading: detailsLoading,
    detailsError: detailsError,
    expandable: detailResolved && _expandable(activity, kind),
  );
}

/// 该活动是否有可展开的完整内容：工具（含并行多条）或推理/输出正文。
bool _expandable(ThreadActivityView activity, ConversationActivityKind kind) {
  if (activity.tools.active.isNotEmpty) return true;
  return switch (kind) {
    ConversationActivityKind.thinking ||
    ConversationActivityKind.responding ||
    ConversationActivityKind.planning ||
    ConversationActivityKind.runningTool => true,
    ConversationActivityKind.preparing ||
    ConversationActivityKind.awaitingApproval ||
    ConversationActivityKind.awaitingInput ||
    ConversationActivityKind.stopping ||
    ConversationActivityKind.storageBlocked => false,
  };
}

/// 完整详情：推理/输出正文按发生顺序列出，工具显示全部参数与输出。
List<ConversationActivityDetail> _activityDetails(
  CurrentThreadActivityDetail detail,
) {
  final out = <ConversationActivityDetail>[];
  for (final part in detail.reasoning) {
    final body = part.text.trim();
    if (body.isEmpty) continue;
    out.add(
      ConversationActivityDetail(id: 'reasoning:${part.itemId}', body: body),
    );
  }
  for (final part in detail.response) {
    final body = part.text.trim();
    if (body.isEmpty) continue;
    out.add(
      ConversationActivityDetail(id: 'response:${part.itemId}', body: body),
    );
  }
  for (final tool in detail.tools) {
    out.add(
      ConversationActivityDetail(
        id: 'tool:${tool.callId}',
        title: tool.name.trim().isEmpty ? null : tool.name.trim(),
        body: _toolDetailBody(tool),
      ),
    );
  }
  return out;
}

String _toolDetailBody(ThreadActivityToolDetail tool) {
  final lines = <String>[
    if (tool.arguments?.trim().isNotEmpty == true) tool.arguments!.trim(),
    if (tool.output?.trim().isNotEmpty == true) tool.output!.trim(),
  ];
  return lines.join('\n\n');
}

ConversationActivityView _storageActivity(ThreadStorageStateView storage) {
  // 已核验可继续（`canResume`）表示保存已恢复、水位核验通过；core 的故障闩在显式继续前
  // 可能仍保留上一次的错误文本，因此此时不把它作为“正在失败”的摘要展示（`errorMessage`
  // 会被展示层优先于摘要）。上一轮的故障原因仍以 typed `fault`/`lastError` 留在 [storage]
  // 供详情与诊断使用，恢复与否只由 typed `canResume` 判定，绝不解析错误文本。
  final recovered = storage.resumeRequired && storage.canResume;
  final reason = recovered ? null : storage.blockingReason;
  return ConversationActivityView(
    // 身份绑定故障代数与硬故障闩：同一代数/闩状态内保持展开，代数变化、显式继续或恢复即重置。
    identity:
        'storage:${storage.faultGeneration}:'
        '${storage.fault?.name ?? (storage.resumeRequired ? 'resumeRequired' : 'paused')}',
    kind: ConversationActivityKind.storageBlocked,
    errorMessage: reason,
    storage: storage,
  );
}

ConversationActivityView _interactionActivity(PendingInteraction interaction) {
  final kind = interaction.kind == InteractionKind.toolApproval
      ? ConversationActivityKind.awaitingApproval
      : ConversationActivityKind.awaitingInput;
  final details = <ConversationActivityDetail>[];
  final String summary;
  switch (interaction.payload) {
    case ToolApprovalInteractionPayload(:final toolName, :final arguments):
      final name = toolName.trim();
      summary = name.isEmpty ? interaction.title : name;
      final formatted = _formatArguments(arguments);
      if (formatted.isNotEmpty) {
        details.add(
          ConversationActivityDetail(
            id: interaction.id,
            title: name.isEmpty ? null : name,
            body: formatted,
          ),
        );
      }
    case UserInputInteractionPayload(:final questions):
      for (final question in questions) {
        details.add(
          ConversationActivityDetail(
            id: question.id,
            title: question.header.trim().isEmpty ? null : question.header,
            body: question.question,
          ),
        );
      }
      summary = _firstNonEmpty([
        if (questions.isNotEmpty) questions.first.header,
        if (questions.isNotEmpty) questions.first.question,
        interaction.title,
      ]);
    case UnknownInteractionPayload():
      summary = interaction.title;
  }
  if (details.isEmpty && interaction.body.trim().isNotEmpty) {
    details.add(
      ConversationActivityDetail(id: interaction.id, body: interaction.body),
    );
  }
  final normalized = _singleLine(summary);
  return ConversationActivityView(
    identity: 'interaction:${interaction.id}',
    kind: kind,
    summary: normalized.isEmpty ? null : normalized,
    details: details,
    expandable: details.isNotEmpty,
  );
}

String _formatArguments(Object? value) {
  if (value == null) return '';
  if (value is String) return value.trim();
  try {
    return const JsonEncoder.withIndent('  ').convert(value);
  } on JsonUnsupportedObjectError {
    return value.toString();
  }
}

String _firstNonEmpty(List<String> values) {
  for (final value in values) {
    final trimmed = value.trim();
    if (trimmed.isNotEmpty) return trimmed;
  }
  return '';
}

String _singleLine(String value) {
  return value.replaceAll(RegExp(r'\s+'), ' ').trim();
}
