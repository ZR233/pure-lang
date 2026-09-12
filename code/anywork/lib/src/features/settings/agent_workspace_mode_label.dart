import 'package:flutter/material.dart';

import '../../domain/models/studio_models.dart';
import '../../l10n/studio_l10n.dart';

String agentWorkspaceModeLabel(BuildContext context, AgentWorkspaceMode mode) =>
    switch (mode) {
      AgentWorkspaceMode.unrestricted =>
        context.l10n.settingsAgentWorkspaceModeUnrestricted,
      AgentWorkspaceMode.directory =>
        context.l10n.settingsAgentWorkspaceModeDirectory,
      AgentWorkspaceMode.worktree =>
        context.l10n.settingsAgentWorkspaceModeWorktree,
    };
