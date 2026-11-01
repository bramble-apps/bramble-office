// The window toolbar, as the desktop lays it out and draws it (a port of pg-app/src/toolbar.rs
// `toolbar` and chrome.rs `paint_capsules` / `title_item`): the user's items in capsules
// (runs of items between spaces), the document's name after View, flexible spaces taking the
// room left; the bar scrolls sideways when they don't all fit. The engine says which items there are and
// whether each is enabled or pressed (ffi.rs lt_chrome); pressing one calls back with its id.
import 'dart:async';
import 'dart:math' as math;

import 'package:flutter/material.dart';

import 'icons.dart';
import 'glass.dart';
import 'look.dart';

const _space = 'Space', _flex = 'FlexibleSpace', _view = 'View', _zoom = 'Zoom';

class DesktopToolbar extends StatefulWidget {
  const DesktopToolbar({super.key, required this.chrome, required this.onPressed, this.titleOpen = false, this.customizing = false, this.onItems, this.onDefaults, this.onLongPress});

  /// View > Customize Toolbar… is open: the items drag (to move them, or off the bar to take
  /// them away) and the sheet's items drop in.
  final bool customizing;

  /// The toolbar's items as rearranged (ids, spaces included); the default set dropped in.
  final void Function(List<String> ids)? onItems;
  final VoidCallback? onDefaults;

  /// A long press on an item (the desktop's right-click): its menu at that place.
  final void Function(Rect at)? onLongPress;

  /// The title's popover is open (the title stays lit).
  final bool titleOpen;

  /// The engine's lt_chrome: look, title, items.
  final Map<String, dynamic> chrome;

  /// An item was pressed: its id, and its place (for a popover under it).
  final void Function(String id, Rect at) onPressed;

  /// toolbar::height
  static double height(Map<String, dynamic> chrome) => chrome['toolbarMode'] == 1 ? 66 : 52;

  @override
  State<DesktopToolbar> createState() => _DesktopToolbarState();
}

class _Entry {
  _Entry(this.j);
  final Map<String, dynamic> j;
  String get id => j['id'] as String;
  String get label => j['label'] as String? ?? '';
  String get tip => j['tip'] as String? ?? '';
  bool get enabled => j['enabled'] == true;
  bool get pressed => j['pressed'] == true;
}

double _textW(String s, TextStyle style) => (TextPainter(text: TextSpan(text: s, style: style), textDirection: TextDirection.ltr, maxLines: 1)..layout()).width;

class _DesktopToolbarState extends State<DesktopToolbar> {
  /// The item under a finger (drawn as the desktop's hover).
  String? _down;

  /// Each entry's place in the bar (entry index, rect), for where a drop lands; and where the
  /// dragged item would go (an x in the bar) while one is over it.
  final _rects = <(int, Rect)>[];
  double? _dropX;
  final _barKey = GlobalKey();

  /// The bar scrolled (too many items for the width), and while an item is dragged near one of
  /// its ends, scrolling on that way (-1, 1) so the rest can be dropped on.
  final _scroll = ScrollController();
  final _viewKey = GlobalKey();
  Timer? _edgeTimer;
  int _edge = 0;

  void _edgeScroll(int dir) {
    if (dir == _edge) return;
    _edge = dir;
    _edgeTimer?.cancel();
    _edgeTimer = null;
    if (dir == 0) return;
    _edgeTimer = Timer.periodic(const Duration(milliseconds: 16), (_) {
      if (!_scroll.hasClients) return;
      final p = _scroll.position;
      final was = p.pixels;
      _scroll.jumpTo((was + dir * 7).clamp(p.minScrollExtent, p.maxScrollExtent));
      // (the finger stays put, so where it is in the bar moves on with the bar)
      if (_dropX != null) setState(() => _dropX = _dropX! + _scroll.position.pixels - was);
    });
  }

  @override
  void dispose() {
    _edgeTimer?.cancel();
    _scroll.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final look = Look(widget.chrome);
    final mode = (widget.chrome['toolbarMode'] as int?) ?? 0;
    final list = [for (final j in (widget.chrome['toolbar'] as List? ?? [])) _Entry(j as Map<String, dynamic>)];
    final title = widget.chrome['title'] as String? ?? '';
    final sub = widget.chrome['sub'] as String?;
    final zoom = (widget.chrome['zoom'] as num?)?.toDouble() ?? 1;
    return Container(
      height: DesktopToolbar.height(widget.chrome),
      // (no fill of its own: it floats over the pages and the inspector, as on the desktop;
      // the strip is 10 × 4 in from the window's edges)
      padding: const EdgeInsets.symmetric(horizontal: 10, vertical: 4),
      child: LayoutBuilder(builder: (context, box) => _bar(look, mode, list, title, sub, zoom, Size(box.maxWidth, box.maxHeight))),
    );
  }

  Widget _bar(Look look, int mode, List<_Entry> list, String title, String? sub, double zoom, Size bar) {
    bool isSpace(String id) => id == _space || id == _flex;
    final titleFont = Look.font(15, look.text, bold: true);
    final titleFull = _textW(title, titleFont) + 44;
    final titleMin = titleFull < 140 ? titleFull : 140.0;
    bool alone(int i) => (i == 0 || isSpace(list[i - 1].id)) && (i + 1 >= list.length || isSpace(list[i + 1].id));
    double entryWidth(int i) {
      final id = list[i].id;
      if (id == _space) return 8;
      if (id == _flex) return 0;
      if (id == _zoom) return 76;
      if (mode == 3) return _textW(list[i].label, Look.font(13, look.text)) + 20;
      if (mode == 1) {
        final w = _textW(list[i].label, Look.font(11, look.text)) + 8;
        final min = alone(i) ? 44.0 : 40.0;
        return w > min ? w : min;
      }
      return alone(i) ? 44 : 40;
    }

    final widths = [for (var i = 0; i < list.length; i++) entryWidth(i)];
    final titleAt = list.isNotEmpty && list.first.id == _view ? 1 : 0;
    double capsPad(List<bool> shown) {
      var n = 0;
      var inCap = false;
      for (var i = 0; i < list.length; i++) {
        if (!shown[i]) continue;
        if (isSpace(list[i].id)) {
          inCap = false;
        } else if (!inCap) {
          n++;
          inCap = true;
        }
      }
      return n * 16.0;
    }

    final shown = List.filled(list.length, true);
    final avail = bar.width - titleMin + 8;
    const overflowW = 44.0;
    double total() {
      var t = 0.0;
      for (var i = 0; i < list.length; i++) {
        if (shown[i]) t += widths[i];
      }
      return t + capsPad(shown);
    }

    // (not enough room: every item stays and the bar scrolls sideways, rather than an
    // overflow menu)
    final overflow = <int>[];
    var flexes = 0;
    for (var i = 0; i < list.length; i++) {
      if (shown[i] && list[i].id == _flex) flexes++;
    }
    var spare = avail - total() - (overflow.isEmpty ? 0 : overflowW);
    if (spare < 0) spare = 0;
    final titleW = titleFull < titleMin + spare ? titleFull : titleMin + spare;
    spare -= titleW - titleMin;
    for (var i = 0; i < list.length; i++) {
      if (list[i].id == _flex && flexes > 0) widths[i] += spare / flexes;
    }

    // lay out
    _rects.clear();
    final cy = mode == 1 ? 20.0 : bar.height / 2;
    final capH = mode == 1 ? 36.0 : 40.0;
    var x = 0.0;
    double? inCap;
    final caps = <Rect>[];
    final kids = <Widget>[];
    void closeCap() {
      final x0 = inCap;
      if (x0 == null) return;
      inCap = null;
      x += 4;
      caps.add(Rect.fromLTRB(x0, cy - capH / 2, x, cy + capH / 2));
      x += 8;
    }

    for (var i = 0; i <= list.length; i++) {
      if (i == titleAt) {
        closeCap();
        final tr = Rect.fromLTWH(x + 8, 0, titleW - 16, bar.height);
        // a click on the title: its popover (name, where, format) under it (chrome.rs title)
        final tw = _textW(_fit(title, titleFont, tr.width - 28), titleFont);
        final blockH = sub != null ? 34.0 : 18.0;
        final hit = Rect.fromLTWH(tr.left - 8, cy - (blockH + 14) / 2, math.min(tw + 36, tr.width + 8), blockH + 14);
        final lit = widget.titleOpen || _down == '__title';
        kids.add(Positioned.fromRect(
          rect: hit,
          child: Builder(
            builder: (ctx) => GestureDetector(
              behavior: HitTestBehavior.opaque,
              onTapDown: (_) => setState(() => _down = '__title'),
              onTapCancel: () => setState(() => _down = null),
              onTapUp: (_) {
                setState(() => _down = null);
                final box = ctx.findRenderObject() as RenderBox;
                widget.onPressed('__title', box.localToGlobal(Offset.zero) & box.size);
              },
              child: CustomPaint(painter: _TitleHit(lit, look, sub != null)),
            ),
          ),
        ));
        kids.add(Positioned.fromRect(rect: tr, child: IgnorePointer(child: _TitleItem(name: _fit(title, titleFont, tr.width - 28), sub: sub, look: look, cy: cy))));
        x += titleW;
      }
      if (i == list.length) break;
      final e = list[i];
      if (!shown[i]) continue;
      if (isSpace(e.id)) {
        closeCap();
        final sr = Rect.fromLTWH(x - 8, 0, widths[i] + 8, bar.height);
        _rects.add((i, sr));
        // spaces show as boxes while customizing, and can be dragged
        if (widget.customizing) {
          final box = Container(margin: const EdgeInsets.symmetric(horizontal: 2, vertical: 10), decoration: BoxDecoration(border: Border.all(color: look.gray(0x60)), borderRadius: BorderRadius.circular(3)));
          kids.add(Positioned.fromRect(rect: sr, child: _draggable(look, e, i, box)));
        }
        x += widths[i];
        continue;
      }
      if (inCap == null) {
        inCap = x;
        x += 4;
      }
      final w = widths[i];
      final r = Rect.fromLTRB(x, 0, x + w, bar.height);
      final iconR = Rect.fromCenter(center: Offset(r.center.dx, cy), width: w, height: capH - 4);
      x += w;
      _rects.add((i, r));
      final cellPaint = CustomPaint(
        size: r.size,
        painter: e.id == _zoom ? _ZoomPainter(look, zoom, iconR.shift(-r.topLeft), false, mode == 1) : _CellPainter(look, e, iconR.shift(-r.topLeft), mode, false),
      );
      if (widget.customizing) {
        kids.add(Positioned.fromRect(rect: r, child: _draggable(look, e, i, cellPaint)));
        if (i + 1 >= list.length || isSpace(list[i + 1].id)) closeCap();
        continue;
      }
      kids.add(Positioned.fromRect(
        rect: r,
        child: _hit(e, r, e.id == _zoom || e.enabled, Tooltip(
          message: e.tip,
          waitDuration: const Duration(milliseconds: 600),
          child: CustomPaint(
            size: r.size,
            painter: e.id == _zoom
                ? _ZoomPainter(look, zoom, iconR.shift(-r.topLeft), _down == e.id, mode == 1)
                : _CellPainter(look, e, iconR.shift(-r.topLeft), mode, _down == e.id),
          ),
        )),
      ));
      if (i + 1 >= list.length || isSpace(list[i + 1].id)) closeCap();
    }
    closeCap();
    if (overflow.isNotEmpty) {
      final r = Rect.fromCenter(center: Offset(bar.width - overflowW / 2, cy), width: 40, height: capH - 4);
      kids.add(Positioned.fromRect(
        rect: r,
        child: GestureDetector(
          onTapDown: (_) => setState(() => _down = '»'),
          onTapCancel: () => setState(() => _down = null),
          onTapUp: (_) {
            setState(() => _down = null);
            _overflowMenu(look, r, [for (final i in overflow) list[i]]);
          },
          child: DecoratedBox(
            decoration: BoxDecoration(color: _down == '»' ? look.gray(0x48) : look.gray(0x36), borderRadius: BorderRadius.circular(r.height / 2)),
            child: Center(child: DesktopIcon(DIcon.overflow, size: 18, color: look.text)),
          ),
        ),
      ));
    }
    final ids = [for (final e in list) e.id];
    final stack = DragTarget<TbPayload>(
      // a drop on the bar: an item in at the place shown, an item of the bar moved there, or the
      // default set (toolbar.rs drag_overlay)
      onMove: (d) {
        final box = _barKey.currentContext?.findRenderObject() as RenderBox?;
        if (box == null) return;
        setState(() => _dropX = box.globalToLocal(d.offset + const Offset(40, 20)).dx);
        final view = _viewKey.currentContext?.findRenderObject() as RenderBox?;
        if (view != null) {
          final vx = view.globalToLocal(d.offset + const Offset(40, 20)).dx;
          _edgeScroll(vx < 48 ? -1 : (vx > view.size.width - 48 ? 1 : 0));
        }
      },
      onLeave: (_) {
        _edgeScroll(0);
        setState(() => _dropX = null);
      },
      onAcceptWithDetails: (d) {
        _edgeScroll(0);
        final x = _dropX;
        setState(() => _dropX = null);
        final p = d.data;
        if (p.defaults) return widget.onDefaults?.call();
        final id = p.id!;
        var slot = ids.length;
        for (final (i, r) in _rects) {
          if (x != null && x < r.center.dx) {
            slot = i;
            break;
          }
        }
        final next = [...ids];
        // an item already in the toolbar moves rather than appearing twice
        var from = p.from;
        if (from == null && !isSpace(id)) {
          final k = next.indexOf(id);
          if (k >= 0) from = k;
        }
        var at = slot;
        if (from != null) {
          next.removeAt(from);
          if (at > from) at--;
        }
        next.insert(at.clamp(0, next.length), id);
        widget.onItems?.call(next);
      },
      builder: (ctx, cand, _) {
        double? lineX;
        if (cand.isNotEmpty && _dropX != null) {
          final over = cand.first;
          if (over != null && !over.defaults) {
            lineX = _rects.isEmpty ? 0 : _rects.last.$2.right;
            for (final (_, r) in _rects) {
              if (_dropX! < r.center.dx) {
                lineX = r.left;
                break;
              }
            }
          }
        }
        return Stack(key: _barKey, clipBehavior: Clip.none, children: [
          Positioned.fill(child: CustomPaint(painter: _CapsulesPainter(look, caps))),
          ...kids,
          // where it would land; the default set: the whole bar ringed
          if (lineX != null) Positioned(left: lineX - 1.5, top: 0, bottom: 0, width: 3, child: ColoredBox(color: look.accent)),
          if (cand.isNotEmpty && cand.first?.defaults == true)
            Positioned.fill(child: IgnorePointer(child: DecoratedBox(decoration: BoxDecoration(borderRadius: BorderRadius.circular(8), border: Border.all(color: look.accent, width: 2))))),
        ]);
      },
    );
    if (x <= bar.width + 0.5) return stack;
    return SingleChildScrollView(
      key: _viewKey,
      controller: _scroll,
      scrollDirection: Axis.horizontal,
      clipBehavior: Clip.none,
      child: SizedBox(width: x, height: bar.height, child: stack),
    );
  }

  Widget _hit(_Entry e, Rect r, bool enabled, Widget child) => Builder(
        builder: (ctx) => GestureDetector(
          behavior: HitTestBehavior.opaque,
          onTapDown: enabled ? (_) => setState(() => _down = e.id) : null,
          onTapCancel: enabled ? () => setState(() => _down = null) : null,
          onTapUp: enabled
              ? (_) {
                  setState(() => _down = null);
                  // (the item's own place on screen: the bar may be scrolled)
                  final box = ctx.findRenderObject() as RenderBox?;
                  widget.onPressed(e.id, box == null ? r : box.localToGlobal(Offset.zero) & box.size);
                }
              : null,
          // (the desktop's right-click: the toolbar's menu)
          onLongPressStart: widget.onLongPress == null
              ? null
              : (_) {
                  setState(() => _down = null);
                  final box = ctx.findRenderObject() as RenderBox?;
                  widget.onLongPress!(box == null ? r : box.localToGlobal(Offset.zero) & box.size);
                },
          child: child,
        ),
      );

  /// An entry of the bar while customizing: dragged to move it, or off the bar to take it away.
  Widget _draggable(Look look, _Entry e, int i, Widget child) => Draggable<TbPayload>(
        data: TbPayload(id: e.id, from: i),
        feedback: TbDragImage(look: look, id: e.id, label: e.label, icon: e.j['icon'] as String? ?? ''),
        childWhenDragging: Opacity(opacity: 0.3, child: child),
        onDraggableCanceled: (_, _) {
          // dragged off the toolbar: gone
          final next = [for (final x in widget.chrome['toolbar'] as List) (x as Map)['id'] as String]..removeAt(i);
          widget.onItems?.call(next);
        },
        child: child,
      );

  Future<void> _overflowMenu(Look look, Rect r, List<_Entry> items) async {
    final box = context.findRenderObject() as RenderBox?;
    final o = box?.localToGlobal(const Offset(10, 4)) ?? Offset.zero;
    final at = r.shift(o);
    final id = await showMenu<String>(
      context: context,
      position: RelativeRect.fromLTRB(at.left, at.bottom, at.right, at.bottom),
      color: look.panel,
      items: [
        for (final e in items) PopupMenuItem(value: e.id, enabled: e.enabled && e.j['popup'] != true, child: Text(e.label, style: Look.font(13, look.text))),
      ],
    );
    if (id != null) widget.onPressed(id, at);
  }

  /// The name cut short with "…" until it fits (toolbar.rs).
  String _fit(String name, TextStyle font, double w) {
    var n = name;
    while (_textW(n, font) > w && n.characters.length > 2) {
      final cs = n.characters.toList();
      n = '${cs.take(cs.length - 2).join()}…'.replaceFirst('……', '…');
    }
    return n;
  }
}

/// chrome.rs title_item: the name, "Edited" (and a Word file's mode) under it.
/// The title's lit look (hovered or its popover open): a rounded fill and a ⌄ at the right.
class _TitleHit extends CustomPainter {
  _TitleHit(this.lit, this.look, this.twoLines);
  final bool lit, twoLines;
  final Look look;
  @override
  void paint(Canvas c, Size s) {
    if (!lit) return;
    c.drawRRect(RRect.fromRectAndRadius(Offset.zero & s, const Radius.circular(8)), Paint()..color = look.gray(0x3a));
    drawIcon(c, Rect.fromCenter(center: Offset(s.width - 14, s.height / 2 + (twoLines ? -7 : 0) + 1), width: 11, height: 11), DIcon.chevronDown, look.textDim);
  }

  @override
  bool shouldRepaint(_TitleHit o) => o.lit != lit;
}

class _TitleItem extends StatelessWidget {
  const _TitleItem({required this.name, required this.sub, required this.look, required this.cy});
  final String name;
  final String? sub;
  final Look look;
  final double cy;

  @override
  Widget build(BuildContext context) => CustomPaint(painter: _TitlePainter(name, sub, look, cy));
}

class _TitlePainter extends CustomPainter {
  _TitlePainter(this.name, this.sub, this.look, this.cy);
  final String name;
  final String? sub;
  final Look look;
  final double cy;

  @override
  void paint(Canvas canvas, Size size) {
    final ny = sub != null ? cy - 7 : cy;
    // over a see-through band (the page shows through it) a soft halo in the window's colour
    // keeps the name readable on a light page
    final halo = look.classic || look.toolbarBackground == 'solid' ? null : [Shadow(color: look.header.withValues(alpha: 0.9), blurRadius: 6)];
    _text(canvas, name, Look.font(15, look.text, bold: true).copyWith(shadows: halo), Offset(0, ny), left: true);
    if (sub != null) _text(canvas, sub!, Look.font(11, look.textDim).copyWith(shadows: halo), Offset(0, cy + 9), left: true);
  }

  @override
  bool shouldRepaint(_TitlePainter o) => o.name != name || o.sub != sub || o.look.text != look.text || o.look.toolbarBackground != look.toolbarBackground;
}

/// Text centred on `at` (or left-aligned at it, centred vertically), as egui's painter.text.
void _text(Canvas c, String s, TextStyle style, Offset at, {bool left = false}) {
  final tp = TextPainter(text: TextSpan(text: s, style: style), textDirection: TextDirection.ltr, maxLines: 1)..layout();
  tp.paint(c, Offset(left ? at.dx : at.dx - tp.width / 2, at.dy - tp.height / 2));
}

/// toolbar.rs cell: the pressed / hover fill, the icon (or label), the label under it.
class _CellPainter extends CustomPainter {
  _CellPainter(this.look, this.e, this.iconR, this.mode, this.down);
  final Look look;
  final _Entry e;
  final Rect iconR;
  final int mode;
  final bool down;

  @override
  void paint(Canvas canvas, Size size) {
    final on = e.pressed;
    final hover = down && e.enabled;
    final bg = on ? look.gray(0x5a) : (hover ? look.gray(0x48) : null);
    final col = e.enabled ? look.gray(0xee) : look.gray(0x70);
    if (mode == 3) {
      if (bg != null) canvas.drawRRect(RRect.fromRectAndRadius(iconR.deflate(3), Radius.circular(iconR.height / 2)), Paint()..color = bg);
      _text(canvas, e.label, Look.font(13, col), iconR.center);
      return;
    }
    if (bg != null) canvas.drawRRect(RRect.fromRectAndRadius(iconR.deflate(2), Radius.circular(iconR.height / 2)), Paint()..color = bg);
    final s = 22.0 < iconR.height - 8 ? 22.0 : iconR.height - 8;
    final ir = Rect.fromCenter(center: iconR.center, width: s, height: s);
    if (!drawToolbarGlyph(canvas, ir, e.id, col)) {
      final icon = iconNamed(e.j['icon'] as String? ?? '');
      if (icon != null) drawIcon(canvas, ir, icon, col);
    }
    if (mode == 1) _text(canvas, e.label, Look.font(11, e.enabled ? look.textDim : look.gray(0x70)), Offset(size.width / 2, size.height - 7));
  }

  @override
  bool shouldRepaint(_CellPainter o) => o.e.j != e.j || o.down != down || o.iconR != iconR || o.look.text != look.text;
}

/// The Zoom item: "125% ⌄" in bold, centred together (toolbar.rs).
class _ZoomPainter extends CustomPainter {
  _ZoomPainter(this.look, this.zoom, this.iconR, this.down, this.labels);
  final Look look;
  final double zoom;
  final Rect iconR;
  final bool down, labels;

  @override
  void paint(Canvas canvas, Size size) {
    final zr = Rect.fromCenter(center: iconR.center, width: 76, height: iconR.height);
    if (down) canvas.drawRRect(RRect.fromRectAndRadius(zr.deflate(2), const Radius.circular(16)), Paint()..color = look.gray(0x48));
    final style = Look.font(14.5, look.text, bold: true);
    final tp = TextPainter(text: TextSpan(text: '${(zoom * 100).round()}%', style: style), textDirection: TextDirection.ltr)..layout();
    final total = tp.width + 4 + 10;
    final x0 = zr.center.dx - total / 2;
    tp.paint(canvas, Offset(x0, zr.center.dy - tp.height / 2));
    drawIcon(canvas, Rect.fromCenter(center: Offset(x0 + total - 5, zr.center.dy + 1), width: 10, height: 10), DIcon.chevronDown, look.text);
    if (labels) _text(canvas, 'Zoom', Look.font(11, look.textDim), Offset(size.width / 2, size.height - 7));
  }

  @override
  bool shouldRepaint(_ZoomPainter o) => o.zoom != zoom || o.down != down || o.iconR != iconR;
}

/// chrome.rs paint_capsules (no liquid glass): dark, a solid fill and a light rim brighter at
/// the top and bottom; light, a white glass with a soft shadow and a faint edge.
class _CapsulesPainter extends CustomPainter {
  _CapsulesPainter(this.look, this.caps);
  final Look look;
  final List<Rect> caps;

  @override
  void paint(Canvas canvas, Size size) {
    for (final c in caps) {
      final r = c.height / 2;
      if (look.glass) {
        // liquid glass, the document's blur showing through unless the band is solid
        paintGlass(canvas, c, r, look, seeThrough: !look.classic && look.toolbarBackground != 'solid');
      } else if (look.classic) {
        canvas.drawRRect(RRect.fromRectAndRadius(c, Radius.circular(r)), Paint()..color = look.gray(0x36));
      } else if (!look.light) {
        canvas.drawRRect(RRect.fromRectAndRadius(c, Radius.circular(r)), Paint()..color = look.capsule);
        canvas.drawRRect(
            RRect.fromRectAndRadius(c.deflate(0.5), Radius.circular(r - 0.5)),
            Paint()
              ..style = PaintingStyle.stroke
              ..strokeWidth = 1
              ..color = const Color.fromARGB(34, 255, 255, 255));
        for (final y in [c.top + 0.5, c.bottom - 0.5]) {
          canvas.drawLine(Offset(c.left + r, y), Offset(c.right - r, y), Paint()
            ..strokeWidth = 1
            ..color = const Color.fromARGB(22, 255, 255, 255));
        }
      } else {
        canvas.drawRRect(RRect.fromRectAndRadius(c.inflate(1), Radius.circular(r + 1)), Paint()..color = const Color.fromARGB(16, 0, 0, 0));
        canvas.drawRRect(RRect.fromRectAndRadius(c, Radius.circular(r)), Paint()..color = const Color.fromARGB(215, 255, 255, 255));
        canvas.drawRRect(
            RRect.fromRectAndRadius(c.deflate(0.5), Radius.circular(r - 0.5)),
            Paint()
              ..style = PaintingStyle.stroke
              ..strokeWidth = 1
              ..color = const Color.fromARGB(22, 0, 0, 0));
      }
    }
  }

  @override
  bool shouldRepaint(_CapsulesPainter o) => true;
}

/// What's dragged onto or off the toolbar (toolbar.rs Payload): an item (`from`: its place in
/// the bar, null from the Customize sheet) or the default set.
class TbPayload {
  const TbPayload({this.id, this.from, this.defaults = false});
  final String? id;
  final int? from;
  final bool defaults;
}

/// The item under the finger while it's dragged: its icon in a round capsule, its name under it.
class TbDragImage extends StatelessWidget {
  const TbDragImage({super.key, required this.look, required this.id, required this.label, required this.icon});
  final Look look;
  final String id, label, icon;
  @override
  Widget build(BuildContext context) => SizedBox(
        width: 80,
        height: 80,
        child: CustomPaint(painter: _DragPainter(look, id, label, icon)),
      );
}

class _DragPainter extends CustomPainter {
  _DragPainter(this.look, this.id, this.label, this.icon);
  final Look look;
  final String id, label, icon;
  @override
  void paint(Canvas c, Size s) {
    final m = Offset(s.width / 2, 20);
    c.drawCircle(m, 20, Paint()..color = look.gray(0x48).withValues(alpha: 0.9));
    final ir = Rect.fromCenter(center: m, width: 22, height: 22);
    if (!drawToolbarGlyph(c, ir, id, look.text)) {
      final ic = iconNamed(icon);
      if (ic != null) drawIcon(c, ir, ic, look.text);
    }
    _text(c, label, Look.font(11, look.text), m + const Offset(0, 30));
  }

  @override
  bool shouldRepaint(_DragPainter o) => false;
}
