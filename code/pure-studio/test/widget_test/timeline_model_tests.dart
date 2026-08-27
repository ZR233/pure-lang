part of '../widget_test.dart';

void registerTimelineModelTests() {
  testWidgets('history image loads through the authorized attachment API', (
    tester,
  ) async {
    final item = _threadItemFixture(
      id: 'message-with-image',
      threadId: 'thread-1',
      turnId: 'turn-1',
      ordinal: 1,
      kind: ThreadItemKind.userMessage,
      text: 'marker',
      attachments: const [
        ThreadAttachmentView(
          id: 'attachment-history-1',
          modality: AttachmentModalityView.image,
          mediaType: 'image/png',
          filename: 'PURE-7429.png',
          byteSize: 68,
          width: 1,
          height: 1,
        ),
      ],
    );
    final api = _FakeStudioApi(_emptyState())
      ..threadAttachmentBytes[(
        threadId: 'thread-1',
        attachmentId: 'attachment-history-1',
      )] = base64Decode(
        'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII=',
      );

    await tester.pumpWidget(
      _timelineHarness(threadId: 'thread-1', items: [item], api: api),
    );
    await tester.pumpAndSettle();

    expect(
      find.byKey(StudioDriverKeys.historyAttachment('attachment-history-1')),
      findsOneWidget,
    );
    expect(api.readThreadAttachmentRequests, [
      (threadId: 'thread-1', attachmentId: 'attachment-history-1'),
    ]);
    await tester.tap(
      find.byKey(StudioDriverKeys.historyAttachment('attachment-history-1')),
    );
    await tester.pumpAndSettle();
    expect(find.byType(Dialog), findsOneWidget);
  });

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

  test('repeated Skill Items remain independent timeline rows', () {
    final rows = timelineRowsFromThreadItems([
      _threadItemFixture(
        id: 'skill-1',
        threadId: 'thread-1',
        turnId: 'turn-1',
        ordinal: 1,
        kind: ThreadItemKind.skill,
        skill: TimelineSkillActivation(
          name: 'pdf',
          source: 'system',
          providerId: 'local-filesystem',
          resourceBase: const SkillResourceBaseView(
            SkillResourceBaseKind.directory,
            '/skills/pdf',
          ),
          cause: const SkillActivationCauseView(
            SkillActivationCauseKind.tool,
            'tool-1',
          ),
          activatedAt: _fixtureDate(1),
        ),
      ),
      _threadItemFixture(
        id: 'skill-2',
        threadId: 'thread-1',
        turnId: 'turn-1',
        ordinal: 2,
        kind: ThreadItemKind.skill,
        skill: TimelineSkillActivation(
          name: 'pdf',
          source: 'system',
          providerId: 'local-filesystem',
          resourceBase: const SkillResourceBaseView(
            SkillResourceBaseKind.directory,
            '/skills/pdf',
          ),
          cause: const SkillActivationCauseView(
            SkillActivationCauseKind.tool,
            'tool-2',
          ),
          activatedAt: _fixtureDate(2),
        ),
      ),
    ]);

    expect(rows, hasLength(2));
    expect(rows.map((row) => row.type), [
      TimelineRowType.skillActivation,
      TimelineRowType.skillActivation,
    ]);
    expect(rows.map((row) => row.part!.skill!.cause.id), ['tool-1', 'tool-2']);
  });

  testWidgets('Skill Item renders a compact localized activation row', (
    tester,
  ) async {
    final item = _threadItemFixture(
      id: 'skill-1',
      threadId: 'thread-1',
      turnId: 'turn-1',
      ordinal: 1,
      kind: ThreadItemKind.skill,
      skill: TimelineSkillActivation(
        name: 'pdf',
        source: 'system',
        providerId: 'local-filesystem',
        resourceBase: const SkillResourceBaseView(
          SkillResourceBaseKind.directory,
          '/skills/pdf',
        ),
        cause: const SkillActivationCauseView(
          SkillActivationCauseKind.tool,
          'tool-1',
        ),
        activatedAt: _fixtureDate(1),
      ),
    );

    await tester.pumpWidget(
      _timelineHarness(threadId: 'thread-1', items: [item]),
    );
    await tester.pumpAndSettle();

    expect(
      find.byKey(StudioDriverKeys.timelineSkillActivation('skill-1')),
      findsOneWidget,
    );
    expect(find.text('Agent activated skill · pdf'), findsOneWidget);
    expect(find.text('system'), findsOneWidget);
  });

  testWidgets('user gesture Skill Item uses distinct localized copy', (
    tester,
  ) async {
    final item = _threadItemFixture(
      id: 'skill-user-1',
      threadId: 'thread-1',
      turnId: 'turn-1',
      ordinal: 1,
      kind: ThreadItemKind.skill,
      skill: TimelineSkillActivation(
        name: 'doc',
        source: 'user',
        providerId: 'local-filesystem',
        resourceBase: const SkillResourceBaseView(
          SkillResourceBaseKind.directory,
          '/skills/doc',
        ),
        cause: const SkillActivationCauseView(
          SkillActivationCauseKind.userGesture,
          'user-skill-0',
        ),
        activatedAt: _fixtureDate(1),
      ),
    );

    await tester.pumpWidget(
      _timelineHarness(threadId: 'thread-1', items: [item]),
    );
    await tester.pumpAndSettle();

    expect(find.text('User activated skill · doc'), findsOneWidget);
  });
}
