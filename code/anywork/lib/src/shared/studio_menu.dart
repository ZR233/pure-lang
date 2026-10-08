import 'package:flutter/material.dart';
import 'package:flutter/services.dart';

import 'studio_anchored_surface.dart';

/// One row of a [StudioMenu].
///
/// [value] is delivered to [StudioMenu.onSelected] when the row is activated.
/// It may be null when the generic type argument is nullable, so an explicit
/// "no value" choice stays selectable. Rows that only present information
/// (group headings, unavailable placeholders) are built with
/// [StudioMenuItem.header] and never activate.
class StudioMenuItem<T> {
  const StudioMenuItem({
    required this.child,
    this.value,
    this.enabled = true,
    this.selected = false,
    this.itemKey,
    this.tooltip,
  });

  /// A non-activating row such as a group heading or placeholder.
  const StudioMenuItem.header({required Widget child, Key? itemKey})
    : this(child: child, enabled: false, itemKey: itemKey);

  /// The value reported to [StudioMenu.onSelected]; may be null for nullable
  /// selection types.
  final T? value;

  /// Whether the row can be activated.
  final bool enabled;

  /// Marks the row as the current choice. Adds a selected background,
  /// selected semantics, and keeps the row visible when a long menu opens;
  /// the row may render an additional hint of its own.
  final bool selected;

  /// Stable interaction key for the row.
  final Key? itemKey;

  /// Tooltip covering the whole row button; null omits the tooltip.
  final String? tooltip;

  /// Row content; owns its own selected and disabled presentation.
  final Widget child;
}

typedef StudioMenuItemBuilder<T> = List<StudioMenuItem<T>> Function(
  BuildContext context,
);

/// Keyboard intents the menu's own trigger answers.
///
/// Escape requests a dismiss; the arrows are the menu's directional entry
/// points (handled by [_MenuDirectionalFocusAction]). The shared anchored
/// surface deliberately installs none of these: free-content surfaces keep
/// their own keyboard and traversal semantics.
const Map<ShortcutActivator, Intent> _menuTriggerShortcuts =
    <ShortcutActivator, Intent>{
      SingleActivator(LogicalKeyboardKey.escape): DismissIntent(),
      SingleActivator(LogicalKeyboardKey.arrowDown): DirectionalFocusIntent(
        TraversalDirection.down,
      ),
      SingleActivator(LogicalKeyboardKey.arrowUp): DirectionalFocusIntent(
        TraversalDirection.up,
      ),
      SingleActivator(LogicalKeyboardKey.arrowLeft): DirectionalFocusIntent(
        TraversalDirection.left,
      ),
      SingleActivator(LogicalKeyboardKey.arrowRight): DirectionalFocusIntent(
        TraversalDirection.right,
      ),
    };

/// Trigger-facing controller over a [StudioMenu].
///
/// Opening routes through the shared anchored surface so every trigger uses
/// the same anchoring and focus contract; there is no direct [MenuController]
/// access that could bypass it. [open] with `focusTrigger: false` keeps focus
/// where it is, for hover-driven entry.
abstract class StudioMenuController {
  /// Whether the menu is currently open.
  bool get isOpen;

  /// Opens the menu; a no-op while open, while disabled, when there are no
  /// rows or when the trigger has no usable anchoring space on screen.
  /// `focusTrigger` decides whether the trigger takes focus for keyboard
  /// navigation.
  void open({bool focusTrigger = true});

  /// Closes the menu.
  void close();

  /// Toggles the menu open or closed.
  void toggle();

  /// Consumes the one-shot auto-open suppression armed when closing restored
  /// focus to the menu's trigger.
  ///
  /// Triggers that auto-open on keyboard focus should consult this in their
  /// focus listener: the restore itself would otherwise look like keyboard
  /// focus and immediately re-open the menu. Returns true at most once per
  /// restore; every other focus gain keeps the trigger's own behavior.
  bool consumeAutoOpenSuppression();
}

typedef StudioMenuTriggerBuilder = Widget Function(
  BuildContext context,
  StudioMenuController controller,
  bool canOpen,
);

/// Shared anchored button menu for Studio.
///
/// The menu rides on the single Raw-driven anchored surface: the framework
/// primitive owns the overlay lifecycle (attach, outside taps, ancestor
/// scrolls, view-size changes, dispose) and reports the real anchor bounds
/// every frame, while the shared layout layer derives the surface constraints
/// and position from the actual laid-out content size in the same layout
/// pass. The menu aligns to the leading text-direction edge of the trigger,
/// keeps a small gap, opens below first and flips above only when the real
/// content does not fit below; the window keeps a fixed margin on every side
/// in both directions. Opening is refused while disabled, while there are no
/// rows or while the trigger has no usable side, and long menus bound to the
/// available side with a visible scrollbar instead of covering the trigger.
///
/// Rows keep their stable [StudioMenuItem.itemKey]s, group structure and
/// selected hints; opening scrolls the selected row into view without moving
/// the menu itself. Keyboard entry focuses the real trigger, arrows and Tab
/// navigate the rows, Enter/Space and clicks select, and Escape, outside taps
/// and selection close with focus restored to the trigger without bouncing
/// the menu open again. The widget only presents choices and reports
/// selection; it never owns business state.
class StudioMenu<T> extends StatefulWidget {
  /// Quiet-label trigger styled by [StudioMenuLabel] or similar content.
  const StudioMenu({
    required String this.tooltip,
    required this.itemBuilder,
    required this.child,
    this.onSelected,
    this.enabled = true,
    this.onBlockedTap,
    this.menuConstraints,
    this.onBeforeOpen,
    this.onOpen,
    this.onClose,
    super.key,
  }) : triggerBuilder = null,
       childFocusNode = null;

  /// Fully custom trigger; the builder owns hover and semantics, receives the
  /// shared controller and whether the menu can currently open, and should
  /// attach [childFocusNode] to its real focusable widget.
  const StudioMenu.custom({
    required this.triggerBuilder,
    required this.itemBuilder,
    this.onSelected,
    this.enabled = true,
    this.menuConstraints,
    this.childFocusNode,
    this.onBeforeOpen,
    this.onOpen,
    this.onClose,
    super.key,
  }) : tooltip = null,
       child = null,
       onBlockedTap = null;

  /// Tooltip and semantics label for the quiet-label trigger.
  final String? tooltip;

  /// Builds the current rows; called on every rebuild so open menus refresh.
  final StudioMenuItemBuilder<T> itemBuilder;

  /// Reported with the activated row's [StudioMenuItem.value].
  final ValueChanged<T>? onSelected;

  /// Quiet-label trigger content; wrapped with tooltip, semantics, hover and
  /// focus handling.
  final Widget? child;

  /// Custom trigger builder used instead of [child].
  final StudioMenuTriggerBuilder? triggerBuilder;

  /// Whether rows can be activated and the quiet trigger can open the menu.
  final bool enabled;

  /// Invoked instead of opening when [enabled] is false.
  final VoidCallback? onBlockedTap;

  /// Size bounds for the menu panel; further clamped to the measured space
  /// beside the trigger on every open and relayout.
  final BoxConstraints? menuConstraints;

  /// Focus node of the custom trigger's real focusable widget; keyboard opens
  /// focus it and closing the menu restores focus to it (see
  /// [StudioMenuController.consumeAutoOpenSuppression]). Hover opens pass
  /// `focusTrigger: false` and leave focus alone.
  final FocusNode? childFocusNode;

  /// Called synchronously before an enabled, non-empty menu attempts to open.
  ///
  /// Owners can close a mutually exclusive parent detail here, before creating
  /// this menu's overlay. The attempt can still be refused for insufficient
  /// anchoring space; use [onOpen] and [onClose] for actual session state.
  final VoidCallback? onBeforeOpen;

  /// Called after the menu opens.
  final VoidCallback? onOpen;

  /// Called after the menu closes.
  final VoidCallback? onClose;

  /// Vertical gap between the trigger and the menu in both directions.
  static const Offset gapOffset = StudioAnchoredSurface.gap;

  @override
  State<StudioMenu<T>> createState() => _StudioMenuState<T>();
}

class _StudioMenuState<T> extends State<StudioMenu<T>>
    implements StudioMenuController {
  final GlobalKey<StudioAnchoredSurfaceState> _surfaceKey = GlobalKey(
    debugLabel: 'studio-menu-surface',
  );
  final FocusNode _triggerFocusNode = FocusNode(
    debugLabel: 'studio-menu-trigger',
  );
  final ScrollController _scrollController = ScrollController(
    debugLabel: 'studio-menu-rows',
  );
  final FocusNode _rowsAnchorNode = FocusNode(
    debugLabel: 'studio-menu-rows-anchor',
    skipTraversal: true,
    canRequestFocus: false,
  );
  final Map<Object?, GlobalKey> _rowScrollKeys = {};
  GlobalKey? _selectedScrollKey;
  bool _hasItems = false;
  List<Object?>? _currentRowIds;
  List<Object?>? _openRowIds;

  StudioAnchoredSurfaceState? get _surface => _surfaceKey.currentState;

  bool get _canOpen => widget.enabled && _hasItems;

  @override
  void dispose() {
    _triggerFocusNode.dispose();
    _scrollController.dispose();
    _rowsAnchorNode.dispose();
    super.dispose();
  }

  @override
  bool get isOpen => _surface?.isOpen ?? false;

  @override
  void open({bool focusTrigger = true}) {
    if (!_canOpen) {
      return;
    }
    widget.onBeforeOpen?.call();
    if (!mounted || !_canOpen) return;
    _surface?.open(focusTrigger: focusTrigger);
  }

  @override
  void close() => _surface?.close();

  @override
  void toggle() {
    if (isOpen) {
      close();
    } else {
      open();
    }
  }

  @override
  bool consumeAutoOpenSuppression() =>
      _surface?.consumeAutoOpenSuppression() ?? false;

  @override
  Widget build(BuildContext context) {
    final items = widget.itemBuilder(context);
    _hasItems = items.isNotEmpty;
    // Identities of the rows this build produced, saved on every build so
    // the open-session baseline can be captured atomically when the overlay
    // reports the open — not on the first build while open, which may
    // already be an external update that removed rows.
    _currentRowIds = [for (final item in items) item.itemKey ?? item.value];
    var rowsValid = true;
    final openIds = _openRowIds;
    if (openIds != null) {
      rowsValid = _noRowRemoved(openIds, _currentRowIds!);
    }
    return StudioAnchoredSurface(
      key: _surfaceKey,
      enabled: _canOpen,
      valid: _canOpen && rowsValid,
      triggerBuilder: _buildTrigger,
      contentBuilder: (context) => _buildRows(context, items),
      userConstraints: widget.menuConstraints,
      focusTriggerNode: widget.triggerBuilder == null
          ? _triggerFocusNode
          : widget.childFocusNode,
      onOpen: _handleOpened,
      onClose: _handleClosed,
    );
  }

  /// Whether every identity of the open session still exists in [rowIds].
  ///
  /// Removing a real row (including the case where the focused row is
  /// removed by a canonical update) invalidates the open menu: focus would
  /// fall out of the surface while the overlay stays open. Purely
  /// presentational updates — labels, selected hints, reordered rows with
  /// stable keys, newly added rows — keep the menu open; focus follows the
  /// stable row keys.
  bool _noRowRemoved(List<Object?> openIds, List<Object?> rowIds) {
    final remaining = [...rowIds];
    for (final id in openIds) {
      final index = remaining.indexOf(id);
      if (index < 0) {
        return false;
      }
      remaining.removeAt(index);
    }
    return true;
  }

  Widget _buildTrigger(BuildContext context) {
    Widget trigger;
    final triggerBuilder = widget.triggerBuilder;
    if (triggerBuilder != null) {
      trigger = triggerBuilder(context, this, _canOpen);
    } else {
      trigger = _buildLabelTrigger();
    }
    // Menu-owned keyboard entry: the trigger carries the menu's Escape and
    // directional entry points. The shared anchored surface deliberately
    // installs none of these, so free-content surfaces keep their own
    // keyboard and traversal semantics.
    return Shortcuts(
      includeSemantics: false,
      shortcuts: _menuTriggerShortcuts,
      child: Actions(
        actions: <Type, Action<Intent>>{
          DismissIntent: CallbackAction<DismissIntent>(
            onInvoke: (_) {
              _surface?.close(reason: StudioSurfaceCloseReason.keyboardDismiss);
              return null;
            },
          ),
          DirectionalFocusIntent: _MenuDirectionalFocusAction(_rowsAnchorNode),
        },
        child: trigger,
      ),
    );
  }

  Widget _buildLabelTrigger() {
    final tooltip = widget.tooltip!;
    return Tooltip(
      message: tooltip,
      child: Semantics(
        button: true,
        enabled: widget.enabled || widget.onBlockedTap != null,
        label: tooltip,
        child: Material(
          color: Colors.transparent,
          child: InkWell(
            borderRadius: BorderRadius.circular(6),
            focusNode: _triggerFocusNode,
            onTap: _canOpen ? toggle : widget.onBlockedTap,
            child: ExcludeFocus(child: widget.child!),
          ),
        ),
      ),
    );
  }

  Widget _buildRows(BuildContext context, List<StudioMenuItem<T>> items) {
    final duplicated = _duplicatedIdentities(items);
    final wrapperKeys = [
      for (final item in items) _rowWrapperKey(item, duplicated),
    ];
    final scrollKeys = [
      for (final item in items) _rowScrollKey(item, duplicated),
    ];
    // The scroll target is the selected row's own constant key, not its
    // business identity: a legitimately null identity (an explicit "no
    // choice" row) has a key like any other unique row, while no selection
    // or a selected row without a unique identity keeps this null.
    _selectedScrollKey = null;
    for (var i = 0; i < items.length; i++) {
      if (items[i].selected && scrollKeys[i] != null) {
        _selectedScrollKey = scrollKeys[i];
        break;
      }
    }
    return FocusTraversalGroup(
      child: Actions(
        actions: <Type, Action<Intent>>{
          DirectionalFocusIntent: _MenuDirectionalFocusAction(_rowsAnchorNode),
        },
        child: Focus(
          focusNode: _rowsAnchorNode,
          skipTraversal: true,
          canRequestFocus: false,
          includeSemantics: false,
          child: Scrollbar(
            controller: _scrollController,
            thumbVisibility: true,
            child: SingleChildScrollView(
              controller: _scrollController,
              child: IntrinsicWidth(
                child: Column(
                  mainAxisSize: MainAxisSize.min,
                  crossAxisAlignment: CrossAxisAlignment.stretch,
                  children: [
                    for (var i = 0; i < items.length; i++)
                      _buildItem(
                        context,
                        items[i],
                        wrapperKeys[i],
                        scrollKeys[i],
                      ),
                  ],
                ),
              ),
            ),
          ),
        ),
      ),
    );
  }

  /// Stable wrapper key per row, aligned with the identity contract
  /// ([StudioMenuItem.itemKey], falling back to [StudioMenuItem.value]).
  ///
  /// A row whose identity appears more than once in the list falls back to
  /// positional identity instead of crashing the list with duplicate keys.
  /// A unique identity is stable even when the value is null (a legitimate
  /// "no choice" row); rows with a unique identity keep their element — and
  /// with it their focus and state — across reordering, selected hints and
  /// label refreshes; only a real identity change replaces a row.
  Set<Object?> _duplicatedIdentities(List<StudioMenuItem<T>> items) {
    final duplicated = <Object?>{};
    final seen = <Object?>{};
    for (final item in items) {
      final identity = item.itemKey ?? item.value;
      if (!seen.add(identity)) {
        duplicated.add(identity);
      }
    }
    return duplicated;
  }

  Key? _rowWrapperKey(StudioMenuItem<T> item, Set<Object?> duplicated) {
    final identity = item.itemKey ?? item.value;
    if (duplicated.contains(identity)) {
      return null;
    }
    final itemKey = item.itemKey;
    if (itemKey != null) {
      return itemKey;
    }
    // A unique identity is a stable key even for a null value: selecting
    // null is a legitimate choice, not a dismissal, so such a row must not
    // fall back to positional identity either.
    return ValueKey<T?>(item.value);
  }

  /// Stable per-identity scrolling anchor for one row.
  ///
  /// Unlike the old single selected-row key that moved between rows, every
  /// uniquely identified row owns a constant anchor from its first build,
  /// so the row's widget topology never changes when the selected hint
  /// moves between rows. Duplicated identities get no anchor (they also
  /// have no wrapper key) instead of sharing a GlobalKey.
  GlobalKey? _rowScrollKey(StudioMenuItem<T> item, Set<Object?> duplicated) {
    final identity = item.itemKey ?? item.value;
    if (duplicated.contains(identity)) {
      return null;
    }
    return _rowScrollKeys.putIfAbsent(identity, () => GlobalKey());
  }

  Widget _buildItem(
    BuildContext context,
    StudioMenuItem<T> item,
    Key? wrapperKey,
    GlobalKey? scrollKey,
  ) {
    final selected = item.selected;
    final itemButton = MenuItemButton(
      // Standalone menu items default to the horizontal overflow axis,
      // which lays the label out without a flex child and hands unbounded
      // width to rows with Expanded content. The vertical axis is what
      // Material menus use for popup rows: the label expands to the row
      // width and every row child receives finite width.
      overflowAxis: Axis.vertical,
      onPressed: widget.enabled && item.enabled
          ? () => _selectItem(item)
          : null,
      style: selected
          ? ButtonStyle(
              backgroundColor: WidgetStatePropertyAll(
                Theme.of(context).colorScheme.surfaceContainerHigh,
              ),
            )
          : null,
      child: item.child,
    );
    // The row topology is constant per identity from the first build:
    // wrapper key → scrolling anchor → [tooltip] → selected semantics →
    // button. The selected hint is an attribute update on the Semantics and
    // the button style, never a structural wrapper that appears or moves,
    // so a canonical selected change keeps the focused row's element and
    // state. A tooltip cannot wrap rows without a message (the SDK forbids
    // both being null), and its presence is a static caller decision, so it
    // stays conditional and constant for a given row.
    Widget row = Semantics(selected: selected, child: itemButton);
    final tooltip = item.tooltip;
    if (tooltip != null) {
      row = Tooltip(message: tooltip, child: row);
    }
    if (scrollKey != null) {
      row = KeyedSubtree(key: scrollKey, child: row);
    }
    if (wrapperKey == null) {
      return row;
    }
    return KeyedSubtree(key: wrapperKey, child: row);
  }

  /// Activates a row: closes the menu and reports the selection.
  ///
  /// Closing removes the overlay focus scope, which restores focus to the
  /// real trigger for click and keyboard opens (armed with the one-shot
  /// suppression); hover-driven opens never focused the trigger and keep
  /// focus wherever it was.
  void _selectItem(StudioMenuItem<T> item) {
    _surface?.close(reason: StudioSurfaceCloseReason.selection);
    widget.onSelected?.call(item.value as T);
  }

  void _handleOpened() {
    // Capture the session baseline atomically before any external open
    // callback runs: the baseline is the rows this overlay was built from
    // (the identities of the latest build), not whatever the first external
    // rebuild — possibly already a canonical update that removed rows —
    // brings later.
    _openRowIds = _currentRowIds;
    widget.onOpen?.call();
    _keepSelectedItemVisible();
  }

  void _handleClosed() {
    // The open session ended; the next open captures a fresh baseline.
    _openRowIds = null;
    widget.onClose?.call();
  }

  void _keepSelectedItemVisible() {
    WidgetsBinding.instance.addPostFrameCallback((_) {
      // Reads the live field when the callback runs, so a row rebuilt (or a
      // selection changed) by the time this frame ends still resolves to the
      // current anchor, never a context captured from a gone overlay build.
      final context = _selectedScrollKey?.currentContext;
      if (context != null && mounted && isOpen) {
        // Scrolls the bounded row list only, through the selected row's own
        // constant anchor; the menu itself never moves to align the
        // selection. A selected row without a unique identity has no anchor
        // and simply keeps its scroll position.
        Scrollable.ensureVisible(context, duration: Duration.zero);
      }
    });
  }
}

/// Icon-button trigger for a [StudioMenu].
///
/// Renders the menu behind an [IconButton] while inheriting the shared
/// anchoring, sizing and keyboard contract of [StudioMenu].
class StudioIconMenu<T> extends StatefulWidget {
  const StudioIconMenu({
    this.tooltip,
    required this.icon,
    required this.itemBuilder,
    this.onSelected,
    this.enabled = true,
    this.iconSize,
    this.padding,
    this.visualDensity,
    this.color,
    this.menuConstraints,
    this.onBeforeOpen,
    this.onOpen,
    this.onClose,
    super.key,
  });

  /// Tooltip for the trigger button; omitted when null.
  final String? tooltip;

  /// Trigger icon.
  final Widget icon;

  /// See [StudioMenu.itemBuilder].
  final StudioMenuItemBuilder<T> itemBuilder;

  /// See [StudioMenu.onSelected].
  final ValueChanged<T>? onSelected;

  /// Whether the button opens the menu and rows can be activated.
  final bool enabled;

  /// See [IconButton.iconSize].
  final double? iconSize;

  /// See [IconButton.padding].
  final EdgeInsetsGeometry? padding;

  /// See [IconButton.visualDensity].
  final VisualDensity? visualDensity;

  /// See [IconButton.color].
  final Color? color;

  /// See [StudioMenu.menuConstraints].
  final BoxConstraints? menuConstraints;

  /// See [StudioMenu.onBeforeOpen].
  final VoidCallback? onBeforeOpen;

  /// See [StudioMenu.onOpen].
  final VoidCallback? onOpen;

  /// See [StudioMenu.onClose].
  final VoidCallback? onClose;

  @override
  State<StudioIconMenu<T>> createState() => _StudioIconMenuState<T>();
}

class _StudioIconMenuState<T> extends State<StudioIconMenu<T>> {
  final FocusNode _triggerFocusNode = FocusNode(
    debugLabel: 'studio-icon-menu-trigger',
  );

  @override
  void dispose() {
    _triggerFocusNode.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    return StudioMenu<T>.custom(
      itemBuilder: widget.itemBuilder,
      onSelected: widget.onSelected,
      enabled: widget.enabled,
      menuConstraints: widget.menuConstraints,
      childFocusNode: _triggerFocusNode,
      onBeforeOpen: widget.onBeforeOpen,
      onOpen: widget.onOpen,
      onClose: widget.onClose,
      triggerBuilder: (context, controller, canOpen) => IconButton(
        tooltip: widget.tooltip,
        onPressed: canOpen ? controller.toggle : null,
        icon: widget.icon,
        iconSize: widget.iconSize,
        padding: widget.padding ?? const EdgeInsets.all(8),
        visualDensity: widget.visualDensity,
        color: widget.color,
        focusNode: _triggerFocusNode,
      ),
    );
  }
}

/// Moves focus within the open menu's rows only.
///
/// With focus on a row, arrow keys move within the row's enclosing focus
/// scope (the open surface's scope) and stop at the first or last row
/// instead of escaping to widgets behind or above the surface (for example
/// a header button whose focus handler would open another popup). With
/// focus on the menu's trigger, arrow down enters the first row and arrow
/// up enters the last row — the same entry points the Material menus
/// provide. While the menu is closed (the rows anchor is detached) the
/// action keeps the platform-default directional navigation.
///
/// The rows anchor is a non-focusable node inside the menu's row list: its
/// enclosing scope is exactly the open surface's scope, and it never needs
/// a context lookup outside build, so no dependency is taken from an
/// action invocation.
class _MenuDirectionalFocusAction extends DirectionalFocusAction {
  _MenuDirectionalFocusAction(this.rowsAnchor);

  final FocusNode rowsAnchor;

  @override
  void invoke(DirectionalFocusIntent intent) {
    final rowsContext = rowsAnchor.context;
    final scope = rowsAnchor.nearestScope;
    if (rowsContext == null || !rowsContext.mounted || scope == null) {
      // Menu not open: keep the platform-default directional navigation.
      super.invoke(intent);
      return;
    }
    final current = FocusManager.instance.primaryFocus;
    if (current != null && scope.descendants.contains(current)) {
      // Focus is on a row: the node's own directional move stays within its
      // enclosing scope and stops at the first or last row.
      current.focusInDirection(intent.direction);
      return;
    }
    final policy = ReadingOrderTraversalPolicy();
    switch (intent.direction) {
      case TraversalDirection.down:
        policy
            .findFirstFocus(rowsAnchor, ignoreCurrentFocus: true)
            ?.requestFocus();
      case TraversalDirection.up:
        policy
            .findLastFocus(rowsAnchor, ignoreCurrentFocus: true)
            .requestFocus();
      case TraversalDirection.left:
      case TraversalDirection.right:
        break;
    }
  }
}
