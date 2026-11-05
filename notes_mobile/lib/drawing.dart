// Drawings (com.apple.paper): their strokes painted as PencilKit inks (pen, monoline, marker,
// pencil, crayon, fountain pen, watercolour, reed), and the drawing screen: the canvas, the
// tool palette along the foot (iOS 26: the instruments in a row, the active one raised, its
// colour band showing; the colour circle at the right), undo and redo, and Done. Tapping the
// active tool again opens its widths and opacity; the eraser takes away whole strokes.
import 'dart:io';
import 'dart:math' as math;
import 'dart:ui' as ui;

import 'package:flutter/cupertino.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';

import 'doc.dart';
import 'theme.dart';
import 'ui/glass.dart';
import 'ui/sheet.dart';
import 'ui/touch_menu.dart';

/// Paints strokes (nt-ffi stroke JSON: ink, color, width, pts = x y t size force azimuth altitude opacity …, tf).
class InkPainter extends CustomPainter {
  InkPainter(this.strokes, this.scale, {this.dark = false, this.live});
  final List<Map<String, dynamic>> strokes;
  final double scale;
  final bool dark;

  /// The stroke being drawn.
  final Map<String, dynamic>? live;

  @override
  void paint(Canvas c, Size size) {
    c.save();
    c.scale(scale);
    for (final s in strokes) {
      paintStroke(c, s, dark: dark);
    }
    if (live != null) paintStroke(c, live!, dark: dark);
    c.restore();
  }

  @override
  bool shouldRepaint(InkPainter o) => true;
}

Color inkColor(Map<String, dynamic> s, {bool dark = false}) {
  final col = (s['color'] as List?)?.map((e) => (e as num).toDouble()).toList() ?? const [0.0, 0.0, 0.0, 1.0];
  var c = Color.from(alpha: col.length > 3 ? col[3] : 1, red: col[0], green: col[1], blue: col[2]);
  // black ink shows white on a dark page, as in Notes
  if (dark && col[0] < 0.15 && col[1] < 0.15 && col[2] < 0.15) c = Color.from(alpha: c.a, red: 1, green: 1, blue: 1);
  return c;
}

void paintStroke(Canvas c, Map<String, dynamic> s, {bool dark = false}) {
  final pts = (s['pts'] as List?)?.map((e) => (e as num).toDouble()).toList() ?? const [];
  if (pts.length < 8) return;
  final ink = s['ink'] as String? ?? 'pen';
  final tf = (s['tf'] as List?)?.map((e) => (e as num).toDouble()).toList() ?? const [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];
  final base = inkColor(s, dark: dark);
  final alpha = switch (ink) {
    'marker' => 0.45,
    'watercolor' => 0.35,
    'pencil' => 0.8,
    'crayon' => 0.85,
    _ => 1.0,
  };
  final width = (s['width'] as num?)?.toDouble() ?? 3;
  Offset at(int i) {
    final x = pts[i * 8], y = pts[i * 8 + 1];
    return Offset(tf[0] * x + tf[2] * y + tf[4], tf[1] * x + tf[3] * y + tf[5]);
  }

  final n = pts.length ~/ 8;
  final paint = Paint()
    ..style = PaintingStyle.stroke
    ..strokeCap = ink == 'marker' || ink == 'reed' ? StrokeCap.square : StrokeCap.round
    ..strokeJoin = StrokeJoin.round
    ..isAntiAlias = true;
  if (ink == 'marker' || ink == 'watercolor' || ink == 'monoline') {
    // one even path (a marker's overlaps don't darken)
    final path = Path()..moveTo(at(0).dx, at(0).dy);
    for (var i = 1; i < n; i++) {
      final a = at(i - 1), b = at(i);
      final m = (a + b) / 2;
      path.quadraticBezierTo(a.dx, a.dy, m.dx, m.dy);
    }
    final last = at(n - 1);
    path.lineTo(last.dx, last.dy);
    var w = 0.0;
    for (var i = 0; i < n; i++) {
      w += pts[i * 8 + 3];
    }
    w = n == 0 ? width : w / n;
    c.drawPath(
      path,
      paint
        ..strokeWidth = math.max(0.5, ink == 'monoline' ? width : w)
        ..color = base.withValues(alpha: base.a * alpha),
    );
    return;
  }
  // pens: each piece as wide as its points
  if (n == 1) {
    c.drawCircle(at(0), math.max(0.5, pts[3] / 2), Paint()..color = base.withValues(alpha: base.a * alpha));
    return;
  }
  // The pieces overlap at every point, so a see-through ink (pencil, crayon, a lowered opacity)
  // is drawn solid into a layer that is then laid down at its opacity: drawn piece by piece,
  // each overlap would darken into a bead along the line.
  var op = 0.0, maxW = 0.0;
  var lo = at(0), hi = at(0);
  for (var i = 0; i < n; i++) {
    final o = pts[i * 8 + 7];
    op += o <= 0 ? 1 : o;
    maxW = math.max(maxW, pts[i * 8 + 3]);
    final q = at(i);
    lo = Offset(math.min(lo.dx, q.dx), math.min(lo.dy, q.dy));
    hi = Offset(math.max(hi.dx, q.dx), math.max(hi.dy, q.dy));
  }
  final a = (base.a * alpha * op / n).clamp(0.0, 1.0);
  final solid = a >= 0.999;
  if (!solid) c.saveLayer(Rect.fromPoints(lo, hi).inflate(maxW + 2), Paint()..color = Color.fromRGBO(0, 0, 0, a));
  paint.color = base.withValues(alpha: 1);
  for (var i = 1; i < n; i++) {
    final w = (pts[(i - 1) * 8 + 3] + pts[i * 8 + 3]) / 2;
    c.drawLine(at(i - 1), at(i), paint..strokeWidth = math.max(0.4, w));
  }
  if (!solid) c.restore();
}

/// What the drawing screen gives back: the canvas size, the strokes, and a picture of them.
class DrawingResult {
  DrawingResult(this.w, this.h, this.strokes, this.png);
  final double w, h;
  final List<Map<String, dynamic>> strokes;
  final String png;
}

Future<DrawingResult?> editDrawing(BuildContext context, {required String dir, Map<String, dynamic>? drawing}) =>
    Navigator.of(context, rootNavigator: true).push<DrawingResult>(
      PageRouteBuilder(
        opaque: true,
        transitionDuration: const Duration(milliseconds: 260),
        pageBuilder: (ctx, a, _) => FadeTransition(
          opacity: a,
          child: _DrawingPage(dir: dir, drawing: drawing),
        ),
      ),
    );

class _Tool {
  const _Tool(this.ink, this.name, this.widths);
  final String ink, name;
  final List<double> widths;
}

const _tools = [
  _Tool('pen', 'Pen', [1, 2, 4, 7, 12]),
  _Tool('monoline', 'Monoline', [1, 2, 3.5, 6, 9]),
  _Tool('marker', 'Marker', [6, 10, 16, 24, 34]),
  _Tool('pencil', 'Pencil', [1, 2, 3, 5, 8]),
  _Tool('crayon', 'Crayon', [3, 5, 8, 12, 18]),
  _Tool('fountain', 'Fountain Pen', [1.5, 3, 5, 8, 12]),
  _Tool('watercolor', 'Watercolor', [8, 14, 22, 32, 44]),
  _Tool('reed', 'Reed Pen', [2, 4, 6, 9, 14]),
  _Tool('eraser', 'Eraser', [8, 14, 22, 32, 44]),
  _Tool('lasso', 'Lasso', []),
];

const _colors = [
  Color(0xFF000000),
  Color(0xFF0A84FF),
  Color(0xFF30D158),
  Color(0xFFFFD60A),
  Color(0xFFFF453A),
  Color(0xFFBF5AF2),
  Color(0xFF8E8E93),
  Color(0xFF64D2FF),
  Color(0xFF0E9F7A),
  Color(0xFFFF9F0A),
  Color(0xFFFF375F),
  Color(0xFFFFFFFF),
];

class _DrawingPage extends StatefulWidget {
  const _DrawingPage({required this.dir, required this.drawing});
  final String dir;
  final Map<String, dynamic>? drawing;
  @override
  State<_DrawingPage> createState() => _DrawingPageState();
}

class _DrawingPageState extends State<_DrawingPage> {
  late double _w = (widget.drawing?['w'] as num?)?.toDouble() ?? 750;
  late double _h = (widget.drawing?['h'] as num?)?.toDouble() ?? 560;
  late List<Map<String, dynamic>> _strokes = [for (final s in (widget.drawing?['strokes'] as List? ?? const [])) (s as Map).cast<String, dynamic>()];
  final _undo = <List<Map<String, dynamic>>>[], _redo = <List<Map<String, dynamic>>>[];
  Map<String, dynamic>? _live;
  int _tool = 0;
  final _width = <String, int>{};
  final _opacity = <String, double>{};
  Color _color = _colors[0];
  double _scale = 1;
  DateTime _t0 = DateTime.now();
  final _scroll = ScrollController();

  /// Lasso: the loop being drawn, the strokes it picked, where a move of them started.
  List<Offset> _lasso = const [];
  Set<int> _sel = {};
  Offset? _moveFrom;

  /// The eraser: whole strokes (Object Eraser) or the part under it (Pixel Eraser).
  bool _pixelEraser = false;

  /// The ruler: shown, its middle and angle (canvas points, radians); dragged by its body,
  /// turned by its knob. Strokes started along its edge are drawn straight along it.
  bool _ruler = false;
  Offset _rulerAt = const Offset(375, 300);
  double _rulerAngle = -math.pi / 12;
  String? _rulerDrag;

  _Tool get tool => _tools[_tool];

  @override
  void initState() {
    super.initState();
    SystemChannels.textInput.invokeMethod('TextInput.hide');
  }

  static const _rulerHalf = 30.0;

  Offset get _rulerDir => Offset(math.cos(_rulerAngle), math.sin(_rulerAngle));
  Offset get _rulerNormal => Offset(-math.sin(_rulerAngle), math.cos(_rulerAngle));
  Offset get _rulerKnob => _rulerAt + _rulerDir * 260;

  /// Distance from `p` across the ruler (negative: above it).
  double _acrossRuler(Offset p) {
    final d = p - _rulerAt;
    return d.dx * _rulerNormal.dx + d.dy * _rulerNormal.dy;
  }

  /// A stroke point drawn near the ruler's edge, put on the edge.
  Offset _onRuler(Offset p) {
    final a = _acrossRuler(p);
    final edge = a < 0 ? -_rulerHalf : _rulerHalf;
    return p - _rulerNormal * (a - edge);
  }

  bool _snapToRuler = false, _sized = false;

  bool _inside(Offset p, List<Offset> poly) {
    var c = false;
    for (var i = 0, j = poly.length - 1; i < poly.length; j = i++) {
      if ((poly[i].dy > p.dy) != (poly[j].dy > p.dy) && p.dx < (poly[j].dx - poly[i].dx) * (p.dy - poly[i].dy) / (poly[j].dy - poly[i].dy) + poly[i].dx) c = !c;
    }
    return c;
  }

  Rect? get _selBounds {
    Rect? r;
    for (final i in _sel) {
      if (i >= _strokes.length) continue;
      final pts = (_strokes[i]['pts'] as List).cast<num>();
      for (var k = 0; k + 1 < pts.length; k += 8) {
        final q = Rect.fromCircle(center: Offset(pts[k].toDouble(), pts[k + 1].toDouble()), radius: 6);
        r = r == null ? q : r.expandToInclude(q);
      }
    }
    return r;
  }

  /// The strokes picked, moved by `d` (canvas points).
  void _moveSel(Offset d) {
    _strokes = [
      for (final (i, s) in _strokes.indexed)
        if (!_sel.contains(i))
          s
        else
          {
            ...s,
            'pts': [for (final (k, v) in (s['pts'] as List).cast<num>().indexed) k % 8 == 0 ? v + d.dx : (k % 8 == 1 ? v + d.dy : v)],
          },
    ];
  }

  void _deleteSel() {
    _push();
    setState(() {
      _strokes = [
        for (final (i, s) in _strokes.indexed)
          if (!_sel.contains(i)) s,
      ];
      _sel = {};
    });
  }

  void _duplicateSel() {
    _push();
    setState(() {
      final copies = [
        for (final i in _sel)
          {
            ..._strokes[i],
            'id': newId(),
            'pts': [for (final (k, v) in (_strokes[i]['pts'] as List).cast<num>().indexed) k % 8 < 2 ? v + 24 : v],
          },
      ];
      _sel = {for (var i = 0; i < copies.length; i++) _strokes.length + i};
      _strokes = [..._strokes, ...copies];
    });
  }

  /// "+": a shape in the middle of what's showing, drawn as a monoline stroke.
  void _addShape(String kind) {
    _push();
    final viewTop = (_scroll.hasClients ? _scroll.offset : 0) / _scale;
    final c = Offset(_w / 2, viewTop + 260);
    const r = 110.0;
    final pts = <Offset>[];
    switch (kind) {
      case 'line':
        pts.addAll([c - const Offset(r, 0), c + const Offset(r, 0)]);
      case 'arrow':
        pts.addAll([c - const Offset(r, 0), c + const Offset(r, 0), c + const Offset(r - 26, -18), c + const Offset(r, 0), c + const Offset(r - 26, 18)]);
      case 'rect':
        pts.addAll([c + const Offset(-r, -70), c + const Offset(r, -70), c + const Offset(r, 70), c + const Offset(-r, 70), c + const Offset(-r, -70)]);
      default:
        for (var i = 0; i <= 64; i++) {
          final a = i / 64 * 2 * math.pi;
          pts.add(c + Offset(math.cos(a) * r, math.sin(a) * 75));
        }
    }
    final w = _tools[1].widths[_width['monoline'] ?? 2];
    setState(() {
      _strokes = [
        ..._strokes,
        {
          'id': newId(),
          'ink': 'monoline',
          'color': [_color.r, _color.g, _color.b, 1.0],
          'width': w,
          'pts': [
            for (final (i, p) in pts.indexed) ...[p.dx, p.dy, i * 0.01, w, 1.0, 0.0, math.pi / 2, 1.0],
          ],
          'tf': [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
        },
      ];
    });
  }

  /// The pixel eraser: the strokes' points under it go, the strokes splitting where it passed.
  void _pixelErase(Offset at) {
    final r = tool.widths[_width['eraser'] ?? 2] / 2;
    var changed = false;
    final out = <Map<String, dynamic>>[];
    for (final s in _strokes) {
      final pts = (s['pts'] as List).cast<num>();
      final runs = <List<num>>[<num>[]];
      var hit = false;
      for (var i = 0; i + 7 < pts.length; i += 8) {
        if ((Offset(pts[i].toDouble(), pts[i + 1].toDouble()) - at).distance < r + pts[i + 3].toDouble() / 2) {
          hit = true;
          if (runs.last.isNotEmpty) runs.add(<num>[]);
        } else {
          runs.last.addAll(pts.sublist(i, i + 8));
        }
      }
      if (!hit) {
        out.add(s);
        continue;
      }
      changed = true;
      for (final run in runs.where((r) => r.length >= 16)) {
        out.add({...s, 'id': newId(), 'pts': run});
      }
    }
    if (changed) setState(() => _strokes = out);
  }

  @override
  void dispose() {
    _scroll.dispose();
    super.dispose();
  }

  void _push() {
    _undo.add([..._strokes]);
    _redo.clear();
  }

  Offset _local(Offset p) => p / _scale;

  final _fingers = <int>{};

  void _down(PointerDownEvent e) {
    _fingers.add(e.pointer);
    // a second finger scrolls: no stroke
    if (_fingers.length > 1) {
      setState(() => _live = null);
      return;
    }
    final at = _local(e.localPosition);
    // the ruler: its knob turns it, its body moves it
    if (_ruler && (at - _rulerKnob).distance < 34) {
      _rulerDrag = 'turn';
      return;
    }
    if (_ruler && _acrossRuler(at).abs() < _rulerHalf - 4) {
      _rulerDrag = 'move';
      return;
    }
    if (tool.ink == 'eraser') {
      _push();
      _pixelEraser ? _pixelErase(at) : _erase(at);
      return;
    }
    if (tool.ink == 'lasso') {
      final b = _selBounds;
      if (b != null && b.inflate(10).contains(at)) {
        _push();
        _moveFrom = at;
      } else {
        setState(() {
          _sel = {};
          _lasso = [at];
        });
      }
      return;
    }
    _snapToRuler = _ruler && (_acrossRuler(at).abs() - _rulerHalf).abs() < 28;
    _t0 = DateTime.now();
    final w = tool.widths[_width[tool.ink] ?? 2];
    final c = _color;
    _live = {
      'id': newId(),
      'ink': tool.ink,
      'color': [c.r, c.g, c.b, (_opacity[tool.ink] ?? 1.0)],
      'width': w,
      'pts': <double>[],
      'tf': [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
    };
    _add(e, at);
  }

  void _add(PointerEvent e, Offset at) {
    final w = (_live!['width'] as num).toDouble();
    final pressure = e.kind == ui.PointerDeviceKind.stylus && e.pressureMax > 0 ? (0.4 + 1.2 * e.pressure / e.pressureMax) : 1.0;
    final t = DateTime.now().difference(_t0).inMicroseconds / 1e6;
    final q = _snapToRuler ? _onRuler(at) : at;
    (_live!['pts'] as List<double>).addAll([q.dx, q.dy, t, w * pressure, e.pressure, 0, math.pi / 2, 1]);
    // the canvas grows as the drawing nears its foot
    if (at.dy > _h - 60) _h += 300;
    setState(() {});
  }

  void _move(PointerMoveEvent e) {
    final at = _local(e.localPosition);
    if (_fingers.length > 1) return;
    if (_rulerDrag == 'turn') return setState(() => _rulerAngle = math.atan2(at.dy - _rulerAt.dy, at.dx - _rulerAt.dx));
    if (_rulerDrag == 'move') return setState(() => _rulerAt += e.localDelta / _scale);
    if (tool.ink == 'eraser') return _pixelEraser ? _pixelErase(at) : _erase(at);
    if (tool.ink == 'lasso') {
      if (_moveFrom != null) {
        setState(() => _moveSel(at - _moveFrom!));
        _moveFrom = at;
      } else if (_lasso.isNotEmpty) {
        setState(() => _lasso = [..._lasso, at]);
      }
      return;
    }
    if (_live != null) _add(e, at);
  }

  void _up(PointerUpEvent e) {
    _fingers.remove(e.pointer);
    if (_rulerDrag != null) {
      _rulerDrag = null;
      return;
    }
    if (tool.ink == 'lasso') {
      if (_moveFrom != null) {
        _moveFrom = null;
        return;
      }
      // the strokes mostly inside the loop
      final loop = _lasso;
      final sel = <int>{};
      if (loop.length > 2) {
        for (final (i, s) in _strokes.indexed) {
          final pts = (s['pts'] as List).cast<num>();
          var inside = 0, n = 0;
          for (var k = 0; k + 1 < pts.length; k += 8) {
            n++;
            if (_inside(Offset(pts[k].toDouble(), pts[k + 1].toDouble()), loop)) inside++;
          }
          if (n > 0 && inside / n >= 0.6) sel.add(i);
        }
      }
      setState(() {
        _sel = sel;
        _lasso = const [];
      });
      return;
    }
    if (_live == null) return;
    _push();
    setState(() {
      _strokes = [..._strokes, _live!];
      _live = null;
    });
  }

  /// The object eraser: strokes passing near the point go.
  void _erase(Offset at) {
    final r = tool.widths[_width['eraser'] ?? 2] / 2;
    final keep = _strokes.where((s) {
      final pts = (s['pts'] as List).cast<num>();
      for (var i = 0; i + 1 < pts.length; i += 8) {
        if ((Offset(pts[i].toDouble(), pts[i + 1].toDouble()) - at).distance < r + (pts[i + 3].toDouble() / 2)) return false;
      }
      return true;
    }).toList();
    if (keep.length != _strokes.length) setState(() => _strokes = keep);
  }

  Future<void> _done() async {
    final nav = Navigator.of(context);
    // a picture of it (what Notes shows where it can't read the strokes)
    final rec = ui.PictureRecorder();
    final cv = Canvas(rec);
    const k = 2.0;
    final used = _usedHeight();
    InkPainter(_strokes, k).paint(cv, Size(_w * k, used * k));
    final img = await rec.endRecording().toImage((_w * k).round(), (used * k).round());
    final png = await img.toByteData(format: ui.ImageByteFormat.png);
    final dir = Directory('${widget.dir}/drawings')..createSync(recursive: true);
    final f = File('${dir.path}/${newId()}.png');
    await f.writeAsBytes(png!.buffer.asUint8List());
    nav.pop(DrawingResult(_w, used, _strokes, f.path));
  }

  /// The canvas's height down to the lowest stroke (at least its first height).
  double _usedHeight() {
    var maxY = 0.0;
    for (final s in _strokes) {
      final pts = (s['pts'] as List).cast<num>();
      for (var i = 1; i < pts.length; i += 8) {
        maxY = math.max(maxY, pts[i].toDouble() + pts[i + 2].toDouble());
      }
    }
    return math.max(math.min(_h, (widget.drawing?['h'] as num?)?.toDouble() ?? 300), maxY + 40).clamp(120, 20000).toDouble();
  }

  void _toolOptions(BuildContext bctx) {
    final t = tool;
    showSheet<void>(
      context,
      title: t.name,
      fit: true,
      dim: false,
      builder: (ctx) => StatefulBuilder(
        builder: (ctx, set) {
          final p = Palette.of(ctx);
          return Padding(
            padding: const EdgeInsets.fromLTRB(16, 0, 16, 12),
            child: Column(
              mainAxisSize: MainAxisSize.min,
              children: [
                Row(
                  mainAxisAlignment: MainAxisAlignment.spaceEvenly,
                  children: [
                    for (final (i, w) in t.widths.indexed)
                      Pressable(
                        onTap: () => set(() => setState(() => _width[t.ink] = i)),
                        child: Container(
                          width: 52,
                          height: 52,
                          decoration: BoxDecoration(
                            shape: BoxShape.circle,
                            color: (_width[t.ink] ?? 2) == i ? p.fill : null,
                            border: Border.all(color: (_width[t.ink] ?? 2) == i ? p.accent : Colors.transparent, width: 2),
                          ),
                          child: Center(
                            child: Container(
                              width: math.min(36, w * 1.2 + 3),
                              height: math.min(36, w * 1.2 + 3),
                              decoration: BoxDecoration(shape: BoxShape.circle, color: p.label),
                            ),
                          ),
                        ),
                      ),
                  ],
                ),
                if (t.ink == 'eraser') ...[
                  const SizedBox(height: 14),
                  Card2(
                    children: [
                      CardRow(label: 'Pixel Eraser', checked: _pixelEraser, onTap: () => set(() => setState(() => _pixelEraser = true))),
                      CardRow(label: 'Object Eraser', checked: !_pixelEraser, onTap: () => set(() => setState(() => _pixelEraser = false))),
                    ],
                  ),
                ],
                if (t.ink != 'eraser') ...[
                  const SizedBox(height: 16),
                  Row(
                    children: [
                      Text('Opacity', style: Type.body(p.label)),
                      Expanded(
                        child: CupertinoSlider(
                          value: _opacity[t.ink] ?? 1,
                          min: 0.1,
                          max: 1,
                          activeColor: p.accent,
                          onChanged: (v) => set(() => setState(() => _opacity[t.ink] = v)),
                        ),
                      ),
                      SizedBox(
                        width: 48,
                        child: Text('${((_opacity[t.ink] ?? 1) * 100).round()}%', textAlign: TextAlign.right, style: Type.body(p.secondary)),
                      ),
                    ],
                  ),
                ],
              ],
            ),
          );
        },
      ),
    );
  }

  void _pickColor() {
    showSheet<void>(
      context,
      title: 'Colors',
      fit: true,
      dim: false,
      builder: (ctx) {
        final p = Palette.of(ctx);
        return Padding(
          padding: const EdgeInsets.fromLTRB(16, 0, 16, 12),
          child: Wrap(
            spacing: 14,
            runSpacing: 14,
            alignment: WrapAlignment.center,
            children: [
              for (final c in _colors)
                Pressable(
                  onTap: () {
                    setState(() => _color = c);
                    Navigator.of(ctx).pop();
                  },
                  child: Container(
                    width: 48,
                    height: 48,
                    decoration: BoxDecoration(
                      shape: BoxShape.circle,
                      color: c,
                      border: Border.all(color: c == _color ? p.accent : p.separator, width: c == _color ? 3 : 1),
                    ),
                  ),
                ),
            ],
          ),
        );
      },
    );
  }

  @override
  Widget build(BuildContext context) {
    final p = Palette.of(context);
    final mq = MediaQuery.of(context);
    return Scaffold(
      backgroundColor: p.page,
      // (no keyboard here: the canvas keeps the whole screen)
      resizeToAvoidBottomInset: false,
      body: Stack(
        // (the whole screen, however short the canvas)
        fit: StackFit.expand,
        children: [
          LayoutBuilder(
            builder: (ctx, box) {
              _scale = box.maxWidth / _w;
              // a new drawing: as tall as the room for it on screen
              if (widget.drawing == null && !_sized) {
                _sized = true;
                _h = math.max(_h, (box.maxHeight - mq.padding.top - 64 - 140 - mq.padding.bottom) / _scale);
              }
              return SingleChildScrollView(
                controller: _scroll,
                // one finger draws; scrolling is two fingers (or outside the canvas)
                physics: const NeverScrollableScrollPhysics(),
                padding: EdgeInsets.only(top: mq.padding.top + 64, bottom: 140 + mq.padding.bottom),
                child: Listener(
                  onPointerDown: _down,
                  onPointerMove: _move,
                  onPointerUp: _up,
                  onPointerCancel: (e) => setState(() {
                    _fingers.remove(e.pointer);
                    _live = null;
                  }),
                  child: SizedBox(
                    width: box.maxWidth,
                    height: _h * _scale,
                    child: CustomPaint(
                      painter: _Paper(p, _scale),
                      foregroundPainter: InkPainter(_strokes, _scale, dark: !p.light, live: _live),
                      child: CustomPaint(
                        size: Size(box.maxWidth, _h * _scale),
                        foregroundPainter: _Overlay(
                          _scale,
                          lasso: _lasso,
                          sel: _selBounds,
                          ruler: _ruler ? (_rulerAt, _rulerAngle, _rulerHalf, _rulerKnob) : null,
                          accent: p.accent,
                        ),
                      ),
                    ),
                  ),
                ),
              );
            },
          ),
          // the top bar: Done, undo, redo
          Positioned(
            top: mq.padding.top + 6,
            left: 12,
            right: 12,
            child: Row(
              children: [
                GlassButton(icon: CupertinoIcons.xmark, semantic: 'Cancel', onTap: () => Navigator.of(context).pop()),
                const Spacer(),
                GlassGroup(
                  items: [
                    GlassItem(
                      icon: CupertinoIcons.arrow_uturn_left,
                      onTap: _undo.isEmpty
                          ? null
                          : (_) => setState(() {
                              _redo.add(_strokes);
                              _strokes = _undo.removeLast();
                            }),
                    ),
                    GlassItem(
                      icon: CupertinoIcons.arrow_uturn_right,
                      onTap: _redo.isEmpty
                          ? null
                          : (_) => setState(() {
                              _undo.add(_strokes);
                              _strokes = _redo.removeLast();
                            }),
                    ),
                  ],
                ),
                const SizedBox(width: 8),
                GlassButton(icon: CupertinoIcons.checkmark_alt, tinted: true, semantic: 'Done', onTap: _done),
              ],
            ),
          ),
          // the lasso's picked strokes: Delete, Duplicate
          if (_sel.isNotEmpty && _selBounds != null)
            Positioned(
              left: 0,
              right: 0,
              top: mq.padding.top + 70,
              child: Center(
                child: GlassGroup(
                  items: [
                    GlassItem(label: 'Delete', color: p.red, onTap: (_) => _deleteSel()),
                    GlassItem(label: 'Duplicate', onTap: (_) => _duplicateSel()),
                  ],
                ),
              ),
            ),
          // two-finger scrolling over the canvas
          Positioned.fill(
            top: mq.padding.top + 64,
            bottom: 120,
            child: _TwoFingerScroll(controller: _scroll),
          ),
          // the palette
          Positioned(
            left: 10,
            right: 10,
            bottom: mq.padding.bottom + 10,
            child: Glass(
              radius: 30,
              child: SizedBox(
                height: 96,
                child: Row(
                  children: [
                    Expanded(
                      child: SingleChildScrollView(
                        scrollDirection: Axis.horizontal,
                        padding: const EdgeInsets.symmetric(horizontal: 10),
                        child: Row(
                          crossAxisAlignment: CrossAxisAlignment.end,
                          children: [
                            for (final (i, t) in _tools.indexed)
                              Builder(
                                builder: (bctx) => GestureDetector(
                                  behavior: HitTestBehavior.opaque,
                                  onTap: () => i == _tool
                                      ? (t.widths.isEmpty ? null : _toolOptions(bctx))
                                      : setState(() {
                                          _tool = i;
                                          _sel = {};
                                        }),
                                  child: Semantics(
                                    button: true,
                                    label: t.name,
                                    child: AnimatedContainer(
                                      duration: const Duration(milliseconds: 160),
                                      width: 44,
                                      height: 96,
                                      padding: EdgeInsets.only(top: i == _tool ? 6 : 22),
                                      child: CustomPaint(
                                        painter: _Instrument(t.ink, t.ink == 'eraser' || t.ink == 'lasso' ? null : (i == _tool ? _color : Colors.black), p),
                                      ),
                                    ),
                                  ),
                                ),
                              ),
                          ],
                        ),
                      ),
                    ),
                    // the ruler and "+" (shapes)
                    Pressable(
                      onTap: () => setState(() {
                        _ruler = !_ruler;
                        // it comes in across the middle of what's showing
                        if (_ruler) {
                          final h = MediaQuery.sizeOf(context).height - mq.padding.top - 64 - 140;
                          _rulerAt = Offset(_w / 2, ((_scroll.hasClients ? _scroll.offset : 0) + h / 2) / _scale);
                        }
                      }),
                      child: Semantics(
                        button: true,
                        label: 'Ruler',
                        selected: _ruler,
                        child: Container(
                          width: 44,
                          height: 44,
                          decoration: BoxDecoration(shape: BoxShape.circle, color: _ruler ? p.accent : p.fill),
                          child: Icon(CupertinoIcons.arrow_right_arrow_left, size: 20, color: _ruler ? Colors.white : p.label),
                        ),
                      ),
                    ),
                    const SizedBox(width: 6),
                    Builder(
                      builder: (bctx) => Pressable(
                        onTap: () => showTouchMenu(context, p.look, globalRect(bctx), [
                          {'label': 'Line', 'icon': CupertinoIcons.minus, 'k': 'line'},
                          {'label': 'Arrow', 'icon': CupertinoIcons.arrow_right, 'k': 'arrow'},
                          {'label': 'Rectangle', 'icon': CupertinoIcons.rectangle, 'k': 'rect'},
                          {'label': 'Oval', 'icon': CupertinoIcons.circle, 'k': 'oval'},
                        ], (i) => _addShape(i['k'] as String)),
                        child: Semantics(
                          button: true,
                          label: 'Add Shape',
                          child: Container(
                            width: 44,
                            height: 44,
                            decoration: BoxDecoration(shape: BoxShape.circle, color: p.fill),
                            child: Icon(CupertinoIcons.plus, size: 22, color: p.label),
                          ),
                        ),
                      ),
                    ),
                    Pressable(
                      onTap: _pickColor,
                      child: Container(
                        width: 48,
                        height: 48,
                        margin: const EdgeInsets.only(right: 14, left: 6),
                        decoration: BoxDecoration(
                          shape: BoxShape.circle,
                          gradient: const SweepGradient(colors: [Colors.red, Colors.yellow, Colors.green, Colors.cyan, Colors.blue, Colors.purple, Colors.red]),
                        ),
                        child: Center(
                          child: Container(
                            width: 30,
                            height: 30,
                            decoration: BoxDecoration(
                              shape: BoxShape.circle,
                              color: _color,
                              border: Border.all(color: Colors.white, width: 2),
                            ),
                          ),
                        ),
                      ),
                    ),
                  ],
                ),
              ),
            ),
          ),
        ],
      ),
    );
  }
}

/// Lets two fingers scroll the canvas while one draws.
class _TwoFingerScroll extends StatefulWidget {
  const _TwoFingerScroll({required this.controller});
  final ScrollController controller;
  @override
  State<_TwoFingerScroll> createState() => _TwoFingerScrollState();
}

class _TwoFingerScrollState extends State<_TwoFingerScroll> {
  final _down = <int, Offset>{};
  @override
  Widget build(BuildContext context) => Listener(
    behavior: HitTestBehavior.translucent,
    onPointerDown: (e) => _down[e.pointer] = e.position,
    onPointerUp: (e) => _down.remove(e.pointer),
    onPointerCancel: (e) => _down.remove(e.pointer),
    onPointerMove: (e) {
      if (_down.length < 2) return;
      final c = widget.controller;
      if (!c.hasClients) return;
      c.jumpTo((c.offset - e.delta.dy / _down.length).clamp(0, c.position.maxScrollExtent));
    },
  );
}

class _Paper extends CustomPainter {
  _Paper(this.p, this.scale);
  final Palette p;
  final double scale;
  @override
  void paint(Canvas c, Size s) {
    c.drawRect(Offset.zero & s, Paint()..color = p.page);
    // the canvas's edge, faint
    c.drawRect(
      (Offset.zero & s).deflate(0.5),
      Paint()
        ..style = PaintingStyle.stroke
        ..color = p.separator.withValues(alpha: 0.4),
    );
  }

  @override
  bool shouldRepaint(_Paper o) => o.p != p;
}

/// A palette instrument: its body, its colour band, its tip.
class _Instrument extends CustomPainter {
  _Instrument(this.ink, this.color, this.p);
  final String ink;
  final Color? color;
  final Palette p;
  @override
  void paint(Canvas c, Size s) {
    final w = s.width * 0.56, x = (s.width - w) / 2;
    final body = Paint()..color = p.light ? const Color(0xFFF6F6F6) : const Color(0xFFE6E6E6);
    final edge = Paint()
      ..style = PaintingStyle.stroke
      ..strokeWidth = 0.8
      ..color = const Color(0x33000000);
    final top = 24.0;
    if (ink == 'lasso') {
      // a body with a dashed loop at its tip
      c.drawRRect(RRect.fromLTRBR(x, top, x + w, s.height, const Radius.circular(4)), body);
      c.drawRRect(RRect.fromLTRBR(x, top, x + w, s.height, const Radius.circular(4)), edge);
      final loop = Paint()
        ..style = PaintingStyle.stroke
        ..strokeWidth = 1.6
        ..color = const Color(0xFF444448);
      final cen = Offset(s.width / 2, top - 9);
      for (var a = 0.0; a < 2 * math.pi; a += math.pi / 5) {
        c.drawArc(Rect.fromCircle(center: cen, radius: 9), a, math.pi / 9, false, loop);
      }
      return;
    }
    if (ink == 'eraser') {
      final r = RRect.fromLTRBR(x, top - 6, x + w, s.height, const Radius.circular(6));
      c.drawRRect(r, body);
      c.drawRRect(RRect.fromLTRBR(x, top - 6, x + w, top + 10, const Radius.circular(6)), Paint()..color = const Color(0xFFF2A0A8));
      c.drawRRect(r, edge);
      return;
    }
    final tipLen = switch (ink) {
      'marker' => 14.0,
      'watercolor' => 18.0,
      'monoline' => 16.0,
      _ => 20.0,
    };
    final tipW = switch (ink) {
      'marker' => w * 0.6,
      'watercolor' => w * 0.5,
      'crayon' => w * 0.55,
      _ => w * 0.22,
    };
    final path = Path()
      ..moveTo(x, s.height)
      ..lineTo(x, top)
      ..lineTo(s.width / 2 - tipW / 2, top - tipLen)
      ..lineTo(s.width / 2 + tipW / 2, top - tipLen)
      ..lineTo(x + w, top)
      ..lineTo(x + w, s.height)
      ..close();
    c.drawPath(path, body);
    c.drawPath(path, edge);
    final band = color ?? Colors.black;
    // the tip in the ink's colour, and a band
    c.drawPath(
      Path()
        ..moveTo(s.width / 2 - tipW / 2 - 2, top - tipLen + 5)
        ..lineTo(s.width / 2 - tipW / 2, top - tipLen)
        ..lineTo(s.width / 2 + tipW / 2, top - tipLen)
        ..lineTo(s.width / 2 + tipW / 2 + 2, top - tipLen + 5)
        ..close(),
      Paint()..color = band,
    );
    c.drawRect(Rect.fromLTWH(x, top + 10, w, 6), Paint()..color = band.withValues(alpha: ink == 'marker' || ink == 'watercolor' ? 0.6 : 1));
  }

  @override
  bool shouldRepaint(_Instrument o) => o.color != color || o.ink != ink || o.p != p;
}

/// The lasso's loop, the picked strokes' box, the ruler.
class _Overlay extends CustomPainter {
  _Overlay(this.scale, {required this.lasso, required this.sel, required this.ruler, required this.accent});
  final double scale;
  final List<Offset> lasso;
  final Rect? sel;
  final (Offset, double, double, Offset)? ruler;
  final Color accent;

  @override
  void paint(Canvas c, Size s) {
    c.save();
    c.scale(scale);
    final dash = Paint()
      ..style = PaintingStyle.stroke
      ..strokeWidth = 1.5 / scale
      ..color = accent;
    if (lasso.length > 1) c.drawPath(Path()..addPolygon(lasso, false), dash);
    if (sel != null) c.drawRRect(RRect.fromRectAndRadius(sel!.inflate(8), Radius.circular(8 / scale)), dash);
    if (ruler != null) {
      final (at, angle, half, knob) = ruler!;
      c.save();
      c.translate(at.dx, at.dy);
      c.rotate(angle);
      final body = Rect.fromLTRB(-2000, -half, 2000, half);
      c.drawRect(body, Paint()..color = const Color(0xCCB8B8BE));
      final tick = Paint()
        ..color = const Color(0xFF444448)
        ..strokeWidth = 1;
      for (var x = -2000.0; x <= 2000; x += 10) {
        final long = (x % 50).abs() < 0.5;
        c.drawLine(Offset(x, -half), Offset(x, -half + (long ? 14 : 7)), tick);
      }
      c.restore();
      c.drawCircle(knob, 16, Paint()..color = accent);
      c.drawCircle(
        knob,
        16,
        Paint()
          ..style = PaintingStyle.stroke
          ..strokeWidth = 2
          ..color = Colors.white,
      );
    }
    c.restore();
  }

  @override
  bool shouldRepaint(_Overlay o) => true;
}
