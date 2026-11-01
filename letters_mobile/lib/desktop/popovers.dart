// The toolbar's popovers that are pictures rather than menus (chrome.rs table_popover,
// shape_popover; kn_ui chartpicker): tiles drawn as the desktop draws them. Each returns the
// action to run (menus.rs menu_action).
import 'dart:math' as math;

import 'package:flutter/material.dart';

import 'inspector.dart' show showDesktopPopover;
import 'look.dart';
import 'widgets.dart';

/// kn_ui contenthub tile colours and ink.
Color _tileBg(Look l) => l.light ? const Color(0xFFF2F2F7) : const Color(0xFF3A3A3A);
Color _tileHover(Look l) => l.light ? const Color(0xFFE4E4EB) : const Color(0xFF4A4A4A);
Color _ink(Look l) => l.light ? const Color(0xFF333333) : const Color(0xFFE4E4E4);

/// chrome.rs tile: a rounded square (radius 18), the accent rim while touched.
class _Tile extends StatefulWidget {
  const _Tile({required this.size, required this.onTap, required this.paint});
  final double size;
  final VoidCallback onTap;
  final void Function(Canvas c, Rect inner) paint;
  @override
  State<_Tile> createState() => _TileState();
}

class _TileState extends State<_Tile> {
  bool _down = false;
  @override
  Widget build(BuildContext context) {
    final look = LookScope.of(context);
    return GestureDetector(
      onTapDown: (_) => setState(() => _down = true),
      onTapCancel: () => setState(() => _down = false),
      onTapUp: (_) {
        setState(() => _down = false);
        widget.onTap();
      },
      child: CustomPaint(
        size: Size.square(widget.size),
        painter: _P((c, s) {
          final r = Offset.zero & s;
          final rr = RRect.fromRectAndRadius(r, const Radius.circular(18));
          c.drawRRect(rr, Paint()..color = _down ? _tileHover(look) : _tileBg(look));
          if (_down) {
            c.drawRRect(rr.deflate(1), Paint()
              ..style = PaintingStyle.stroke
              ..strokeWidth = 2
              ..color = look.accent);
          }
          widget.paint(c, r.deflate(widget.size * 0.2));
        }),
      ),
    );
  }
}

class _P extends CustomPainter {
  _P(this.f);
  final void Function(Canvas, Size) f;
  @override
  void paint(Canvas canvas, Size size) => f(canvas, size);
  @override
  bool shouldRepaint(_P o) => true;
}

/// The Table popover: Headers, Basic, Plain, Sums, drawn as small tables.
Future<String?> showTablePopover(BuildContext context, Rect anchor) => showDesktopPopover<String>(context, anchor, centered: true, fit: true, width: 2 * 120 + 12 + 12, child: (ctx) => Wrap(spacing: 12, runSpacing: 12, children: [
      for (var k = 0; k < 4; k++)
        _Tile(
          size: 120,
          onTap: () => Navigator.of(ctx).pop('table:$k'),
          paint: (c, r) {
            const cols = 4, rows = 5;
            final cw = r.width / cols, rh = r.height / rows;
            const head = Color(0xFFC4C4C4);
            c.drawRect(r, Paint()..color = const Color(0xFFEEEEEE));
            final (hr, hc, foot) = switch (k) { 0 => (true, true, false), 1 => (true, false, false), 2 => (false, false, false), _ => (true, false, true) };
            if (hr) c.drawRect(Rect.fromLTWH(r.left, r.top, r.width, rh), Paint()..color = head);
            if (hc) c.drawRect(Rect.fromLTWH(r.left, r.top, cw, r.height), Paint()..color = head);
            if (foot) c.drawRect(Rect.fromLTWH(r.left, r.bottom - rh, r.width, rh), Paint()..color = head);
            final line = Paint()
              ..strokeWidth = 0.8
              ..color = const Color(0xFFA8A8A8);
            for (var i = 1; i < rows; i++) {
              c.drawLine(Offset(r.left, r.top + rh * i), Offset(r.right, r.top + rh * i), line);
            }
            for (var i = 1; i < cols; i++) {
              c.drawLine(Offset(r.left + cw * i, r.top), Offset(r.left + cw * i, r.bottom), line);
            }
          },
        ),
    ]));

/// The engine's path ([["M", x, y], ["L", …], ["Q", …], ["C", …], ["Z"]], in a `w` × `h` box,
/// 100 × 100 unless said) scaled into `r`.
Path enginePath(List<dynamic> els, Rect r, {double w = 100, double h = 100}) {
  final p = Path();
  double x(num v) => r.left + v.toDouble() / w * r.width;
  double y(num v) => r.top + v.toDouble() / h * r.height;
  for (final e in els) {
    final l = e as List;
    switch (l[0]) {
      case 'M':
        p.moveTo(x(l[1]), y(l[2]));
      case 'L':
        p.lineTo(x(l[1]), y(l[2]));
      case 'Q':
        p.quadraticBezierTo(x(l[1]), y(l[2]), x(l[3]), y(l[4]));
      case 'C':
        p.cubicTo(x(l[1]), y(l[2]), x(l[3]), y(l[4]), x(l[5]), y(l[6]));
      case 'Z':
        p.close();
    }
  }
  return p;
}

/// The Shape popover: a text box, a line, a line with an arrowhead, the basic shapes; then
/// Browse Content Hub.
Future<String?> showShapePopover(BuildContext context, Rect anchor, Map<String, dynamic> data) {
  final shapes = [for (final s in (data['shapes'] as List? ?? [])) s as Map<String, dynamic>];
  return showDesktopPopover<String>(context, anchor, centered: true, fit: true, width: 4 * 64 + 3 * 10 + 12, child: (ctx) {
    final look = LookScope.of(ctx);
    final ink = _ink(look);
    final tiles = <Widget>[
      _Tile(size: 64, onTap: () => Navigator.of(ctx).pop('shape:text'), paint: (c, r) {
        final tp = TextPainter(text: TextSpan(text: 'Text', style: Look.font(15, ink)), textDirection: TextDirection.ltr)..layout();
        tp.paint(c, r.center - Offset(tp.width / 2, tp.height / 2));
      }),
      for (final arrow in [false, true])
        _Tile(size: 64, onTap: () => Navigator.of(ctx).pop(arrow ? 'shape:arrow' : 'shape:line'), paint: (c, r) {
          final p = Paint()
            ..strokeWidth = 2.5
            ..color = ink;
          c.drawLine(r.bottomLeft, r.topRight, p);
          if (arrow) {
            // egui's arrow tip: two strokes back from the end
            final dir = (r.topRight - r.bottomLeft) / (r.topRight - r.bottomLeft).distance;
            final tip = r.topRight;
            final len = (r.topRight - r.bottomLeft).distance / 4;
            for (final a in [math.pi / 4, -math.pi / 4]) {
              final d = Offset(dir.dx * math.cos(a) - dir.dy * math.sin(a), dir.dx * math.sin(a) + dir.dy * math.cos(a));
              c.drawLine(tip, tip - d * len, p);
            }
          }
        }),
      for (final s in shapes)
        _Tile(size: 64, onTap: () => Navigator.of(ctx).pop(s['action'] as String), paint: (c, r) {
          final path = enginePath(s['path'] as List, r);
          c.drawPath(path, Paint()..color = ink);
          c.drawPath(path, Paint()
            ..style = PaintingStyle.stroke
            ..strokeWidth = 1
            ..color = ink);
        }),
    ];
    return Column(crossAxisAlignment: CrossAxisAlignment.stretch, children: [
      Wrap(spacing: 10, runSpacing: 10, children: tiles),
      const SizedBox(height: 8 + 6),
      DButton(height: 40, radius: 20, fill: _tileBg(look), onTap: () => Navigator.of(ctx).pop('media:Content Hub Shapes'), child: const Text('Browse Content Hub')),
    ]);
  });
}

// ---------------------------------------------------------------- the Chart popover (kn_ui chartpicker)

const _types2d = ['Column', 'Stacked Column', 'Bar', 'Stacked Bar', 'Line', 'Area', 'Stacked Area', 'Pie', 'Donut', 'Scatter', 'Bubble', 'Mixed', '2-Axis', 'Radar'];
const _types3d = ['Column', 'Stacked Column', 'Bar', 'Stacked Bar', 'Line', 'Area', 'Stacked Area', 'Pie'];
const _typesInteractive = ['Column', 'Bar', 'Scatter', 'Bubble'];
const _fallback = [Color(0xFF00A2FF), Color(0xFF61D836), Color(0xFF999999), Color(0xFFF8BA00), Color(0xFFEF5FA7), Color(0xFF00C7BE)];

Future<String?> showChartPopover(BuildContext context, Rect anchor, Map<String, dynamic> data) {
  final styles = [for (final s in (data['styles'] as List? ?? [])) s as Map<String, dynamic>];
  return showDesktopPopover<String>(context, anchor, centered: true, fit: true, width: 3 * 84 + 2 * 8 + 12, child: (ctx) => _ChartPicker(styles: styles, onPick: (a) => Navigator.of(ctx).pop(a)));
}

class _ChartPicker extends StatefulWidget {
  const _ChartPicker({required this.styles, required this.onPick});
  final List<Map<String, dynamic>> styles;
  final void Function(String action) onPick;
  @override
  State<_ChartPicker> createState() => _ChartPickerState();
}

class _ChartPickerState extends State<_ChartPicker> {
  static int _tab = 0, _page = 0;
  final _pages = PageController(initialPage: _page);

  @override
  void dispose() {
    _pages.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final look = LookScope.of(context);
    final types = switch (_tab) { 0 => _types2d, 1 => _types3d, _ => _typesInteractive };
    final prefix = const ['2D', '3D', 'Interactive'][_tab];
    final pages = math.max(1, widget.styles.length);
    final rows = (types.length + 2) ~/ 3;
    final gridH = rows * 76.0 + (rows - 1) * 8;
    return Column(mainAxisSize: MainAxisSize.min, children: [
      // the switch, touch-sized: 34 tall, round ends, 14-point labels
      Container(
        height: 34,
        decoration: BoxDecoration(color: touchFill(look), borderRadius: BorderRadius.circular(17)),
        child: Row(children: [
          for (final (i, t) in const ['2D', '3D', 'Interactive'].indexed)
            Expanded(
              child: GestureDetector(
                onTap: () => setState(() => _tab = i),
                child: Container(
                  margin: const EdgeInsets.all(2),
                  decoration: BoxDecoration(color: _tab == i ? look.accent : null, borderRadius: BorderRadius.circular(15)),
                  alignment: Alignment.center,
                  child: Text(t, style: Look.font(14, _tab == i ? Colors.white : look.text)),
                ),
              ),
            ),
        ]),
      ),
      const SizedBox(height: 8 + 6),
      // the style pages side by side, swiped (the desktop's strip)
      SizedBox(
        height: gridH,
        child: PageView.builder(
          controller: _pages,
          itemCount: pages,
          onPageChanged: (p) => setState(() => _page = p),
          itemBuilder: (ctx, k) {
            final cols = widget.styles.isEmpty ? _fallback : [for (final c in (widget.styles[k]['colors'] as List? ?? [])) colorOf(c)];
            final cs = cols.isEmpty ? _fallback : cols;
            return Wrap(spacing: 8, runSpacing: 8, children: [
              for (final ty in types)
                _ChartTile(
                  onTap: () => widget.onPick('chart:$prefix $ty:${widget.styles.isEmpty ? '' : widget.styles[k]['id']}'),
                  paint: (c, r) => chartPreview(c, r.deflate(10), ty, cs, _tab == 1, _tab == 2),
                ),
            ]);
          },
        ),
      ),
      if (pages > 1) ...[
        const SizedBox(height: 6 + 6),
        Row(mainAxisAlignment: MainAxisAlignment.center, children: [
          GestureDetector(onTap: _page > 0 ? () => _pages.animateToPage(_page - 1, duration: const Duration(milliseconds: 260), curve: Curves.easeOutBack) : null, child: Padding(padding: const EdgeInsets.symmetric(horizontal: 10), child: Text('‹', style: Look.font(20, _page > 0 ? look.text : look.textDim)))),
          for (var k = 0; k < pages; k++)
            GestureDetector(
              onTap: () => _pages.animateToPage(k, duration: const Duration(milliseconds: 260), curve: Curves.easeOutBack),
              child: SizedBox(width: 26, height: 30, child: Center(child: Container(width: 8, height: 8, decoration: BoxDecoration(shape: BoxShape.circle, color: k == _page ? look.text : look.textDim.withValues(alpha: 0.5))))),
            ),
          GestureDetector(onTap: _page + 1 < pages ? () => _pages.animateToPage(_page + 1, duration: const Duration(milliseconds: 260), curve: Curves.easeOutBack) : null, child: Padding(padding: const EdgeInsets.symmetric(horizontal: 10), child: Text('›', style: Look.font(20, _page + 1 < pages ? look.text : look.textDim)))),
        ]),
      ],
    ]);
  }
}

class _ChartTile extends StatefulWidget {
  const _ChartTile({required this.onTap, required this.paint});
  final VoidCallback onTap;
  final void Function(Canvas, Rect) paint;
  @override
  State<_ChartTile> createState() => _ChartTileState();
}

class _ChartTileState extends State<_ChartTile> {
  bool _down = false;
  @override
  Widget build(BuildContext context) {
    final look = LookScope.of(context);
    return GestureDetector(
      onTapDown: (_) => setState(() => _down = true),
      onTapCancel: () => setState(() => _down = false),
      onTapUp: (_) {
        setState(() => _down = false);
        widget.onTap();
      },
      child: CustomPaint(
        size: const Size(84, 76),
        painter: _P((c, s) {
          final r = Offset.zero & s;
          final rr = RRect.fromRectAndRadius(r, const Radius.circular(8));
          c.drawRRect(rr, Paint()..color = _down ? look.accent.withValues(alpha: 0.25) : const Color(0xFFF2F2F2));
          if (_down) {
            c.drawRRect(rr.deflate(0.75), Paint()
              ..style = PaintingStyle.stroke
              ..strokeWidth = 1.5
              ..color = look.accent);
          }
          widget.paint(c, r);
        }),
      ),
    );
  }
}

Color _col(List<Color> cols, int i) => cols[i % cols.length];
Color _shade(Color c, double k) => Color.from(alpha: 1, red: c.r * k, green: c.g * k, blue: c.b * k);
Color _lighten(Color c) => Color.from(alpha: 1, red: c.r + (1 - c.r) * 0.3, green: c.g + (1 - c.g) * 0.3, blue: c.b + (1 - c.b) * 0.3);

void _bar3d(Canvas c, Rect r, Color col, Offset depth) {
  c.drawPath(Path()..addPolygon([r.topLeft, r.topLeft + depth, r.topRight + depth, r.topRight], true), Paint()..color = _lighten(col));
  c.drawPath(Path()..addPolygon([r.topRight, r.topRight + depth, r.bottomRight + depth, r.bottomRight], true), Paint()..color = _shade(col, 0.7));
  c.drawRect(r, Paint()..color = col);
}

/// kn_ui chartpicker preview: a small picture of chart type `ty` in `r`.
void chartPreview(Canvas p, Rect r0, String ty, List<Color> cols, bool threeD, bool interactive) {
  final axis = Paint()
    ..strokeWidth = 1
    ..color = const Color(0xFFB0B0B0);
  final depth = threeD ? const Offset(4, -4) : Offset.zero;
  var r = r0;
  if (interactive) {
    final s = Rect.fromLTRB(r.left + 6, r.bottom - 4, r.right - 6, r.bottom - 2);
    p.drawRRect(RRect.fromRectAndRadius(s, const Radius.circular(1)), Paint()..color = const Color(0xFFC4C4C4));
    final k = Offset(s.left + s.width * 0.35, s.center.dy);
    p.drawCircle(k, 3, Paint()..color = Colors.white);
    p.drawCircle(k, 3, Paint()
      ..style = PaintingStyle.stroke
      ..strokeWidth = 0.8
      ..color = const Color(0xFF909090));
    r = Rect.fromLTRB(r.left, r.top, r.right, r.bottom - 9);
  }
  void base() => p.drawLine(r.bottomLeft, r.bottomRight, axis);
  const sets = [[0.45, 0.7], [0.65, 0.5], [0.9, 0.75]];
  Paint fill(Color c) => Paint()..color = c;
  void poly(List<Offset> pts, Color c) => p.drawPath(Path()..addPolygon(pts, true), fill(c));
  void line(List<Offset> pts, double w, Color c) => p.drawPath(
      Path()..addPolygon(pts, false),
      Paint()
        ..style = PaintingStyle.stroke
        ..strokeWidth = w
        ..color = c);
  switch (ty) {
    case 'Column' || '2-Axis' || 'Mixed':
      base();
      final w = r.width / 9;
      for (final (k, s) in sets.indexed) {
        for (final (j, v) in s.indexed) {
          final x = r.left + w * 0.5 + k * w * 3 + j * w;
          final br = Rect.fromLTRB(x, r.bottom - v * (r.height - 4), x + w * 0.9, r.bottom);
          if (threeD) {
            _bar3d(p, br, _col(cols, j), depth);
          } else if (ty != 'Column' && j == 1) {
          } else {
            p.drawRect(br, fill(_col(cols, j)));
          }
        }
      }
      if (ty != 'Column' && !threeD) {
        final pts = [for (final (k, s) in sets.indexed) Offset(r.left + w * 2 + k * w * 3, r.bottom - s[1] * (r.height - 4))];
        line(pts, 2, _col(cols, 1));
        for (final q in pts) {
          p.drawCircle(q, 2, fill(_col(cols, 1)));
        }
        if (ty == '2-Axis') p.drawLine(r.topRight, r.bottomRight, axis);
      }
      if (ty != 'Column' || threeD) p.drawLine(r.topLeft, r.bottomLeft, axis);
    case 'Stacked Column':
      base();
      final w = r.width / 5;
      for (final (k, s) in sets.indexed) {
        final x = r.left + w * 0.6 + k * w * 1.6;
        var y = r.bottom;
        for (final (j, v) in s.indexed) {
          final h = v * (r.height - 4) / 1.7;
          final br = Rect.fromLTRB(x, y - h, x + w, y);
          if (threeD && j == s.length - 1) {
            _bar3d(p, br, _col(cols, j), depth);
          } else {
            if (threeD) poly([br.topRight, br.topRight + depth, br.bottomRight + depth, br.bottomRight], _shade(_col(cols, j), 0.7));
            p.drawRect(br, fill(_col(cols, j)));
          }
          y -= h;
        }
      }
    case 'Bar':
      p.drawLine(r.topLeft, r.bottomLeft, axis);
      final h = r.height / 9;
      for (final (k, s) in sets.indexed) {
        for (final (j, v) in s.indexed) {
          final y = r.top + h * 0.5 + k * h * 3 + j * h;
          final br = Rect.fromLTRB(r.left, y, r.left + v * (r.width - 4), y + h * 0.9);
          threeD ? _bar3d(p, br, _col(cols, j), depth) : p.drawRect(br, fill(_col(cols, j)));
        }
      }
    case 'Stacked Bar':
      p.drawLine(r.topLeft, r.bottomLeft, axis);
      final h = r.height / 5;
      for (final (k, s) in sets.indexed) {
        final y = r.top + h * 0.6 + k * h * 1.6;
        var x = r.left;
        for (final (j, v) in s.indexed) {
          final w = v * (r.width - 4) / 1.7;
          final br = Rect.fromLTRB(x, y, x + w, y + h);
          threeD ? _bar3d(p, br, _col(cols, j), depth) : p.drawRect(br, fill(_col(cols, j)));
          x += w;
        }
      }
    case 'Line':
      base();
      p.drawLine(r.topLeft, r.bottomLeft, axis);
      for (var j = 0; j < 2; j++) {
        final ys = j == 0 ? [0.3, 0.55, 0.45, 0.8] : [0.15, 0.3, 0.6, 0.5];
        final pts = [for (final (k, v) in ys.indexed) Offset(r.left + 4 + k * (r.width - 8) / 3, r.bottom - v * (r.height - 4))];
        if (threeD) {
          line([for (final q in pts) q + depth], 4, _shade(_col(cols, j), 0.65));
          line(pts, 4, _col(cols, j));
        } else {
          line(pts, 2, _col(cols, j));
          for (final q in pts) {
            p.drawCircle(q, 2.2, fill(_col(cols, j)));
          }
        }
      }
    case 'Area' || 'Stacked Area':
      base();
      final series = ty == 'Area' ? [[0.55, 0.8, 0.6, 0.9], [0.25, 0.45, 0.3, 0.5]] : [[0.35, 0.5, 0.4, 0.55], [0.2, 0.3, 0.25, 0.35]];
      var below = [0.0, 0.0, 0.0, 0.0];
      for (final (j, s) in series.indexed) {
        final tops = [for (var k = 0; k < 4; k++) ty == 'Area' ? s[k] : s[k] + below[k]];
        double x(int k) => r.left + k * r.width / 3;
        double y(double v) => r.bottom - v * (r.height - 4);
        for (var k = 0; k < 3; k++) {
          final (b0, b1) = ty == 'Area' ? (0.0, 0.0) : (below[k], below[k + 1]);
          final pts = [Offset(x(k), y(b0)), Offset(x(k), y(tops[k])), Offset(x(k + 1), y(tops[k + 1])), Offset(x(k + 1), y(b1))];
          if (threeD) poly([for (final q in pts) q + depth * (1.0 - j)], _shade(_col(cols, j), 0.75));
          poly(pts, _col(cols, j).withValues(alpha: ty == 'Area' && j == 0 ? 0.9 : 1));
        }
        if (ty != 'Area') below = tops;
      }
    case 'Pie' || 'Donut':
      final ctr = r.center;
      final rad = math.min(r.width, r.height) / 2 - 1;
      const parts = [0.38, 0.24, 0.2, 0.18];
      var a = -math.pi / 2;
      final squash = threeD ? 0.6 : 1.0;
      Offset at(double ang, double rr, double lift) => Offset(ctr.dx + rr * math.cos(ang), ctr.dy + lift + rr * math.sin(ang) * squash);
      if (threeD) {
        var a2 = a;
        for (final (j, f) in parts.indexed) {
          final b = a2 + f * 2 * math.pi;
          const steps = 10;
          for (var s = 0; s < steps; s++) {
            final u = a2 + (b - a2) * s / steps, v = a2 + (b - a2) * (s + 1) / steps;
            if (math.sin(u) > -0.2 || math.sin(v) > -0.2) poly([at(u, rad, 0), at(v, rad, 0), at(v, rad, 6), at(u, rad, 6)], _shade(_col(cols, j), 0.7));
          }
          a2 = b;
        }
      }
      for (final (j, f) in parts.indexed) {
        final b = a + f * 2 * math.pi;
        const steps = 16;
        poly([ctr, for (var s = 0; s <= steps; s++) at(a + (b - a) * s / steps, rad, 0)], _col(cols, j));
        a = b;
      }
      if (ty == 'Donut') p.drawOval(Rect.fromCenter(center: ctr, width: rad * 1.05, height: rad * squash * 1.05), fill(const Color(0xFFF2F2F2)));
    case 'Scatter' || 'Bubble':
      base();
      p.drawLine(r.topLeft, r.bottomLeft, axis);
      const pts = [(0.15, 0.3), (0.3, 0.55), (0.45, 0.4), (0.6, 0.75), (0.75, 0.6), (0.85, 0.85), (0.25, 0.15), (0.55, 0.25), (0.8, 0.35)];
      for (final (k, (x, y)) in pts.indexed) {
        final q = Offset(r.left + x * r.width, r.bottom - y * r.height);
        final j = k ~/ 6 + (k >= 6 ? 1 : 0);
        if (ty == 'Bubble') {
          p.drawCircle(q, 2.5 + ((k * 7) % 5) * 1.4, fill(_col(cols, j).withValues(alpha: 0.85)));
        } else {
          p.drawCircle(q, 2.6, fill(_col(cols, j)));
        }
      }
    case 'Radar':
      final ctr = r.center;
      final rad = math.min(r.width, r.height) / 2;
      const n = 6;
      Offset at(int k, double f) {
        final a = -math.pi / 2 + k * 2 * math.pi / n;
        return Offset(ctr.dx + rad * f * math.cos(a), ctr.dy + rad * f * math.sin(a));
      }

      for (final ring in [0.5, 1.0]) {
        p.drawPath(Path()..addPolygon([for (var k = 0; k < n; k++) at(k, ring)], true), Paint()
          ..style = PaintingStyle.stroke
          ..strokeWidth = 1
          ..color = const Color(0xFFB0B0B0));
      }
      for (final (j, vals) in const [[0.9, 0.6, 0.8, 0.5, 0.7, 0.65], [0.5, 0.85, 0.4, 0.75, 0.45, 0.35]].indexed) {
        final pts = [for (final (k, f) in vals.indexed) at(k, f)];
        poly([...pts], _col(cols, j).withValues(alpha: 0.55));
        p.drawPath(Path()..addPolygon(pts, true), Paint()
          ..style = PaintingStyle.stroke
          ..strokeWidth = 1.5
          ..color = _col(cols, j));
      }
  }
}
