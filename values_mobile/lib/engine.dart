// Values' document library (Rust: numbers-linux/crates/nm-app/src/ffi.rs, libnm_app.so): the
// desktop's spreadsheets, editing logic and sheet drawing, with no interface of its own.
// Everything on screen is Flutter's; this tells the engine what happened (a press at a point of
// the view, text typed, a command) and asks it for state as data and for the sheet as tiles.
import 'dart:convert';
import 'dart:ffi';
import 'dart:isolate';
import 'dart:typed_data';
import 'dart:ui' as ui;

import 'package:ffi/ffi.dart';

typedef _S = Pointer<Utf8> Function();
typedef _VS = Void Function(Pointer<Utf8>);
typedef _DS = void Function(Pointer<Utf8>);

const _libName = 'libnm_app.so';

/// The Content Hub's catalogue, searches, thumbnails and downloads (ffi.rs vl_hub): they
/// hold no document, so the slow ones run on a worker isolate with its own handle.
dynamic hubSync(String kind, String arg) {
  final lib = DynamicLibrary.open(_libName);
  final f = lib.lookupFunction<Pointer<Utf8> Function(Pointer<Utf8>, Pointer<Utf8>), Pointer<Utf8> Function(Pointer<Utf8>, Pointer<Utf8>)>('vl_hub');
  final free = lib.lookupFunction<Void Function(Pointer<Utf8>), void Function(Pointer<Utf8>)>('vl_free_str');
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
  Engine() : _lib = DynamicLibrary.open(_libName);
  final DynamicLibrary _lib;

  late final _home = _lib.lookupFunction<_VS, _DS>('vl_home');
  late final _init = _lib.lookupFunction<Void Function(Float, Float), void Function(double, double)>('vl_init');
  late final _view = _lib.lookupFunction<Void Function(Float, Float), void Function(double, double)>('vl_view');
  late final _free = _lib.lookupFunction<Void Function(Pointer<Utf8>), void Function(Pointer<Utf8>)>('vl_free_str');
  late final _error = _lib.lookupFunction<_S, Pointer<Utf8> Function()>('vl_error');
  late final _open = _lib.lookupFunction<Int32 Function(Pointer<Utf8>), int Function(Pointer<Utf8>)>('vl_open');
  late final _save = _lib.lookupFunction<Int32 Function(Pointer<Utf8>), int Function(Pointer<Utf8>)>('vl_save');
  late final _export = _lib.lookupFunction<Int32 Function(Pointer<Utf8>), int Function(Pointer<Utf8>)>('vl_export');
  late final _scroll = _lib.lookupFunction<Void Function(Float, Float, Float), void Function(double, double, double)>('vl_scroll');
  late final _geometry = _lib.lookupFunction<_S, Pointer<Utf8> Function()>('vl_geometry');
  late final _renderTile = _lib.lookupFunction<Int32 Function(Int32, Int32, Float, Pointer<Uint8>, Uint32), int Function(int, int, double, Pointer<Uint8>, int)>('vl_render_tile');
  late final _renderHeader = _lib.lookupFunction<Int32 Function(Uint64, Bool, Uint32, Uint32, Float, Pointer<Uint8>, Uint32, Pointer<Uint32>, Pointer<Uint32>), int Function(int, bool, int, int, double, Pointer<Uint8>, int, Pointer<Uint32>, Pointer<Uint32>)>('vl_render_header');
  late final _overlays = _lib.lookupFunction<_S, Pointer<Utf8> Function()>('vl_overlays');
  late final _hit = _lib.lookupFunction<Pointer<Utf8> Function(Float, Float), Pointer<Utf8> Function(double, double)>('vl_hit');
  late final _pointer = _lib.lookupFunction<Void Function(Uint32, Float, Float, Uint32, Bool, Bool), void Function(int, double, double, int, bool, bool)>('vl_pointer');
  late final _contextMenu = _lib.lookupFunction<_S, Pointer<Utf8> Function()>('vl_context_menu');
  late final _action = _lib.lookupFunction<_VS, _DS>('vl_action');
  late final _table = _lib.lookupFunction<_S, Pointer<Utf8> Function()>('vl_table');
  late final _tableTabMenu = _lib.lookupFunction<Pointer<Utf8> Function(Bool, Uint32), Pointer<Utf8> Function(bool, int)>('vl_table_tab_menu');
  late final _popover = _lib.lookupFunction<Pointer<Utf8> Function(Pointer<Utf8>), Pointer<Utf8> Function(Pointer<Utf8>)>('vl_popover');
  late final _text = _lib.lookupFunction<_VS, _DS>('vl_text');
  late final _key = _lib.lookupFunction<Void Function(Pointer<Utf8>, Bool, Bool, Bool), void Function(Pointer<Utf8>, bool, bool, bool)>('vl_key');
  late final _paste = _lib.lookupFunction<_VS, _DS>('vl_paste');
  late final _copied = _lib.lookupFunction<_S, Pointer<Utf8> Function()>('vl_copied');
  late final _menus = _lib.lookupFunction<_S, Pointer<Utf8> Function()>('vl_menus');
  late final _run = _lib.lookupFunction<_VS, _DS>('vl_run');
  late final _toolbar = _lib.lookupFunction<_VS, _DS>('vl_toolbar');
  late final _status = _lib.lookupFunction<_S, Pointer<Utf8> Function()>('vl_status');
  late final _inspector = _lib.lookupFunction<_S, Pointer<Utf8> Function()>('vl_inspector');
  late final _inspectorSet = _lib.lookupFunction<Void Function(Pointer<Utf8>, Pointer<Utf8>), void Function(Pointer<Utf8>, Pointer<Utf8>)>('vl_inspector_set');
  late final _thumbnail = _lib.lookupFunction<Pointer<Uint8> Function(Uint64, Uint32, Pointer<Uint32>), Pointer<Uint8> Function(int, int, Pointer<Uint32>)>('vl_thumbnail');
  late final _freeBytes = _lib.lookupFunction<Void Function(Pointer<Uint8>, Uint32), void Function(Pointer<Uint8>, int)>('vl_free_bytes');
  late final _chrome = _lib.lookupFunction<_S, Pointer<Utf8> Function()>('vl_chrome');

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
  String? save(String path) => _str(path, _save) == 0 ? null : error;
  String? export(String path) => _str(path, _export) == 0 ? null : error;

  // ---- the sheet ----

  /// The sheet's view: its size in points.
  void view(double w, double h) => _view(w, h);

  /// Where the view is: the sheet point at its top-left, and the zoom.
  void scroll(double x, double y, double zoom) => _scroll(x, y, zoom);

  /// {zoom, scroll, content: [x0, y0, x1, y1], version, sheet, tile}.
  Map<String, dynamic> geometry() => _map(_geometry());

  /// Tile (tx, ty) at `scale` pixels a point: `px` pixels square (the engine's TILE), covering
  /// px / scale sheet points from (tx, ty) × that.
  Future<ui.Image?> renderTile(int tx, int ty, double scale, int px) async {
    final n = px * px * 4;
    final buf = calloc<Uint8>(n);
    Uint8List bytes;
    try {
      if (_renderTile(tx, ty, scale, buf, n) != 0) return null;
      bytes = Uint8List.fromList(buf.asTypedList(n));
    } finally {
      calloc.free(buf);
    }
    final buffer = await ui.ImmutableBuffer.fromUint8List(bytes);
    final desc = ui.ImageDescriptor.raw(buffer, width: px, height: px, pixelFormat: ui.PixelFormat.rgba8888);
    return (await (await desc.instantiateCodec()).getNextFrame()).image;
  }

  /// Table `id`'s frozen header rows (`cols` false) or its header columns over rows r0..r1
  /// at `scale` pixels a point (a point's margin round them).
  Future<ui.Image?> renderHeader(int id, bool cols, int r0, int r1, double scale) async {
    // (tiles.rs: at most 8192 × 4096 pixels; malloc's untouched pages cost nothing)
    const cap = 8192 * 4096 * 4;
    final buf = malloc<Uint8>(cap), w = calloc<Uint32>(), h = calloc<Uint32>();
    Uint8List bytes;
    int pw, ph;
    try {
      if (_renderHeader(id, cols, r0, r1, scale, buf, cap, w, h) != 0) return null;
      pw = w.value;
      ph = h.value;
      bytes = Uint8List.fromList(buf.asTypedList(pw * ph * 4));
    } finally {
      malloc.free(buf);
      calloc.free(w);
      calloc.free(h);
    }
    final buffer = await ui.ImmutableBuffer.fromUint8List(bytes);
    final desc = ui.ImageDescriptor.raw(buffer, width: pw, height: ph, pixelFormat: ui.PixelFormat.rgba8888);
    return (await (await desc.instantiateCodec()).getNextFrame()).image;
  }

  /// {frozen, objects, live, text, comments, refs, titles, version}: what's drawn over the sheet.
  Map<String, dynamic> overlays() => _map(_overlays());

  /// The cell under view point (x, y): {table, r, c}, or null.
  Map<String, dynamic>? hit(double x, double y) => _json(_hit(x, y)) as Map<String, dynamic>?;

  /// The pointer on the view (points): kind 0 moved (held), 1 pressed, 2 released; button 1 the
  /// secondary.
  void pointer(int kind, double x, double y, {int button = 0, bool shift = false, bool ctrl = false}) => _pointer(kind, x, y, button, shift, ctrl);

  /// The edit menu's items for what's selected.
  List<dynamic> contextMenu() => (_json(_contextMenu()) as List?) ?? [];
  void action(String a) => _str(a, _action);

  /// The table with the selected cells, for its chrome on the sheet (null: none).
  Map<String, dynamic>? table() => _json(_table()) as Map<String, dynamic>?;

  /// A row or column tab's menu items.
  List<dynamic> tableTabMenu(bool rows, int i) => (_json(_tableTabMenu(rows, i)) as List?) ?? [];

  /// A toolbar popover's contents (View, Insert, Media: menu items; Table, Shape, Chart: tiles).
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
