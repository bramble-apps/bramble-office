// The suite's Colors panel (kn_ui colors.rs): a mode strip (Wheel, Sliders, Palette), the round
// colour wheel with a brightness track, RGB / HSB sliders with a hex field, Apple's crayons, an
// opacity track, the eyedropper and current colour, and the saved colours. The Colors window
// holds it: View > Show Colors (with Apply to Selection), or a colour well's wheel button
// editing that well.
import 'dart:math' as math;
import 'dart:ui' as ui;

import 'package:flutter/material.dart';
import 'package:flutter/rendering.dart';

import 'glide.dart';
import 'look.dart';
import 'widgets.dart';

/// The whole window as a picture (the eyedropper samples it); main.dart puts it round the app.
final appShot = GlobalKey();

/// HSV (each 0..1) to RGB (each 0..1).
List<double> hsvToRgb(double h, double s, double v) {
  final hh = (h % 1.0) * 6;
  final i = hh.floor();
  final f = hh - i;
  final p = v * (1 - s), q = v * (1 - s * f), t = v * (1 - s * (1 - f));
  return switch (i % 6) { 0 => [v, t, p], 1 => [q, v, p], 2 => [p, v, t], 3 => [p, q, v], 4 => [t, p, v], _ => [v, p, q] };
}

List<double> rgbToHsv(double r, double g, double b) {
  final mx = math.max(r, math.max(g, b)), mn = math.min(r, math.min(g, b)), d = mx - mn;
  double h;
  if (d <= 1e-6) {
    h = 0;
  } else if (mx == r) {
    h = (((g - b) / d) % 6) / 6;
  } else if (mx == g) {
    h = ((b - r) / d + 2) / 6;
  } else {
    h = ((r - g) / d + 4) / 6;
  }
  return [h, mx <= 1e-6 ? 0 : d / mx, mx];
}

Color _to(List<double> rgb, [int a = 255]) {
  int k(double v) => (v.clamp(0.0, 1.0) * 255).round();
  return Color.fromARGB(a, k(rgb[0]), k(rgb[1]), k(rgb[2]));
}

int _a8(Color c) => (c.a * 255).round();

/// "#RRGGBB", "#RGB" or "#RRGGBBAA" (the # optional).
Color? parseHex(String s) {
  var t = s.trim().replaceFirst('#', '');
  if (t.length == 3) t = '${t.split('').map((c) => '$c$c').join()}ff';
  if (t.length == 6) t = '${t}ff';
  if (t.length != 8) return null;
  final v = int.tryParse(t, radix: 16);
  if (v == null) return null;
  return Color.fromARGB(v & 0xff, (v >> 24) & 0xff, (v >> 16) & 0xff, (v >> 8) & 0xff);
}

String hexOf(Color c) {
  String h(double v) => (v * 255).round().toRadixString(16).padLeft(2, '0').toUpperCase();
  return '#${h(c.r)}${h(c.g)}${h(c.b)}${_a8(c) == 255 ? '' : h(c.a)}';
}

/// Red to the right, hues anticlockwise (the Mac's), saturation outwards.
Offset wheelPoint(Offset c, double r, double h, double s) {
  final a = h * 2 * math.pi;
  return c + Offset(math.cos(a), -math.sin(a)) * (s * r);
}

(double, double) wheelHs(Offset c, double r, Offset p) {
  final d = p - c;
  final h = math.atan2(-d.dy, d.dx) / (2 * math.pi);
  return (h % 1.0, (d.distance / r).clamp(0.0, 1.0));
}

const crayons = [
  0x000000, 0x191919, 0x333333, 0x4C4C4C, 0x666666, 0x7F7F7F, 0x999999, 0xB2B2B2, //
  0xCCCCCC, 0xE5E5E5, 0xFFFFFF, 0x941100, 0x945200, 0x929000, 0x4F8F00, 0x008F00,
  0x009051, 0x009193, 0x005493, 0x011993, 0x531B93, 0x942193, 0x941751, 0xFF2600,
  0xFF9300, 0xFFFB00, 0x8EFA00, 0x00F900, 0x00FA92, 0x00FDFF, 0x0096FF, 0x0433FF,
  0x9437FF, 0xFF40FF, 0xFF2F92, 0xFF7E79, 0xFFD479, 0xFFFC79, 0xD4FB79, 0x73FA79,
  0x73FCD6, 0x73FDFF, 0x76D6FF, 0x7A81FF, 0xD783FF, 0xFF85FF, 0xFF8AD8, 0xFFFFFF,
];

/// What the panel remembers (one for the whole app, as the desktop's app.color_state).
class ColorPanelState {
  int mode = 0;
  bool hsb = false;
  List<double> hsv = [0, 0, 1];
  Color last = Colors.white;
  List<Color> saved = [];
  String hex = '';
  bool picking = false;

  static final shared = ColorPanelState();
  String savedToString() => saved.map(hexOf).join(',');
  void savedFrom(String s) => saved = [for (final x in s.split(',')) ?parseHex(x)];
}

/// A colour well's wheel button opened the Colors window on its colour: the window edits it.
class WellTarget {
  WellTarget(this.owner, this.color, this.onChanged);
  final Object owner;
  Color color;
  final void Function(Color) onChanged;
  static final current = WellNotifier();
}

class WellNotifier extends ValueNotifier<WellTarget?> {
  WellNotifier() : super(null);

  /// The target's colour changed in place.
  void poke() => notifyListeners();
}

/// The see-through checkerboard (5-point squares) inside a rounded rectangle.
void _checker(Canvas c, Rect r, double radius) {
  c.save();
  c.clipRRect(RRect.fromRectAndRadius(r, Radius.circular(radius)));
  c.drawRect(r, Paint()..color = Colors.white);
  const k = 5.0;
  var row = 0;
  for (var y = r.top; y < r.bottom; y += k, row++) {
    for (var x = r.left + (row.isEven ? 0 : k); x < r.right; x += 2 * k) {
      c.drawRect(Rect.fromLTRB(x, y, math.min(x + k, r.right), math.min(y + k, r.bottom)), Paint()..color = const Color(0xFFC8C8C8));
    }
  }
  c.restore();
}

/// A 14-tall capsule track filled with `stops`, a round white knob at `t`.
class _Track extends StatelessWidget {
  const _Track({required this.width, required this.stops, required this.t, required this.onT, this.checkered = false});
  final double width, t;
  final List<Color> stops;
  final bool checkered;
  final void Function(double) onT;
  @override
  Widget build(BuildContext context) {
    void at(Offset p) => onT(((p.dx - 7) / (width - 14)).clamp(0.0, 1.0));
    return GestureDetector(
      onTapDown: (d) => at(d.localPosition),
      onHorizontalDragUpdate: (d) => at(d.localPosition),
      child: CustomPaint(
        size: Size(width, 14),
        painter: _Fn((c, s) {
          final r = Offset.zero & s;
          if (checkered) _checker(c, r, 7);
          final rr = RRect.fromRectAndRadius(r, const Radius.circular(7));
          c.drawRRect(rr, Paint()..shader = ui.Gradient.linear(r.centerLeft, r.centerRight, stops, [for (var i = 0; i < stops.length; i++) i / math.max(1, stops.length - 1)]));
          c.drawRRect(rr.deflate(0.5), Paint()
            ..style = PaintingStyle.stroke
            ..color = const Color.fromARGB(60, 0, 0, 0));
          final k = Offset(7 + (s.width - 14) * t.clamp(0.0, 1.0), 7);
          c.drawCircle(k, 7, Paint()..color = Colors.white);
          c.drawCircle(k, 6.5, Paint()
            ..style = PaintingStyle.stroke
            ..color = const Color.fromARGB(90, 0, 0, 0));
        }),
      ),
    );
  }
}

class _Fn extends CustomPainter {
  _Fn(this.f);
  final void Function(Canvas, Size) f;
  @override
  void paint(Canvas c, Size s) => f(c, s);
  @override
  bool shouldRepaint(_Fn o) => true;
}

/// The panel, `width` wide (232 in the window).
class ColorPanel extends StatefulWidget {
  const ColorPanel({super.key, required this.color, required this.onChanged, this.width = 232, this.inWindow = true, this.onSavedChanged});
  final Color color;
  final double width;
  final bool inWindow;
  final void Function(Color) onChanged;
  final VoidCallback? onSavedChanged;
  @override
  State<ColorPanel> createState() => _ColorPanelState();
}

class _ColorPanelState extends State<ColorPanel> {
  ColorPanelState get st => ColorPanelState.shared;
  late final _hex = TextEditingController();

  @override
  void dispose() {
    _hex.dispose();
    super.dispose();
  }

  /// A colour changed from outside: edit that one (greys keep their hue).
  void _follow() {
    final c = widget.color;
    if (c.withValues(alpha: 1) != st.last.withValues(alpha: 1)) {
      final n = rgbToHsv(c.r, c.g, c.b);
      st.hsv = [n[1] > 0.001 ? n[0] : st.hsv[0], n[1], n[2]];
      st.last = c;
      st.hex = hexOf(c);
      _hex.text = st.hex;
    }
  }

  void _set(List<double> hsv, [int? alpha]) {
    final c = _to(hsvToRgb(hsv[0], hsv[1], hsv[2]), alpha ?? _a8(widget.color));
    setState(() {
      st.hsv = hsv;
      st.last = c;
      st.hex = hexOf(c);
      _hex.text = st.hex;
    });
    widget.onChanged(c);
  }

  void _use(Color c) {
    final n = rgbToHsv(c.r, c.g, c.b);
    _set([n[1] > 0.001 ? n[0] : st.hsv[0], n[1], n[2]], _a8(c));
  }

  /// The eyedropper: the next tap anywhere picks the colour under it.
  Future<void> _pick() async {
    setState(() => st.picking = true);
    final at = await Navigator.of(context).push<Offset>(PageRouteBuilder(
      opaque: false,
      barrierColor: Colors.transparent,
      pageBuilder: (ctx, _, _) => MouseRegion(
        cursor: SystemMouseCursors.precise,
        child: GestureDetector(behavior: HitTestBehavior.opaque, onTapUp: (d) => Navigator.of(ctx).pop(d.globalPosition)),
      ),
    ));
    setState(() => st.picking = false);
    if (at == null) return;
    final b = appShot.currentContext?.findRenderObject() as RenderRepaintBoundary?;
    if (b == null) return;
    final img = await b.toImage(pixelRatio: 1);
    final data = await img.toByteData();
    img.dispose();
    if (data == null) return;
    final x = at.dx.round(), y = at.dy.round();
    if (x < 0 || y < 0 || x >= img.width || y >= img.height) return;
    final o = (y * img.width + x) * 4;
    _use(Color.fromARGB(255, data.getUint8(o), data.getUint8(o + 1), data.getUint8(o + 2)));
  }

  @override
  Widget build(BuildContext context) {
    _follow();
    final look = LookScope.of(context);
    final w = widget.width;
    final hsv = st.hsv;
    final alpha = _a8(widget.color);
    Widget small(String t) => Padding(padding: const EdgeInsets.only(bottom: 6), child: Text(t, style: Look.font(9, look.text)));
    final kids = <Widget>[
      // the mode strip
      Row(mainAxisAlignment: MainAxisAlignment.center, children: [
        for (var m = 0; m < 3; m++) ...[
          if (m > 0) const SizedBox(width: 2),
          GestureDetector(
            onTap: () => setState(() => st.mode = m),
            child: Container(
              width: 32,
              height: 24,
              decoration: BoxDecoration(color: st.mode == m ? look.accent : null, borderRadius: BorderRadius.circular(6)),
              child: CustomPaint(painter: _Fn((c, s) => _modeIcon(c, s.center(Offset.zero), m, st.mode == m, look))),
            ),
          ),
        ],
      ]),
      const SizedBox(height: 14),
    ];
    switch (st.mode) {
      case 0:
        final d = math.min(w, 220.0);
        kids.addAll([
          Center(
            child: GestureDetector(
              onPanDown: (e) => _wheel(e.localPosition, d),
              onPanUpdate: (e) => _wheel(e.localPosition, d),
              child: CustomPaint(size: Size(d, d), painter: _Wheel(hsv)),
            ),
          ),
          const SizedBox(height: 14),
          _Track(width: w, stops: [_to(hsvToRgb(hsv[0], hsv[1], 0)), _to(hsvToRgb(hsv[0], hsv[1], 1))], t: hsv[2], onT: (t) => _set([hsv[0], hsv[1], t])),
        ]);
      case 1:
        kids.add(Row(children: [
          for (final (k, n) in ['RGB', 'HSB'].indexed) ...[
            if (k > 0) const SizedBox(width: 8),
            GestureDetector(
              onTap: () => setState(() => st.hsb = k == 1),
              child: Container(
                height: 24,
                padding: const EdgeInsets.symmetric(horizontal: 10),
                decoration: BoxDecoration(color: st.hsb == (k == 1) ? look.accent : null, borderRadius: BorderRadius.circular(6)),
                child: Center(widthFactor: 1, child: Text(n, style: Look.font(13, look.text))),
              ),
            ),
          ],
        ]));
        kids.add(const SizedBox(height: 10));
        final tw = w - 56;
        if (st.hsb) {
          for (final (k, label) in ['Hue', 'Saturation', 'Brightness'].indexed) {
            final stops = [for (var i = 0; i <= 12; i++) _to(hsvToRgb(k == 0 ? i / 12 : hsv[0], k == 1 ? i / 12 : hsv[1], k == 2 ? i / 12 : hsv[2]))];
            final max = k == 0 ? 360 : 100;
            kids.addAll([
              small(label),
              Row(children: [
                _Track(width: tw, stops: stops, t: hsv[k], onT: (t) => _set([for (var j = 0; j < 3; j++) j == k ? t : hsv[j]])),
                const SizedBox(width: 8),
                _Num(value: (hsv[k] * max).round(), max: max, onChanged: (v) => _set([for (var j = 0; j < 3; j++) j == k ? v / max : hsv[j]])),
              ]),
              const SizedBox(height: 6),
            ]);
          }
        } else {
          final rgb = hsvToRgb(hsv[0], hsv[1], hsv[2]);
          void setRgb(int k, double v) {
            final x = [for (var j = 0; j < 3; j++) j == k ? v : rgb[j]];
            final n = rgbToHsv(x[0], x[1], x[2]);
            _set([n[1] > 0.001 ? n[0] : hsv[0], n[1], n[2]]);
          }

          for (final (k, label) in ['Red', 'Green', 'Blue'].indexed) {
            final lo = [for (var j = 0; j < 3; j++) j == k ? 0.0 : rgb[j]], hi = [for (var j = 0; j < 3; j++) j == k ? 1.0 : rgb[j]];
            kids.addAll([
              small(label),
              Row(children: [
                _Track(width: tw, stops: [_to(lo), _to(hi)], t: rgb[k], onT: (t) => setRgb(k, t)),
                const SizedBox(width: 8),
                _Num(value: (rgb[k] * 255).round(), max: 255, onChanged: (v) => setRgb(k, v / 255)),
              ]),
              const SizedBox(height: 6),
            ]);
          }
        }
        if (_hex.text.isEmpty) _hex.text = st.hex;
        kids.addAll([
          const SizedBox(height: 10),
          Row(children: [
            Text('Hex', style: Look.font(9, look.text)),
            const SizedBox(width: 8),
            SizedBox(
              width: 80,
              height: 24,
              child: Container(
                padding: const EdgeInsets.symmetric(horizontal: 6),
                decoration: BoxDecoration(color: look.field, borderRadius: BorderRadius.circular(6), border: Border.all(color: look.fieldStroke)),
                child: TextField(
                  controller: _hex,
                  style: Look.font(13, look.text).copyWith(fontFamily: 'monospace'),
                  decoration: const InputDecoration(isCollapsed: true, border: InputBorder.none),
                  textAlignVertical: TextAlignVertical.center,
                  onSubmitted: (t) {
                    final p = parseHex(t);
                    if (p != null) _use(p.withValues(alpha: widget.color.a));
                  },
                ),
              ),
            ),
          ]),
        ]);
      default:
        final cell = ((w - 7 * 4) / 8).floorToDouble();
        kids.add(Wrap(spacing: 4, runSpacing: 4, children: [
          for (final v in crayons)
            GestureDetector(
              onTap: () => _use(Color(0xFF000000 | v).withValues(alpha: widget.color.a)),
              child: CustomPaint(
                size: Size(cell, cell * 0.7),
                painter: _Fn((c, s) {
                  final col = Color(0xFF000000 | v);
                  final r = RRect.fromRectAndRadius(Offset.zero & s, const Radius.circular(3));
                  c.drawRRect(r, Paint()..color = col);
                  c.drawRRect(r.deflate(0.5), Paint()
                    ..style = PaintingStyle.stroke
                    ..color = const Color.fromARGB(50, 0, 0, 0));
                  if (col.toARGB32() & 0xFFFFFF == widget.color.toARGB32() & 0xFFFFFF) {
                    c.drawRRect(r.inflate(2.5), Paint()
                      ..style = PaintingStyle.stroke
                      ..strokeWidth = 2
                      ..color = look.accent);
                  }
                }),
              ),
            ),
        ]));
    }
    // opacity
    final opaque = _to(hsvToRgb(hsv[0], hsv[1], hsv[2]));
    final now = _to(hsvToRgb(hsv[0], hsv[1], hsv[2]), alpha);
    kids.addAll([
      const SizedBox(height: 14),
      small('Opacity  ${(alpha / 2.55).round()}%'),
      _Track(width: w, stops: [opaque.withValues(alpha: 0), opaque], t: alpha / 255, checkered: true, onT: (t) => _set(hsv, (t * 255).round())),
      const SizedBox(height: 14),
      // the eyedropper (window only), then the colour
      Row(children: [
        if (widget.inWindow) ...[
          GestureDetector(
            onTap: _pick,
            child: Container(
              width: 30,
              height: 26,
              decoration: BoxDecoration(color: st.picking ? look.accent : look.gray(0x38), borderRadius: BorderRadius.circular(6)),
              child: CustomPaint(painter: _Fn((c, s) => _eyedropper(c, (Offset.zero & s).deflate(4), st.picking ? Colors.white : look.text))),
            ),
          ),
          const SizedBox(width: 6),
        ],
        Expanded(
          child: CustomPaint(
            size: const Size(0, 26),
            painter: _Fn((c, s) {
              final r = Offset.zero & s;
              if (alpha < 255) _checker(c, r, 4);
              final rr = RRect.fromRectAndRadius(r, const Radius.circular(4));
              c.drawRRect(rr, Paint()..color = now);
              c.drawRRect(rr.deflate(0.5), Paint()
                ..style = PaintingStyle.stroke
                ..color = const Color.fromARGB(70, 0, 0, 0));
            }),
          ),
        ),
      ]),
      const SizedBox(height: 12),
    ]);
    // + saves this colour; a saved one is used on a tap
    const slot = 18.0;
    final perRow = ((w + 4) / (slot + 4)).floor();
    kids.add(Wrap(spacing: 4, runSpacing: 4, children: [
      GestureDetector(
        onTap: () {
          if (st.saved.contains(now)) return;
          setState(() {
            st.saved.insert(0, now);
            if (st.saved.length > perRow * 2 - 1) st.saved = st.saved.sublist(0, perRow * 2 - 1);
          });
          widget.onSavedChanged?.call();
        },
        child: CustomPaint(
          size: const Size(slot, slot),
          painter: _Fn((c, s) {
            final p = Paint()
              ..style = PaintingStyle.stroke
              ..color = look.textDim;
            c.drawRRect(RRect.fromRectAndRadius((Offset.zero & s).deflate(0.5), const Radius.circular(3)), p);
            final m = s.center(Offset.zero);
            p.strokeWidth = 1.3;
            c.drawLine(m - const Offset(4.5, 0), m + const Offset(4.5, 0), p);
            c.drawLine(m - const Offset(0, 4.5), m + const Offset(0, 4.5), p);
          }),
        ),
      ),
      for (final sc in st.saved)
        GestureDetector(
          onTap: () => _use(sc),
          child: CustomPaint(
            size: const Size(slot, slot),
            painter: _Fn((c, s) {
              final r = Offset.zero & s;
              if (sc.a < 1) _checker(c, r, 3);
              final rr = RRect.fromRectAndRadius(r, const Radius.circular(3));
              c.drawRRect(rr, Paint()..color = sc);
              c.drawRRect(rr.deflate(0.5), Paint()
                ..style = PaintingStyle.stroke
                ..color = const Color.fromARGB(60, 0, 0, 0));
            }),
          ),
        ),
    ]));
    return SizedBox(width: w, child: Column(crossAxisAlignment: CrossAxisAlignment.start, mainAxisSize: MainAxisSize.min, children: kids));
  }

  void _wheel(Offset p, double d) {
    final (h, s) = wheelHs(Offset(d / 2, d / 2), d / 2 - 2, p);
    _set([h, s, st.hsv[2]]);
  }
}

/// A DragValue for the sliders' numbers.
class _Num extends StatelessWidget {
  const _Num({required this.value, required this.max, required this.onChanged});
  final int value, max;
  final void Function(int) onChanged;
  @override
  Widget build(BuildContext context) {
    final look = LookScope.of(context);
    var acc = value.toDouble();
    return GestureDetector(
      onHorizontalDragStart: (_) => acc = value.toDouble(),
      onHorizontalDragUpdate: (d) {
        acc = (acc + d.delta.dx).clamp(0, max.toDouble());
        if (acc.round() != value) onChanged(acc.round());
      },
      child: Container(
        width: 48,
        height: 24,
        decoration: BoxDecoration(color: look.control, borderRadius: BorderRadius.circular(6), border: Border.all(color: look.controlStroke)),
        alignment: Alignment.center,
        child: Text('$value', style: Look.font(13, look.text)),
      ),
    );
  }
}

/// The wheel: hue round, saturation out, at the brightness; the ring marks the colour.
class _Wheel extends CustomPainter {
  _Wheel(this.hsv);
  final List<double> hsv;
  @override
  void paint(Canvas c, Size s) {
    final ctr = s.center(Offset.zero), rad = s.width / 2 - 2;
    const seg = 96, rings = 12;
    final pos = <Offset>[ctr], cols = <Color>[_to(hsvToRgb(0, 0, hsv[2]))];
    for (var k = 1; k <= rings; k++) {
      for (var i = 0; i < seg; i++) {
        final h = i / seg, sat = k / rings;
        pos.add(wheelPoint(ctr, rad, h, sat));
        cols.add(_to(hsvToRgb(h, sat, hsv[2])));
      }
    }
    int idx(int k, int i) => 1 + (k - 1) * seg + i % seg;
    final tri = <int>[];
    for (var i = 0; i < seg; i++) {
      tri.addAll([0, idx(1, i), idx(1, i + 1)]);
    }
    for (var k = 2; k <= rings; k++) {
      for (var i = 0; i < seg; i++) {
        tri.addAll([idx(k - 1, i), idx(k, i), idx(k, i + 1), idx(k - 1, i), idx(k, i + 1), idx(k - 1, i + 1)]);
      }
    }
    c.drawVertices(ui.Vertices(ui.VertexMode.triangles, pos, colors: cols, indices: tri), BlendMode.dst, Paint());
    c.drawCircle(ctr, rad, Paint()
      ..style = PaintingStyle.stroke
      ..color = const Color.fromARGB(70, 0, 0, 0));
    final at = wheelPoint(ctr, rad, hsv[0], hsv[1]);
    c.drawCircle(at, 6, Paint()
      ..style = PaintingStyle.stroke
      ..strokeWidth = 2
      ..color = Colors.white);
    c.drawCircle(at, 7, Paint()
      ..style = PaintingStyle.stroke
      ..color = const Color.fromARGB(120, 0, 0, 0));
  }

  @override
  bool shouldRepaint(_Wheel o) => o.hsv != hsv;
}

void _modeIcon(Canvas c, Offset m, int mode, bool on, Look look) {
  final col = on ? Colors.white : look.textDim;
  switch (mode) {
    case 0:
      for (var k = 0; k < 12; k++) {
        final a0 = k / 12 * 2 * math.pi, a1 = (k + 1) / 12 * 2 * math.pi;
        c.drawPath(Path()..addPolygon([m, m + Offset(math.cos(a0), -math.sin(a0)) * 7, m + Offset(math.cos(a1), -math.sin(a1)) * 7], true), Paint()..color = _to(hsvToRgb(k / 12, 0.8, 1)));
      }
    case 1:
      for (final (i, x) in [0.3, 0.7, 0.45].indexed) {
        final y = m.dy - 4 + i * 4;
        c.drawLine(Offset(m.dx - 7, y), Offset(m.dx + 7, y), Paint()
          ..color = col
          ..strokeWidth = 1.2);
        c.drawCircle(Offset(m.dx - 7 + 14 * x, y), 1.8, Paint()..color = col);
      }
    default:
      for (var i = 0; i < 3; i++) {
        for (var j = 0; j < 3; j++) {
          c.drawRRect(RRect.fromRectAndRadius(Rect.fromLTWH(m.dx - 6 + j * 4.5, m.dy - 6 + i * 4.5, 3.4, 3.4), const Radius.circular(0.6)), Paint()..color = col);
        }
      }
  }
}

/// Apple's eyedropper (colors.rs draw_eyedropper).
void _eyedropper(Canvas c, Rect r, Color col) {
  final s = math.min(r.width, r.height);
  Offset at(double x, double y) => Offset(r.center.dx + (x - 0.5) * s, r.center.dy + (y - 0.5) * s);
  final thin = Paint()
    ..color = col
    ..style = PaintingStyle.stroke
    ..strokeWidth = math.max(s * 0.08, 1.2);
  c.drawLine(at(0.08, 0.92), at(0.2, 0.8), thin);
  final a = at(0.2, 0.8), b = at(0.55, 0.45);
  final n = const Offset(1, 1) / math.sqrt2 * (s * 0.07);
  c.drawPath(Path()..addPolygon([a - n, b - n, b + n, a + n], true), thin);
  final mm = const Offset(1, 1) / math.sqrt2 * (s * 0.16);
  c.drawLine(at(0.57, 0.43) - mm, at(0.57, 0.43) + mm, Paint()
    ..color = col
    ..strokeWidth = math.max(s * 0.12, 1.8));
  final w = s * 0.12;
  c.drawLine(at(0.66, 0.34), at(0.8, 0.2), Paint()
    ..color = col
    ..strokeWidth = w * 2);
  c.drawCircle(at(0.8, 0.2), w, Paint()..color = col);
}

/// The Colors window (no egui title bar: its own 30-point header, "Colors" and ×, a line the
/// window's width, then the panel 12 in from each side); `extra` below the panel.
class ColorsWindow extends StatelessWidget {
  const ColorsWindow({super.key, required this.color, required this.onChanged, required this.onClose, this.onDrag, this.extra, this.onSavedChanged});
  final Color color;
  final void Function(Color) onChanged;
  final VoidCallback onClose;
  final void Function(Offset)? onDrag;
  final Widget? extra;
  final VoidCallback? onSavedChanged;
  @override
  Widget build(BuildContext context) {
    final look = LookScope.of(context);
    return Container(
      width: 256,
      padding: const EdgeInsets.symmetric(vertical: 6),
      decoration: BoxDecoration(
        color: look.panel,
        borderRadius: BorderRadius.circular(10),
        border: Border.all(color: look.windowStroke, width: 1 / MediaQuery.of(context).devicePixelRatio),
        boxShadow: [BoxShadow(color: Color(look.light ? 0x19000000 : 0x60000000), offset: const Offset(10, 20), blurRadius: 15)],
      ),
      child: Column(mainAxisSize: MainAxisSize.min, crossAxisAlignment: CrossAxisAlignment.stretch, children: [
        GestureDetector(
          onPanUpdate: (d) => (GlideHandle.of(context)?.drag ?? onDrag)?.call(d.delta),
          onPanEnd: (d) => GlideHandle.of(context)?.release(d.velocity),
          child: SizedBox(
            height: 30,
            child: Stack(children: [
              Center(child: Text('Colors', style: Look.font(18, look.text))),
              Positioned(
                right: 8,
                top: 0,
                bottom: 0,
                width: 14,
                child: GestureDetector(
                  onTap: onClose,
                  child: CustomPaint(painter: _Fn((c, s) {
                    final g = Rect.fromCenter(center: s.center(Offset.zero), width: 10, height: 10);
                    final p = Paint()
                      ..color = look.text
                      ..strokeWidth = 1;
                    c.drawLine(g.topLeft, g.bottomRight, p);
                    c.drawLine(g.topRight, g.bottomLeft, p);
                  })),
                ),
              ),
            ]),
          ),
        ),
        const SizedBox(height: 2),
        Container(height: 1, color: look.separator),
        const SizedBox(height: 6),
        Padding(
          padding: const EdgeInsets.fromLTRB(12, 4, 12, 8),
          child: Column(crossAxisAlignment: CrossAxisAlignment.start, mainAxisSize: MainAxisSize.min, children: [
            ColorPanel(color: color, onChanged: onChanged, onSavedChanged: onSavedChanged),
            if (extra != null) ...[const SizedBox(height: 14), extra!],
          ]),
        ),
      ]),
    );
  }
}
