// The controls the desktop canvas draws on the pages (canvas.rs, from canvas::overlays
// "controls"): a shape's points being edited (points.rs), the guides (guides.rs), the Edit
// Chart Data capsule (chartedit.rs canvas_button), a gallery's arrows and "2 of 5"
// (mediainsp.rs gallery_controls), the ▶ on movies and web videos and the video playing in
// place (media.rs canvas_controls), the Insert Page Number button (hfnumber.rs ui). Drawn as
// the desktop draws them; taps and drags on them go to the engine as its actions.
import 'dart:math' as math;
import 'dart:ui' as ui;

import 'package:flutter/material.dart';

import 'icons.dart';
import 'look.dart';

/// guides.rs COLOR: Keynote / Pages guide yellow.
const guideColor = Color(0xFFEECA00);

/// What a finger or click at a point hits among the controls.
sealed class ControlHit {}

/// A tap that runs an engine action (or shows the page number menu).
class TapHit extends ControlHit {
  TapHit(this.action, {this.menu, this.menuAt});
  final String action;
  final List<dynamic>? menu;
  final Offset? menuAt;
}

/// A shape's point (part 0) or one of its handles (1 in, 2 out).
class PointHit extends ControlHit {
  PointHit(this.s, this.i, this.part, this.at);
  final int s, i, part;
  final Offset at;
}

/// One of the page's own guides.
class GuideHit extends ControlHit {
  GuideHit(this.page, this.index, this.vertical);
  final int page, index;
  final bool vertical;
}

/// A guide being dragged (index -1: a new one from the ruler) and where it is (page points).
class GuideDrag {
  GuideDrag(this.page, this.index, this.vertical);
  int page;
  final int index;
  final bool vertical;
  double? pos;
  Offset pointer = Offset.zero;
}

/// A point being dragged: which, and by how much so far (view points).
class PointDrag {
  PointDrag(this.s, this.i, this.part);
  final int s, i, part;
  Offset delta = Offset.zero;
}

class CanvasControls {
  CanvasControls(this.c, this.toLocal, this.zoom, this.pages, this.view, {this.dragging = false});

  /// canvas::overlays "controls".
  final Map<String, dynamic> c;
  final Offset Function(int page, double x, double y) toLocal;
  final double zoom;
  final List<Rect> pages;

  /// The view (local points): the controls are clipped to it.
  final Rect view;

  /// An engine drag going on (no buttons then).
  final bool dragging;

  List<double> _l(dynamic v) => [for (final n in v as List) (n as num).toDouble()];
  bool _ok(dynamic m) => m is Map && (m['page'] as int) < pages.length;

  Rect _frame(Map m) {
    final f = _l(m['frame']), p = m['page'] as int;
    return Rect.fromPoints(toLocal(p, f[0], f[1]), toLocal(p, f[2], f[3]));
  }

  // ---------------------------------------------------------------- places

  /// hfnumber.rs ui: the button below the header's band (above the footer's), under the field.
  Rect? pageNumberRect() {
    final m = c['pageNumber'];
    if (!_ok(m)) return null;
    final p = m['page'] as int;
    final f = _l(m['field']), b = _l(m['band']);
    final cx = (toLocal(p, f[0], f[1]).dx + toLocal(p, f[2], f[3]).dx) / 2;
    const size = Size(168, 24);
    final y = m['footer'] == true ? toLocal(p, b[0], b[1]).dy - size.height / 2 - 6 : toLocal(p, b[2], b[3]).dy + size.height / 2 + 6;
    final r = Rect.fromCenter(center: Offset(cx, y), width: size.width, height: size.height);
    return r.overlaps(view) ? r : null;
  }

  /// chartedit.rs canvas_button: 140 × 26 just below the chart.
  Rect? chartRect() {
    final m = c['chart'];
    if (dragging || !_ok(m)) return null;
    final f = _frame(m);
    final r = Rect.fromCenter(center: f.bottomCenter + Offset(0, 22 * zoom + 24), width: 140, height: 26);
    return r.overlaps(view) ? r : null;
  }

  /// mediainsp.rs gallery_controls: the arrows (left, right) and the "2 of 5" pill.
  (Rect, Rect, Rect, String)? galleryRects() {
    final m = c['gallery'];
    if (!_ok(m)) return null;
    final r = _frame(m);
    final label = '${(m['shown'] as int) + 1} of ${m['n']}';
    return (
      Rect.fromCenter(center: Offset(r.left + 22, r.center.dy), width: 30, height: 30),
      Rect.fromCenter(center: Offset(r.right - 22, r.center.dy), width: 30, height: 30),
      Rect.fromCenter(center: Offset(r.center.dx, r.bottom - 18), width: label.length * 7.5 + 20, height: 22),
      label,
    );
  }

  /// media.rs canvas_controls: the movies and web videos in view, each with its frame and ▶.
  List<(Map, Rect, Offset, double)> media() {
    final out = <(Map, Rect, Offset, double)>[];
    for (final m in (c['media'] as List? ?? [])) {
      if (!_ok(m)) continue;
      final r = _frame(m);
      if (!r.overlaps(view)) continue;
      out.add((m as Map, r, r.center, (math.min(r.width, r.height) * 0.12).clamp(14.0, 40.0)));
    }
    return out;
  }

  /// The points being edited (local): [s, i, kind, point, in, out].
  List<(int, int, int, Offset, Offset, Offset)> points() {
    final m = c['points'];
    if (!_ok(m)) return [];
    final p = m['page'] as int;
    final o = _l(m['origin']);
    Offset at(double x, double y) => toLocal(p, o[0] + x, o[1] + y);
    return [
      for (final n in (m['nodes'] as List))
        if (n is List) ((n[0] as num).toInt(), (n[1] as num).toInt(), (n[2] as num).toInt(), at(_d(n[3]), _d(n[4])), at(_d(n[5]), _d(n[6])), at(_d(n[7]), _d(n[8]))),
    ];
  }

  double _d(dynamic v) => (v as num).toDouble();

  (int, int)? get selectedPoint {
    final s = (c['points'] as Map?)?['selected'];
    return s is List ? ((s[0] as num).toInt(), (s[1] as num).toInt()) : null;
  }

  /// A guide's line across its page (local).
  (Offset, Offset) guideLine(int page, bool vertical, double pos) {
    final r = pages[page];
    return vertical ? (toLocal(page, pos, 0), toLocal(page, pos, r.height)) : (toLocal(page, 0, pos), toLocal(page, r.width, pos));
  }

  List<Map> guides() => [
        for (final g in ((c['guides'] as Map?)?['lines'] as List? ?? []))
          if ((g['page'] as int) < pages.length) g as Map,
      ];

  // ---------------------------------------------------------------- hits

  /// Whether an object is at `p` (local) on a page with guides.
  bool _overObject(Offset p) {
    for (final o in ((c['guides'] as Map?)?['objects'] as List? ?? [])) {
      final v = _l(o), page = v[0].toInt();
      if (page < pages.length && Rect.fromPoints(toLocal(page, v[1], v[2]), toLocal(page, v[3], v[4])).inflate(2).contains(p)) return true;
    }
    return false;
  }

  /// What's at `p` (local): the desktop's order (points, the buttons, the page number, the
  /// guides). `touch` widens the reach for a finger; a video plays at `dpr` pixels a point.
  ControlHit? hit(Offset p, {bool touch = false, double dpr = 1}) {
    final reach = touch ? 14.0 : 6.0;
    // points: a handle of the selected point first, then the points
    final sel = selectedPoint;
    final pts = points();
    for (final (s, i, kind, _, inc, out) in pts) {
      if (sel == (s, i) && kind != 0) {
        if ((inc - p).distance <= reach) return PointHit(s, i, 1, inc);
        if ((out - p).distance <= reach) return PointHit(s, i, 2, out);
      }
    }
    for (final (s, i, _, pt, _, _) in pts) {
      if ((pt - p).distance <= reach) return PointHit(s, i, 0, pt);
    }
    final grow = touch ? 6.0 : 0.0;
    final ch = chartRect();
    if (ch != null && ch.inflate(grow).contains(p)) return TapHit('chartedit:${(c['chart'] as Map)['id']}');
    for (final (m, r, centre, rad) in media()) {
      if (m['playing'] == true && r.contains(p)) return TapHit('mediaplay:${m['id']}:0:0');
      if ((m['selected'] == true || !touch) && (centre - p).distance <= rad + grow) return TapHit('mediaplay:${m['id']}:${(r.width * dpr).round()}:${(r.height * dpr).round()}');
    }
    final g = galleryRects();
    if (g != null) {
      final m = c['gallery'] as Map;
      final n = m['n'] as int, shown = m['shown'] as int;
      if (g.$1.inflate(grow).contains(p)) return TapHit('galleryshow:${m['id']}:${(shown + n - 1) % n}');
      if (g.$2.inflate(grow).contains(p)) return TapHit('galleryshow:${m['id']}:${(shown + 1) % n}');
    }
    final pn = pageNumberRect();
    if (pn != null && pn.inflate(grow).contains(p)) {
      final m = c['pageNumber'] as Map;
      return TapHit('', menu: m['items'] as List, menuAt: m['footer'] == true ? pn.topLeft - const Offset(0, 210) : pn.bottomLeft + const Offset(0, 4));
    }
    // guides (not over an object)
    if ((c['guides'] as Map?)?['shown'] == true && !_overObject(p)) {
      final tol = touch ? 12.0 : 4.0;
      for (final gd in guides()) {
        if (gd['template'] == true) continue;
        final page = gd['page'] as int;
        final v = gd['vertical'] == true;
        final pr = Rect.fromPoints(toLocal(page, 0, 0), toLocal(page, pages[page].width, pages[page].height));
        final (a, _) = guideLine(page, v, _d(gd['pos']));
        if (pr.inflate(2).contains(p) && (v ? (p.dx - a.dx).abs() : (p.dy - a.dy).abs()) < tol) return GuideHit(page, gd['index'] as int, v);
      }
    }
    return null;
  }

  // ---------------------------------------------------------------- drawing

  void paint(Canvas canvas, Look look, {Offset? hover, bool pageNumberHot = false, GuideDrag? guide, PointDrag? point, ui.Image? movie, int? movieId, int units = 0}) {
    final accent = look.accent;
    canvas.save();
    canvas.clipRect(view);
    // the guides (a template's fainter; the one being moved isn't drawn where it was)
    for (final g in guides()) {
      final page = g['page'] as int;
      if (guide != null && guide.index >= 0 && guide.page == page && guide.index == g['index'] && g['template'] != true) continue;
      final (a, b) = guideLine(page, g['vertical'] == true, _d(g['pos']));
      canvas.drawLine(a, b, Paint()
        ..strokeWidth = 1
        ..color = g['template'] == true ? guideColor.withValues(alpha: 0.55) : guideColor);
    }
    if (guide != null && guide.pos != null && guide.page < pages.length) {
      final (a, b) = guideLine(guide.page, guide.vertical, guide.pos!);
      canvas.drawLine(a, b, Paint()
        ..strokeWidth = 1
        ..color = guideColor);
      // the position, as Pages shows it
      final at = (guide.vertical ? Offset(a.dx, guide.pointer.dy) : Offset(guide.pointer.dx, a.dy)) + const Offset(8, 8);
      final tp = _text(fmtLen(guide.pos!, units), 12, Colors.white);
      final lr = Rect.fromLTWH(at.dx, at.dy, tp.width + 12, tp.height + 6);
      canvas.drawRRect(RRect.fromRectAndRadius(lr, const Radius.circular(4)), Paint()..color = const Color.fromARGB(200, 0, 0, 0));
      tp.paint(canvas, lr.topLeft + const Offset(6, 3));
    }
    // movies and web videos: the video playing over its frame, else a ▶ on the selected or
    // pointed-at one
    for (final (m, r, ctr, rad) in media()) {
      if (m['playing'] == true) {
        if (movie != null && movieId == m['id']) {
          canvas.drawImageRect(movie, Offset.zero & Size(movie.width.toDouble(), movie.height.toDouble()), r, Paint()..filterQuality = FilterQuality.medium);
        }
        continue;
      }
      if (!(m['selected'] == true || (hover != null && r.contains(hover)))) continue;
      canvas.drawCircle(ctr, rad, Paint()..color = const Color.fromARGB(140, 0, 0, 0));
      canvas.drawCircle(ctr, rad, Paint()
        ..style = PaintingStyle.stroke
        ..strokeWidth = 2
        ..color = Colors.white);
      final s = rad * 0.45;
      canvas.drawPath(Path()..addPolygon([ctr + Offset(-s * 0.55, -s), ctr + Offset(s, 0), ctr + Offset(-s * 0.55, s)], true), Paint()..color = Colors.white);
    }
    // a gallery's arrows and "2 of 5"
    final g = galleryRects();
    if (g != null) {
      for (final (b, ch) in [(g.$1, '‹'), (g.$2, '›')]) {
        final hot = hover != null && b.contains(hover);
        canvas.drawCircle(b.center, 15, Paint()..color = Color.fromARGB(hot ? 170 : 120, 0, 0, 0));
        final tp = _text(ch, 22, Colors.white);
        tp.paint(canvas, b.center - Offset(tp.width / 2, tp.height / 2));
      }
      canvas.drawRRect(RRect.fromRectAndRadius(g.$3, const Radius.circular(11)), Paint()..color = const Color.fromARGB(130, 0, 0, 0));
      final tp = _text(g.$4, 12, Colors.white);
      tp.paint(canvas, g.$3.center - Offset(tp.width / 2, tp.height / 2));
    }
    canvas.restore();
    // the shape's points (Make Editable)
    final sel = selectedPoint;
    final stroke = Paint()
      ..style = PaintingStyle.stroke
      ..strokeWidth = 1
      ..color = accent;
    for (final (s, i, kind, pt0, inc0, out0) in points()) {
      var (pt, inc, out) = (pt0, inc0, out0);
      if (point != null && point.s == s && point.i == i) {
        final d = point.delta;
        if (point.part == 0) {
          (pt, inc, out) = (pt + d, inc + d, out + d);
        } else {
          final moved = (point.part == 1 ? inc : out) + d;
          final other = kind == 1 ? pt * 2 - moved : (point.part == 1 ? out : inc);
          (inc, out) = point.part == 1 ? (moved, other) : (other, moved);
        }
      }
      final isSel = sel == (s, i);
      if (isSel && kind != 0) {
        for (final h in [inc, out]) {
          canvas.drawLine(pt, h, stroke);
          canvas.drawCircle(h, 3.5, Paint()..color = Colors.white);
          canvas.drawCircle(h, 3.5, stroke);
        }
      }
      final fill = Paint()..color = isSel ? accent : Colors.white;
      if (kind == 0) {
        final r = Rect.fromCenter(center: pt, width: 7, height: 7);
        canvas.drawRect(r, fill);
        canvas.drawRect(r, stroke);
      } else {
        canvas.drawCircle(pt, 4, fill);
        canvas.drawCircle(pt, 4, stroke);
      }
      if (point != null && point.s == s && point.i == i) {
        canvas.drawCircle(pt, 4.5, Paint()..color = accent);
        canvas.drawCircle(pt, 4.5, Paint()
          ..style = PaintingStyle.stroke
          ..strokeWidth = 1
          ..color = Colors.white);
      }
    }
    // the capsules: Edit Chart Data, Insert Page Number
    final ch = chartRect();
    if (ch != null) _capsule(canvas, look, ch, 13, 'Edit Chart Data', 12.5, hover != null && ch.contains(hover), chevron: false);
    final pn = pageNumberRect();
    if (pn != null) _capsule(canvas, look, pn, 12, 'Insert Page Number', 12, pageNumberHot || (hover != null && pn.contains(hover)), chevron: true);
  }

  /// Pages' white capsule with accent text (accent with white text while hot).
  void _capsule(Canvas canvas, Look look, Rect r, double radius, String label, double size, bool hot, {required bool chevron}) {
    final accent = look.accent;
    final (fill, ink) = hot ? (accent, Colors.white) : (Colors.white, accent);
    canvas.drawRRect(RRect.fromRectAndRadius(r.shift(const Offset(0, 1.5)), Radius.circular(radius)), Paint()..color = const Color.fromARGB(30, 0, 0, 0));
    canvas.drawRRect(RRect.fromRectAndRadius(r, Radius.circular(radius)), Paint()..color = fill);
    canvas.drawRRect(RRect.fromRectAndRadius(r.deflate(0.5), Radius.circular(radius - 0.5)), Paint()
      ..style = PaintingStyle.stroke
      ..strokeWidth = 1
      ..color = accent.withValues(alpha: 0.6));
    final tp = _text(label, size, ink);
    final c = r.center - Offset(chevron ? 7 : 0, 0);
    tp.paint(canvas, c - Offset(tp.width / 2, tp.height / 2));
    if (chevron) drawIcon(canvas, Rect.fromCenter(center: Offset(r.right - 14, r.center.dy), width: 10, height: 10), DIcon.chevronDown, ink);
  }

  TextPainter _text(String s, double size, Color color) => TextPainter(text: TextSpan(text: s, style: Look.font(size, color)), textDirection: TextDirection.ltr)..layout();
}

/// ruler.rs fmt_len: a length in the ruler's units.
String fmtLen(double v, int units) => switch (units) {
      1 => '${(v / 72).toStringAsFixed(2)} in',
      2 => '${v.toStringAsFixed(0)} pt',
      3 => '${(v / 72 * 25.4).toStringAsFixed(0)} mm',
      _ => '${(v / 72 * 2.54).toStringAsFixed(2)} cm',
    };
