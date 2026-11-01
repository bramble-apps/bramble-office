// The controls the desktop canvas draws over the sheet (canvas.rs, from the engine's overlays
// "controls"): the guides and View > Show Rulers' rulers (a guide dragged out of a ruler, moved,
// or dropped back to remove it), the green badge on linked objects, a shape's points being
// edited (points.rs), the ▶ on movies, sounds and web videos, a gallery's arrows and "2 of 5",
// the cursor cell's comment (the desktop shows it on hover), and the bars of Draw with Pen,
// Edit Mask and Edit Data References. Drawn as the desktop draws them; taps and drags go to the
// engine as its "ctl:" actions.
import 'dart:math' as math;

import 'package:flutter/material.dart';

import 'look.dart';
import 'widgets.dart';

/// canvas.rs guides: Numbers 15.4's #E8CB44 (the desktop's 0xeeca00).
const guideColor = Color(0xFFEECA00);

/// canvas.rs RULER_BAND.
const rulerBand = 18.0;

/// What a finger or click at a point hits among the controls.
sealed class ControlHit {}

/// A tap that runs an engine action.
class TapHit extends ControlHit {
  TapHit(this.action);
  final String action;
}

/// A shape's point (part 0) or one of its handles (1 in, 2 out).
class PointHit extends ControlHit {
  PointHit(this.s, this.i, this.part);
  final int s, i, part;
}

/// A guide (index -1: a new one out of a ruler).
class GuideHit extends ControlHit {
  GuideHit(this.index, this.vertical, this.pos);
  final int index;
  final bool vertical;
  final double pos;
}

/// A drag on the controls going on: a guide, a point, the pen's stroke.
class ControlDrag {
  ControlDrag.guide(GuideHit g)
      : guide = g,
        point = null,
        pen = null,
        pos = g.pos;
  ControlDrag.point(PointHit p)
      : point = p,
        guide = null,
        pen = null;
  ControlDrag.pen(Offset first)
      : pen = [first],
        guide = null,
        point = null;
  final GuideHit? guide;
  final PointHit? point;

  /// The pen's samples (sheet points).
  final List<Offset>? pen;

  /// Where the guide is (sheet points); null when it's off the sheet (it goes).
  double? pos;

  /// How far the point moved (sheet points).
  Offset delta = Offset.zero;
}

class SheetControls {
  SheetControls(this.c, this.toLocal, this.zoom, this.view);

  /// api.rs overlays "controls".
  final Map<String, dynamic> c;
  final Offset Function(double x, double y) toLocal;
  final double zoom;
  final Size view;

  static List<double> _l(dynamic v) => [for (final n in v as List) (n as num).toDouble()];

  Rect frame(dynamic f) {
    final v = _l(f);
    return Rect.fromPoints(toLocal(v[0], v[1]), toLocal(v[2], v[3]));
  }

  Map<String, dynamic> get _guides => (c['guides'] as Map?)?.cast<String, dynamic>() ?? const {};
  bool get rulers => _guides['rulers'] == true;
  bool get penOn => c['pen'] is Map;

  /// The sheet point under a local point.
  Offset toSheet(Offset p) {
    final o = toLocal(0, 0);
    return (p - o) / zoom;
  }

  List<(bool, double)> guides() => [for (final g in (_guides['lines'] as List? ?? [])) (g[0] == true, (g[1] as num).toDouble())];

  /// The badges of linked objects: (object, the badge's rect).
  List<(int, Rect)> links() => [
        for (final l in (c['links'] as List? ?? []))
          (l['id'] as int, Rect.fromCenter(center: frame(l['frame']).topRight + const Offset(-2, 2), width: 20, height: 20)),
      ];

  /// The points being edited (local): [s, i, kind, point, in, out].
  List<(int, int, int, Offset, Offset, Offset)> points() {
    final m = c['points'];
    if (m is! Map) return [];
    final o = _l(m['origin']);
    Offset at(dynamic x, dynamic y) => toLocal(o[0] + (x as num).toDouble(), o[1] + (y as num).toDouble());
    return [
      for (final n in (m['nodes'] as List))
        if (n is List) ((n[0] as num).toInt(), (n[1] as num).toInt(), (n[2] as num).toInt(), at(n[3], n[4]), at(n[5], n[6]), at(n[7], n[8])),
    ];
  }

  (int, int)? get selectedPoint {
    final s = (c['points'] as Map?)?['selected'];
    return s is List ? ((s[0] as num).toInt(), (s[1] as num).toInt()) : null;
  }

  /// Movies, sounds and web videos in view: (the object, its frame, the ▶'s centre and radius).
  List<(Map, Rect, Offset, double)> media() {
    final out = <(Map, Rect, Offset, double)>[];
    for (final m in (c['media'] as List? ?? [])) {
      final r = frame(m['frame']);
      if (!r.overlaps(Offset.zero & view)) continue;
      out.add((m as Map, r, r.center, (math.min(r.width, r.height) * 0.12).clamp(14.0, 40.0)));
    }
    return out;
  }

  /// The selected gallery's arrows (left, right), its "2 of 5" pill and that label.
  (Rect, Rect, Rect, String)? galleryRects() {
    final m = c['gallery'];
    if (m is! Map) return null;
    final r = frame(m['frame']);
    final label = '${(m['shown'] as int) + 1} of ${m['n']}';
    return (
      Rect.fromCenter(center: Offset(r.left + 22, r.center.dy), width: 30, height: 30),
      Rect.fromCenter(center: Offset(r.right - 22, r.center.dy), width: 30, height: 30),
      Rect.fromCenter(center: Offset(r.center.dx, r.bottom - 18), width: label.length * 7.5 + 20, height: 22),
      label,
    );
  }

  /// What's at `p` (local), the desktop's order: points, the rulers and guides, the buttons.
  ControlHit? hit(Offset p, {bool touch = true, bool drag = false}) {
    final reach = touch ? 14.0 : 6.0;
    final sel = selectedPoint;
    final pts = points();
    for (final (s, i, kind, _, inc, out) in pts) {
      if (sel == (s, i) && kind != 0) {
        if ((inc - p).distance <= reach) return PointHit(s, i, 1);
        if ((out - p).distance <= reach) return PointHit(s, i, 2);
      }
    }
    for (final (s, i, _, pt, _, _) in pts) {
      if ((pt - p).distance <= reach) return PointHit(s, i, 0);
    }
    final sp = toSheet(p);
    if (drag) {
      // a ruler: a new guide (the top ruler's are horizontal); a guide: moved
      if (rulers && p.dy < rulerBand && p.dx > rulerBand) return GuideHit(-1, false, sp.dy);
      if (rulers && p.dx < rulerBand && p.dy > rulerBand) return GuideHit(-1, true, sp.dx);
      final tol = (touch ? 10.0 : 4.0) / zoom;
      for (final (k, (v, pos)) in guides().indexed) {
        if (((v ? sp.dx : sp.dy) - pos).abs() < tol) return GuideHit(k, v, pos);
      }
      return null;
    }
    for (final (id, r) in links()) {
      if (r.inflate(touch ? 8 : 0).contains(p)) return TapHit('ctl:follow:$id');
    }
    for (final (m, r, centre, rad) in media()) {
      if ((m['selected'] == true || !touch) && (centre - p).distance <= rad + (touch ? 6 : 0)) return TapHit('ctl:mediaopen:${m['id']}');
      if (m['selected'] != true && touch && r.contains(p)) continue;
    }
    final g = galleryRects();
    if (g != null) {
      final m = c['gallery'] as Map;
      final n = m['n'] as int, shown = m['shown'] as int;
      if (g.$1.inflate(touch ? 6 : 0).contains(p)) return TapHit('ctl:galleryshow:${m['id']}:${(shown - 1 + n) % n}');
      if (g.$2.inflate(touch ? 6 : 0).contains(p)) return TapHit('ctl:galleryshow:${m['id']}:${(shown + 1) % n}');
    }
    return null;
  }

  // ---------------------------------------------------------------- drawing

  void paint(Canvas canvas, Look look, ControlDrag? drag) {
    final size = view;
    // the guides (one being moved: where it is now)
    final gp = Paint()
      ..color = guideColor
      ..strokeWidth = 1;
    final moving = drag?.guide;
    for (final (k, (v, pos)) in guides().indexed) {
      if (moving != null && moving.index == k) continue;
      _guideLine(canvas, v, pos, gp);
    }
    if (moving != null && drag!.pos != null) {
      _guideLine(canvas, moving.vertical, drag.pos!, gp);
      final at = moving.vertical ? toLocal(drag.pos!, 0) : toLocal(0, drag.pos!);
      _tip(canvas, look, moving.vertical ? Offset(at.dx + 12, rulerBand + 12) : Offset(rulerBand + 12, at.dy + 12), '${moving.vertical ? 'x' : 'y'} ${drag.pos!.round()} pt');
    }
    if (rulers) _rulers(canvas, look, size);
    // link badges: a green circle with a white arrow (links.rs badge)
    for (final (_, r) in links()) {
      canvas.drawCircle(r.center, 9, Paint()..color = const Color(0xFF30B94D));
      final a = Paint()
        ..color = Colors.white
        ..strokeWidth = 1.6
        ..style = PaintingStyle.stroke
        ..strokeCap = StrokeCap.round;
      final ctr = r.center;
      canvas.drawLine(ctr + const Offset(-3, 3), ctr + const Offset(3, -3), a);
      canvas.drawPath(Path()..moveTo(ctr.dx - 1, ctr.dy - 3)..lineTo(ctr.dx + 3, ctr.dy - 3)..lineTo(ctr.dx + 3, ctr.dy + 1), a);
    }
    // movies and web videos: the ▶ on the selected one (a dark disc, a white triangle)
    for (final (m, _, centre, rad) in media()) {
      if (m['selected'] != true) continue;
      canvas.drawCircle(centre, rad, Paint()..color = const Color(0x99000000));
      final t = rad * 0.45;
      canvas.drawPath(
        Path()
          ..moveTo(centre.dx - t * 0.7, centre.dy - t)
          ..lineTo(centre.dx + t, centre.dy)
          ..lineTo(centre.dx - t * 0.7, centre.dy + t)
          ..close(),
        Paint()..color = Colors.white,
      );
    }
    // the selected gallery: ‹ › and "2 of 5"
    final g = galleryRects();
    if (g != null) {
      for (final (k, r) in [g.$1, g.$2].indexed) {
        canvas.drawCircle(r.center, 15, Paint()..color = const Color(0x8C000000));
        final a = Paint()
          ..color = Colors.white
          ..strokeWidth = 2
          ..style = PaintingStyle.stroke;
        final d = k == 0 ? 1.0 : -1.0;
        canvas.drawPath(Path()..moveTo(r.center.dx + 3 * d, r.center.dy - 6)..lineTo(r.center.dx - 3 * d, r.center.dy)..lineTo(r.center.dx + 3 * d, r.center.dy + 6), a);
      }
      canvas.drawRRect(RRect.fromRectAndRadius(g.$3, const Radius.circular(11)), Paint()..color = const Color(0x8C000000));
      final tp = TextPainter(text: TextSpan(text: g.$4, style: Look.font(12, Colors.white)), textDirection: TextDirection.ltr)..layout();
      tp.paint(canvas, g.$3.center - Offset(tp.width / 2, tp.height / 2));
    }
    // a shape's points: squares for sharp ones, circles for the others, the selected one's handles
    final accent = look.accent;
    final sel = selectedPoint;
    final pd = drag?.point;
    for (final (s, i, kind, pt0, inc0, out0) in points()) {
      var (pt, inc, out) = (pt0, inc0, out0);
      if (pd != null && pd.s == s && pd.i == i) {
        final d = drag!.delta * zoom;
        if (pd.part == 0) {
          (pt, inc, out) = (pt + d, inc + d, out + d);
        } else if (pd.part == 1) {
          inc += d;
          if (kind == 1) out = pt * 2 - inc;
        } else {
          out += d;
          if (kind == 1) inc = pt * 2 - out;
        }
      }
      final on = sel == (s, i);
      final stroke = Paint()
        ..color = accent
        ..strokeWidth = 1
        ..style = PaintingStyle.stroke;
      if (on && kind != 0) {
        for (final h in [inc, out]) {
          canvas.drawLine(pt, h, stroke);
          canvas.drawCircle(h, 3.5, Paint()..color = Colors.white);
          canvas.drawCircle(h, 3.5, stroke);
        }
      }
      final fill = Paint()..color = on ? accent : Colors.white;
      if (kind == 0) {
        final r = Rect.fromCenter(center: pt, width: 8, height: 8);
        canvas.drawRect(r, fill);
        canvas.drawRect(r, stroke);
      } else {
        canvas.drawCircle(pt, 4.5, fill);
        canvas.drawCircle(pt, 4.5, stroke);
      }
    }
    // the stroke being drawn with the pen
    final pen = drag?.pen;
    final pc = c['pen'];
    if (pen != null && pen.length > 1 && pc is Map) {
      final col = pc['color'] as List;
      final path = Path();
      for (final (k, q) in pen.indexed) {
        final p = toLocal(q.dx, q.dy);
        k == 0 ? path.moveTo(p.dx, p.dy) : path.lineTo(p.dx, p.dy);
      }
      canvas.drawPath(
        path,
        Paint()
          ..color = Color.fromARGB(255, col[0] as int, col[1] as int, col[2] as int)
          ..strokeWidth = (pc['width'] as num).toDouble() * zoom
          ..style = PaintingStyle.stroke
          ..strokeCap = StrokeCap.round
          ..strokeJoin = StrokeJoin.round,
      );
    }
  }

  void _guideLine(Canvas canvas, bool vertical, double pos, Paint p) {
    if (vertical) {
      final x = toLocal(pos, 0).dx;
      canvas.drawLine(Offset(x, 0), Offset(x, view.height), p);
    } else {
      final y = toLocal(0, pos).dy;
      canvas.drawLine(Offset(0, y), Offset(view.width, y), p);
    }
  }

  void _tip(Canvas canvas, Look look, Offset at, String text) {
    final tp = TextPainter(text: TextSpan(text: text, style: Look.font(11, Colors.white)), textDirection: TextDirection.ltr)..layout();
    final r = Rect.fromLTWH(at.dx, at.dy, tp.width + 12, tp.height + 6);
    canvas.drawRRect(RRect.fromRectAndRadius(r, const Radius.circular(4)), Paint()..color = const Color(0xE0303030));
    tp.paint(canvas, r.topLeft + const Offset(6, 3));
  }

  /// canvas.rs draw_rulers: along the top and left, in Settings' units, from the sheet's
  /// top-left; the labels every major tick that has room.
  void _rulers(Canvas canvas, Look look, Size size) {
    final bg = Paint()..color = look.header;
    canvas.drawRect(Rect.fromLTWH(0, 0, size.width, rulerBand), bg);
    canvas.drawRect(Rect.fromLTWH(0, 0, rulerBand, size.height), bg);
    final edge = Paint()
      ..color = look.separator
      ..strokeWidth = 1;
    canvas.drawLine(const Offset(0, rulerBand), Offset(size.width, rulerBand), edge);
    canvas.drawLine(const Offset(rulerBand, 0), Offset(rulerBand, size.height), edge);
    final tick = Paint()
      ..color = look.textDim
      ..strokeWidth = 1;
    final units = _guides['units'] as int? ?? 0;
    final (unitPt, major, minor) = switch (units) { 1 => (72.0, 1.0, 0.125), 2 => (72.0 / 2.54, 1.0, 0.25), _ => (1.0, 100.0, 10.0) };
    var every = 1.0;
    while (major * every * unitPt * zoom < 40) {
      every *= every == 1 ? 2 : 2.5;
    }
    final minorPt = minor * unitPt;
    final perMajor = (major / minor).round();
    final perLabel = perMajor * every.round();
    for (final axis in [0, 1]) {
      final a = axis == 0 ? toSheet(const Offset(rulerBand, 0)).dx : toSheet(const Offset(0, rulerBand)).dy;
      final b = axis == 0 ? toSheet(Offset(size.width, 0)).dx : toSheet(Offset(0, size.height)).dy;
      final n0 = (a / minorPt).floor(), n1 = (b / minorPt).ceil();
      if (n1 - n0 > 6000) continue;
      for (var n = n0; n <= n1; n++) {
        final v = n * minorPt;
        final isMajor = n % perMajor == 0;
        if (!isMajor && minorPt * zoom < 4) continue;
        final l = isMajor ? rulerBand * 0.55 : rulerBand * 0.25;
        String? label;
        if (n % perLabel == 0) {
          final val = n * minor;
          label = (val - val.roundToDouble()).abs() < 0.001 ? val.round().toString() : '$val';
        }
        if (axis == 0) {
          final x = toLocal(v, 0).dx;
          if (x < rulerBand) continue;
          canvas.drawLine(Offset(x, rulerBand - l), Offset(x, rulerBand), tick);
          if (label != null) (TextPainter(text: TextSpan(text: label, style: Look.font(9, look.textDim)), textDirection: TextDirection.ltr)..layout()).paint(canvas, Offset(x + 2, 1));
        } else {
          final y = toLocal(0, v).dy;
          if (y < rulerBand) continue;
          canvas.drawLine(Offset(rulerBand - l, y), Offset(rulerBand, y), tick);
          if (label != null) (TextPainter(text: TextSpan(text: label, style: Look.font(9, look.textDim)), textDirection: TextDirection.ltr)..layout()).paint(canvas, Offset(1, y + 2));
        }
      }
    }
  }
}

/// A popup's frame (egui's Frame::popup): the panel colour, rounded, a hairline and a shadow.
class _Popup extends StatelessWidget {
  const _Popup({required this.child});
  final Widget child;
  @override
  Widget build(BuildContext context) {
    final look = LookScope.of(context);
    return Container(
      padding: const EdgeInsets.symmetric(horizontal: 14, vertical: 10),
      decoration: BoxDecoration(
        color: look.panel,
        borderRadius: BorderRadius.circular(12),
        border: Border.all(color: look.windowStroke),
        boxShadow: const [BoxShadow(color: Color(0x50000000), blurRadius: 16, offset: Offset(0, 4))],
      ),
      child: child,
    );
  }
}

/// Draw with Pen's bar (canvas.rs pen_bar): six colours, the width, Done.
class PenBar extends StatelessWidget {
  const PenBar({super.key, required this.pen, required this.act});
  final Map pen;
  final void Function(String a) act;
  static const _colors = [Color(0xFF000000), Color(0xFF0A84FF), Color(0xFFFF453A), Color(0xFF30D158), Color(0xFFFFD60A), Color(0xFFFFFFFF)];
  @override
  Widget build(BuildContext context) {
    final look = LookScope.of(context);
    final cur = pen['color'] as List;
    final w = (pen['width'] as num).toDouble();
    int ch(double v) => (v * 255).round();
    return _Popup(
      child: Row(mainAxisSize: MainAxisSize.min, children: [
        for (final c in _colors)
          GestureDetector(
            onTap: () => act('ctl:pen:color:${ch(c.r)}:${ch(c.g)}:${ch(c.b)}'),
            child: Container(
              width: 30,
              height: 30,
              alignment: Alignment.center,
              decoration: BoxDecoration(shape: BoxShape.circle, border: ch(c.r) == cur[0] && ch(c.g) == cur[1] && ch(c.b) == cur[2] ? Border.all(color: look.accent, width: 2) : null),
              child: Container(width: 20, height: 20, decoration: BoxDecoration(color: c, shape: BoxShape.circle, border: Border.all(color: look.separator))),
            ),
          ),
        const SizedBox(width: 8),
        SizedBox(width: 120, child: DStepper(value: w, min: 1, max: 24, suffix: ' pt', onChanged: (v) => act('ctl:pen:width:$v'))),
        const SizedBox(width: 8),
        DButton(fill: look.accent, onTap: () => act('ctl:pen:done'), child: const Text('Done')),
      ]),
    );
  }
}

/// Edit Mask's bar under the mask (mask.rs mask_bar): the picture's size about the mask's centre
/// (100–500 %), Done.
class MaskBar extends StatefulWidget {
  const MaskBar({super.key, required this.pct, required this.act});
  final double pct;
  final void Function(String a) act;
  @override
  State<MaskBar> createState() => _MaskBarState();
}

class _MaskBarState extends State<MaskBar> {
  double? _v;
  @override
  Widget build(BuildContext context) {
    final look = LookScope.of(context);
    return _Popup(
      child: Row(mainAxisSize: MainAxisSize.min, children: [
        SizedBox(
          width: 180,
          child: Slider(
            value: (_v ?? widget.pct).clamp(100, 500),
            min: 100,
            max: 500,
            activeColor: look.accent,
            onChanged: (v) => setState(() => _v = v),
            onChangeEnd: (v) {
              widget.act('ctl:mask:size:$v');
              setState(() => _v = null);
            },
          ),
        ),
        DButton(fill: look.accent, onTap: () => widget.act('ctl:mask:done'), child: const Text('Done')),
      ]),
    );
  }
}

/// Edit Data References' bar at the foot of the sheet (datarefs.rs bar): Columns / Rows as
/// Series, Cancel, Done, and what to do.
class DataRefsBar extends StatelessWidget {
  const DataRefsBar({super.key, required this.rows, required this.act});
  final bool rows;
  final void Function(String a) act;
  @override
  Widget build(BuildContext context) {
    final look = LookScope.of(context);
    return _Popup(
      child: Column(mainAxisSize: MainAxisSize.min, crossAxisAlignment: CrossAxisAlignment.start, children: [
        Wrap(crossAxisAlignment: WrapCrossAlignment.center, spacing: 8, runSpacing: 6, children: [
          Text('Edit Data References', style: Look.font(13, look.text, bold: true)),
          SizedBox(width: 250, child: DSegmented(labels: const ['Columns as Series', 'Rows as Series'], cur: rows ? 1 : 0, onPick: (i) => act('ctl:datarefs:rows:$i'))),
          DButton(onTap: () => act('ctl:datarefs:cancel'), child: const Text('Cancel')),
          DButton(fill: look.accent, onTap: () => act('ctl:datarefs:done'), child: const Text('Done')),
        ]),
        const SizedBox(height: 4),
        Text('Select the cells to plot, then tap Done.', style: Look.font(11, look.textDim)),
      ]),
    );
  }
}

/// The cursor cell's comment, in Numbers' yellow note (canvas.rs: shown on hover there).
class CommentNote extends StatelessWidget {
  const CommentNote({super.key, required this.text, required this.author});
  final String text, author;
  @override
  Widget build(BuildContext context) {
    return Container(
      constraints: const BoxConstraints(maxWidth: 260),
      padding: const EdgeInsets.all(6),
      decoration: BoxDecoration(color: const Color(0xFFFFF6C8), borderRadius: BorderRadius.circular(4), border: Border.all(color: const Color(0xFFD8B430))),
      child: Text(author.isEmpty ? text : '$text\n— $author', style: Look.font(12, const Color(0xFF202020))),
    );
  }
}
