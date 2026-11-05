// The touch menus, ported as they are from Letters (desktop/touch_menu.dart), so every Bramble
// Android app's menus look and work the same: a rounded panel with a soft shadow,
// 48-dp rows (iOS 26's 44-point rows at the suite's touch size), 17-point text, an icon column, shortcuts at the right, gaps with a hairline between
// sections, checkmarks for ticked items. Submenus open beside their row on a wide screen and
// take the menu's place (with a back row) on a phone. Notes' items carry their symbol as `icon`
// (an IconData) and `destructive` (red), as iOS's menus show them.
import 'dart:async';
import 'dart:math' as math;

import 'package:flutter/material.dart';

import 'look.dart';

/// The menus' measures.
class TouchMenu {
  static const row = 48.0, radius = 26.0, pad = 8.0, rowRadius = 16.0, minWidth = 260.0, maxWidth = 400.0;

  /// Narrower than this, submenus replace their menu instead of opening beside it.
  static const wide = 600.0;

  /// The panel: the panel colour, large corners, a hairline and a soft shadow.
  static BoxDecoration panel(Look look) => BoxDecoration(
    color: look.panel,
    borderRadius: BorderRadius.circular(radius),
    border: Border.all(color: look.windowStroke.withValues(alpha: look.light ? 0.6 : 0.9), width: 0.5),
    boxShadow: [
      BoxShadow(color: Color.fromARGB(look.light ? 30 : 110, 0, 0, 0), blurRadius: 32, offset: const Offset(0, 10)),
      BoxShadow(color: Color.fromARGB(look.light ? 14 : 50, 0, 0, 0), blurRadius: 4, offset: const Offset(0, 1)),
    ],
  );

  /// A pressed (or hovered, or open) row's fill: clearer than egui's hover on the panel.
  static Color highlight(Look look) => look.text.withValues(alpha: look.light ? 0.09 : 0.14);

  /// Where a row's text goes.
  static TextStyle label(Look look, {bool enabled = true, bool bold = false}) =>
      Look.font(17, enabled ? look.text : look.text.withValues(alpha: 0.38), bold: bold);
}

/// A popup `width` wide centred under `anchor`, or centred on the screen (`screen` wide) when
/// that would put it past `minX`…`maxX`.
double centeredX(Rect anchor, double width, double screen, double minX, double maxX) {
  final x = anchor.center.dx - width / 2;
  return x < minX || x > maxX ? math.max(0, (screen - width) / 2) : x;
}

/// `anchor` (global) in the coordinates of the root overlay (which Settings > Scaling scales).
Rect inOverlay(BuildContext context, Rect anchor) {
  final box = Overlay.of(context, rootOverlay: true).context.findRenderObject() as RenderBox?;
  if (box == null) return anchor;
  return Rect.fromPoints(box.globalToLocal(anchor.topLeft), box.globalToLocal(anchor.bottomRight));
}

/// Opens `items` (lt_menus' {label, command, enabled, checked, shortcut, kids, separator} or a
/// popover's {label, action, enabled, bold, checked, note, kids, separator}) under `anchor` (a
/// global rect; a point for a zero-size one); `onPick` gets the item chosen after it closes.
/// `centered`: a toolbar button's menu, centred under it (centred on the screen when it can't
/// be without going off it). `passThrough` (the text's edit menu): touches outside it close it
/// and go on to what's under them, so the selection's handles can be dragged straight away.
Future<void> showTouchMenu(
  BuildContext context,
  Look look,
  Rect anchor,
  List<dynamic> items,
  void Function(Map<String, dynamic> item) onPick, {
  bool centered = false,
  bool passThrough = false,
}) async {
  final at = inOverlay(context, anchor);
  if (passThrough) {
    final done = Completer<Map<String, dynamic>?>();
    late final OverlayEntry entry;
    void close([Map<String, dynamic>? item]) {
      if (done.isCompleted) return;
      entry.remove();
      done.complete(item);
    }

    entry = OverlayEntry(
      builder: (ctx) => Stack(
        children: [
          // (a touch outside: noted, not kept)
          Positioned.fill(
            child: Listener(behavior: HitTestBehavior.translucent, onPointerDown: (_) => close()),
          ),
          _MenuHost(look: look, anchor: at, items: items, centered: centered, done: close),
        ],
      ),
    );
    Overlay.of(context, rootOverlay: true).insert(entry);
    final picked = await done.future;
    if (picked != null) onPick(picked);
    return;
  }
  final picked = await showGeneralDialog<Map<String, dynamic>>(
    context: context,
    useRootNavigator: true,
    barrierDismissible: true,
    barrierLabel: 'Close menu',
    barrierColor: Colors.transparent,
    transitionDuration: const Duration(milliseconds: 140),
    transitionBuilder: (ctx, a, _, child) => FadeTransition(
      opacity: CurvedAnimation(parent: a, curve: Curves.easeOut),
      child: child,
    ),
    pageBuilder: (ctx, _, _) => _MenuHost(look: look, anchor: at, items: items, centered: centered),
  );
  if (picked != null) onPick(picked);
}

/// A level of open menus: its items, the row that opened it (null for the first) and its label.
class _Level {
  _Level(this.items, this.from, this.title);
  final List<dynamic> items;
  final Rect? from;
  final String? title;
}

class _MenuHost extends StatefulWidget {
  const _MenuHost({required this.look, required this.anchor, required this.items, this.centered = false, this.done});
  final Look look;
  final Rect anchor;
  final List<dynamic> items;
  final bool centered;

  /// Closes it with the item picked (not a route of its own): else the navigator's pop.
  final void Function(Map<String, dynamic> item)? done;
  @override
  State<_MenuHost> createState() => _MenuHostState();
}

class _MenuHostState extends State<_MenuHost> {
  late final _levels = [_Level(widget.items, null, null)];

  void _open(int depth, Map<String, dynamic> item, Rect row, bool wide) => setState(() {
    _levels.removeRange(depth + 1, _levels.length);
    _levels.add(_Level(item['kids'] as List? ?? const [], wide ? row : null, item['label'] as String?));
  });

  @override
  Widget build(BuildContext context) {
    final mq = MediaQuery.of(context);
    // (the room the menu has: Settings > Scaling scales it, not MediaQuery's size)
    return LayoutBuilder(builder: (context, box) => _build(context, mq, box.maxWidth >= TouchMenu.wide));
  }

  Widget _build(BuildContext context, MediaQueryData mq, bool wide) {
    final margin = EdgeInsets.fromLTRB(
      math.max(8, mq.padding.left + 8),
      math.max(8, mq.padding.top + 8),
      math.max(8, mq.padding.right + 8),
      math.max(8, mq.padding.bottom + 8),
    );
    Widget panel(int depth, {VoidCallback? back}) => _Panel(
      look: widget.look,
      items: _levels[depth].items,
      back: back,
      backLabel: back == null ? null : _levels[depth].title,
      onOpen: (item, row) => _open(depth, item, row, wide),
      onPick: (item) => widget.done != null ? widget.done!(item) : Navigator.of(context).pop(item),
      openLabel: depth + 1 < _levels.length ? _levels[depth + 1].title : null,
    );
    return Stack(
      children: [
        if (wide)
          for (final (d, l) in _levels.indexed)
            CustomSingleChildLayout(
              delegate: _Place(l.from ?? widget.anchor, margin, beside: l.from != null, centered: widget.centered && l.from == null),
              child: panel(d),
            )
        else
          // a phone: the deepest menu in the first one's place, a back row at its top
          CustomSingleChildLayout(
            delegate: _Place(widget.anchor, margin, beside: false, centered: widget.centered),
            child: KeyedSubtree(
              key: ValueKey(_levels.length),
              child: panel(_levels.length - 1, back: _levels.length > 1 ? () => setState(() => _levels.removeLast()) : null),
            ),
          ),
      ],
    );
  }
}

/// Places a menu under its anchor (or above, when there's more room there), or beside the row
/// that opened it (right, else left), inside the screen's margins.
class _Place extends SingleChildLayoutDelegate {
  _Place(this.anchor, this.margin, {required this.beside, this.centered = false});
  final Rect anchor;
  final EdgeInsets margin;
  final bool beside, centered;

  @override
  BoxConstraints getConstraintsForChild(BoxConstraints c) {
    final w = math.min(TouchMenu.maxWidth, c.maxWidth - margin.horizontal);
    final h = beside ? c.maxHeight - margin.vertical : math.max(anchor.top - 6 - margin.top, c.maxHeight - margin.bottom - anchor.bottom - 6);
    return BoxConstraints(minWidth: math.min(TouchMenu.minWidth, w), maxWidth: w, maxHeight: math.max(TouchMenu.row * 2, h));
  }

  @override
  Offset getPositionForChild(Size size, Size child) {
    final minX = margin.left, maxX = math.max(margin.left, size.width - margin.right - child.width);
    final minY = margin.top, maxY = math.max(margin.top, size.height - margin.bottom - child.height);
    if (beside) {
      var x = anchor.right + TouchMenu.pad + 4;
      if (x + child.width > size.width - margin.right) x = anchor.left - TouchMenu.pad - 4 - child.width;
      return Offset(x.clamp(minX, maxX), (anchor.top - TouchMenu.pad).clamp(minY, maxY));
    }
    final below = size.height - margin.bottom - anchor.bottom - 6;
    final above = anchor.top - 6 - margin.top;
    final y = child.height <= below || below >= above ? anchor.bottom + 6 : anchor.top - 6 - child.height;
    return Offset(centered ? centeredX(anchor, child.width, size.width, minX, maxX) : anchor.left.clamp(minX, maxX), y.clamp(minY, maxY));
  }

  @override
  bool shouldRelayout(_Place o) => o.anchor != anchor || o.margin != margin || o.beside != beside || o.centered != centered;
}

class _Panel extends StatelessWidget {
  const _Panel({required this.look, required this.items, required this.onOpen, required this.onPick, this.back, this.backLabel, this.openLabel});
  final Look look;
  final List<dynamic> items;
  final void Function(Map<String, dynamic> item, Rect row) onOpen;
  final void Function(Map<String, dynamic> item) onPick;
  final VoidCallback? back;
  final String? backLabel;

  /// The submenu open beside this menu (its row stays highlighted).
  final String? openLabel;

  @override
  Widget build(BuildContext context) {
    // an icon column when any item has an icon or a tick
    final lead = items.any((i) => i['separator'] != true && (i['icon'] != null || i['checked'] == true));
    return Material(
      type: MaterialType.transparency,
      child: Container(
        decoration: TouchMenu.panel(look),
        child: ClipRRect(
          borderRadius: BorderRadius.circular(TouchMenu.radius),
          child: IntrinsicWidth(
            child: SingleChildScrollView(
              padding: const EdgeInsets.all(TouchMenu.pad),
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.stretch,
                mainAxisSize: MainAxisSize.min,
                children: [
                  if (back != null) ...[
                    _Row(
                      look: look,
                      onTap: back,
                      builder: (fg) => Row(
                        children: [
                          SizedBox(
                            width: 30,
                            child: CustomPaint(size: const Size(30, 22), painter: _Chevron(fg, left: true)),
                          ),
                          const SizedBox(width: 12),
                          Flexible(
                            child: Text(backLabel ?? '', maxLines: 1, overflow: TextOverflow.ellipsis, style: TouchMenu.label(look, bold: true)),
                          ),
                        ],
                      ),
                    ),
                    _Gap(look),
                  ],
                  for (final i in items) _entry(i as Map<String, dynamic>, lead),
                ],
              ),
            ),
          ),
        ),
      ),
    );
  }

  Widget _entry(Map<String, dynamic> i, bool lead) {
    if (i['separator'] == true) return _Gap(look);
    // iOS 26's row of icon buttons at a menu's top (the note menu's Scan, Pin, Lock)
    if (i['row'] is List) {
      final row = (i['row'] as List).cast<Map<String, dynamic>>();
      return Padding(
        padding: const EdgeInsets.only(bottom: 4),
        child: Row(
          children: [
            for (final b in row)
              Expanded(
                child: _Row(
                  look: look,
                  onTap: b['enabled'] == false ? null : () => onPick(b),
                  builder: (fg) => Padding(
                    padding: const EdgeInsets.symmetric(vertical: 12),
                    child: Column(
                      mainAxisSize: MainAxisSize.min,
                      children: [
                        Icon(b['icon'] as IconData?, size: 25, color: b['destructive'] == true ? const Color(0xFFFF3B30) : fg),
                        const SizedBox(height: 6),
                        Text(b['label'] as String? ?? '', maxLines: 1, overflow: TextOverflow.ellipsis, style: Look.font(14, fg)),
                      ],
                    ),
                  ),
                ),
              ),
          ],
        ),
      );
    }
    final label = i['label'] as String? ?? '';
    if (i['note'] == true) {
      return Container(
        constraints: const BoxConstraints(minHeight: 40),
        padding: EdgeInsets.only(left: 16 + (lead ? 42 : 0), right: 16),
        alignment: Alignment.centerLeft,
        child: Text(label, style: Look.font(14, look.textDim)),
      );
    }
    final enabled = i['enabled'] != false;
    final kids = i['kids'] as List?;
    final checked = i['checked'] == true;
    final shortcut = i['shortcut'] as String?;
    final icon = i['icon'] as IconData?;
    final red = i['destructive'] == true ? const Color(0xFFFF3B30) : null;
    final dim = look.weakText.withValues(alpha: enabled ? 1 : 0.5);
    return Builder(
      builder: (ctx) => _Row(
        look: look,
        on: kids != null && openLabel == label,
        onTap: !enabled
            ? null
            : kids != null
            ? () {
                final box = ctx.findRenderObject() as RenderBox;
                onOpen(i, box.localToGlobal(Offset.zero, ancestor: Overlay.of(ctx, rootOverlay: true).context.findRenderObject()) & box.size);
              }
            : () => onPick(i),
        builder: (fg) {
          final c = enabled ? (red ?? fg) : fg.withValues(alpha: 0.38);
          return Row(
            children: [
              if (lead) ...[
                SizedBox(
                  width: 30,
                  child: checked
                      ? CustomPaint(size: const Size(30, 22), painter: _Check(c))
                      : icon == null
                      ? null
                      : Center(child: Icon(icon, size: 23, color: c)),
                ),
                const SizedBox(width: 12),
              ],
              Expanded(
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  mainAxisSize: MainAxisSize.min,
                  children: [
                    Text(
                      label,
                      maxLines: 1,
                      overflow: TextOverflow.ellipsis,
                      style: Look.font(17, c, bold: i['bold'] == true),
                    ),
                    // (iOS's second line: Math Results' "Suggest Results")
                    if (i['subtitle'] is String) Text(i['subtitle'] as String, maxLines: 1, overflow: TextOverflow.ellipsis, style: Look.font(13, dim)),
                  ],
                ),
              ),
              if (shortcut != null && shortcut.isNotEmpty)
                Padding(
                  padding: const EdgeInsets.only(left: 24),
                  child: Text(shortcut, style: Look.font(15, dim)),
                ),
              if (kids != null)
                Padding(
                  padding: const EdgeInsets.only(left: 16),
                  child: CustomPaint(size: const Size(10, 22), painter: _Chevron(dim)),
                ),
            ],
          );
        },
      ),
    );
  }
}

/// A section break: a wider gap with a hairline across it.
class _Gap extends StatelessWidget {
  const _Gap(this.look);
  final Look look;
  @override
  Widget build(BuildContext context) => Padding(
    padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 8),
    child: Container(height: 0.5, color: look.separator),
  );
}

/// A 48-dp row: filled while pressed or hovered (or while its submenu is open).
class _Row extends StatefulWidget {
  const _Row({required this.look, required this.onTap, required this.builder, this.on = false});
  final Look look;
  final VoidCallback? onTap;
  final Widget Function(Color fg) builder;
  final bool on;
  @override
  State<_Row> createState() => _RowState();
}

class _RowState extends State<_Row> {
  bool _down = false, _hover = false;
  @override
  Widget build(BuildContext context) {
    final look = widget.look;
    final active = widget.onTap != null;
    final fill = active && (_down || _hover || widget.on) ? TouchMenu.highlight(look) : null;
    return MouseRegion(
      onEnter: (_) => setState(() => _hover = true),
      onExit: (_) => setState(() => _hover = false),
      child: GestureDetector(
        behavior: HitTestBehavior.opaque,
        onTapDown: active ? (_) => setState(() => _down = true) : null,
        onTapCancel: () => setState(() => _down = false),
        onTapUp: active
            ? (_) {
                setState(() => _down = false);
                widget.onTap!();
              }
            : null,
        child: AnimatedContainer(
          duration: const Duration(milliseconds: 80),
          constraints: const BoxConstraints(minHeight: TouchMenu.row),
          padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 6),
          decoration: BoxDecoration(color: fill, borderRadius: BorderRadius.circular(TouchMenu.rowRadius)),
          child: widget.builder(look.text),
        ),
      ),
    );
  }
}

/// A ✓ for a ticked item.
class _Check extends CustomPainter {
  _Check(this.color);
  final Color color;
  @override
  void paint(Canvas canvas, Size size) {
    final c = size.center(Offset.zero);
    final p = Paint()
      ..style = PaintingStyle.stroke
      ..strokeWidth = 2.2
      ..strokeCap = StrokeCap.round
      ..strokeJoin = StrokeJoin.round
      ..color = color;
    canvas.drawPath(Path()..addPolygon([c + const Offset(-6, 0), c + const Offset(-1.5, 4.5), c + const Offset(6.5, -5)], false), p);
  }

  @override
  bool shouldRepaint(_Check o) => o.color != color;
}

/// A › (or ‹) chevron.
class _Chevron extends CustomPainter {
  _Chevron(this.color, {this.left = false});
  final Color color;
  final bool left;
  @override
  void paint(Canvas canvas, Size size) {
    final c = size.center(Offset.zero);
    final d = left ? -1.0 : 1.0;
    final p = Paint()
      ..style = PaintingStyle.stroke
      ..strokeWidth = left ? 2.4 : 2
      ..strokeCap = StrokeCap.round
      ..strokeJoin = StrokeJoin.round
      ..color = color;
    canvas.drawPath(Path()..addPolygon([c + Offset(-3 * d, -6), c + Offset(3 * d, 0), c + Offset(-3 * d, 6)], false), p);
  }

  @override
  bool shouldRepaint(_Chevron o) => o.color != color || o.left != left;
}
