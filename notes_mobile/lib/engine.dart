// Notes' library (Rust: notes-linux/crates/nt-ffi over nt-core, libnotes_core.so): the desktop
// Notes' store, note bodies, tables, drawings, locked notes, search and Smart Folders, with no
// interface of its own. Every call is a method name and JSON arguments in, JSON out.
import 'dart:convert';
import 'dart:ffi';
import 'dart:isolate';

import 'package:ffi/ffi.dart';

typedef _CallC = Pointer<Utf8> Function(Pointer<Utf8>, Pointer<Utf8>);
typedef _FreeC = Void Function(Pointer<Utf8>);
typedef _FreeD = void Function(Pointer<Utf8>);

/// A call the engine refused (its message).
class EngineError implements Exception {
  EngineError(this.message);
  final String message;
  @override
  String toString() => message;
}

dynamic _call(DynamicLibrary lib, String method, Map<String, dynamic> args) {
  final f = lib.lookupFunction<_CallC, _CallC>('nt_call');
  final free = lib.lookupFunction<_FreeC, _FreeD>('nt_free_str');
  final m = method.toNativeUtf8(), a = jsonEncode(args).toNativeUtf8();
  try {
    final r = f(m, a);
    if (r == nullptr) return null;
    final s = r.toDartString();
    free(r);
    final v = jsonDecode(s);
    if (v is Map && v['error'] is String) throw EngineError(v['error'] as String);
    return v;
  } finally {
    malloc.free(m);
    malloc.free(a);
  }
}

class Engine {
  Engine() : _lib = DynamicLibrary.open(libName);
  static const libName = 'libnotes_core.so';
  final DynamicLibrary _lib;

  dynamic call(String method, [Map<String, dynamic> args = const {}]) => _call(_lib, method, args);

  /// The same on a worker isolate (reading a big library, importing, searching): the engine's
  /// state is the process's, so the worker sees the open library.
  Future<dynamic> run(String method, [Map<String, dynamic> args = const {}]) => Isolate.run(() => _call(DynamicLibrary.open(libName), method, args));

  Map<String, dynamic> map(String method, [Map<String, dynamic> args = const {}]) => (call(method, args) as Map).cast<String, dynamic>();
}
