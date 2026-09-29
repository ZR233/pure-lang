import 'terminal_launcher.dart';

Future<bool> probeHostTerminalAvailable() async => false;

Future<void> launchHostTerminal(HostTerminalTarget target) =>
    Future<void>.error(
      UnsupportedError('Host terminal requires a desktop host'),
    );
