// Keydeck's document library (Rust: keynote-linux/crates/kn-app/src/ffi.rs, libkn_app.so): the
// desktop's presentations, editing logic and slide drawing, with no interface of its own.
// Everything on screen is Flutter's; this tells the engine what happened (a tap or a drag at a
// slide point, text typed, a command) and asks it for state as data and for the slides' pixels.
import 'dart:convert';
import 'dart:ffi';
import 'dart:isolate';
import 'dart:typed_data';
import 'dart:ui' as ui;

import 'package:ffi/ffi.dart';

typedef _S = Pointer<Utf8> Function();
typedef _VS = Void Function(Pointer<Utf8>);
typedef _DS = void Function(Pointer<Utf8>);

/// The Content Hub's catalogue, searches, thumbnails and downloads (ffi.rs kd_hub): they
/// hold no document, so the slow ones run on a worker isolate with its own handle.
dynamic hubSync(String kind, String arg) {
  final lib = DynamicLibrary.open('libkn_app.so');
  final f = lib.lookupFunction<Pointer<Utf8> Function(Pointer<Utf8>, Pointer<Utf8>), Pointer<Utf8> Function(Pointer<Utf8>, Pointer<Utf8>)>('kd_hub');
  final free = lib.lookupFunction<Void Function(Pointer<Utf8>), void Function(Pointer<Utf8>)>('kd_free_str');
  final k = kind.toNativeUtf8(), a = arg.toNativeUtf8();
  try {
    final r = f(k, a);
    if (r == nullptr) return null;
    final s = r.toDartString();
    free(r);
    return jsonDecode(s);
  } finally {
    malloc.free(k);
    malloc.free(a);
  }
}

Future<dynamic> hubAsync(String kind, String arg) => Isolate.run(() => hubSync(kind, arg));

class Engine {
  Engine() : _lib = DynamicLibrary.open('libkn_app.so');
  final DynamicLibrary _lib;

  late final _home = _lib.lookupFunction<_VS, _DS>('kd_home');
  late final _init = _lib.lookupFunction<Void Function(), void Function()>('kd_init');
  late final _free = _lib.lookupFunction<Void Function(Pointer<Utf8>), void Function(Pointer<Utf8>)>('kd_free_str');
  late final _error = _lib.lookupFunction<_S, Pointer<Utf8> Function()>('kd_error');
  late final _open = _lib.lookupFunction<Int32 Function(Pointer<Utf8>), int Function(Pointer<Utf8>)>('kd_open');
  late final _new = _lib.lookupFunction<Int32 Function(Uint32), int Function(int)>('kd_new');
  late final _themes = _lib.lookupFunction<_S, Pointer<Utf8> Function()>('kd_themes');
  late final _save = _lib.lookupFunction<Int32 Function(Pointer<Utf8>), int Function(Pointer<Utf8>)>('kd_save');
  late final _export = _lib.lookupFunction<Int32 Function(Pointer<Utf8>), int Function(Pointer<Utf8>)>('kd_export');
  late final _geometry = _lib.lookupFunction<_S, Pointer<Utf8> Function()>('kd_geometry');
  late final _navigator = _lib.lookupFunction<_S, Pointer<Utf8> Function()>('kd_navigator');
  late final _render = _lib.lookupFunction<Int32 Function(Uint32, Float, Float, Float, Uint32, Pointer<Uint8>, Uint32, Uint32), int Function(int, double, double, double, int, Pointer<Uint8>, int, int)>('kd_render');
  late final _renderLayout = _lib.lookupFunction<Int32 Function(Uint32, Pointer<Uint8>, Uint32, Uint32), int Function(int, Pointer<Uint8>, int, int)>('kd_render_layout');
  late final _hit = _lib.lookupFunction<Pointer<Utf8> Function(Float, Float, Float), Pointer<Utf8> Function(double, double, double)>('kd_hit');
  late final _tap = _lib.lookupFunction<Void Function(Float, Float, Float, Uint32, Bool), void Function(double, double, double, int, bool)>('kd_tap');
  late final _hold = _lib.lookupFunction<Void Function(Float, Float, Float), void Function(double, double, double)>('kd_hold');
  late final _drag = _lib.lookupFunction<Void Function(Uint32, Float, Float, Float, Bool, Bool), void Function(int, double, double, double, bool, bool)>('kd_drag');
  late final _textPoint = _lib.lookupFunction<Void Function(Float, Float, Uint32), void Function(double, double, int)>('kd_text_point');
  late final _overlays = _lib.lookupFunction<_S, Pointer<Utf8> Function()>('kd_overlays');
  late final _contextMenu = _lib.lookupFunction<Pointer<Utf8> Function(Int32), Pointer<Utf8> Function(int)>('kd_context_menu');
  late final _popover = _lib.lookupFunction<Pointer<Utf8> Function(Pointer<Utf8>), Pointer<Utf8> Function(Pointer<Utf8>)>('kd_popover');
  late final _action = _lib.lookupFunction<_VS, _DS>('kd_action');
  late final _text = _lib.lookupFunction<_VS, _DS>('kd_text');
  late final _key = _lib.lookupFunction<Void Function(Pointer<Utf8>, Bool, Bool, Bool), void Function(Pointer<Utf8>, bool, bool, bool)>('kd_key');
  late final _paste = _lib.lookupFunction<_VS, _DS>('kd_paste');
  late final _copied = _lib.lookupFunction<_S, Pointer<Utf8> Function()>('kd_copied');
  late final _menus = _lib.lookupFunction<_S, Pointer<Utf8> Function()>('kd_menus');
  late final _run = _lib.lookupFunction<_VS, _DS>('kd_run');
  late final _toolbar = _lib.lookupFunction<_VS, _DS>('kd_toolbar');
  late final _status = _lib.lookupFunction<_S, Pointer<Utf8> Function()>('kd_status');
  late final _chrome = _lib.lookupFunction<_S, Pointer<Utf8> Function()>('kd_chrome');
  late final _inspector = _lib.lookupFunction<_S, Pointer<Utf8> Function()>('kd_inspector');
  late final _inspectorSet = _lib.lookupFunction<Void Function(Pointer<Utf8>, Pointer<Utf8>), void Function(Pointer<Utf8>, Pointer<Utf8>)>('kd_inspector_set');
  late final _thumbnail = _lib.lookupFunction<Pointer<Uint8> Function(Uint64, Uint32, Pointer<Uint32>), Pointer<Uint8> Function(int, int, Pointer<Uint32>)>('kd_thumbnail');
  late final _freeBytes = _lib.lookupFunction<Void Function(Pointer<Uint8>, Uint32), void Function(Pointer<Uint8>, int)>('kd_free_bytes');
  late final _show = _lib.lookupFunction<_S, Pointer<Utf8> Function()>('kd_show');
  late final _showGo = _lib.lookupFunction<_VS, _DS>('kd_show_go');
  late final _motion = _lib.lookupFunction<Int32 Function(Uint32, Float, Pointer<Uint8>, Uint32, Uint32), int Function(int, double, Pointer<Uint8>, int, int)>('kd_motion');
  late final _tick = _lib.lookupFunction<Int32 Function(), int Function()>('kd_tick');
  late final _motionPreset = _lib.lookupFunction<Int32 Function(Uint32, Pointer<Uint8>, Uint32, Uint32), int Function(int, Pointer<Uint8>, int, int)>('kd_motion_preset');
  late final _notes = _lib.lookupFunction<_S, Pointer<Utf8> Function()>('kd_notes');
  late final _outline = _lib.lookupFunction<_S, Pointer<Utf8> Function()>('kd_outline');
  late final _urls = _lib.lookupFunction<_S, Pointer<Utf8> Function()>('kd_urls');
  late final _playStart = _lib.lookupFunction<Void Function(Uint32, Bool), void Function(int, bool)>('kd_play_start');
  late final _playFrame = _lib.lookupFunction<Pointer<Uint8> Function(Float, Float, Float, Pointer<Utf8>, Pointer<Uint32>), Pointer<Uint8> Function(double, double, double, Pointer<Utf8>, Pointer<Uint32>)>('kd_play_frame');
  late final _playStatus = _lib.lookupFunction<_S, Pointer<Utf8> Function()>('kd_play_status');
  late final _playStop = _lib.lookupFunction<Void Function(), void Function()>('kd_play_stop');
  late final _showFrame = _lib.lookupFunction<Int32 Function(Uint32, Uint32, Pointer<Uint8>, Uint32, Uint32), int Function(int, int, Pointer<Uint8>, int, int)>('kd_show_frame');

  String _take(Pointer<Utf8> p) {
    if (p == nullptr) return '';
    final s = p.toDartString();
    _free(p);
    return s;
  }

  T _str<T>(String s, T Function(Pointer<Utf8>) f) {
    final p = s.toNativeUtf8();
    try {
      return f(p);
    } finally {
      calloc.free(p);
    }
  }

  dynamic _json(Pointer<Utf8> p) {
    final s = _take(p);
    return s.isEmpty ? null : jsonDecode(s);
  }

  Map<String, dynamic> _map(Pointer<Utf8> p) => (_json(p) as Map<String, dynamic>?) ?? {};

  void start(String home) {
    _str(home, _home);
    _init();
  }

  /// Pixels the engine drew into a buffer of `w` × `h`, as an image (null when it couldn't).
  Future<ui.Image?> _image(int w, int h, int Function(Pointer<Uint8> buf) draw) async {
    if (w <= 0 || h <= 0) return null;
    final buf = calloc<Uint8>(w * h * 4);
    Uint8List bytes;
    try {
      if (draw(buf) != 0) return null;
      bytes = Uint8List.fromList(buf.asTypedList(w * h * 4));
    } finally {
      calloc.free(buf);
    }
    final buffer = await ui.ImmutableBuffer.fromUint8List(bytes);
    final desc = ui.ImageDescriptor.raw(buffer, width: w, height: h, pixelFormat: ui.PixelFormat.rgba8888);
    return (await (await desc.instantiateCodec()).getNextFrame()).image;
  }

  // ---- files ----
  String get error => _take(_error());
  String? open(String path) => _str(path, _open) == 0 ? null : error;
  String? create(int theme) => _new(theme) == 0 ? null : error;
  String? save(String path) => _str(path, _save) == 0 ? null : error;
  String? export(String path) => _str(path, _export) == 0 ? null : error;

  /// The theme chooser: {themes: [{name, preview}], recent: [{path, name}], default}.
  Map<String, dynamic> themes() => _map(_themes());

  // ---- the slides ----

  /// {size: [w, h], count, current, selected, key}.
  Map<String, dynamic> geometry() => _map(_geometry());

  /// {slides: [{key, skipped, depth, collapsed, hidden, number, index}], current, selected}.
  Map<String, dynamic> navigator() => _map(_navigator());

  /// Part of slide `index`: slide points from (`x`, `y`) at `scale` pixels a point, `w` × `h`
  /// pixels. `mode` 0 all, 1 without what's being dragged, 2 only that.
  Future<ui.Image?> render(int index, double scale, double x, double y, int w, int h, {int mode = 0}) => _image(w, h, (b) => _render(index, scale, x, y, mode, b, w, h));

  /// A layout's picture (the Add Slide popover's `index`), `w` × `h` pixels.
  /// Slide `index`'s dynamic background at `t` seconds, `w` × `h` pixels (null: it has none).
  Future<ui.Image?> motion(int index, double t, int w, int h) => _image(w, h, (b) => _motion(index, t, b, w, h));

  /// The theme's dynamic background preset `index`, `w` × `h` pixels.
  Future<ui.Image?> motionPreset(int index, int w, int h) => _image(w, h, (b) => _motionPreset(index, b, w, h));

  Future<ui.Image?> renderLayout(int index, int w, int h) => _image(w, h, (b) => _renderLayout(index, b, w, h));

  /// What's under a slide point: {what: handle|lineEnd|editing|selected|object|none, id, handle}.
  Map<String, dynamic> hit(double x, double y, double tol) => _map(_hit(x, y, tol));
  void tap(double x, double y, double tol, {int count = 1, bool shift = false}) => _tap(x, y, tol, count, shift);
  void hold(double x, double y, double tol) => _hold(x, y, tol);

  /// A drag on the slide: kind 1 starts, 0 moves, 2 ends, 3 cancels.
  void drag(int kind, double x, double y, double tol, {bool shift = false, bool ctrl = false}) => _drag(kind, x, y, tol, shift, ctrl);

  /// The caret (end 0) or the text selection's moving end (1) / other end (2) to a slide point.
  void textPoint(double x, double y, int end) => _textPoint(x, y, end);

  /// {selection: [{id, corners, handles, line, locked}], editing: {frame, caret, rects, ends},
  /// rubber, dragging, moving, size}.
  Map<String, dynamic> overlays() => _map(_overlays());

  /// The slide's right-click menu (`slide` ≥ 0: a navigator slide's).
  List<dynamic> contextMenu([int slide = -1]) => (_json(_contextMenu(slide)) as List?) ?? [];

  /// A toolbar popover's contents (View, Media: menu items; AddSlide: the layouts).
  dynamic popover(String id) => _str(id, (p) => _json(_popover(p)));
  void action(String a) => _str(a, _action);

  // ---- typing ----
  void text(String s) => _str(s, _text);
  void key(String name, {bool shift = false, bool ctrl = false, bool alt = false}) => _str(name, (p) => _key(p, shift, ctrl, alt));
  void paste(String s) => _str(s, _paste);
  String? copied() {
    final p = _copied();
    return p == nullptr ? null : _take(p);
  }

  // ---- commands and state ----
  List<dynamic> menus() => (_json(_menus()) as List?) ?? [];
  void run(String command) => _str(command, _run);
  void toolbar(String id) => _str(id, _toolbar);
  Map<String, dynamic> status() => _map(_status());
  Map<String, dynamic> chrome() => _map(_chrome());

  final _thumbs = <int, ui.Image?>{};

  /// A picture of the document's (its data id), at most `max` pixels across (kept).
  Future<ui.Image?> thumbnail(int data, {int max = 192}) async {
    if (_thumbs.containsKey(data)) return _thumbs[data];
    final len = calloc<Uint32>();
    Uint8List? png;
    try {
      final p = _thumbnail(data, max, len);
      if (p != nullptr) {
        png = Uint8List.fromList(p.asTypedList(len.value));
        _freeBytes(p, len.value);
      }
    } finally {
      calloc.free(len);
    }
    if (png == null) return _thumbs[data] = null;
    final codec = await ui.instantiateImageCodec(png);
    return _thumbs[data] = (await codec.getNextFrame()).image;
  }

  /// The inspector: which pane shows and its controls' values.
  Map<String, dynamic> inspector() => _map(_inspector());

  /// Changes an inspector control (`key` as inspector() names it; "dlg.<window>.<button>" for
  /// the windows').
  void inspectorSet(String key, Object? value) => _str(key, (k) => _str(jsonEncode(value), (v) => _inspectorSet(k, v)));

  // ---- the slideshow ----

  /// {slide, step, steps, count, transition, builds, notes, end, size}, or null when not playing.
  Map<String, dynamic>? show() => _json(_show()) as Map<String, dynamic>?;

  /// "start:<i>", "next", "prev", "go:<i>", "end".
  void showGo(String how) => _str(how, _showGo);

  /// Slide `index` at build `step`, `w` × `h` pixels.
  Future<ui.Image?> showFrame(int index, int step, int w, int h) => _image(w, h, (b) => _showFrame(index, step, b, w, h));

  /// The current slide's presenter notes: {text}.
  Map<String, dynamic> notes() => _map(_notes());

  /// View > Outline: [{slide, title: {storage, text}?, texts: [{storage, text}], skipped}].
  List<dynamic> outline() => (_json(_outline()) as List?) ?? const [];

  /// Web pages the engine asked to open since the last call.
  List<String> urls() => [for (final u in (_json(_urls()) as List?) ?? const []) u as String];

  /// Background work finishing (kd_status "busy"): non-zero when something was waiting.
  int tick() => _tick();

  // ---- the desktop's show, drawn here from its triangles (kn-app showapi.rs) ----

  /// Starts the show at slide `index` (`rehearse`: Rehearse Slideshow).
  void playStart(int index, {bool rehearse = false}) => _playStart(index, rehearse);

  /// One frame of the show on a `w` × `h` point screen after `input` (taps, keys): its bytes
  /// (showapi.rs), null when the show isn't playing.
  Uint8List? playFrame(double w, double h, double ppp, List<Map<String, Object>> input) {
    final len = calloc<Uint32>();
    try {
      final p = _str(jsonEncode(input), (i) => _playFrame(w, h, ppp, i, len));
      if (p == nullptr) return null;
      final b = Uint8List.fromList(p.asTypedList(len.value));
      _freeBytes(p, len.value);
      return b;
    } finally {
      calloc.free(len);
    }
  }

  /// {slide, step, steps, count, rehearse, timer, animating, notes, next, size, repaint}, null
  /// when the show isn't playing.
  Map<String, dynamic>? playStatus() => _json(_playStatus()) as Map<String, dynamic>?;
  void playStop() => _playStop();
}
