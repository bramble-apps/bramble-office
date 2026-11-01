// The sheet tabs under the toolbar (app.rs sheet_tabs, Numbers 15.4): the "+" menu (New Sheet,
// Hide the open one, Unhide ▸) and a separator at the left, then the sheets — the open one a
// capsule in the accent colour (or its tab colour), the others grey labels — scrolling sideways
// when they don't fit, the open one kept in view. Tap opens a sheet; on the open one (or a touch
// and hold) its menu: Rename, Duplicate, Move Left / Right, Hide, Delete. A long drag along the
// strip moves a tab.
import 'dart:math' as math;

import 'package:flutter/material.dart';

import '../engine.dart';
import 'look.dart';

class SheetTabs extends StatefulWidget {
  const SheetTabs({super.key, required this.engine, required this.s, required this.look, required this.onChanged, required this.onMenu});
  final Engine engine;

  /// vl_status "sheets": {sheets: [{name, hidden, color}], current}.
  final Map<String, dynamic> s;
  final Look look;
  final VoidCallback onChanged;

  /// A menu of these items at this point (global).
  final void Function(List<dynamic> items, Rect global) onMenu;

  static const height = 42.0;

  @override
  State<SheetTabs> createState() => _SheetTabsState();
}

class _SheetTabsState extends State<SheetTabs> {
  final _scroll = ScrollController();
  final _keys = <int, GlobalKey>{};
  int _shown = -1;

  /// A tab being dragged along the strip: its sheet, and the finger's x in the strip.
  int? _drag;
  double _dragX = 0;

  @override
  void dispose() {
    _scroll.dispose();
    super.dispose();
  }

  void act(String a) {
    widget.engine.action('sheet:$a');
    widget.onChanged();
  }

  List<Map<String, dynamic>> get _sheets => [for (final s in (widget.s['sheets'] as List? ?? [])) s as Map<String, dynamic>];
  int get _cur => widget.s['current'] as int? ?? 0;

  /// A button's box on the screen (its menu hangs centred under it).
  static Rect _rect(BuildContext c) {
    final box = c.findRenderObject() as RenderBox;
    return box.localToGlobal(Offset.zero) & box.size;
  }

  void _plus(Rect global) {
    final sheets = _sheets;
    final visible = sheets.where((s) => s['hidden'] != true).length;
    final cur = _cur < sheets.length ? sheets[_cur]['name'] as String : '';
    final hidden = [for (final (i, s) in sheets.indexed) if (s['hidden'] == true) (i, s['name'] as String)];
    widget.onMenu([
      {'label': 'New Sheet', 'action': 'sheet:add', 'enabled': true},
      {'label': 'Hide “$cur”', 'action': 'sheet:hide:$_cur', 'enabled': visible > 1},
      {
        'label': 'Unhide',
        'enabled': hidden.isNotEmpty,
        'kids': [for (final (i, n) in hidden) {'label': n, 'action': 'sheet:unhide:$i', 'enabled': true}],
      },
    ], global);
  }

  void _tabMenu(int i, Rect global) {
    final n = _sheets.length;
    widget.onMenu([
      {'label': 'Rename', 'action': 'rename:$i', 'enabled': true},
      {'label': 'Duplicate', 'action': 'sheet:duplicate:$i', 'enabled': true},
      {'separator': true},
      {'label': 'Move Left', 'action': 'sheet:move:$i:${i - 1}', 'enabled': i > 0},
      {'label': 'Move Right', 'action': 'sheet:move:$i:${i + 1}', 'enabled': i + 1 < n},
      {'separator': true},
      {'label': 'Hide Sheet', 'action': 'sheet:hide:$i', 'enabled': _sheets.where((s) => s['hidden'] != true).length > 1},
      {'label': 'Delete', 'action': 'sheet:delete:$i', 'enabled': n > 1},
    ], global);
  }

  @override
  Widget build(BuildContext context) {
    final look = widget.look;
    final sheets = _sheets;
    final cur = _cur;
    if (cur != _shown) {
      _shown = cur;
      WidgetsBinding.instance.addPostFrameCallback((_) {
        final c = _keys[cur]?.currentContext;
        if (c != null) Scrollable.ensureVisible(c, duration: const Duration(milliseconds: 250), curve: Curves.easeInOut, alignmentPolicy: ScrollPositionAlignmentPolicy.keepVisibleAtEnd);
      });
    }
    final tabs = <Widget>[];
    for (final (i, s) in sheets.indexed) {
      if (s['hidden'] == true) continue;
      final on = i == cur;
      final name = s['name'] as String? ?? '';
      final chars = name.characters.toList();
      final shown = chars.length > 30 ? '${chars.sublist(0, 13).join()}…${chars.sublist(chars.length - 15).join()}' : name;
      final c = s['color'] as List?;
      final tint = c == null ? null : Color.fromARGB(((c[3] as num) * 255).round(), ((c[0] as num) * 255).round(), ((c[1] as num) * 255).round(), ((c[2] as num) * 255).round());
      final key = _keys.putIfAbsent(i, GlobalKey.new);
      tabs.add(Padding(
        key: key,
        padding: const EdgeInsets.only(right: 4),
        child: Opacity(
          opacity: _drag == i ? 0.35 : 1,
          child: GestureDetector(
            behavior: HitTestBehavior.opaque,
            onTapUp: (d) {
              if (on) {
                _tabMenu(i, _rect(_keys[i]!.currentContext!));
              } else {
                act('go:$i');
              }
            },
            onLongPressStart: (d) => setState(() {
              _drag = i;
              _dragX = (context.findRenderObject() as RenderBox).globalToLocal(d.globalPosition).dx;
            }),
            onLongPressMoveUpdate: (d) => setState(() => _dragX = (context.findRenderObject() as RenderBox).globalToLocal(d.globalPosition).dx),
            onLongPressEnd: (d) => _drop(i, d.globalPosition),
            child: Container(
              height: 32,
              constraints: BoxConstraints(minWidth: on ? 117 : 90),
              padding: const EdgeInsets.symmetric(horizontal: 14),
              decoration: BoxDecoration(color: on ? (tint ?? look.accent) : tint?.withValues(alpha: 0.32), borderRadius: BorderRadius.circular(16)),
              alignment: Alignment.center,
              child: Text(shown, maxLines: 1, style: Look.font(14, on ? Colors.white : look.textDim, bold: on)),
            ),
          ),
        ),
      ));
    }
    return Container(
      height: SheetTabs.height,
      decoration: BoxDecoration(color: look.tabBar, border: Border(bottom: BorderSide(color: look.separator))),
      child: Stack(children: [
        Row(children: [
          const SizedBox(width: 6),
          // "+": New Sheet, Hide, Unhide
          Builder(
            builder: (ctx) => GestureDetector(
              behavior: HitTestBehavior.opaque,
              onTapUp: (d) => _plus(_rect(ctx)),
              child: SizedBox(width: 44, height: 40, child: CustomPaint(painter: _Plus(look.textDim))),
            ),
          ),
          Container(width: 1, height: 22, color: look.separator, margin: const EdgeInsets.symmetric(horizontal: 6)),
          Expanded(
            child: SingleChildScrollView(
              controller: _scroll,
              scrollDirection: Axis.horizontal,
              padding: const EdgeInsets.only(left: 6, right: 12),
              child: Row(crossAxisAlignment: CrossAxisAlignment.center, children: tabs),
            ),
          ),
        ]),
        if (_drag != null && _drag! < sheets.length)
          Positioned(
            left: math.max(0, _dragX - 58),
            top: 5,
            child: IgnorePointer(
              child: Container(
                height: 32,
                width: 117,
                decoration: BoxDecoration(color: look.accent, borderRadius: BorderRadius.circular(16), boxShadow: const [BoxShadow(color: Color(0x40000000), blurRadius: 8, offset: Offset(0, 2))]),
                alignment: Alignment.center,
                child: Text(sheets[_drag!]['name'] as String? ?? '', maxLines: 1, overflow: TextOverflow.ellipsis, style: Look.font(14, Colors.white, bold: true)),
              ),
            ),
          ),
      ]),
    );
  }

  /// A dragged tab let go: it goes to the slot it's over.
  void _drop(int from, Offset global) {
    setState(() => _drag = null);
    var to = from;
    for (final e in _keys.entries) {
      final c = e.value.currentContext;
      if (c == null || e.key >= _sheets.length) continue;
      final box = c.findRenderObject() as RenderBox;
      final r = box.localToGlobal(Offset.zero) & box.size;
      if (global.dx >= r.left && global.dx <= r.right) to = e.key;
    }
    if (to != from) act('move:$from:$to');
  }
}

class _Plus extends CustomPainter {
  _Plus(this.c);
  final Color c;
  @override
  void paint(Canvas canvas, Size s) {
    final ctr = s.center(Offset.zero);
    final p = Paint()
      ..strokeWidth = 1.8
      ..color = c;
    canvas.drawLine(ctr - const Offset(8, 0), ctr + const Offset(8, 0), p);
    canvas.drawLine(ctr - const Offset(0, 8), ctr + const Offset(0, 8), p);
  }

  @override
  bool shouldRepaint(_Plus o) => o.c != c;
}
