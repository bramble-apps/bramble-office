// Liquid glass (kn_ui glass.rs shapes, Settings > General): a faint contact shadow, the theme's
// surface tint (40% over the toolbar's backdrop, 72% where there's none to see through), a
// fine rim lit at 45° and a hairline edge.
import 'dart:math' as math;
import 'dart:ui';

import 'look.dart';

/// Points round a rounded rectangle with their outward normals (glass.rs outline).
List<(Offset, Offset)> _outline(Rect r, double radius) {
  final rad = math.max(0.5, math.min(radius, math.min(r.width / 2, r.height / 2)));
  final corners = [
    (Offset(r.left + rad, r.top + rad), math.pi),
    (Offset(r.right - rad, r.top + rad), 1.5 * math.pi),
    (Offset(r.right - rad, r.bottom - rad), 0.0),
    (Offset(r.left + rad, r.bottom - rad), 0.5 * math.pi),
  ];
  const steps = 16;
  final out = <(Offset, Offset)>[];
  for (var i = 0; i < 4; i++) {
    final (c, a0) = corners[i];
    for (var k = 0; k <= steps; k++) {
      final a = a0 + k / steps * math.pi / 2;
      final n = Offset(math.cos(a), math.sin(a));
      out.add((c + n * rad, n));
    }
    // along the straight side to the next corner
    final (next, na) = corners[(i + 1) % 4];
    final end = next + Offset(math.cos(na), math.sin(na)) * rad;
    final (start, n) = out.last;
    final parts = ((end - start).distance / 5).floor();
    for (var j = 1; j < parts; j++) {
      out.add((Offset.lerp(start, end, j / parts)!, n));
    }
  }
  return out;
}

/// A glass capsule or rounded rectangle under its contents; `seeThrough`: the toolbar's
/// backdrop shows through it (the tint at 40%).
void paintGlass(Canvas c, Rect r, double radius, Look look, {bool seeThrough = false}) {
  final rr = RRect.fromRectAndRadius(r, Radius.circular(radius));
  c.drawRRect(rr.shift(const Offset(0, 1)), Paint()
    ..color = Color.fromARGB(look.light ? 14 : 40, 0, 0, 0)
    ..maskFilter = const MaskFilter.blur(BlurStyle.normal, 4));
  c.drawRRect(rr, Paint()..color = look.glassTint.withValues(alpha: seeThrough ? 0.4 : 0.72));
  // the rim light at 45° (both lit corners), white at up to 42%, just inside the edge
  final pts = _outline(r, radius);
  const dir = Offset(math.sqrt1_2, math.sqrt1_2);
  for (var i = 0; i < pts.length; i++) {
    final (q, n) = pts[i];
    final (q2, n2) = pts[(i + 1) % pts.length];
    final k = ((n.dx * dir.dx + n.dy * dir.dy).abs() + (n2.dx * dir.dx + n2.dy * dir.dy).abs()) / 2;
    c.drawLine(q - n * 0.3, q2 - n2 * 0.3, Paint()
      ..strokeWidth = 1
      ..color = Color.fromARGB((k * 0.42 * 255 * 0.6).round(), 255, 255, 255));
  }
  c.drawRRect(rr.deflate(0.25), Paint()
    ..style = PaintingStyle.stroke
    ..strokeWidth = 0.5
    ..color = Color.fromARGB(look.light ? 28 : 14, 255, 255, 255));
}
