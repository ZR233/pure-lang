class SessionRuntimeView {
  const SessionRuntimeView({
    required this.model,
    required this.contextTokens,
    required this.contextWindow,
    required this.totalTokens,
    required this.costLabel,
    required this.activeSkills,
    required this.activeMcpServers,
    required this.activeLspServers,
    required this.agentCount,
  });

  final String model;
  final int contextTokens;
  final int contextWindow;
  final int totalTokens;
  final String costLabel;
  final List<String> activeSkills;
  final List<String> activeMcpServers;
  final List<String> activeLspServers;
  final int agentCount;

  SessionRuntimeView copyWith({
    String? model,
    int? contextTokens,
    int? contextWindow,
    int? totalTokens,
    String? costLabel,
    List<String>? activeSkills,
    List<String>? activeMcpServers,
    List<String>? activeLspServers,
    int? agentCount,
  }) {
    return SessionRuntimeView(
      model: model ?? this.model,
      contextTokens: contextTokens ?? this.contextTokens,
      contextWindow: contextWindow ?? this.contextWindow,
      totalTokens: totalTokens ?? this.totalTokens,
      costLabel: costLabel ?? this.costLabel,
      activeSkills: activeSkills ?? this.activeSkills,
      activeMcpServers: activeMcpServers ?? this.activeMcpServers,
      activeLspServers: activeLspServers ?? this.activeLspServers,
      agentCount: agentCount ?? this.agentCount,
    );
  }
}
