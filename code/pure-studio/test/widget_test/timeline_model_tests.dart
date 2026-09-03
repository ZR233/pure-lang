part of '../widget_test.dart';

class _ImmediateTestImageProvider extends ImageProvider<String> {
  const _ImmediateTestImageProvider(this.key, this.image);

  final String key;
  final ui.Image image;

  @override
  Future<String> obtainKey(ImageConfiguration configuration) =>
      SynchronousFuture(key);

  @override
  ImageStreamCompleter loadImage(String key, ImageDecoderCallback decode) {
    return OneFrameImageStreamCompleter(
      SynchronousFuture(ImageInfo(image: image.clone())),
    );
  }
}

void registerTimelineModelTests() {
  test(
    'tool JSON projections share tolerant object and string normalization',
    () {
      expect(decodeJsonObject('not-json'), isEmpty);
      expect(decodeJsonObject('{"query":" rust "}')['query'], ' rust ');
      expect(jsonStringValue('  rust  '), 'rust');
      expect(jsonStringValue('   '), isNull);
      expect(jsonObject(<dynamic, dynamic>{1: 'value'})['1'], 'value');
    },
  );

  testWidgets(
    'view_image is visible in the collapsed tool gallery and opens its authorized thumbnail',
    (tester) async {
      const attachment = ThreadAttachmentView(
        id: 'tool-image-1',
        modality: AttachmentModalityView.image,
        mediaType: 'image/png',
        filename: 'PURE-7429.png',
        byteSize: 68,
        width: 1,
        height: 1,
      );
      final item = _threadItemFixture(
        id: 'view-image-tool-item',
        threadId: 'thread-1',
        turnId: 'turn-1',
        ordinal: 1,
        kind: ThreadItemKind.toolCall,
        status: 'succeeded',
        channel: null,
        tool: const TimelineToolPart(
          toolCallId: 'tool-call-1',
          callId: 'call-1',
          name: 'view_image',
          result: '{"viewedImage":true}',
          attachments: [attachment],
        ),
      );
      final api = _FakeStudioApi(_emptyState())
        ..threadAttachmentBytes[(
          threadId: 'thread-1',
          attachmentId: 'tool-image-1',
        )] = base64Decode(
          'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII=',
        );

      await tester.pumpWidget(
        _timelineHarness(threadId: 'thread-1', items: [item], api: api),
      );
      await tester.pumpAndSettle();

      expect(find.text('Image read'), findsOneWidget);
      expect(
        find.byKey(
          StudioDriverKeys.timelineToolGroupSummary(
            'tool-group:turn-1:view-image-tool-item',
          ),
        ),
        findsOneWidget,
      );
      expect(
        find.byKey(StudioDriverKeys.viewImageThumbnail('tool-image-1')),
        findsOneWidget,
      );
      expect(
        find.byKey(StudioDriverKeys.historyAttachment('tool-image-1')),
        findsNothing,
      );

      expect(api.readThreadAttachmentRequests, [
        (threadId: 'thread-1', attachmentId: 'tool-image-1'),
      ]);

      await tester.tap(
        find.byKey(const ValueKey('timeline-tool-group-summary')),
      );
      await tester.pumpAndSettle();
      expect(
        find.byKey(StudioDriverKeys.viewImageTool('call-1')),
        findsOneWidget,
      );
      expect(
        find.byKey(StudioDriverKeys.viewImageThumbnail('tool-image-1')),
        findsOneWidget,
      );
      expect(
        find.byKey(
          StudioDriverKeys.toolImageGallery(
            'tool-group:turn-1:view-image-tool-item',
          ),
        ),
        findsOneWidget,
      );
      expect(api.readThreadAttachmentRequests, [
        (threadId: 'thread-1', attachmentId: 'tool-image-1'),
      ]);

      await tester.tap(
        find.byKey(StudioDriverKeys.viewImageThumbnail('tool-image-1')),
      );
      await tester.pumpAndSettle();
      expect(
        find.byKey(StudioDriverKeys.viewImageDialog('tool-image-1')),
        findsOneWidget,
      );
      expect(find.byType(InteractiveViewer), findsOneWidget);
      await tester.tap(find.byKey(StudioDriverKeys.timelineImageClose));
      await tester.pumpAndSettle();
      expect(
        find.byKey(StudioDriverKeys.viewImageDialog('tool-image-1')),
        findsNothing,
      );
    },
  );

  testWidgets('view_image reports an authorized attachment load failure', (
    tester,
  ) async {
    const attachment = ThreadAttachmentView(
      id: 'tool-image-failed',
      modality: AttachmentModalityView.image,
      mediaType: 'image/png',
      filename: 'missing.png',
      byteSize: 68,
      width: 1,
      height: 1,
    );
    final item = _threadItemFixture(
      id: 'view-image-failed-item',
      threadId: 'thread-1',
      turnId: 'turn-1',
      ordinal: 1,
      kind: ThreadItemKind.toolCall,
      status: 'succeeded',
      channel: null,
      tool: const TimelineToolPart(
        toolCallId: 'tool-call-failed',
        callId: 'call-failed',
        name: 'view_image',
        attachments: [attachment],
      ),
    );
    final api = _FakeStudioApi(_emptyState())
      ..threadAttachmentErrors[(
        threadId: 'thread-1',
        attachmentId: 'tool-image-failed',
      )] = StateError(
        'attachment lease expired',
      );

    await tester.pumpWidget(
      _timelineHarness(threadId: 'thread-1', items: [item], api: api),
    );
    await tester.pumpAndSettle();

    expect(
      find.byKey(const ValueKey('attachment-load-failed-tool-image-failed')),
      findsOneWidget,
    );
    expect(
      find.byKey(StudioDriverKeys.viewImageDialog('tool-image-failed')),
      findsNothing,
    );

    api.threadAttachmentErrors.clear();
    api.threadAttachmentBytes[(
      threadId: 'thread-1',
      attachmentId: 'tool-image-failed',
    )] = base64Decode(
      'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII=',
    );
    await tester.tap(
      find.byKey(StudioDriverKeys.timelineImageRetry('tool-image-failed')),
    );
    await tester.pumpAndSettle();
    expect(api.readThreadAttachmentRequests, [
      (threadId: 'thread-1', attachmentId: 'tool-image-failed'),
      (threadId: 'thread-1', attachmentId: 'tool-image-failed'),
    ]);
  });

  testWidgets('multiple tool images use compact tiles and share one loader', (
    tester,
  ) async {
    const attachments = [
      ThreadAttachmentView(
        id: 'tool-image-a',
        modality: AttachmentModalityView.image,
        mediaType: 'image/png',
        filename: 'a.png',
        byteSize: 68,
        width: 1,
        height: 1,
      ),
      ThreadAttachmentView(
        id: 'tool-image-b',
        modality: AttachmentModalityView.image,
        mediaType: 'image/png',
        filename: 'b.png',
        byteSize: 68,
        width: 1,
        height: 1,
      ),
    ];
    final item = _threadItemFixture(
      id: 'image-tool-item',
      threadId: 'thread-1',
      turnId: 'turn-1',
      ordinal: 1,
      kind: ThreadItemKind.toolCall,
      status: 'succeeded',
      tool: const TimelineToolPart(
        toolCallId: 'tool-call-images',
        name: 'mcp__images__generate',
        attachments: attachments,
      ),
    );
    final bytes = base64Decode(
      'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII=',
    );
    final api = _FakeStudioApi(_emptyState());
    for (final attachment in attachments) {
      api.threadAttachmentBytes[(
            threadId: 'thread-1',
            attachmentId: attachment.id,
          )] =
          bytes;
    }

    await tester.pumpWidget(
      _timelineHarness(threadId: 'thread-1', items: [item], api: api),
    );
    await tester.pumpAndSettle();

    for (final attachment in attachments) {
      expect(
        tester.getSize(
          find.byKey(StudioDriverKeys.viewImageThumbnail(attachment.id)),
        ),
        const Size.square(64),
      );
    }
    await tester.tap(find.byKey(const ValueKey('timeline-tool-group-summary')));
    await tester.pumpAndSettle();
    expect(
      find.byKey(StudioDriverKeys.viewImageThumbnail('tool-image-a')),
      findsOneWidget,
    );
    expect(api.readThreadAttachmentRequests.length, 2);
  });

  testWidgets(
    'assistant HTTPS markdown image waits for click while local and user images stay inert',
    (tester) async {
      final testImage = (await tester.runAsync(
        () => createTestImage(width: 2, height: 2),
      ))!;
      var remoteLoads = 0;
      const remoteUrl = 'https://images.example/preview.png';
      final items = [
        _threadItemFixture(
          id: 'assistant-remote-image',
          threadId: 'thread-1',
          turnId: 'turn-1',
          ordinal: 1,
          text: '![Remote preview]($remoteUrl)',
        ),
        _threadItemFixture(
          id: 'assistant-local-image',
          threadId: 'thread-1',
          turnId: 'turn-1',
          ordinal: 2,
          text: r'![Local output](./output.png)',
        ),
        _threadItemFixture(
          id: 'assistant-http-image',
          threadId: 'thread-1',
          turnId: 'turn-1',
          ordinal: 3,
          text: '![HTTP output](http://images.example/output.png)',
        ),
        _threadItemFixture(
          id: 'assistant-file-image',
          threadId: 'thread-1',
          turnId: 'turn-1',
          ordinal: 4,
          text: '![File output](file:///tmp/output.png)',
        ),
        _threadItemFixture(
          id: 'assistant-data-image',
          threadId: 'thread-1',
          turnId: 'turn-1',
          ordinal: 5,
          text: '![Data output](data:image/png;base64,AAAA)',
        ),
        _threadItemFixture(
          id: 'user-remote-image',
          threadId: 'thread-1',
          turnId: 'turn-1',
          ordinal: 6,
          kind: ThreadItemKind.userMessage,
          channel: null,
          text: '![User image](https://images.example/user.png)',
        ),
      ];

      await tester.pumpWidget(
        _timelineHarness(
          threadId: 'thread-1',
          items: items,
          remoteImageProviderFactory: (url) {
            remoteLoads += 1;
            return _ImmediateTestImageProvider(url, testImage);
          },
        ),
      );
      await tester.pumpAndSettle();

      expect(
        find.byKey(StudioDriverKeys.markdownImageSource(remoteUrl)),
        findsOneWidget,
      );
      expect(remoteLoads, 0);
      expect(find.text('Local output'), findsOneWidget);
      expect(find.text('HTTP output'), findsOneWidget);
      expect(find.text('File output'), findsOneWidget);
      expect(find.text('Data output'), findsOneWidget);
      expect(find.text('User image'), findsOneWidget);
      expect(
        find.byKey(
          StudioDriverKeys.markdownImageSource(
            'https://images.example/user.png',
          ),
        ),
        findsNothing,
      );

      await tester.tap(
        find.byKey(StudioDriverKeys.markdownImageSource(remoteUrl)),
      );
      await tester.pumpAndSettle();
      expect(remoteLoads, 1);
      expect(
        find.byKey(
          StudioDriverKeys.markdownImageThumbnail(remoteUrl),
          skipOffstage: false,
        ),
        findsOneWidget,
      );
      expect(
        find.byKey(StudioDriverKeys.markdownImageDialog(remoteUrl)),
        findsOneWidget,
      );
    },
  );

  testWidgets('failed HTTPS markdown image stays a retryable source card', (
    tester,
  ) async {
    final validImage = (await tester.runAsync(
      () => createTestImage(width: 2, height: 2),
    ))!;
    var attempts = 0;
    const remoteUrl = 'https://images.example/retry.png';
    final item = _threadItemFixture(
      id: 'assistant-retry-image',
      threadId: 'thread-1',
      turnId: 'turn-1',
      ordinal: 1,
      text: '![Retry preview]($remoteUrl)',
    );

    await tester.pumpWidget(
      _timelineHarness(
        threadId: 'thread-1',
        items: [item],
        remoteImageProviderFactory: (url) {
          attempts += 1;
          return attempts == 1
              ? MemoryImage(Uint8List.fromList(const [1, 2, 3]))
              : _ImmediateTestImageProvider('retry-success', validImage);
        },
      ),
    );
    await tester.pumpAndSettle();

    await tester.tap(
      find.byKey(StudioDriverKeys.markdownImageSource(remoteUrl)),
    );
    await tester.pumpAndSettle();
    expect(attempts, 1);
    expect(find.text('Retry'), findsOneWidget);
    expect(
      find.byKey(StudioDriverKeys.markdownImageDialog(remoteUrl)),
      findsNothing,
    );

    await tester.tap(
      find.byKey(StudioDriverKeys.markdownImageSource(remoteUrl)),
    );
    await tester.pumpAndSettle();
    expect(attempts, 2);
    expect(
      find.byKey(StudioDriverKeys.markdownImageDialog(remoteUrl)),
      findsOneWidget,
    );
  });

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

  test('user, parent agent, commentary and final channels remain distinct', () {
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
        id: 'parent-agent',
        threadId: 'thread-1',
        turnId: 'turn-1',
        ordinal: 1,
        kind: ThreadItemKind.parentAgentMessage,
        channel: null,
        text: 'follow-up guidance',
      ),
      _threadItemFixture(
        id: 'commentary',
        threadId: 'thread-1',
        turnId: 'turn-1',
        ordinal: 2,
        channel: AgentMessageChannel.commentary,
        text: 'working',
      ),
      _threadItemFixture(
        id: 'final',
        threadId: 'thread-1',
        turnId: 'turn-1',
        ordinal: 3,
        channel: AgentMessageChannel.finalAnswer,
        text: 'done',
      ),
    ]);

    expect(rows.map((row) => row.type), [
      TimelineRowType.userMessage,
      TimelineRowType.parentAgentMessage,
      TimelineRowType.commentary,
      TimelineRowType.finalAnswer,
    ]);
  });

  testWidgets('parent agent message has its own label and hierarchy icon', (
    tester,
  ) async {
    final item = _threadItemFixture(
      id: 'parent-agent-message',
      threadId: 'child-thread',
      turnId: 'child-turn',
      ordinal: 1,
      kind: ThreadItemKind.parentAgentMessage,
      channel: null,
      text: 'Check the latest result.',
    );

    await tester.pumpWidget(
      _timelineHarness(threadId: 'child-thread', items: [item]),
    );
    await tester.pumpAndSettle();

    expect(find.text('Main agent'), findsOneWidget);
    expect(find.text('Check the latest result.'), findsOneWidget);
    expect(find.byIcon(Icons.account_tree_outlined), findsOneWidget);
    expect(find.byIcon(Icons.person_outline), findsNothing);
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
