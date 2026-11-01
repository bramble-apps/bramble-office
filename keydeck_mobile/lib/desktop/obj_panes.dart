// The object panes Keydeck shares with Letters (letters_mobile inspector.dart): Style with the
// theme's presets, Image, Chart / Axis / Series, Table and Cell, Movie / Audio, Gallery and Web
// Video, Arrange; their values from the engine (kn-app objapi.rs, the same shapes as Letters').
part of 'inspector.dart';

/// egui's radio button: a 14-point circle, a dot inside when chosen; its label.
class DRadio extends StatelessWidget {
  const DRadio({super.key, required this.on, required this.label, required this.onTap});
  final bool on;
  final String label;
  final VoidCallback onTap;
  @override
  Widget build(BuildContext context) {
    final look = LookScope.of(context);
    // (touch: 44 tall, a 22-point circle filled with the accent when chosen)
    final touch = TouchScope.of(context);
    final d = touch ? 22.0 : 14.0;
    return GestureDetector(
      behavior: HitTestBehavior.opaque,
      onTap: onTap,
      child: SizedBox(
        height: touch ? 44 : 20,
        child: Row(mainAxisSize: MainAxisSize.min, children: [
          Container(
            width: d,
            height: d,
            decoration: BoxDecoration(color: touch ? (on ? look.accent : touchFill(look)) : look.control, shape: BoxShape.circle, border: touch ? null : Border.all(color: look.light ? look.controlStroke : look.gray(0x5a))),
            alignment: Alignment.center,
            child: on ? Container(width: touch ? 8 : 6, height: touch ? 8 : 6, decoration: BoxDecoration(color: touch ? Colors.white : look.text, shape: BoxShape.circle)) : null,
          ),
          SizedBox(width: touch ? 12 : 6),
          Flexible(child: Text(label, maxLines: 1, overflow: TextOverflow.ellipsis, style: Look.font(touch ? 16 : 13, look.text))),
        ]),
      ),
    );
  }
}

class StylePane extends StatelessWidget {
  const StylePane({super.key, required this.s, required this.ins});
  final Map<String, dynamic> s;
  final InspectorState ins;
  void put(String k, Object? v) => ins.set('style.$k', v);

  @override
  Widget build(BuildContext context) {
    final look = LookScope.of(context);
    final isLine = s['isLine'] == true;
    final shadow = s['shadow'] as Map<String, dynamic>?;
    final stroke = s['stroke'] as Map<String, dynamic>?;
    final presets = [for (final p in (s['presets'] as List? ?? [])) p as Map<String, dynamic>];
    return EguiColumn(children: [
      if (presets.isNotEmpty) ...[
        _Presets(presets: presets, kind: s['presetKind'] as String? ?? '', imageData: s['imageData'] as int?, ins: ins, onPick: (id) => put('preset', id)),
      ],
      if (s['fill'] != null) ...[
        DDisclosure(title: 'Fill', open: ins.isOpen('fill'), onToggle: () => ins.toggle('fill'), swatch: s['fillSwatch'] == null ? null : colorOf(s['fillSwatch']), swatchNone: s['fillSwatch'] == null),
        if (ins.isOpen('fill')) FillEditor(fill: s['fill'] as Map<String, dynamic>, onChanged: (f) => put('fill', f)),
      ],
      if (s['hasBorder'] == true) ...[
        DDisclosure(title: isLine ? 'Stroke' : 'Border', open: ins.isOpen('border'), onToggle: () => ins.toggle('border'), swatch: stroke == null ? null : colorOf(stroke['color']), swatchNone: stroke == null),
        if (ins.isOpen('border')) ...[
          StrokeEditor(stroke: stroke, allowNone: !isLine, onChanged: (v) => put('stroke', v)),
          if (isLine) ...[
            DCheckbox(on: s['tail'] == true, label: 'Start arrow', onChanged: (v) => put('ends', [v, s['head'] == true])),
            DCheckbox(on: s['head'] == true, label: 'End arrow', onChanged: (v) => put('ends', [s['tail'] == true, v])),
          ],
        ],
      ],
      DDisclosure(title: 'Shadow', open: ins.isOpen('shadow'), onToggle: () => ins.toggle('shadow'), swatch: shadow == null ? null : look.gray(40), swatchNone: shadow == null),
      if (ins.isOpen('shadow')) ..._shadow(shadow),
      const DSpace(4),
      const DSeparator(),
      DCheckbox(on: s['reflection'] != null, bold: true, size: 15, label: 'Reflection', onChanged: (v) => put('reflection', v)),
      if (s['reflection'] != null)
        Row(children: [
          Expanded(child: DSlider(value: ((s['reflection'] as num).toDouble() * 100), min: 0, max: 100, showValue: false, onChanged: (v) => put('reflection', v))),
          const SizedBox(width: 8),
          DStepper(value: ((s['reflection'] as num).toDouble() * 100).roundToDouble(), min: 0, max: 100, suffix: '%', onChanged: (v) => put('reflection', v)),
        ]),
      const DSpace(4),
      const DSeparator(),
      const DLabel('Opacity', size: 15, bold: true),
      Row(children: [
        Expanded(child: DSlider(value: ((s['opacity'] as num?)?.toDouble() ?? 1) * 100, min: 0, max: 100, showValue: false, onChanged: (v) => put('opacity', v))),
        const SizedBox(width: 8),
        DStepper(value: (((s['opacity'] as num?)?.toDouble() ?? 1) * 100).roundToDouble(), min: 0, max: 100, suffix: '%', onChanged: (v) => put('opacity', v)),
      ]),
      const DSpace(4),
      const DSeparator(),
      DCheckbox(on: s['title'] == true, label: 'Title', onChanged: (v) => put('title', v)),
      DCheckbox(on: s['caption'] == true, label: 'Caption', onChanged: (v) => put('caption', v)),
    ]);
  }

  List<Widget> _shadow(Map<String, dynamic>? sh) {
    const kinds = ['No Shadow', 'Drop Shadow', 'Contact Shadow', 'Curved Shadow'];
    final cur = sh == null ? 0 : ((sh['kind'] as int? ?? 0) + 1).clamp(0, 3);
    final out = <Widget>[
      DCombo<int>(text: kinds[cur], items: () => [for (final (i, n) in kinds.indexed) DChoice(n, i, on: i == cur)], onPick: (k) {
        if (k != cur) put('shadowKind', k);
      }),
    ];
    if (sh == null) return out;
    double n(String k) => (sh[k] as num?)?.toDouble() ?? 0;
    Widget row(String label, double v, double min, double max, String suffix, String key) => DRow(label, DStepper(value: v.roundToDouble(), min: min, max: max, suffix: suffix, onChanged: (x) => put('shadow', {key: x})));
    final kind = sh['kind'] as int? ?? 0;
    if (kind != 2) out.add(row('Blur', n('blur'), 0, 100, ' pt', 'blur'));
    out.add(row('Offset', n('offset'), 0, 100, ' pt', 'offset'));
    out.add(row('Opacity', n('opacity') * 100, 0, 100, '%', 'opacity'));
    out.add(switch (kind) {
      0 => row('Angle', n('angle'), 0, 359, '°', 'angle'),
      1 => row('Perspective', math.asin(n('height').clamp(0.0, 1.0)) * 180 / math.pi, 1, 90, '°', 'perspective'),
      _ => row('Curvature', n('curve') * 100, 0, 100, '%', 'curve'),
    });
    out.add(DRow('Color', DColorWell(color: colorOf(sh['color'], Colors.black), onChanged: (c) => put('shadow', {'color': colorTo(c)}))));
    return out;
  }
}

/// The theme's Shape / Text / Image Styles: six to a page, ‹ › and page dots (objinsp style).
class _Presets extends StatefulWidget {
  const _Presets({required this.presets, required this.kind, required this.imageData, required this.ins, required this.onPick});
  final List<Map<String, dynamic>> presets;
  final String kind;
  final int? imageData;
  final InspectorState ins;
  final void Function(int id) onPick;
  @override
  State<_Presets> createState() => _PresetsState();
}

class _PresetsState extends State<_Presets> {
  int _page = 0;
  ui.Image? _thumb;

  @override
  void initState() {
    super.initState();
    final d = widget.imageData;
    if (d != null) widget.ins.engine.thumbnail(d).then((t) => mounted ? setState(() => _thumb = t) : null);
  }

  @override
  Widget build(BuildContext context) {
    final look = LookScope.of(context);
    final pages = (widget.presets.length + 5) ~/ 6;
    _page = _page.clamp(0, pages - 1);
    final image = widget.kind == 'Image Styles';
    Widget arrow(String t, bool on, VoidCallback f) => GestureDetector(
          onTap: on ? f : null,
          child: SizedBox(width: 24, height: 60, child: Center(child: Text(t, style: Look.font(26, on ? look.text : look.textDim)))),
        );
    return Column(children: [
      LayoutBuilder(builder: (ctx, box) {
        final w = box.maxWidth - 56;
        final cell = Size(math.min(w / 3, 96), 60);
        return Row(children: [
          arrow('‹', _page > 0, () => setState(() => _page--)),
          const SizedBox(width: 4),
          Column(children: [
            for (var row = 0; row < 2; row++)
              Row(children: [
                for (var col = 0; col < 3; col++)
                  SizedBox.fromSize(
                    size: cell,
                    child: Builder(builder: (_) {
                      final i = _page * 6 + row * 3 + col;
                      if (i >= widget.presets.length) return const SizedBox.shrink();
                      final p = widget.presets[i];
                      return GestureDetector(
                        onTap: () => widget.onPick(p['id'] as int),
                        child: CustomPaint(painter: _PresetPainter(p, look, image, _thumb)),
                      );
                    }),
                  ),
              ]),
          ]),
          const SizedBox(width: 4),
          arrow('›', _page + 1 < pages, () => setState(() => _page++)),
        ]);
      }),
      const SizedBox(height: 6),
      Text(widget.kind, style: Look.font(15, look.text, bold: true)),
      const SizedBox(height: 6),
      Text([for (var i = 0; i < pages; i++) i == _page ? '●' : '○'].join(' '), style: Look.font(10, look.textDim)),
      const SizedBox(height: 6),
    ]);
  }
}

class _PresetPainter extends CustomPainter {
  _PresetPainter(this.p, this.look, this.image, this.thumb);
  final Map<String, dynamic> p;
  final Look look;
  final bool image;
  final ui.Image? thumb;

  @override
  void paint(Canvas c, Size size) {
    final r = Offset.zero & size;
    final stroke = p['stroke'] as Map<String, dynamic>?;
    if (image) {
      final inner = r.deflate(9);
      if (p['shadow'] == true) c.drawRRect(RRect.fromRectAndRadius(inner.shift(const Offset(2, 2)), const Radius.circular(1)), Paint()..color = const Color.fromARGB(110, 0, 0, 0));
      if (thumb != null) {
        c.drawImageRect(thumb!, Offset.zero & Size(thumb!.width.toDouble(), thumb!.height.toDouble()), inner, Paint()..filterQuality = FilterQuality.medium);
      } else {
        c.drawRRect(RRect.fromRectAndRadius(inner, const Radius.circular(1)), Paint()..color = look.gray(0x60));
      }
      if (stroke != null) {
        final w = ((stroke['width'] as num?)?.toDouble() ?? 1).clamp(1.0, 4.0);
        c.drawRect(inner.inflate(w / 2), Paint()
          ..style = PaintingStyle.stroke
          ..strokeWidth = w
          ..color = colorOf(stroke['color']));
      }
      if (p['reflection'] == true) c.drawRect(Rect.fromLTWH(inner.left, inner.bottom + 1, inner.width, 6), Paint()..color = const Color.fromARGB(30, 255, 255, 255));
      return;
    }
    final inner = r.deflate(7);
    final rr = RRect.fromRectAndRadius(inner, const Radius.circular(3));
    if (p['fill'] != null) {
      c.drawRRect(rr, Paint()..color = colorOf(p['fill']));
    } else {
      c.drawRRect(rr, Paint()..color = look.gray(0x2a));
      final tp = TextPainter(text: TextSpan(text: 'Aa', style: Look.font(16, look.text)), textDirection: TextDirection.ltr)..layout();
      tp.paint(c, inner.center - Offset(tp.width / 2, tp.height / 2));
    }
    if (stroke != null) {
      c.drawRRect(rr.deflate(1), Paint()
        ..style = PaintingStyle.stroke
        ..strokeWidth = 2
        ..color = colorOf(stroke['color']));
    }
  }

  @override
  bool shouldRepaint(_PresetPainter o) => o.thumb != thumb || o.p != p;
}

// ---------------------------------------------------------------- Arrange (objinsp arrange)

class ArrangePane extends StatefulWidget {
  const ArrangePane({super.key, required this.s, required this.ins});
  final Map<String, dynamic> s;
  final InspectorState ins;
  @override
  State<ArrangePane> createState() => _ArrangePaneState();
}

class _ArrangePaneState extends State<ArrangePane> {
  bool _constrain = false;
  Map<String, dynamic> get s => widget.s;
  void put(String k, Object? v) => widget.ins.set('arrange.$k', v);
  void cmd(String c) => put('command', c);
  double d(String k) => (s[k] as num?)?.toDouble() ?? 0;

  @override
  Widget build(BuildContext context) {
    final look = LookScope.of(context);
    final count = s['count'] as int? ?? 1;
    final locked = s['locked'] == true;
    final inline = s['inline'] == true;
    final wrap = s['wrap'] as int? ?? 4;
    final wraps = inline ? const [('Inline with Text', 4 - 4), ('Around', 5), ('Above and Below', 3), ('None', 2)] : const [('Automatic', 4), ('Around', 5), ('Above and Below', 3), ('None', 2)];
    Widget caption(String t) => Text(t, style: Look.font(13, look.textDim));
    // (a locked object's size, position and angle are faded and can't be changed)
    Widget lock(Widget w) => Opacity(opacity: locked ? 0.5 : 1, child: IgnorePointer(ignoring: locked, child: w));
    Widget geo(String label, String key, double v) => lock(DRow(label, DStepper(value: v, min: key == 'w' || key == 'h' ? 1 : -100000, max: 100000, suffix: ' pt', onChanged: (x) => put('geometry', {key: x, 'constrain': _constrain}))));
    return EguiColumn(children: [
      if (s['wordProcessing'] == true) ...[
        ...dSection('Object Placement'),
        DSegmented(labels: const ['Stay on Page', 'Move with Text'], cur: inline ? 1 : 0, onPick: (i) => put('placement', i == 1)),
      ],
      ...dSection('Text Wrap'),
      DCombo<int>(
        text: wraps.firstWhere((w) => w.$2 == wrap, orElse: () => wraps.first).$1,
        items: () => [for (final w in wraps) DChoice(w.$1, w.$2, on: w.$2 == wrap)],
        onPick: (w) => put('wrap', w),
      ),
      if (!inline && wrap != 2) DRow('Spacing', DStepper(value: d('wrapMargin'), min: 0, max: 200, suffix: ' pt', onChanged: (v) => put('wrapMargin', v))),
      const DSeparator(),
      // the layer buttons, captions under them
      Padding(
        padding: const EdgeInsets.symmetric(vertical: 6),
        child: Row(children: [
          for (final (i, pair) in const [
            [('Back', 'Send to Back', false), ('Front', 'Bring to Front', true)],
            [('Backward', 'Send Backward', false), ('Forward', 'Bring Forward', true)],
          ].indexed) ...[
            if (i > 0) const SizedBox(width: 12),
            Expanded(
              child: Column(children: [
                SizedBox(
                  height: 44,
                  child: Row(crossAxisAlignment: CrossAxisAlignment.stretch, children: [
                    for (final (k, (_, c, top)) in pair.indexed) ...[
                      if (k > 0) Center(child: Container(width: 1, height: 22, color: look.separator)),
                      Expanded(child: GestureDetector(behavior: HitTestBehavior.opaque, onTap: () => cmd(c), child: CustomPaint(painter: _LayerIcon(top, look.text)))),
                    ],
                  ]),
                ),
                const SizedBox(height: 6),
                Row(children: [for (final (label, _, _) in pair) Expanded(child: Center(child: caption(label)))]),
              ]),
            ),
          ],
        ]),
      ),
      Row(children: [
        Expanded(
          child: Opacity(
            opacity: count >= 2 ? 1 : 0.5,
            child: IgnorePointer(
              ignoring: count < 2,
              child: DCombo<String>(text: 'Align', items: () => [for (final a in ['Left', 'Center', 'Right', 'Top', 'Middle', 'Bottom']) DChoice(a, 'Align Objects > $a')], onPick: cmd),
            ),
          ),
        ),
        Container(width: 1, height: 26, margin: const EdgeInsets.symmetric(horizontal: 12), color: look.separator),
        Expanded(
          child: Opacity(
            opacity: count >= 3 ? 1 : 0.5,
            child: IgnorePointer(
              ignoring: count < 3,
              child: DCombo<String>(text: 'Distribute', items: () => [for (final a in ['Evenly', 'Horizontally', 'Vertically']) DChoice(a, 'Distribute Objects > $a')], onPick: cmd),
            ),
          ),
        ),
      ]),
      ...dSection('Size'),
      geo('Width', 'w', d('w')),
      geo('Height', 'h', d('h')),
      lock(DCheckbox(on: _constrain, label: 'Constrain proportions', onChanged: (v) => setState(() => _constrain = v))),
      ...dSection('Position'),
      geo('X', 'x', d('x')),
      geo('Y', 'y', d('y')),
      ...dSection('Rotate'),
      lock(Row(children: [
        _Dial(angle: d('angle'), onChanged: (a, done) => put('geometry', {'angle': a, 'merge': !done})),
        const SizedBox(width: 12),
        Expanded(child: caption('Angle')),
        DStepper(value: d('angle').roundToDouble(), min: 0, max: 359, suffix: '°', onChanged: (a) => put('geometry', {'angle': a})),
      ])),
      lock(DRow('Flip', Row(mainAxisSize: MainAxisSize.min, children: [
        DButton(width: 52, onTap: () => put('geometry', {'vflip': true}), child: const Text('⇅')),
        const SizedBox(width: 8),
        DButton(width: 52, onTap: () => put('geometry', {'hflip': true}), child: const Text('⇆')),
      ]))),
      const DSeparator(),
      Row(children: [
        Expanded(child: DButton(height: 28, onTap: locked ? null : () => cmd('Lock'), child: const Text('Lock'))),
        const SizedBox(width: 8),
        Expanded(child: DButton(height: 28, onTap: locked ? () => cmd('Unlock') : null, child: const Text('Unlock'))),
      ]),
      Row(children: [
        Expanded(child: DButton(height: 28, onTap: count >= 2 ? () => cmd('Group') : null, child: const Text('Group'))),
        const SizedBox(width: 8),
        Expanded(child: DButton(height: 28, onTap: s['group'] == true ? () => cmd('Ungroup') : null, child: const Text('Ungroup'))),
      ]),
    ]);
  }
}

class _LayerIcon extends CustomPainter {
  _LayerIcon(this.top, this.col);
  final bool top;
  final Color col;
  @override
  void paint(Canvas c, Size size) {
    final ctr = size.center(Offset.zero);
    for (var k = 0; k < 3; k++) {
      final y = ctr.dy - 5 + k * 4;
      final path = Path()..addPolygon([Offset(ctr.dx - 8, y), Offset(ctr.dx, y - 3.5), Offset(ctr.dx + 8, y), Offset(ctr.dx, y + 3.5)], true);
      final solid = (top && k == 0) || (!top && k == 2);
      c.drawPath(path, solid ? (Paint()..color = col) : (Paint()
        ..style = PaintingStyle.stroke
        ..strokeWidth = 1.1
        ..color = col));
    }
  }

  @override
  bool shouldRepaint(_LayerIcon o) => o.col != col;
}

/// The rotation dial: drag round it to rotate (objinsp arrange).
class _Dial extends StatefulWidget {
  const _Dial({required this.angle, required this.onChanged});
  final double angle;
  final void Function(double angle, bool done) onChanged;
  @override
  State<_Dial> createState() => _DialState();
}

class _DialState extends State<_Dial> {
  double? _live;
  double _at(Offset p) => (-math.atan2(p.dy - 20, p.dx - 20) * 180 / math.pi % 360).roundToDouble();
  @override
  Widget build(BuildContext context) {
    final look = LookScope.of(context);
    final a = _live ?? widget.angle;
    return GestureDetector(
      onPanUpdate: (d) {
        setState(() => _live = _at(d.localPosition));
        widget.onChanged(_live!, false);
      },
      onPanEnd: (_) {
        if (_live != null) widget.onChanged(_live!, true);
        setState(() => _live = null);
      },
      child: CustomPaint(
        size: const Size(40, 40),
        painter: _FnPaint((c, s) {
          final ctr = s.center(Offset.zero);
          c.drawCircle(ctr, 20, Paint()..color = touchFill(look));
          final r = -a * math.pi / 180;
          c.drawLine(ctr, ctr + Offset(math.cos(r), math.sin(r)) * 14, Paint()
            ..strokeWidth = 2
            ..color = look.text);
        }),
      ),
    );
  }
}

class _FnPaint extends CustomPainter {
  _FnPaint(this.f);
  final void Function(Canvas, Size) f;
  @override
  void paint(Canvas canvas, Size size) => f(canvas, size);
  @override
  bool shouldRepaint(_FnPaint o) => true;
}

// ---------------------------------------------------------------- Format > Image (objinsp image_panel)

class ImagePane extends StatefulWidget {
  const ImagePane({super.key, required this.s, required this.ins});
  final Map<String, dynamic> s;
  final InspectorState ins;
  @override
  State<ImagePane> createState() => _ImagePaneState();
}

class _ImagePaneState extends State<ImagePane> {
  late final _desc = TextEditingController(text: widget.s['description'] as String? ?? '');
  final _descFocus = FocusNode();

  Map<String, dynamic> get s => widget.s;
  void put(String k, Object? v) => widget.ins.set('image.$k', v);

  @override
  void initState() {
    super.initState();
    _descFocus.addListener(() {
      if (!_descFocus.hasFocus && _desc.text != (s['description'] as String? ?? '')) put('description', _desc.text);
    });
  }

  @override
  void dispose() {
    _desc.dispose();
    _descFocus.dispose();
    super.dispose();
  }

  Widget _tile(Look look, String label, bool on, void Function(Canvas, Offset, Paint) draw, VoidCallback tap) => Expanded(
        child: GestureDetector(
          onTap: tap,
          child: Container(
            height: 60,
            decoration: BoxDecoration(color: on ? look.accent : null, borderRadius: BorderRadius.circular(14)),
            child: CustomPaint(painter: _FnPaint((c, sz) {
              final ctr = sz.center(Offset.zero);
              draw(c, ctr - const Offset(0, 8), Paint()
                ..strokeWidth = 1.4
                ..style = PaintingStyle.stroke
                ..color = look.text);
              final tp = TextPainter(text: TextSpan(text: label, style: Look.font(14, on ? Colors.white : look.text)), textDirection: TextDirection.ltr)..layout();
              tp.paint(c, ctr + Offset(-tp.width / 2, 14 - tp.height / 2));
            })),
          ),
        ),
      );

  Widget _adjust(String label, String key) {
    final v = ((s[key] as num?)?.toDouble() ?? 0) * 100;
    return Column(crossAxisAlignment: CrossAxisAlignment.stretch, children: [
      const SizedBox(height: 6),
      DLabel(label),
      Row(children: [
        Expanded(child: DSlider(value: v, min: -100, max: 100, showValue: false, onChanged: (x) => put(key, x))),
        const SizedBox(width: 8),
        DStepper(value: v.roundToDouble(), min: -100, max: 100, suffix: '%', onChanged: (x) => put(key, x)),
      ]),
    ]);
  }

  @override
  Widget build(BuildContext context) {
    final look = LookScope.of(context);
    final editing = s['editingMask'] == true;
    return EguiColumn(children: [
      ...dSection('File Info'),
      Row(children: [
        DesktopIcon(DIcon.file, size: 18, color: look.textDim),
        const SizedBox(width: 8),
        Expanded(child: DLabel(s['name'] as String? ?? 'Image')),
        GestureDetector(
          onTap: () async {
            final f = await FilePicker.pickFile(type: FileType.custom, allowedExtensions: ['png', 'jpg', 'jpeg']);
            if (f?.path != null) put('replace', f!.path);
          },
          child: SizedBox(
            width: 48,
            height: 36,
            child: CustomPaint(painter: _FnPaint((c, sz) {
              final f = Rect.fromCenter(center: sz.center(Offset.zero), width: 14, height: 10);
              final p = Paint()
                ..style = PaintingStyle.stroke
                ..strokeWidth = 1.2
                ..color = look.text;
              c.drawRRect(RRect.fromRectAndRadius(f.deflate(0.6), const Radius.circular(1.5)), p);
              c.drawLine(f.topLeft + const Offset(0, 2), f.topLeft + const Offset(5, 2), p);
            })),
          ),
        ),
      ]),
      const DSeparator(),
      Row(children: [
        _tile(look, editing ? 'Done' : 'Edit Mask', editing, (c, ctr, p) {
          c.drawLine(ctr + const Offset(-4, -8), ctr + const Offset(-4, 4), p);
          c.drawLine(ctr + const Offset(-4, 4), ctr + const Offset(8, 4), p);
          c.drawLine(ctr + const Offset(-8, -4), ctr + const Offset(4, -4), p);
          c.drawLine(ctr + const Offset(4, -4), ctr + const Offset(4, 8), p);
        }, () => put('command', 'Edit Mask')),
        const SizedBox(width: 8),
        _tile(look, 'Auto Crop', false, (c, ctr, p) {
          final r = Rect.fromCenter(center: ctr, width: 14, height: 12);
          for (final (a, b) in [(r.topLeft, const Offset(4, 0)), (r.topLeft, const Offset(0, 4)), (r.topRight, const Offset(-4, 0)), (r.topRight, const Offset(0, 4)), (r.bottomLeft, const Offset(4, 0)), (r.bottomLeft, const Offset(0, -4)), (r.bottomRight, const Offset(-4, 0)), (r.bottomRight, const Offset(0, -4))]) {
            c.drawLine(a, a + b, p);
          }
          c.drawRRect(RRect.fromRectAndRadius(r.deflate(3.5), const Radius.circular(1)), p);
        }, () => put('command', 'Auto Crop')),
      ]),
      DButton(height: 26, onTap: () => put('command', 'Remove Background'), child: const Text('Remove Background')),
      if (s['masked'] == true) DButton(height: 26, onTap: () => put('command', 'Reset Mask'), child: const Text('Reset Mask')),
      ...dSection('Adjustments'),
      _adjust('Exposure', 'exposure'),
      _adjust('Saturation', 'saturation'),
      Row(children: [
        Expanded(child: DButton(height: 26, fill: s['enhance'] == true ? look.accent : null, onTap: () => put('enhance', true), child: Text('Enhance', style: Look.font(13, s['enhance'] == true ? Colors.white : look.text)))),
        const SizedBox(width: 8),
        DButton(width: 44, height: 26, onTap: () {}, child: const DIconBox(DIcon.sliders, size: 20)),
        const SizedBox(width: 8),
        Expanded(child: DButton(height: 26, onTap: () => put('reset', true), child: const Text('Reset'))),
      ]),
      if (s['equation'] != null) ...[
        ...dSection('Equation'),
        Container(padding: const EdgeInsets.all(8), decoration: BoxDecoration(color: touchFill(look), borderRadius: BorderRadius.circular(10)), child: Text(s['equation'] as String, style: TextStyle(fontFamily: 'monospace', fontSize: 13, color: look.text))),
      ],
      ...dSection('Description'),
      Container(
        margin: const EdgeInsets.symmetric(vertical: 6),
        padding: const EdgeInsets.symmetric(vertical: 4),
        child: TextField(
          controller: _desc,
          focusNode: _descFocus,
          minLines: 5,
          maxLines: 5,
          style: Look.font(15, look.text),
          cursorColor: look.accent,
          decoration: InputDecoration(isDense: true, border: InputBorder.none, hintText: 'Describe this image for people who use assistive technology', hintStyle: Look.font(15, look.textDim)),
        ),
      ),
    ]);
  }
}

// ---------------------------------------------------------------- Chart / Axis / Series (kn_ui chartpanes)

/// chartpanes section: 8 points, a line, the title.
List<Widget> _chartSection(String t) => [const DSpace(8), const DSeparator(), DSectionTitle(t, size: 13, bold: false)];

/// ui.horizontal: the items 8 apart (a leading label takes the room left, so the controls sit
/// at the row's right).
Widget _hrow(List<Widget> kids) => SizedBox(
      height: 44,
      child: Row(children: [
        for (final (i, k) in kids.indexed) ...[
          if (i > 0) const SizedBox(width: 8),
          if (i == 0 && (k is DLabel || k is DCheckbox)) Expanded(child: k) else k,
        ],
      ]),
    );

const _formats = ['Automatic', 'Number', 'Percentage', 'Currency'];

/// chartpanes format_controls: Format ▾ and its decimals.
List<Widget> _formatRow(Map<String, dynamic> s, void Function(Map<String, Object?>) put) {
  final kind = s['formatKind'] as int? ?? 0;
  final dec = (s['decimals'] as num?)?.toDouble() ?? 0;
  return [
    DRow('Format', DCombo<int>(width: 130, text: _formats[kind], items: () => [for (final (i, f) in _formats.indexed) DChoice(f, i, on: i == kind)], onPick: (k) => put({'kind': k, 'decimals': dec.round()}))),
    if (kind != 0) DRow('Decimals', DStepper(value: dec, min: 0, max: 10, onChanged: (v) => put({'kind': kind, 'decimals': v.round()}))),
  ];
}

class ChartPane extends StatelessWidget {
  const ChartPane({super.key, required this.s, required this.ins});
  final Map<String, dynamic> s;
  final InspectorState ins;
  void put(String k, Object? v) => ins.set('chart.$k', v);

  @override
  Widget build(BuildContext context) {
    final look = LookScope.of(context);
    final presets = [for (final p in (s['presets'] as List? ?? [])) p as Map<String, dynamic>];
    final types = [for (final t in (s['types'] as List? ?? [])) (t[0] as String, t[1] as String)];
    final type = s['type'] as String? ?? '';
    final font = s['font'] as String? ?? '';
    const fonts = ['HelveticaNeue', 'Avenir-Book', 'AvenirNext-Regular', 'Futura-Medium', 'GillSans', 'Georgia', 'TimesNewRomanPSMT', 'Menlo-Regular'];
    final border = s['borderStroke'] as Map<String, dynamic>?;
    return EguiColumn(children: [
      Align(alignment: Alignment.centerLeft, child: DButton(onTap: () => put('editData', true), child: const Text('Edit Chart Data'))),
      DLabel(s['summary'] as String? ?? '', dim: true),
      if (presets.isNotEmpty) ...[
        ..._chartSection('Chart Styles'),
        Padding(
          padding: const EdgeInsets.symmetric(vertical: 8),
          child: Wrap(spacing: 8, runSpacing: 8, children: [
          for (final p in presets)
            GestureDetector(
              onTap: p['id'] == s['preset'] ? null : () => put('preset', p['id']),
              child: CustomPaint(size: const Size(60, 44), painter: _FnPaint((c, sz) {
                final r = Offset.zero & sz;
                c.drawRRect(RRect.fromRectAndRadius(r, const Radius.circular(8)), Paint()..color = touchFill(look));
                final cols = [for (final x in (p['colors'] as List? ?? [])) colorOf(x)];
                final n = cols.length.clamp(1, 4);
                final bw = (r.width - 12) / n;
                for (var k = 0; k < math.min(n, cols.length); k++) {
                  final hgt = r.height * (0.35 + 0.15 * k);
                  final x = 6 + bw * k;
                  c.drawRRect(RRect.fromLTRBR(x + 1, r.height - 4 - hgt, x + bw - 1, r.height - 4, const Radius.circular(1)), Paint()..color = cols[k]);
                }
                if (p['id'] == s['preset']) {
                  c.drawRRect(RRect.fromRectAndRadius(r.deflate(1), const Radius.circular(8)), Paint()
                    ..style = PaintingStyle.stroke
                    ..strokeWidth = 2
                    ..color = look.accent);
                }
              })),
            ),
          ]),
        ),
      ],
      ..._chartSection('Chart Font'),
      _hrow([
        Expanded(child: DCombo<String>(text: font, items: () => [for (final f in fonts) DChoice(f, f, on: f == font)], onPick: (f) => put('font', f))),
        DButton(onTap: () => put('fontScale', 1 / 1.11), child: Text('A', style: Look.font(13, look.text))),
        DButton(onTap: () => put('fontScale', 1.11), child: Text('A', style: Look.font(19, look.text))),
      ]),
      ..._chartSection('Chart Type'),
      DCombo<String>(text: types.firstWhere((t) => t.$1 == type, orElse: () => (type, type)).$2, items: () => [for (final t in types) DChoice(t.$2, t.$1, on: t.$1 == type)], onPick: (t) => put('type', t)),
      ..._chartSection('Chart Options'),
      DCheckbox(on: s['legend'] == true, label: 'Legend', onChanged: (v) => put('legend', v)),
      DCheckbox(on: s['title'] != null, label: 'Title', onChanged: (v) => put('title', v)),
      if (s['title'] != null) _LineField(text: s['title'] as String, onDone: (t) => put('title', t)),
      DCheckbox(on: s['border'] == true, label: 'Border', onChanged: (v) => put('border', v)),
      if (s['border'] == true)
        _hrow([
          DColorWell(width: 110, color: colorOf(border?['color'], const Color.from(alpha: 1, red: 0.6, green: 0.6, blue: 0.6)), onChanged: (c) => put('borderStroke', {'color': colorTo(c), 'width': border?['width'] ?? 1.0})),
          DStepper(value: (border?['width'] as num?)?.toDouble() ?? 1, step: 0.5, min: 0, max: 20, suffix: ' pt', decimals: 1, onChanged: (w) => put('borderStroke', {'color': border?['color'] ?? [0.6, 0.6, 0.6, 1.0], 'width': w})),
        ]),
      ..._chartSection('Background'),
      _hrow([
        DCheckbox(on: s['background'] != null, label: 'Fill', onChanged: (v) => put('background', v)),
        if (s['background'] != null) DColorWell(width: 110, color: colorOf(s['background']), onChanged: (c) => put('background', colorTo(c))),
      ]),
      if (s['bars'] == true) ...[
        ..._chartSection('Bars'),
        _hrow([const DLabel('Rounded Corners'), DStepper(value: (s['cornerRadius'] as num?)?.toDouble() ?? 0, step: 5, min: 0, max: 100, suffix: '%', onChanged: (v) => put('cornerRadius', v))]),
        if (((s['cornerRadius'] as num?) ?? 0) > 0) DCheckbox(on: s['outerOnly'] == true, label: 'Outside Corners Only', onChanged: (v) => put('outerOnly', v)),
        _hrow([const DLabel('Gap between bars'), DStepper(value: (s['barGap'] as num?)?.toDouble() ?? 0, step: 5, min: 0, max: 500, suffix: '%', onChanged: (v) => put('barGap', v))]),
        _hrow([const DLabel('Gap between sets'), DStepper(value: (s['setGap'] as num?)?.toDouble() ?? 0, step: 5, min: 0, max: 500, suffix: '%', onChanged: (v) => put('setGap', v))]),
      ],
      if (s['donut'] == true) ...[
        ..._chartSection('Donut'),
        _hrow([const DLabel('Inner Radius'), DStepper(value: (s['innerRadius'] as num?)?.toDouble() ?? 0, step: 5, min: 0, max: 95, suffix: '%', onChanged: (v) => put('innerRadius', v))]),
      ],
    ]);
  }
}

class AxisPane extends StatelessWidget {
  const AxisPane({super.key, required this.s, required this.ins});
  final Map<String, dynamic> s;
  final InspectorState ins;
  void put(String k, Object? v) => ins.set('chart.$k', v);

  @override
  Widget build(BuildContext context) {
    final names = [for (final n in (s['names'] as List? ?? [])) n as String];
    final ai = s['axis'] as int? ?? 0;
    final isValue = s['isValue'] == true;
    const angles = [(0.0, 'Horizontal'), (45.0, '45°'), (90.0, 'Vertical'), (-45.0, '−45°')];
    final angle = (s['angle'] as num?)?.toDouble() ?? 0;
    final angleName = angles.where((a) => (a.$1 - angle).abs() < 0.5).map((a) => a.$2).firstOrNull ?? 'Custom';
    final grid = s['gridStroke'] as Map<String, dynamic>?;
    List<Widget> minMax(String label, String key) {
      final cur = (s[key] as num?)?.toDouble();
      return [
        DCheckbox(on: cur == null, label: '$label: Auto', onChanged: (auto) {
          if (!auto) put(key, label == 'Min' ? 0.0 : 100.0);
        }),
        if (cur != null) DRow(label, DStepper(value: cur, min: -1e12, max: 1e12, decimals: 2, onChanged: (v) => put(key, v))),
      ];
    }

    return EguiColumn(children: [
      if (names.isNotEmpty) DSegmented(labels: names, cur: ai, after: 0, onPick: (i) => put('axisPick', i)),
      ..._chartSection('Axis Options'),
      DCheckbox(on: s['title'] == true, label: 'Axis Name', onChanged: (v) => put('axisTitle', v)),
      DCheckbox(on: s['labels'] == true, label: 'Labels', onChanged: (v) => put('axisLabels', v)),
      DCheckbox(on: s['line'] == true, label: 'Axis Line', onChanged: (v) => put('axisLine', v)),
      if (s['labels'] == true) ...[
        _hrow([const DLabel('Label Angle'), DCombo<double>(width: 100, text: angleName, items: () => [for (final a in angles) DChoice(a.$2, a.$1, on: a.$2 == angleName)], onPick: (a) => put('angle', a))]),
        if (isValue) ..._formatRow(s, (f) => put('axisFormat', f)),
      ],
      if (isValue) ...[
        ..._chartSection('Axis Scale'),
        DCheckbox(on: s['log'] == true, label: 'Logarithmic', onChanged: (v) => put('log', v)),
        ...minMax('Min', 'min'),
        ...minMax('Max', 'max'),
        _hrow([const DLabel('Major Steps'), DStepper(value: (s['major'] as num?)?.toDouble() ?? 4, min: 1, max: 20, onChanged: (v) => put('major', v))]),
        _hrow([const DLabel('Minor Steps'), DStepper(value: (s['minor'] as num?)?.toDouble() ?? 2, min: 1, max: 20, onChanged: (v) => put('minor', v))]),
      ],
      ..._chartSection('Gridlines and Tick Marks'),
      DCheckbox(on: s['gridlines'] == true, label: 'Major Gridlines', onChanged: (v) => put('gridlines', v)),
      DCheckbox(on: s['minorGridlines'] == true, label: 'Minor Gridlines', onChanged: (v) => put('minorGridlines', v)),
      DCheckbox(on: s['majorTicks'] == true, label: 'Major Tick Marks', onChanged: (v) => put('majorTicks', v)),
      DCheckbox(on: s['minorTicks'] == true, label: 'Minor Tick Marks', onChanged: (v) => put('minorTicks', v)),
      if (s['gridlines'] == true) ...[
        DRow('Gridline Color', DColorWell(width: 110, color: colorOf(grid?['color'], const Color.from(alpha: 1, red: 0.72, green: 0.72, blue: 0.72)), onChanged: (c) => put('gridStroke', {'color': colorTo(c), 'width': grid?['width'] ?? 1.0}))),
        DRow('Gridline Width', DStepper(value: (grid?['width'] as num?)?.toDouble() ?? 1, step: 0.5, min: 0, max: 10, suffix: ' pt', decimals: 1, onChanged: (w) => put('gridStroke', {'color': grid?['color'] ?? [0.72, 0.72, 0.72, 1.0], 'width': w}))),
      ],
    ]);
  }
}

class SeriesPane extends StatelessWidget {
  const SeriesPane({super.key, required this.s, required this.ins});
  final Map<String, dynamic> s;
  final InspectorState ins;
  void put(String k, Object? v) => ins.set('chart.$k', v);

  @override
  Widget build(BuildContext context) {
    if (s['empty'] == true) return const SizedBox.shrink();
    final names = [for (final n in (s['names'] as List? ?? [])) n as String];
    final i = s['series'] as int? ?? 0;
    final v = s['valueLabels'] == true;
    final stroke = s['stroke'] as Map<String, dynamic>?;
    final lineKind = s['lineKind'] == true;
    final outline = stroke != null && ((stroke['width'] as num?) ?? 0) > 0;
    final sc = stroke?['color'] ?? [0.0, 0.0, 0.0, 1.0];
    final sw = (stroke?['width'] as num?)?.toDouble() ?? 1;
    return EguiColumn(children: [
      ..._chartSection('Value Labels'),
      DCheckbox(on: v, label: 'Show value labels', onChanged: (x) => put('valueLabels', x)),
      if (s['pieOrBubble'] == true) DCheckbox(on: s['namesShown'] == true, label: 'Series names', onChanged: (x) => put('seriesNames', x)),
      if (v) ..._formatRow(s, (f) => put('seriesFormat', f)),
      ..._chartSection('Series'),
      DCombo<int>(text: names.isEmpty ? '' : names[i.clamp(0, names.length - 1)], items: () => [for (final (k, n) in names.indexed) DChoice(n, k, on: k == i)], onPick: (k) => put('seriesPick', k)),
      _hrow([const DLabel('Fill'), DColorWell(width: 110, color: colorOf(s['color']), onChanged: (c) => put('seriesFill', colorTo(c)))]),
      _hrow([
        if (lineKind) const DLabel('Line') else Expanded(child: DCheckbox(on: outline, label: 'Outline', onChanged: (x) => put('seriesStroke', x ? {'color': sc, 'width': math.max(1.0, sw)} : null))),
        if (outline || lineKind) DColorWell(width: 110, color: colorOf(sc), onChanged: (c) => put('seriesStroke', {'color': colorTo(c), 'width': sw})),
      ]),
      if (outline || lineKind) DRow('Width', DStepper(value: sw, step: 0.5, min: 0, max: 20, suffix: ' pt', decimals: 1, onChanged: (w) => put('seriesStroke', {'color': sc, 'width': w}))),
      if (s['pie'] == true) ...[
        _hrow([const DLabel('Distance from Center'), DStepper(value: (s['labelDistance'] as num?)?.toDouble() ?? 62, step: 5, min: 0, max: 200, suffix: '%', onChanged: (x) => put('labelDistance', x))]),
        _hrow([const DLabel('Wedge Offset'), DStepper(value: (s['explosion'] as num?)?.toDouble() ?? 0, step: 5, min: 0, max: 100, suffix: '%', onChanged: (x) => put('explosion', x))]),
      ],
    ]);
  }
}

/// A one-line text field committed when it loses focus (egui's singleline TextEdit).
class _LineField extends StatefulWidget {
  const _LineField({required this.text, required this.onDone});
  final String text;
  final void Function(String) onDone;
  @override
  State<_LineField> createState() => _LineFieldState();
}

class _LineFieldState extends State<_LineField> {
  late final _c = TextEditingController(text: widget.text);
  final _f = FocusNode();
  @override
  void initState() {
    super.initState();
    _f.addListener(() {
      if (!_f.hasFocus && _c.text != widget.text) widget.onDone(_c.text);
      setState(() {});
    });
  }

  @override
  void dispose() {
    _c.dispose();
    _f.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final look = LookScope.of(context);
    return Container(
      height: 40,
      padding: EdgeInsets.zero,
      alignment: Alignment.centerLeft,
      child: TextField(
        controller: _c,
        focusNode: _f,
        style: Look.font(16, look.text),
        cursorColor: look.accent,
        decoration: const InputDecoration(isDense: true, border: InputBorder.none, contentPadding: EdgeInsets.symmetric(vertical: 8)),
        onSubmitted: (t) => widget.onDone(t),
      ),
    );
  }
}

// ---------------------------------------------------------------- Table (tblinsp table_panel)

class TablePane extends StatefulWidget {
  const TablePane({super.key, required this.s, required this.ins});
  final Map<String, dynamic> s;
  final InspectorState ins;
  @override
  State<TablePane> createState() => _TablePaneState();
}

class _TablePaneState extends State<TablePane> {
  int _page = 0;
  Map<String, dynamic> get s => widget.s;
  void put(String k, Object? v) => widget.ins.set('table.$k', v);
  double d(String k) => (s[k] as num?)?.toDouble() ?? 0;

  @override
  Widget build(BuildContext context) {
    final look = LookScope.of(context);
    final presets = [for (final p in (s['presets'] as List? ?? [])) p as Map<String, dynamic>];
    final grid = [for (final g in (s['gridlines'] as List? ?? [])) g == true];
    Widget headerCount(String key, String label) => DRow(label, DStepper(value: d(key), min: 0, max: 5, onChanged: (v) => put(key, v)));
    return EguiColumn(children: [
      if (presets.isNotEmpty) ..._styles(look, presets),
      ...dSection('Table Options'),
      DCheckbox(on: s['title'] == true, label: 'Title', onChanged: (v) => put('title', v)),
      DCheckbox(on: s['caption'] == true, label: 'Caption', onChanged: (v) => put('caption', v)),
      ...dSection('Headers & Footer'),
      headerCount('headerRows', 'Header Rows'),
      headerCount('headerCols', 'Header Columns'),
      headerCount('footerRows', 'Footer'),
      const DSeparator(),
      DRow('Rows', DStepper(value: d('rows'), min: 1, max: 999, onChanged: (v) => put('rows', v))),
      DRow('Columns', DStepper(value: d('cols'), min: 1, max: 999, onChanged: (v) => put('cols', v))),
      Row(children: [
        Expanded(child: DLabel('Table Font Size  ${d('fontSize').toStringAsFixed(0)} pt')),
        DButton(width: 44, onTap: () => put('fontSmaller', true), child: Text('A', style: Look.font(13, look.text))),
        const SizedBox(width: 8),
        DButton(width: 44, onTap: () => put('fontBigger', true), child: Text('A', style: Look.font(19, look.text))),
      ]),
      ...dSection('Table Outline'),
      StrokeEditor(stroke: s['outline'] as Map<String, dynamic>?, onChanged: (v) => put('outline', v)),
      ...dSection('Gridlines'),
      Row(children: [
        for (var g = 0; g < 5; g++) ...[
          if (g > 0) const SizedBox(width: 8),
          GestureDetector(
            onTap: () => put('gridline', g),
            child: Container(
              width: 46,
              height: 40,
              decoration: BoxDecoration(color: g < grid.length && grid[g] ? look.accent : null, borderRadius: BorderRadius.circular(12)),
              child: CustomPaint(painter: _GridlinePainter(g, g < grid.length && grid[g], look)),
            ),
          ),
        ],
      ]),
      DCheckbox(on: s['banded'] == true, label: 'Alternating Row Color', onChanged: (v) => put('banded', v)),
      if (s['banded'] == true) DRow('Row Color', DColorWell(color: colorOf(s['bandColor']), onChanged: (c) => put('bandColor', colorTo(c)))),
      ...dSection('Row & Column Size'),
      _hrow([const DLabel('Height'), DStepper(value: d('height'), min: 8, max: 2000, suffix: ' pt', onChanged: (v) => put('height', v)), DButton(onTap: () => put('fitHeight', true), child: const Text('Fit'))]),
      _hrow([const DLabel('Width'), DStepper(value: d('width'), min: 8, max: 2000, suffix: ' pt', onChanged: (v) => put('width', v)), DButton(onTap: () => put('fitWidth', true), child: const Text('Fit'))]),
      DButton(height: 26, onTap: () => put('distributeRows', true), child: const Text('Distribute Rows')),
      DButton(height: 26, onTap: () => put('distributeCols', true), child: const Text('Distribute Columns')),
    ]);
  }

  List<Widget> _styles(Look look, List<Map<String, dynamic>> presets) {
    final tiles = presets.length + 1;
    final pages = (tiles + 5) ~/ 6;
    _page = _page.clamp(0, pages - 1);
    Widget arrow(String t, bool on, VoidCallback f) => GestureDetector(
          onTap: on ? f : null,
          child: SizedBox(width: 24, height: 164, child: Center(child: Text(t, style: Look.font(26, on ? look.text : look.textDim.withValues(alpha: 0.4))))),
        );
    return [
      LayoutBuilder(builder: (ctx, box) {
        final w = box.maxWidth - 48;
        final cell = Size((w / 3).floorToDouble(), 82);
        return Row(children: [
          arrow('‹', _page > 0, () => setState(() => _page--)),
          Column(children: [
            for (var row = 0; row < 2; row++)
              Row(children: [
                for (var col = 0; col < 3; col++)
                  SizedBox.fromSize(
                    size: cell,
                    child: Builder(builder: (_) {
                      final i = _page * 6 + row * 3 + col;
                      final thumb = Size(math.min(cell.width - 16, 88), 70);
                      if (i < presets.length) {
                        return GestureDetector(onTap: () => put('preset', presets[i]['id']), child: Center(child: CustomPaint(size: thumb, painter: _TableThumb(presets[i]))));
                      }
                      if (i == presets.length) {
                        return GestureDetector(
                          onTap: () => put('savePreset', true),
                          child: Center(child: CustomPaint(size: thumb, painter: _FnPaint((c, sz) {
                            final p = Paint()
                              ..style = PaintingStyle.stroke
                              ..strokeWidth = 1.2
                              ..color = look.textDim;
                            final r = Offset.zero & sz;
                            for (final (a, b) in [(r.topLeft, r.topRight), (r.topRight, r.bottomRight), (r.bottomRight, r.bottomLeft), (r.bottomLeft, r.topLeft)]) {
                              final len = (b - a).distance;
                              for (var t = 0.0; t < len; t += 7) {
                                c.drawLine(Offset.lerp(a, b, t / len)!, Offset.lerp(a, b, math.min(1, (t + 4) / len))!, p);
                              }
                            }
                            final tp = TextPainter(text: TextSpan(text: '+', style: Look.font(26, look.textDim)), textDirection: TextDirection.ltr)..layout();
                            tp.paint(c, r.center - Offset(tp.width / 2, tp.height / 2));
                          }))),
                        );
                      }
                      return const SizedBox.shrink();
                    }),
                  ),
              ]),
          ]),
          arrow('›', _page + 1 < pages, () => setState(() => _page++)),
        ]);
      }),
      Center(child: Text('Table Styles', style: Look.font(15, look.text, bold: true))),
      // (a ui.horizontal: 24 tall)
      SizedBox(height: 28, child: Row(mainAxisAlignment: MainAxisAlignment.center, children: [
        for (var k = 0; k < pages; k++)
          GestureDetector(
            onTap: () => setState(() => _page = k),
            child: SizedBox(width: 22, height: 28, child: Center(child: Container(width: 8, height: 8, decoration: BoxDecoration(shape: BoxShape.circle, color: k == _page ? look.text : look.textDim.withValues(alpha: 0.5))))),
          ),
      ])),
    ];
  }
}

/// tblinsp table_preset_thumb: a little table in the style's colours.
class _TableThumb extends CustomPainter {
  _TableThumb(this.p);
  final Map<String, dynamic> p;
  @override
  void paint(Canvas c, Size size) {
    Color? col(String k) => p[k] == null ? null : colorOf(p[k]);
    final r = Offset.zero & size;
    c.drawRect(r, Paint()..color = col('body') ?? Colors.white);
    const rows = 7, cols = 4;
    final ch = r.height / rows, cw = r.width / cols;
    for (var y = 0; y < rows; y++) {
      for (var x = 0; x < cols; x++) {
        final f = y == 0
            ? col('headerRow')
            : x == 0
                ? col('headerCol')
                : y == rows - 1 && p['footer'] != null
                    ? col('footer')
                    : y % 2 == 0
                        ? col('band')
                        : null;
        if (f != null) c.drawRect(Rect.fromLTWH(x * cw, y * ch, cw, ch), Paint()..color = f);
      }
    }
    final line = Paint()
      ..strokeWidth = 0.7
      ..color = col('line') ?? const Color(0xFFBEBEBE);
    void seg(Offset a, Offset b) {
      if (p['dashed'] == true) {
        final len = (b - a).distance;
        for (var t = 0.0; t < len; t += 3) {
          c.drawLine(Offset.lerp(a, b, t / len)!, Offset.lerp(a, b, math.min(1, (t + 1.5) / len))!, line);
        }
      } else {
        c.drawLine(a, b, line);
      }
    }

    for (var y = 1; y < rows; y++) {
      seg(Offset(0, y * ch), Offset(r.width, y * ch));
    }
    for (var x = 1; x < cols; x++) {
      seg(Offset(x * cw, 0), Offset(x * cw, r.height));
    }
    c.drawRect(r.deflate(0.35), Paint()
      ..style = PaintingStyle.stroke
      ..strokeWidth = 0.7
      ..color = const Color(0xFF969696));
  }

  @override
  bool shouldRepaint(_TableThumb o) => o.p != p;
}

class _GridlinePainter extends CustomPainter {
  _GridlinePainter(this.g, this.on, this.look);
  final int g;
  final bool on;
  final Look look;
  @override
  void paint(Canvas c, Size size) {
    final b = Rect.fromCenter(center: size.center(Offset.zero), width: 20, height: 16);
    final thin = Paint()
      ..style = PaintingStyle.stroke
      ..strokeWidth = 1
      ..color = look.gray(0x90);
    final bold = Paint()
      ..strokeWidth = 2
      ..color = on ? Colors.white : look.gray(0xee);
    c.drawRRect(RRect.fromRectAndRadius(b.deflate(0.5), const Radius.circular(1)), thin);
    final x1 = b.left + b.width / 3, x2 = b.left + 2 * b.width / 3, y1 = b.top + b.height / 3, y2 = b.top + 2 * b.height / 3;
    switch (g) {
      case 0:
        c.drawLine(Offset(b.left, y1), Offset(b.right, y1), bold);
        c.drawLine(Offset(b.left, y2), Offset(b.right, y2), bold);
      case 1:
        c.drawLine(Offset(x1, b.top), Offset(x1, b.bottom), bold);
        c.drawLine(Offset(x2, b.top), Offset(x2, b.bottom), bold);
      case 2:
        c.drawLine(Offset(b.left, y1), Offset(b.right, y1), bold);
      case 3:
        c.drawLine(Offset(x1, b.top), Offset(x1, b.bottom), bold);
      default:
        c.drawLine(Offset(b.left, y2), Offset(b.right, y2), bold);
    }
  }

  @override
  bool shouldRepaint(_GridlinePainter o) => o.on != on;
}

// ---------------------------------------------------------------- Cell (tblinsp cell_panel)

class CellPane extends StatelessWidget {
  const CellPane({super.key, required this.s, required this.ins});
  final Map<String, dynamic> s;
  final InspectorState ins;
  void put(String k, Object? v) => ins.set('cell.$k', v);

  static const _kinds = ['Automatic', 'Number', 'Currency', 'Percentage', 'Fraction', 'Numeral System', 'Scientific', 'Text', 'Date & Time', 'Duration'];

  Map<String, Object?> _new(int k) => switch (k) {
        1 => {'kind': 1, 'decimals': null, 'thousands': false, 'negative': 0},
        2 => {'kind': 2, 'code': 'USD', 'decimals': 2, 'thousands': false, 'negative': 0, 'accounting': false},
        3 => {'kind': 3, 'decimals': null, 'thousands': false, 'negative': 0},
        4 => {'kind': 4, 'accuracy': 4294967293},
        5 => {'kind': 5, 'base': 2, 'places': 0, 'minus': true},
        6 => {'kind': 6, 'decimals': 2},
        7 => {'kind': 7},
        8 => {'kind': 8, 'date': 'M/d/yy', 'time': 'h:mm a'},
        9 => {'kind': 9, 'style': 1, 'largest': 1, 'smallest': 32, 'automatic': true},
        _ => {'kind': 0},
      };

  @override
  Widget build(BuildContext context) {
    final look = LookScope.of(context);
    final f = Map<String, Object?>.from((s['format'] as Map?) ?? {'kind': 0});
    final kind = (f['kind'] as int?) ?? 0;
    void setF(Map<String, Object?> changes) => put('format', {...f, ...changes});
    List<Widget> decimalsRow({bool negative = true}) {
      final dec = f['decimals'] as int?;
      final neg = (f['negative'] as int?) ?? 0;
      const styles = [('-100', false), ('100', true), ('(100)', false), ('(100)', true)];
      TextStyle st(int i) => Look.font(13, styles[i].$2 ? const Color(0xFFFF453A) : look.text);
      return [
        _hrow([
          const DLabel('Decimals'),
          if (dec == null)
            DButton(width: 60, height: 22, onTap: () => setF({'decimals': 2}), child: const Text('Auto'))
          else ...[
            DStepper(value: dec.toDouble(), min: 0, max: 10, onChanged: (v) => setF({'decimals': v.round()})),
            DButton(height: 18, onTap: () => setF({'decimals': null}), child: const Text('Auto')),
          ],
        ]),
        if (negative) DRow('Negative Numbers', DCombo<int>(text: styles[neg.clamp(0, 3)].$1, items: () => [for (var i = 0; i < 4; i++) DChoice(styles[i].$1, i, on: i == neg, style: st(i))], onPick: (i) => setF({'negative': i}), width: 90)),
      ];
    }

    final out = <Widget>[
      ...dSection('Data Format'),
      DCombo<int>(text: kind >= 0 && kind < _kinds.length ? _kinds[kind] : 'Custom', items: () => [for (final (i, n) in _kinds.indexed) DChoice(n, i, on: i == kind)], onPick: (k) {
        if (k != kind) put('format', _new(k));
      }),
    ];
    switch (kind) {
      case 1 || 3:
        out.addAll([...decimalsRow(), DCheckbox(on: f['thousands'] == true, label: 'Thousands Separator', onChanged: (v) => setF({'thousands': v}))]);
      case 2:
        final code = f['code'] as String? ?? 'USD';
        out.addAll([
          ...decimalsRow(),
          DCheckbox(on: f['thousands'] == true, label: 'Thousands Separator', onChanged: (v) => setF({'thousands': v})),
          DCheckbox(on: f['accounting'] == true, label: 'Accounting Style', onChanged: (v) => setF({'accounting': v})),
          _hrow([const DLabel('Currency'), Expanded(child: DCombo<String>(text: code, items: () => [for (final c in (s['currencies'] as List? ?? [])) DChoice(c as String, c, on: c == code)], onPick: (c) => setF({'code': c})))]),
        ]);
      case 6:
        out.addAll(decimalsRow(negative: false));
      case 4:
        final acc = f['accuracy'];
        final fr = [for (final x in (s['fractions'] as List? ?? [])) (x[0] as int, x[1] as String)];
        out.add(_hrow([const DLabel('Accuracy'), Expanded(child: DCombo<int>(text: fr.where((x) => x.$1 == acc).map((x) => x.$2).firstOrNull ?? 'Up to 3 digits (445/553)', items: () => [for (final x in fr) DChoice(x.$2, x.$1, on: x.$1 == acc)], onPick: (a) => setF({'accuracy': a})))]));
      case 5:
        out.addAll([
          DRow('Base', DStepper(value: ((f['base'] as num?) ?? 2).toDouble(), min: 2, max: 36, onChanged: (v) => setF({'base': v.round()}))),
          DRow('Places', DStepper(value: ((f['places'] as num?) ?? 0).toDouble(), min: 0, max: 32, onChanged: (v) => setF({'places': v.round()}))),
          DCheckbox(on: f['minus'] == true, label: 'Negative numbers with a minus sign', onChanged: (v) => setF({'minus': v})),
        ]);
      case 8:
        final dates = [for (final x in (s['dates'] as List? ?? [])) (x[0] as String, x[1] as String)];
        final times = [for (final x in (s['times'] as List? ?? [])) (x[0] as String, x[1] as String)];
        String label(List<(String, String)> l, Object? p) => l.where((x) => x.$1 == p).map((x) => x.$2).firstOrNull ?? 'None';
        out.addAll([
          _hrow([const DLabel('Date'), Expanded(child: DCombo<String?>(text: label(dates, f['date']), items: () => [DChoice<String?>('None', null, on: f['date'] == null), for (final x in dates) DChoice<String?>(x.$2, x.$1, on: x.$1 == f['date'])], onPick: (p) => setF({'date': p})))]),
          _hrow([const DLabel('Time'), Expanded(child: DCombo<String?>(text: label(times, f['time']), items: () => [DChoice<String?>('None', null, on: f['time'] == null), for (final x in times) DChoice<String?>(x.$2, x.$1, on: x.$1 == f['time'])], onPick: (p) => setF({'time': p})))]),
        ]);
      case 9:
        const styles = ['0:00:00', '0h 0m 0s', '0 hours 0 minutes 0 seconds'];
        const units = [(1, 'Weeks'), (2, 'Days'), (4, 'Hours'), (8, 'Minutes'), (16, 'Seconds'), (32, 'Milliseconds')];
        String unit(Object? u) => units.where((x) => x.$1 == u).map((x) => x.$2).firstOrNull ?? 'Days';
        final st = ((f['style'] as int?) ?? 1).clamp(0, 2);
        out.addAll([
          _hrow([const DLabel('Format'), Expanded(child: DCombo<int>(text: styles[st], items: () => [for (final (i, n) in styles.indexed) DChoice(n, i, on: i == st)], onPick: (i) => setF({'style': i})))]),
          DCheckbox(on: f['automatic'] == true, label: 'Automatic Units', onChanged: (v) => setF({'automatic': v})),
          if (f['automatic'] != true)
            _hrow([
              DCombo<int>(width: 110, text: unit(f['largest']), items: () => [for (final u in units) DChoice(u.$2, u.$1, on: u.$1 == f['largest'])], onPick: (u) => setF({'largest': u})),
              const DLabel('to'),
              DCombo<int>(width: 110, text: unit(f['smallest']), items: () => [for (final u in units) DChoice(u.$2, u.$1, on: u.$1 == f['smallest'])], onPick: (u) => setF({'smallest': u})),
            ]),
        ]);
    }
    final edges = s['edges'] as int?;
    final line = s['line'] as Map<String, dynamic>?;
    const lineNames = ['Solid', 'Dashed', 'Dotted', 'No Border'];
    final lineCur = line == null ? 3 : lineNames.indexOf(line['dash'] as String? ?? 'Solid').clamp(0, 2);
    out.addAll([
      ...dSection('Fill'),
      FillEditor(fill: (s['fill'] as Map<String, dynamic>?) ?? {'kind': 0}, onChanged: (v) => put('fill', v)),
      ...dSection('Border'),
      // pick which borders (a 3 × 3 grid), then their line
      Padding(
        padding: const EdgeInsets.symmetric(vertical: 8),
        child: Center(
          child: Column(children: [
            for (var row = 0; row < 3; row++)
              Padding(
                padding: EdgeInsets.only(bottom: row < 2 ? 6 : 0),
                child: Row(mainAxisSize: MainAxisSize.min, children: [
                  for (var col = 0; col < 3; col++)
                    Padding(
                      padding: EdgeInsets.only(right: col < 2 ? 6 : 0),
                      child: GestureDetector(
                        onTap: () => put('edges', row * 3 + col),
                        child: Container(
                          width: 52,
                          height: 40,
                          decoration: BoxDecoration(color: edges == row * 3 + col ? look.accent : null, borderRadius: BorderRadius.circular(10)),
                          child: CustomPaint(painter: _EdgesPainter(row * 3 + col, edges == row * 3 + col, look)),
                        ),
                      ),
                    ),
                ]),
              ),
          ]),
        ),
      ),
      DCombo<int>(text: lineNames[lineCur], items: () => [for (final (i, n) in lineNames.indexed) DChoice(n, i, on: i == lineCur)], onPick: (i) {
        final base = line ?? {'color': [0.0, 0.0, 0.0, 1.0], 'width': 1.0};
        put('line', i == 3 ? null : {...base, 'dash': lineNames[i]});
      }),
      if (line != null) ...[
        DRow('Color', DColorWell(color: colorOf(line['color'], Colors.black), onChanged: (c) => put('line', {...line, 'color': colorTo(c)}))),
        DRow('Width', DStepper(value: (line['width'] as num?)?.toDouble() ?? 1, step: 0.5, min: 0.5, max: 20, suffix: ' pt', decimals: 1, onChanged: (w) => put('line', {...line, 'width': w}))),
      ],
      const DNote('Pick which borders, then their line.'),
      ...dSection('Conditional Highlighting'),
      DButton(height: 24, onTap: () => put('conditional', true), child: const Text('Conditional Highlighting…')),
    ]);
    return EguiColumn(children: out);
  }
}

class _EdgesPainter extends CustomPainter {
  _EdgesPainter(this.e, this.on, this.look);
  final int e;
  final bool on;
  final Look look;
  @override
  void paint(Canvas c, Size size) {
    // All, Inside, Outside / Left, Center, Right / Top, Middle, Bottom
    final b = Rect.fromCenter(center: size.center(Offset.zero), width: 14, height: 14);
    final thin = Paint()
      ..strokeWidth = 1
      ..color = look.gray(0x80);
    final bold = Paint()
      ..strokeWidth = 2
      ..color = on ? Colors.white : look.gray(0xee);
    final cx = b.center.dx, cy = b.center.dy;
    final lines = [
      ((b.topLeft, b.topRight), e == 0 || e == 2 || e == 6),
      ((b.bottomLeft, b.bottomRight), e == 0 || e == 2 || e == 8),
      ((b.topLeft, b.bottomLeft), e == 0 || e == 2 || e == 3),
      ((b.topRight, b.bottomRight), e == 0 || e == 2 || e == 5),
      ((Offset(b.left, cy), Offset(b.right, cy)), e == 0 || e == 1 || e == 7),
      ((Offset(cx, b.top), Offset(cx, b.bottom)), e == 0 || e == 1 || e == 4),
    ];
    for (final ((a, z), _) in lines) {
      c.drawLine(a, z, thin);
    }
    for (final ((a, z), strong) in lines) {
      if (strong) c.drawLine(a, z, bold);
    }
  }

  @override
  bool shouldRepaint(_EdgesPainter o) => o.on != on;
}

// ---------------------------------------------------------------- Movie / Audio / Gallery / Web Video (mediainsp)

class MoviePane extends StatelessWidget {
  const MoviePane({super.key, required this.s, required this.ins});
  final Map<String, dynamic> s;
  final InspectorState ins;
  void put(String k, Object? v) => ins.set('media.$k', v);
  double d(String k) => (s[k] as num?)?.toDouble() ?? 0;

  @override
  Widget build(BuildContext context) {
    final look = LookScope.of(context);
    final len = d('length');
    const names = ['None', 'Loop', 'Loop Back and Forth'];
    final cur = ((s['loop'] as int?) ?? 0).clamp(0, 2);
    return EguiColumn(children: [
      ...dSection(s['audioOnly'] == true ? 'Audio' : 'Movie'),
      const DLabel('Volume', bold: true),
      Row(children: [
        Expanded(child: DSlider(value: d('volume') * 100, min: 0, max: 100, showValue: false, onChanged: (v) => put('volume', v / 100))),
        SizedBox(width: 56, child: Text(' ${(d('volume') * 100).round()}%', style: Look.font(15, look.textDim))),
      ]),
      const DLabel('Repeat', bold: true),
      DCombo<int>(text: names[cur], items: () => [for (final (i, n) in names.indexed) DChoice(n, i, on: i == cur)], onPick: (i) => put('loop', i)),
      if (len > 0) ...[
        const DLabel('Trim', bold: true),
        Row(children: [Expanded(child: DSlider(value: d('start'), min: 0, max: len, suffix: ' s', onChanged: (a) => put('trim', [a, d('end')]))), const DLabel(' Start')]),
        Row(children: [Expanded(child: DSlider(value: d('end'), min: 0, max: len, suffix: ' s', onChanged: (b) => put('trim', [d('start'), b]))), const DLabel(' End')]),
        if (s['audioOnly'] != true) ...[
          const DLabel('Poster Frame', bold: true),
          DSlider(value: d('poster'), min: 0, max: len, suffix: ' s', onChanged: (t) => put('poster', t)),
        ],
      ] else
        const DNote('Trim and Poster Frame need ffmpeg to read the movie.'),
    ]);
  }
}

class GalleryPane extends StatefulWidget {
  const GalleryPane({super.key, required this.s, required this.ins});
  final Map<String, dynamic> s;
  final InspectorState ins;
  @override
  State<GalleryPane> createState() => _GalleryPaneState();
}

class _GalleryPaneState extends State<GalleryPane> {
  final Map<int, ui.Image?> _thumbs = {};
  Map<String, dynamic> get s => widget.s;
  void put(String k, Object? v) => widget.ins.set('media.$k', v);

  @override
  Widget build(BuildContext context) {
    final look = LookScope.of(context);
    final items = [for (final i in (s['items'] as List? ?? [])) i as Map<String, dynamic>];
    final shown = s['shown'] as int? ?? 0;
    final mode = s['mode'] as int? ?? 1;
    for (final it in items) {
      final d = it['data'] as int?;
      if (d != null && !_thumbs.containsKey(d)) {
        _thumbs[d] = null;
        widget.ins.engine.thumbnail(d, max: 96).then((t) => mounted ? setState(() => _thumbs[d] = t) : null);
      }
    }
    return EguiColumn(children: [
      ...dSection('Appearance'),
      DCheckbox(on: s['caption'] == true, label: 'Caption', onChanged: (v) => put('caption', v)),
      if (s['caption'] == true)
        Padding(
          padding: const EdgeInsets.only(left: 18),
          child: Column(crossAxisAlignment: CrossAxisAlignment.start, children: [
            DRadio(on: mode == 1, label: 'Individual caption per image', onTap: () => put('mode', 1)),
            const SizedBox(height: 6),
            DRadio(on: mode == 2, label: 'Same caption for all images', onTap: () => put('mode', 2)),
          ]),
        ),
      ...dSection('Images'),
      Container(
        constraints: const BoxConstraints(minHeight: 180),
        margin: const EdgeInsets.symmetric(vertical: 6),
        padding: const EdgeInsets.all(4),
        child: Column(children: [
          for (final (k, it) in items.indexed)
            GestureDetector(
              onTap: () => put('show', k),
              child: Container(
                height: 52,
                decoration: BoxDecoration(color: k == shown ? touchFill(look) : null, borderRadius: BorderRadius.circular(10)),
                child: Row(children: [
                  const SizedBox(width: 8),
                  SizedBox(
                    width: 44,
                    height: 32,
                    child: _thumbs[it['data']] == null ? Container(decoration: BoxDecoration(color: look.gray(0x50), borderRadius: BorderRadius.circular(2))) : RawImage(image: _thumbs[it['data']], fit: BoxFit.fill),
                  ),
                  const SizedBox(width: 10),
                  Expanded(child: DLabel(it['name'] as String? ?? '')),
                ]),
              ),
            ),
        ]),
      ),
      Row(children: [
        DButton(
          onTap: () async {
            final r = await FilePicker.pickFiles(type: FileType.image);
            final paths = [for (final f in r) if (f.path != null) f.path!];
            if (paths.isNotEmpty) put('add', paths);
          },
          child: const Text('+'),
        ),
        const SizedBox(width: 8),
        DButton(onTap: items.length > 1 ? () => put('remove', true) : null, child: const Text('−')),
      ]),
      ...dSection('Image Description'),
      if (shown < items.length) _MultiField(key: ValueKey(shown), text: items[shown]['description'] as String? ?? '', rows: 4, onDone: (t) => put('description', t)),
    ]);
  }
}

class WebVideoPane extends StatelessWidget {
  const WebVideoPane({super.key, required this.s, required this.ins});
  final Map<String, dynamic> s;
  final InspectorState ins;
  @override
  Widget build(BuildContext context) {
    final look = LookScope.of(context);
    final title = s['title'] as String? ?? '';
    return EguiColumn(children: [
      const DSpace(6),
      DLabel(title.isEmpty ? 'Web Video' : title, size: 15, bold: true),
      if ((s['author'] as String? ?? '').isNotEmpty) DLabel(s['author'] as String, dim: true),
      Text(s['url'] as String? ?? '', maxLines: 2, overflow: TextOverflow.ellipsis, style: Look.font(13, look.accent)),
      DButton(height: 26, onTap: () => ins.set('media.replaceWebVideo', true), child: const Text('Replace')),
      const DNote('Third party content and services may be subject to additional terms and conditions.'),
    ]);
  }
}

/// A multi-line text field committed when it loses focus (egui's multiline TextEdit).
class _MultiField extends StatefulWidget {
  const _MultiField({super.key, required this.text, required this.rows, required this.onDone});
  final String text;
  final int rows;
  final void Function(String) onDone;
  @override
  State<_MultiField> createState() => _MultiFieldState();
}

class _MultiFieldState extends State<_MultiField> {
  late final _c = TextEditingController(text: widget.text);
  final _f = FocusNode();
  @override
  void initState() {
    super.initState();
    _f.addListener(() {
      if (!_f.hasFocus && _c.text != widget.text) widget.onDone(_c.text);
      setState(() {});
    });
  }

  @override
  void dispose() {
    _c.dispose();
    _f.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final look = LookScope.of(context);
    return Container(
      margin: const EdgeInsets.symmetric(vertical: 6),
      padding: const EdgeInsets.symmetric(vertical: 4),
      child: TextField(
        controller: _c,
        focusNode: _f,
        minLines: widget.rows,
        maxLines: widget.rows,
        style: Look.font(15, look.text),
        cursorColor: look.accent,
        decoration: InputDecoration(isDense: true, border: InputBorder.none, hintText: 'Describe this image for people who use assistive technology', hintStyle: Look.font(15, look.textDim)),
      ),
    );
  }
}
