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

  Future<void> _motionTick(Duration _) async {
    if (!_motion || _motionBusy || _view.isEmpty) return;
    final now = _motionClock.elapsedMilliseconds;
    if (now - _motionAt < 33) return;
    _motionAt = now;
    _motionBusy = true;
    try {
      final w = (_slide.width * zoom * _dpr / 2.5).clamp(320.0, 1024.0).round();
      final h = (w * _slide.height / _slide.width).round().clamp(2, 4096);
      final img = await e.motion(_index, now / 1000, w, h);
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
    _motionImg?.dispose();
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

  void _queue() {
    _settle?.cancel();
    _settle = Timer(const Duration(milliseconds: 30), _render);
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
      } else {
        final s = math.min(_scale, 2048 / _slide.width);
        final w = (_slide.width * s).ceil(), h = (_slide.height * s).ceil();
        if (_under == null) {
          final img = await e.render(_index, s, 0, 0, w, h, mode: 1 + _bgMode);
          if (img != null) _under = _Pic(img, Offset.zero & _slide, 0, s);
        }
        final img = await e.render(_index, s, 0, 0, w, h, mode: 2);
        if (img != null) {
          _moving?.image.dispose();
          _moving = _Pic(img, Offset.zero & _slide, 0, s);
        }
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

  void _tapUp(TapUpDetails d) {
    if (_count == 0) return;
    final now = DateTime.now();
    final near = (d.localPosition - _lastTapAt).distance < 30;
    _taps = now.difference(_lastTap).inMilliseconds < 350 && near ? _taps + 1 : 1;
    _lastTap = now;
    _lastTapAt = d.localPosition;
    final menuWasUp = _menu != null;
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
    if (!menuWasUp && _taps == 1 && (before['what'] == 'selected' && !editing || (editing && caretBefore != null && jsonEncode(caretBefore) == jsonEncode(caretNow)))) {
      _showMenu(d.localPosition);
    } else if (_taps >= 2 && editing && ((_ov['editing'] as Map?)?['rects'] as List? ?? const []).isNotEmpty) {
      _showMenu(d.localPosition);
    }
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
        final p = toSlide(d.localFocalPoint);
        e.drag(0, p.dx, p.dy, _tol);
        _ov = e.overlays();
        _queueLive();
        _paint();
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
        e.drag(2, 0, 0, _tol);
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
      showTouchMenu(context, widget.look, anchor, items, (i) {
        widget.onMenuAction(i['action'] as String? ?? '');
        if (typing && e.status()['editingText'] == true) showKeyboard();
      }, passThrough: true);
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
        SchedulerBinding.instance.addPostFrameCallback((_) => _queue());
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
    if (live) draw(s._moving!);
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
  }

  @override
  bool shouldRepaint(_SlidePainter o) => true;
}
