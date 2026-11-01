// Automated checks. On Waydroid (its own screenshots come out black): when the app's external
// files folder has a file `test-snapshots`, the app saves what it shows as snap.png there
// whenever a file `snap-now` appears, and runs the key hook lines of a file `keys-now`.
// On Linux: KEYDECK_SNAP names that folder, and
// KEYDECK_SCRIPT runs a script of touches and typing (taps and drags as real pointer events),
// `;`-separated: tap x y | drag x1 y1 x2 y2 [ms] | pinch x y scale | type text | key name |
// hold x1 y1 x2 y2 (stays down) | release | wait ms | snap name | frames (frame timings from
// here) | report name (their build / raster times appended to frames.txt) | quit.
import 'dart:async';
import 'dart:io';
import 'dart:math' as math;
import 'dart:ui' as ui;

import 'package:flutter/gestures.dart';
import 'package:flutter/rendering.dart';
import 'package:flutter/scheduler.dart';
import 'package:flutter/widgets.dart';
import 'package:path_provider/path_provider.dart';

/// Frame timings since `frames` (the performance checks).
final _timings = <ui.FrameTiming>[];
bool _recording = false;

void _startFrames() {
  _timings.clear();
  if (!_recording) {
    _recording = true;
    SchedulerBinding.instance.addTimingsCallback(_timings.addAll);
  }
}

/// The frames' build, raster and total (vsync to raster end) times: median, 90th, 99th
/// percentile and worst, in ms, and how many missed 120 and 60 fps.
void _report(String folder, String name) {
  double ms(Duration d) => d.inMicroseconds / 1000;
  String stats(List<double> v) {
    if (v.isEmpty) return '-';
    v.sort();
    double q(double f) => v[math.min(v.length - 1, (v.length * f).floor())];
    return 'p50 ${q(0.5).toStringAsFixed(1)} p90 ${q(0.9).toStringAsFixed(1)} p99 ${q(0.99).toStringAsFixed(1)} max ${v.last.toStringAsFixed(1)}';
  }

  final build = [for (final t in _timings) ms(t.buildDuration)];
  final raster = [for (final t in _timings) ms(t.rasterDuration)];
  final total = [for (final t in _timings) ms(t.totalSpan)];
  final over8 = total.where((t) => t > 8.33).length, over16 = total.where((t) => t > 16.7).length;
  // frames shown a second over the span they cover (a drag that stalls shows fewer)
  final span = _timings.isEmpty ? 0 : _timings.last.timestampInMicroseconds(ui.FramePhase.rasterFinish) - _timings.first.timestampInMicroseconds(ui.FramePhase.vsyncStart);
  final fps = span > 0 ? (_timings.length - 1) * 1e6 / span : 0;
  File('$folder/frames.txt').writeAsStringSync(
    '$name: ${_timings.length} frames, ${fps.toStringAsFixed(0)} fps; build ${stats(build)}; raster ${stats(raster)}; total ${stats(total)}; >8.3ms $over8, >16.7ms $over16\n',
    mode: FileMode.append,
  );
  _timings.clear();
}

Future<void> startTestSnapshots(GlobalKey boundary, {void Function(String text)? type, void Function(String key)? key}) async {
  Directory? dir;
  try {
    dir = await getExternalStorageDirectory();
  } catch (_) {
    final env = Platform.environment['KEYDECK_SNAP'];
    if (env != null) dir = Directory(env);
  }
  if (dir == null) return;
  final folder = dir.path;
  Future<void> snap(String name) async {
    await SchedulerBinding.instance.endOfFrame;
    final b = boundary.currentContext?.findRenderObject() as RenderRepaintBoundary?;
    if (b == null) return;
    final img = await b.toImage(pixelRatio: ui.PlatformDispatcher.instance.views.first.devicePixelRatio);
    final png = await img.toByteData(format: ui.ImageByteFormat.png);
    if (png != null) await File('$folder/$name.png').writeAsBytes(png.buffer.asUint8List());
  }

  final script = Platform.environment['KEYDECK_SCRIPT'];
  if (script != null) {
    await Future<void>.delayed(const Duration(milliseconds: 1500));
    var pointer = 100;
    void send(PointerEvent e) => GestureBinding.instance.handlePointerEvent(e);
    Future<void> frames(int ms) => Future<void>.delayed(Duration(milliseconds: ms));
    var held = Offset.zero;
    for (final raw in script.split(';')) {
      final p = raw.trim().split(RegExp(r'\s+'));
      if (p.isEmpty || p[0].isEmpty) continue;
      double n(int i) => double.parse(p[i]);
      switch (p[0]) {
        case 'tap':
          final at = Offset(n(1), n(2));
          pointer++;
          send(PointerDownEvent(pointer: pointer, position: at, kind: PointerDeviceKind.touch));
          await frames(60);
          send(PointerUpEvent(pointer: pointer, position: at, kind: PointerDeviceKind.touch));
          await frames(120);
        case 'drag':
          final a = Offset(n(1), n(2)), b = Offset(n(3), n(4));
          final ms = p.length > 5 ? n(5).toInt() : 300;
          pointer++;
          send(PointerDownEvent(pointer: pointer, position: a, kind: PointerDeviceKind.touch));
          // (a move every 8 ms, as a 120 Hz touch screen sends them)
          final steps = math.max(20, ms ~/ 8);
          for (var i = 1; i <= steps; i++) {
            await frames(ms ~/ steps);
            send(PointerMoveEvent(pointer: pointer, position: Offset.lerp(a, b, i / steps)!, delta: (b - a) / steps.toDouble(), kind: PointerDeviceKind.touch));
          }
          send(PointerUpEvent(pointer: pointer, position: b, kind: PointerDeviceKind.touch));
          await frames(100);
        case 'hold':
          // a drag that stays down (a snap mid-drag; 'release' lifts it)
          final a = Offset(n(1), n(2)), b = Offset(n(3), n(4));
          pointer++;
          held = b;
          send(PointerDownEvent(pointer: pointer, position: a, kind: PointerDeviceKind.touch));
          for (var i = 1; i <= 20; i++) {
            await frames(15);
            send(PointerMoveEvent(pointer: pointer, position: Offset.lerp(a, b, i / 20)!, delta: (b - a) / 20, kind: PointerDeviceKind.touch));
          }
        case 'release':
          send(PointerUpEvent(pointer: pointer, position: held, kind: PointerDeviceKind.touch));
          await frames(100);
        case 'pinch':
          // two fingers from 60 apart round (x, y) to `scale` times that
          final c = Offset(n(1), n(2));
          final k = n(3);
          final a = pointer + 1, b = pointer + 2;
          pointer += 2;
          send(PointerDownEvent(pointer: a, position: c - const Offset(60, 0), kind: PointerDeviceKind.touch));
          send(PointerDownEvent(pointer: b, position: c + const Offset(60, 0), kind: PointerDeviceKind.touch));
          for (var i = 1; i <= 20; i++) {
            await frames(16);
            final d = 60 * (1 + (k - 1) * i / 20);
            send(PointerMoveEvent(pointer: a, position: c - Offset(d, 0), kind: PointerDeviceKind.touch));
            send(PointerMoveEvent(pointer: b, position: c + Offset(d, 0), kind: PointerDeviceKind.touch));
          }
          final d = 60 * k;
          send(PointerUpEvent(pointer: a, position: c - Offset(d, 0), kind: PointerDeviceKind.touch));
          send(PointerUpEvent(pointer: b, position: c + Offset(d, 0), kind: PointerDeviceKind.touch));
          await frames(100);
        case 'type':
          type?.call(p.sublist(1).join(' ').replaceAll(r'\n', '\n'));
          await frames(50);
        case 'key':
          key?.call(p[1]);
          await frames(50);
        case 'menu':
          key?.call('menu:${p.sublist(1).join(' ')}');
          await frames(100);
        case 'wait':
          await frames(n(1).toInt());
        case 'snap':
          await snap(p[1]);
        case 'frames':
          _startFrames();
        case 'mark':
          stderr.writeln('MARK');
        case 'report':
          _report(folder, p[1]);
        case 'quit':
          exit(0);
      }
    }
    return;
  }
  if (!File('$folder/test-snapshots').existsSync()) return;
  Timer.periodic(const Duration(milliseconds: 500), (_) async {
    // `keys-now`: lines for the key hook ("menu:<command>", "action:<engine action>",
    // "set:<key>=<json>", "menu:open:<file>"), run in order
    final keys = File('$folder/keys-now');
    if (keys.existsSync()) {
      final lines = keys.readAsLinesSync();
      try {
        keys.deleteSync();
      } catch (_) {}
      for (final l in lines.where((l) => l.trim().isNotEmpty)) {
        // ("frames" / "report:<name>": the frame timings, as the script's)
        if (l.trim() == 'frames') {
          _startFrames();
          continue;
        }
        if (l.trim().startsWith('report:')) {
          _report(folder, l.trim().substring(7));
          continue;
        }
        key?.call(l.trim());
        await Future<void>.delayed(const Duration(milliseconds: 150));
      }
    }
    final trigger = File('$folder/snap-now');
    if (!trigger.existsSync()) return;
    try {
      trigger.deleteSync();
    } catch (_) {}
    await snap('snap');
  });
}
