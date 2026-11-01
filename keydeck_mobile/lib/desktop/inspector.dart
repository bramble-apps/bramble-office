// The inspector at the window's right (kn-app inspector.rs): Format shows what's selected (an
// object's Style, Text, Arrange and Image tabs, or the slide's layout and background with
// nothing selected), Animate its builds (or the slide's transition), Document the slideshow's
// settings and the slide size. The controls are the suite's touch shapes, grouped in rounded
// cards as Keynote on iPad (TouchScope), their values from the engine (kd_inspector) and each
// change sent back as the control's key (kd_inspector_set).
import 'dart:math' as math;
import 'dart:ui' as ui;

import 'package:file_picker/file_picker.dart';

import 'package:flutter/material.dart';

import '../engine.dart';
import 'fill.dart';
import 'icons.dart';
import 'layout_grid.dart';
import 'look.dart';
import 'scale.dart';
import 'widgets.dart';

part 'obj_panes.dart';

class Inspector extends StatefulWidget {
  const Inspector({super.key, required this.engine, required this.look, required this.topInset, required this.revision, required this.onChanged, this.sheet = false});
  final Engine engine;

  /// In the phone layout's sheet (it draws the frame).
  final bool sheet;
  final Look look;
  final double topInset;

  /// Bumped when the presentation or the selection changed (the values are read again).
  final int revision;
  final VoidCallback onChanged;

  @override
  State<Inspector> createState() => InspectorState();
}

class InspectorState extends State<Inspector> {
  Engine get engine => widget.engine;
  Map<String, dynamic> _s = {};

  @override
  void initState() {
    super.initState();
    _s = widget.engine.inspector();
  }

  @override
  void didUpdateWidget(Inspector oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (oldWidget.revision != widget.revision) _s = widget.engine.inspector();
  }

  void set(String key, Object? value) {
    widget.engine.inspectorSet(key, value);
    setState(() => _s = widget.engine.inspector());
    widget.onChanged();
  }

  /// Disclosure sections' open state (open at first).
  final Map<String, bool> _open = {};
  bool isOpen(String id) => _open[id] ?? true;
  void toggle(String id) => setState(() => _open[id] = !isOpen(id));

  @override
  Widget build(BuildContext context) {
    final look = widget.look;
    final pane = _s['pane'] as String? ?? 'none';
    Widget body = switch (pane) {
      'object' => _ObjectPane(s: _s, ins: this),
      'slide' => _SlidePane(s: _s, ins: this),
      'build' => _BuildPane(s: _s, ins: this),
      'transition' => _TransitionPane(s: _s, ins: this),
      'document' => _DocumentPane(s: _s, ins: this),
      _ => const _Nothing(),
    };
    body = SingleChildScrollView(padding: const EdgeInsets.only(bottom: 24), child: body);
    // (the touch-sized cards drawn at four fifths, in proportion with the rest of the window)
    body = ScaledBox(k: 0.8, child: TouchScope(child: body));
    if (widget.sheet) return LookScope(look: look, child: body);
    final frost = look.light ? 0.86 : 0.64;
    final bg = touchBackground(look);
    final panel = Container(
      width: 300,
      decoration: BoxDecoration(color: look.translucent ? bg.withValues(alpha: frost) : bg, border: Border(left: BorderSide(color: look.separator))),
      padding: EdgeInsets.fromLTRB(12, 12 + widget.topInset, 12, 0),
      child: body,
    );
    return LookScope(
      look: look,
      child: look.translucent ? ClipRect(child: BackdropFilter(filter: ui.ImageFilter.blur(sigmaX: 20, sigmaY: 20), child: panel)) : panel,
    );
  }
}

double _n(dynamic v, [double d = 0]) => (v as num?)?.toDouble() ?? d;

/// Cards with space between them.
Widget _cards(List<Widget> kids) => Column(crossAxisAlignment: CrossAxisAlignment.stretch, children: [for (final (i, k) in kids.indexed) ...[if (i > 0) const SizedBox(height: 14), k]]);

Widget _titled(String title, Widget card) => Column(crossAxisAlignment: CrossAxisAlignment.stretch, children: [DSectionTitle(title), const SizedBox(height: 6), card]);

class _Nothing extends StatelessWidget {
  const _Nothing();
  @override
  Widget build(BuildContext context) {
    final look = LookScope.of(context);
    return Column(children: [
      const SizedBox(height: 60),
      DesktopIcon(DIcon.format, size: 34, color: look.textDim),
      const SizedBox(height: 22),
      Text('No presentation open.', style: Look.font(15, look.textDim)),
    ]);
  }
}

// ------------------------------------------------------------------ Format: objects

class _ObjectPane extends StatelessWidget {
  const _ObjectPane({required this.s, required this.ins});
  final Map<String, dynamic> s;
  final InspectorState ins;

  @override
  Widget build(BuildContext context) {
    final tabs = [for (final t in (s['tabs'] as List? ?? const [])) t as String];
    final tab = (s['tab'] as num?)?.toInt() ?? 0;
    final body = (s['data'] as Map<String, dynamic>?) ?? {};
    final name = tabs.isEmpty ? '' : tabs[tab.clamp(0, tabs.length - 1)];
    return Column(crossAxisAlignment: CrossAxisAlignment.stretch, children: [
      DSegmented(labels: tabs, cur: tab, radius: 12, onPick: (i) => ins.set('tab', i)),
      const SizedBox(height: 14),
      switch (name) {
        'Text' => _TextPane(s: body, ins: ins),
        'Arrange' => ArrangePane(s: body, ins: ins),
        'Image' => ImagePane(s: body, ins: ins),
        'Chart' => ChartPane(s: body, ins: ins),
        'Axis' => AxisPane(s: body, ins: ins),
        'Series' => SeriesPane(s: body, ins: ins),
        'Table' => TablePane(s: body, ins: ins),
        'Cell' => CellPane(s: body, ins: ins),
        'Movie' || 'Audio' => MoviePane(s: body, ins: ins),
        'Gallery' => GalleryPane(s: body, ins: ins),
        'Web Video' => WebVideoPane(s: body, ins: ins),
        _ => StylePane(s: body, ins: ins),
      },
    ]);
  }
}

class _TextPane extends StatelessWidget {
  const _TextPane({required this.s, required this.ins});
  final Map<String, dynamic> s;
  final InspectorState ins;

  @override
  Widget build(BuildContext context) {
    final look = LookScope.of(context);
    final fonts = [for (final f in (s['fonts'] as List? ?? const [])) f as String];
    final family = s['family'] as String? ?? '';
    Widget toggle(String k, String label, TextStyle style) {
      final on = s[k] == true;
      return Expanded(
        child: GestureDetector(
          onTap: () => ins.set('text.$k', !on),
          child: Container(
            height: 40,
            margin: const EdgeInsets.symmetric(horizontal: 3),
            decoration: BoxDecoration(color: on ? look.accent : touchFill(look), borderRadius: BorderRadius.circular(10)),
            alignment: Alignment.center,
            child: Text(label, style: style.copyWith(color: on ? const Color(0xFFFFFFFF) : look.text)),
          ),
        ),
      );
    }

    return _cards([
      _titled('Font', DCard(children: [
        // the font: typed or picked (the list narrows to what's typed)
        DCombo<String>(
          text: family.isEmpty ? 'Font' : family,
          search: true,
          typed: (t) => t,
          listWidth: 260,
          items: () => [for (final f in fonts) DChoice(f, f, on: f == family)],
          onPick: (f) => ins.set('text.family', f),
        ),
        DRow('Size', DStepper(value: _n(s['size'], 12), min: 1, max: 1638, suffix: ' pt', onChanged: (v) => ins.set('text.size', v))),
        Row(children: [
          toggle('bold', 'B', Look.font(16, look.text).copyWith(fontWeight: FontWeight.w700)),
          toggle('italic', 'I', Look.font(16, look.text).copyWith(fontStyle: FontStyle.italic)),
          toggle('underline', 'U', Look.font(16, look.text).copyWith(decoration: TextDecoration.underline)),
          toggle('strike', 'S', Look.font(16, look.text).copyWith(decoration: TextDecoration.lineThrough)),
        ]),
        DRow('Color', DColorWell(color: colorOf(s['color'], Colors.black), onChanged: (c) => ins.set('text.color', colorTo(c)))),
      ])),
      _titled('Alignment', DCard(children: [
        DIconSegments(n: 4, selected: (s['align'] as num?)?.toInt(), onPick: (i) => ins.set('text.align', i), draw: _alignIcon),
        DIconSegments(n: 3, selected: (s['valign'] as num?)?.toInt(), onPick: (i) => ins.set('text.valign', i), draw: _valignIcon),
        DRow('Line Spacing', DStepper(value: _n(s['spacing'], 1), min: 0.5, max: 5, step: 0.1, decimals: 1, onChanged: (v) => ins.set('text.spacing', v))),
      ])),
    ]);
  }
}

/// Lines drawn left, centred, right or justified.
void _alignIcon(Canvas c, Rect r, int i, Color col) {
  final p = Paint()
    ..color = col
    ..strokeWidth = 1.6
    ..strokeCap = StrokeCap.round;
  final w = r.width * 0.5, x0 = r.center.dx - w / 2;
  for (var k = 0; k < 4; k++) {
    final y = r.center.dy - 6 + k * 4;
    final lw = i == 3 || k.isEven ? w : w * 0.6;
    final x = switch (i) {
      1 => r.center.dx - lw / 2,
      2 => x0 + w - lw,
      _ => x0,
    };
    c.drawLine(Offset(x, y), Offset(x + lw, y), p);
  }
}

/// A box with its lines at the top, the middle or the bottom.
void _valignIcon(Canvas c, Rect r, int i, Color col) {
  final p = Paint()
    ..color = col
    ..strokeWidth = 1
    ..strokeCap = StrokeCap.round;
  final box = Rect.fromCenter(center: r.center, width: 18, height: 16);
  c.drawLine(box.topLeft, box.topRight, p);
  c.drawLine(box.bottomLeft, box.bottomRight, p);
  final y0 = switch (i) {
    0 => box.top + 4,
    1 => box.center.dy - 2,
    _ => box.bottom - 8,
  };
  p.strokeWidth = 1.6;
  c.drawLine(Offset(box.left + 3, y0), Offset(box.right - 3, y0), p);
  c.drawLine(Offset(box.left + 5, y0 + 4), Offset(box.right - 5, y0 + 4), p);
}

// ------------------------------------------------------------------ Format: the slide

/// A picture the engine draws (a layout, a dynamic background preset), kept once made.
class _EnginePic extends StatefulWidget {
  const _EnginePic(this.cacheKey, this.make, {required this.width, required this.height});
  final String cacheKey;
  final Future<ui.Image?> Function() make;
  final double width, height;
  static final _cache = <String, Future<ui.Image?>>{};
  @override
  State<_EnginePic> createState() => _EnginePicState();
}

class _EnginePicState extends State<_EnginePic> {
  @override
  Widget build(BuildContext context) => FutureBuilder<ui.Image?>(
        future: _EnginePic._cache.putIfAbsent(widget.cacheKey, widget.make),
        builder: (ctx, snap) => SizedBox(
          width: widget.width,
          height: widget.height,
          child: snap.data == null ? ColoredBox(color: LookScope.of(context).gray(0x50)) : RawImage(image: snap.data, fit: BoxFit.cover),
        ),
      );
}

/// Format with nothing selected: the slide's layout (a picture and its name; tap for the
/// layouts), Appearance, Background (Standard: a fill; Dynamic: the theme's moving backgrounds,
/// Preview Motion, their colours and options), Skip Slide, Edit Slide Layout.
class _SlidePane extends StatefulWidget {
  const _SlidePane({required this.s, required this.ins});
  final Map<String, dynamic> s;
  final InspectorState ins;
  @override
  State<_SlidePane> createState() => _SlidePaneState();
}

class _SlidePaneState extends State<_SlidePane> {
  int _page = 0;

  @override
  Widget build(BuildContext context) {
    final s = widget.s, ins = widget.ins;
    final look = LookScope.of(context);
    final e = ins.engine;
    final layouts = [for (final l in (s['layouts'] as List? ?? const [])) l as String];
    final layout = s['layout'] as String? ?? '';
    final li = (s['layoutIndex'] as num?)?.toInt();
    // (the pictures are of this presentation's theme)
    final theme = e.status()['path'] ?? e.status()['title'] ?? '';
    Widget layoutPic(int i, double w) => _EnginePic('layout:$theme:$i:$w', () => e.renderLayout(i, (w * 2).round(), (w * 9 / 16 * 2).round()), width: w, height: w * 9 / 16);
    final tab = (s['bgTab'] as num?)?.toInt() ?? 0;
    final dyn = s['dynamic'] as Map<String, dynamic>?;
    final presets = [for (final p in (s['presets'] as List? ?? const [])) p as String];
    final pages = (presets.length / 6).ceil().clamp(1, 99);
    final cur = (dyn?['preset'] as num?)?.toInt();
    if (_page >= pages) _page = pages - 1;
    return _cards([
      Text('Slide', textAlign: TextAlign.center, style: Look.font(17, look.text).copyWith(fontWeight: FontWeight.w700)),
      DCard(children: [
        GestureDetector(
          behavior: HitTestBehavior.opaque,
          onTapUp: (d) async {
            final box = context.findRenderObject() as RenderBox?;
            final at = box == null ? Rect.fromCenter(center: d.globalPosition, width: 1, height: 1) : box.localToGlobal(Offset.zero) & box.size;
            final size = e.geometry()['size'] as List?;
            final aspect = size == null ? 9 / 16 : (size[1] as num) / (size[0] as num);
            final pick = await showLayoutGrid(context, at, e, names: layouts, aspect: aspect.toDouble(), current: li, title: 'Slide Layout');
            if (pick != null) ins.set('slide.layout', pick);
          },
          child: Row(children: [
            if (li != null) ClipRRect(borderRadius: BorderRadius.circular(3), child: layoutPic(li, 96)),
            const SizedBox(width: 10),
            Expanded(
              child: Column(crossAxisAlignment: CrossAxisAlignment.start, children: [
                Text('Slide Layout', style: Look.font(13, look.textDim)),
                Text(layout, style: Look.font(15, look.text, bold: true), maxLines: 2, overflow: TextOverflow.ellipsis),
              ]),
            ),
            Icon(Icons.chevron_right, color: look.textDim),
          ]),
        ),
        DButton(height: 40, onTap: () => ins.set('slide.command', 'Reapply Layout to Slide'), child: Text('Reapply Layout', style: Look.font(14, look.text))),
      ]),
      _titled('Appearance', DCard(children: [
        for (final a in (s['appearance'] as List? ?? const []).cast<Map<String, dynamic>>())
          DRow(a['label'] as String, DToggle(on: a['on'] == true, onChanged: a['enabled'] == true ? (v) => ins.set('slide.appearance', {'field': a['field'], 'on': v}) : (_) {})),
      ])),
      _titled('Background', DCard(children: [
        DSegmented(labels: const ['Standard', 'Dynamic'], cur: tab, onPick: (i) => ins.set('slide.bgTab', i)),
        if (tab == 0) FillEditor(fill: (s['background'] as Map<String, dynamic>?) ?? {'kind': 0}, onChanged: (f) => ins.set('slide.background', f)),
        if (tab == 1 && presets.isEmpty) Text('This theme has no dynamic backgrounds.', style: Look.font(14, look.textDim)),
        if (tab == 1 && presets.isNotEmpty) ...[
          Row(children: [
            GestureDetector(onTap: _page > 0 ? () => setState(() => _page--) : null, child: Icon(Icons.chevron_left, color: _page > 0 ? look.text : look.textDim.withValues(alpha: 0.4))),
            Expanded(
              child: Wrap(alignment: WrapAlignment.center, spacing: 8, runSpacing: 8, children: [
                for (var i = _page * 6; i < presets.length && i < _page * 6 + 6; i++)
                  GestureDetector(
                    onTap: () => ins.set('slide.preset', i),
                    child: Container(
                      decoration: BoxDecoration(borderRadius: BorderRadius.circular(5), border: Border.all(color: i == cur ? look.accent : look.gray(0x44), width: i == cur ? 2 : 1)),
                      child: ClipRRect(borderRadius: BorderRadius.circular(4), child: _EnginePic('motion:$theme:$i', () => e.motionPreset(i, 128, 72), width: 64, height: 36)),
                    ),
                  ),
              ]),
            ),
            GestureDetector(onTap: _page + 1 < pages ? () => setState(() => _page++) : null, child: Icon(Icons.chevron_right, color: _page + 1 < pages ? look.text : look.textDim.withValues(alpha: 0.4))),
          ]),
          if (dyn == null) Text('Choose a dynamic background.', style: Look.font(14, look.textDim)),
          if (dyn != null) ...[
            DRow('Preview Motion', DToggle(on: s['previewMotion'] == true, onChanged: (v) => ins.set('slide.previewMotion', v))),
            Text('Colors', style: Look.font(15, look.text)),
            Wrap(spacing: 8, runSpacing: 6, children: [
                for (final (k, st) in (dyn['colors'] as List).indexed)
                  DColorWell(
                    width: 64,
                    color: colorOf((st as List)[1]),
                    onChanged: (c) {
                      final cs = [for (final x in dyn['colors'] as List) [(x as List)[0], x[1]]];
                      cs[k][1] = colorTo(c);
                      ins.set('slide.colors', cs);
                    },
                  ),
            ]),
            if (dyn['background'] != null) DRow('Background Color', DColorWell(color: colorOf(dyn['background']), onChanged: (c) => ins.set('slide.dynBackground', colorTo(c)))),
            for (final c in (dyn['controls'] as List).cast<Map<String, dynamic>>())
              DRow(
                c['label'] as String,
                DStepper(
                  value: _n(c['value']),
                  min: _n(c['min']),
                  max: _n(c['max']),
                  step: (c['decimals'] as num) == 0 ? 1 : 0.1,
                  decimals: (c['decimals'] as num).toInt(),
                  suffix: c['suffix'] as String,
                  onChanged: (v) => ins.set('slide.num', {'prop': c['prop'], 'value': v}),
                ),
              ),
          ],
        ],
      ])),
      DCard(children: [
        DRow('Skip Slide', DToggle(on: s['skipped'] == true, onChanged: (v) => ins.set('slide.command', v ? 'Skip Slide' : "Don't Skip Slide"))),
      ]),
      DButton(height: 40, onTap: () => ins.set('slide.command', 'Edit Slide Layouts'), child: Text('Edit Slide Layout', style: Look.font(14, look.text))),
    ]);
  }
}

// ------------------------------------------------------------------ Animate

class _TransitionPane extends StatelessWidget {
  const _TransitionPane({required this.s, required this.ins});
  final Map<String, dynamic> s;
  final InspectorState ins;
  @override
  Widget build(BuildContext context) {
    final look = LookScope.of(context);
    final effects = [for (final f in (s['effects'] as List? ?? const [])) f as String];
    final effect = s['effect'] as String?;
    final auto = s['automatic'] == true;
    return _cards([
      Text('Transitions', textAlign: TextAlign.center, style: Look.font(13, look.textDim)),
      if (effect == null) Text('No Transition Effect', textAlign: TextAlign.center, style: Look.font(17, look.textDim)),
      DCard(children: [
        DCombo<String>(
          text: effect ?? 'Add an Effect',
          maxHeight: 520,
          items: () => [DChoice<String>('None', '', on: effect == null), for (final f in effects) DChoice<String>(f, f, on: f == effect)],
          onPick: (f) => ins.set('animate.tr.effect', f.isEmpty ? null : f),
        ),
        if (effect != null) DButton(height: 40, onTap: () => ins.set('animate.preview', null), child: Text('Preview', style: Look.font(14, look.text))),
        if (effect != null) DRow('Duration', DStepper(value: _n(s['duration'], 1), min: 0.1, max: 60, step: 0.1, decimals: 2, suffix: ' s', onChanged: (v) => ins.set('animate.tr.duration', v))),
      ]),
      _titled('Start Transition', DCard(children: [
        DSegmented(labels: const ['On Click', 'Automatically'], cur: auto ? 1 : 0, onPick: (i) => ins.set('animate.tr.automatic', i == 1)),
        DRow('Delay', DStepper(value: _n(s['delay']), min: 0, max: 60, step: 0.1, decimals: 2, suffix: ' s', onChanged: (v) => ins.set('animate.tr.delay', v)), enabled: auto),
      ])),
      DButton(height: 40, onTap: () => ins.set('animate.buildOrder', null), child: Text('Build Order', style: Look.font(14, look.text))),
    ]);
  }
}

/// Animate with an object selected (inspector.rs build_inspector): Build In / Action / Build
/// Out, each effect with its options (duration, direction, bounce, rotation, scale, opacity,
/// acceleration, text animation, delivery, order), Add Action, Build Order.
class _BuildPane extends StatelessWidget {
  const _BuildPane({required this.s, required this.ins});
  final Map<String, dynamic> s;
  final InspectorState ins;

  List<DChoice<String>> _effects(String? cur) => [
        DChoice<String>('None', '', on: cur == null),
        for (final sec in (s['sections'] as List? ?? const []).cast<Map<String, dynamic>>())
          for (final n in (sec['names'] as List)) DChoice<String>(n as String, n, on: n == cur),
      ];

  @override
  Widget build(BuildContext context) {
    final look = LookScope.of(context);
    final tabs = [for (final t in (s['tabs'] as List? ?? const [])) t as String];
    final tab = (s['tab'] as num?)?.toInt() ?? 0;
    final builds = (s['builds'] as List? ?? const []).cast<Map<String, dynamic>>();
    final steps = (s['steps'] as num?)?.toInt() ?? 0;
    Widget combo<T>(String text, List<(T, String)> opts, T cur, void Function(T) pick, {double? width = 170}) =>
        DCombo<T>(text: text, width: width, items: () => [for (final o in opts) DChoice<T>(o.$2, o.$1, on: o.$1 == cur)], onPick: pick);
    final cards = <Widget>[];
    if (builds.isEmpty) {
      cards.add(Text(const ['No Build In Effect', 'No Action Build Effect', 'No Build Out Effect'][tab.clamp(0, 2)], textAlign: TextAlign.center, style: Look.font(17, look.textDim)));
      cards.add(DCard(children: [DCombo<String>(text: 'Add an Effect', maxHeight: 520, items: () => _effects(null), onPick: (f) => f.isEmpty ? null : ins.set('animate.add', {'value': f}))]));
    }
    for (final b in builds) {
      final id = b['id'];
      void opt(String option, Object v) => ins.set('animate.option', {'id': id, 'option': option, 'value': v});
      final dir = b['direction'] as Map<String, dynamic>?;
      final rot = b['rotation'] as Map<String, dynamic>?;
      cards.add(DCard(children: [
        Text(b['name'] as String, style: Look.font(18, look.text, bold: true)),
        DCombo<String>(text: 'Change', maxHeight: 520, items: () => _effects(b['name'] as String), onPick: (f) => ins.set('animate.change', {'id': id, 'value': f.isEmpty ? null : f})),
        DButton(height: 36, onTap: () => ins.set('animate.preview', null), child: Text('Preview ▶', style: Look.font(14, look.text))),
      ]));
      final opts = <Widget>[
        DRow(dir == null ? 'Duration' : 'Duration & Direction', DStepper(value: _n(b['duration'], 1), min: 0.1, max: 60, step: 0.1, decimals: 2, suffix: ' s', onChanged: (v) => ins.set('animate.duration', {'id': id, 'value': v}))),
      ];
      if (dir != null) {
        final o = [for (final x in dir['options'] as List) ((x as List)[0] as num, x[1] as String)];
        final cur = dir['value'] as num;
        opts.add(combo<num>(o.firstWhere((x) => x.$1 == cur, orElse: () => o.first).$2, o, cur, (v) => opt('direction', v), width: null));
      }
      if (b['bounce'] != null) opts.add(DRow('Bounce', DToggle(on: b['bounce'] == true, onChanged: (v) => opt('bounce', v))));
      if (rot != null) {
        final angle = _n(rot['angle']), turns = _n(rot['turns']);
        opts.add(DSegmented(labels: const ['↻ Clockwise', '↺ Counterclockwise'], cur: rot['clockwise'] == true ? 0 : 1, onPick: (i) => opt('clockwise', i == 0)));
        opts.add(DRow('Angle', DStepper(value: angle, min: 0, max: 359, suffix: '°', onChanged: (v) => opt('angle', turns * 360 + v))));
        opts.add(DRow('Rotations', DStepper(value: turns, min: 0, max: 9, onChanged: (v) => opt('angle', v * 360 + angle))));
      }
      if (b['scale'] != null) opts.add(DRow('Scale', DStepper(value: _n(b['scale']), min: 1, max: 5000, suffix: ' %', onChanged: (v) => opt('scale', v))));
      if (b['opacity'] != null) opts.add(DRow('Opacity', DStepper(value: _n(b['opacity']), min: 0, max: 100, suffix: ' %', onChanged: (v) => opt('opacity', v))));
      if (b['acceleration'] != null) {
        const names = ['None', 'Ease In', 'Ease Out', 'Ease In & Out'];
        final cur = (b['acceleration'] as num).toInt();
        opts.add(DRow('Acceleration', combo<int>(names[cur], [for (final (i, n) in names.indexed) (i, n)], cur, (v) => opt('acceleration', v))));
      }
      cards.add(DCard(children: opts));
      final more = <Widget>[];
      if (b['textAnimation'] != null) {
        const names = ['By Object', 'By Word', 'By Character'];
        final cur = (b['textAnimation'] as num).toInt();
        more.add(DRow('Text Animation', combo<int>(names[cur], [for (final (i, n) in names.indexed) (i, n)], cur, (v) => ins.set('animate.textAnimation', {'id': id, 'value': v}))));
      }
      if (b['order'] != null) {
        final cur = (b['order'] as num).toInt();
        more.add(DRow('Order', combo<int>('${cur + 1}', [for (var k = 0; k < steps; k++) (k, '${k + 1}')], cur, (v) => ins.set('animate.order', {'id': id, 'value': v}))));
      }
      if (b['delivery'] != null) {
        const names = ['All at Once', 'By Paragraph', 'By Paragraph Group', 'By Highlighted Paragraph'];
        final cur = (b['delivery'] as num).toInt();
        more.add(DRow('Delivery', combo<int>(names[cur], [for (final (i, n) in names.indexed) (i, n)], cur, (v) => ins.set('animate.delivery', {'id': id, 'value': v}))));
      }
      if (more.isNotEmpty) cards.add(DCard(children: more));
    }
    if (s['action'] == true && builds.isNotEmpty) {
      cards.add(DCard(children: [DCombo<String>(text: 'Add Action', maxHeight: 520, items: () => _effects(null), onPick: (f) => f.isEmpty ? null : ins.set('animate.add', {'value': f}))]));
    }
    cards.add(DButton(height: 40, onTap: () => ins.set('animate.buildOrder', null), child: Text('Build Order', style: Look.font(14, look.text))));
    return Column(crossAxisAlignment: CrossAxisAlignment.stretch, children: [
      DSegmented(labels: tabs, cur: tab, radius: 12, onPick: (i) => ins.set('animate.tab', i)),
      const SizedBox(height: 14),
      _cards(cards),
    ]);
  }
}

// ------------------------------------------------------------------ Document

class _DocumentPane extends StatelessWidget {
  const _DocumentPane({required this.s, required this.ins});
  final Map<String, dynamic> s;
  final InspectorState ins;
  @override
  Widget build(BuildContext context) {
    final look = LookScope.of(context);
    final mode = (s['mode'] as num?)?.toInt() ?? 0;
    final tab = (s['tab'] as num?)?.toInt() ?? 0;
    final size = (s['size'] as List?) ?? const [1920, 1080];
    final w = _n(size[0], 1920), h = _n(size[1], 1080);
    const modes = ['Normal', 'Self-Playing', 'Links Only'];
    final idle = s['idle'] == true;
    final e = ins.engine;
    final key = '${e.status()['path'] ?? e.status()['title']}:${s['theme']}:$w';
    final tabs = DSegmented(labels: const ['Document', 'Audio'], cur: tab, radius: 12, onPick: (i) => ins.set('doc.tab', i));
    if (tab == 1) {
      return Column(crossAxisAlignment: CrossAxisAlignment.stretch, children: [
        tabs,
        const SizedBox(height: 14),
        _titled('Soundtrack', DCard(children: [Text('No soundtrack', style: Look.font(15, look.textDim))])),
      ]);
    }
    return Column(crossAxisAlignment: CrossAxisAlignment.stretch, children: [
      tabs,
      const SizedBox(height: 14),
      _cards([
        DCard(children: [
          Row(children: [
            ClipRRect(borderRadius: BorderRadius.circular(3), child: _EnginePic('first:$key', () => e.render(0, 240 / w, 0, 0, 240, (240 * h / w).round()), width: 120, height: 120 * h / w)),
            const SizedBox(width: 12),
            Expanded(
              child: Column(crossAxisAlignment: CrossAxisAlignment.start, children: [
                Text('Theme', style: Look.font(13, look.textDim)),
                Text(s['theme'] as String? ?? '', style: Look.font(15, look.text, bold: true)),
              ]),
            ),
          ]),
          DButton(height: 40, onTap: () => ins.set('doc.changeTheme', true), child: Text('Change Theme', style: Look.font(14, look.text))),
        ]),
        _titled('Slideshow Settings', DCard(children: [
          DRow('Automatically play upon open', DToggle(on: s['autoplay'] == true, onChanged: (v) => ins.set('doc.autoplay', v))),
          DRow('Loop slideshow', DToggle(on: s['loop'] == true, onChanged: (v) => ins.set('doc.loop', v))),
          DRow('Restart show if idle', DToggle(on: idle, onChanged: (v) => ins.set('doc.idle', v))),
          DRow('Idle for', DStepper(value: _n(s['idleMinutes'], 15), min: 1, max: 1440, suffix: ' m', onChanged: (v) => ins.set('doc.idleMinutes', v)), enabled: idle),
        ])),
        _titled('Presentation Type', DCard(children: [
          DCombo<int>(text: modes[mode.clamp(0, 2)], items: () => [for (final (i, n) in modes.indexed) DChoice(n, i, on: i == mode)], onPick: (i) => ins.set('doc.mode', i)),
          if (mode == 1) ...[
            DRow('Transitions', DStepper(value: _n(s['transitionDelay']), min: 0, max: 600, step: 0.5, decimals: 1, suffix: ' s', onChanged: (v) => ins.set('doc.transitionDelay', v))),
            DRow('Builds', DStepper(value: _n(s['buildDelay']), min: 0, max: 600, step: 0.5, decimals: 1, suffix: ' s', onChanged: (v) => ins.set('doc.buildDelay', v))),
          ],
        ])),
        _titled('Slide Size', DCard(children: [
          DCombo<String>(
            text: s['sizeName'] as String? ?? '',
            items: () => [DChoice('Standard (4:3)', '4:3'), DChoice('Wide (16:9)', '16:9'), DChoice('Custom Slide Size…', 'custom')],
            onPick: (v) => v == 'custom' ? null : ins.set('doc.size', v),
          ),
          DRow('Width', DStepper(value: w, min: 16, max: 10000, suffix: ' pt', onChanged: (v) => ins.set('doc.size', [v, h]))),
          DRow('Height', DStepper(value: h, min: 16, max: 10000, suffix: ' pt', onChanged: (v) => ins.set('doc.size', [w, v]))),
        ])),
        DCard(children: [
          DRow('Require password to open', DToggle(on: s['password'] == true, onChanged: (v) => ins.set('doc.password', v))),
          if (s['password'] == true) DButton(height: 40, onTap: () => ins.set('doc.password', true), child: Text('Change Password…', style: Look.font(14, look.text))),
        ]),
      ]),
    ]);
  }
}
