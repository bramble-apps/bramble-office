// The inspector at the window's right (kn-app inspector.rs): Format shows what's selected (an
// object's Style, Text, Arrange and Image tabs, or the slide's layout and background with
// nothing selected), Animate its builds (or the slide's transition), Document the slideshow's
// settings and the slide size. The controls are the suite's touch shapes, grouped in rounded
// cards as Keynote on iPad (TouchScope), their values from the engine (kd_inspector) and each
// change sent back as the control's key (kd_inspector_set).
import 'dart:ui' as ui;

import 'package:flutter/material.dart';

import '../engine.dart';
import 'fill.dart';
import 'icons.dart';
import 'look.dart';
import 'scale.dart';
import 'widgets.dart';

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
    final body = (s['body'] as Map<String, dynamic>?) ?? {};
    final name = tabs.isEmpty ? '' : tabs[tab.clamp(0, tabs.length - 1)];
    return Column(crossAxisAlignment: CrossAxisAlignment.stretch, children: [
      DSegmented(labels: tabs, cur: tab, radius: 12, onPick: (i) => ins.set('tab', i)),
      const SizedBox(height: 14),
      switch (name) {
        'Text' => _TextPane(s: body, ins: ins),
        'Arrange' => _ArrangePane(s: body, ins: ins),
        'Image' => _ImagePane(s: body, ins: ins),
        _ => _StylePane(s: body, ins: ins),
      },
    ]);
  }
}

class _StylePane extends StatelessWidget {
  const _StylePane({required this.s, required this.ins});
  final Map<String, dynamic> s;
  final InspectorState ins;
  @override
  Widget build(BuildContext context) {
    final fill = s['fill'] as Map<String, dynamic>?;
    return _cards([
      if (fill != null) _titled('Fill', DCard(children: [FillEditor(fill: fill, onChanged: (f) => ins.set('style.fill', f))])),
      _titled('Border', DCard(children: [StrokeEditor(stroke: s['stroke'] as Map<String, dynamic>?, onChanged: (v) => ins.set('style.stroke', v))])),
      DCard(children: [
        DRow('Shadow', DToggle(on: s['shadow'] == true, onChanged: (v) => ins.set('style.shadow', v))),
        DRow('Opacity', DStepper(value: _n(s['opacity'], 100), min: 0, max: 100, step: 5, suffix: '%', onChanged: (v) => ins.set('style.opacity', v))),
      ]),
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

class _ArrangePane extends StatelessWidget {
  const _ArrangePane({required this.s, required this.ins});
  final Map<String, dynamic> s;
  final InspectorState ins;

  @override
  Widget build(BuildContext context) {
    final look = LookScope.of(context);
    final one = (s['count'] as num?)?.toInt() == 1;
    Widget cmd(String label, String command, [DIcon? icon]) => Expanded(
          child: DButton(
            height: 40,
            onTap: () => ins.set('arrange.command', command),
            child: Row(mainAxisAlignment: MainAxisAlignment.center, children: [
              if (icon != null) ...[DesktopIcon(icon, size: 18, color: look.text), const SizedBox(width: 6)],
              Flexible(child: Text(label, maxLines: 1, overflow: TextOverflow.ellipsis, style: Look.font(13, look.text))),
            ]),
          ),
        );
    Widget pair(Widget a, Widget b) => Row(children: [a, const SizedBox(width: 8), b]);
    final locked = s['locked'] == true;
    return _cards([
      _titled('Layer', DCard(children: [
        pair(cmd('Back', 'Send to Back', DIcon.layerBack), cmd('Front', 'Bring to Front', DIcon.layerFront)),
        pair(cmd('Backward', 'Send Backward', DIcon.layerBackward), cmd('Forward', 'Bring Forward', DIcon.layerForward)),
      ])),
      if (one)
        _titled('Size and Position', DCard(children: [
          DRow('Width', DStepper(value: _n(s['w']), min: 1, suffix: ' pt', onChanged: (v) => ins.set('arrange.w', v))),
          DRow('Height', DStepper(value: _n(s['h']), min: 1, suffix: ' pt', onChanged: (v) => ins.set('arrange.h', v))),
          DRow('X', DStepper(value: _n(s['x']), suffix: ' pt', onChanged: (v) => ins.set('arrange.x', v))),
          DRow('Y', DStepper(value: _n(s['y']), suffix: ' pt', onChanged: (v) => ins.set('arrange.y', v))),
          DRow('Rotate', DStepper(value: _n(s['angle']), min: -360, max: 360, suffix: '°', onChanged: (v) => ins.set('arrange.angle', v))),
        ])),
      _titled('Flip', DCard(children: [
        pair(cmd('Horizontally', 'Flip Horizontally', DIcon.flipH), cmd('Vertically', 'Flip Vertically', DIcon.flipV)),
      ])),
      DCard(children: [
        pair(cmd(locked ? 'Unlock' : 'Lock', locked ? 'Unlock' : 'Lock', locked ? DIcon.unlock : DIcon.lock), one ? cmd('Ungroup', 'Ungroup', DIcon.ungroup) : cmd('Group', 'Group', DIcon.group)),
      ]),
    ]);
  }
}

class _ImagePane extends StatelessWidget {
  const _ImagePane({required this.s, required this.ins});
  final Map<String, dynamic> s;
  final InspectorState ins;
  @override
  Widget build(BuildContext context) {
    final look = LookScope.of(context);
    Widget cmd(String label, String command) => DButton(height: 40, onTap: () => ins.set('arrange.command', command), child: Text(label, style: Look.font(14, look.text)));
    return _cards([
      DCard(children: [
        cmd(s['masked'] == true ? 'Edit Mask' : 'Mask', 'Edit Mask'),
        cmd('Remove Background', 'Remove Background'),
      ]),
      if (s['description'] != null) _titled('Description', DCard(children: [DNote(s['description'] as String)])),
    ]);
  }
}

// ------------------------------------------------------------------ Format: the slide

class _SlidePane extends StatelessWidget {
  const _SlidePane({required this.s, required this.ins});
  final Map<String, dynamic> s;
  final InspectorState ins;
  @override
  Widget build(BuildContext context) {
    final look = LookScope.of(context);
    final layouts = [for (final l in (s['layouts'] as List? ?? const [])) l as String];
    final layout = s['layout'] as String? ?? '';
    return _cards([
      Text('Slide Layout', textAlign: TextAlign.center, style: Look.font(17, look.text).copyWith(fontWeight: FontWeight.w700)),
      DCard(children: [
        DCombo<int>(text: layout.isEmpty ? 'Layout' : layout, items: () => [for (final (i, l) in layouts.indexed) DChoice(l, i, on: l == layout)], onPick: (i) => ins.set('slide.layout', i)),
        DButton(height: 40, onTap: () => ins.set('slide.command', 'Reapply Layout to Slide'), child: Text('Reapply Layout', style: Look.font(14, look.text))),
      ]),
      _titled('Background', DCard(children: [FillEditor(fill: (s['background'] as Map<String, dynamic>?) ?? {'kind': 0}, onChanged: (f) => ins.set('slide.background', f))])),
      DCard(children: [
        DRow('Skip Slide', DToggle(on: s['skipped'] == true, onChanged: (v) => ins.set('slide.command', v ? 'Skip Slide' : "Don't Skip Slide"))),
      ]),
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
        if (effect != null) DRow('Duration', DStepper(value: _n(s['duration'], 1), min: 0.1, max: 60, step: 0.1, decimals: 2, suffix: ' s', onChanged: (v) => ins.set('animate.tr.duration', v))),
      ]),
      _titled('Start Transition', DCard(children: [
        DSegmented(labels: const ['On Click', 'Automatically'], cur: auto ? 1 : 0, onPick: (i) => ins.set('animate.tr.automatic', i == 1)),
        DRow('Delay', DStepper(value: _n(s['delay']), min: 0, max: 60, step: 0.1, decimals: 2, suffix: ' s', onChanged: (v) => ins.set('animate.tr.delay', v)), enabled: auto),
      ])),
    ]);
  }
}

class _BuildPane extends StatelessWidget {
  const _BuildPane({required this.s, required this.ins});
  final Map<String, dynamic> s;
  final InspectorState ins;
  @override
  Widget build(BuildContext context) {
    final look = LookScope.of(context);
    final tabs = [for (final t in (s['tabs'] as List? ?? const [])) t as String];
    final tab = (s['tab'] as num?)?.toInt() ?? 0;
    final effect = s['effect'] as String?;
    final sections = [for (final x in (s['sections'] as List? ?? const [])) x as Map<String, dynamic>];
    final order = s['order'] as num?;
    return Column(crossAxisAlignment: CrossAxisAlignment.stretch, children: [
      DSegmented(labels: tabs, cur: tab, radius: 12, onPick: (i) => ins.set('animate.tab', i)),
      const SizedBox(height: 14),
      _cards([
        if (effect == null) Text(tab == 1 ? 'No Action' : 'No Build', textAlign: TextAlign.center, style: Look.font(17, look.textDim)),
        DCard(children: [
          DCombo<String>(
            text: effect ?? 'Add an Effect',
            maxHeight: 520,
            items: () => [
              DChoice<String>('None', '', on: effect == null),
              for (final sec in sections)
                for (final n in (sec['names'] as List)) DChoice<String>(n as String, n, on: n == effect),
            ],
            onPick: (f) => ins.set('animate.effect', f.isEmpty ? null : f),
          ),
          if (effect != null) DRow('Duration', DStepper(value: _n(s['duration'], 1), min: 0.1, max: 60, step: 0.1, decimals: 2, suffix: ' s', onChanged: (v) => ins.set('animate.duration', v))),
        ]),
        if (order != null) DNote('Build ${order.toInt() + 1} of ${s['steps']}', align: TextAlign.center),
      ]),
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
    final mode = (s['mode'] as num?)?.toInt() ?? 0;
    final wide = s['wide'] == true;
    const modes = ['Normal', 'Self-Playing', 'Links Only'];
    return _cards([
      _titled('Slideshow Settings', DCard(children: [
        DRow('Play upon open', DToggle(on: s['autoplay'] == true, onChanged: (v) => ins.set('doc.autoplay', v))),
        DRow('Loop slideshow', DToggle(on: s['loop'] == true, onChanged: (v) => ins.set('doc.loop', v))),
      ])),
      _titled('Presentation Type', DCard(children: [
        DCombo<int>(text: modes[mode.clamp(0, 2)], items: () => [for (final (i, n) in modes.indexed) DChoice(n, i, on: i == mode)], onPick: (i) => ins.set('doc.mode', i)),
        if (mode == 1) ...[
          DRow('Transitions', DStepper(value: _n(s['transitionDelay']), min: 0, max: 600, step: 0.5, decimals: 1, suffix: ' s', onChanged: (v) => ins.set('doc.transitionDelay', v))),
          DRow('Builds', DStepper(value: _n(s['buildDelay']), min: 0, max: 600, step: 0.5, decimals: 1, suffix: ' s', onChanged: (v) => ins.set('doc.buildDelay', v))),
        ],
      ])),
      _titled('Slide Size', DCard(children: [
        DCombo<String>(text: wide ? 'Widescreen (16:9)' : 'Standard (4:3)', items: () => [DChoice('Standard (4:3)', '4:3', on: !wide), DChoice('Widescreen (16:9)', '16:9', on: wide)], onPick: (v) => ins.set('doc.size', v)),
      ])),
    ]);
  }
}
