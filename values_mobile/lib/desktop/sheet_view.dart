// The sheet, as the desktop canvas shows it (nm-app canvas.rs: the white sheet, the selected
// objects' frames and handles, the text being edited, commented cells' corners, a formula's
// references) — scrolled, flung and pinch-zoomed by Flutter. The sheet's content is the
// engine's, drawn in 512-pixel tiles that are kept and only drawn again when the sheet changes
// (or, after a zoom, sharper); a pinch scales what's there meanwhile. The selected table's
// chrome (table_chrome.dart) and the cell editor sit over it.
// Touches: one finger scrolls; on the selection (an object, its handles, the cells' grab dots,
// the text's handles) it moves that instead; a tap is the desktop's click (twice: a double
// click); touch and hold, its right-click.
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
import 'table_chrome.dart';
import 'touch_menu.dart';

class SheetView extends StatefulWidget {
  const SheetView({super.key, required this.engine, required this.look, required this.onChanged, required this.onMenuAction, required this.onMenu, this.message, this.compact = false});
  final Engine engine;
  final Look look;

  /// The sheet, selection or cell being edited changed (the chrome and inspector follow).
  final VoidCallback onChanged;

  /// An edit menu item picked (its action).
  final void Function(String action) onMenuAction;

  /// A menu of these items at this point (global): a table tab's.
  final void Function(List<dynamic> items, Offset global) onMenu;

  /// A message to show in the message pill.
  final String? message;

  /// The phone layout.
  final bool compact;

  @override
  State<SheetView> createState() => SheetViewState();
}

class _Tile {
  _Tile(this.version, this.scale, this.image);
  int version;
  double scale;
  ui.Image? image;
}

class SheetViewState extends State<SheetView> with TickerProviderStateMixin {
  Engine get e => widget.engine;

  // ---- the view ----
  double _zoom = 1;
  Offset _scroll = Offset.zero; // sheet point at the view's top-left
  Size _view = Size.zero;
  double _dpr = 2;
  Rect _content = Rect.zero;
  int _version = -1, _sheet = -1;
  int _tilePx = 512;
  Map<String, dynamic> _ov = {};
  Map<String, dynamic>? _table;

  // a pinch going on: the zoom it shows and the scroll for it
  double? _pinchZoom;
  Offset _pinchScroll = Offset.zero;
  double _zoomAtStart = 1;
  Offset _sheetAtFocal = Offset.zero;

  double get zoom => _zoom;
  double get _shownZoom => _pinchZoom ?? _zoom;
  Offset get _shownScroll => _pinchZoom != null ? _pinchScroll : _scroll;

  // ---- tiles ----
  final _tiles = <(int, int), _Tile>{};

  /// Earlier zooms' tiles, drawn stretched under the new ones until those are in.
  final _old = <(double, Map<(int, int), _Tile>)>[];
  double _tileScale = 0;
  bool _rendering = false, _again = false;

  // ---- frozen headers ----
  /// Pinned header pictures: what they were drawn for (table, kind, rows, scale, version) →
  /// the picture; null while it's being drawn.
  final _headers = <String, ui.Image?>{};

  /// The header picture for `key`, asked for once (painted when it comes).
  ui.Image? _header(int id, bool cols, int r0, int r1) {
    final scale = _scaleFor(_zoom);
    final key = '$id $cols $r0 $r1 $scale $_version';
    if (_headers.containsKey(key)) return _headers[key];
    _headers[key] = null;
    e.renderHeader(id, cols, r0, r1, scale).then((img) {
      if (!mounted) return img?.dispose();
      // (older pictures of the same strip go)
      final prefix = '$id $cols ';
      for (final k in _headers.keys.where((k) => k.startsWith(prefix) && k != key).toList()) {
        _headers.remove(k)?.dispose();
      }
      _headers[key] = img;
      _paint();
    });
    // (meanwhile the last one of this strip, if any)
    for (final en in _headers.entries) {
      if (en.key.startsWith('$id $cols ') && en.value != null) return en.value;
    }
    return null;
  }

  // ---- motion ----
  late final AnimationController _fling = AnimationController.unbounded(vsync: this)..addListener(_flingTick);
  Simulation? _simX, _simY;
  Offset _flingFrom = Offset.zero;
  late final AnimationController _blink = AnimationController(vsync: this, duration: const Duration(milliseconds: 1060))..repeat();

  /// A zoom step easing (menus, the zoom popover): from, to, the view point kept still.
  late final AnimationController _zoomAnim = AnimationController(vsync: this, duration: const Duration(milliseconds: 285))..addListener(_zoomTick);
  double _za0 = 1, _za1 = 1;
  Offset _zaFocal = Offset.zero, _zaSheet = Offset.zero;

  // ---- touches ----
  /// what a one-finger drag does: scroll, or move the selection (an engine drag)
  bool _grab = false;
  Offset _grabOffset = Offset.zero;
  bool _shiftDrag = false;

  /// dragging one of the selected cells' grab dots: the corner kept still
  (int, int)? _cellAnchor;
  Offset _lastFocal = Offset.zero;
  int _fingers = 0;
  bool _multi = false;
  Offset _downAt = Offset.zero;
  PointerDeviceKind _downKind = PointerDeviceKind.touch;

  // ---- the edit menu ----
  List<dynamic>? _menu;
  Offset _menuPoint = Offset.zero;
  String _menuSig = '';
  bool _menuWasUp = false;

  // ---- the keyboard ----
  final _input = TextEditingController();
  late final FocusNode _focus = FocusNode(onKeyEvent: _hardKey);

  /// The sheet's own keys (cells selected, an object: arrows, typing over a cell) with no
  /// on-screen keyboard.
  late final FocusNode _keys = FocusNode(onKeyEvent: _sheetKey);

  final _repaint = ValueNotifier(0);

  /// The cell being edited last frame (to bring a new one into view above the keyboard).
  String _editingCell = '';

  @override
  void initState() {
    super.initState();
    _resetInput();
  }

  @override
  void dispose() {
    _fling.dispose();
    _blink.dispose();
    _zoomAnim.dispose();
    _input.dispose();
    _focus.dispose();
    _keys.dispose();
    for (final t in _tiles.values) {
      t.image?.dispose();
    }
    for (final (_, set) in _old) {
      for (final t in set.values) {
        t.image?.dispose();
      }
    }
    for (final i in _headers.values) {
      i?.dispose();
    }
    super.dispose();
  }

  void _paint() => _repaint.value++;

  // ---------------------------------------------------------------- the engine's state

  /// Reads the view and the overlays again (after anything that may have changed them); the
  /// engine may have moved the view (a sheet opened, a cell kept in view).
  void refresh() {
    final g = e.geometry();
    _readGeometry(g, takeScroll: _pinchZoom == null && !_zoomAnim.isAnimating);
    _setOverlays(e.overlays());
    _table = e.table();
    _blink.forward(from: 0);
    _blink.repeat();
    _paint();
    _queueTiles();
    _followEditor();
    // the sheet's keys: when cells or an object are selected and nothing types elsewhere
    final st = e.status();
    if (st['editingText'] == true) {
      if (!_focus.hasFocus) showKeyboard();
    } else if ((st['cells'] == true || (st['objects'] as int? ?? 0) > 0) && st['editingCell'] != true) {
      // (only when nothing else has the keys: the find field, a dialog's field keep them)
      final f = FocusManager.instance.primaryFocus;
      if (f == null || f == _focus || (f.context != null && f.context!.findAncestorStateOfType<EditableTextState>() == null && st['find'] == null)) {
        if (_focus.hasFocus) _focus.unfocus();
        _keys.requestFocus();
      }
    }
    widget.onChanged();
  }

  void _readGeometry(Map<String, dynamic> g, {bool takeScroll = true}) {
    final c = g['content'] as List?;
    if (c != null) _content = Rect.fromLTRB((c[0] as num).toDouble(), (c[1] as num).toDouble(), (c[2] as num).toDouble(), (c[3] as num).toDouble());
    final v = (g['version'] as int?) ?? _version;
    final sheet = (g['sheet'] as int?) ?? _sheet;
    _tilePx = (g['tile'] as int?) ?? 512;
    if (sheet != _sheet) {
      // another sheet: none of the tiles are of it
      _sheet = sheet;
      _dropTiles();
    }
    _version = v;
    if (takeScroll) {
      _zoom = (g['zoom'] as num?)?.toDouble() ?? _zoom;
      final s = g['scroll'] as List?;
      if (s != null) {
        final want = Offset((s[0] as num).toDouble(), (s[1] as num).toDouble());
        if ((want - _scroll).distance > 0.5) {
          _fling.stop();
          _scroll = want;
        }
      }
    }
  }

  void _dropTiles() {
    for (final t in _tiles.values) {
      t.image?.dispose();
    }
    _tiles.clear();
    for (final (_, set) in _old) {
      for (final t in set.values) {
        t.image?.dispose();
      }
    }
    _old.clear();
  }

  void _setOverlays(Map<String, dynamic> ov) {
    _ov = ov;
    if (_menu != null && _selSig() != _menuSig) _menu = null;
  }

  /// Tells the engine where the view is now.
  void _sync() => e.scroll(_scroll.dx, _scroll.dy, _zoom);

  /// The view kept on the sheet: from 44 / 96 points before the content to ~350 / ~311 past it.
  Offset _clampScroll(Offset s, double z) {
    // (api.rs TOUCH_MARGIN: room for the table chrome's bars above a table's name)
    final minX = _content.left - 44 / z, minY = _content.top - 96 / z;
    final maxX = math.max(minX, _content.right + 350 / z - _view.width / z);
    final maxY = math.max(minY, _content.bottom + 311 / z - _view.height / z);
    return Offset(s.dx.clamp(minX, maxX), s.dy.clamp(minY, maxY));
  }

  // ---------------------------------------------------------------- zoom

  /// Zoom to `z`, eased over ~285 ms (Numbers' zoom steps); `at` (a view point) stays still
  /// (default: the top-left).
  void setZoom(double z, {Offset? at}) {
    z = z.clamp(0.25, 4.0);
    _fling.stop();
    _za0 = _zoom;
    _za1 = z;
    _zaFocal = at ?? Offset.zero;
    _zaSheet = _scroll + _zaFocal / _zoom;
    _zoomAnim.forward(from: 0);
  }

  void _zoomTick() {
    final t = Curves.easeInOutCubic.transform(_zoomAnim.value);
    final z = _za0 * math.pow(_za1 / _za0, t);
    if (_zoomAnim.isCompleted) {
      _pinchZoom = null;
      _commitZoom(_za1, _zaFocal, _zaSheet);
      return;
    }
    _pinchZoom = z.toDouble();
    _pinchScroll = _zaSheet - _zaFocal / _pinchZoom!;
    _paint();
  }

  void _commitZoom(double z, Offset focal, Offset sheetAt) {
    _zoom = z;
    _scroll = _clampScroll(sheetAt - focal / z, z);
    _pinchZoom = null;
    _sync();
    _readGeometry(e.geometry());
    _setOverlays(e.overlays());
    _table = e.table();
    _paint();
    _queueTiles();
    widget.onChanged();
  }

  // ---------------------------------------------------------------- tiles

  /// The pixels per point tiles are drawn at for zoom `z`: the screen's exactly.
  double _scaleFor(double z) => (z * _dpr * 1000).round() / 1000;

  Rect _visibleSheet() {
    final z = _shownZoom, s = _shownScroll;
    return Rect.fromLTWH(s.dx, s.dy, _view.width / z, _view.height / z);
  }

  /// The tiles in view (and a ring round it), nearest the middle first.
  List<(int, int)> _wanted(double scale, {bool ring = true}) {
    final span = _tilePx / scale;
    final vis = _visibleSheet();
    final k = ring ? 1 : 0;
    final x0 = (vis.left / span).floor() - k, x1 = (vis.right / span).floor() + k;
    final y0 = (vis.top / span).floor() - k, y1 = (vis.bottom / span).floor() + k;
    final cx = (x0 + x1) / 2, cy = (y0 + y1) / 2;
    final out = [for (var y = y0; y <= y1; y++) for (var x = x0; x <= x1; x++) (x, y)];
    out.sort((a, b) => ((a.$1 - cx).abs() + (a.$2 - cy).abs()).compareTo((b.$1 - cx).abs() + (b.$2 - cy).abs()));
    return out;
  }

  void _queueTiles() {
    if (_rendering) {
      _again = true;
      return;
    }
    _again = false;
    _rendering = true;
    SchedulerBinding.instance.scheduleFrameCallback((_) => _renderSome());
  }

  /// Draws tiles that are missing, out of date or blurry (after a zoom), a few per frame.
  Future<void> _renderSome() async {
    if (!mounted) return;
    if (_pinchZoom != null || _view.isEmpty || _version < 0) {
      _rendering = false;
      return;
    }
    final scale = _scaleFor(_zoom);
    if ((scale - _tileScale).abs() > 1e-4) {
      // a new zoom: the old set stays to stretch until the new one is in
      if (_tiles.isNotEmpty) {
        _old.add((_tileScale, Map.of(_tiles)));
        _tiles.clear();
        while (_old.length > 3) {
          for (final t in _old.removeAt(0).$2.values) {
            t.image?.dispose();
          }
        }
      }
      _tileScale = scale;
    }
    final t0 = DateTime.now();
    var did = false;
    final inView = _wanted(scale, ring: false).toSet();
    for (final k in _wanted(scale)) {
      final have = _tiles[k];
      if (have != null && have.version == _version && have.image != null) continue;
      // (the ring round the view only when the view itself is in)
      if (!inView.contains(k) && inView.any((v) => _tiles[v]?.version != _version)) continue;
      final img = await e.renderTile(k.$1, k.$2, scale, _tilePx);
      if (!mounted) return img?.dispose();
      if ((scale - _tileScale).abs() > 1e-4) {
        img?.dispose();
        break;
      }
      final old = _tiles[k];
      if (old != null && old.image != img) old.image?.dispose();
      _tiles[k] = _Tile(_version, scale, img);
      did = true;
      if (DateTime.now().difference(t0).inMilliseconds > 12) break;
    }
    // every tile in view is in: the stretched ones can go
    if (inView.every((k) => _tiles[k]?.version == _version) && _old.isNotEmpty) {
      for (final (_, set) in _old) {
        for (final t in set.values) {
          t.image?.dispose();
        }
      }
      _old.clear();
      did = true;
    }
    // forget far-away tiles
    if (_tiles.length > 200) {
      final keep = _wanted(scale).toSet();
      _tiles.removeWhere((k, v) {
        if (keep.contains(k)) return false;
        v.image?.dispose();
        return true;
      });
    }
    _rendering = false;
    if (did) _paint();
    final missing = _wanted(scale).any((k) => _tiles[k]?.version != _version);
    if (did && missing || _again) _queueTiles();
  }

  // ---------------------------------------------------------------- view ↔ sheet points

  /// Sheet point → this widget's point.
  Offset toLocal(double x, double y) {
    final z = _shownZoom, s = _shownScroll;
    return Offset((x - s.dx) * z, (y - s.dy) * z);
  }

  /// (table_chrome's form: page and point)
  Offset _chromeLocal(int page, double x, double y) => toLocal(x, y);

  List<double> _l(dynamic v) => [for (final n in v as List) (n as num).toDouble()];

  Rect _rect(dynamic v) {
    final r = _l(v);
    return Rect.fromPoints(toLocal(r[0], r[1]), toLocal(r[2], r[3]));
  }

  /// The selected text's handles (local): (is start, knob, the caret line's middle).
  List<(bool, Offset, Offset)> _textHandles() {
    final t = _ov['text'];
    if (t is! Map || t['ends'] == null) return [];
    final ends = t['ends'] as List;
    Offset p(dynamic v) => toLocal((v[0] as num).toDouble(), (v[1] as num).toDouble());
    final a0 = p(ends[0][0]), a1 = p(ends[0][1]), b0 = p(ends[1][0]), b1 = p(ends[1][1]);
    return [(true, a0 - const Offset(0, 7), (a0 + a1) / 2), (false, b1 + const Offset(0, 7), (b0 + b1) / 2)];
  }

  /// The selected cells' grab dots (local): top-left and bottom-right of the selection.
  List<Offset> _cellDots() {
    final t = _table;
    if (t == null || t['editing'] != null) return [];
    final sel = (t['sel'] as List?)?.map((v) => v as int).toList();
    if (sel == null) return [];
    final cw = _l(t['colWidths']), rh = _l(t['rowHeights']);
    double edge(List<double> s, int n) => s.take(n).fold(0.0, (a, b) => a + b);
    final x = (t['x'] as num).toDouble(), y = (t['y'] as num).toDouble();
    return [toLocal(x + edge(cw, sel[1]), y + edge(rh, sel[0])), toLocal(x + edge(cw, sel[3] + 1), y + edge(rh, sel[2] + 1))];
  }

  /// What a finger at `local` grabs: a text handle, an object handle or a selected object.
  /// Returns where the engine's press goes (and whether as a Shift-press), or null to scroll.
  (Offset, bool)? _grabAt(Offset local) {
    for (final (start, knob, mid) in _textHandles()) {
      if ((knob - local).distance < 26 || (mid - local).distance < 14) {
        if (start) {
          // the start handle: the other end becomes the anchor first (a click there)
          final end = _textHandles().last.$3;
          e.pointer(1, end.dx, end.dy);
          e.pointer(2, end.dx, end.dy);
        }
        return (mid, true);
      }
    }
    for (final o in (_ov['objects'] as List? ?? [])) {
      if (o['handles'] != true) continue;
      final r = _rect(o['frame']);
      for (final h in _handlePoints(r)) {
        if ((h - local).distance < 22) return (h, false);
      }
    }
    for (final o in (_ov['objects'] as List? ?? [])) {
      if (o['editing'] == true) continue;
      if (_rect(o['frame']).inflate(10).contains(local)) return (local, false);
    }
    return null;
  }

  static List<Offset> _handlePoints(Rect r) => [r.topLeft, r.topCenter, r.topRight, r.centerLeft, r.centerRight, r.bottomLeft, r.bottomCenter, r.bottomRight];

  // ---------------------------------------------------------------- the edit menu

  String _selSig() => jsonEncode([_ov['text']?['caret'], _ov['text']?['rects'], for (final o in (_ov['objects'] as List? ?? [])) o['frame'], _table?['sel'], _table?['editing']?['r']]);

  /// The edit menu over what's selected (`at`: the touch, local).
  void _showMenu(Offset at) {
    final items = e.contextMenu();
    if (items.isEmpty) return;
    // text in a text box being edited: the menu as a list (touches go through to the handles)
    if (_ov['text'] is Map) {
      final local = _menuAnchor(at);
      final box = context.findRenderObject() as RenderBox;
      final anchor = box.localToGlobal(local.topLeft) & local.size;
      showTouchMenu(context, widget.look, anchor, items, (i) {
        widget.onMenuAction(i['action'] as String? ?? '');
        if (e.status()['editingText'] == true) showKeyboard();
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

  /// What the menu points at (local): the selected cells, the selected objects or the text,
  /// else the touch; kept to the part in view.
  Rect _menuAnchor(Offset at) {
    Rect? r;
    void add(Rect x) => r = r == null ? x : r!.expandToInclude(x);
    final t = _ov['text'];
    if (t is Map) {
      for (final q in (t['rects'] as List? ?? [])) {
        for (final p in q as List) {
          final o = toLocal((p[0] as num).toDouble(), (p[1] as num).toDouble());
          add(Rect.fromCenter(center: o, width: 1, height: 1));
        }
      }
      final c = t['caret'];
      if (r == null && c is List) {
        add(Rect.fromPoints(toLocal((c[0][0] as num).toDouble(), (c[0][1] as num).toDouble()), toLocal((c[1][0] as num).toDouble(), (c[1][1] as num).toDouble())));
      }
    }
    if (r == null) {
      for (final o in (_ov['objects'] as List? ?? [])) {
        add(_rect(o['frame']));
      }
    }
    if (r == null) {
      final d = _cellDots();
      if (d.length == 2) r = Rect.fromPoints(d[0], d[1]);
    }
    final view = Offset.zero & _view;
    final shown = r?.intersect(view);
    if (shown == null || shown.width < 0 || shown.height < 0) return Rect.fromCenter(center: at, width: 1, height: 1);
    return shown;
  }

  bool _onSelection(Offset local) {
    final d = _cellDots();
    if (d.length == 2 && Rect.fromPoints(d[0], d[1]).inflate(2).contains(local)) return true;
    for (final o in (_ov['objects'] as List? ?? [])) {
      if (_rect(o['frame']).contains(local)) return true;
    }
    return false;
  }

  // ---------------------------------------------------------------- gestures

  (DateTime, Offset)? _lastTap;

  void _tapUp(TapUpDetails d) {
    _fling.stop();
    var at = d.localPosition;
    final now = DateTime.now();
    final last = _lastTap;
    final touch = d.kind != PointerDeviceKind.mouse;
    final again = last != null && now.difference(last.$1).inMilliseconds < 450 && (at - last.$2).distance < 24;
    if (touch && again) at = last.$2;
    _lastTap = (now, at);
    // the table's title: a double tap renames it
    if (again && _titleAt(at) != null) {
      _renameTitle(_titleAt(at)!);
      return;
    }
    final wasSel = _selSig();
    final wasCell = _table?['sel'];
    final onSel = _onSelection(at);
    e.pointer(1, at.dx, at.dy);
    e.pointer(2, at.dx, at.dy);
    refresh();
    if (!touch) return;
    // (iPad) a tap on the selection that left it as it was: the edit menu (again hides it)
    final same = _selSig() == wasSel && jsonEncode(_table?['sel']) == jsonEncode(wasCell);
    if (onSel && same && !_menuWasUp && _table?['editing'] == null && e.status()['editingText'] != true) _showMenu(at);
  }

  Map<String, dynamic>? _titleAt(Offset at) {
    for (final t in (_ov['titles'] as List? ?? [])) {
      final c = toLocal((t['x'] as num).toDouble(), (t['y'] as num).toDouble());
      final w = math.max(60.0, (t['name'] as String? ?? '').length * 7.5 * _shownZoom);
      if (Rect.fromLTRB(c.dx - w / 2 - 8, c.dy - 20 * _shownZoom - 6, c.dx + w / 2 + 8, c.dy + 4).contains(at)) return t as Map<String, dynamic>;
    }
    return null;
  }

  Future<void> _renameTitle(Map<String, dynamic> t) async {
    final c = TextEditingController(text: t['name'] as String? ?? '');
    final name = await showDialog<String>(
      context: context,
      builder: (ctx) => AlertDialog(
        title: const Text('Table Name'),
        content: TextField(controller: c, autofocus: true, onSubmitted: (v) => Navigator.of(ctx).pop(v)),
        actions: [TextButton(onPressed: () => Navigator.of(ctx).pop(), child: const Text('Cancel')), TextButton(onPressed: () => Navigator.of(ctx).pop(c.text), child: const Text('Rename'))],
      ),
    );
    if (name != null) {
      e.action('title:${t['id']}:$name');
      refresh();
    }
  }

  void _longPress(LongPressStartDetails d) {
    if (_multi || _fingers != 1 || _pinchZoom != null) return;
    _fling.stop();
    HapticFeedback.selectionClick();
    final v = d.localPosition;
    e.pointer(1, v.dx, v.dy, button: 1);
    refresh();
    _showMenu(v);
  }

  void _secondaryTap(TapUpDetails d) {
    _fling.stop();
    final v = d.localPosition;
    e.pointer(1, v.dx, v.dy, button: 1);
    refresh();
    _showMenu(v);
  }

  void _scaleStart(ScaleStartDetails d) {
    _fling.stop();
    _zoomAnim.stop();
    _lastFocal = d.localFocalPoint;
    _grab = false;
    _cellAnchor = null;
    if (d.pointerCount >= 2) {
      _startPinch(d.localFocalPoint);
      return;
    }
    // a selected cells' dot: the other corner stays, this one follows the finger
    final dots = _cellDots();
    for (var k = 0; k < dots.length; k++) {
      if ((dots[k] - _downAt).distance < 24) {
        final sel = (_table!['sel'] as List).map((v) => v as int).toList();
        _cellAnchor = k == 0 ? (sel[2], sel[3]) : (sel[0], sel[1]);
        _grabOffset = dots[k] - _downAt;
        return;
      }
    }
    // a mouse drags as on the desktop (a selection across cells, an object moved)
    if (_downKind == PointerDeviceKind.mouse) {
      _grab = true;
      _shiftDrag = false;
      _grabOffset = Offset.zero;
      e.pointer(1, _downAt.dx, _downAt.dy);
      e.pointer(0, d.localFocalPoint.dx, d.localFocalPoint.dy);
      _afterDrag();
      return;
    }
    final g = _grabAt(_downAt);
    if (g != null) {
      _grab = true;
      _shiftDrag = g.$2;
      _grabOffset = g.$1 - d.localFocalPoint;
      e.pointer(1, g.$1.dx, g.$1.dy, shift: _shiftDrag);
      _afterDrag();
    }
  }

  void _afterDrag() {
    _setOverlays(e.overlays());
    _table = e.table();
    _paint();
  }

  void _startPinch(Offset focal) {
    _zoomAtStart = _zoom;
    _pinchZoom = _zoom;
    _pinchScroll = _scroll;
    _sheetAtFocal = _scroll + focal / _zoom;
  }

  void _scaleUpdate(ScaleUpdateDetails d) {
    _focal = d.localFocalPoint;
    if (d.pointerCount >= 2) {
      if (_grab) {
        final v = _lastFocal + _grabOffset;
        e.pointer(2, v.dx, v.dy, shift: _shiftDrag);
        _grab = false;
      }
      _cellAnchor = null;
      if (_pinchZoom == null) _startPinch(d.localFocalPoint);
      final z = (_zoomAtStart * d.scale).clamp(0.25, 4.0);
      _pinchZoom = z;
      _pinchScroll = _sheetAtFocal - d.localFocalPoint / z;
      _paint();
      return;
    }
    if (_pinchZoom != null) return;
    final anchor = _cellAnchor;
    if (anchor != null) {
      final p = d.localFocalPoint + _grabOffset;
      final h = e.hit(p.dx, p.dy);
      if (h != null && h['table'] == _table?['id']) {
        e.action('tbl:select:${anchor.$1}:${anchor.$2}:${h['r']}:${h['c']}');
        _afterDrag();
        widget.onChanged();
      }
      _edgeScroll(d.localFocalPoint);
      return;
    }
    if (_grab) {
      _lastFocal = d.localFocalPoint;
      final v = d.localFocalPoint + _grabOffset;
      e.pointer(0, v.dx, v.dy, shift: _shiftDrag);
      _afterDrag();
      _edgeScroll(d.localFocalPoint);
      return;
    }
    _scroll = _clampScroll(_scroll - d.focalPointDelta / _zoom, _zoom);
    _sync();
    _paint();
    _queueTiles();
  }

  /// A drag held near the view's edge scrolls that way (the sheet grows with it).
  void _edgeScroll(Offset p) {
    double out(double v, double lo, double hi) => v < lo ? v - lo : (v > hi ? v - hi : 0);
    final dx = out(p.dx, 24, _view.width - 24), dy = out(p.dy, 24, _view.height - 24);
    if (dx == 0 && dy == 0) return;
    double speed(double d) => d.sign * math.min(4 + d.abs() * 0.5, 30);
    _scroll += Offset(dx == 0 ? 0 : speed(dx), dy == 0 ? 0 : speed(dy)) / _zoom;
    _sync();
    _paint();
    _queueTiles();
  }

  Offset _focal = Offset.zero;

  void _scaleEnd(ScaleEndDetails d) {
    if (_pinchZoom != null && !_zoomAnim.isAnimating) {
      final z = _pinchZoom!;
      final sheetAt = _pinchScroll + _focal / z;
      _commitZoom(z, _focal, sheetAt);
      return;
    }
    if (_cellAnchor != null) {
      _cellAnchor = null;
      refresh();
      return;
    }
    if (_grab) {
      final v = _lastFocal + _grabOffset;
      e.pointer(2, v.dx, v.dy, shift: _shiftDrag);
      _grab = false;
      refresh();
      if (_shiftDrag && _ov['text'] is Map && ((_ov['text'] as Map)['rects'] as List? ?? const []).isNotEmpty) _showMenu(_lastFocal);
      return;
    }
    // a fling: glides on and slows down (the platform's scrolling physics)
    final vel = -d.velocity.pixelsPerSecond / _zoom;
    if (vel.distance < 50) return;
    _flingFrom = _scroll;
    _simX = ClampingScrollSimulation(position: _scroll.dx, velocity: vel.dx);
    _simY = ClampingScrollSimulation(position: _scroll.dy, velocity: vel.dy);
    _fling.value = 0;
    _fling.animateWith(_TimeSimulation());
  }

  void _flingTick() {
    final t = _fling.value;
    final x = _simX?.x(t) ?? _flingFrom.dx, y = _simY?.x(t) ?? _flingFrom.dy;
    final want = Offset(x, y);
    _scroll = _clampScroll(want, _zoom);
    if (((_simX?.isDone(t) ?? true) && (_simY?.isDone(t) ?? true)) || (_scroll - want).distance > 0.5) _fling.stop();
    _sync();
    _paint();
    _queueTiles();
  }

  void _lifted() {
    _fingers = math.max(0, _fingers - 1);
    if (_fingers == 0) _multi = false;
  }

  /// The mouse wheel and touchpad: scroll; with Ctrl, zoom about the pointer.
  void _signal(PointerSignalEvent ev) {
    if (ev is PointerScrollEvent) {
      if (HardwareKeyboard.instance.isControlPressed) {
        final z = (_zoom * math.pow(1.0015, -ev.scrollDelta.dy)).clamp(0.25, 4.0).toDouble();
        final at = _scroll + ev.localPosition / _zoom;
        _commitZoom(z, ev.localPosition, at);
        return;
      }
      _scroll = _clampScroll(_scroll + ev.scrollDelta / _zoom, _zoom);
      _sync();
      _paint();
      _queueTiles();
    } else if (ev is PointerScaleEvent) {
      final z = (_zoom * ev.scale).clamp(0.25, 4.0);
      _commitZoom(z, ev.localPosition, _scroll + ev.localPosition / _zoom);
    }
  }

  // ---------------------------------------------------------------- the cell editor in view

  /// A cell just opened for editing: brought into view (above the keyboard and the formula bar).
  void _followEditor() {
    final ed = _table?['editing'];
    final sig = ed == null ? '' : '${_table!['id']}:${ed['r']}:${ed['c']}';
    if (sig == _editingCell) return;
    _editingCell = sig;
    if (ed == null) return;
    WidgetsBinding.instance.addPostFrameCallback((_) => _revealEditor());
    // (and again once the keyboard is up and the view has shrunk)
    Timer(const Duration(milliseconds: 450), () {
      if (mounted) _revealEditor();
    });
  }

  void _revealEditor() {
    final t = _table, ed = t?['editing'];
    if (t == null || ed == null || _view.isEmpty) return;
    final cw = _l(t['colWidths']), rh = _l(t['rowHeights']);
    double edge(List<double> s, int n) => s.take(n).fold(0.0, (a, b) => a + b);
    final x = (t['x'] as num).toDouble(), y = (t['y'] as num).toDouble();
    final r = ed['r'] as int, c = ed['c'] as int;
    final cell = Rect.fromLTRB(x + edge(cw, c), y + edge(rh, r), x + edge(cw, (ed['c1'] as int? ?? c) + 1), y + edge(rh, (ed['r1'] as int? ?? r) + 1));
    // (the room over the formula bar: it covers the view's foot while a cell is edited)
    final full = _visibleSheet();
    final vis = Rect.fromLTWH(full.left, full.top, full.width, math.max(full.height * 0.3, full.height - 110 / _zoom));
    final m = 24 / _zoom;
    var s = _scroll;
    if (cell.bottom + m > vis.bottom) s = Offset(s.dx, cell.bottom + m - vis.height);
    if (cell.top - m < s.dy) s = Offset(s.dx, cell.top - m);
    if (cell.right + m > vis.right) s = Offset(math.min(cell.left - m, cell.right + m - vis.width), s.dy);
    if (cell.left - m < s.dx) s = Offset(cell.left - m, s.dy);
    if ((s - _scroll).distance < 0.5) return;
    _scroll = s;
    _sync();
    _paint();
    _queueTiles();
  }

  // ---------------------------------------------------------------- the keyboard

  static const _sentinel = '​​';
  String _last = _sentinel;

  void _resetInput() {
    _last = _sentinel;
    _input.value = const TextEditingValue(text: _sentinel, selection: TextSelection.collapsed(offset: 2));
  }

  /// What the on-screen keyboard typed into a text box (text, deletions, new lines).
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

  /// Ctrl shortcuts as the desktop's menus have them (Ctrl+V pastes the system clipboard).
  KeyEventResult _ctrlKey(KeyEvent ev, bool shift, bool alt) {
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

  /// A hardware keyboard in a text box: moving the caret, Tab, Escape and the menus' Ctrl
  /// shortcuts go to the engine; typing goes through the hidden text field.
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
    if (ctrl) return _ctrlKey(ev, shift, alt);
    return KeyEventResult.ignored;
  }

  /// A hardware keyboard on the sheet (cells or an object selected): moves, Return, Delete,
  /// shortcuts, and typing over the cell (its editor opens with what was typed).
  KeyEventResult _sheetKey(FocusNode node, KeyEvent ev) {
    // (keys a text field below didn't take stay with it)
    if (ev is KeyUpEvent || FocusManager.instance.primaryFocus != _keys) return KeyEventResult.ignored;
    final hk = HardwareKeyboard.instance;
    final shift = hk.isShiftPressed, ctrl = hk.isControlPressed || hk.isMetaPressed, alt = hk.isAltPressed;
    if (ctrl) return _ctrlKey(ev, shift, alt);
    final name = _keyNames[ev.logicalKey] ??
        switch (ev.logicalKey) {
          LogicalKeyboardKey.enter || LogicalKeyboardKey.numpadEnter => 'Enter',
          LogicalKeyboardKey.backspace => 'Backspace',
          LogicalKeyboardKey.f2 => 'F2',
          _ => null,
        };
    if (name != null) {
      e.key(name, shift: shift, alt: alt);
      refresh();
      return KeyEventResult.handled;
    }
    final ch = ev.character;
    if (ch != null && ch.isNotEmpty && ch.codeUnitAt(0) >= 32) {
      e.text(ch);
      refresh();
      return KeyEventResult.handled;
    }
    return KeyEventResult.ignored;
  }

  /// Text as a keyboard would type it (automated checks): into the cell being edited, a text
  /// box, or over the selected cell.
  void typeForTest(String t) {
    if (e.status()['editingText'] == true) return _typed(TextEditingValue(text: _last + t));
    e.text(t);
    refresh();
  }

  void showKeyboard() {
    _resetInput();
    _focus.requestFocus();
    SystemChannels.textInput.invokeMethod('TextInput.show');
  }

  void hideKeyboard() {
    _focus.unfocus();
    FocusManager.instance.primaryFocus?.unfocus();
  }

  // ---------------------------------------------------------------- build

  @override
  Widget build(BuildContext context) {
    _dpr = MediaQuery.of(context).devicePixelRatio;
    return LayoutBuilder(builder: (context, box) {
      final view = Size(box.maxWidth, box.maxHeight);
      if (view != _view) {
        _view = view;
        WidgetsBinding.instance.addPostFrameCallback((_) {
          if (!mounted) return;
          e.view(view.width, view.height);
          _readGeometry(e.geometry());
          _sync();
          refresh();
        });
      }
      return TapRegion(
        onTapOutside: (_) => _hideMenu(),
        child: Focus(
          focusNode: _keys,
          child: ClipRect(
            child: Stack(children: [
              Positioned.fill(
                child: Listener(
                  onPointerDown: (ev) {
                    _downAt = ev.localPosition;
                    _downKind = ev.kind;
                    _menuWasUp = _menu != null;
                    _hideMenu();
                    _fingers++;
                    if (_fingers > 1) _multi = true;
                  },
                  onPointerUp: (_) => _lifted(),
                  onPointerCancel: (_) => _lifted(),
                  onPointerSignal: _signal,
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
                          ..onUpdate = _scaleUpdate
                          ..onEnd = _scaleEnd,
                      ),
                    },
                    child: CustomPaint(painter: _SheetPainter(this), child: const SizedBox.expand()),
                  ),
                ),
              ),
              // the selected table's chrome (tabs, corner, add buttons, autofill, cell editor)
              if (_table != null && _pinchZoom == null)
                Positioned.fill(
                  child: ValueListenableBuilder<int>(
                    valueListenable: _repaint,
                    builder: (ctx, _, _) => _table == null ? const SizedBox.shrink() : TableChrome(engine: e, t: _table!, toLocal: _chromeLocal, toView: (p) => p, zoom: _zoom, onChanged: refresh, onMenu: widget.onMenu),
                  ),
                ),
              // a message: a pill near the bottom
              if (widget.message != null)
                Positioned(
                  left: 20,
                  right: 20,
                  bottom: 24,
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
              // the hidden field the on-screen keyboard types into (a text box being edited)
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
              // the edit menu over the selection
              if (_menu != null && _pinchZoom == null)
                Positioned.fill(
                  child: TextFieldTapRegion(
                    child: ValueListenableBuilder<int>(
                      valueListenable: _repaint,
                      builder: (ctx, _, _) => _menu == null
                          ? const SizedBox.shrink()
                          : EditMenu(
                              items: _menu!,
                              anchor: _menuAnchor(_menuPoint),
                              bounds: Offset.zero & _view,
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
        ),
      );
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

Color _rgb(int v, [double a = 1]) => Color.fromARGB((a * 255).round(), v >> 16 & 255, v >> 8 & 255, v & 255);

/// The sheet and what's over it, as canvas.rs draws them.
class _SheetPainter extends CustomPainter {
  _SheetPainter(this.s) : super(repaint: Listenable.merge([s._repaint, s._blink]));
  final SheetViewState s;

  List<double> _l(dynamic v) => [for (final n in v as List) (n as num).toDouble()];

  @override
  void paint(Canvas canvas, Size size) {
    final look = s.widget.look;
    canvas.drawRect(Offset.zero & size, Paint()..color = Colors.white);
    final z = s._shownZoom, o = s._shownScroll, d = s._dpr;
    final px = s._tilePx.toDouble();
    double snap(double v) => (v * d).roundToDouble() / d;
    void drawSet(double scale, Map<(int, int), _Tile> set, bool exact) {
      final span = px / scale;
      final vis = s._visibleSheet();
      set.forEach((k, t) {
        final img = t.image;
        if (img == null) return;
        final tr = Rect.fromLTWH(k.$1 * span, k.$2 * span, span, span);
        if (!tr.overlaps(vis)) return;
        final dst = Rect.fromLTRB(snap((tr.left - o.dx) * z), snap((tr.top - o.dy) * z), snap((tr.right - o.dx) * z), snap((tr.bottom - o.dy) * z));
        canvas.drawImageRect(img, Rect.fromLTWH(0, 0, px, px), dst, Paint()..filterQuality = exact ? FilterQuality.none : FilterQuality.medium);
      });
    }

    for (final (scale, set) in s._old) {
      drawSet(scale, set, false);
    }
    final exact = s._pinchZoom == null && (s._tileScale - s._scaleFor(s._zoom)).abs() < 1e-3;
    drawSet(s._tileScale == 0 ? 1 : s._tileScale, s._tiles, exact);
    if (s._pinchZoom == null) _frozen(canvas, size, z, o);
    _overlay(canvas, size, z, look);
  }

  /// canvas.rs: frozen header rows stay at the view's top while their table scrolls under them,
  /// frozen header columns at its left (for the rows in view, in blocks of 32).
  void _frozen(Canvas canvas, Size size, double z, Offset o) {
    final line = Paint()
      ..strokeWidth = 1
      ..color = const Color.fromARGB(40, 0, 0, 0);
    for (final f in (s._ov['frozen'] as List? ?? [])) {
      final fr = _l(f['frame']);
      final (fx, fy, fw, fh) = (fr[0], fr[1], fr[2], fr[3]);
      final hh = (f['hh'] as num).toDouble(), hw = (f['hw'] as num).toDouble();
      final id = f['id'] as int;
      final top = (fy - o.dy) * z, left = (fx - o.dx) * z;
      if (f['rows'] == true && top < 0 && (fy + fh - hh - o.dy) * z > hh * z) {
        final img = s._header(id, false, 0, 0);
        if (img != null) {
          final dst = Rect.fromLTWH(left - z, -z, img.width / s._scaleFor(s._zoom) * z, img.height / s._scaleFor(s._zoom) * z);
          canvas.drawImageRect(img, Rect.fromLTWH(0, 0, img.width.toDouble(), img.height.toDouble()), dst, Paint()..filterQuality = FilterQuality.medium);
          canvas.drawLine(Offset(left, hh * z + 1), Offset(left + fw * z, hh * z + 1), line);
        }
      }
      if (f['cols'] == true && left < 0 && (fx + fw - o.dx) * z > hw * z) {
        final rh = _l(f['rowHeights']);
        final vTop = o.dy - fy, vBot = o.dy + size.height / z - fy;
        var acc = 0.0, first = 0, last = rh.length - 1;
        for (var i = 0; i < rh.length; i++) {
          if (acc + rh[i] < vTop) first = i + 1;
          if (acc <= vBot) last = i;
          acc += rh[i];
        }
        final n = rh.length;
        if (n == 0) continue;
        final r0 = math.min(first ~/ 32 * 32, n - 1), r1 = math.max(r0, math.min((last ~/ 32 + 1) * 32, n - 1));
        final img = s._header(id, true, r0, r1);
        if (img != null) {
          final y0 = rh.take(r0).fold(0.0, (a, b) => a + b);
          final dst = Rect.fromLTWH(-z, (fy + y0 - o.dy) * z - z, img.width / s._scaleFor(s._zoom) * z, img.height / s._scaleFor(s._zoom) * z);
          canvas.drawImageRect(img, Rect.fromLTWH(0, 0, img.width.toDouble(), img.height.toDouble()), dst, Paint()..filterQuality = FilterQuality.medium);
          canvas.drawLine(Offset(hw * z + 1, math.max(0, top)), Offset(hw * z + 1, (fy + fh - o.dy) * z), line);
        }
      }
    }
  }

  void _overlay(Canvas canvas, Size size, double z, Look look) {
    final ov = s._ov;
    final accent = look.accent;
    Offset at(double x, double y) => s.toLocal(x, y);
    Rect rect(List<double> v) => Rect.fromPoints(at(v[0], v[1]), at(v[2], v[3]));
    // table names above their tables (canvas.rs: 14 points at the sheet's zoom, centred)
    for (final t in (ov['titles'] as List? ?? [])) {
      final name = t['name'] as String? ?? '';
      if (name.isEmpty) continue;
      final p = at((t['x'] as num).toDouble(), (t['y'] as num).toDouble());
      final tp = TextPainter(text: TextSpan(text: name, style: TextStyle(fontFamily: 'Carlito', fontFamilyFallback: const ['DejaVuSans'], fontSize: 14 * z, color: Colors.black)), textDirection: TextDirection.ltr, maxLines: 1)..layout();
      tp.paint(canvas, Offset(p.dx - tp.width / 2, p.dy - tp.height));
    }
    // commented cells: a triangle in the cell's top-right corner
    for (final c in (ov['comments'] as List? ?? [])) {
      final v = _l(c);
      final r = rect(v);
      final k = math.min(7.0, r.height * 0.4);
      canvas.drawPath(Path()..addPolygon([r.topRight, r.topRight + Offset(0, k), r.topRight - Offset(k, 0)], true), Paint()..color = _rgb(v[4].toInt()));
    }
    // a formula's references, each in its colour
    for (final c in (ov['refs'] as List? ?? [])) {
      final v = _l(c);
      final r = rect(v);
      final col = _rgb(v[4].toInt());
      canvas.drawRect(r, Paint()..color = col.withValues(alpha: 0.15));
      canvas.drawRect(r, Paint()
        ..style = PaintingStyle.stroke
        ..strokeWidth = 2
        ..color = col);
    }
    // the text box being edited: its frame, the selection, the caret, the handles
    final t = ov['text'];
    if (t is Map) {
      Offset p(dynamic q) => at((q[0] as num).toDouble(), (q[1] as num).toDouble());
      final fr = t['frame'] as List;
      final frame = Path()..addPolygon([for (final q in fr) p(q)], true);
      canvas.drawPath(frame, Paint()
        ..style = PaintingStyle.stroke
        ..strokeWidth = 1
        ..color = accent.withValues(alpha: 0.6));
      for (final q in (t['rects'] as List? ?? [])) {
        canvas.drawPath(Path()..addPolygon([for (final pt in q as List) p(pt)], true), Paint()..color = const Color.fromARGB(90, 0x3a, 0x82, 0xf7));
      }
      final c = t['caret'];
      if (c is List && s._blink.value * 1.06 < 0.62) {
        canvas.drawLine(p(c[0]), p(c[1]), Paint()
          ..strokeWidth = 2
          ..color = accent);
      }
      for (final (start, knob, _) in s._textHandles()) {
        final ends = t['ends'] as List;
        final pair = start ? ends[0] as List : ends[1] as List;
        canvas.drawLine(p(pair[0]), p(pair[1]), Paint()
          ..strokeWidth = 2
          ..color = accent);
        canvas.drawCircle(knob, 5.5, Paint()..color = accent);
      }
    }
    // selected objects: frame and handles (eight small white squares)
    for (final ob in (ov['objects'] as List? ?? [])) {
      final r = rect(_l(ob['frame']));
      canvas.drawRect(r, Paint()
        ..style = PaintingStyle.stroke
        ..strokeWidth = ob['handles'] == true ? 1 : 1.5
        ..color = accent);
      if (ob['handles'] != true) continue;
      for (final h in SheetViewState._handlePoints(r)) {
        final hr = Rect.fromCenter(center: h, width: 8, height: 8);
        if (ob['locked'] == true) {
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
    // a move or resize going on: its frame, the guides it snapped to, its size
    final live = ov['live'];
    if (live is Map) {
      final r = rect(_l(live['frame']));
      canvas.drawRect(r, Paint()
        ..style = PaintingStyle.stroke
        ..strokeWidth = 1
        ..color = const Color(0xFF808080));
      for (final g in (live['guides'] as List? ?? [])) {
        final v = _l(g);
        canvas.drawLine(at(v[0], v[1]), at(v[2], v[3]), Paint()
          ..strokeWidth = 1
          ..color = const Color(0xFFE8CB44));
      }
      final tip = live['tip'] as String? ?? '';
      if (tip.isNotEmpty) {
        final tp = TextPainter(text: TextSpan(text: tip, style: Look.font(12, Colors.white)), textDirection: TextDirection.ltr)..layout();
        final a = r.bottomRight + const Offset(12, 12);
        canvas.drawRRect(RRect.fromRectAndRadius(Rect.fromLTWH(a.dx, a.dy, tp.width + 14, tp.height + 8), const Radius.circular(6)), Paint()..color = const Color.fromARGB(200, 0, 0, 0));
        tp.paint(canvas, a + const Offset(7, 4));
      }
    }
  }

  @override
  bool shouldRepaint(_SheetPainter o) => false;
}
