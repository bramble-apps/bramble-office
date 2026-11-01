// The slide navigator (kn-app navigator.rs): the slides as thumbnails down the window's left,
// numbered, the selected ones on a grey row, skipped slides as thin bars, groups indented.
// Tap goes to a slide (with Shift, adds it), touch and hold opens its menu, touch, hold and drag
// moves it. In the phone layout it runs along the foot of the window instead (`horizontal`).
// The thumbnails are the engine's pictures of the slides (kd_render), kept by their key.
import 'dart:ui' as ui;

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';

import '../engine.dart';
import 'look.dart';
import 'touch_menu.dart';

class Navigator2 extends StatefulWidget {
  const Navigator2({super.key, required this.engine, required this.look, required this.s, required this.slide, required this.onChanged, required this.onAction, this.horizontal = false, this.topInset = 0});
  final Engine engine;
  final Look look;

  /// kd_navigator: {slides, current, selected}.
  final Map<String, dynamic> s;

  /// The slides' size (points).
  final Size slide;
  final VoidCallback onChanged;

  /// A menu item's action (kd_action / "cmd:…").
  final void Function(String action) onAction;
  final bool horizontal;
  final double topInset;

  @override
  State<Navigator2> createState() => _NavigatorState();
}

class _NavigatorState extends State<Navigator2> {
  Engine get e => widget.engine;
  final _thumbs = <int, ui.Image>{};
  final _asked = <int>{};
  final _scroll = ScrollController();
  int? _dragFrom, _dragTo;
  int _shownCurrent = -1;

  List<Map<String, dynamic>> get _slides => [for (final s in (widget.s['slides'] as List? ?? const [])) s as Map<String, dynamic>];

  @override
  void dispose() {
    _scroll.dispose();
    for (final t in _thumbs.values) {
      t.dispose();
    }
    super.dispose();
  }

  /// A slide's thumbnail, asked of the engine the first time its key is seen.
  ui.Image? _thumb(int index, int key, double w, double dpr) {
    final t = _thumbs[key];
    if (t != null) return t;
    if (_asked.add(key)) {
      final pw = (w * dpr).round().clamp(40, 600), ph = (pw * widget.slide.height / widget.slide.width).round();
      // (one at a time, in the order asked: a long deck fills in without stalling the window)
      _pending = _pending.then((_) async {
        if (!mounted) return;
        final img = await e.render(index, pw / widget.slide.width, 0, 0, pw, ph);
        if (img != null && mounted) setState(() => _thumbs[key] = img);
      });
    }
    return null;
  }

  Future<void> _pending = Future.value();

  void _tap(int i) {
    final shift = HardwareKeyboard.instance.isShiftPressed;
    e.action(shift ? 'slide:$i:shift' : 'slide:$i');
    widget.onChanged();
  }

  void _menu(int i, Rect at) {
    HapticFeedback.selectionClick();
    final items = e.contextMenu(i);
    widget.onChanged();
    showTouchMenu(context, widget.look, at, items, (it) => widget.onAction(it['action'] as String? ?? ''));
  }

  @override
  Widget build(BuildContext context) {
    final look = widget.look;
    final dpr = MediaQuery.of(context).devicePixelRatio;
    final slides = _slides;
    final current = (widget.s['current'] as num?)?.toInt() ?? 0;
    final selected = {for (final x in (widget.s['selected'] as List? ?? const [])) (x as num).toInt()};
    final aspect = widget.slide.height / widget.slide.width;
    final visible = [for (final s in slides) if (s['hidden'] != true) s];
    // (pictures of what the slides no longer show go)
    final keys = {for (final s in slides) (s['key'] as num).toInt()};
    for (final k in [..._thumbs.keys]) {
      if (!keys.contains(k)) _thumbs.remove(k)?.dispose();
    }
    _asked.retainWhere(keys.contains);
    return LayoutBuilder(builder: (ctx, box) {
      final horizontal = widget.horizontal;
      final thumbW = horizontal ? (box.maxHeight - 26) / aspect : (box.maxWidth - 44).clamp(60.0, 400.0);
      double rowExtent(Map<String, dynamic> s) => s['skipped'] == true ? (horizontal ? 30 : 18) : (horizontal ? thumbW + 30 : thumbW * aspect - (s['depth'] as int) * 12 * aspect + 14);
      // the current slide kept in view
      if (current != _shownCurrent && _scroll.hasClients) {
        _shownCurrent = current;
        var at = 0.0;
        for (final s in visible) {
          if (s['index'] == current) break;
          at += rowExtent(s);
        }
        final viewLen = horizontal ? box.maxWidth : box.maxHeight - widget.topInset;
        WidgetsBinding.instance.addPostFrameCallback((_) {
          if (!_scroll.hasClients) return;
          final off = _scroll.offset;
          final ext = visible.isEmpty ? 0.0 : rowExtent(visible.firstWhere((s) => s['index'] == current, orElse: () => visible.first));
          if (at < off || at + ext > off + viewLen) _scroll.animateTo((at - viewLen / 2 + ext / 2).clamp(0.0, _scroll.position.maxScrollExtent), duration: const Duration(milliseconds: 200), curve: Curves.easeOut);
        });
      }
      Widget row(Map<String, dynamic> s) {
        final i = s['index'] as int;
        final depth = (s['depth'] as int).toDouble();
        final sel = selected.contains(i);
        final key = (s['key'] as num).toInt();
        final skipped = s['skipped'] == true;
        final ext = rowExtent(s);
        final w = thumbW - (horizontal ? 0 : depth * 12);
        final img = skipped ? null : _thumb(i, key, w, dpr);
        final dropBefore = _dragFrom != null && _dragTo == i && _dragTo! < _dragFrom!;
        final dropAfter = _dragFrom != null && _dragTo == i && _dragTo! > _dragFrom!;
        Widget thumb = skipped
            ? Container(width: horizontal ? 8 : w, height: horizontal ? thumbW * aspect : 8, decoration: BoxDecoration(color: look.gray(140), borderRadius: BorderRadius.circular(2)))
            : Container(
                width: w,
                height: w * aspect,
                decoration: BoxDecoration(border: Border.all(color: look.gray(70)), color: const Color(0xFFFFFFFF)),
                child: img == null ? null : RawImage(image: img, fit: BoxFit.fill, filterQuality: FilterQuality.medium),
              );
        final number = Text('${i + 1}', style: Look.font(11, sel ? look.text : look.textDim));
        final content = horizontal
            ? Column(mainAxisAlignment: MainAxisAlignment.center, children: [thumb, const SizedBox(height: 3), number])
            : Row(crossAxisAlignment: CrossAxisAlignment.start, children: [
                SizedBox(width: 24 + depth * 12, child: Padding(padding: EdgeInsets.only(top: skipped ? 0 : w * aspect - 8, right: 4), child: Align(alignment: Alignment.topRight, child: number))),
                const SizedBox(width: 6),
                Padding(padding: EdgeInsets.only(top: skipped ? 3 : 5), child: thumb),
              ]);
        return Builder(builder: (rctx) {
          Rect at() {
            final b = rctx.findRenderObject() as RenderBox;
            return b.localToGlobal(Offset.zero) & b.size;
          }

          return GestureDetector(
            behavior: HitTestBehavior.opaque,
            onTap: () => _tap(i),
            onSecondaryTapUp: (_) => _menu(i, at()),
            onLongPressStart: (_) {
              HapticFeedback.selectionClick();
              setState(() {
                _dragFrom = i;
                _dragTo = i;
              });
            },
            onLongPressMoveUpdate: (d) {
              // where it would land: the row under the finger
              final list = context.findRenderObject() as RenderBox;
              final p = list.globalToLocal(d.globalPosition);
              var pos = (horizontal ? p.dx : p.dy - widget.topInset) + _scroll.offset;
              var to = visible.last['index'] as int;
              for (final v in visible) {
                final x = rowExtent(v);
                if (pos < x) {
                  to = v['index'] as int;
                  break;
                }
                pos -= x;
              }
              if (to != _dragTo) setState(() => _dragTo = to);
            },
            onLongPressEnd: (_) {
              final from = _dragFrom, to = _dragTo;
              setState(() => _dragFrom = _dragTo = null);
              if (from == null || to == null) return;
              if (from == to) {
                _menu(i, at());
              } else {
                e.action('move:$from:$to');
                widget.onChanged();
              }
            },
            child: Container(
              width: horizontal ? ext : null,
              height: horizontal ? null : ext,
              margin: horizontal ? const EdgeInsets.symmetric(vertical: 4) : const EdgeInsets.symmetric(horizontal: 4),
              decoration: BoxDecoration(
                color: _dragFrom == i ? look.accent.withValues(alpha: 0.25) : (sel ? (look.light ? const Color(0xFFD2D2D2) : look.gray(0x3a)) : null),
                borderRadius: BorderRadius.circular(6),
                border: Border(
                  top: !horizontal && dropBefore ? BorderSide(color: look.accent, width: 3) : BorderSide.none,
                  bottom: !horizontal && dropAfter ? BorderSide(color: look.accent, width: 3) : BorderSide.none,
                  left: horizontal && dropBefore ? BorderSide(color: look.accent, width: 3) : BorderSide.none,
                  right: horizontal && dropAfter ? BorderSide(color: look.accent, width: 3) : BorderSide.none,
                ),
              ),
              child: content,
            ),
          );
        });
      }

      return Container(
        color: look.translucent ? look.sidebar.withValues(alpha: 0.86) : look.sidebar,
        padding: EdgeInsets.only(top: horizontal ? 0 : widget.topInset),
        child: ListView(
          controller: _scroll,
          scrollDirection: horizontal ? Axis.horizontal : Axis.vertical,
          padding: horizontal ? const EdgeInsets.symmetric(horizontal: 6) : const EdgeInsets.only(top: 4, bottom: 40),
          children: [for (final s in visible) row(s)],
        ),
      );
    });
  }
}
