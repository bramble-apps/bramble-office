// The slide canvas (kn-app canvas.rs): the current slide on the canvas colour, fitted edge to
// edge under the floating toolbar, pinched to zoom and dragged to pan when zoomed in. The
// slide's pixels are the engine's (kd_render: the whole slide at the zoom, and the part in view
// sharper when zoomed in far); everything over it is drawn here from the engine's overlays
// (selection frames and handles, the caret, the text selection and its handles). Taps select
// and start typing, drags move, resize, rotate (two fingers on a handle) and move line ends,
// touch and hold shows the edit menu, as Keynote on iPad.
import 'dart:async';
import 'dart:convert';
import 'dart:math' as math;
import 'dart:ui' as ui;

import 'package:flutter/gestures.dart';
import 'package:flutter/material.dart';
import 'package:flutter/scheduler.dart';
import 'package:flutter/services.dart';

import '../engine.dart';
import 'edit_menu.dart';
import 'look.dart';
import 'touch_menu.dart';

class SlideView extends StatefulWidget {
  const SlideView({super.key, required this.engine, required this.look, required this.topInset, required this.onChanged, required this.onMenuAction, this.message, this.bottomInset = 0});
  final Engine engine;
  final Look look;

  /// The floating toolbar's band over the top of the canvas.
  final double topInset;

  /// The phone layout's inspector sheet over the foot of the canvas (the slide fits above it).
  final double bottomInset;

  /// The message pill's text (app.notify), shown at the foot.
  final String? message;

  /// After a change: the window reads the state again.
  final VoidCallback onChanged;

  /// An edit menu item's action ("cmd:…").
  final void Function(String action) onMenuAction;

  @override
  State<SlideView> createState() => SlideViewState();
}

/// A picture of the slide (or part of it) and the slide points it covers.
class _Pic {
  _Pic(this.image, this.rect, this.key, this.scale);
  final ui.Image image;
  final Rect rect;
  final int key;
  final double scale;
}

class SlideViewState extends State<SlideView> with TickerProviderStateMixin {
  Engine get e => widget.engine;
  final _repaint = ValueNotifier<int>(0);
  void _paint() => _repaint.value++;

  Size _view = Size.zero;
  double _dpr = 2;

  // the slide: its size (points), which one, what it shows
  Size _slide = const Size(1920, 1080);
  int _index = 0, _count = 0, _key = 0;
  Map<String, dynamic> _ov = {};

  // the view: zoom (logical pixels a slide point; null: fit) and the slide's top-left in the view
  double? _zoomSet;
  Offset _pan = Offset.zero;
  double? _pinchZoom;
  Offset _pinchFocalSlide = Offset.zero, _pinchFocal = Offset.zero;

  _Pic? _base, _detail, _under, _moving;

  /// The slide's dynamic background playing under it (Preview Motion, as the desktop's editor):
  /// the slide's pictures are drawn without it, and its frames come from the engine.
  bool _motion = false;
  ui.Image? _motionImg;
  late final Ticker _motionTicker = createTicker(_motionTick);
  final _motionClock = Stopwatch()..start();
  bool _motionBusy = false;
  int _motionAt = 0;

  /// The background being played, copied out of the engine for the worker isolate (a slide's
  /// own, until its key changes: an edit or another slide).
  int _motionHandle = 0, _motionHandleKey = -2;

  void _freeMotionHandle() {
    if (_motionHandle != 0) e.motionFree(_motionHandle);
    _motionHandle = 0;
    _motionHandleKey = -2;
  }

  Future<void> _motionTick(Duration _) async {
    if (!_motion || _motionBusy || _view.isEmpty) return;
    final now = _motionClock.elapsedMilliseconds;
    // (60 frames a second, drawn off the UI thread)
    if (now - _motionAt < 16) return;
    _motionAt = now;
    _motionBusy = true;
    try {
      if (_motionHandleKey != _key) {
        _freeMotionHandle();
        _motionHandle = e.motionHandle(_index);
        _motionHandleKey = _key;
      }
      final w = (_slide.width * zoom * _dpr / 2.5).clamp(320.0, 1024.0).round();
      final h = (w * _slide.height / _slide.width).round().clamp(2, 4096);
      final img = await e.motionOff(_motionHandle, now / 1000, w, h);
      if (!mounted) return img?.dispose();
      _motionImg?.dispose();
      _motionImg = img;
      _paint();
    } finally {
      _motionBusy = false;
    }
  }

  int get _bgMode => _motion ? 4 : 0;
  bool _rendering = false, _again = false;
  Timer? _settle;

  double get fitZoom {
    final h = _view.height - widget.topInset - widget.bottomInset;
    if (_view.width <= 0 || h <= 0) return 1;
    return math.min(_view.width / _slide.width, h / _slide.height);
  }

  double get zoom => _pinchZoom ?? _zoomSet ?? fitZoom;

  /// The slide's top-left in the view (fitted: in the middle of the room under the toolbar).
  Offset get origin {
    final z = zoom;
    final w = _slide.width * z, h = _slide.height * z;
    final room = Rect.fromLTWH(0, widget.topInset, _view.width, _view.height - widget.topInset - widget.bottomInset);
    if (_pinchZoom != null) return _pinchFocal - _pinchFocalSlide * z;
    if (_zoomSet == null) return Offset(room.left + (room.width - w) / 2, room.top + (room.height - h) / 2);
    return _clampPan(_pan, z);
  }

  Offset _clampPan(Offset p, double z) {
    final w = _slide.width * z, h = _slide.height * z;
    final room = Rect.fromLTWH(0, widget.topInset, _view.width, _view.height - widget.topInset - widget.bottomInset);
    double axis(double v, double size, double lo, double len) => size <= len ? lo + (len - size) / 2 : v.clamp(lo + len - size, lo);
    return Offset(axis(p.dx, w, room.left, room.width), axis(p.dy, h, room.top, room.height));
  }

  Offset toSlide(Offset local) => (local - origin) / zoom;
  Offset toLocal(double x, double y) => origin + Offset(x, y) * zoom;
  Offset _pt(dynamic p) => toLocal((p[0] as num).toDouble(), (p[1] as num).toDouble());

  /// A finger's reach in slide points.
  double get _tol => 22 / zoom;

  /// Whether the view shows the slide fitted (not zoomed in or out).
  bool get fitted => _zoomSet == null;

  void setZoom(double? z) {
    setState(() {
      _zoomSet = z == null || (z - fitZoom).abs() < 1e-3 ? null : z;
      if (_zoomSet != null) _pan = origin;
    });
    _queue();
  }

  @override
  void didUpdateWidget(SlideView old) {
    super.didUpdateWidget(old);
    if (old.bottomInset != widget.bottomInset || old.topInset != widget.topInset) _queue();
  }

  @override
  void initState() {
    super.initState();
    HardwareKeyboard.instance.addHandler(_keyHandler);
    WidgetsBinding.instance.addPostFrameCallback((_) {
      if (mounted) refresh();
    });
    _blink = Timer.periodic(const Duration(milliseconds: 530), (_) {
      if (_ov['editing'] is Map && (_ov['editing'] as Map)['caret'] != null) {
        _caretOn = !_caretOn;
        _paint();
      }
    });
  }

  @override
  void dispose() {
    HardwareKeyboard.instance.removeHandler(_keyHandler);
    _blink?.cancel();
    _settle?.cancel();
    _motionTicker.dispose();
    _cell.dispose();
    _cellFocus.dispose();
    _motionImg?.dispose();
    // (a frame still being drawn keeps its background until it's done)
    final h = _motionHandle;
    if (h != 0) Future<void>.delayed(const Duration(seconds: 1), () => e.motionFree(h));
    _focus.dispose();
    _input.dispose();
    super.dispose();
  }

  Timer? _blink;
  bool _caretOn = true;

  /// Reads the slide and the overlays again (after anything changed) and redraws what changed.
  void refresh() {
    final g = e.geometry();
    final size = g['size'] as List?;
    if (size != null) _slide = Size((size[0] as num).toDouble(), (size[1] as num).toDouble());
    final idx = (g['current'] as num?)?.toInt() ?? 0;
    if (idx != _index) {
      _zoomSet = null;
      _hideMenu();
    }
    _index = idx;
    _count = (g['count'] as num?)?.toInt() ?? 0;
    final key = (g['key'] as num?)?.toInt() ?? 0;
    final motion = g['motion'] == true;
    if (motion != _motion) {
      _motion = motion;
      _key = -1;
      _base?.image.dispose();
      _detail?.image.dispose();
      _base = _detail = null;
      _motionImg?.dispose();
      _motionImg = null;
      if (motion && !_motionTicker.isActive) _motionTicker.start();
      if (!motion && _motionTicker.isActive) _motionTicker.stop();
    }
    final wasDragging = _ov['dragging'] == true;
    final before = _selSig();
    _ov = e.overlays();
    if (_menu != null && _selSig() != _menuSig) _hideMenu();
    _caretOn = true;
    if (key != _key) {
      _key = key;
      _queue();
    }
    if (wasDragging != (_ov['dragging'] == true) || _ov['dragging'] == true) _queueLive();
    if (before != _selSig() && e.status()['editingText'] != true) hideKeyboard();
    widget.onChanged();
    _paint();
  }

  String _selSig() => jsonEncode([_ov['selection'], (_ov['editing'] as Map?)?['caret'], (_ov['editing'] as Map?)?['rects']]);

  // ---------------------------------------------------------------- drawing

  /// The scale pictures are drawn at: the zoom in device pixels, on whole thousandths.
  double get _scale => (zoom * _dpr * 1000).round() / 1000;

  /// Draws the slide again once things rest for `ms` (a resize: once its size has settled, the
  /// picture scaled meanwhile).
  void _queue({int ms = 30}) {
    _settle?.cancel();
    _settle = Timer(Duration(milliseconds: ms), _render);
  }

  /// The whole slide at the zoom (at most 4096 pixels across), and when zoomed in further the
  /// part in view at full sharpness.
  Future<void> _render() async {
    if (_rendering) {
      _again = true;
      return;
    }
    if (_count == 0 || _view.isEmpty) return;
    _rendering = true;
    try {
      final key = _key, idx = _index;
      final want = _scale;
      final baseScale = math.min(want, 4096 / _slide.width);
      if (_base == null || _base!.key != key || (_base!.scale - baseScale).abs() > 1e-3) {
        final w = (_slide.width * baseScale).ceil(), h = (_slide.height * baseScale).ceil();
        final img = await e.render(idx, baseScale, 0, 0, w, h, mode: _bgMode);
        if (img != null) {
          _base?.image.dispose();
          _base = _Pic(img, Offset.zero & _slide, key, baseScale);
        }
      }
      if (want > baseScale + 1e-3) {
        // the part in view (whole pixels), sharp
        final o = origin, z = zoom;
        final vis = Rect.fromLTWH(-o.dx / z, (widget.topInset - o.dy) / z, _view.width / z, (_view.height - widget.topInset) / z).intersect(Offset.zero & _slide);
        if (!vis.isEmpty && (_detail == null || _detail!.key != key || (_detail!.scale - want).abs() > 1e-3 || !_detail!.rect.contains(vis.topLeft) || !_detail!.rect.contains(vis.bottomRight - const Offset(0.01, 0.01)))) {
          final x0 = (vis.left * want).floorToDouble() / want, y0 = (vis.top * want).floorToDouble() / want;
          final w = ((vis.right - x0) * want).ceil(), h = ((vis.bottom - y0) * want).ceil();
          final img = await e.render(idx, want, x0, y0, w, h, mode: _bgMode);
            if (img != null) {
            _detail?.image.dispose();
            _detail = _Pic(img, Rect.fromLTWH(x0, y0, w / want, h / want), key, want);
          }
        }
      } else {
        _detail?.image.dispose();
        _detail = null;
      }
    } finally {
      _rendering = false;
    }
    _paint();
    if (_again) {
      _again = false;
      _queue();
    }
  }

  bool _liveBusy = false, _liveAgain = false;

  /// While objects are dragged: the slide without them (once) and them alone at their live
  /// places (each move).
  Future<void> _queueLive() async {
    if (_liveBusy) {
      _liveAgain = true;
      return;
    }
    _liveBusy = true;
    try {
      if (_ov['dragging'] != true || (_ov['moving'] as List? ?? const []).isEmpty) {
        _under?.image.dispose();
        _moving?.image.dispose();
        _under = _moving = null;
        _movingSel = null;
      } else {
        final s = math.min(_scale, 2048 / _slide.width);
        final w = (_slide.width * s).ceil(), h = (_slide.height * s).ceil();
        if (_under == null) {
          final img = await e.render(_index, s, 0, 0, w, h, mode: 1 + _bgMode);
          if (img != null) _under = _Pic(img, Offset.zero & _slide, 0, s);
        }
        // moved only (their frames the same size): the picture of them slides with them, on
        // the GPU; drawn again only when one is resized or turned
        final sel = _selBox();
        final was = _movingSel;
        if (_moving != null && was != null && sel != null && (sel.width - was.width).abs() < 0.01 && (sel.height - was.height).abs() < 0.01) return;
        // (them alone: just the part of the slide round where they are, with room for shadows
        // and a reflection below, rather than the whole slide each move)
        final part = _movingPart();
        final x0 = (part.left * s).floorToDouble() / s, y0 = (part.top * s).floorToDouble() / s;
        final pw = ((part.right - x0) * s).ceil(), ph = ((part.bottom - y0) * s).ceil();
        final img = pw > 0 && ph > 0 ? await e.render(_index, s, x0, y0, pw, ph, mode: 2) : null;
        _moving?.image.dispose();
        _moving = img == null ? null : _Pic(img, Rect.fromLTWH(x0, y0, pw / s, ph / s), 0, s);
        // (where they were when drawn: the picture's offset follows the selection from there)
        _movingSel = img == null ? null : sel;
      }
    } finally {
      _liveBusy = false;
    }
    _paint();
    if (_liveAgain) {
      _liveAgain = false;
      _queueLive();
    }
  }

  /// The selection's frames' bounds now (slide points), or null.
  Rect? _selBox() {
    Rect? r;
    for (final sel in (_ov['selection'] as List? ?? const [])) {
      for (final p in (sel['corners'] as List? ?? const [])) {
        final q = Rect.fromLTWH((p[0] as num).toDouble(), (p[1] as num).toDouble(), 0, 0);
        r = r == null ? q : r.expandToInclude(q);
      }
    }
    return r;
  }

  /// The selection's bounds when [_moving] was drawn.
  Rect? _movingSel;

  /// Where the objects being dragged are now (the selection's frames, in slide points), with
  /// room round them for text past their frames, shadows and a reflection below.
  Rect _movingPart() {
    Rect? r;
    for (final sel in (_ov['selection'] as List? ?? const [])) {
      for (final p in (sel['corners'] as List? ?? const [])) {
        final q = Offset((p[0] as num).toDouble(), (p[1] as num).toDouble());
        r = r == null ? Rect.fromLTWH(q.dx, q.dy, 0, 0) : r.expandToInclude(Rect.fromLTWH(q.dx, q.dy, 0, 0));
      }
    }
    if (r == null) return Offset.zero & _slide;
    // (text runs past its frame, a big title's well past it: a quarter of the slide's room
    // round, and off the slide too, as what's dragged can come in from outside it)
    final m = math.max(_slide.width, _slide.height) / 4;
    r = Rect.fromLTRB(r.left - m, r.top - m, r.right + m, r.bottom + m + r.height);
    return r.intersect((Offset.zero & _slide).inflate(m));
  }

  // ---------------------------------------------------------------- the toolbar's band

  static double _smoother(double t) => t * t * t * (t * (t * 6 - 15) + 10);

  List<Widget> _toolbarBand() {
    final look = widget.look;
    final h = widget.topInset;
    if (h <= 0) return [];
    switch (look.toolbarBackground) {
      case 'solid':
        return [Positioned(left: 0, right: 0, top: 0, height: h, child: IgnorePointer(child: ColoredBox(color: look.header)))];
      case 'feathered':
        // the slide drawn again, blurred, over the band, in three layers whose blur thins out
        // going down (progressive_blur): strong under the buttons, gentler, then nearly sharp
        // as it fades. The fade is the desktop's (glass.rs FEATHER_REACH, 40 points) but over
        // the band's foot, done where the toolbar ends rather than reaching onto the slide
        const reach = 40.0;
        Widget layer(double sigma, double fadeFrom, double fadeTo) => Positioned(
              left: 0,
              right: 0,
              top: 0,
              height: h,
              child: IgnorePointer(
                child: ClipRect(
                  child: ShaderMask(
                    blendMode: BlendMode.dstIn,
                    shaderCallback: (r) => LinearGradient(
                      begin: Alignment.topCenter,
                      end: Alignment.bottomCenter,
                      colors: [for (var k = 0; k <= 8; k++) Color.fromARGB((255 * (1 - _smoother(k / 8))).round(), 255, 255, 255)],
                      stops: [for (var k = 0; k <= 8; k++) (fadeFrom + (fadeTo - fadeFrom) * k / 8) / h],
                    ).createShader(r),
                    child: ImageFiltered(
                      imageFilter: ui.ImageFilter.blur(sigmaX: sigma, sigmaY: sigma, tileMode: TileMode.decal),
                      child: ClipRect(child: CustomPaint(painter: _SlidePainter(this, band: true), size: Size.infinite)),
                    ),
                  ),
                ),
              ),
            );
        double up(double y) => math.max(0, y - reach);
        return [
          layer(1.5, up(h), h),
          layer(4, up(h * 0.85), up(h + reach * 0.6)),
          layer(8, up(h * 0.6), up(h + reach * 0.25)),
        ];
      default:
        return [
          Positioned(
            left: 0,
            right: 0,
            top: 0,
            height: h,
            child: IgnorePointer(child: ClipRect(child: BackdropFilter(filter: ui.ImageFilter.blur(sigmaX: 12, sigmaY: 12), child: ColoredBox(color: look.toolbarTint)))),
          ),
        ];
    }
  }

  // ---------------------------------------------------------------- touches

  int _taps = 0;
  DateTime _lastTap = DateTime(0);
  Offset _lastTapAt = Offset.zero;
  Offset _downAt = Offset.zero;

  /// What this touch does: 'object' (the engine drags), 'text0'/'text1' (a text handle: the
  /// selection's start / end), 'pan', or null.
  String? _gesture;

  List<(bool, Offset, Offset)> _textHandles() {
    final ed = _ov['editing'];
    if (ed is! Map) return [];
    final ends = ed['ends'];
    if (ends is! List || ends.length < 2 || ends[0] == null || ends[1] == null) return [];
    final a = ends[0] as List, b = ends[1] as List;
    final at = _pt(a[0]), ab = _pt(a[1]), bt = _pt(b[0]), bb = _pt(b[1]);
    // (kn_ui touch.rs knobs: 7 above the start, 7 below the end)
    return [(true, at - const Offset(0, 7), Offset(at.dx, (at.dy + ab.dy) / 2)), (false, bb + const Offset(0, 7), Offset(bt.dx, (bt.dy + bb.dy) / 2))];
  }

  /// View > Show Rulers: a ruler along the top and the left of the view, in Settings' units,
  /// following the slide's zoom; pulling from one brings out a guide (kd_drag moves it).
  List<Widget> _rulers() {
    final gd = _ov['guides'] as Map?;
    if (gd?['rulers'] != true) return const [];
    final units = (gd?['units'] as num?)?.toInt() ?? 0;
    const t = 20.0;
    final top = widget.topInset;
    Widget ruler(bool horiz) => GestureDetector(
          behavior: HitTestBehavior.opaque,
          onPanStart: (d) {
            final p = toSlide(d.localPosition + (horiz ? Offset(t, top) : Offset(0, top + t)));
            e.action(horiz ? 'guide:h:${p.dy}' : 'guide:v:${p.dx}');
            refresh();
          },
          onPanUpdate: (d) {
            final p = toSlide(d.localPosition + (horiz ? Offset(t, top) : Offset(0, top + t)));
            e.drag(0, p.dx, p.dy, _tol);
            _ov = e.overlays();
            _paint();
          },
          onPanEnd: (_) {
            e.drag(2, 0, 0, _tol);
            refresh();
          },
          child: CustomPaint(painter: _RulerPainter(this, horiz, units)),
        );
    return [
      Positioned(left: t, right: 0, top: top, height: t, child: ruler(true)),
      Positioned(left: 0, width: t, top: top + t, bottom: 0, child: ruler(false)),
    ];
  }

  /// The bars of the canvas's modes: Draw With Pen (colours, width, Done), Edit Mask (the
  /// picture's size, Done) and point editing (Done).
  List<Widget> _modeBars() {
    final look = widget.look;
    Widget bar(List<Widget> kids) => Positioned(
          left: 0,
          right: 0,
          bottom: 20,
          child: Center(
            child: Container(
              padding: const EdgeInsets.symmetric(horizontal: 10, vertical: 6),
              decoration: BoxDecoration(color: look.panel, borderRadius: BorderRadius.circular(14), boxShadow: const [BoxShadow(color: Color(0x40000000), blurRadius: 10)]),
              child: Row(mainAxisSize: MainAxisSize.min, children: kids),
            ),
          ),
        );
    Widget done(String action) => TextButton(onPressed: () {
          e.action(action);
          refresh();
        }, child: Text('Done', style: Look.font(15, look.accent, bold: true)));
    final out = <Widget>[];
    final pen = _ov['pen'] as Map?;
    if (pen != null) {
      final cur = (pen['color'] as List).cast<int>();
      out.add(bar([
        for (final c in const [[0x0a, 0x84, 0xff], [0xff, 0x45, 0x3a], [0x30, 0xd1, 0x58], [0xff, 0xd6, 0x0a], [0, 0, 0], [255, 255, 255]])
          GestureDetector(
            onTap: () {
              e.action('pen:color:${c[0]}:${c[1]}:${c[2]}');
              refresh();
            },
            child: Container(
              width: 30,
              height: 30,
              margin: const EdgeInsets.symmetric(horizontal: 3),
              decoration: BoxDecoration(color: Color.fromARGB(255, c[0], c[1], c[2]), shape: BoxShape.circle, border: Border.all(color: cur[0] == c[0] && cur[1] == c[1] && cur[2] == c[2] ? look.accent : look.gray(0x60), width: 2.5)),
            ),
          ),
        const SizedBox(width: 8),
        SizedBox(
          width: 140,
          child: Slider(value: (pen['width'] as num).toDouble().clamp(1, 24), min: 1, max: 24, onChanged: (v) {
            e.action('pen:width:${v.toStringAsFixed(1)}');
            refresh();
          }),
        ),
        done('pen:done'),
      ]));
    }
    final mask = _ov['mask'] as Map?;
    if (mask != null) {
      out.add(bar([
        Text('Size', style: Look.font(14, look.text)),
        SizedBox(
          width: 180,
          child: Slider(value: (mask['size'] as num).toDouble().clamp(100, 500), min: 100, max: 500, onChangeEnd: (v) {
            e.action('mask:size:${v.round()}');
            refresh();
          }, onChanged: (_) {}),
        ),
        done('mask:done'),
      ]));
    }
    final gal = _ov['gallery'] as Map?;
    if (gal != null) {
      final c = [for (final p in gal['corners'] as List) _pt(p)];
      final r = Rect.fromPoints(c[0], c[2]);
      final n = (gal['count'] as num).toInt(), k = (gal['shown'] as num).toInt();
      Widget arrow(bool next) => Positioned(
            left: next ? r.right - 44 : r.left + 8,
            top: r.center.dy - 18,
            child: GestureDetector(
              onTap: () {
                e.action('gallery:${gal['id']}:${((k + (next ? 1 : -1)) % n + n) % n}');
                refresh();
              },
              child: Container(width: 36, height: 36, decoration: const BoxDecoration(color: Color(0x99000000), shape: BoxShape.circle), child: Icon(next ? Icons.chevron_right : Icons.chevron_left, color: Colors.white)),
            ),
          );
      out.add(arrow(false));
      out.add(arrow(true));
      out.add(Positioned(
        left: r.left,
        width: r.width,
        top: r.bottom + 6,
        child: Row(mainAxisAlignment: MainAxisAlignment.center, children: [
          for (var i = 0; i < n; i++) Container(width: 7, height: 7, margin: const EdgeInsets.all(3), decoration: BoxDecoration(shape: BoxShape.circle, color: i == k ? widget.look.accent : widget.look.gray(0x80))),
        ]),
      ));
    }
    if (_ov['points'] != null) out.add(bar([Text('Drag the points', style: Look.font(14, look.textDim)), const SizedBox(width: 8), done('points:done')]));
    return out;
  }

  final _cell = TextEditingController();
  final _cellFocus = FocusNode();
  String? _cellKey;

  /// The cell editor (canvas.rs cell_editor): Return enters the cell, Tab moves right, a tap
  /// elsewhere enters it too; the text goes to the engine as typed.
  Widget _cellEditor(Map<String, dynamic>? ed) {
    if (ed == null) return const SizedBox.shrink();
    final q = [for (final p in ed['quad'] as List) _pt(p)];
    final r = Rect.fromPoints(q[0], q[2]);
    final key = '${(_ov['table'] as Map)['id']}:${ed['row']}:${ed['col']}';
    if (key != _cellKey) {
      _cellKey = key;
      _cell.text = ed['text'] as String? ?? '';
      _cell.selection = TextSelection.collapsed(offset: _cell.text.length);
      WidgetsBinding.instance.addPostFrameCallback((_) => _cellFocus.requestFocus());
    }
    final pad = (ed['padding'] as List?)?.map((v) => (v as num).toDouble() * zoom).toList() ?? [4.0, 4.0, 4.0, 4.0];
    final align = const [TextAlign.left, TextAlign.center, TextAlign.right][((ed['align'] as num?)?.toInt() ?? 0).clamp(0, 2)];
    void done(String how) {
      e.action('cell:text:${_cell.text}');
      e.action('cell:$how');
      _cellKey = null;
      refresh();
    }

    return Positioned.fromRect(
      rect: r,
      child: Container(
        decoration: BoxDecoration(color: colorOfList(ed['fill']), border: Border.all(color: widget.look.accent, width: 2)),
        padding: EdgeInsets.fromLTRB(pad[0], pad[1], pad[2], pad[3]),
        alignment: Alignment.center,
        child: Focus(
          onKeyEvent: (n, k) {
            if (k is KeyDownEvent && k.logicalKey == LogicalKeyboardKey.tab) {
              done(HardwareKeyboard.instance.isShiftPressed ? 'backtab' : 'tab');
              return KeyEventResult.handled;
            }
            if (k is KeyDownEvent && k.logicalKey == LogicalKeyboardKey.escape) {
              e.action('cell:cancel');
              _cellKey = null;
              refresh();
              return KeyEventResult.handled;
            }
            return KeyEventResult.ignored;
          },
          child: TextField(
            controller: _cell,
            focusNode: _cellFocus,
            textAlign: align,
            style: TextStyle(fontSize: ((ed['size'] as num?)?.toDouble() ?? 20) * zoom, color: colorOfList(ed['color'])),
            cursorColor: widget.look.accent,
            decoration: const InputDecoration(isCollapsed: true, border: InputBorder.none),
            textInputAction: TextInputAction.done,
            onChanged: (t) => e.action('cell:text:$t'),
            onSubmitted: (_) => done('enter'),
            onTapOutside: (_) => done('enter'),
          ),
        ),
      ),
    );
  }

  static Color colorOfList(dynamic v) => v is List && v.length >= 3 ? Color.from(alpha: v.length > 3 ? (v[3] as num).toDouble() : 1, red: (v[0] as num).toDouble(), green: (v[1] as num).toDouble(), blue: (v[2] as num).toDouble()) : const Color(0xFFFFFFFF);

  /// A comment's sticky note on the screen (22 points, up and left of its object's corner).
  Rect _commentRect(Map<String, dynamic> c) {
    final at = c['at'] as List;
    return Rect.fromLTWH(0, 0, 22, 22).shift(toLocal((at[0] as num).toDouble(), (at[1] as num).toDouble()) - const Offset(10, 10));
  }

  void _tapUp(TapUpDetails d) {
    for (final c in (_ov['comments'] as List? ?? const []).cast<Map<String, dynamic>>()) {
      if (_commentRect(c).inflate(8).contains(d.localPosition)) {
        e.action('comment:${c['id']}');
        return refresh();
      }
    }
    if (_count == 0) return;
    final now = DateTime.now();
    final near = (d.localPosition - _lastTapAt).distance < 30;
    _taps = now.difference(_lastTap).inMilliseconds < 350 && near ? _taps + 1 : 1;
    _lastTap = now;
    _lastTapAt = d.localPosition;
    final menuWasUp = _menu != null || _listMenuUp;
    _hideMenu();
    final p = toSlide(d.localPosition);
    final before = e.hit(p.dx, p.dy, _tol);
    final caretBefore = (_ov['editing'] as Map?)?['caret'];
    e.tap(p.dx, p.dy, _tol, count: math.min(_taps, 3));
    refresh();
    final editing = e.status()['editingText'] == true;
    if (editing) showKeyboard();
    // the edit menu: a tap on what was selected already (an object, or the caret itself)
    final caretNow = (_ov['editing'] as Map?)?['caret'];
    if (!menuWasUp && _taps == 1 && (before['what'] == 'selected' && !editing || (editing && caretBefore != null && jsonEncode(caretBefore) == jsonEncode(caretNow) && _nearCaret(d.localPosition)))) {
      _showMenu(d.localPosition);
    } else if (_taps >= 2 && editing && ((_ov['editing'] as Map?)?['rects'] as List? ?? const []).isNotEmpty) {
      _showMenu(d.localPosition);
    }
  }

  /// `at` (local) within a finger's reach of the caret drawn now (Letters: a tap off at the
  /// line's end puts the caret where it was, and isn't a tap on it).
  bool _nearCaret(Offset at) {
    final c = (_ov['editing'] as Map?)?['caret'];
    if (c is! List || c.length != 2) return false;
    final r = Rect.fromPoints(_pt(c[0]), _pt(c[1]));
    return (at.dx - r.center.dx).abs() < 24 && at.dy > r.top - 16 && at.dy < r.bottom + 16;
  }

  void _longPress(LongPressStartDetails d) {
    if (_count == 0) return;
    final p = toSlide(d.localPosition);
    e.hold(p.dx, p.dy, _tol);
    refresh();
    HapticFeedback.selectionClick();
    _showMenu(d.localPosition);
  }

  void _scaleStart(ScaleStartDetails d) {
    _hideMenu();
    if (d.pointerCount >= 2) {
      // a second finger: a pinch (an object drag going on is let go where it is)
      if (_gesture == 'object') e.drag(2, 0, 0, _tol);
      _gesture = 'pinch';
      _pinchFocal = d.localFocalPoint;
      _pinchFocalSlide = toSlide(d.localFocalPoint);
      _zoomAtStart = zoom;
      _pinchZoom = zoom;
      return;
    }
    final at = _downAt;
    _gesture = null;
    for (final (start, knob, mid) in _textHandles()) {
      if ((knob - at).distance < 26 || (mid - at).distance < 14) {
        _gesture = start ? 'text0' : 'text1';
        return;
      }
    }
    final p = toSlide(at);
    // Draw With Pen and point editing take every drag (the engine draws / moves the node)
    if (_ov['pen'] != null || _ov['points'] != null) {
      _gesture = 'object';
      e.drag(1, p.dx, p.dy, _tol);
      refresh();
      return;
    }
    final h = e.hit(p.dx, p.dy, _tol);
    final what = h['what'] as String? ?? 'none';
    if (what == 'handle' || what == 'lineEnd' || what == 'selected' || what == 'object') {
      _gesture = 'object';
      e.drag(1, p.dx, p.dy, _tol);
      refresh();
      return;
    }
    if (what == 'editing') {
      // dragging in the text being edited: the selection extends from the caret
      _gesture = 'text1';
      return;
    }
    _gesture = 'pan';
    _panStart = origin;
    _panFrom = d.localFocalPoint;
  }

  Offset _panStart = Offset.zero, _panFrom = Offset.zero;
  double _zoomAtStart = 1;

  void _scaleUpdate(ScaleUpdateDetails d) {
    switch (_gesture) {
      case 'pinch':
        final z = (_zoomAtStart * d.scale).clamp(fitZoom * 0.5, 8.0);
        _pinchZoom = z;
        _pinchFocal = d.localFocalPoint;
        _paint();
      case 'object':
        // (moves go to the engine once a frame, the latest one: a 120 Hz screen sends more)
        final first = _dragTo == null;
        _dragTo = toSlide(d.localFocalPoint);
        if (first) {
          SchedulerBinding.instance.scheduleFrameCallback((_) => _flushDrag());
          SchedulerBinding.instance.scheduleFrame();
        }
      case 'text0' || 'text1':
        final p = toSlide(d.localFocalPoint);
        e.textPoint(p.dx, p.dy, _gesture == 'text0' ? 2 : 1);
        refresh();
      case 'pan':
        if (_zoomSet == null) return;
        _pan = _clampPan(_panStart + d.localFocalPoint - _panFrom, zoom);
        _paint();
    }
  }

  /// The finger's latest place on the slide not yet given to the engine (an object drag).
  Offset? _dragTo;

  void _flushDrag() {
    final p = _dragTo;
    if (p == null || _gesture != 'object') return;
    _dragTo = null;
    e.drag(0, p.dx, p.dy, _tol);
    _ov = e.overlays();
    _queueLive();
    _paint();
  }

  void _scaleEnd(ScaleEndDetails d) {
    switch (_gesture) {
      case 'pinch':
        final z = _pinchZoom ?? zoom;
        final o = origin;
        _pinchZoom = null;
        // back to the fit when let go about there (or smaller)
        if (z <= fitZoom * 1.08) {
          _zoomSet = null;
        } else {
          _zoomSet = z;
          _pan = _clampPan(o, z);
        }
        _queue();
        _paint();
        setState(() {});
      case 'object':
        _flushDrag();
        e.drag(2, 0, 0, _tol);
        _movingSel = null;
        refresh();
      case 'text0' || 'text1':
        _showMenu(_downAt);
      case 'pan':
        if (_zoomSet != null) _queue();
    }
    _gesture = null;
  }

  // ---------------------------------------------------------------- the edit menu

  List<dynamic>? _menu;
  bool _listMenuUp = false;
  String _menuSig = '';
  Offset _menuPoint = Offset.zero;

  void _showMenu(Offset at) {
    final items = e.contextMenu();
    final ed = _ov['editing'];
    if (ed is Map && (ed['caret'] != null || (ed['rects'] as List? ?? const []).isNotEmpty)) {
      // typing: the menu as a list by the text; touches go through it to the handles
      _menuPoint = at;
      final local = _menuAnchor();
      final box = context.findRenderObject() as RenderBox;
      final anchor = box.localToGlobal(local.topLeft) & local.size;
      final typing = _focus.hasFocus;
      // (touches go through it: dragging a handle closes it and drags; it's still "up" for that
      // touch, until its closing comes round after it)
      _listMenuUp = true;
      showTouchMenu(context, widget.look, anchor, items, (i) {
        widget.onMenuAction(i['action'] as String? ?? '');
        if (typing && e.status()['editingText'] == true) showKeyboard();
      }, passThrough: true).whenComplete(() => _listMenuUp = false);
      return;
    }
    setState(() {
      _menu = items;
      _menuPoint = at;
      _menuSig = _selSig();
    });
  }

  void _hideMenu() {
    if (_menu != null) setState(() => _menu = null);
  }

  /// What the menu points at: the selected text, else the selected objects, else the caret,
  /// else the touch; kept to the part in view.
  Rect _menuAnchor() {
    Rect? r;
    void add(Rect x) => r = r == null ? x : r!.expandToInclude(x);
    final ed = _ov['editing'];
    if (ed is Map) {
      for (final q in (ed['rects'] as List? ?? const [])) {
        for (final p in q as List) {
          final l = _pt(p);
          add(Rect.fromLTWH(l.dx, l.dy, 0, 0));
        }
      }
      if (r != null) r = Rect.fromLTRB(r!.left, r!.top - 14, r!.right, r!.bottom + 14);
      final c = ed['caret'];
      if (r == null && c is List) add(Rect.fromPoints(_pt(c[0]), _pt(c[1])).inflate(1));
    }
    if (r == null) {
      for (final s in (_ov['selection'] as List? ?? const [])) {
        for (final p in (s['corners'] as List)) {
          final l = _pt(p);
          add(Rect.fromLTWH(l.dx, l.dy, 0, 0));
        }
      }
    }
    final view = Rect.fromLTWH(0, widget.topInset, _view.width, _view.height - widget.topInset);
    final shown = r?.intersect(view);
    if (shown == null || shown.width < 0 || shown.height < 0) return Rect.fromCenter(center: _menuPoint, width: 1, height: 1);
    return shown;
  }

  // ---------------------------------------------------------------- the keyboard

  final _focus = FocusNode();
  final _input = TextEditingController();
  static const _sentinel = '​​';
  String _last = _sentinel;

  void _resetInput() {
    _last = _sentinel;
    _input.value = const TextEditingValue(text: _sentinel, selection: TextSelection.collapsed(offset: 2));
  }

  /// What the on-screen keyboard typed (text, deletions, new lines), as the desktop's keys.
  void _typed(TextEditingValue v) {
    final now = v.text;
    var p = 0;
    while (p < now.length && p < _last.length && now.codeUnitAt(p) == _last.codeUnitAt(p)) {
      p++;
    }
    final removed = _last.substring(p).runes.length;
    var added = now.substring(p).replaceAll('​', '');
    for (var i = 0; i < removed; i++) {
      e.key('Backspace');
    }
    while (added.isNotEmpty) {
      final nl = added.indexOf('\n');
      final part = nl < 0 ? added : added.substring(0, nl);
      if (part.isNotEmpty) e.text(part);
      if (nl < 0) break;
      e.key('Enter');
      added = added.substring(nl + 1);
    }
    _last = now;
    if (!now.startsWith(_sentinel)) _resetInput();
    refresh();
  }

  static final _keyNames = <LogicalKeyboardKey, String>{
    LogicalKeyboardKey.arrowLeft: 'ArrowLeft', LogicalKeyboardKey.arrowRight: 'ArrowRight', LogicalKeyboardKey.arrowUp: 'ArrowUp',
    LogicalKeyboardKey.arrowDown: 'ArrowDown', LogicalKeyboardKey.home: 'Home', LogicalKeyboardKey.end: 'End',
    LogicalKeyboardKey.tab: 'Tab', LogicalKeyboardKey.escape: 'Escape', LogicalKeyboardKey.delete: 'Delete',
  };

  /// A hardware keyboard's keys when no text field has them: arrows nudge the selection,
  /// Delete removes it, Ctrl shortcuts run the menus' commands.
  bool _keyHandler(KeyEvent ev) {
    if (ev is KeyUpEvent || !mounted) return false;
    final focused = FocusManager.instance.primaryFocus;
    if (focused != null && focused != _focus && focused.context?.findAncestorStateOfType<EditableTextState>() != null) return false;
    final hk = HardwareKeyboard.instance;
    final shift = hk.isShiftPressed, ctrl = hk.isControlPressed || hk.isMetaPressed, alt = hk.isAltPressed;
    final name = _keyNames[ev.logicalKey] ?? (ev.logicalKey == LogicalKeyboardKey.backspace && !_focus.hasFocus ? 'Backspace' : null);
    if (name != null) {
      e.key(name, shift: shift, ctrl: ctrl, alt: alt);
      refresh();
      return true;
    }
    if (ctrl) {
      final label = ev.logicalKey.keyLabel;
      if (label.length != 1 || !RegExp(r'[A-Za-z]').hasMatch(label)) return false;
      final n = label.toUpperCase();
      if (n == 'V') {
        Clipboard.getData(Clipboard.kTextPlain).then((c) {
          e.paste(c?.text ?? '');
          refresh();
        });
        return true;
      }
      e.key(n, shift: shift, ctrl: true, alt: alt);
      final copied = e.copied();
      if (copied != null) Clipboard.setData(ClipboardData(text: copied));
      refresh();
      return true;
    }
    return false;
  }

  /// Text as the on-screen keyboard would type it (automated checks).
  void typeForTest(String t) => _typed(TextEditingValue(text: _last + t));

  void showKeyboard() {
    _resetInput();
    _focus.requestFocus();
    SystemChannels.textInput.invokeMethod('TextInput.show');
  }

  void hideKeyboard() => _focus.unfocus();

  // ---------------------------------------------------------------- build

  @override
  Widget build(BuildContext context) {
    _dpr = MediaQuery.of(context).devicePixelRatio;
    return LayoutBuilder(builder: (ctx, box) {
      final size = box.biggest;
      if (size != _view) {
        _view = size;
        SchedulerBinding.instance.addPostFrameCallback((_) => _queue(ms: 120));
      }
      return TapRegion(
        onTapOutside: (_) => _hideMenu(),
        child: ClipRect(
          child: Stack(children: [
            Positioned.fill(
              child: Listener(
                onPointerDown: (ev) {
                  _downAt = ev.localPosition;
                  if (_menu != null && !_menuAnchor().inflate(40).contains(ev.localPosition)) _hideMenu();
                },
                onPointerSignal: (ev) {
                  // a mouse wheel with Ctrl zooms, else pans when zoomed in
                  if (ev is PointerScrollEvent) {
                    if (HardwareKeyboard.instance.isControlPressed) {
                      setZoom((zoom * (ev.scrollDelta.dy < 0 ? 1.1 : 1 / 1.1)).clamp(fitZoom * 0.5, 8.0));
                    } else if (_zoomSet != null) {
                      _pan = _clampPan(origin - ev.scrollDelta, zoom);
                      _queue();
                      _paint();
                    }
                  }
                },
                child: GestureDetector(
                  behavior: HitTestBehavior.opaque,
                  onTapUp: _tapUp,
                  onLongPressStart: _longPress,
                  onSecondaryTapUp: (d) {
                    final p = toSlide(d.localPosition);
                    e.hold(p.dx, p.dy, _tol);
                    refresh();
                    _showMenu(d.localPosition);
                  },
                  onScaleStart: _scaleStart,
                  onScaleUpdate: _scaleUpdate,
                  onScaleEnd: _scaleEnd,
                  child: CustomPaint(painter: _SlidePainter(this), size: Size.infinite),
                ),
              ),
            ),
            ..._toolbarBand(),
            // a table cell being edited: a field over it in the cell's own size and look
            if ((_ov['table'] as Map?)?['editing'] is Map)
              ValueListenableBuilder<int>(
                valueListenable: _repaint,
                builder: (ctx, _, _) => _cellEditor(((_ov['table'] as Map?)?['editing'] as Map?)?.cast<String, dynamic>()),
              ),
            ..._modeBars(),
            ..._rulers(),
            if (widget.message != null)
              Positioned(
                left: 0,
                right: 0,
                bottom: 16,
                child: IgnorePointer(
                  child: Center(
                    child: Container(
                      constraints: const BoxConstraints(maxWidth: 520),
                      padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 9),
                      decoration: BoxDecoration(color: widget.look.gray(0x30).withValues(alpha: 0.95), borderRadius: BorderRadius.circular(17)),
                      child: Text(widget.message!, maxLines: 2, overflow: TextOverflow.ellipsis, textAlign: TextAlign.center, style: Look.font(13, widget.look.text)),
                    ),
                  ),
                ),
              ),
            // the hidden field the on-screen keyboard types into
            Positioned(
              left: 0,
              top: 0,
              width: 2,
              height: 2,
              child: Opacity(
                opacity: 0,
                child: TextField(
                  controller: _input,
                  focusNode: _focus,
                  keyboardType: TextInputType.multiline,
                  maxLines: null,
                  enableInteractiveSelection: false,
                  onChanged: (_) => _typed(_input.value),
                ),
              ),
            ),
            if (_menu != null && _pinchZoom == null)
              Positioned.fill(
                child: TextFieldTapRegion(
                  child: ValueListenableBuilder<int>(
                    valueListenable: _repaint,
                    builder: (ctx, _, _) => _menu == null
                        ? const SizedBox.shrink()
                        : EditMenu(
                            items: _menu!,
                            anchor: _menuAnchor(),
                            bounds: Rect.fromLTWH(0, widget.topInset, _view.width, _view.height - widget.topInset),
                            look: widget.look,
                            onPick: (a) {
                              _hideMenu();
                              widget.onMenuAction(a);
                            },
                          ),
                  ),
                ),
              ),
          ]),
        ),
      );
    });
  }
}

class _RulerPainter extends CustomPainter {
  _RulerPainter(this.s, this.horiz, this.units) : super(repaint: s._repaint);
  final SlideViewState s;
  final bool horiz;
  final int units;

  @override
  void paint(Canvas canvas, Size size) {
    final look = s.widget.look;
    canvas.drawRect(Offset.zero & size, Paint()..color = look.panel);
    canvas.drawLine(horiz ? Offset(0, size.height - 0.5) : Offset(size.width - 0.5, 0), horiz ? Offset(size.width, size.height - 0.5) : Offset(size.width - 0.5, size.height), Paint()..color = look.separator);
    // points per unit: cm, in, pt, mm (settings.rs ruler_units)
    final per = const [72 / 2.54, 72.0, 1.0, 72 / 25.4][units.clamp(0, 3)];
    final z = s.zoom, o = s.origin;
    // a labelled tick at a step that keeps labels about 60 points apart
    var step = per;
    while (step * z < 50) {
      step *= units == 2 ? 10 : (units == 3 ? 10 : 2);
    }
    while (step * z > 160) {
      step /= 2;
    }
    final origin = horiz ? o.dx - 20 : o.dy - s.widget.topInset - 20;
    final len = horiz ? size.width : size.height;
    final first = (-origin / z / step).floor() - 1;
    final tick = Paint()..color = look.textDim;
    for (var k = first; k * step * z + origin < len + step * z; k++) {
      final v = k * step * z + origin;
      for (var m = 0; m < 4; m++) {
        final w = v + m * step * z / 4;
        final h = m == 0 ? 8.0 : 4.0;
        if (horiz) {
          canvas.drawLine(Offset(w, size.height - h), Offset(w, size.height), tick);
        } else {
          canvas.drawLine(Offset(size.width - h, w), Offset(size.width, w), tick);
        }
      }
      final label = (k * step / per).toStringAsFixed(step / per < 1 ? 1 : 0);
      final tp = TextPainter(text: TextSpan(text: label, style: TextStyle(color: look.textDim, fontSize: 9)), textDirection: TextDirection.ltr)..layout();
      if (horiz) {
        tp.paint(canvas, Offset(v + 3, 1));
      } else {
        canvas.save();
        canvas.translate(2, v + 3 + tp.width);
        canvas.rotate(-1.5708);
        tp.paint(canvas, Offset.zero);
        canvas.restore();
      }
    }
  }

  @override
  bool shouldRepaint(_RulerPainter o) => true;
}

class _SlidePainter extends CustomPainter {
  _SlidePainter(this.s, {this.band = false}) : super(repaint: s._repaint);
  final SlideViewState s;

  /// For the toolbar's band: the canvas and the slide only.
  final bool band;

  @override
  void paint(Canvas canvas, Size size) {
    final look = s.widget.look;
    canvas.drawRect(Offset.zero & size, Paint()..color = look.canvas);
    if (s._count == 0) return;
    final z = s.zoom, o = s.origin;
    final dpr = s._dpr;
    // (on whole device pixels: the pictures' rows and columns on the screen's)
    double snap(double v) => (v * dpr).roundToDouble() / dpr;
    Rect place(Rect r) => Rect.fromLTRB(snap(o.dx + r.left * z), snap(o.dy + r.top * z), snap(o.dx + r.right * z), snap(o.dy + r.bottom * z));
    final slideRect = place(Offset.zero & s._slide);
    // a soft shadow round the slide, as the desktop's
    canvas.drawRect(slideRect.shift(const Offset(0, 1)), Paint()
      ..color = const Color(0x37000000)
      ..maskFilter = const MaskFilter.blur(BlurStyle.normal, 5));
    final paint = Paint()..filterQuality = FilterQuality.medium;
    void draw(_Pic p) => canvas.drawImageRect(p.image, Offset.zero & Size(p.image.width.toDouble(), p.image.height.toDouble()), place(p.rect), paint);
    final live = s._under != null && s._moving != null;
    final motion = s._motionImg;
    if (s._motion && motion != null) canvas.drawImageRect(motion, Offset.zero & Size(motion.width.toDouble(), motion.height.toDouble()), slideRect, paint);
    if (live) {
      draw(s._under!);
    } else if (s._base != null) {
      draw(s._base!);
      if (s._detail != null && s._detail!.key == s._base!.key) draw(s._detail!);
    } else {
      canvas.drawRect(slideRect, Paint()..color = const Color(0xFFFFFFFF));
    }
    if (band) return;
    if (live) {
      // (slid from where it was drawn to where the selection is now)
      final now = s._selBox(), was = s._movingSel;
      final m = s._moving!;
      draw(now == null || was == null ? m : _Pic(m.image, m.rect.shift(now.topLeft - was.topLeft), m.key, m.scale));
    }
    _overlay(canvas, look);
  }

  void _overlay(Canvas canvas, Look look) {
    final ov = s._ov;
    final accent = look.accent;
    final line = Paint()
      ..color = accent
      ..style = PaintingStyle.stroke
      ..strokeWidth = 1;
    void knob(Offset c, {double r = 6}) {
      canvas.drawCircle(c + const Offset(0, 0.5), r + 1, Paint()..color = const Color(0x40000000));
      canvas.drawCircle(c, r, Paint()..color = const Color(0xFFFFFFFF));
      canvas.drawCircle(c, r, line..strokeWidth = 1.2);
    }

    // empty placeholders: their outline and grey prompt (canvas.rs)
    for (final p in (ov['prompts'] as List? ?? const [])) {
      final r = p['rect'] as List;
      final x = (r[0] as num).toDouble(), y = (r[1] as num).toDouble();
      final box = Rect.fromPoints(s.toLocal(x, y), s.toLocal(x + (r[2] as num).toDouble(), y + (r[3] as num).toDouble()));
      canvas.drawRect(box.deflate(0.5), Paint()
        ..color = const Color(0xFFBEBEBE)
        ..style = PaintingStyle.stroke
        ..strokeWidth = 1);
      final size = ((p['size'] as num).toDouble() * s.zoom).clamp(9.0, 64.0);
      final tp = TextPainter(text: TextSpan(text: p['text'] as String, style: Look.font(size, const Color(0xFFA0A0A0))), textDirection: TextDirection.ltr, textAlign: TextAlign.center, maxLines: 2)..layout(maxWidth: box.width);
      tp.paint(canvas, box.center - Offset(tp.width / 2, tp.height / 2));
    }
    // the selected objects: their frames, and handles on the one selected
    for (final o in (ov['selection'] as List? ?? const [])) {
      final ln = o['line'];
      if (ln is List) {
        final a = s._pt(ln[0]), b = s._pt(ln[1]);
        canvas.drawLine(a, b, line..strokeWidth = 1);
        if (o['handles'] == true) {
          knob(a);
          knob(b);
        }
        continue;
      }
      final cs = [for (final p in (o['corners'] as List)) s._pt(p)];
      final path = Path()..addPolygon(cs, true);
      canvas.drawPath(path, line..strokeWidth = 1);
      if (o['locked'] == true) {
        // locked: small crosses at the corners (Keynote)
        for (final c in cs) {
          canvas.drawLine(c - const Offset(4, 4), c + const Offset(4, 4), line);
          canvas.drawLine(c - const Offset(-4, 4), c + const Offset(-4, 4), line);
        }
        continue;
      }
      if (o['handles'] == true) {
        for (var i = 0; i < 4; i++) {
          knob(cs[i]);
          knob(Offset.lerp(cs[i], cs[(i + 1) % 4], 0.5)!, r: 5);
        }
      }
    }
    final ed = ov['editing'];
    if (ed is Map) {
      final f = [for (final p in (ed['frame'] as List)) s._pt(p)];
      canvas.drawPath(Path()..addPolygon(f, true), Paint()
        ..color = accent.withValues(alpha: 0.6)
        ..style = PaintingStyle.stroke
        ..strokeWidth = 1);
      for (final q in (ed['rects'] as List? ?? const [])) {
        canvas.drawPath(Path()..addPolygon([for (final p in q as List) s._pt(p)], true), Paint()..color = accent.withValues(alpha: 0.3));
      }
      final c = ed['caret'];
      if (c is List && s._caretOn) {
        canvas.drawLine(s._pt(c[0]), s._pt(c[1]), Paint()
          ..color = accent
          ..strokeWidth = 2);
      }
      for (final (start, k, _) in s._textHandles()) {
        final ends = ed['ends'] as List;
        final e = ends[start ? 0 : 1] as List;
        canvas.drawLine(s._pt(e[0]), s._pt(e[1]), Paint()
          ..color = accent
          ..strokeWidth = 2);
        canvas.drawCircle(k, 6, Paint()..color = accent);
      }
    }
    final rb = ov['rubber'];
    if (rb is List) {
      final r = Rect.fromPoints(s.toLocal((rb[0] as num).toDouble(), (rb[1] as num).toDouble()), s.toLocal((rb[0] as num).toDouble() + (rb[2] as num).toDouble(), (rb[1] as num).toDouble() + (rb[3] as num).toDouble()));
      canvas.drawRect(r, Paint()..color = accent.withValues(alpha: 0.12));
      canvas.drawRect(r, line);
    }
    // guides (canvas.rs draw_user_guides / draw_guides): Keynote's yellow
    final gd = ov['guides'] as Map?;
    if (gd != null) {
      const gold = Color(0xFFEECA00);
      final size = s._slide;
      void guide(List g, Color c) {
        final v = g[0] == true, pos = (g[1] as num).toDouble();
        final a = v ? s.toLocal(pos, 0) : s.toLocal(0, pos), b = v ? s.toLocal(pos, size.height) : s.toLocal(size.width, pos);
        canvas.drawLine(a, b, Paint()..color = c);
      }

      for (final g in (gd['user'] as List? ?? const []).cast<List>()) {
        guide(g, gold);
      }
      for (final g in (gd['layout'] as List? ?? const []).cast<List>()) {
        guide(g, gold.withValues(alpha: 0.6));
      }
      final dr = gd['dragging'] as List?;
      if (dr != null) {
        guide(dr, gold);
        final v = dr[0] == true, pos = (dr[1] as num).toDouble();
        final at = (v ? s.toLocal(pos, 0) : s.toLocal(0, pos)) + const Offset(6, 6);
        final tp = TextPainter(text: TextSpan(text: '${pos.round()} pt', style: const TextStyle(color: Color(0xFFFFFFFF), fontSize: 12)), textDirection: TextDirection.ltr)..layout();
        canvas.drawRRect(RRect.fromRectAndRadius(Rect.fromLTWH(at.dx, at.dy, tp.width + 12, tp.height + 6), const Radius.circular(4)), Paint()..color = const Color(0xC8000000));
        tp.paint(canvas, at + const Offset(6, 3));
      }
      final gp = Paint()
        ..color = gold
        ..strokeWidth = 1;
      for (final g in (gd['snap'] as List? ?? const []).cast<List>()) {
        switch (g[0]) {
          case 'line':
            canvas.drawLine(s._pt(g[1]), s._pt(g[2]), gp);
          case 'gap':
            final axis = g[1] as num, from = (g[2] as num).toDouble(), to = (g[3] as num).toDouble(), at = (g[4] as num).toDouble();
            final a = axis == 0 ? s.toLocal(from, at) : s.toLocal(at, from), b = axis == 0 ? s.toLocal(to, at) : s.toLocal(at, to);
            canvas.drawLine(a, b, gp);
            final n = axis == 0 ? const Offset(0, 4) : const Offset(4, 0);
            canvas.drawLine(a - n, a + n, gp);
            canvas.drawLine(b - n, b + n, gp);
          case 'size':
            final bx = (g[2] as List).map((v) => (v as num).toDouble()).toList();
            final r = Rect.fromPoints(s.toLocal(bx[0], bx[1]), s.toLocal(bx[2], bx[3]));
            if (g[1] == 0) {
              canvas.drawLine(r.bottomLeft + const Offset(0, 6), r.bottomRight + const Offset(0, 6), gp);
            } else {
              canvas.drawLine(r.topRight + const Offset(6, 0), r.bottomRight + const Offset(6, 0), gp);
            }
        }
      }
    }
    // Draw With Pen: the stroke being drawn
    final pen = ov['pen'] as Map?;
    final stroke = pen?['stroke'] as List?;
    if (pen != null && stroke != null && stroke.length > 1) {
      final c = pen['color'] as List;
      final path = Path()..moveTo(s._pt(stroke[0]).dx, s._pt(stroke[0]).dy);
      for (final q in stroke.skip(1)) {
        final o = s._pt(q);
        path.lineTo(o.dx, o.dy);
      }
      canvas.drawPath(path, Paint()
        ..color = Color.fromARGB(255, c[0] as int, c[1] as int, c[2] as int)
        ..style = PaintingStyle.stroke
        ..strokeWidth = ((pen['width'] as num).toDouble() * s.zoom).clamp(1, 200)
        ..strokeCap = StrokeCap.round
        ..strokeJoin = StrokeJoin.round);
    }
    // Make Editable: the outline as edited, the nodes and the chosen node's handles
    final pts = ov['points'] as Map?;
    if (pts != null) {
      for (final l in (pts['lines'] as List).cast<List>()) {
        if (l.length < 2) continue;
        final path = Path()..addPolygon([for (final q in l) s._pt(q)], false);
        canvas.drawPath(path, Paint()
          ..color = accent
          ..style = PaintingStyle.stroke
          ..strokeWidth = 1.5);
      }
      for (final n in (pts['nodes'] as List).cast<Map<String, dynamic>>()) {
        final c = s._pt(n['at']);
        final on = n['on'] == true;
        for (final k in ['in', 'out']) {
          if (n[k] == null) continue;
          final h = s._pt(n[k]);
          if ((h - c).distance < 0.5) continue;
          canvas.drawLine(c, h, Paint()..color = accent);
          canvas.drawCircle(h, 4, Paint()..color = on ? accent : const Color(0xFFFFFFFF));
        }
        final r = Rect.fromCenter(center: c, width: 10, height: 10);
        canvas.drawRect(r, Paint()..color = on ? accent : const Color(0xFFFFFFFF));
        canvas.drawRect(r, Paint()
          ..color = accent
          ..style = PaintingStyle.stroke);
      }
    }
    // the selected table's cell and range (canvas.rs: the accent frame; the range tinted)
    final tbl = ov['table'] as Map?;
    if (tbl != null) {
      Path quad(List q) => Path()..addPolygon([for (final p in q) s._pt(p)], true);
      if (tbl['range'] is List) {
        final p = quad(tbl['range'] as List);
        canvas.drawPath(p, Paint()..color = accent.withValues(alpha: 0.15));
        canvas.drawPath(p, Paint()
          ..color = accent
          ..style = PaintingStyle.stroke
          ..strokeWidth = 1.5);
      } else if (tbl['editing'] == null) {
        canvas.drawPath(quad(tbl['cell'] as List), Paint()
          ..color = accent
          ..style = PaintingStyle.stroke
          ..strokeWidth = 2.5);
      }
    }
    // View > Comments: sticky notes (canvas.rs comment_markers)
    for (final c in (ov['comments'] as List? ?? const []).cast<Map<String, dynamic>>()) {
      final r = s._commentRect(c);
      final col = c['color'] is List ? Color.from(alpha: 1, red: ((c['color'] as List)[0] as num).toDouble(), green: ((c['color'] as List)[1] as num).toDouble(), blue: ((c['color'] as List)[2] as num).toDouble()) : const Color(0xFFFFD60A);
      final rr = RRect.fromRectAndRadius(r, const Radius.circular(3));
      canvas.drawRRect(rr, Paint()..color = col);
      canvas.drawRRect(rr, Paint()
        ..color = const Color(0x5A000000)
        ..style = PaintingStyle.stroke);
      for (var k = 0; k < 3; k++) {
        final y = r.top + 7 + k * 4;
        canvas.drawLine(Offset(r.left + 5, y), Offset(r.right - 5, y), Paint()..color = const Color(0x6E000000));
      }
    }
  }

  @override
  bool shouldRepaint(_SlidePainter o) => true;
}
