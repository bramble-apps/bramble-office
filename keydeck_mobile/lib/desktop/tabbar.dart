// The document tab bar (pg-app tabs.rs bar): every open document a tab of the one window,
// shown with two tabs or more. Equal tabs in a rounded bar, the active one a lighter capsule, a
// close button on the tab pointed at (and on the active tab for a finger, which has no hover),
// "+" for a new document.
import 'package:flutter/gestures.dart';
import 'package:flutter/material.dart';

import '../engine.dart';
import 'look.dart';
import 'widgets.dart';

class DocTabBar extends StatefulWidget {
  const DocTabBar({super.key, required this.engine, required this.s, required this.onChanged});
  final Engine engine;

  /// lt_status tabs: {tabs: [{name, edited}], active}.
  final Map<String, dynamic> s;
  final VoidCallback onChanged;

  /// tabs.rs bar_height.
  static const height = 30.0;

  @override
  State<DocTabBar> createState() => _DocTabBarState();
}

class _DocTabBarState extends State<DocTabBar> {
  Offset? _hover;
  bool _touch = false;
  Offset? _down;

  void _act(String a) {
    widget.engine.action('tab:$a');
    widget.onChanged();
  }

  @override
  Widget build(BuildContext context) {
    final look = LookScope.of(context);
    final tabs = widget.s['tabs'] as List? ?? const [];
    final active = widget.s['active'] as int? ?? 0;
    return Container(
      height: DocTabBar.height,
      decoration: BoxDecoration(color: look.header, border: Border(bottom: BorderSide(color: look.separator, width: 1))),
      child: LayoutBuilder(builder: (context, box) {
        // (too many tabs for the width: each at least 140 wide, and the bar scrolls sideways)
        final h = DocTabBar.height - 1;
        final need = 8 + tabs.length * 140.0 + 6 + h + 8;
        final width = need > box.maxWidth ? need : box.maxWidth;
        final g = _TabGeometry(Size(width, h), tabs.length);
        final bar = Listener(
          onPointerDown: (e) => setState(() {
            _touch = e.kind == PointerDeviceKind.touch;
            _down = e.localPosition;
          }),
          onPointerUp: (_) => setState(() => _down = null),
          onPointerCancel: (_) => setState(() => _down = null),
          child: MouseRegion(
            onHover: (e) => setState(() {
              _touch = false;
              _hover = e.localPosition;
            }),
            onExit: (_) => setState(() => _hover = null),
            child: GestureDetector(
              behavior: HitTestBehavior.opaque,
              onTapUp: (d) {
                final p = d.localPosition;
                if (g.plus.contains(p)) return _act('new');
                final i = g.tabAt(p);
                if (i == null) return;
                // the close button: on the tab pointed at, or the active one for a finger
                if (g.close(i).inflate(_touch ? 6 : 0).contains(p) && (!_touch || i == active)) return _act('close:$i');
                _act('select:$i');
              },
              onTertiaryTapUp: (d) {
                final i = g.tabAt(d.localPosition);
                if (i != null) _act('close:$i');
              },
              child: CustomPaint(
                size: Size(width, h),
                painter: _TabsPainter(look, g, [for (final t in tabs) '${t['name']}${t['edited'] == true ? ' — Edited' : ''}'], active, _hover ?? (_touch ? null : _down), _touch, _down),
              ),
            ),
          ),
        );
        return width > box.maxWidth ? SingleChildScrollView(scrollDirection: Axis.horizontal, child: SizedBox(width: width, height: h, child: bar)) : bar;
      }),
    );
  }
}

/// Where the bar's parts are (the panel's rect shrunk by 8 × 3; "+" a circle at the right).
class _TabGeometry {
  _TabGeometry(Size size, this.n) {
    final full = Rect.fromLTRB(8, 3, size.width - 8, size.height + 1 - 3);
    plus = Rect.fromCenter(center: Offset(full.right - full.height / 2, full.center.dy), width: full.height, height: full.height);
    bar = Rect.fromLTRB(full.left, full.top, plus.left - 6, full.bottom);
    w = n == 0 ? 0 : bar.width / n;
  }
  final int n;
  late final Rect plus, bar;
  late final double w;

  Rect tab(int i) => Rect.fromLTWH(bar.left + i * w, bar.top, w, bar.height);
  Rect close(int i) => Rect.fromCenter(center: tab(i).centerLeft + const Offset(14, 0), width: 16, height: 16);
  int? tabAt(Offset p) {
    for (var i = 0; i < n; i++) {
      if (tab(i).contains(p)) return i;
    }
    return null;
  }
}

class _TabsPainter extends CustomPainter {
  _TabsPainter(this.look, this.g, this.labels, this.active, this.hover, this.touch, this.down);
  final Look look;
  final _TabGeometry g;
  final List<String> labels;
  final int active;
  final Offset? hover, down;
  final bool touch;

  @override
  void paint(Canvas canvas, Size size) {
    final r = g.bar.height / 2;
    canvas.drawRRect(RRect.fromRectAndRadius(g.bar, Radius.circular(r)), Paint()..color = look.tabBar);
    for (var i = 0; i < g.n; i++) {
      final t = g.tab(i);
      final isActive = i == active;
      final hovered = hover != null && t.contains(hover!);
      if (isActive) {
        canvas.drawRRect(RRect.fromRectAndRadius(t.deflate(2), Radius.circular(r - 2)), Paint()..color = look.tabActive);
      } else if (hovered) {
        canvas.drawRRect(RRect.fromRectAndRadius(t.deflate(2), Radius.circular(r - 2)), Paint()..color = look.light ? const Color.fromARGB(10, 0, 0, 0) : const Color.fromARGB(8, 255, 255, 255));
      }
      if (i > 0 && !isActive && i != active + 1) {
        canvas.drawLine(Offset(t.left, t.top + 6), Offset(t.left, t.bottom - 6), Paint()
          ..strokeWidth = 1
          ..color = look.tabLine);
      }
      final tp = TextPainter(
        text: TextSpan(text: labels[i], style: Look.font(12, isActive ? look.text : look.textDim)),
        textDirection: TextDirection.ltr,
        maxLines: 1,
        textAlign: TextAlign.center,
      )..layout(maxWidth: (g.w - 44).clamp(20, double.infinity));
      canvas.save();
      canvas.clipRect(Rect.fromLTRB(t.left + 24, t.top, t.right - 24, t.bottom));
      tp.paint(canvas, t.center - Offset(tp.width / 2, tp.height / 2));
      canvas.restore();
      final cr = g.close(i);
      final onClose = hover != null && cr.contains(hover!);
      if (hovered || onClose || (touch && isActive)) {
        if (onClose || (touch && isActive && down != null && cr.inflate(6).contains(down!))) canvas.drawCircle(cr.center, 8, Paint()..color = look.gray(0x44));
        const k = 3.5;
        final st = Paint()
          ..strokeWidth = 1.2
          ..color = look.textDim;
        canvas.drawLine(cr.center - const Offset(k, k), cr.center + const Offset(k, k), st);
        canvas.drawLine(cr.center + const Offset(-k, k), cr.center + const Offset(k, -k), st);
      }
    }
    final plusHot = (hover != null && g.plus.contains(hover!)) || (down != null && g.plus.contains(down!));
    canvas.drawCircle(g.plus.center, g.plus.width / 2, Paint()..color = plusHot ? look.gray(0x48) : look.tabBar);
    final st = Paint()
      ..strokeWidth = 1.4
      ..color = look.text;
    canvas.drawLine(g.plus.center - const Offset(5, 0), g.plus.center + const Offset(5, 0), st);
    canvas.drawLine(g.plus.center - const Offset(0, 5), g.plus.center + const Offset(0, 5), st);
  }

  @override
  bool shouldRepaint(_TabsPainter o) => true;
}
