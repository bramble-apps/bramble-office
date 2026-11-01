// The desktop's toolbar / inspector / menu icons, drawn as it draws them: a Dart port of
// keynote-linux/crates/kn-ui/src/icons.rs (vector strokes on a 24-unit grid, after Keynote
// 15.4's symbols). Keep the two in step: same shapes, same numbers.
import 'dart:math' as math;

import 'package:flutter/widgets.dart';

enum DIcon {
  projector, menu, undo, redo, sidebar, play, addSlide, table, chart, text, shape, media, comment,
  share, format, animate, document, chevronDown, print, search, line, highlight, gallery, webVideo,
  liveVideo, audio, file, layerForward, layerFront, layerBackward, layerBack, alignObjects,
  distribute, flipH, flipV, list, palette, sliders, music, photo, mic, smiley, playWindow,
  rehearse, layouts, skip, record, group, ungroup, bigger, smaller, superscript, subscript,
  outdent, indent, connect, copyStyle, pasteStyle, guides, mask, removeBg, lock, unlock, fonts,
  link, overflow,
}

/// The icon in a square of `size`, in `color` (the desktop's icons::draw).
class DesktopIcon extends StatelessWidget {
  const DesktopIcon(this.icon, {super.key, this.size = 22, required this.color});
  final DIcon icon;
  final double size;
  final Color color;

  @override
  Widget build(BuildContext context) => CustomPaint(size: Size.square(size), painter: _IconPainter(icon, color));
}

class _IconPainter extends CustomPainter {
  _IconPainter(this.icon, this.color);
  final DIcon icon;
  final Color color;

  @override
  void paint(Canvas canvas, Size size) => drawIcon(canvas, Offset.zero & size, icon, color);

  @override
  bool shouldRepaint(_IconPainter o) => o.icon != icon || o.color != color;
}

const _pi = math.pi;

void drawIcon(Canvas c, Rect r, DIcon icon, Color color) {
  final k = r.width / 24;
  final sw = math.max(1.6 * k, 1.2);
  Offset p(double x, double y) => Offset(r.left + x / 24 * r.width, r.top + y / 24 * r.height);
  Paint stroke(double w) => Paint()
    ..style = PaintingStyle.stroke
    ..strokeWidth = w
    ..color = color
    ..strokeJoin = StrokeJoin.miter
    ..strokeCap = StrokeCap.butt
    ..isAntiAlias = true;
  final s = stroke(sw);
  final fill = Paint()
    ..style = PaintingStyle.fill
    ..color = color
    ..isAntiAlias = true;
  void rrect(double x0, double y0, double x1, double y1, double radius, [Paint? st]) =>
      c.drawRRect(RRect.fromRectAndRadius(Rect.fromPoints(p(x0, y0), p(x1, y1)), Radius.circular(radius * k)), st ?? s);
  void polyline(List<Offset> pts, Paint st) {
    if (pts.length < 2) return;
    final path = Path()..moveTo(pts.first.dx, pts.first.dy);
    for (final q in pts.skip(1)) {
      path.lineTo(q.dx, q.dy);
    }
    c.drawPath(path, st);
  }

  void line(List<(double, double)> pts, [Paint? st]) => polyline([for (final (x, y) in pts) p(x, y)], st ?? s);
  List<Offset> arc(double cx, double cy, double rad, double a0, double a1) {
    const n = 24;
    return [
      for (var i = 0; i <= n; i++) p(cx + rad * math.cos(a0 + (a1 - a0) * i / n), cy + rad * math.sin(a0 + (a1 - a0) * i / n)),
    ];
  }

  void polygon(List<Offset> pts, {bool filled = true, Paint? st}) {
    final path = Path()..addPolygon(pts, true);
    c.drawPath(path, filled ? fill : (st ?? s));
  }

  void circleStroke(double x, double y, double rad) => c.drawCircle(p(x, y), rad * k, s);
  void circleFilled(double x, double y, double rad) => c.drawCircle(p(x, y), rad * k, fill);
  void text(String t, Offset at, double px) {
    final tp = TextPainter(text: TextSpan(text: t, style: TextStyle(color: color, fontSize: px)), textDirection: TextDirection.ltr)..layout();
    tp.paint(c, at - Offset(tp.width / 2, tp.height / 2));
  }

  switch (icon) {
    case DIcon.projector:
      line([(2, 4), (22, 4)], stroke(sw * 1.5));
      rrect(4, 4, 20, 16.5, 1);
      polyline(arc(8.8, 10.8, 2.6, -_pi / 2, 1.5 * _pi), stroke(sw * 0.9));
      line([(8.8, 8.2), (8.8, 10.8), (11.4, 10.8)], stroke(sw * 0.9));
      for (final (x, top) in [(14.0, 11.0), (16.2, 8.5)]) {
        line([(x, 13.6), (x, top)], stroke(sw * 1.1));
      }
      line([(12, 16.5), (12, 19.5)], stroke(sw * 0.8));
      circleFilled(12, 20.3, 1.1);
    case DIcon.sidebar:
      rrect(3, 5, 21, 19, 3);
      line([(9, 5), (9, 19)]);
      for (final y in [8.5, 11.0, 13.5]) {
        line([(5, y), (7, y)], stroke(sw * 0.9));
      }
    case DIcon.play:
      polygon([p(7.5, 5), p(19, 12), p(7.5, 19)]);
      polygon([p(7.5, 5), p(19, 12), p(7.5, 19)], filled: false, st: stroke(1 * k));
    case DIcon.addSlide:
      rrect(3.5, 5, 20.5, 19, 2.5);
      line([(12, 8.5), (12, 15.5)]);
      line([(8.5, 12), (15.5, 12)]);
    case DIcon.table:
      rrect(3, 5, 21, 19, 2);
      line([(3, 9.5), (21, 9.5)]);
      line([(3, 14.25), (21, 14.25)]);
      line([(9, 9.5), (9, 19)]);
      line([(15, 9.5), (15, 19)]);
    case DIcon.chart:
      polyline(arc(12, 12, 8, -_pi / 2, 1.5 * _pi), s);
      line([(12, 4), (12, 12), (19, 15.5)]);
    case DIcon.text:
      for (final (x, y, dx, dy) in [(3.5, 5.0, 1.0, 1.0), (20.5, 5.0, -1.0, 1.0), (3.5, 19.0, 1.0, -1.0), (20.5, 19.0, -1.0, -1.0)]) {
        line([(x + 2.5 * dx, y), (x, y), (x, y + 2.5 * dy)]);
      }
      line([(8.5, 16), (12, 7.5), (15.5, 16)]);
      line([(9.8, 13), (14.2, 13)]);
    case DIcon.shape:
      rrect(3.5, 3.5, 15, 15, 2.5);
      circleStroke(14.5, 14.5, 6);
    case DIcon.media:
      final pts = <Offset>[p(9, 17), ...arc(12, 6.5, 3, _pi, 2 * _pi), p(15, 16.5), ...arc(12, 16.5, 3, 0, _pi), p(9, 8), ...arc(10.5, 8, 1.5, _pi, 2 * _pi), p(12, 16)];
      polyline(pts, s);
    case DIcon.comment:
      rrect(3.5, 4.5, 20.5, 16.5, 3);
      line([(8, 16.5), (7.5, 20), (11.5, 16.5)]);
      line([(7.5, 8.5), (16.5, 8.5)]);
      line([(7.5, 12), (14, 12)]);
    case DIcon.share:
      line([(9, 9.5), (6, 9.5), (6, 20.5), (18, 20.5), (18, 9.5), (15, 9.5)]);
      line([(12, 3.5), (12, 14.5)]);
      line([(8.5, 7), (12, 3.5), (15.5, 7)]);
    case DIcon.format:
      line([(17.5, 3.5), (20.5, 6.5), (12.5, 14.5), (9.5, 11.5), (17.5, 3.5)]);
      line([(9.5, 11.5), (6, 13), (4, 20), (11, 18), (12.5, 14.5)]);
    case DIcon.animate:
      line([(12, 3.5), (20.5, 12), (12, 20.5), (3.5, 12), (12, 3.5)]);
      line([(15, 3.5), (23, 11.5)], stroke(sw * 0.8));
    case DIcon.document:
      rrect(3, 5, 21, 19, 2.5);
      c.drawRRect(RRect.fromRectAndRadius(Rect.fromPoints(p(6.5, 8.5), p(17.5, 15.5)), Radius.circular(1 * k)), fill);
    case DIcon.chevronDown:
      line([(7, 9.5), (12, 14.5), (17, 9.5)]);
    case DIcon.print:
      rrect(3.5, 9, 20.5, 17, 2);
      rrect(7, 4, 17, 9, 0.5);
      rrect(7, 14, 17, 20.5, 0.5);
    case DIcon.search:
      circleStroke(10.5, 10.5, 6);
      line([(15, 15), (20.5, 20.5)]);
    case DIcon.line:
      line([(5, 19), (19, 5)]);
    case DIcon.highlight:
      line([(6, 18), (16, 8), (18.5, 10.5), (8.5, 20.5), (6, 18)]);
      line([(4, 21), (20, 21)]);
    case DIcon.gallery:
      rrect(4, 5, 20, 15, 1.5);
      for (final x in [8.0, 12.0, 16.0]) {
        circleFilled(x, 19, 0.9);
      }
    case DIcon.webVideo:
      polyline(arc(12, 12, 8, 0, 2 * _pi), s);
      polygon([p(10, 8.5), p(15.5, 12), p(10, 15.5)]);
    case DIcon.liveVideo:
      rrect(3, 8, 15, 17, 2);
      line([(15, 11), (20.5, 8), (20.5, 17), (15, 14)]);
      circleStroke(6.5, 5.5, 1.8);
      circleStroke(11.5, 5.5, 1.8);
    case DIcon.audio:
      final hs = [3.0, 7.0, 11.0, 7.0, 4.0, 9.0, 5.0];
      for (var i = 0; i < hs.length; i++) {
        final x = 5.0 + i * 2.3;
        line([(x, 12 - hs[i] / 2), (x, 12 + hs[i] / 2)]);
      }
    case DIcon.file:
      line([(6, 3.5), (14, 3.5), (18.5, 8), (18.5, 20.5), (6, 20.5), (6, 3.5)]);
      line([(14, 3.5), (14, 8), (18.5, 8)]);
    case DIcon.layerForward || DIcon.layerFront || DIcon.layerBackward || DIcon.layerBack:
      final solidTop = icon == DIcon.layerForward || icon == DIcon.layerFront;
      final n = icon == DIcon.layerFront || icon == DIcon.layerBack ? 3 : 2;
      for (var i = 0; i < n; i++) {
        final y = 8.0 + i * 4;
        final pts = [p(4, y), p(12, y - 4), p(20, y), p(12, y + 4)];
        polygon(pts, filled: (solidTop && i == 0) || (!solidTop && i == n - 1));
      }
    case DIcon.alignObjects:
      line([(12, 3.5), (12, 20.5)]);
      rrect(6, 6, 18, 10, 1);
      rrect(8, 14, 16, 18, 1);
    case DIcon.distribute:
      line([(4, 4), (4, 20)]);
      line([(20, 4), (20, 20)]);
      rrect(9, 7, 15, 17, 1);
    case DIcon.flipH:
      line([(12, 3.5), (12, 20.5)], stroke(sw * 0.7));
      polygon([p(10, 6), p(10, 18), p(4, 18)]);
      polygon([p(14, 6), p(14, 18), p(20, 18)], filled: false);
    case DIcon.flipV:
      line([(3.5, 12), (20.5, 12)], stroke(sw * 0.7));
      polygon([p(6, 10), p(18, 10), p(18, 4)]);
      polygon([p(6, 14), p(18, 14), p(18, 20)], filled: false);
    case DIcon.list:
      for (final y in [7.0, 12.0, 17.0]) {
        circleFilled(5, y, 1);
        line([(8.5, y), (20, y)]);
      }
    case DIcon.palette:
      polyline(arc(12, 12, 8.5, 0.3, 2 * _pi - 0.6), s);
      for (final (x, y) in [(8.0, 9.0), (12.0, 7.0), (16.0, 9.0), (8.5, 14.0)]) {
        circleFilled(x, y, 1.2);
      }
    case DIcon.sliders:
      for (final (y, x) in [(7.0, 15.0), (12.0, 9.0), (17.0, 13.0)]) {
        line([(4, y), (20, y)]);
        circleFilled(x, y, 1.8);
      }
    case DIcon.music:
      line([(10, 18), (10, 5), (17, 7)]);
      circleFilled(8, 18, 2.5);
    case DIcon.photo:
      rrect(3.5, 6, 17.5, 18, 1.5);
      rrect(6.5, 3.5, 20.5, 15.5, 1.5);
    case DIcon.mic:
      rrect(9, 3.5, 15, 14, 3);
      polyline(arc(12, 11, 6, 0, _pi), s);
      line([(12, 17), (12, 20.5)]);
    case DIcon.playWindow:
      rrect(3, 5, 21, 19, 2.5);
      line([(3, 8.5), (21, 8.5)]);
      polygon([p(10, 11), p(15.5, 14), p(10, 17)]);
    case DIcon.rehearse:
      rrect(3, 5, 21, 19, 3);
      polygon([p(9.5, 8.5), p(16, 12), p(9.5, 15.5)]);
    case DIcon.layouts:
      rrect(3, 5, 21, 19, 2);
      rrect(6, 8, 18, 11, 1);
      line([(6, 14), (18, 14)]);
      line([(6, 16.5), (14, 16.5)]);
    case DIcon.skip:
      polyline(arc(12, 14, 7, _pi, 2 * _pi), s);
      line([(16, 14), (19, 14), (19, 11)]);
      circleFilled(12, 17, 1.5);
    case DIcon.record:
      circleStroke(12, 12, 8);
      circleFilled(12, 12, 4);
    case DIcon.group:
      rrect(4, 4, 20, 20, 3);
      rrect(8, 8, 16, 16, 1.5);
    case DIcon.ungroup:
      for (final (x, y) in [(4.0, 4.0), (16.0, 4.0), (4.0, 16.0), (16.0, 16.0)]) {
        rrect(x, y, x + 4, y + 4, 1);
      }
      rrect(9, 9, 15, 15, 1.5);
    case DIcon.bigger || DIcon.smaller || DIcon.superscript || DIcon.subscript || DIcon.fonts:
      final (big, small, dy) = switch (icon) {
        DIcon.bigger => ('A', '+', 0.0),
        DIcon.smaller => ('A', '−', 0.0),
        DIcon.superscript => ('A', '1', -5.0),
        DIcon.subscript => ('A', '1', 5.0),
        _ => ('Aa', '', 0.0),
      };
      text(big, p(small.isEmpty ? 12 : 10, 12.5), 13 * k);
      if (small.isNotEmpty) text(small, p(18, 12 + dy), 8.5 * k);
    case DIcon.outdent || DIcon.indent:
      for (final y in [5.0, 19.0]) {
        line([(4, y), (20, y)]);
      }
      for (final y in [10.0, 14.0]) {
        line([(11, y), (20, y)]);
      }
      polygon(icon == DIcon.indent ? [p(4, 9), p(8, 12), p(4, 15)] : [p(8, 9), p(4, 12), p(8, 15)]);
    case DIcon.connect:
      circleStroke(6.5, 17.5, 2.5);
      circleStroke(17.5, 6.5, 2.5);
      line([(8.5, 15.5), (15.5, 8.5)]);
    case DIcon.copyStyle || DIcon.pasteStyle:
      line([(5, 19), (14, 10)]);
      rrect(13, 5, 19, 11, 2.5);
      if (icon == DIcon.pasteStyle) circleFilled(5, 19, 2);
    case DIcon.guides:
      line([(12, 3), (12, 21)]);
      line([(3, 12), (21, 12)]);
      rrect(7, 7, 17, 17, 1, stroke(sw * 0.7));
    case DIcon.mask:
      line([(7, 3), (7, 17), (21, 17)]);
      line([(3, 7), (17, 7), (17, 21)]);
    case DIcon.removeBg:
      rrect(3, 5, 21, 19, 2);
      for (final (x, y) in [(5.0, 7.0), (9.0, 11.0), (5.0, 15.0), (13.0, 7.0), (17.0, 11.0), (13.0, 15.0)]) {
        c.drawRect(Rect.fromPoints(p(x, y), p(x + 2.5, y + 2.5)), fill);
      }
    case DIcon.lock || DIcon.unlock:
      rrect(6, 11, 18, 20, 2);
      final a1 = icon == DIcon.lock ? 2 * _pi : 1.75 * _pi;
      polyline([p(8.5, 11), ...arc(12, 7.5, 3.5, _pi, a1), if (icon == DIcon.lock) p(15.5, 11)], s);
    case DIcon.link:
      circleStroke(12, 12, 8);
      line([(8.5, 15.5), (15.5, 8.5)]);
      line([(11, 8.5), (15.5, 8.5), (15.5, 13)]);
    case DIcon.menu:
      for (final y in [7.0, 12.0, 17.0]) {
        line([(4.5, y), (19.5, y)]);
      }
    case DIcon.undo || DIcon.redo:
      double flip(double x) => icon == DIcon.redo ? 24 - x : x;
      final pts = <(double, double)>[
        for (var i = 0; i <= 12; i++) (flip(13 + 6 * math.cos(-_pi / 2 + _pi * i / 12)), 13 + 5 * math.sin(-_pi / 2 + _pi * i / 12)),
      ];
      line([(flip(6), 8), (flip(13), 8)]);
      line(pts);
      line([(flip(13), 18), (flip(8), 18)]);
      line([(flip(9.5), 4.5), (flip(6), 8), (flip(9.5), 11.5)]);
    case DIcon.overflow:
      line([(6, 7), (11, 12), (6, 17)]);
      line([(13, 7), (18, 12), (13, 17)]);
    case DIcon.smiley:
      circleStroke(12, 12, 8);
      circleFilled(9.5, 10, 1);
      circleFilled(14.5, 10, 1);
      polyline(arc(12, 12.5, 3.5, 0.3, _pi - 0.3), s);
  }
}

/// Letters' own toolbar glyphs (pg-app/src/tbicons.rs): Insert and Shape; false for others.
bool drawToolbarGlyph(Canvas c, Rect r, String id, Color col) {
  final s = r.width;
  final w = math.max(s * 0.075, 1.3);
  final st = Paint()
    ..style = PaintingStyle.stroke
    ..strokeWidth = w
    ..color = col
    ..isAntiAlias = true;
  final fill = Paint()..color = col;
  Offset at(double x, double y) => Offset(r.left + x * s, r.top + y * s);
  switch (id) {
    case 'Insert':
      const x0 = 0.04, y0 = 0.18, x1 = 0.96, y1 = 0.84, gap = 0.11;
      c.drawLine(at(x0 + gap, y0), at(x1, y0), st);
      c.drawLine(at(x1, y0), at(x1, y1 - gap), st);
      c.drawLine(at(x1 - gap, y1), at(x0, y1), st);
      c.drawLine(at(x0, y1), at(x0, y0 + gap), st);
      c.drawCircle(at(x0, y0), w * 0.95, fill);
      c.drawCircle(at(x1, y1), w * 0.95, fill);
      final m = at(0.5, 0.51);
      final a = s * 0.17;
      c.drawLine(m - Offset(a, 0), m + Offset(a, 0), st);
      c.drawLine(m - Offset(0, a), m + Offset(0, a), st);
      return true;
    case 'Shape':
      // a circle behind a rounded square: only the circle's part outside the square shows
      final q = Rect.fromPoints(at(0.36, 0.36), at(0.94, 0.94));
      final ctr = at(0.36, 0.36);
      final rad = s * 0.3;
      const n = 72;
      Offset pt(int k) => ctr + Offset(math.cos(k / n * 2 * math.pi), math.sin(k / n * 2 * math.pi)) * rad;
      bool outside(Offset p) => !q.inflate(w * 0.6).contains(p);
      var run = <Offset>[];
      void flush() {
        if (run.length > 1) c.drawPath(Path()..addPolygon(run, false), st);
        run = [];
      }

      for (var k = 0; k <= n; k++) {
        final p0 = pt(k);
        if (outside(p0)) {
          run.add(p0);
        } else {
          flush();
        }
      }
      flush();
      c.drawRRect(RRect.fromRectAndRadius(q, Radius.circular(s * 0.1)), st);
      return true;
  }
  return false;
}

/// The desktop's icon names (kn_ui::icons::Icon's Debug names, as the engine reports them).
DIcon? iconNamed(String name) {
  if (name.isEmpty) return null;
  final n = name[0].toLowerCase() + name.substring(1);
  for (final i in DIcon.values) {
    if (i.name == n) return i;
  }
  return null;
}
