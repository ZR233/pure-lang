String repairAgentMarkdownForDisplay(String text) {
  var repaired = text.replaceAll('\r\n', '\n').replaceAll('\r', '\n');
  repaired = _separateTrailingFenceLines(repaired);
  repaired = _repairTightHeadings(repaired);
  return repaired;
}

String _repairTightHeadings(String text) {
  final lines = text.split('\n');
  var changed = false;
  final repaired = lines
      .map((line) {
        final match = RegExp(r'^(\s{0,3})(#{1,6})([^\s#].*)$').firstMatch(line);
        if (match == null) {
          return line;
        }
        changed = true;
        return '${match.group(1)}${match.group(2)} ${match.group(3)}';
      })
      .toList(growable: false);
  return changed ? repaired.join('\n') : text;
}

String _separateTrailingFenceLines(String text) {
  final lines = text.split('\n');
  final normalized = <String>[];
  String? openFenceChar;
  int openFenceLength = 0;
  var changed = false;

  for (final line in lines) {
    final trimmedLeft = line.trimLeft();
    final standaloneFence = _standaloneFence(trimmedLeft);
    if (standaloneFence != null) {
      normalized.add(line);
      if (openFenceChar == null) {
        openFenceChar = standaloneFence.substring(0, 1);
        openFenceLength = standaloneFence.length;
      } else if (_matchesOpenFence(
        standaloneFence,
        openFenceChar,
        openFenceLength,
      )) {
        openFenceChar = null;
        openFenceLength = 0;
      }
      continue;
    }

    final split = _splitTrailingFence(line, openFenceChar, openFenceLength);
    if (split == null) {
      normalized.add(line);
      continue;
    }

    normalized.add(split.$1.trimRight());
    normalized.add(split.$2);
    changed = true;
    if (openFenceChar == null) {
      openFenceChar = split.$2.substring(0, 1);
      openFenceLength = split.$2.length;
    } else {
      openFenceChar = null;
      openFenceLength = 0;
    }
  }

  return changed ? normalized.join('\n') : text;
}

String? _standaloneFence(String trimmedLeftLine) {
  final match = RegExp(r'^(`{3,}|~{3,})\s*$').firstMatch(trimmedLeftLine);
  return match?.group(1);
}

bool _matchesOpenFence(
  String fence,
  String openFenceChar,
  int openFenceLength,
) {
  return fence.substring(0, 1) == openFenceChar &&
      fence.length >= openFenceLength;
}

(String, String)? _splitTrailingFence(
  String line,
  String? openFenceChar,
  int openFenceLength,
) {
  final match = RegExp(r'(`{3,}|~{3,})\s*$').firstMatch(line);
  if (match == null || match.start == 0) {
    return null;
  }
  final fence = match.group(1);
  if (fence == null) {
    return null;
  }
  if (openFenceChar != null &&
      !_matchesOpenFence(fence, openFenceChar, openFenceLength)) {
    return null;
  }
  final prefix = line.substring(0, match.start);
  if (prefix.trim().isEmpty) {
    return null;
  }
  return (prefix, fence);
}
