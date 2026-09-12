import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../app/theme/studio_tokens.dart';
import '../../data/repositories/studio_repository.dart';
import '../../domain/models/studio_models.dart';
import '../../l10n/studio_l10n.dart';
import 'settings_common.dart';

class InstructionsTab extends ConsumerStatefulWidget {
  const InstructionsTab({super.key, required this.settings});

  final InstructionsSettingsView settings;

  @override
  ConsumerState<InstructionsTab> createState() => _InstructionsTabState();
}

class _InstructionsTabState extends ConsumerState<InstructionsTab> {
  late final TextEditingController _baseController;
  late final TextEditingController _developerController;
  late final TextEditingController _userController;
  Timer? _saveTimer;
  bool _saving = false;
  int _section = 0;
  String? _error;

  @override
  void initState() {
    super.initState();
    _baseController = TextEditingController(text: widget.settings.baseOverride);
    _developerController = TextEditingController(
      text: widget.settings.developer,
    );
    _userController = TextEditingController(text: widget.settings.user);
  }

  @override
  void didUpdateWidget(covariant InstructionsTab oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (_saving) {
      return;
    }
    if (oldWidget.settings.baseOverride != widget.settings.baseOverride &&
        _baseController.text != widget.settings.baseOverride) {
      _baseController.text = widget.settings.baseOverride;
    }
    if (oldWidget.settings.developer != widget.settings.developer &&
        _developerController.text != widget.settings.developer) {
      _developerController.text = widget.settings.developer;
    }
    if (oldWidget.settings.user != widget.settings.user &&
        _userController.text != widget.settings.user) {
      _userController.text = widget.settings.user;
    }
  }

  @override
  void dispose() {
    _saveTimer?.cancel();
    _baseController.dispose();
    _developerController.dispose();
    _userController.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final labels = [
      context.l10n.settingsBaseInstructions,
      context.l10n.settingsDeveloperInstructions,
      context.l10n.settingsUserContext,
    ];
    final controllers = [
      _baseController,
      _developerController,
      _userController,
    ];
    return SettingsPageLayout(
      header: SettingsHeader(
        title: context.l10n.settingsInstructionsTitle,
        subtitle: context.l10n.settingsInstructionsSubtitle,
        trailing: _saving
            ? const SizedBox(width: 80, child: LinearProgressIndicator())
            : null,
      ),
      toolbar: Wrap(
        spacing: 8,
        runSpacing: 8,
        children: [
          for (var index = 0; index < labels.length; index++)
            TextButton(
              key: ValueKey('instruction-section-$index'),
              onPressed: () => setState(() => _section = index),
              style: TextButton.styleFrom(
                foregroundColor: index == _section
                    ? context.colors.primary
                    : context.colors.onSurfaceVariant,
                backgroundColor: index == _section
                    ? context.colors.surfaceContainer
                    : Colors.transparent,
              ),
              child: Text(labels[index]),
            ),
        ],
      ),
      child: ListView(
        children: [
          TextField(
            key: ValueKey('instruction-editor-$_section'),
            controller: controllers[_section],
            minLines: 16,
            maxLines: null,
            style: context.text.bodyMedium?.copyWith(height: 1.7),
            decoration: InputDecoration(
              labelText: labels[_section],
              hintText: context.l10n.settingsInstructionHint,
              alignLabelWithHint: true,
              contentPadding: const EdgeInsets.all(20),
            ),
            onChanged: (_) => _scheduleSave(),
          ),
          if (_error != null) SettingsInlineError(message: _error!),
        ],
      ),
    );
  }

  void _scheduleSave() {
    _saveTimer?.cancel();
    _saveTimer = Timer(const Duration(milliseconds: 650), () {
      unawaited(_save());
    });
  }

  Future<void> _save() async {
    setState(() {
      _saving = true;
      _error = null;
    });
    try {
      await ref
          .read(studioControllerProvider.notifier)
          .saveInstructionsSettings(
            InstructionsSettingsCommand(
              baseOverride: _baseController.text,
              developer: _developerController.text,
              user: _userController.text,
              projectDocMaxBytes: widget.settings.projectDocMaxBytes,
              projectDocFallbackFilenames:
                  widget.settings.projectDocFallbackFilenames,
            ),
          );
    } catch (error) {
      if (mounted) {
        setState(() => _error = error.toString());
      }
    } finally {
      if (mounted) {
        setState(() => _saving = false);
      }
    }
  }
}
