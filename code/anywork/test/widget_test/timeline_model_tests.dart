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
  testWidgets('terminal turn failure remains visible after restoring history', (
    tester,
  ) async {
    final terminal = ThreadItemView(
      id: 'turn-failed',
      threadId: 'thread-1',
      turnId: 'turn-1',
      ordinal: 1,
      revision: 2,
      createdAt: _fixtureDate(1),
      updatedAt: _fixtureDate(2),
      state: const ThreadTurnItemStateView(
        FailedStudioTurnState(
          startedAt: 1,
          completedAt: 2,
          failure: StudioTurnFailureView(
            category: 'validation',
            providerKind: null,
            code: null,
            httpStatus: null,
            retryAfterMs: null,
            message: 'turn ended before its report',
            retryable: false,
          ),
        ),
      ),
    );
    final message = _threadItemFixture(
      id: 'answer',
      threadId: 'thread-1',
      turnId: 'turn-1',
      ordinal: 2,
      text: 'Generated report',
    );
    final metadata = {
      'turn-1': TimelineTurnView(
        turn: StudioTurnView(
          inputId: null,
          turnId: 'turn-1',
          threadId: 'thread-1',
          revision: terminal.revision,
          state: (terminal.state as ThreadTurnItemStateView).state,
          updatedAt: terminal.updatedAt,
        ),
        lastItemId: message.id,
      ),
    };
    expect(
      timelineRowsFromThreadItems(
        [terminal],
        turns: metadata,
      ).where((row) => row.type == TimelineRowType.turnOutcome),
      isEmpty,
    );
    await tester.pumpWidget(
      _timelineApp(
        home: Scaffold(
          body: TimelineView(
            threadId: 'thread-1',
            turn: null,
            rows: timelineRowsFromThreadItems([message], turns: metadata),
          ),
        ),
      ),
    );
    await tester.pumpAndSettle();
    expect(find.text('turn ended before its report'), findsOneWidget);
    expect(
      tester.getTopLeft(find.text('turn ended before its report')).dy,
      greaterThan(tester.getTopLeft(find.text('Generated report')).dy),
    );
  });

  testWidgets(
    'view_image shows a clickable read label and only loads bytes after inline expansion',
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

      final entryId = StudioDriverKeys.toolImageEntryId(
        'call-1',
        'tool-image-1',
      );

      // 默认只显示可点击的「已读取图片」+ 文件名/路径，不预加载图片字节。
      expect(find.text('Image read'), findsOneWidget);
      expect(find.text('Image read · PURE-7429.png'), findsOneWidget);
      expect(
        find.byKey(
          StudioDriverKeys.timelineToolGroupSummary(
            'tool-group:turn-1:view-image-tool-item',
          ),
        ),
        findsOneWidget,
      );
      expect(
        find.byKey(StudioDriverKeys.viewImageToggle(entryId)),
        findsOneWidget,
      );
      expect(
        find.byKey(StudioDriverKeys.viewImageThumbnail(entryId)),
        findsNothing,
      );
      expect(
        find.byKey(StudioDriverKeys.historyAttachment('tool-image-1')),
        findsNothing,
      );
      expect(api.readThreadAttachmentRequests, isEmpty);

      // 普通工具详情仍可在同一工具组内展开。
      await tester.tap(
        find.byKey(const ValueKey('timeline-tool-group-summary')),
      );
      await tester.pumpAndSettle();
      expect(
        find.byKey(StudioDriverKeys.viewImageTool('call-1')),
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
      expect(
        find.byKey(StudioDriverKeys.viewImageToggle(entryId)),
        findsOneWidget,
      );
      expect(api.readThreadAttachmentRequests, isEmpty);

      // 第一次展开才读取归档附件。
      await tester.tap(find.byKey(StudioDriverKeys.viewImageToggle(entryId)));
      await tester.pumpAndSettle();
      expect(
        find.byKey(StudioDriverKeys.viewImageThumbnail(entryId)),
        findsOneWidget,
      );
      expect(api.readThreadAttachmentRequests, [
        (threadId: 'thread-1', attachmentId: 'tool-image-1'),
      ]);

      // 再次点击收起，复用缓存不再读取。
      await tester.tap(find.byKey(StudioDriverKeys.viewImageToggle(entryId)));
      await tester.pumpAndSettle();
      expect(
        find.byKey(StudioDriverKeys.viewImageThumbnail(entryId)),
        findsNothing,
      );
      expect(api.readThreadAttachmentRequests, [
        (threadId: 'thread-1', attachmentId: 'tool-image-1'),
      ]);

      // 重新展开仍复用缓存。
      await tester.tap(find.byKey(StudioDriverKeys.viewImageToggle(entryId)));
      await tester.pumpAndSettle();
      expect(api.readThreadAttachmentRequests, [
        (threadId: 'thread-1', attachmentId: 'tool-image-1'),
      ]);

      // 展开后可进一步放大弹窗。
      await tester.tap(
        find.byKey(StudioDriverKeys.viewImageThumbnail(entryId)),
      );
      await tester.pumpAndSettle();
      expect(
        find.byKey(StudioDriverKeys.viewImageDialog(entryId)),
        findsOneWidget,
      );
      expect(find.byType(InteractiveViewer), findsOneWidget);
      await tester.tap(find.byKey(StudioDriverKeys.timelineImageClose));
      await tester.pumpAndSettle();
      expect(
        find.byKey(StudioDriverKeys.viewImageDialog(entryId)),
        findsNothing,
      );
    },
  );

  testWidgets(
    'image attachment without a filename uses the localized attachment fallback',
    (tester) async {
      const attachment = ThreadAttachmentView(
        id: 'tool-image-noname',
        modality: AttachmentModalityView.image,
        mediaType: 'image/png',
        byteSize: 68,
      );
      final item = _threadItemFixture(
        id: 'view-image-noname-item',
        threadId: 'thread-1',
        turnId: 'turn-1',
        ordinal: 1,
        kind: ThreadItemKind.toolCall,
        status: 'succeeded',
        channel: null,
        tool: const TimelineToolPart(
          toolCallId: 'tool-call-noname',
          callId: 'call-noname',
          name: 'view_image',
          result: '{"viewedImage":true}',
          attachments: [attachment],
        ),
      );
      final api = _FakeStudioApi(_emptyState())
        ..threadAttachmentBytes[(
          threadId: 'thread-1',
          attachmentId: 'tool-image-noname',
        )] = base64Decode(
          'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII=',
        );

      await tester.pumpWidget(
        _timelineHarness(
          threadId: 'thread-1',
          items: [item],
          api: api,
          locale: const Locale.fromSubtags(
            languageCode: 'zh',
            scriptCode: 'Hans',
          ),
        ),
      );
      await tester.pumpAndSettle();

      await tester.tap(
        find.byKey(
          StudioDriverKeys.viewImageToggle('call-noname:tool-image-noname'),
        ),
      );
      await tester.pumpAndSettle();

      expect(find.byTooltip('附件 · 68 B'), findsOneWidget);
      expect(find.byTooltip('Image · 68 B'), findsNothing);
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

    final entryId = StudioDriverKeys.toolImageEntryId(
      'call-failed',
      'tool-image-failed',
    );

    // 默认折叠不加载图片字节，也不冒充成功。
    expect(
      find.byKey(StudioDriverKeys.viewImageThumbnail(entryId)),
      findsNothing,
    );
    expect(api.readThreadAttachmentRequests, isEmpty);

    // 第一次展开才读取，失败以显式状态呈现。
    await tester.tap(find.byKey(StudioDriverKeys.viewImageToggle(entryId)));
    await tester.pumpAndSettle();
    expect(
      find.byKey(ValueKey('attachment-load-failed-$entryId')),
      findsOneWidget,
    );
    // 失败态不挂载已加载图片 key，Driver 无法把失败误判为已读取。
    expect(
      find.byKey(StudioDriverKeys.viewImageThumbnail(entryId)),
      findsNothing,
    );
    expect(find.byKey(StudioDriverKeys.viewImageDialog(entryId)), findsNothing);
    expect(api.readThreadAttachmentRequests, [
      (threadId: 'thread-1', attachmentId: 'tool-image-failed'),
    ]);

    api.threadAttachmentErrors.clear();
    api.threadAttachmentBytes[(
      threadId: 'thread-1',
      attachmentId: 'tool-image-failed',
    )] = base64Decode(
      'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII=',
    );
    await tester.tap(find.byKey(StudioDriverKeys.timelineImageRetry(entryId)));
    await tester.pumpAndSettle();
    expect(api.readThreadAttachmentRequests, [
      (threadId: 'thread-1', attachmentId: 'tool-image-failed'),
      (threadId: 'thread-1', attachmentId: 'tool-image-failed'),
    ]);
    expect(
      find.byKey(StudioDriverKeys.viewImageThumbnail(entryId)),
      findsOneWidget,
    );
  });

  testWidgets(
    'multiple tool images expand independently and share one loader',
    (tester) async {
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

      final entryA = StudioDriverKeys.toolImageEntryId(
        'tool-call-images',
        'tool-image-a',
      );
      final entryB = StudioDriverKeys.toolImageEntryId(
        'tool-call-images',
        'tool-image-b',
      );

      // 默认两个可点击文字入口，均未加载图片字节。
      expect(find.text('a.png'), findsOneWidget);
      expect(find.text('b.png'), findsOneWidget);
      expect(
        find.byKey(StudioDriverKeys.viewImageToggle(entryA)),
        findsOneWidget,
      );
      expect(
        find.byKey(StudioDriverKeys.viewImageToggle(entryB)),
        findsOneWidget,
      );
      expect(
        find.byKey(StudioDriverKeys.viewImageThumbnail(entryA)),
        findsNothing,
      );
      expect(
        find.byKey(StudioDriverKeys.viewImageThumbnail(entryB)),
        findsNothing,
      );
      expect(api.readThreadAttachmentRequests, isEmpty);

      Future<void> tapToggle(String entryId) async {
        await tester.tap(find.byKey(StudioDriverKeys.viewImageToggle(entryId)));
        await tester.pumpAndSettle();
      }

      // 独立展开 a：只加载 a，b 保持折叠。
      await tapToggle(entryA);
      expect(
        find.byKey(StudioDriverKeys.viewImageThumbnail(entryA)),
        findsOneWidget,
      );
      expect(
        find.byKey(StudioDriverKeys.viewImageThumbnail(entryB)),
        findsNothing,
      );
      expect(api.readThreadAttachmentRequests, [
        (threadId: 'thread-1', attachmentId: 'tool-image-a'),
      ]);

      // 收起 a，不再重复读取。
      await tapToggle(entryA);
      expect(
        find.byKey(StudioDriverKeys.viewImageThumbnail(entryA)),
        findsNothing,
      );
      expect(api.readThreadAttachmentRequests, [
        (threadId: 'thread-1', attachmentId: 'tool-image-a'),
      ]);

      // 重新展开 a，复用同一 loader 缓存不再读取。
      await tapToggle(entryA);
      expect(
        find.byKey(StudioDriverKeys.viewImageThumbnail(entryA)),
        findsOneWidget,
      );
      expect(api.readThreadAttachmentRequests.length, 1);

      // 再展开 b，a 保持展开，多图互不影响。
      await tapToggle(entryB);
      expect(
        find.byKey(StudioDriverKeys.viewImageThumbnail(entryA)),
        findsOneWidget,
      );
      expect(
        find.byKey(StudioDriverKeys.viewImageThumbnail(entryB)),
        findsOneWidget,
      );
      expect(api.readThreadAttachmentRequests, [
        (threadId: 'thread-1', attachmentId: 'tool-image-a'),
        (threadId: 'thread-1', attachmentId: 'tool-image-b'),
      ]);
    },
  );

  testWidgets(
    'two view_image calls reading the same resource keep independent entries and share bytes',
    (tester) async {
      const attachment = ThreadAttachmentView(
        id: 'shared-image',
        modality: AttachmentModalityView.image,
        mediaType: 'image/png',
        filename: 'shared.png',
        byteSize: 68,
        width: 1,
        height: 1,
      );
      final first = _threadItemFixture(
        id: 'view-image-first',
        threadId: 'thread-1',
        turnId: 'turn-1',
        ordinal: 1,
        kind: ThreadItemKind.toolCall,
        status: 'succeeded',
        channel: null,
        tool: const TimelineToolPart(
          toolCallId: 'tool-call-first',
          callId: 'call-first',
          name: 'view_image',
          attachments: [attachment],
        ),
      );
      final second = _threadItemFixture(
        id: 'view-image-second',
        threadId: 'thread-1',
        turnId: 'turn-1',
        ordinal: 2,
        kind: ThreadItemKind.toolCall,
        status: 'succeeded',
        channel: null,
        tool: const TimelineToolPart(
          toolCallId: 'tool-call-second',
          callId: 'call-second',
          name: 'view_image',
          attachments: [attachment],
        ),
      );
      final api = _FakeStudioApi(_emptyState())
        ..threadAttachmentBytes[(
          threadId: 'thread-1',
          attachmentId: 'shared-image',
        )] = base64Decode(
          'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII=',
        );

      await tester.pumpWidget(
        _timelineHarness(
          threadId: 'thread-1',
          items: [first, second],
          api: api,
          height: 1000,
        ),
      );
      await tester.pumpAndSettle();

      final entryFirst = StudioDriverKeys.toolImageEntryId(
        'call-first',
        'shared-image',
      );
      final entrySecond = StudioDriverKeys.toolImageEntryId(
        'call-second',
        'shared-image',
      );
      final toggleFirst = find.byKey(
        StudioDriverKeys.viewImageToggle(entryFirst),
      );
      final toggleSecond = find.byKey(
        StudioDriverKeys.viewImageToggle(entrySecond),
      );

      // 同一资源在两个调用下各自保留独立文字入口，不静默合并、不重复 key。
      expect(find.text('Image read · shared.png'), findsNWidgets(2));
      expect(toggleFirst, findsOneWidget);
      expect(toggleSecond, findsOneWidget);
      expect(
        find.byKey(StudioDriverKeys.viewImageThumbnail(entryFirst)),
        findsNothing,
      );
      expect(
        find.byKey(StudioDriverKeys.viewImageThumbnail(entrySecond)),
        findsNothing,
      );

      // 展开第一个调用：仅该条目展开，图片锚定在自身文字入口下方。
      await tester.tap(toggleFirst);
      await tester.pumpAndSettle();
      expect(
        find.byKey(StudioDriverKeys.viewImageThumbnail(entryFirst)),
        findsOneWidget,
      );
      expect(
        find.byKey(StudioDriverKeys.viewImageThumbnail(entrySecond)),
        findsNothing,
      );
      expect(
        tester
            .getTopLeft(
              find.byKey(StudioDriverKeys.viewImageThumbnail(entryFirst)),
            )
            .dy,
        lessThan(tester.getTopLeft(toggleSecond).dy),
      );

      // 展开第二个调用：第一个保持展开，展开态彼此独立。
      await tester.tap(toggleSecond);
      await tester.pumpAndSettle();
      expect(
        find.byKey(StudioDriverKeys.viewImageThumbnail(entryFirst)),
        findsOneWidget,
      );
      expect(
        find.byKey(StudioDriverKeys.viewImageThumbnail(entrySecond)),
        findsOneWidget,
      );

      // 字节按 threadId + attachmentId 去重：两个条目共享一次读取。
      expect(api.readThreadAttachmentRequests, [
        (threadId: 'thread-1', attachmentId: 'shared-image'),
      ]);
    },
  );

  testWidgets(
    'two view_image calls reading the same failed resource retry independently',
    (tester) async {
      const attachment = ThreadAttachmentView(
        id: 'shared-image',
        modality: AttachmentModalityView.image,
        mediaType: 'image/png',
        filename: 'shared.png',
        byteSize: 68,
        width: 1,
        height: 1,
      );
      final first = _threadItemFixture(
        id: 'view-image-first',
        threadId: 'thread-1',
        turnId: 'turn-1',
        ordinal: 1,
        kind: ThreadItemKind.toolCall,
        status: 'succeeded',
        channel: null,
        tool: const TimelineToolPart(
          toolCallId: 'tool-call-first',
          callId: 'call-first',
          name: 'view_image',
          attachments: [attachment],
        ),
      );
      final second = _threadItemFixture(
        id: 'view-image-second',
        threadId: 'thread-1',
        turnId: 'turn-1',
        ordinal: 2,
        kind: ThreadItemKind.toolCall,
        status: 'succeeded',
        channel: null,
        tool: const TimelineToolPart(
          toolCallId: 'tool-call-second',
          callId: 'call-second',
          name: 'view_image',
          attachments: [attachment],
        ),
      );
      final api = _FakeStudioApi(_emptyState())
        ..threadAttachmentErrors[(
          threadId: 'thread-1',
          attachmentId: 'shared-image',
        )] = StateError(
          'attachment lease expired',
        );

      await tester.pumpWidget(
        _timelineHarness(
          threadId: 'thread-1',
          items: [first, second],
          api: api,
          height: 1000,
        ),
      );
      await tester.pumpAndSettle();

      final entryFirst = StudioDriverKeys.toolImageEntryId(
        'call-first',
        'shared-image',
      );
      final entrySecond = StudioDriverKeys.toolImageEntryId(
        'call-second',
        'shared-image',
      );

      await tester.tap(
        find.byKey(StudioDriverKeys.viewImageToggle(entryFirst)),
      );
      await tester.pumpAndSettle();
      await tester.tap(
        find.byKey(StudioDriverKeys.viewImageToggle(entrySecond)),
      );
      await tester.pumpAndSettle();

      // 同一资源在两个调用下各自失败：失败与重试 key 必须叠加 entryId，
      // 不能共用附件 id，否则全树出现重复 ValueKey、Driver 无法唯一定位。
      expect(
        find.byKey(ValueKey('attachment-load-failed-$entryFirst')),
        findsOneWidget,
      );
      expect(
        find.byKey(ValueKey('attachment-load-failed-$entrySecond')),
        findsOneWidget,
      );
      expect(
        find.byKey(StudioDriverKeys.timelineImageRetry(entryFirst)),
        findsOneWidget,
      );
      expect(
        find.byKey(StudioDriverKeys.timelineImageRetry(entrySecond)),
        findsOneWidget,
      );
      // 失败按 threadId + attachmentId 共享：两个条目共用一次读取。
      expect(api.readThreadAttachmentRequests, [
        (threadId: 'thread-1', attachmentId: 'shared-image'),
      ]);

      // 修复共享资源后只重试第一个条目：它转为已读取，另一个仍保留自身失败态。
      api.threadAttachmentErrors.clear();
      api.threadAttachmentBytes[(
        threadId: 'thread-1',
        attachmentId: 'shared-image',
      )] = base64Decode(
        'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII=',
      );
      await tester.tap(
        find.byKey(StudioDriverKeys.timelineImageRetry(entryFirst)),
      );
      await tester.pumpAndSettle();
      expect(
        find.byKey(StudioDriverKeys.viewImageThumbnail(entryFirst)),
        findsOneWidget,
      );
      expect(
        find.byKey(ValueKey('attachment-load-failed-$entrySecond')),
        findsOneWidget,
      );
      expect(
        find.byKey(StudioDriverKeys.viewImageThumbnail(entrySecond)),
        findsNothing,
      );
    },
  );

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

  testWidgets('timeline projects ThreadItems by immutable ordinal', (
    tester,
  ) async {
    final items = [
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
    ];

    await tester.pumpWidget(
      _timelineHarness(threadId: 'thread-1', items: items),
    );
    await tester.pumpAndSettle();
    expect(
      tester.getTopLeft(find.text('earlier')).dy,
      lessThan(tester.getTopLeft(find.text('later')).dy),
    );
  });

  testWidgets(
    'user, parent agent, commentary and final channels remain distinct',
    (tester) async {
      final items = [
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
      ];

      await tester.pumpWidget(
        _timelineHarness(threadId: 'thread-1', items: items),
      );
      await tester.pumpAndSettle();
      for (final text in [
        'prompt',
        'follow-up guidance',
        'working',
        'done',
        'Main agent',
      ]) {
        expect(find.text(text), findsOneWidget);
      }
      for (final item in items) {
        expect(
          find.byKey(StudioDriverKeys.timelineRow(item.id)),
          findsOneWidget,
        );
      }
    },
  );

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
    expect(
      find.byKey(StudioDriverKeys.parentAgentLabel(item.id)),
      findsOneWidget,
    );
    expect(find.text('Check the latest result.'), findsOneWidget);
    expect(find.byIcon(Icons.account_tree_outlined), findsOneWidget);
    expect(find.byIcon(Icons.person_outline), findsNothing);
  });

  testWidgets('tool grouping stops at a message boundary', (tester) async {
    final items = [
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
    ];

    await tester.pumpWidget(
      _timelineHarness(threadId: 'thread-1', items: items),
    );
    await tester.pumpAndSettle();
    expect(find.text('next'), findsOneWidget);
    expect(
      find.byKey(const ValueKey('timeline-tool-group-summary')),
      findsNWidgets(2),
    );
  });

  testWidgets('adjacent reasoning Items become one reasoning row', (
    tester,
  ) async {
    final items = [
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
    ];

    await tester.pumpWidget(
      _timelineHarness(threadId: 'thread-1', items: items),
    );
    await tester.pumpAndSettle();
    await tester.tap(find.byIcon(Icons.psychology_alt_outlined));
    await tester.pumpAndSettle();
    expect(find.textContaining('summary'), findsWidgets);
    expect(find.textContaining('details'), findsWidgets);
  });

  testWidgets('file Items stay outside the transcript timeline', (
    tester,
  ) async {
    final items = [
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
    ];

    await tester.pumpWidget(
      _timelineHarness(threadId: 'thread-1', items: items),
    );
    await tester.pumpAndSettle();
    expect(find.text('report.md'), findsNothing);
    expect(tester.takeException(), isNull);
  });

  testWidgets('repeated Skill Items remain independent timeline rows', (
    tester,
  ) async {
    final items = [
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
    ];

    await tester.pumpWidget(
      _timelineHarness(threadId: 'thread-1', items: items),
    );
    await tester.pumpAndSettle();
    expect(find.text('Agent activated skill · pdf'), findsNWidgets(2));
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

  testWidgets(
    'a previewed item renders a visible load path that requests its full body',
    (tester) async {
      final item = _threadItemFixture(
        id: 'bulk-1',
        threadId: 'thread-1',
        turnId: 'turn-1',
        ordinal: 1,
        text: 'preview…',
      );
      String? requested;

      await tester.pumpWidget(
        _timelineHarness(
          threadId: 'thread-1',
          items: [item],
          previewedItemIds: const {'bulk-1'},
          onLoadItemBody: (itemId) => requested = itemId,
        ),
      );
      await tester.pumpAndSettle();

      // 预览预算截断的条目在真实行渲染里给出显式回源入口，并保持阅读位置。
      expect(
        find.byKey(StudioDriverKeys.timelineItemBodyNotice('bulk-1')),
        findsOneWidget,
      );
      expect(
        find.text('This page shows a truncated preview of a large item.'),
        findsOneWidget,
      );
      await tester.tap(
        find.byKey(StudioDriverKeys.timelineItemBodyLoad('bulk-1')),
      );
      await tester.pump();
      expect(requested, 'bulk-1');
    },
  );

  testWidgets('an in-flight item body load shows a loading notice', (
    tester,
  ) async {
    final item = _threadItemFixture(
      id: 'bulk-2',
      threadId: 'thread-1',
      turnId: 'turn-1',
      ordinal: 1,
      text: 'preview…',
    );

    await tester.pumpWidget(
      _timelineHarness(
        threadId: 'thread-1',
        items: [item],
        previewedItemIds: const {'bulk-2'},
        loadingItemIds: const {'bulk-2'},
        onLoadItemBody: (_) {},
      ),
    );
    // 加载态含持续动画，不能用 pumpAndSettle。
    await tester.pump();
    await tester.pump(const Duration(milliseconds: 200));

    expect(find.text('Loading full content…'), findsOneWidget);
    expect(
      find.byKey(StudioDriverKeys.timelineItemBodyLoad('bulk-2')),
      findsNothing,
    );
    // 卸载持续动画，避免测试结束时有活动 Ticker。
    await tester.pumpWidget(const SizedBox.shrink());
    await tester.pump();
  });

  testWidgets('a failed item body load shows the error with a retry path', (
    tester,
  ) async {
    final item = _threadItemFixture(
      id: 'bulk-3',
      threadId: 'thread-1',
      turnId: 'turn-1',
      ordinal: 1,
      text: 'preview…',
    );
    var retried = false;

    await tester.pumpWidget(
      _timelineHarness(
        threadId: 'thread-1',
        items: [item],
        previewedItemIds: const {'bulk-3'},
        itemBodyErrors: const {'bulk-3': 'body unavailable'},
        onLoadItemBody: (_) => retried = true,
      ),
    );
    await tester.pumpAndSettle();

    expect(find.text('body unavailable'), findsOneWidget);
    await tester.tap(
      find.byKey(StudioDriverKeys.timelineItemBodyRetry('bulk-3')),
    );
    await tester.pump();
    expect(retried, isTrue);
  });

  testWidgets('an unavailable item body offers no retry that cannot succeed', (
    tester,
  ) async {
    final item = _threadItemFixture(
      id: 'bulk-4',
      threadId: 'thread-1',
      turnId: 'turn-1',
      ordinal: 1,
      text: 'preview…',
    );

    await tester.pumpWidget(
      _timelineHarness(
        threadId: 'thread-1',
        items: [item],
        previewedItemIds: const {'bulk-4'},
        unavailableItemIds: const {'bulk-4'},
        onLoadItemBody: (_) {},
      ),
    );
    await tester.pumpAndSettle();

    expect(
      find.text('Full content is unavailable from this data source.'),
      findsOneWidget,
    );
    expect(
      find.byKey(StudioDriverKeys.timelineItemBodyLoad('bulk-4')),
      findsNothing,
    );
    expect(
      find.byKey(StudioDriverKeys.timelineItemBodyRetry('bulk-4')),
      findsNothing,
    );
  });

  testWidgets(
    'a previewed tool result loads by canonical item id through its group row',
    (tester) async {
      // 同一 Turn 的相邻工具调用被投影成一个合成身份的分组行；回源入口必须按底层
      // item id 暴露，否则超大工具输出永远打不开。
      final first = _threadItemFixture(
        id: 'tool-item-1',
        threadId: 'thread-1',
        turnId: 'turn-1',
        ordinal: 1,
        kind: ThreadItemKind.toolCall,
        tool: const TimelineToolPart(
          toolCallId: 'tool-call-1',
          callId: 'call-1',
          name: 'read_file',
          result: '…[truncated]',
        ),
      );
      final second = _threadItemFixture(
        id: 'tool-item-2',
        threadId: 'thread-1',
        turnId: 'turn-1',
        ordinal: 2,
        kind: ThreadItemKind.toolCall,
        tool: const TimelineToolPart(
          toolCallId: 'tool-call-2',
          callId: 'call-2',
          name: 'list_dir',
          result: 'ok',
        ),
      );
      String? requested;

      await tester.pumpWidget(
        _timelineHarness(
          threadId: 'thread-1',
          items: [first, second],
          previewedItemIds: const {'tool-item-1'},
          onLoadItemBody: (itemId) => requested = itemId,
        ),
      );
      await tester.pumpAndSettle();

      expect(
        find.byKey(StudioDriverKeys.timelineItemBodyNotice('tool-item-1')),
        findsOneWidget,
      );
      // 数据来源标签区分同组内的多条回源入口；分组展开不承担完整正文检索。
      expect(
        find.text(
          'read_file · This page shows a truncated preview of a large item.',
        ),
        findsOneWidget,
      );
      expect(
        find.byKey(StudioDriverKeys.timelineItemBodyNotice('tool-item-2')),
        findsNothing,
      );

      await tester.tap(
        find.byKey(StudioDriverKeys.timelineItemBodyLoad('tool-item-1')),
      );
      await tester.pump();
      expect(requested, 'tool-item-1');
    },
  );

  testWidgets(
    'a previewed reasoning body loads by canonical item id through its group row',
    (tester) async {
      final first = _threadItemFixture(
        id: 'reason-item-1',
        threadId: 'thread-1',
        turnId: 'turn-1',
        ordinal: 1,
        kind: ThreadItemKind.reasoning,
        reasoningContent: const ['short'],
      );
      final second = _threadItemFixture(
        id: 'reason-item-2',
        threadId: 'thread-1',
        turnId: 'turn-1',
        ordinal: 2,
        kind: ThreadItemKind.reasoning,
        reasoningContent: const ['…[truncated]'],
      );
      String? requested;

      await tester.pumpWidget(
        _timelineHarness(
          threadId: 'thread-1',
          items: [first, second],
          previewedItemIds: const {'reason-item-2'},
          onLoadItemBody: (itemId) => requested = itemId,
        ),
      );
      await tester.pumpAndSettle();

      expect(
        find.byKey(StudioDriverKeys.timelineItemBodyNotice('reason-item-2')),
        findsOneWidget,
      );
      expect(
        find.byKey(StudioDriverKeys.timelineItemBodyNotice('reason-item-1')),
        findsNothing,
      );

      await tester.tap(
        find.byKey(StudioDriverKeys.timelineItemBodyLoad('reason-item-2')),
      );
      await tester.pump();
      expect(requested, 'reason-item-2');
    },
  );

  testWidgets('a previewed raw payload exposes the full-body path', (
    tester,
  ) async {
    final raw =
        _threadItemFixture(
          id: 'raw-bulk',
          threadId: 'thread-1',
          turnId: 'turn-1',
          ordinal: 1,
        ).copyWith(
          state: ThreadRawItemStateView(
            const [RawHistoryPayload('future.payload', 99, '…[truncated]')],
            'Unsupported saved format',
            DateTime.fromMillisecondsSinceEpoch(1000),
          ),
        );
    String? requested;

    await tester.pumpWidget(
      _timelineHarness(
        threadId: 'thread-1',
        items: [raw],
        previewedItemIds: const {'raw-bulk'},
        onLoadItemBody: (itemId) => requested = itemId,
      ),
    );
    await tester.pumpAndSettle();

    // raw 行仍以 ExpansionTile 呈现已截断载荷；完整正文入口独立于展开。
    expect(find.byKey(const ValueKey('raw-history-raw-bulk')), findsOneWidget);
    expect(
      find.byKey(StudioDriverKeys.timelineItemBodyNotice('raw-bulk')),
      findsOneWidget,
    );

    await tester.tap(
      find.byKey(StudioDriverKeys.timelineItemBodyLoad('raw-bulk')),
    );
    await tester.pump();
    expect(requested, 'raw-bulk');
  });
}
