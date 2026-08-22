part of '../widget_test.dart';

void registerTimelineModelTests() {
  test('timeline projects ThreadItems by immutable ordinal', () {
    final rows = timelineRowsFromThreadItems([
      _threadItemFixture(
        id: 'later',
        threadId: 'thread-1',
        turnId: 'turn-1',
        ordinal: 2,
        text: 'later',
      ),
      _threadItemFixture(
        id: 'earlier',
        threadId: 'thread-1',
        turnId: 'turn-1',
        ordinal: 1,
        text: 'earlier',
      ),
    ]);

    expect(rows.map((row) => row.id), ['earlier', 'later']);
    expect(rows.map((row) => row.part!.text), ['earlier', 'later']);
  });

  test('user, commentary and final channels remain distinct', () {
    final rows = timelineRowsFromThreadItems([
      _threadItemFixture(
        id: 'user',
        threadId: 'thread-1',
        turnId: 'turn-1',
        ordinal: 0,
        kind: ThreadItemKind.userMessage,
        channel: null,
        text: 'prompt',
      ),
      _threadItemFixture(
        id: 'commentary',
        threadId: 'thread-1',
        turnId: 'turn-1',
        ordinal: 1,
        channel: AgentMessageChannel.commentary,
        text: 'working',
      ),
      _threadItemFixture(
        id: 'final',
        threadId: 'thread-1',
        turnId: 'turn-1',
        ordinal: 2,
        channel: AgentMessageChannel.finalAnswer,
        text: 'done',
      ),
    ]);

    expect(rows.map((row) => row.type), [
      TimelineRowType.userMessage,
      TimelineRowType.commentary,
      TimelineRowType.finalAnswer,
    ]);
  });

  test('adjacent tool Items are grouped only in the visual projection', () {
    final first = _threadItemFixture(
      id: 'tool-1',
      threadId: 'thread-1',
      turnId: 'turn-1',
      ordinal: 0,
      kind: ThreadItemKind.toolCall,
      status: 'succeeded',
      channel: null,
      tool: const TimelineToolPart(toolCallId: 'call-1', name: 'read_file'),
    );
    final second = _threadItemFixture(
      id: 'tool-2',
      threadId: 'thread-1',
      turnId: 'turn-1',
      ordinal: 1,
      kind: ThreadItemKind.toolCall,
      status: 'succeeded',
      channel: null,
      tool: const TimelineToolPart(toolCallId: 'call-2', name: 'rg'),
    );

    final rows = timelineRowsFromThreadItems([first, second]);

    expect(rows, hasLength(1));
    expect(rows.single.type, TimelineRowType.toolGroup);
    expect(rows.single.toolGroup!.items, hasLength(2));
    expect(first.id, 'tool-1');
    expect(second.id, 'tool-2');
  });

  test('tool grouping stops at a message boundary', () {
    final rows = timelineRowsFromThreadItems([
      _threadItemFixture(
        id: 'tool-1',
        threadId: 'thread-1',
        turnId: 'turn-1',
        ordinal: 0,
        kind: ThreadItemKind.toolCall,
        status: 'succeeded',
        channel: null,
        tool: const TimelineToolPart(toolCallId: 'call-1', name: 'rg'),
      ),
      _threadItemFixture(
        id: 'commentary',
        threadId: 'thread-1',
        turnId: 'turn-1',
        ordinal: 1,
        channel: AgentMessageChannel.commentary,
        text: 'next',
      ),
      _threadItemFixture(
        id: 'tool-2',
        threadId: 'thread-1',
        turnId: 'turn-1',
        ordinal: 2,
        kind: ThreadItemKind.toolCall,
        status: 'succeeded',
        channel: null,
        tool: const TimelineToolPart(toolCallId: 'call-2', name: 'test'),
      ),
    ]);

    expect(rows.map((row) => row.type), [
      TimelineRowType.toolGroup,
      TimelineRowType.commentary,
      TimelineRowType.toolGroup,
    ]);
  });

  test('adjacent reasoning Items become one reasoning row', () {
    final rows = timelineRowsFromThreadItems([
      _threadItemFixture(
        id: 'reason-1',
        threadId: 'thread-1',
        turnId: 'turn-1',
        ordinal: 0,
        kind: ThreadItemKind.reasoning,
        channel: null,
        reasoningSummary: const ['summary'],
      ),
      _threadItemFixture(
        id: 'reason-2',
        threadId: 'thread-1',
        turnId: 'turn-1',
        ordinal: 1,
        kind: ThreadItemKind.reasoning,
        channel: null,
        reasoningContent: const ['details'],
      ),
    ]);

    expect(rows, hasLength(1));
    expect(rows.single.reasoningGroup!.parts, hasLength(2));
  });

  test('file Items stay outside the transcript timeline', () {
    final rows = timelineRowsFromThreadItems([
      _threadItemFixture(
        id: 'file-1',
        threadId: 'thread-1',
        turnId: 'turn-1',
        ordinal: 0,
        revision: 0,
        status: 'completed',
        createdAt: _fixtureDate(1),
        updatedAt: _fixtureDate(1),
        kind: ThreadItemKind.file,
        filePath: 'report.md',
        mediaType: 'text/markdown',
      ),
    ]);

    expect(rows, isEmpty);
  });
}
