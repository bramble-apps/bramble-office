// Letters' document library (Rust: pages-linux/crates/pg-app/src/ffi.rs, libpg_app.so): the
// desktop's documents, editing logic and page drawing, with no interface of its own. Everything
// on screen is Flutter's; this tells the engine what happened (a press at a point of the view,
// text typed, a command) and asks it for state as data and for the pages' content as strips.
import 'dart:convert';
import 'dart:ffi';
import 'dart:isolate';
import 'dart:typed_data';
import 'dart:ui' as ui;

import 'package:ffi/ffi.dart';

typedef _S = Pointer<Utf8> Function();
typedef _VS = Void Function(Pointer<Utf8>);
typedef _DS = void Function(Pointer<Utf8>);

/// The Content Hub's catalogue, searches, thumbnails and downloads (ffi.rs lt_hub): they
/// hold no document, so the slow ones run on a worker isolate with its own handle.
dynamic hubSync(String kind, String arg) {
  final lib = DynamicLibrary.open('libpg_app.so');
  final f = lib.lookupFunction<Pointer<Utf8> Function(Pointer<Utf8>, Pointer<Utf8>), Pointer<Utf8> Function(Pointer<Utf8>, Pointer<Utf8>)>('lt_hub');
  final free = lib.lookupFunction<Void Function(Pointer<Utf8>), void Function(Pointer<Utf8>)>('lt_free_str');
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
  Engine() : _lib = DynamicLibrary.open('libpg_app.so');
  final DynamicLibrary _lib;

  late final _home = _lib.lookupFunction<_VS, _DS>('lt_home');
  late final _init = _lib.lookupFunction<Void Function(Float, Float), void Function(double, double)>('lt_init');
  late final _view = _lib.lookupFunction<Void Function(Float, Float), void Function(double, double)>('lt_view');
  late final _free = _lib.lookupFunction<Void Function(Pointer<Utf8>), void Function(Pointer<Utf8>)>('lt_free_str');
  late final _error = _lib.lookupFunction<_S, Pointer<Utf8> Function()>('lt_error');
  late final _open = _lib.lookupFunction<Int32 Function(Pointer<Utf8>), int Function(Pointer<Utf8>)>('lt_open');
  late final _openBegin = _lib.lookupFunction<_VS, _DS>('lt_open_begin');
  late final _openPoll = _lib.lookupFunction<Int32 Function(), int Function()>('lt_open_poll');
  late final _openCancel = _lib.lookupFunction<Void Function(), void Function()>('lt_open_cancel');
  late final _save = _lib.lookupFunction<Int32 Function(Pointer<Utf8>), int Function(Pointer<Utf8>)>('lt_save');
  late final _export = _lib.lookupFunction<Int32 Function(Pointer<Utf8>), int Function(Pointer<Utf8>)>('lt_export');
  late final _scroll = _lib.lookupFunction<Void Function(Float, Float, Float), void Function(double, double, double)>('lt_scroll');
  late final _geometry = _lib.lookupFunction<_S, Pointer<Utf8> Function()>('lt_geometry');
  late final _stripKey = _lib.lookupFunction<Uint64 Function(Uint32, Uint32), int Function(int, int)>('lt_strip_key');
  late final _renderStrip = _lib.lookupFunction<Int32 Function(Uint32, Uint32, Float, Pointer<Uint8>, Uint32, Uint32), int Function(int, int, double, Pointer<Uint8>, int, int)>('lt_render_strip');
  late final _renderPage = _lib.lookupFunction<Int32 Function(Uint32, Float, Pointer<Uint8>, Uint32, Uint32), int Function(int, double, Pointer<Uint8>, int, int)>('lt_render_page');
  late final _renderObject = _lib.lookupFunction<Int32 Function(Uint64, Float, Pointer<Uint8>, Uint32, Uint32), int Function(int, double, Pointer<Uint8>, int, int)>('lt_render_object');
  late final _sidebar = _lib.lookupFunction<_S, Pointer<Utf8> Function()>('lt_sidebar');
  late final _overlays = _lib.lookupFunction<_S, Pointer<Utf8> Function()>('lt_overlays');
  late final _pointer = _lib.lookupFunction<Void Function(Uint32, Float, Float, Uint32, Bool, Bool), void Function(int, double, double, int, bool, bool)>('lt_pointer');
  late final _contextMenu = _lib.lookupFunction<_S, Pointer<Utf8> Function()>('lt_context_menu');
  late final _action = _lib.lookupFunction<_VS, _DS>('lt_action');
  late final _table = _lib.lookupFunction<_S, Pointer<Utf8> Function()>('lt_table');
  late final _tableTabMenu = _lib.lookupFunction<Pointer<Utf8> Function(Bool, Uint32), Pointer<Utf8> Function(bool, int)>('lt_table_tab_menu');
  late final _popover = _lib.lookupFunction<Pointer<Utf8> Function(Pointer<Utf8>), Pointer<Utf8> Function(Pointer<Utf8>)>('lt_popover');
  late final _text = _lib.lookupFunction<_VS, _DS>('lt_text');
  late final _key = _lib.lookupFunction<Void Function(Pointer<Utf8>, Bool, Bool, Bool), void Function(Pointer<Utf8>, bool, bool, bool)>('lt_key');
  late final _paste = _lib.lookupFunction<_VS, _DS>('lt_paste');
  late final _copied = _lib.lookupFunction<_S, Pointer<Utf8> Function()>('lt_copied');
  late final _menus = _lib.lookupFunction<_S, Pointer<Utf8> Function()>('lt_menus');
  late final _run = _lib.lookupFunction<_VS, _DS>('lt_run');
  late final _toolbar = _lib.lookupFunction<_VS, _DS>('lt_toolbar');
  late final _status = _lib.lookupFunction<_S, Pointer<Utf8> Function()>('lt_status');
  late final _inspector = _lib.lookupFunction<_S, Pointer<Utf8> Function()>('lt_inspector');
  late final _inspectorSet = _lib.lookupFunction<Void Function(Pointer<Utf8>, Pointer<Utf8>), void Function(Pointer<Utf8>, Pointer<Utf8>)>('lt_inspector_set');
  late final _thumbnail = _lib.lookupFunction<Pointer<Uint8> Function(Uint64, Uint32, Pointer<Uint32>), Pointer<Uint8> Function(int, int, Pointer<Uint32>)>('lt_thumbnail');
  late final _freeBytes = _lib.lookupFunction<Void Function(Pointer<Uint8>, Uint32), void Function(Pointer<Uint8>, int)>('lt_free_bytes');
  late final _chrome = _lib.lookupFunction<_S, Pointer<Utf8> Function()>('lt_chrome');
  late final _mediaFrame = _lib.lookupFunction<Uint64 Function(Pointer<Uint8>, Uint32, Pointer<Uint32>, Pointer<Uint32>), int Function(Pointer<Uint8>, int, Pointer<Uint32>, Pointer<Uint32>)>('lt_media_frame');

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

  void start(String home, double w, double h) {
    _str(home, _home);
    _init(w, h);
  }

  // ---- files ----
  String get error => _take(_error());
  String? open(String path) => _str(path, _open) == 0 ? null : error;
  /// Opening read on another thread (the "Opening…" panel animates meanwhile): start it, then
  /// poll until it's open (false), or cancel it.
  void openBegin(String path) => _str(path, _openBegin);
  bool openPoll() => _openPoll() != 0;
  void openCancel() => _openCancel();
  void save(String path) => _str(path, _save);
  String? export(String path) => _str(path, _export) == 0 ? null : error;

  // ---- the pages ----

  /// The pages' view: its size in points.
  void view(double w, double h) => _view(w, h);

  /// Where the view is: the document point at its top-left, and the zoom.
  void scroll(double x, double y, double zoom) => _scroll(x, y, zoom);

  /// {zoom, scroll, size, pages: [[x, y, w, h]…], strip, version, pageInView}.
  Map<String, dynamic> geometry() => _map(_geometry());

  /// A key of what strip `strip` of page `page` shows (it changes when it does).
  int stripKey(int page, int strip) => _stripKey(page, strip);

  /// Strip `strip` of page `page` (`pageW` points wide, `stripH` tall) at `scale` pixels a point.
  Future<ui.Image?> renderStrip(int page, int strip, double scale, double pageW, double stripH) async {
    final pw = (pageW * scale).ceil(), ph = (stripH * scale).ceil();
    if (pw <= 0 || ph <= 0) return null;
    final buf = calloc<Uint8>(pw * ph * 4);
    Uint8List bytes;
    try {
      if (_renderStrip(page, strip, scale, buf, pw, ph) != 0) return null;
      bytes = Uint8List.fromList(buf.asTypedList(pw * ph * 4));
    } finally {
      calloc.free(buf);
    }
    final buffer = await ui.ImmutableBuffer.fromUint8List(bytes);
    final desc = ui.ImageDescriptor.raw(buffer, width: pw, height: ph, pixelFormat: ui.PixelFormat.rgba8888);
    final codec = await desc.instantiateCodec();
    return (await codec.getNextFrame()).image;
  }

  /// Page `page` whole (`pageW` × `pageH` points) at `scale` pixels a point: a thumbnail.
  Future<ui.Image?> renderPage(int page, double scale, double pageW, double pageH) async {
    final pw = (pageW * scale).ceil(), ph = (pageH * scale).ceil();
    if (pw <= 0 || ph <= 0) return null;
    final buf = calloc<Uint8>(pw * ph * 4);
    Uint8List bytes;
    try {
      if (_renderPage(page, scale, buf, pw, ph) != 0) return null;
      bytes = Uint8List.fromList(buf.asTypedList(pw * ph * 4));
    } finally {
      calloc.free(buf);
    }
    final buffer = await ui.ImmutableBuffer.fromUint8List(bytes);
    final desc = ui.ImageDescriptor.raw(buffer, width: pw, height: ph, pixelFormat: ui.PixelFormat.rgba8888);
    return (await (await desc.instantiateCodec()).getNextFrame()).image;
  }

  /// A moving object alone (overlays' live objects: its box, `w` × `h` points) at `scale`
  /// pixels a point, to draw at its live place while it's dragged.
  Future<ui.Image?> renderObject(int id, double scale, double w, double h) async {
    final pw = (w * scale).ceil(), ph = (h * scale).ceil();
    if (pw <= 0 || ph <= 0) return null;
    final buf = calloc<Uint8>(pw * ph * 4);
    Uint8List bytes;
    try {
      if (_renderObject(id, scale, buf, pw, ph) != 0) return null;
      bytes = Uint8List.fromList(buf.asTypedList(pw * ph * 4));
    } finally {
      calloc.free(buf);
    }
    final buffer = await ui.ImmutableBuffer.fromUint8List(bytes);
    final desc = ui.ImageDescriptor.raw(buffer, width: pw, height: ph, pixelFormat: ui.PixelFormat.rgba8888);
    return (await (await desc.instantiateCodec()).getNextFrame()).image;
  }

  /// The sidebar: {mode: none / thumbs / toc, pages, current, layoutDoc, headings, canInsertToc}.
  Map<String, dynamic> sidebar() => _map(_sidebar());

  /// {caret, rects, objects, cells, live (its objects' boxes), liveFrame, marquee, dragging,
  /// measure, version}.
  Map<String, dynamic> overlays() => _map(_overlays());

  /// The newest frame of the movie playing in place (lt_action "mediaplay"), at most `cap`
  /// bytes: (its drawable, the picture), null when no new frame came.
  Future<(int, ui.Image)?> mediaFrame(int cap) async {
    final buf = calloc<Uint8>(cap), w = calloc<Uint32>(), h = calloc<Uint32>();
    Uint8List bytes;
    int id, pw, ph;
    try {
      id = _mediaFrame(buf, cap, w, h);
      if (id == 0) return null;
      pw = w.value;
      ph = h.value;
      bytes = Uint8List.fromList(buf.asTypedList(pw * ph * 4));
    } finally {
      calloc.free(buf);
      calloc.free(w);
      calloc.free(h);
    }
    final buffer = await ui.ImmutableBuffer.fromUint8List(bytes);
    final desc = ui.ImageDescriptor.raw(buffer, width: pw, height: ph, pixelFormat: ui.PixelFormat.rgba8888);
    return (id, (await (await desc.instantiateCodec()).getNextFrame()).image);
  }

  /// The pointer on the view (points): kind 0 moved (held), 1 pressed, 2 released; button 1 the
  /// secondary.
  void pointer(int kind, double x, double y, {int button = 0, bool shift = false, bool ctrl = false}) => _pointer(kind, x, y, button, shift, ctrl);

  /// The right-click menu's items for what's selected.
  List<dynamic> contextMenu() => (_json(_contextMenu()) as List?) ?? [];
  void action(String a) => _str(a, _action);

  /// The selected table for its chrome on the page (null: none).
  Map<String, dynamic>? table() => _json(_table()) as Map<String, dynamic>?;

  /// A row or column tab's menu items.
  List<dynamic> tableTabMenu(bool rows, int i) => (_json(_tableTabMenu(rows, i)) as List?) ?? [];

  /// A toolbar popover's contents (View, Insert, Media: menu items; Shape, Chart: their tiles).
  dynamic popover(String id) => _str(id, (p) => _json(_popover(p)));

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

  /// Changes an inspector control (`key` as inspector() names it).
  void inspectorSet(String key, Object? value) => _str(key, (k) => _str(jsonEncode(value), (v) => _inspectorSet(k, v)));
}
