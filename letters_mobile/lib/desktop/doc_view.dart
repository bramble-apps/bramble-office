// The pages, as the desktop canvas shows them (pg-app canvas.rs: the soft shadow round each
// white page, its outline, the selection, caret, handles and frames over them, the scrollbar,
// Pages' tinted blur under the floating toolbar) — scrolled, flung and pinch-zoomed by Flutter.
// The page content is the engine's, drawn in strips that are kept and only drawn again when
// what they show changes (or, after a zoom, sharper); a pinch scales what's there meanwhile.
// Touches: one finger scrolls; on the selection (an object, its handles, the text handles) it
// moves that instead; a tap is the desktop's click (twice and three times: word and paragraph);
// touch and hold, its right-click.
import 'dart:async';
import 'dart:convert';
import 'dart:math' as math;
import 'dart:typed_data';
import 'dart:ui' as ui;

import 'package:flutter/gestures.dart';
import 'package:flutter/material.dart';
import 'package:flutter/scheduler.dart';
import 'package:flutter/services.dart';

import '../engine.dart';
import 'canvas_controls.dart';
import 'edit_menu.dart';
import 'look.dart';
import 'touch_menu.dart';
import 'table_chrome.dart';

/// canvas.rs PAD: space round the pages.
const _pad = 12.0;

class DocView extends StatefulWidget {
  const DocView({super.key, required this.engine, required this.look, required this.topInset, required this.onChanged, required this.onMenuAction, required this.onMenu, this.ruler, this.wordCount, this.message, this.screenView = false});
  final Engine engine;
  final Look look;

  /// The floating toolbar's height: the view starts under it, the pages scroll up under it.
  final double topInset;

  /// The document, selection or caret changed (the chrome and inspector follow).
  final VoidCallback onChanged;

  /// An edit menu item picked (its action, as the right-click menu's).
  final void Function(String action) onMenuAction;

  /// A menu of these items at this point (global): a table tab's.
  final void Function(List<dynamic> items, Offset global) onMenu;

  /// View > Show Ruler: the ruler's data (ruler.rs api_state), shown over the pages.
  final Map<String, dynamic>? ruler;

  /// View > Show Word Count: the pill's text; a message to show in the message pill.
  final String? wordCount, message;

  /// View > Enter Screen View: its pill at the top right of the pages.
  final bool screenView;

  @override
  State<DocView> createState() => DocViewState();
}

class _Strip {
  _Strip(this.key, this.version, this.scale, this.image);
  int key, version;
  double scale;
  ui.Image? image;
}

class DocViewState extends State<DocView> with TickerProviderStateMixin {
  Engine get e => widget.engine;

  // ---- the view ----
  double _zoom = 1.25;
  Offset _scroll = Offset.zero; // document point at the view's top-left
  Size _view = Size.zero; // the view under the toolbar (points)
  double _dpr = 2;

  // the pages (document points)
  List<Rect> _pages = [];
  Size _doc = Size.zero;
  double _stripH = 96;
  int _version = -1;
  Map<String, dynamic> _ov = {};
  Map<String, dynamic>? _table;

  // a pinch going on: the zoom it shows and the scroll for it
  double? _pinchZoom;
  Offset _pinchScroll = Offset.zero;
  double _zoomAtStart = 1;
  Offset _docAtFocal = Offset.zero;

  double get zoom => _zoom;
  double get _shownZoom => _pinchZoom ?? _zoom;
  Offset get _shownScroll => _pinchZoom != null ? _pinchScroll : _scroll;

  // ---- strips ----
  final _strips = <int, _Strip>{};
  bool _rendering = false, _again = false;

  // ---- motion ----
  late final AnimationController _fling = AnimationController.unbounded(vsync: this)..addListener(_flingTick);
  Simulation? _simX, _simY;
  Offset _flingFrom = Offset.zero;
  late final AnimationController _blink = AnimationController(vsync: this, duration: const Duration(milliseconds: 1060))..repeat();

  // ---- touches ----
  /// what a one-finger drag does: scroll, or move the selection (an engine drag)
  bool _grab = false;
  Offset _grabOffset = Offset.zero; // finger → the point dragged (a handle's centre)
  bool _shiftDrag = false;
  bool _scrollbarDrag = false;
  Offset _lastFocal = Offset.zero;
  // fingers down now, and whether a second came during this touch (no touch and hold then)
  int _fingers = 0;
  bool _multi = false;

  // ---- the edit menu (edit_menu.dart) ----
  /// Its items while it shows; the touch it was asked at; what was selected then.
  List<dynamic>? _menu;
  Offset _menuPoint = Offset.zero;
  String _menuSig = '';
  bool _menuWasUp = false; // it showed when this touch began (a tap on the selection hides it)
  bool _listMenuUp = false; // the text's edit menu (showTouchMenu) is open

  // ---- the keyboard ----
  final _input = TextEditingController();
  late final FocusNode _focus = FocusNode(onKeyEvent: _hardKey);

  // ---- the on-page controls (canvas_controls.dart) ----
  GuideDrag? _guide;
  PointDrag? _point;
  bool _onControl = false; // a touch began on a button (no scrolling)
  Offset _downAt = Offset.zero; // where the finger went down (the scale starts past the slop)
  Offset? _hover; // the mouse (a ▶ shows on the movie it's over, buttons light up)
  bool _pageNumberMenu = false; // the page number button's menu is open (the button stays lit)
  ui.Image? _movie;
  int? _movieId;
  int _movieCap = 0, _movieTicks = 0;
  bool _movieBusy = false;
  Timer? _movieTimer;

  final _repaint = ValueNotifier(0);

  // ---- a drag going on ----
  /// What's being dragged (the strips leave it out while it is: their keys are asked again).
  String _dragSig = '';

  /// The moving objects drawn at their live place: id → (what it was drawn for, the picture).
  final _liveImages = <int, (String, ui.Image?)>{};

  /// After a drop: the live pictures stay where they were dropped until the strips show the
  /// objects there.
  Map<String, dynamic>? _dropped;

  @override
  void initState() {
    super.initState();
    _resetInput();
  }

  @override
  void dispose() {
    _fling.dispose();
    _blink.dispose();
    _movieTimer?.cancel();
    _movie?.dispose();
    _input.dispose();
    _slideEnd?.cancel();
    _focus.dispose();
    for (final s in _strips.values) {
      s.image?.dispose();
    }
    for (final l in _liveImages.values) {
      l.$2?.dispose();
    }
    super.dispose();
  }

  void _paint() => _repaint.value++;

  // ---------------------------------------------------------------- the engine's state

  /// Reads the pages' places and the overlays again (after anything that may have changed
  /// them); the engine may have moved the view (the caret kept in view, Go to Page).
  void refresh() {
    _pageNumberMenu = false;
    final g = e.geometry();
    _readGeometry(g);
    _setOverlays(e.overlays());
    _table = e.table();
    final s = g['scroll'] as List?;
    if (s != null && _pinchZoom == null) {
      final want = Offset((s[0] as num).toDouble(), (s[1] as num).toDouble());
      if ((want - _scroll).distance > 0.5) {
        _fling.stop();
        _scroll = want;
      }
    }
    _blink.forward(from: 0);
    _blink.repeat();
    _paint();
    _queueStrips();
    _watchMovie();
    widget.onChanged();
  }

  // ---------------------------------------------------------------- on-page controls

  CanvasControls _controls() => CanvasControls((_ov['controls'] as Map<String, dynamic>?) ?? const {}, _toLocal, _shownZoom, _pages, Rect.fromLTWH(0, _top, _view.width, _view.height), dragging: _ov['dragging'] == true);

  /// A tap on a control: its action (the page number button: its menu). False when none there.
  bool _tapControl(Offset local, Offset global, bool touch) {
    final h = _controls().hit(local, touch: touch, dpr: _dpr);
    switch (h) {
      case null:
        return false;
      case TapHit(menu: final menu?, menuAt: final at?):
        _pageNumberMenu = true;
        _paint();
        widget.onMenu(menu, global - local + at);
      case TapHit(:final action):
        if (action.startsWith('mediaplay:')) {
          final p = action.split(':');
          _movieCap = math.max(_movieCap, ((int.tryParse(p[2]) ?? 0) + 2) * ((int.tryParse(p[3]) ?? 0) + 2) * 4);
        }
        e.action(action);
        refresh();
      case PointHit(:final s, :final i):
        e.action('points:sel:$s:$i');
        refresh();
      case GuideHit():
        break;
    }
    return true;
  }

  /// The nearest page to `local` and the page point there.
  (int, Offset) _nearPage(Offset local) {
    final d = _shownScroll + _toView(local) / _shownZoom;
    var best = 0;
    var dist = double.infinity;
    for (var i = 0; i < _pages.length; i++) {
      final r = _pages[i];
      final dy = d.dy < r.top ? r.top - d.dy : (d.dy > r.bottom ? d.dy - r.bottom : 0.0);
      if (dy < dist) {
        dist = dy;
        best = i;
      }
    }
    return (best, d - _pages[best].topLeft);
  }

  /// guides.rs ui: the guide dragged follows `local`; off its page it goes (or isn't added).
  void _guideTo(Offset local) {
    final g = _guide;
    if (g == null || _pages.isEmpty) return;
    final (page, q) = _nearPage(local);
    if (g.index < 0) g.page = page;
    final pos = g.vertical ? q.dx : q.dy;
    final size = _pages[g.page].size;
    final inside = pos >= 0 && pos <= (g.vertical ? size.width : size.height) && (page == g.page || g.index < 0);
    g.pos = inside ? pos : null;
    g.pointer = local;
    _paint();
  }

  void _guideEnd() {
    final g = _guide;
    _guide = null;
    if (g == null) return;
    if (g.pos == null && g.index < 0) return _paint();
    e.action('guideput:${g.page}:${g.index}:${g.vertical ? 'v' : 'h'}:${g.pos?.toStringAsFixed(2) ?? 'x'}');
    refresh();
  }

  /// A drag from the ruler away from its markers: a new horizontal guide (ruler.rs).
  void startRulerGuide() => _guide = GuideDrag(_pages.isEmpty ? 0 : _nearPage(Offset(_view.width / 2, _top + _view.height / 2)).$1, -1, false);

  /// The movie playing in place: its frames read while it plays (or waits for its stream).
  void _watchMovie() {
    final media = ((_ov['controls'] as Map?)?['media'] as List?) ?? const [];
    final on = media.any((m) => m['playing'] == true || m['waiting'] == true);
    if (on) {
      _movieTimer ??= Timer.periodic(const Duration(milliseconds: 33), (_) => _pollMovie());
    } else if (_movieTimer != null) {
      _movieTimer!.cancel();
      _movieTimer = null;
      _movie?.dispose();
      _movie = null;
      _movieId = null;
      _paint();
    }
  }

  Future<void> _pollMovie() async {
    if (_movieBusy || !mounted) return;
    _movieBusy = true;
    final f = await e.mediaFrame(math.max(_movieCap, 4 << 20));
    _movieBusy = false;
    if (!mounted) return f?.$2.dispose();
    if (f != null) {
      _movie?.dispose();
      _movieId = f.$1;
      _movie = f.$2;
      _paint();
    }
    // whether it finished (or its stream came), now and then
    if (++_movieTicks % 15 == 0) {
      _setOverlays(e.overlays());
      _paint();
      _watchMovie();
    }
  }

  static double _smoother(double t) => t * t * t * (t * (t * 6 - 15) + 10);

  List<Widget> _toolbarBand() {
    final look = widget.look;
    final h = widget.topInset;
    Widget blur(double top, double height, double sigma, [Color? tint]) => Positioned(
          left: 0,
          right: 0,
          top: top,
          height: height,
          child: IgnorePointer(
            child: ClipRect(
              child: BackdropFilter(filter: ui.ImageFilter.blur(sigmaX: sigma, sigmaY: sigma), child: tint == null ? const SizedBox.expand() : ColoredBox(color: tint)),
            ),
          ),
        );
    switch (look.toolbarBackground) {
      case 'solid':
        return [Positioned(left: 0, right: 0, top: 0, height: h, child: IgnorePointer(child: ColoredBox(color: look.header)))];
      case 'feathered':
        // the pages drawn again, blurred, over the band, in three layers whose blur thins out
        // going down (progressive_blur): strong under the buttons, gentler, then nearly sharp
        // as it fades into the page, so there's no band where it starts or ends. The fade is
        // the desktop's (glass.rs FEATHER_REACH, 40 points) but over the band's foot, done where
        // the toolbar ends rather than reaching onto the page below it
        const reach = 40.0;
        final total = h;
        Widget layer(double sigma, double fadeFrom, double fadeTo) => Positioned(
              left: 0,
              right: 0,
              top: 0,
              height: total,
              child: IgnorePointer(
                child: ClipRect(
                  child: ShaderMask(
                  blendMode: BlendMode.dstIn,
                  shaderCallback: (r) => LinearGradient(
                    begin: Alignment.topCenter,
                    end: Alignment.bottomCenter,
                    colors: [for (var k = 0; k <= 8; k++) Color.fromARGB((255 * (1 - _smoother(k / 8))).round(), 255, 255, 255)],
                    stops: [for (var k = 0; k <= 8; k++) (fadeFrom + (fadeTo - fadeFrom) * k / 8) / total],
                  ).createShader(r),
                  child: ImageFiltered(
                    imageFilter: ui.ImageFilter.blur(sigmaX: sigma, sigmaY: sigma, tileMode: TileMode.decal),
                    child: ClipRect(child: CustomPaint(painter: _DocPainter(this, band: true), size: Size.infinite)),
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
        return [blur(0, h, 12, look.toolbarTint)];
    }
  }

  /// The overlays read anew. A drag starting or ending changes what the strips show (the
  /// objects dragged are left out of them and drawn live over them, canvas.rs dragged).
  void _setOverlays(Map<String, dynamic> ov) {
    final was = _ov['live'];
    _ov = ov;
    // the selection changed (typing, a toolbar button, an item run): the edit menu closes
    if (_menu != null && _selSig() != _menuSig) _menu = null;
    final live = ov['live'];
    final sig = '${ov['dragging']} ${live is Map ? live['ids'] : ''}';
    if (sig != _dragSig) {
      _dragSig = sig;
      if (live == null && was is Map) _dropped = was.cast<String, dynamic>();
      for (final st in _strips.values) {
        st.version = -1;
      }
      _queueStrips();
    }
    if (live is Map) _loadLive(live);
  }

  /// Draws the moving objects alone (once for each zoom and edit), for their live place.
  void _loadLive(Map live) {
    final scale = _scaleFor(_zoom);
    for (final o in (live['objects'] as List? ?? [])) {
      final id = o['id'] as int;
      final b = _l(o['box']);
      final key = '$scale $_version ${b[2]} ${b[3]}';
      if (_liveImages[id]?.$1 == key) continue;
      _liveImages[id] = (key, _liveImages[id]?.$2);
      e.renderObject(id, scale, b[2], b[3]).then((img) {
        if (!mounted || _liveImages[id]?.$1 != key) return img?.dispose();
        _liveImages[id]?.$2?.dispose();
        _liveImages[id] = (key, img);
        _paint();
      });
    }
  }

  /// The engine's fit mode ("width", "page", "none").
  String _fit = 'none';

  /// A side pane sliding with Fit Width: the width and zoom it began at, and the timer that
  /// re-fits once it stops (meanwhile the pages on screen are scaled, as in a pinch).
  double? _slideW0, _slideZoom0;
  Timer? _slideEnd;

  void _readGeometry(Map<String, dynamic> g) {
    _fit = g['fit'] as String? ?? _fit;
    _pages = [for (final p in (g['pages'] as List? ?? [])) Rect.fromLTWH((p[0] as num).toDouble(), (p[1] as num).toDouble(), (p[2] as num).toDouble(), (p[3] as num).toDouble())];
    final size = g['size'] as List?;
    if (size != null) _doc = Size((size[0] as num).toDouble(), (size[1] as num).toDouble());
    _stripH = (g['strip'] as num?)?.toDouble() ?? 96;
    _version = (g['version'] as int?) ?? _version;
    _zoom = (g['zoom'] as num?)?.toDouble() ?? _zoom;
  }

  /// Tells the engine where the view is now (its logic works in the view's points).
  void _sync() => e.scroll(_scroll.dx, _scroll.dy, _zoom);

  Offset _clampScroll(Offset s, double z) {
    final maxX = math.max(0.0, _doc.width - _view.width / z);
    final maxY = math.max(0.0, _doc.height - _view.height / z);
    return Offset(s.dx.clamp(0.0, maxX), s.dy.clamp(0.0, maxY));
  }

  // ---------------------------------------------------------------- zoom

  /// Zoom to `z` around the view point `at` (default: the top-left stays), animated.
  void setZoom(double z, {Offset? at}) {
    z = z.clamp(0.25, 4.0);
    final focal = at ?? Offset.zero;
    final docAt = _scroll + focal / _zoom;
    _commitZoom(z, focal, docAt);
  }

  void _commitZoom(double z, Offset focal, Offset docAt) {
    // the page point under the focal point, kept there (the pages are centred anew at a new zoom)
    final pi = _pageAt(docAt);
    final inPage = pi == null ? null : docAt - _pages[pi].topLeft;
    _zoom = z;
    e.scroll(_scroll.dx, _scroll.dy, z);
    _readGeometry(e.geometry());
    final d = pi != null && pi < _pages.length ? _pages[pi].topLeft + inPage! : docAt;
    _scroll = _clampScroll(d - focal / z, z);
    _pinchZoom = null;
    _sync();
    _setOverlays(e.overlays());
    _paint();
    _queueStrips();
    widget.onChanged();
  }

  int? _pageAt(Offset doc) {
    for (var i = 0; i < _pages.length; i++) {
      if (_pages[i].inflate(_pad).contains(doc)) return i;
    }
    return null;
  }

  // ---------------------------------------------------------------- strips

  int _sk(int page, int strip) => page << 16 | strip;

  /// The pixels per point strips are drawn at for zoom `z`: the screen's exactly, so a strip
  /// lands pixel for pixel (a scale rounded up and shrunk back made the text jagged).
  double _scaleFor(double z) => (z * _dpr * 1000).round() / 1000;

  Rect _visibleDoc() {
    final z = _shownZoom, s = _shownScroll;
    // (and what's under the toolbar)
    return Rect.fromLTWH(s.dx, s.dy - _top / z, _view.width / z, (_view.height + _top) / z);
  }

  /// The strips in view (and a little round it), nearest the middle first.
  List<(int, int)> _wanted() {
    final vis = _visibleDoc().inflate(200);
    final out = <(int, int, double)>[];
    for (var p = 0; p < _pages.length; p++) {
      final r = _pages[p];
      if (!r.overlaps(vis)) continue;
      final n = (r.height / _stripH).ceil();
      for (var s = 0; s < n; s++) {
        final sr = Rect.fromLTWH(r.left, r.top + s * _stripH, r.width, _stripH);
        if (sr.overlaps(vis)) out.add((p, s, (sr.center - vis.center).distance));
      }
    }
    out.sort((a, b) => a.$3.compareTo(b.$3));
    return [for (final w in out) (w.$1, w.$2)];
  }

  void _queueStrips() {
    if (_rendering) {
      _again = true;
      return;
    }
    _again = false;
    _rendering = true;
    SchedulerBinding.instance.scheduleFrameCallback((_) => _renderSome());
  }

  /// Draws strips that are missing, out of date or blurry (after a zoom), a few per frame.
  Future<void> _renderSome() async {
    if (!mounted) return;
    if (_pinchZoom != null || _view.isEmpty) {
      _rendering = false;
      return;
    }
    final scale = _scaleFor(_zoom);
    final t0 = DateTime.now();
    var did = false;
    for (final (p, s) in _wanted()) {
      final k = _sk(p, s);
      final have = _strips[k];
      if (have != null && have.version != _version) {
        final key = e.stripKey(p, s);
        have.version = _version;
        if (key != have.key) have.key = -1;
      }
      final fresh = have != null && have.key != -1 && have.image != null && (have.scale - scale).abs() < 1e-3;
      if (fresh) continue;
      final key = have?.key == -1 || have == null ? e.stripKey(p, s) : have.key;
      final img = await e.renderStrip(p, s, scale, _pages[p].width, _stripH);
      if (!mounted) return img?.dispose();
      final old = _strips[k];
      if (old != null && old.image != img) old.image?.dispose();
      _strips[k] = _Strip(key, _version, scale, img);
      did = true;
      if (DateTime.now().difference(t0).inMilliseconds > 10) break;
    }
    // forget strips far from the view
    if (_strips.length > 160) {
      final keep = {for (final (p, s) in _wanted()) _sk(p, s)};
      _strips.removeWhere((k, v) {
        if (keep.contains(k)) return false;
        v.image?.dispose();
        return true;
      });
    }
    _rendering = false;
    // (every strip in view is up to date: the dropped objects are in them now)
    if (!did && !_again && _dropped != null) {
      _dropped = null;
      did = true;
    }
    if (did) _paint();
    if (did || _again) _queueStrips();
  }

  // ---------------------------------------------------------------- view ↔ page points

  /// Where the view starts: under the toolbar, and under the ruler when it shows (26).
  double get _top => widget.topInset + (widget.ruler != null ? 26 : 0);

  /// Page point → this widget's point (the view starts under the toolbar).
  Offset _toLocal(int page, double x, double y) {
    final z = _shownZoom, s = _shownScroll;
    final p = _pages[page];
    return Offset((p.left + x - s.dx) * z, (p.top + y - s.dy) * z + _top);
  }

  /// This widget's point → the engine's view point.
  Offset _toView(Offset local) => local - Offset(0, _top);

  List<double> _l(dynamic v) => [for (final n in v as List) (n as num).toDouble()];

  /// The text selection's handles (local points): (is start, knob, the caret line's middle).
  List<(bool, Offset, Offset)> _textHandles() {
    final rects = [for (final r in (_ov['rects'] as List? ?? [])) _l(r)];
    if (rects.isEmpty || _pages.isEmpty) return [];
    final f = rects.first, l = rects.last;
    final top = _toLocal(f[0].toInt(), f[1], f[2]), bot = _toLocal(f[0].toInt(), f[1], f[4]);
    final ltop = _toLocal(l[0].toInt(), l[3], l[2]), lbot = _toLocal(l[0].toInt(), l[3], l[4]);
    // (kn_ui touch.rs knobs: 7 above the start, 7 below the end)
    return [(true, top - const Offset(0, 7), Offset(top.dx, (top.dy + bot.dy) / 2)), (false, lbot + const Offset(0, 7), Offset(ltop.dx, (ltop.dy + lbot.dy) / 2))];
  }

  /// What a finger at `local` grabs: a text handle, an object handle or a selected object.
  /// Returns where the engine's press goes (and whether as a Shift-press), or null to scroll.
  (Offset, bool)? _grabAt(Offset local) {
    for (final (start, knob, mid) in _textHandles()) {
      if ((knob - local).distance < 26 || (mid - local).distance < 14) {
        if (start) {
          // the start handle: the other end becomes the anchor first (a click there)
          final end = _textHandles().last.$3;
          final v = _toView(end);
          e.pointer(1, v.dx, v.dy);
          e.pointer(2, v.dx, v.dy);
        }
        return (mid, true);
      }
    }
    for (final o in (_ov['objects'] as List? ?? [])) {
      final page = o['page'] as int;
      if (page >= _pages.length) continue;
      for (final h in (o['handles'] as List? ?? [])) {
        final hp = _l(h);
        final at = _toLocal(page, hp[0], hp[1]);
        if ((at - local).distance < 24) return (at, false);
      }
    }
    for (final o in (_ov['objects'] as List? ?? [])) {
      if (o['editing'] == true) continue;
      final page = o['page'] as int;
      if (page >= _pages.length) continue;
      final f = _l(o['frame']);
      final r = Rect.fromPoints(_toLocal(page, f[0], f[1]), _toLocal(page, f[2], f[3]));
      if (r.inflate(10).contains(local)) return (local, false);
    }
    return null;
  }

  // ---------------------------------------------------------------- the edit menu

  /// What's selected (the caret, the text, the objects), to tell when it changes.
  String _selSig() => jsonEncode([_ov['caret'], _ov['rects'], for (final o in (_ov['objects'] as List? ?? [])) [o['page'], o['frame'], o['editing']]]);

  /// The selected objects (not being edited), as a signature; '' with none.
  String _objSig() => jsonEncode([for (final o in (_ov['objects'] as List? ?? [])) if (o['editing'] != true) [o['page'], o['frame']]]).replaceAll('[]', '');

  /// The right-click menu's items as the edit bar over what's selected (`at`: the touch, local).
  void _showMenu(Offset at) {
    final items = e.contextMenu();
    // editing text (a caret or selected text): the menu as a list, in the menus' style, by
    // what's selected; objects keep the bar
    final rects = _ov['rects'] as List? ?? const [];
    if (_ov['caret'] != null || rects.isNotEmpty) {
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
        // (back to typing where it was)
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

  /// What the menu points at (local): the selected text, else the selected objects, else the
  /// caret, else the touch; kept to the part in view.
  Rect _menuAnchor() {
    final np = _pages.length;
    Rect? r;
    void add(Rect x) => r = r == null ? x : r!.expandToInclude(x);
    for (final v in [for (final x in (_ov['rects'] as List? ?? [])) _l(x)]) {
      if (v[0].toInt() < np) add(Rect.fromPoints(_toLocal(v[0].toInt(), v[1], v[2]), _toLocal(v[0].toInt(), v[3], v[4])));
    }
    // (room for the handles' knobs above the start and below the end)
    if (r != null) r = Rect.fromLTRB(r!.left, r!.top - 14, r!.right, r!.bottom + 14);
    if (r == null) {
      for (final o in (_ov['objects'] as List? ?? [])) {
        final page = o['page'] as int;
        if (o['editing'] == true || page >= np) continue;
        final f = _l(o['frame']);
        add(Rect.fromPoints(_toLocal(page, f[0], f[1]), _toLocal(page, f[2], f[3])));
      }
    }
    final c = _ov['caret'];
    if (r == null && c is List && c.length == 4) {
      final v = _l(c);
      if (v[0].toInt() < np) {
        final a = _toLocal(v[0].toInt(), v[1], v[2]);
        r = Rect.fromLTWH(a.dx, a.dy, 1, v[3] * _shownZoom);
      }
    }
    final view = Rect.fromLTWH(0, _top, _view.width, _view.height);
    final shown = r?.intersect(view);
    if (shown == null || shown.width < 0 || shown.height < 0) return Rect.fromCenter(center: _menuPoint, width: 1, height: 1);
    return shown;
  }

  /// Whether `local` is on the selected text.
  bool _onSelection(Offset local) {
    for (final v in [for (final x in (_ov['rects'] as List? ?? [])) _l(x)]) {
      if (v[0].toInt() >= _pages.length) continue;
      if (Rect.fromPoints(_toLocal(v[0].toInt(), v[1], v[2]), _toLocal(v[0].toInt(), v[3], v[4])).inflate(6).contains(local)) return true;
    }
    return false;
  }

  // ---------------------------------------------------------------- gestures

  /// The last tap (when, where): a finger's second tap near it counts as the double tap.
  (DateTime, Offset)? _lastTap;

  void _tapUp(TapUpDetails d) {
    _fling.stop();
    if (_tapControl(d.localPosition, d.globalPosition, d.kind != PointerDeviceKind.mouse)) return;
    var at = d.localPosition;
    // the engine counts presses within 5 points as a double / triple click (a mouse's); a
    // finger lands further off, so a tap within 24 points and 0.45 s goes where the first was
    final now = DateTime.now();
    final last = _lastTap;
    final touch = d.kind != PointerDeviceKind.mouse;
    final again = last != null && now.difference(last.$1).inMilliseconds < 450 && (at - last.$2).distance < 24;
    if (touch && again) at = last.$2;
    _lastTap = (now, at);
    // (iOS) a tap on the selected text shows or hides the edit menu, the selection kept; a
    // quick second tap still goes on (a triple tap selects the paragraph)
    if (touch && !again && _onSelection(at)) {
      if (!_menuWasUp) _showMenu(at);
      return;
    }
    final caret = jsonEncode(_ov['caret']), objs = _objSig();
    final v = _toView(at);
    e.pointer(1, v.dx, v.dy);
    e.pointer(2, v.dx, v.dy);
    refresh();
    if (e.status()['editingText'] == true) showKeyboard();
    if (!touch) return;
    final rects = (_ov['rects'] as List? ?? []).isNotEmpty;
    // a double tap's word; a tap on the caret where it was, or on the objects selected
    if ((again && rects) || (!_menuWasUp && !rects && _ov['caret'] != null && jsonEncode(_ov['caret']) == caret && _nearCaret(at)) || (!_menuWasUp && objs.isNotEmpty && _objSig() == objs)) {
      _showMenu(at);
    }
  }

  /// `at` (local) within a finger's reach of the caret drawn now.
  bool _nearCaret(Offset at) {
    final c = _ov['caret'];
    if (c is! List || c.length != 4) return false;
    final v = [for (final n in c) (n as num).toDouble()];
    final p = _toLocal(v[0].toInt(), v[1], v[2]);
    final h = v[3] * _shownZoom;
    return (at.dx - p.dx).abs() < 24 && at.dy > p.dy - 16 && at.dy < p.dy + h + 16;
  }

  void _longPress(LongPressStartDetails d) {
    if (_multi || _fingers != 1 || _pinchZoom != null) return;
    _fling.stop();
    HapticFeedback.selectionClick();
    final v = _toView(d.localPosition);
    // the desktop's right-click: what's there selected by its rules, then its menu
    e.pointer(1, v.dx, v.dy, button: 1);
    refresh();
    _showMenu(d.localPosition);
  }

  /// A mouse's right-click: the same edit menu.
  void _secondaryTap(TapUpDetails d) {
    _fling.stop();
    final v = _toView(d.localPosition);
    e.pointer(1, v.dx, v.dy, button: 1);
    refresh();
    _showMenu(d.localPosition);
  }

  bool _onScrollbar(Offset local) => _scrollbarRect() != null && local.dx > _view.width - 16 && local.dy > _top;

  void _scaleStart(ScaleStartDetails d) {
    _fling.stop();
    _lastFocal = d.localFocalPoint;
    _grab = false;
    _scrollbarDrag = false;
    _onControl = false;
    if (d.pointerCount >= 2) {
      _startPinch(d.localFocalPoint);
      return;
    }
    if (_onScrollbar(d.localFocalPoint)) {
      _scrollbarDrag = true;
      return;
    }
    // a shape's point or a guide moves; a button stays put (its tap runs it)
    switch (_controls().hit(_downAt, touch: d.kind != PointerDeviceKind.mouse, dpr: _dpr)) {
      case PointHit(:final s, :final i, :final part):
        _point = PointDrag(s, i, part)..delta = d.localFocalPoint - _downAt;
        e.action('points:sel:$s:$i');
        _setOverlays(e.overlays());
        _paint();
        return;
      case GuideHit(:final page, :final index, :final vertical):
        _guide = GuideDrag(page, index, vertical);
        _guideTo(d.localFocalPoint);
        return;
      case TapHit():
        _onControl = true;
        return;
      case null:
        break;
    }
    final g = _grabAt(d.localFocalPoint);
    if (g != null) {
      _grab = true;
      _shiftDrag = g.$2;
      _grabOffset = g.$1 - d.localFocalPoint;
      final v = _toView(g.$1);
      e.pointer(1, v.dx, v.dy, shift: _shiftDrag);
      _setOverlays(e.overlays());
      _paint();
    }
  }

  void _startPinch(Offset focal) {
    _zoomAtStart = _zoom;
    _pinchZoom = _zoom;
    _pinchScroll = _scroll;
    _docAtFocal = _scroll + _toView(focal) / _zoom;
  }

  void _scaleUpdate(ScaleUpdateDetails d) {
    if (d.pointerCount >= 2) {
      if (_grab) {
        final v = _toView(_lastFocal + _grabOffset);
        e.pointer(2, v.dx, v.dy, shift: _shiftDrag);
        _grab = false;
      }
      _point = null;
      _guide = null;
      if (_pinchZoom == null) _startPinch(d.localFocalPoint);
      final z = (_zoomAtStart * d.scale).clamp(0.25, 4.0);
      _pinchZoom = z;
      // the document point under the fingers stays under them (and follows them)
      _pinchScroll = _docAtFocal - _toView(d.localFocalPoint) / z;
      _paint();
      return;
    }
    if (_pinchZoom != null) return;
    if (_point != null) {
      _point!.delta += d.focalPointDelta;
      _paint();
      return;
    }
    if (_guide != null) {
      _guideTo(d.localFocalPoint);
      return;
    }
    if (_onControl) return;
    if (_scrollbarDrag) {
      final sb = _scrollbarRect();
      if (sb != null) {
        final (track, thumb) = sb;
        final maxY = math.max(0.0, _doc.height - _view.height / _zoom);
        final k = maxY / math.max(1.0, track.height - thumb.height);
        _scroll = _clampScroll(_scroll + Offset(0, d.focalPointDelta.dy * k), _zoom);
        _sync();
        _paint();
        _queueStrips();
      }
      return;
    }
    if (_grab) {
      _lastFocal = d.localFocalPoint;
      final v = _toView(d.localFocalPoint + _grabOffset);
      e.pointer(0, v.dx, v.dy, shift: _shiftDrag);
      _setOverlays(e.overlays());
      _paint();
      return;
    }
    _scroll = _clampScroll(_scroll - d.focalPointDelta / _zoom, _zoom);
    _sync();
    _paint();
    _queueStrips();
  }

  void _scaleEnd(ScaleEndDetails d) {
    if (_pinchZoom != null) {
      final focal = _lastFocalView();
      final z = _pinchZoom!;
      final docAt = _pinchScroll + focal / z;
      _commitZoom(z, focal, docAt);
      return;
    }
    if (_grab) {
      final v = _toView(_lastFocal + _grabOffset);
      e.pointer(2, v.dx, v.dy, shift: _shiftDrag);
      _grab = false;
      refresh();
      // (iOS) a text handle let go: the edit menu back over the selection
      if (_shiftDrag && (_ov['rects'] as List? ?? const []).isNotEmpty) _showMenu(_lastFocal);
      return;
    }
    if (_point != null) {
      final p = _point!;
      _point = null;
      final dd = p.delta / _zoom;
      if (dd.distance > 0) e.action('points:move:${p.s}:${p.i}:${p.part}:${dd.dx.toStringAsFixed(3)}:${dd.dy.toStringAsFixed(3)}');
      refresh();
      return;
    }
    if (_guide != null) return _guideEnd();
    if (_onControl) return;
    if (_scrollbarDrag) {
      _scrollbarDrag = false;
      return;
    }
    // a fling: glides on and slows down (the platform's scrolling physics)
    final vel = -d.velocity.pixelsPerSecond / _zoom;
    if (vel.distance < 50) return;
    _flingFrom = _scroll;
    final maxX = math.max(0.0, _doc.width - _view.width / _zoom);
    final maxY = math.max(0.0, _doc.height - _view.height / _zoom);
    _simX = ClampingScrollSimulation(position: _scroll.dx, velocity: vel.dx);
    _simY = ClampingScrollSimulation(position: _scroll.dy, velocity: vel.dy);
    _flingBounds = Size(maxX, maxY);
    _fling.value = 0;
    _fling.animateWith(_TimeSimulation());
  }

  Size _flingBounds = Size.zero;

  void _flingTick() {
    final t = _fling.value;
    final x = _simX?.x(t) ?? _flingFrom.dx, y = _simY?.x(t) ?? _flingFrom.dy;
    _scroll = Offset(x.clamp(0.0, _flingBounds.width), y.clamp(0.0, _flingBounds.height));
    if ((_simX?.isDone(t) ?? true) && (_simY?.isDone(t) ?? true)) _fling.stop();
    _sync();
    _paint();
    _queueStrips();
  }

  void _lifted() {
    _fingers = math.max(0, _fingers - 1);
    if (_fingers == 0) _multi = false;
  }

  Offset _focalView = Offset.zero;
  Offset _lastFocalView() => _focalView;

  // ---------------------------------------------------------------- the keyboard

  static const _sentinel = '​​';
  String _last = _sentinel;

  void _resetInput() {
    _last = _sentinel;
    _input.value = const TextEditingValue(text: _sentinel, selection: TextSelection.collapsed(offset: 2));
  }

  /// What the on-screen keyboard typed (text, deletions, new paragraphs), as the desktop's keys.
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
    LogicalKeyboardKey.pageUp: 'PageUp', LogicalKeyboardKey.pageDown: 'PageDown', LogicalKeyboardKey.tab: 'Tab',
    LogicalKeyboardKey.escape: 'Escape', LogicalKeyboardKey.delete: 'Delete',
  };

  /// A hardware keyboard's keys: moving the caret, Tab, Escape and the menus' Ctrl shortcuts go
  /// to the engine as the desktop's; typing goes through the text field.
  KeyEventResult _hardKey(FocusNode node, KeyEvent ev) {
    if (ev is KeyUpEvent) return KeyEventResult.ignored;
    final hk = HardwareKeyboard.instance;
    final shift = hk.isShiftPressed, ctrl = hk.isControlPressed || hk.isMetaPressed, alt = hk.isAltPressed;
    final name = _keyNames[ev.logicalKey];
    if (name != null) {
      e.key(name, shift: shift, ctrl: ctrl, alt: alt);
      refresh();
      return KeyEventResult.handled;
    }
    if (ctrl) {
      final label = ev.logicalKey.keyLabel;
      final n = label.length == 1 && RegExp(r'[A-Za-z]').hasMatch(label)
          ? label.toUpperCase()
          : switch (ev.logicalKey) {
              LogicalKeyboardKey.equal => 'Equals',
              LogicalKeyboardKey.minus => 'Minus',
              LogicalKeyboardKey.add => 'Plus',
              _ => null,
            };
      if (n == null) return KeyEventResult.ignored;
      if (n == 'V') {
        Clipboard.getData(Clipboard.kTextPlain).then((c) {
          if (c?.text != null) {
            e.paste(c!.text!);
            refresh();
          }
        });
        return KeyEventResult.handled;
      }
      e.key(n, shift: shift, ctrl: true, alt: alt);
      final copied = e.copied();
      if (copied != null) Clipboard.setData(ClipboardData(text: copied));
      refresh();
      return KeyEventResult.handled;
    }
    return KeyEventResult.ignored;
  }

  /// Text as the on-screen keyboard would type it (automated checks).
  void typeForTest(String t) => _typed(TextEditingValue(text: _last + t));

  void showKeyboard() {
    _resetInput();
    _focus.requestFocus();
    SystemChannels.textInput.invokeMethod('TextInput.show');
  }

  void hideKeyboard() => _focus.unfocus();

  // ---------------------------------------------------------------- scrollbar

  /// canvas.rs scrollbar: the track at the right (under the toolbar's band) and its thumb.
  (Rect, Rect)? _scrollbarRect() {
    final z = _shownZoom;
    final viewH = _view.height / z;
    if (_doc.height <= viewH + 1) return null;
    final top = _top + 4;
    final track = Rect.fromLTRB(_view.width - 14, top, _view.width - 2, _top + _view.height - 4);
    final maxY = _doc.height - viewH;
    final thumbH = (track.height * viewH / _doc.height).clamp(36.0, track.height);
    final thumbY = track.top + (track.height - thumbH) * (_shownScroll.dy / maxY).clamp(0.0, 1.0);
    final w = _scrollbarDrag ? 9.0 : 5.0;
    return (track, Rect.fromLTWH(track.right - 2 - w, thumbY, w, thumbH));
  }

  // ---------------------------------------------------------------- build

  @override
  Widget build(BuildContext context) {
    _dpr = MediaQuery.of(context).devicePixelRatio;
    return LayoutBuilder(builder: (context, box) {
      final view = Size(box.maxWidth, math.max(0, box.maxHeight - _top));
      if (view != _view) {
        final was = _view;
        _view = view;
        // the view getting wider or narrower with Fit Width (a pane sliding in or out): the
        // pages scale with it frame by frame, and the engine re-fits them once it stops
        if (_fit == 'width' && was.width > 0 && (view.width - was.width).abs() > 0.5 && (view.height - was.height).abs() < 0.5 && _pages.isNotEmpty) {
          _slideW0 ??= was.width;
          _slideZoom0 ??= _zoom;
          _pinchZoom = _slideZoom0! * view.width / _slideW0!;
          _pinchScroll = Offset(0, _scroll.dy);
          _slideEnd?.cancel();
          _slideEnd = Timer(const Duration(milliseconds: 90), () {
            if (!mounted) return;
            _slideW0 = _slideZoom0 = null;
            e.view(_view.width, _view.height);
            _readGeometry(e.geometry());
            _pinchZoom = null;
            _sync();
            refresh();
          });
        } else {
          WidgetsBinding.instance.addPostFrameCallback((_) {
            e.view(view.width, view.height);
            // (the engine's zoom first: with a fit mode it fits the new size; sending the old
            // zoom back would end the fit)
            _readGeometry(e.geometry());
            _sync();
            refresh();
          });
        }
      }
      // (a tap anywhere outside the view, the toolbar's say, closes the edit menu)
      return TapRegion(
          onTapOutside: (_) => _hideMenu(),
          child: ClipRect(
          child: Stack(children: [
        Positioned.fill(
          child: MouseRegion(
          onHover: (ev) {
            _hover = ev.localPosition;
            final c = _ov['controls'] as Map?;
            if (c != null && (c['chart'] != null || c['gallery'] != null || c['pageNumber'] != null || (c['media'] as List? ?? []).isNotEmpty)) _paint();
          },
          onExit: (_) {
            _hover = null;
            _paint();
          },
          child: Listener(
          onPointerDown: (ev) {
            _downAt = ev.localPosition;
            // any touch on the page closes the edit menu (a tap on the selection may reopen it)
            _menuWasUp = _menu != null || _listMenuUp;
            _hideMenu();
            _fingers++;
            if (_fingers > 1) _multi = true;
          },
          onPointerUp: (_) => _lifted(),
          onPointerCancel: (_) => _lifted(),
          child: RawGestureDetector(
            behavior: HitTestBehavior.opaque,
            gestures: {
              TapGestureRecognizer: GestureRecognizerFactoryWithHandlers<TapGestureRecognizer>(() => TapGestureRecognizer(), (t) => t
                ..onTapUp = _tapUp
                ..onSecondaryTapUp = _secondaryTap),
              LongPressGestureRecognizer: GestureRecognizerFactoryWithHandlers<LongPressGestureRecognizer>(
                () => LongPressGestureRecognizer(duration: const Duration(milliseconds: 450)),
                (t) => t.onLongPressStart = _longPress,
              ),
              ScaleGestureRecognizer: GestureRecognizerFactoryWithHandlers<ScaleGestureRecognizer>(
                () => ScaleGestureRecognizer(dragStartBehavior: DragStartBehavior.down),
                (t) => t
                  ..onStart = _scaleStart
                  ..onUpdate = (d) {
                    _focalView = _toView(d.localFocalPoint);
                    _scaleUpdate(d);
                  }
                  ..onEnd = _scaleEnd,
              ),
            },
            child: CustomPaint(
              painter: _DocPainter(this),
              child: const SizedBox.expand(),
            ),
          ),
          ),
          ),
        ),
        // a selected table's chrome (tabs, add buttons, cell editor)
        if (_table != null && _pinchZoom == null)
          Positioned.fill(
            child: ValueListenableBuilder<int>(
              valueListenable: _repaint,
              builder: (ctx, _, _) => _table == null
                  ? const SizedBox.shrink()
                  : TableChrome(engine: e, t: _table!, toLocal: _toLocal, toView: _toView, zoom: _zoom, onChanged: refresh, onMenu: widget.onMenu),
            ),
          ),
        // what shows under the floating toolbar (Settings > Toolbar, canvas.rs paint_toolbar_band):
        // Pages' tint over the pages scrolled up under it, blurred; Keydeck's feathered blur,
        // thinning out to sharp below the toolbar; or the plain band
        if (widget.topInset > 0) ..._toolbarBand(),
        if (widget.ruler != null && _pages.isNotEmpty)
          Positioned(
            left: 0,
            right: 0,
            top: widget.topInset,
            height: 34,
            child: ValueListenableBuilder<int>(valueListenable: _repaint, builder: (ctx, _, _) => _Ruler(s: widget.ruler!, state: this)),
          ),
        // View > Show Word Count (notes.rs word_count_pill): tap for the next count
        if (widget.wordCount != null)
          Positioned(
            left: 0,
            right: 0,
            bottom: 26 - 13.5,
            child: Center(
              child: GestureDetector(
                onTap: () {
                  e.action('wc:');
                  widget.onChanged();
                },
                child: Container(
                  height: 27,
                  padding: const EdgeInsets.symmetric(horizontal: 14),
                  decoration: BoxDecoration(color: widget.look.gray(0x2c).withValues(alpha: 0.96), borderRadius: BorderRadius.circular(13.5)),
                  alignment: Alignment.center,
                  child: Text('${widget.wordCount}  ⇕', style: Look.font(12.5, widget.look.text)),
                ),
              ),
            ),
          ),
        // View > Enter Screen View (canvas.rs): a pill at the top right of the pages
        if (widget.screenView)
          Positioned(
            top: _top + 10,
            right: 14,
            width: 176,
            height: 26,
            child: IgnorePointer(
              child: Container(
                decoration: BoxDecoration(color: widget.look.gray(0x30).withValues(alpha: 0.92), borderRadius: BorderRadius.circular(13)),
                child: Row(children: [
                  const SizedBox(width: 10),
                  Container(width: 8, height: 8, decoration: BoxDecoration(color: widget.look.accent, shape: BoxShape.circle)),
                  const SizedBox(width: 8),
                  Text('Screen View is on.', style: Look.font(12.5, widget.look.text)),
                ]),
              ),
            ),
          ),
        // a message (canvas.rs): a pill near the bottom
        if (widget.message != null)
          Positioned(
            left: 20,
            right: 20,
            bottom: 60 - 17,
            child: IgnorePointer(
              child: Center(
                child: Container(
                  height: 34,
                  padding: const EdgeInsets.symmetric(horizontal: 20),
                  decoration: BoxDecoration(color: widget.look.gray(0x30).withValues(alpha: 0.95), borderRadius: BorderRadius.circular(17)),
                  alignment: Alignment.center,
                  child: Text(widget.message!, maxLines: 1, overflow: TextOverflow.ellipsis, style: Look.font(13, widget.look.text)),
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
        // the edit menu over the selection (kept with the keyboard up while it's tapped)
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
                        bounds: Rect.fromLTWH(0, _top, _view.width, _view.height),
                        look: widget.look,
                        onPick: (a) {
                          _hideMenu();
                          widget.onMenuAction(a);
                        },
                      ),
              ),
            ),
          ),
      ])));
    });
  }
}

/// A clock for the fling: its value is the seconds since it started.
class _TimeSimulation extends Simulation {
  @override
  double x(double time) => time;
  @override
  double dx(double time) => 1;
  @override
  bool isDone(double time) => time > 8;
}

/// `v` as an f32 (the engine's arithmetic).
final _f32Box = Float32List(1);
double _f32(double v) {
  _f32Box[0] = v;
  return _f32Box[0];
}

/// The pages and what's over them, as canvas.rs paint_pages and paint_overlay draw them.
class _DocPainter extends CustomPainter {
  _DocPainter(this.s, {this.band = false}) : super(repaint: band ? s._repaint : Listenable.merge([s._repaint, s._blink]));
  final DocViewState s;

  /// The copy under the floating toolbar (feathered): the pages only, no caret or handles.
  final bool band;

  List<double> _l(dynamic v) => [for (final n in v as List) (n as num).toDouble()];

  @override
  void paint(Canvas canvas, Size size) {
    final look = s.widget.look;
    canvas.drawRect(Offset.zero & size, Paint()..color = look.canvas);
    if (s._pages.isEmpty) return;
    final z = s._shownZoom;
    final vis = s._visibleDoc();
    final scale = s._scaleFor(s._zoom);
    for (var p = 0; p < s._pages.length; p++) {
      final r = s._pages[p];
      if (!r.overlaps(vis)) continue;
      final pr = Rect.fromPoints(s._toLocal(p, 0, 0), s._toLocal(p, r.width, r.height));
      // a soft shadow all round the page (Pages'), which also parts the pages
      canvas.drawRect(pr.shift(const Offset(0, 1)), Paint()
        ..color = const Color.fromARGB(45, 0, 0, 0)
        ..maskFilter = const MaskFilter.blur(BlurStyle.normal, 4));
      canvas.drawRect(pr, Paint()..color = Colors.white);
      final n = (r.height / s._stripH).ceil();
      for (var k = 0; k < n; k++) {
        final strip = s._strips[s._sk(p, k)];
        final img = strip?.image;
        if (img == null) continue;
        // the engine's strip: page pixel rows round(k × strip × scale) to the next strip's (f32,
        // as it works them out), so strips meet on whole pixels
        final ss = strip!.scale;
        final row0 = _f32(_f32(k * _f32(s._stripH)) * _f32(ss)).roundToDouble(), row1 = _f32(_f32((k + 1) * _f32(s._stripH)) * _f32(ss)).roundToDouble();
        // (the last one no further than the page's foot)
        final rows = math.min(img.height.toDouble(), math.min(row1, (r.height * ss).ceilToDouble()) - row0);
        final src = Rect.fromLTWH(0, 0, math.min(img.width.toDouble(), r.width * ss), rows);
        final exact = (ss - scale).abs() < 1e-3 && s._pinchZoom == null;
        final d = s._dpr;
        Rect dst;
        if (exact) {
          // pixel for pixel: on the screen's pixel grid, unfiltered
          final o = s._toLocal(p, 0, 0);
          final (left, top) = ((o.dx * d).roundToDouble() / d, (o.dy * d).roundToDouble() / d);
          dst = Rect.fromLTWH(left, top + row0 / d, src.width / d, rows / d);
        } else {
          // scaled (a zoom going on, a strip from before it): a pixel over the next strip, which
          // hides its soft edge
          dst = Rect.fromPoints(s._toLocal(p, 0, row0 / ss), s._toLocal(p, r.width, (row0 + rows) / ss));
          if (k < n - 1) dst = Rect.fromLTRB(dst.left, dst.top, dst.right, dst.bottom + 1 / d);
        }
        if (dst.bottom < 0 || dst.top > size.height) continue;
        canvas.drawImageRect(img, src, dst, Paint()..filterQuality = exact ? FilterQuality.none : FilterQuality.medium);
      }
      canvas.drawRect(pr.inflate(0.25), Paint()
        ..style = PaintingStyle.stroke
        ..strokeWidth = 0.5
        ..color = const Color(0xFF4D4D4D));
    }
    if (band) return;
    _overlay(canvas, size, z, look);
    // the scrollbar
    final sb = s._scrollbarRect();
    if (sb != null) {
      final (track, thumb) = sb;
      final hot = s._scrollbarDrag;
      if (hot) {
        canvas.drawRRect(RRect.fromRectAndRadius(Rect.fromLTRB(track.right - 3 - thumb.width, track.top, track.right - 1, track.bottom), const Radius.circular(6)), Paint()..color = look.gray(0x80).withValues(alpha: 0.12));
      }
      final ink = look.light ? Color.fromARGB(hot ? 150 : 110, 0, 0, 0) : Color.fromARGB(hot ? 170 : 120, 255, 255, 255);
      canvas.drawRRect(RRect.fromRectAndRadius(thumb, Radius.circular(thumb.width / 2)), Paint()..color = ink);
    }
  }

  void _overlay(Canvas canvas, Size size, double z, Look look) {
    final ov = s._ov;
    final accent = look.accent;
    Offset at(int page, double x, double y) => s._toLocal(page, x, y);
    final np = s._pages.length;
    // header / footer fields while the caret is in one (#D9DCDE outlines)
    for (final r in (ov['hf'] as List? ?? [])) {
      final v = _l(r);
      if (v[0].toInt() >= np) continue;
      canvas.drawRRect(RRect.fromRectAndRadius(Rect.fromPoints(at(v[0].toInt(), v[1], v[2]), at(v[0].toInt(), v[3], v[4])), const Radius.circular(2)), Paint()
        ..style = PaintingStyle.stroke
        ..strokeWidth = 1
        ..color = const Color(0xFFD9DCDE));
    }
    // find matches (the current one stronger)
    for (final r in (ov['find'] as List? ?? [])) {
      final l = r as List;
      final v = _l(l.sublist(0, 5));
      if (v[0].toInt() >= np) continue;
      canvas.drawRRect(RRect.fromRectAndRadius(Rect.fromPoints(at(v[0].toInt(), v[1], v[2]), at(v[0].toInt(), v[3], v[4])), const Radius.circular(2)), Paint()..color = l[5] == true ? const Color.fromARGB(170, 0xff, 0xd6, 0x00) : const Color.fromARGB(110, 0xff, 0xe8, 0x70));
    }
    // misspelled words: red dots along their baseline
    final k = z.clamp(0.8, 1.6);
    for (final r in (ov['spell'] as List? ?? [])) {
      final v = _l(r);
      if (v[0].toInt() >= np) continue;
      final a = at(v[0].toInt(), v[1], v[3]), b = at(v[0].toInt(), v[2], v[3]);
      for (var x = a.dx; x < b.dx; x += 3.5 * k) {
        canvas.drawCircle(Offset(x, a.dy), 1.1 * k, Paint()..color = const Color(0xFFE03030));
      }
    }
    // the text selection (theme::ACCENT().gamma_multiply(0.28))
    final rects = [for (final r in (ov['rects'] as List? ?? [])) _l(r)];
    for (final v in rects) {
      if (v[0].toInt() >= s._pages.length) continue;
      canvas.drawRect(Rect.fromPoints(at(v[0].toInt(), v[1], v[2]), at(v[0].toInt(), v[3], v[4])), Paint()..color = accent.withValues(alpha: 0.28));
    }
    // its handles (kn_ui touch.rs selection_handles)
    if (rects.isNotEmpty) {
      final f = rects.first, l = rects.last;
      if (f[0].toInt() < s._pages.length && l[0].toInt() < s._pages.length) {
        final st = [at(f[0].toInt(), f[1], f[2]), at(f[0].toInt(), f[1], f[4])];
        final en = [at(l[0].toInt(), l[3], l[2]), at(l[0].toInt(), l[3], l[4])];
        final line = Paint()
          ..color = accent
          ..strokeWidth = 2;
        for (final (ends, knob) in [(st, st[0] - const Offset(0, 7)), (en, en[1] + const Offset(0, 7))]) {
          canvas.drawLine(ends[0], ends[1], line);
          canvas.drawCircle(knob, 5.5, Paint()..color = accent);
        }
      }
    }
    // the caret, blinking (on 0.62 s of every 1.06 s from the last change)
    final c = ov['caret'];
    if (c is List && c.length == 4 && s._blink.value * 1.06 < 0.62) {
      final v = _l(c);
      if (v[0].toInt() < s._pages.length) {
        final a = at(v[0].toInt(), v[1], v[2]);
        canvas.drawRect(Rect.fromLTWH(a.dx, a.dy, (1.6 * z).clamp(1.5, 3.0), v[3] * z), Paint()..color = accent);
      }
    }
    // the moving objects themselves at their live place (draw_live_object: 235 of 255 opaque);
    // after a drop, where they were dropped until the strips show them there
    final shown = ov['live'] is Map ? ov['live'] as Map : s._dropped;
    if (shown != null) {
      final d = Offset((shown['dx'] as num).toDouble(), (shown['dy'] as num).toDouble());
      for (final o in (shown['objects'] as List? ?? [])) {
        final img = s._liveImages[o['id']]?.$2;
        final page = o['page'] as int;
        if (img == null || page >= np) continue;
        final b = _l(o['box']);
        final dst = Rect.fromPoints(at(page, b[0] + d.dx, b[1] + d.dy), at(page, b[0] + b[2] + d.dx, b[1] + b[3] + d.dy));
        canvas.drawImageRect(img, Rect.fromLTWH(0, 0, img.width.toDouble(), img.height.toDouble()), dst, Paint()
          ..filterQuality = FilterQuality.medium
          ..color = const Color.fromARGB(235, 255, 255, 255));
      }
    }
    // selected objects: frame and handles
    final live = ov['live'];
    final liveOff = live is Map ? Offset((live['dx'] as num).toDouble(), (live['dy'] as num).toDouble()) : Offset.zero;
    final moving = live is Map ? [for (final i in (live['ids'] as List? ?? [])) i as int] : <int>[];
    final lf = ov['liveFrame'];
    for (final o in (ov['objects'] as List? ?? [])) {
      final page = o['page'] as int;
      if (page >= s._pages.length) continue;
      var f = _l(o['frame']);
      if (lf is Map && lf['id'] == o['id']) {
        final g = _l(lf['frame']);
        f = [g[0], g[1], g[0] + math.max(g[2], 1), g[1] + math.max(g[3], 1)];
      }
      if (moving.contains(o['id'])) f = [f[0] + liveOff.dx, f[1] + liveOff.dy, f[2] + liveOff.dx, f[3] + liveOff.dy];
      final r = Rect.fromPoints(at(page, f[0], f[1]), at(page, f[2], f[3]));
      canvas.drawRect(r, Paint()
        ..style = PaintingStyle.stroke
        ..strokeWidth = 1
        ..color = accent);
      if (o['editing'] == true || moving.contains(o['id'])) continue;
      for (final h in _handlePoints(r)) {
        final hr = Rect.fromCenter(center: h, width: 8, height: 8);
        if (o['locked'] == true) {
          final tp = TextPainter(text: TextSpan(text: '×', style: Look.font(11, const Color(0xFF808080))), textDirection: TextDirection.ltr)..layout();
          tp.paint(canvas, h - Offset(tp.width / 2, tp.height / 2));
        } else {
          canvas.drawRRect(RRect.fromRectAndRadius(hr, const Radius.circular(1)), Paint()..color = Colors.white);
          canvas.drawRRect(RRect.fromRectAndRadius(hr, const Radius.circular(1)), Paint()
            ..style = PaintingStyle.stroke
            ..strokeWidth = 1
            ..color = const Color(0xFF808080));
        }
      }
    }
    // size and position while dragging (Settings > Rulers): a dark label right of the frame
    for (final m in (ov['measure'] as List? ?? [])) {
      final page = m[0] as int;
      if (page >= np) continue;
      final a = at(page, (m[1] as num).toDouble(), (m[2] as num).toDouble()) + const Offset(12, -6);
      final tp = TextPainter(text: TextSpan(text: m[3] as String, style: Look.font(11.5, Colors.white)), textDirection: TextDirection.ltr)..layout();
      canvas.drawRRect(RRect.fromRectAndRadius(Rect.fromLTWH(a.dx, a.dy, tp.width + 12, tp.height + 6), const Radius.circular(5)), Paint()..color = const Color.fromARGB(190, 0, 0, 0));
      tp.paint(canvas, a + const Offset(6, 3));
    }
    // snapping guides (Keynote's yellow)
    if (live is Map) {
      final page = live['page'] as int;
      if (page < s._pages.length) {
        for (final g in (live['guides'] as List? ?? [])) {
          final v = _l(g);
          canvas.drawLine(at(page, v[0], v[1]), at(page, v[2], v[3]), Paint()
            ..strokeWidth = 1
            ..color = const Color(0xFFF5D02A));
        }
      }
    }
    // the on-page controls (points, guides, buttons, the movie playing)
    final cc = s._controls();
    cc.paint(canvas, look, hover: s._hover, pageNumberHot: s._pageNumberMenu, guide: s._guide, point: s._point, movie: s._movie, movieId: s._movieId, units: ((ov['controls'] as Map?)?['guides'] as Map?)?['units'] as int? ?? 0);
    // the marquee
    final m = ov['marquee'];
    if (m is List) {
      final v = _l(m);
      if (v[0].toInt() < s._pages.length) {
        final r = Rect.fromPoints(at(v[0].toInt(), v[1], v[2]), at(v[0].toInt(), v[3], v[4]));
        canvas.drawRect(r, Paint()..color = accent.withValues(alpha: 0.12));
        canvas.drawRect(r, Paint()
          ..style = PaintingStyle.stroke
          ..strokeWidth = 1
          ..color = accent);
      }
    }
  }

  /// canvas.rs handle_points: the corners and the middles of the sides (Values' order).
  List<Offset> _handlePoints(Rect r) => [r.topLeft, r.topCenter, r.topRight, r.centerLeft, r.centerRight, r.bottomLeft, r.bottomCenter, r.bottomRight];

  @override
  bool shouldRepaint(_DocPainter o) => false;
}

/// View > Show Ruler (ruler.rs): the page's width with its margins and gutters shaded, ticks in
/// the ruler's units, the caret paragraph's indents as markers; drag a margin, gutter edge or
/// marker to change it.
class _Ruler extends StatefulWidget {
  const _Ruler({required this.s, required this.state});
  final Map<String, dynamic> s;
  final DocViewState state;
  @override
  State<_Ruler> createState() => _RulerState();
}

/// The ruler's ticks in its units (ruler.rs): the step between ticks in points, and every how
/// many ticks a numbered one comes.
(double, int) _rulerSteps(Object? units) => switch (units) { 1 => (72 / 8, 8), 2 => (12.0, 6), 3 => (72 / 25.4 * 5, 2), _ => (72 / 2.54 / 2, 2) };

class _RulerState extends State<_Ruler> {
  String? _grab;
  double? _at;

  /// The dragged edge's value (points), on a tick; a tick of haptics each time it moves on one.
  double? _snapped;

  double get z => widget.state._shownZoom;
  double _left() => widget.state._toLocal(widget.s['page'] as int? ?? 0, 0, 0).dx;
  double n(String k) => (widget.s[k] as num?)?.toDouble() ?? 0;

  /// The markers and edges near `x` (their keys), nearest first: indents win over margins.
  String? _near(double x, double y) {
    final l = _left();
    double xo(double pt) => l + pt * z;
    final c = <(double, String)>[(xo(n('left')), 'margin:L'), (xo(n('width') - n('right')), 'margin:R')];
    final ind = widget.s['indents'] as List?;
    if (ind != null) {
      c.add((xo(n('colL') + (ind[0] as num).toDouble()), 'first'));
      c.add((xo(n('colL') + (ind[1] as num).toDouble()), 'left'));
      c.add((xo(n('colR') - (ind[2] as num).toDouble()), 'right'));
    }
    for (final (k, g) in (widget.s['gutters'] as List? ?? []).indexed) {
      c.add((xo((g[0] as num).toDouble()), 'gutter:$k'));
      c.add((xo((g[1] as num).toDouble()), 'gutter:$k'));
    }
    // (by height too: the first-line marker at the top, the indents at the bottom, the
    // margins' grips in the middle)
    double score((double, String) g) =>
        (x - g.$1).abs() +
        switch (g.$2) {
          'first' => y < 12 ? 0 : 6,
          'left' || 'right' => y > 22 ? 0 : 6,
          _ when g.$2.startsWith('margin') => y >= 10 && y <= 24 ? 0 : 4,
          _ => 2,
        };
    // (a finger's reach: 22 points either side)
    final near = c.where((g) => (x - g.$1).abs() < 22).toList()..sort((a, b) => score(a).compareTo(score(b)));
    return near.isEmpty ? null : near.first.$2;
  }

  /// The finger at `x`: the nearest tick (in the ruler's units), haptics when it changes.
  void _snap(double x) {
    final (step, _) = _rulerSteps(widget.s['units']);
    // (in steps from what it measures from: a right margin from the page's right edge, an
    // indent from its column's edge)
    final g = _grab ?? '';
    final ref = switch (g) { 'margin:R' => n('width'), 'right' => n('colR'), 'first' || 'left' => n('colL'), _ => 0.0 };
    final v = (ref + (((x - _left()) / z - ref) / step).round() * step).clamp(0.0, n('width'));
    if (_snapped != null && v != _snapped) HapticFeedback.selectionClick();
    _snapped = v;
  }

  @override
  Widget build(BuildContext context) {
    final look = widget.state.widget.look;
    // (where the finger went down picks the marker, not where the drag got going)
    return GestureDetector(
      behavior: HitTestBehavior.opaque,
      dragStartBehavior: DragStartBehavior.down,
      onPanStart: (d) => setState(() {
        _grab = _near(d.localPosition.dx, d.localPosition.dy);
        _at = d.localPosition.dx;
        _snapped = null;
        // away from the markers: a new horizontal guide, dragged down onto the page
        if (_grab == null) {
          widget.state.startRulerGuide();
        } else {
          _snap(d.localPosition.dx);
        }
      }),
      onPanUpdate: (d) {
        if (widget.state._guide != null) return widget.state._guideTo(d.localPosition + Offset(0, widget.state.widget.topInset));
        setState(() {
          _at = d.localPosition.dx;
          _snap(_at!);
        });
      },
      onPanEnd: (_) {
        if (widget.state._guide != null) return widget.state._guideEnd();
        final g = _grab, v = _snapped;
        setState(() => _grab = null);
        if (g == null || v == null) return;
        widget.state.e.action('ruler:$g:${v.toStringAsFixed(2)}');
        widget.state.refresh();
      },
      child: CustomPaint(painter: _RulerPainter(widget.s, _left(), z, look, _grab == null || _snapped == null ? null : _left() + _snapped! * z, _grab)),
    );
  }
}

class _RulerPainter extends CustomPainter {
  _RulerPainter(this.s, this.left, this.z, this.look, this.drag, [this.grab]);
  final Map<String, dynamic> s;
  final double left, z;
  final Look look;
  final double? drag;

  /// What's dragged (its key): the label gives a margin or indent, not the place.
  final String? grab;

  double n(String k) => (s[k] as num?)?.toDouble() ?? 0;

  @override
  void paint(Canvas c, Size size) {
    double xo(double pt) => left + pt * z;
    final rect = Offset.zero & size;
    c.drawRect(rect, Paint()..color = look.panel);
    final w = n('width');
    final band = Rect.fromLTRB(xo(0), 4, xo(w), size.height - 4);
    c.drawRRect(RRect.fromRectAndRadius(band, const Radius.circular(2)), Paint()..color = look.gray(0x5a));
    c.drawRect(Rect.fromLTRB(xo(n('left')), band.top, xo(w - n('right')), band.bottom), Paint()..color = const Color(0xFFF2F2F2));
    for (final g in (s['gutters'] as List? ?? [])) {
      final gr = Rect.fromLTRB(xo((g[0] as num).toDouble()), band.top, xo((g[1] as num).toDouble()), band.bottom);
      c.drawRect(gr, Paint()..color = look.gray(0x5a));
      for (final x in [gr.left, gr.right]) {
        c.drawRRect(RRect.fromRectAndRadius(Rect.fromCenter(center: Offset(x, band.center.dy), width: 3, height: band.height - 6), const Radius.circular(1)), Paint()..color = const Color(0xFF9A9A9A));
      }
    }
    final (step, every) = _rulerSteps(s['units']);
    final tick = Paint()
      ..strokeWidth = 1
      ..color = const Color(0xFF707070);
    var i = 0;
    for (var v = 0.0; v <= w; v += step, i++) {
      final x = xo(v);
      final major = i % every == 0;
      c.drawLine(Offset(x, band.bottom - (major ? 8 : 4)), Offset(x, band.bottom), tick);
      if (major && i > 0) {
        final tp = TextPainter(text: TextSpan(text: '${i ~/ every}', style: Look.font(9, const Color(0xFF505050))), textDirection: TextDirection.ltr)..layout();
        tp.paint(c, Offset(x + 2, band.top + 1));
      }
    }
    final ind = s['indents'] as List?;
    if (ind != null) {
      const blue = Color(0xFF3D8BF0);
      // (touch-sized: 16 wide, 11 deep)
      void marker(double x, bool down) {
        final (y0, y1) = down ? (band.top, band.top + 11) : (band.bottom, band.bottom - 11);
        final p = Path()..addPolygon([Offset(x - 8, y0), Offset(x + 8, y0), Offset(x, y1)], true);
        c.drawPath(p, Paint()..color = blue);
        c.drawPath(p, Paint()
          ..style = PaintingStyle.stroke
          ..color = const Color(0xFF303030));
      }

      marker(xo(n('colL') + (ind[0] as num).toDouble()), true);
      marker(xo(n('colL') + (ind[1] as num).toDouble()), false);
      marker(xo(n('colR') - (ind[2] as num).toDouble()), false);
    }
    // the margins' handles: a grip at each edge of the white
    for (final x in [xo(n('left')), xo(w - n('right'))]) {
      final r = RRect.fromRectAndRadius(Rect.fromCenter(center: Offset(x, band.center.dy), width: 6, height: band.height - 4), const Radius.circular(3));
      c.drawRRect(r, Paint()..color = const Color(0xFF8A8A8A));
      c.drawRRect(r, Paint()
        ..style = PaintingStyle.stroke
        ..strokeWidth = 1
        ..color = const Color(0xFFFFFFFF));
    }
    if (drag != null) {
      c.drawLine(Offset(drag!, 0), Offset(drag!, size.height), Paint()
        ..strokeWidth = 2
        ..color = look.accent);
      // the value it lands on, in the ruler's units
      final at = (drag! - left) / z;
      final pts = switch (grab) { 'margin:R' => n('width') - at, 'right' => n('colR') - at, 'first' || 'left' => at - n('colL'), _ => at };
      final (u, d) = switch (s['units']) { 1 => ('in', pts / 72), 2 => ('pt', pts), 3 => ('mm', pts / 72 * 25.4), _ => ('cm', pts / 72 * 2.54) };
      final tp = TextPainter(text: TextSpan(text: '${d.toStringAsFixed(s['units'] == 2 || s['units'] == 3 ? 0 : 2)} $u', style: Look.font(12, Colors.white, bold: true)), textDirection: TextDirection.ltr)..layout();
      final bx = (drag! + 8 + tp.width + 12 > size.width) ? drag! - 8 - tp.width - 12 : drag! + 8;
      final pill = RRect.fromRectAndRadius(Rect.fromLTWH(bx, (size.height - tp.height - 6) / 2, tp.width + 12, tp.height + 6), const Radius.circular(8));
      c.drawRRect(pill, Paint()..color = look.accent);
      tp.paint(c, Offset(bx + 6, pill.top + 3));
    }
  }

  @override
  bool shouldRepaint(_RulerPainter o) => true;
}
