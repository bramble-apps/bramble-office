// The other views (kn-app navigator.rs light_table / outline, app.rs presenter notes):
// Light Table (every slide as a big thumbnail in a grid: tap selects, double-tap opens it in
// the navigator view, hold and drag moves it), Outline (each slide's title and text, edited in
// place), and the presenter notes field under the slide.
import 'dart:async';
import 'dart:convert';
import 'dart:math' as math;
import 'dart:ui' as ui;

import 'package:flutter/material.dart';

import '../engine.dart';
import 'look.dart';

class LightTable extends StatefulWidget {
  const LightTable({super.key, required this.engine, required this.look, required this.s, required this.slide, required this.onChanged, this.topInset = 0});
  final Engine engine;
  final Look look;

  /// kd_navigator.
  final Map<String, dynamic> s;
  final Size slide;
  final VoidCallback onChanged;
  final double topInset;

  @override
  State<LightTable> createState() => _LightTableState();
}

class _LightTableState extends State<LightTable> {
  final _thumbs = <int, ui.Image>{};
  final _asked = <int>{};
  Future<void> _pending = Future.value();
  int? _dragFrom, _dragTo;

  @override
  void dispose() {
    for (final t in _thumbs.values) {
      t.dispose();
    }
    super.dispose();
  }

  ui.Image? _thumb(int i, int key, double w) {
    final t = _thumbs[key];
    if (!_asked.add(key)) return t;
    final pw = (w * 2).round(), ph = (pw * widget.slide.height / widget.slide.width).round();
    _pending = _pending.then((_) async {
      if (!mounted) return;
      final img = await widget.engine.render(i, pw / widget.slide.width, 0, 0, pw, ph);
      if (img != null && mounted) {
        setState(() {
          _thumbs.remove(key)?.dispose();
          _thumbs[key] = img;
        });
      }
    });
    return t;
  }

  @override
  Widget build(BuildContext context) {
    final look = widget.look;
    final slides = [for (final s in (widget.s['slides'] as List? ?? const [])) s as Map<String, dynamic>];
    final selected = {for (final s in (widget.s['selected'] as List? ?? const [])) (s as num).toInt()};
    const tw = 240.0;
    final th = tw * widget.slide.height / widget.slide.width;
    return ColoredBox(
      color: look.canvas,
      child: GridView.builder(
        padding: EdgeInsets.fromLTRB(20, widget.topInset + 20, 20, 20),
        gridDelegate: SliverGridDelegateWithMaxCrossAxisExtent(maxCrossAxisExtent: tw + 24, mainAxisExtent: th + 34, crossAxisSpacing: 4, mainAxisSpacing: 4),
        itemCount: slides.length,
        itemBuilder: (ctx, i) {
          final s = slides[i];
          final key = (s['key'] as num?)?.toInt() ?? i;
          final img = _thumb(i, key, tw);
          final on = selected.contains(i);
          final drop = _dragTo == i && _dragFrom != null && _dragFrom != i;
          return GestureDetector(
            onTap: () {
              widget.engine.action('slide:$i');
              widget.onChanged();
            },
            onDoubleTap: () {
              widget.engine.action('slide:$i');
              widget.engine.run('Navigator');
              widget.onChanged();
            },
            onLongPressStart: (_) => setState(() => _dragFrom = _dragTo = i),
            onLongPressMoveUpdate: (d) {
              final box = ctx.findRenderObject() as RenderBox?;
              if (box == null) return;
              final cols = ((MediaQuery.of(context).size.width - 40) / (tw + 28)).floor().clamp(1, 99);
              final dx = (d.offsetFromOrigin.dx / (tw + 28)).round(), dy = (d.offsetFromOrigin.dy / (th + 38)).round();
              setState(() => _dragTo = (i + dx + dy * cols).clamp(0, slides.length - 1));
            },
            onLongPressEnd: (_) {
              final f = _dragFrom, t = _dragTo;
              setState(() => _dragFrom = _dragTo = null);
              if (f != null && t != null && f != t) {
                widget.engine.action('move:$f:$t');
                widget.onChanged();
              }
            },
            child: Opacity(
              opacity: s['skipped'] == true ? 0.45 : (_dragFrom == i ? 0.6 : 1),
              child: Column(children: [
                Container(
                  width: tw,
                  height: th,
                  decoration: BoxDecoration(
                    color: Colors.white,
                    border: Border.all(color: on || drop ? look.accent : look.gray(0x50), width: on || drop ? 3 : 1),
                    borderRadius: BorderRadius.circular(3),
                  ),
                  child: img == null ? null : RawImage(image: img, fit: BoxFit.fill),
                ),
                const SizedBox(height: 6),
                Text('${i + 1}', style: Look.font(13, on ? look.text : look.textDim)),
              ]),
            ),
          );
        },
      ),
    );
  }
}

class OutlineView extends StatefulWidget {
  const OutlineView({super.key, required this.engine, required this.look, required this.revision, required this.current, required this.onChanged, this.topInset = 0});
  final Engine engine;
  final Look look;
  final int revision, current;
  final VoidCallback onChanged;
  final double topInset;
  @override
  State<OutlineView> createState() => _OutlineViewState();
}

class _OutlineViewState extends State<OutlineView> {
  List<dynamic> _o = const [];
  final _fields = <int, TextEditingController>{};

  @override
  void initState() {
    super.initState();
    _o = widget.engine.outline();
  }

  @override
  void didUpdateWidget(OutlineView old) {
    super.didUpdateWidget(old);
    if (old.revision != widget.revision) _o = widget.engine.outline();
  }

  @override
  void dispose() {
    for (final c in _fields.values) {
      c.dispose();
    }
    super.dispose();
  }

  Widget _field(int slide, Map<String, dynamic> t, {required bool title}) {
    final st = (t['storage'] as num).toInt();
    final c = _fields.putIfAbsent(st, () => TextEditingController(text: t['text'] as String));
    final look = widget.look;
    return Focus(
      onFocusChange: (on) {
        if (on) {
          widget.engine.action('slide:$slide');
          widget.onChanged();
        } else if (c.text != t['text']) {
          widget.engine.action('outline:${jsonEncode({'storage': st, 'slide': slide, 'text': c.text})}');
          widget.onChanged();
        }
      },
      child: TextField(
        controller: c,
        maxLines: title ? 1 : null,
        style: Look.font(title ? 18 : 15, look.text, bold: title),
        cursorColor: look.accent,
        decoration: InputDecoration(isDense: true, border: InputBorder.none, hintText: title ? 'Title' : 'Text', hintStyle: Look.font(title ? 18 : 15, look.textDim)),
      ),
    );
  }

  @override
  Widget build(BuildContext context) {
    final look = widget.look;
    return ColoredBox(
      color: look.panel,
      child: ListView(
        padding: EdgeInsets.fromLTRB(16, widget.topInset + 16, 16, 40),
        children: [
          for (final s in _o.cast<Map<String, dynamic>>())
            Padding(
              padding: const EdgeInsets.only(bottom: 10),
              child: Row(crossAxisAlignment: CrossAxisAlignment.start, children: [
                SizedBox(
                  width: 56,
                  child: Row(children: [
                    Text('${(s['slide'] as num).toInt() + 1}', style: Look.font(13, look.textDim)),
                    const SizedBox(width: 6),
                    Icon(Icons.crop_landscape, size: 18, color: s['slide'] == widget.current ? look.accent : look.textDim),
                  ]),
                ),
                Expanded(
                  child: Column(crossAxisAlignment: CrossAxisAlignment.start, children: [
                    if (s['title'] != null) _field(s['slide'] as int, s['title'] as Map<String, dynamic>, title: true) else Text('(no title)', style: Look.font(16, look.textDim).copyWith(fontStyle: FontStyle.italic)),
                    for (final t in (s['texts'] as List).cast<Map<String, dynamic>>()) Padding(padding: const EdgeInsets.only(left: 8), child: _field(s['slide'] as int, t, title: false)),
                  ]),
                ),
              ]),
            ),
        ],
      ),
    );
  }
}

/// View > Show Presenter Notes: the current slide's notes under it, typed into directly.
class NotesPanel extends StatefulWidget {
  const NotesPanel({super.key, required this.engine, required this.look, required this.current, required this.revision, required this.onHide});
  final Engine engine;
  final Look look;
  final int current, revision;
  /// dragged off the foot (View > Hide Presenter Notes)
  final VoidCallback onHide;
  @override
  State<NotesPanel> createState() => _NotesPanelState();
}

class _NotesPanelState extends State<NotesPanel> {
  final _c = TextEditingController();
  Timer? _save;
  int _slide = -1;

  void _read() {
    _slide = widget.current;
    _c.text = widget.engine.notes()['text'] as String? ?? '';
  }

  @override
  void initState() {
    super.initState();
    _read();
  }

  @override
  void didUpdateWidget(NotesPanel old) {
    super.didUpdateWidget(old);
    if (widget.current != _slide) {
      _flush();
      _read();
    }
  }

  void _flush() {
    if (_save?.isActive == true) {
      _save!.cancel();
      widget.engine.action('notes:${_c.text}');
    }
  }

  @override
  void dispose() {
    _flush();
    _c.dispose();
    super.dispose();
  }

  (Look, Widget)? _field;

  Widget _textField(Look look) => Padding(
        padding: const EdgeInsets.fromLTRB(16, 0, 16, 8),
        child: TextField(
          controller: _c,
          maxLines: null,
          expands: true,
          style: Look.font(15, look.text),
          cursorColor: look.accent,
          decoration: InputDecoration(border: InputBorder.none, hintText: 'Click to add notes', hintStyle: Look.font(15, look.textDim)),
          onChanged: (_) {
            _save?.cancel();
            _save = Timer(const Duration(milliseconds: 600), () => widget.engine.action('notes:${_c.text}'));
          },
        ),
      );

  // (the desktop's notes splitter: 60 to 400 points; kept while the app runs)
  static double _height = 150;
  double? _dragFrom;

  @override
  Widget build(BuildContext context) {
    final look = widget.look;
    final max = math.min(400.0, MediaQuery.of(context).size.height * 0.6);
    final h = _height.clamp(60.0, max);
    // the top edge: a grabber with touch-sized room; dragged to the foot, the notes hide
    final handle = GestureDetector(
      behavior: HitTestBehavior.opaque,
      onVerticalDragStart: (d) => _dragFrom = h,
      onVerticalDragUpdate: (d) => setState(() {
        _dragFrom = (_dragFrom ?? h) - d.delta.dy;
        _height = _dragFrom!.clamp(60.0, max);
      }),
      onVerticalDragEnd: (_) {
        final hide = (_dragFrom ?? h) < 40;
        _dragFrom = null;
        if (hide) {
          _height = 150;
          _flush();
          widget.onHide();
        }
      },
      child: MouseRegion(
        cursor: SystemMouseCursors.resizeRow,
        child: Container(
          height: 22,
          alignment: Alignment.center,
          decoration: BoxDecoration(border: Border(top: BorderSide(color: look.separator))),
          child: Container(width: 36, height: 4, decoration: BoxDecoration(color: look.textDim.withValues(alpha: 0.5), borderRadius: BorderRadius.circular(2))),
        ),
      ),
    );
    final dragging = _dragFrom != null && _dragFrom! < 60;
    return Container(
      height: (dragging ? _dragFrom!.clamp(22.0, 60.0) : h) + 22,
      color: look.panel,
      child: Column(children: [
        handle,
        Expanded(
          // (the same widget each build while the look stays: a resize lays it out again
          // without building it again)
          child: _field?.$1 == look ? _field!.$2 : (_field = (look, _textField(look))).$2,
        ),
      ]),
    );
  }
}
