// The Fill and Border editors (objinsp.rs fill_editor, advanced_gradient_editor,
// stroke_editor): No Fill / Color / Gradient / Image / Advanced Image / Advanced Gradient, and
// No Border / Line with its dash, colour and width. They edit the engine's fill data
// (inspapi fill_json) and send back the whole new fill.
import 'dart:math' as math;

import 'package:file_picker/file_picker.dart';
import 'package:flutter/material.dart';

import 'look.dart';
import 'widgets.dart';

const fillNames = ['No Fill', 'Color Fill', 'Gradient Fill', 'Image Fill', 'Advanced Image Fill', 'Advanced Gradient Fill'];

Color _lerp(Color a, Color b, double t) => Color.lerp(a, b, t)!;

class FillEditor extends StatelessWidget {
  const FillEditor({super.key, required this.fill, required this.onChanged});
  final Map<String, dynamic> fill;
  final void Function(Map<String, Object?> fill) onChanged;

  Color _base() => switch (fill['kind']) {
        1 => colorOf(fill['color']),
        2 => colorOf(fill['from']),
        5 => colorOf(((fill['stops'] as List?)?.firstOrNull as List?)?[1], Colors.white),
        _ => const Color.from(alpha: 1, red: 0, green: 0.63, blue: 1),
      };

  Future<void> _pick(int k) async {
    final base = _base();
    switch (k) {
      case 0:
        onChanged({'kind': 0});
      case 1:
        onChanged({'kind': 1, 'color': colorTo(base)});
      case 2:
        onChanged({'kind': 2, 'from': colorTo(base), 'to': colorTo(Colors.white), 'angle': 270.0});
      case 3 || 4:
        final tint = k == 4 ? [0.0, 0.48, 1.0, 0.35] : null;
        if (fill['kind'] == 3 || fill['kind'] == 4) {
          onChanged({...fill, 'kind': k, 'tint': tint});
        } else {
          final f = await FilePicker.pickFile(type: FileType.image);
          if (f?.path != null) onChanged({'kind': k, 'path': f!.path, 'technique': 'ScaleToFill', 'tint': tint});
        }
      default:
        final stops = fill['kind'] == 2
            ? [[0.0, fill['from'], 0.5], [1.0, fill['to'], 0.5]]
            : [[0.0, colorTo(base), 0.5], [1.0, colorTo(Colors.white), 0.5]];
        onChanged({'kind': 5, 'stops': stops, 'radial': false, 'angle': (fill['angle'] as num?)?.toDouble() ?? 270.0});
    }
  }

  @override
  Widget build(BuildContext context) {
    final kind = (fill['kind'] as int?) ?? 0;
    return EguiColumn(children: [
      DCombo<int>(text: fillNames[kind], items: () => [for (final (i, n) in fillNames.indexed) DChoice(n, i, on: i == kind)], onPick: (k) {
        if (k != kind) _pick(k);
      }),
      if (kind == 1) DRow('Color', DColorWell(color: colorOf(fill['color']), onChanged: (c) => onChanged({'kind': 1, 'color': colorTo(c)}))),
      if (kind == 2) ..._gradient(),
      if (kind == 5) AdvancedGradient(fill: fill, onChanged: onChanged),
      if (kind == 3 || kind == 4) ..._image(context),
    ]);
  }

  List<Widget> _gradient() {
    final from = colorOf(fill['from']), to = colorOf(fill['to']);
    final angle = (fill['angle'] as num?)?.toDouble() ?? 0;
    void put(Color f, Color t, double a) => onChanged({'kind': 2, 'from': colorTo(f), 'to': colorTo(t), 'angle': a});
    return [
      Row(children: [
        DColorWell(color: from, onChanged: (c) => put(c, to, angle)),
        const SizedBox(width: 8),
        DColorWell(color: to, onChanged: (c) => put(from, c, angle)),
        const Spacer(),
        DButton(onTap: () => put(to, from, angle), child: const Text('⇄')),
      ]),
      DRow('Angle', DStepper(value: angle, min: 0, max: 360, suffix: '°', onChanged: (a) => put(from, to, a))),
    ];
  }

  List<Widget> _image(BuildContext context) {
    const techniques = [('Natural', 'Original Size'), ('Stretch', 'Stretch'), ('Tile', 'Tile'), ('ScaleToFill', 'Scale to Fill'), ('ScaleToFit', 'Scale to Fit')];
    final t = fill['technique'] as String? ?? 'ScaleToFill';
    final tint = fill['tint'];
    final natural = fill['natural'] as List?;
    final size = fill['size'] as List?;
    return [
      DCombo<String>(
        text: techniques.firstWhere((x) => x.$1 == t, orElse: () => techniques[3]).$2,
        items: () => [for (final x in techniques) DChoice(x.$2, x.$1, on: x.$1 == t)],
        onPick: (v) => onChanged({...fill, 'technique': v}),
      ),
      Row(children: [
        if (tint != null) ...[
          const DLabel('Tint'),
          const SizedBox(width: 8),
          DColorWell(color: colorOf(tint), onChanged: (c) {
            // the fallback colour: the picture's own average under the new tint (objinsp)
            final r = fill['reference'];
            List<double>? reference;
            if (r is List) {
              final tc = colorOf(tint);
              final a = tc.a.clamp(0.0, 0.99);
              double un(double v, double tv) => ((v - tv * a) / (1 - a)).clamp(0.0, 1.0);
              final rc = colorOf(r);
              final b = [un(rc.r, tc.r), un(rc.g, tc.g), un(rc.b, tc.b)];
              reference = [b[0] + (c.r - b[0]) * c.a, b[1] + (c.g - b[1]) * c.a, b[2] + (c.b - b[2]) * c.a, 1.0];
            }
            onChanged({...fill, 'tint': colorTo(c), 'reference': reference});
          }),
        ],
        const Spacer(),
        DButton(
          onTap: () async {
            final f = await FilePicker.pickFile(type: FileType.image);
            if (f?.path != null) onChanged({...fill, 'path': f!.path});
          },
          child: const Text('Choose…'),
        ),
      ]),
      if ((t == 'Tile' || t == 'Natural') && natural != null)
        Row(children: [
          const DLabel('Scale'),
          const SizedBox(width: 8),
          Expanded(
            child: DSlider(
              value: size == null ? 100 : (size[0] as num).toDouble() / math.max(1, (natural[0] as num).toDouble()) * 100,
              min: 1,
              max: 400,
              suffix: '%',
              onChanged: (p) {
                final f = p / 100;
                onChanged({...fill, 'size': [((natural[0] as num) * f).roundToDouble(), ((natural[1] as num) * f).roundToDouble()]});
              },
            ),
          ),
        ]),
    ];
  }
}

/// egui's Slider in kn_ui's look (the track filled up to a round white knob) and its value.
class DSlider extends StatefulWidget {
  const DSlider({super.key, required this.value, required this.min, required this.max, required this.onChanged, this.suffix = '', this.showValue = true});
  final double value, min, max;
  final String suffix;
  final bool showValue;
  final void Function(double) onChanged;
  @override
  State<DSlider> createState() => _DSliderState();
}

class _DSliderState extends State<DSlider> {
  double? _live;
  @override
  Widget build(BuildContext context) {
    final look = LookScope.of(context);
    final touch = TouchScope.of(context);
    final v = (_live ?? widget.value).clamp(widget.min, widget.max);
    return Row(children: [
      Expanded(
        child: LayoutBuilder(builder: (ctx, box) {
          final w = box.maxWidth;
          double at(double x) => (widget.min + (x - 9.6) / math.max(1, w - 19.2) * (widget.max - widget.min)).clamp(widget.min, widget.max).roundToDouble();
          return GestureDetector(
            behavior: HitTestBehavior.opaque,
            onHorizontalDragUpdate: (d) => setState(() => _live = at(d.localPosition.dx)),
            onHorizontalDragEnd: (_) {
              if (_live != null) widget.onChanged(_live!);
              setState(() => _live = null);
            },
            onTapUp: (d) => widget.onChanged(at(d.localPosition.dx)),
            child: SizedBox(
              height: touch ? 40 : 24,
              child: CustomPaint(painter: _SliderPainter((v - widget.min) / math.max(1e-9, widget.max - widget.min), look, touch)),
            ),
          );
        }),
      ),
      if (widget.showValue) ...[
        const SizedBox(width: 8),
        SizedBox(width: touch ? 56 : 48, child: Text('${v.round()}${widget.suffix}', style: Look.font(touch ? 15 : 13, look.text))),
      ],
    ]);
  }
}

class _SliderPainter extends CustomPainter {
  _SliderPainter(this.t, this.look, this.touch);
  final double t;
  final Look look;
  final bool touch;
  @override
  void paint(Canvas c, Size s) {
    if (touch) {
      // touch: iOS's slider: a 4-point rail, the accent up to a white 28-point knob
      final y = s.height / 2;
      const r = 14.0;
      final x = r + t * (s.width - 2 * r);
      c.drawRRect(RRect.fromLTRBR(0, y - 2, s.width, y + 2, const Radius.circular(2)), Paint()..color = touchFill(look));
      c.drawRRect(RRect.fromLTRBR(0, y - 2, x, y + 2, const Radius.circular(2)), Paint()..color = look.accent);
      c.drawCircle(Offset(x, y + 1), r, Paint()
        ..color = const Color(0x33000000)
        ..maskFilter = const MaskFilter.blur(BlurStyle.normal, 2));
      c.drawCircle(Offset(x, y), r, Paint()..color = Colors.white);
      return;
    }
    // egui's Slider: an 8-point rail the slider's width, the accent up to the handle (trailing
    // fill), a round handle of radius height / 2.5 travelling inside the ends
    final y = s.height / 2;
    final r = s.height / 2.5;
    final x = r + t * (s.width - 2 * r);
    c.drawRRect(RRect.fromLTRBR(0, y - 4, s.width, y + 4, const Radius.circular(4)), Paint()..color = look.control);
    c.drawRRect(RRect.fromLTRBR(0, y - 4, x, y + 4, const Radius.circular(4)), Paint()..color = look.accent);
    c.drawCircle(Offset(x, y), r, Paint()..color = look.control);
    c.drawCircle(Offset(x, y), r - 0.5, Paint()
      ..style = PaintingStyle.stroke
      ..color = look.text);
  }

  @override
  bool shouldRepaint(_SliderPainter o) => o.t != t || o.touch != touch;
}

/// objinsp advanced_gradient_editor: the bar, stops under it (drag to move, tap the bar to
/// add one, drag one well below to remove it), inflection diamonds over it; Color of the
/// chosen stop; Linear | Radial, reverse, Angle.
class AdvancedGradient extends StatefulWidget {
  const AdvancedGradient({super.key, required this.fill, required this.onChanged});
  final Map<String, dynamic> fill;
  final void Function(Map<String, Object?>) onChanged;
  @override
  State<AdvancedGradient> createState() => _AdvancedGradientState();
}

class _AdvancedGradientState extends State<AdvancedGradient> {
  List<(double, Color, double)>? _work;
  int _sel = 0;
  int? _dragStop, _dragInfl;

  List<(double, Color, double)> get _stops => _work ?? [for (final s in (widget.fill['stops'] as List? ?? [])) ((s[0] as num).toDouble(), colorOf(s[1]), (s[2] as num).toDouble())];

  Color _at(List<(double, Color, double)> s, double x) {
    if (s.isEmpty) return Colors.white;
    if (x <= s.first.$1) return s.first.$2;
    for (var i = 0; i + 1 < s.length; i++) {
      if (x <= s[i + 1].$1) {
        final t = ((x - s[i].$1) / math.max(1e-4, s[i + 1].$1 - s[i].$1)).clamp(0.0, 1.0);
        final m = s[i].$3.clamp(0.01, 0.99);
        final u = t < m ? t / m * 0.5 : 0.5 + (t - m) / (1 - m) * 0.5;
        return _lerp(s[i].$2, s[i + 1].$2, u);
      }
    }
    return s.last.$2;
  }

  void _commit(List<(double, Color, double)> s, {bool? radial, double? angle}) {
    setState(() => _work = null);
    widget.onChanged({
      'kind': 5,
      'stops': [for (final x in s) [x.$1, colorTo(x.$2), x.$3]],
      'radial': radial ?? widget.fill['radial'] == true,
      'angle': angle ?? (widget.fill['angle'] as num?)?.toDouble() ?? 270,
    });
  }

  @override
  Widget build(BuildContext context) {
    final look = LookScope.of(context);
    final stops = _stops;
    _sel = _sel.clamp(0, math.max(0, stops.length - 1));
    final radial = widget.fill['radial'] == true;
    final angle = (widget.fill['angle'] as num?)?.toDouble() ?? 270;
    return EguiColumn(children: [
      LayoutBuilder(builder: (ctx, box) {
        final bar = Rect.fromLTRB(8, 10, box.maxWidth - 8, 30);
        double frac(double x) => ((x - bar.left) / bar.width).clamp(0.0, 1.0);
        return GestureDetector(
          behavior: HitTestBehavior.opaque,
          onTapUp: (d) {
            final p = d.localPosition;
            for (var i = 0; i < stops.length; i++) {
              if ((Offset(bar.left + stops[i].$1 * bar.width, bar.bottom + 4) - p).distance < 12) {
                setState(() => _sel = i);
                return;
              }
            }
            if (bar.inflate(4).contains(p)) {
              final f = frac(p.dx);
              final s = [...stops];
              final at = s.indexWhere((x) => x.$1 > f);
              final idx = at < 0 ? s.length : at;
              s.insert(idx, (f, _at(stops, f), 0.5));
              _sel = idx;
              _commit(s);
            }
          },
          onPanStart: (d) {
            final p = d.localPosition;
            _dragStop = null;
            _dragInfl = null;
            for (var i = 0; i < stops.length; i++) {
              if ((Offset(bar.left + stops[i].$1 * bar.width, bar.bottom + 4) - p).distance < 14) {
                _dragStop = i;
                _sel = i;
                break;
              }
            }
            if (_dragStop == null) {
              for (var i = 0; i + 1 < stops.length; i++) {
                final a = stops[i].$1, b = stops[i + 1].$1;
                if ((Offset(bar.left + (a + (b - a) * stops[i].$3) * bar.width, bar.top - 4) - p).distance < 12) _dragInfl = i;
              }
            }
            setState(() => _work = [...stops]);
          },
          onPanUpdate: (d) {
            final w = _work;
            if (w == null) return;
            final p = d.localPosition;
            if (_dragStop != null) {
              final i = _dragStop!;
              final lo = i > 0 ? w[i - 1].$1 : 0.0, hi = i + 1 < w.length ? w[i + 1].$1 : 1.0;
              w[i] = (frac(p.dx).clamp(lo, hi), w[i].$2, w[i].$3);
              // pulled well away from the bar: it goes (two stops always stay)
              if (p.dy > bar.bottom + 40 && w.length > 2) {
                w.removeAt(i);
                _dragStop = null;
                _sel = _sel.clamp(0, w.length - 1);
              }
            } else if (_dragInfl != null) {
              final i = _dragInfl!;
              final a = w[i].$1, b = w[i + 1].$1;
              final f = frac(p.dx).clamp(a, b);
              w[i] = (w[i].$1, w[i].$2, ((f - a) / math.max(1e-4, b - a)).clamp(0.01, 0.99));
            }
            setState(() {});
          },
          onPanEnd: (_) {
            final w = _work;
            if (w != null) _commit(w);
          },
          child: SizedBox(height: 44, child: CustomPaint(painter: _GradientPainter(stops, bar, _sel, look, _at))),
        );
      }),
      DRow('Color', DColorWell(color: stops.isEmpty ? Colors.white : stops[_sel].$2, onChanged: (c) {
        final s = [...stops];
        s[_sel] = (s[_sel].$1, c, s[_sel].$3);
        _commit(s);
      })),
      Row(children: [
        DButton(fill: radial ? null : look.accent, onTap: radial ? () => _commit(stops, radial: false) : null, child: Text('Linear', style: Look.font(15, radial ? look.text : Colors.white))),
        const SizedBox(width: 6),
        DButton(fill: radial ? look.accent : null, onTap: radial ? null : () => _commit(stops, radial: true), child: Text('Radial', style: Look.font(15, radial ? Colors.white : look.text))),
        const SizedBox(width: 6),
        DButton(
          onTap: () {
            final n = stops.length;
            _commit([for (var i = 0; i < n; i++) (1 - stops[n - 1 - i].$1, stops[n - 1 - i].$2, n - i - 2 >= 0 ? 1 - stops[n - i - 2].$3 : 0.5)]);
          },
          child: const Text('⇄'),
        ),
      ]),
      if (!radial) DRow('Angle', DStepper(value: angle, min: 0, max: 360, suffix: '°', onChanged: (a) => _commit(stops, angle: a))),
    ]);
  }
}

class _GradientPainter extends CustomPainter {
  _GradientPainter(this.stops, this.bar, this.sel, this.look, this.at);
  final List<(double, Color, double)> stops;
  final Rect bar;
  final int sel;
  final Look look;
  final Color Function(List<(double, Color, double)>, double) at;

  @override
  void paint(Canvas c, Size size) {
    const n = 64;
    for (var i = 0; i < n; i++) {
      final t0 = i / n, t1 = (i + 1) / n;
      c.drawRect(Rect.fromLTRB(bar.left + t0 * bar.width, bar.top, bar.left + t1 * bar.width + 0.5, bar.bottom), Paint()..color = at(stops, (t0 + t1) / 2));
    }
    c.drawRRect(RRect.fromRectAndRadius(bar.deflate(0.5), const Radius.circular(3)), Paint()
      ..style = PaintingStyle.stroke
      ..strokeWidth = 1
      ..color = look.gray(0x80));
    for (var i = 0; i + 1 < stops.length; i++) {
      final a = stops[i].$1, b = stops[i + 1].$1;
      final x = bar.left + (a + (b - a) * stops[i].$3) * bar.width;
      final ctr = Offset(x, bar.top - 4);
      final path = Path()..addPolygon([ctr + const Offset(0, -4), ctr + const Offset(4, 0), ctr + const Offset(0, 4), ctr + const Offset(-4, 0)], true);
      c.drawPath(path, Paint()..color = look.gray(0x90));
      c.drawPath(path, Paint()
        ..style = PaintingStyle.stroke
        ..color = look.gray(0x50));
    }
    for (var i = 0; i < stops.length; i++) {
      final r = Rect.fromCenter(center: Offset(bar.left + stops[i].$1 * bar.width, bar.bottom + 4), width: 12, height: 14);
      c.drawRRect(RRect.fromRectAndRadius(r, const Radius.circular(2)), Paint()..color = stops[i].$2);
      c.drawRRect(RRect.fromRectAndRadius(r.inflate(i == sel ? 1 : 0.5), const Radius.circular(2)), Paint()
        ..style = PaintingStyle.stroke
        ..strokeWidth = i == sel ? 2 : 1
        ..color = i == sel ? look.accent : look.gray(0x60));
    }
  }

  @override
  bool shouldRepaint(_GradientPainter o) => true;
}

/// objinsp stroke_editor: No Border / Line, then the dash, colour and width.
class StrokeEditor extends StatelessWidget {
  const StrokeEditor({super.key, required this.stroke, required this.onChanged, this.allowNone = true});
  final Map<String, dynamic>? stroke;
  final bool allowNone;
  final void Function(Map<String, Object?>? stroke) onChanged;

  @override
  Widget build(BuildContext context) {
    final s = stroke ?? (allowNone ? null : {'color': [0.0, 0.0, 0.0, 1.0], 'width': 1.0, 'dash': 'Solid'});
    final kind = s == null ? 0 : 1;
    return EguiColumn(children: [
      if (allowNone)
        DCombo<int>(text: const ['No Border', 'Line'][kind], items: () => [for (final (i, n) in const ['No Border', 'Line'].indexed) DChoice(n, i, on: i == kind)], onPick: (k) {
          if (k != kind) onChanged(k == 1 ? {'color': [0.0, 0.0, 0.0, 1.0], 'width': 1.0, 'dash': 'Solid'} : null);
        }),
      if (s != null) ...[
        DRow('Style', DCombo<String>(
          width: 120,
          text: s['dash'] as String? ?? 'Solid',
          items: () => [for (final d in ['Solid', 'Dashed', 'Dotted']) DChoice(d, d, on: d == s['dash'])],
          onPick: (d) => onChanged({...s, 'dash': d}),
        )),
        DRow('Color', DColorWell(color: colorOf(s['color'], Colors.black), onChanged: (c) => onChanged({...s, 'color': colorTo(c)}))),
        DRow('Width', DStepper(value: (s['width'] as num?)?.toDouble() ?? 1, step: 0.5, min: 0, max: 100, suffix: ' pt', decimals: 1, onChanged: (w) => onChanged({...s, 'width': w}))),
      ],
    ]);
  }
}
