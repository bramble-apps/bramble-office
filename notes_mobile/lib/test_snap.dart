// Automated checks. On Waydroid (its own screenshots come out black): when the app's external
// files folder has a file `test-snapshots`, the app saves what it shows as snap.png there
// whenever a file `snap-now` appears. On Linux: NOTES_SNAP names that folder, and
// NOTES_SCRIPT runs a script of touches and typing (taps and drags as real pointer events),
// `;`-separated: tap x y | drag x1 y1 x2 y2 [ms] | pinch x y scale | type text | key name |
// hold x1 y1 x2 y2 (stays down) | release | wait ms | snap name | quit.
import 'dart:async';
import 'dart:io';
import 'dart:ui' as ui;

import 'package:flutter/gestures.dart';
import 'package:flutter/rendering.dart';
import 'package:flutter/scheduler.dart';
import 'package:flutter/widgets.dart';
import 'package:path_provider/path_provider.dart';

Future<void> startTestSnapshots(GlobalKey boundary, {void Function(String text)? type, void Function(String key)? key}) async {
  Directory? dir;
  try {
    dir = await getExternalStorageDirectory();
  } catch (_) {
    final env = Platform.environment['NOTES_SNAP'];
    if (env != null) dir = Directory(env);
  }
  if (dir == null) return;
  final folder = dir.path;
  Future<void> snap(String name) async {
    await SchedulerBinding.instance.endOfFrame;
    // (with a `measure` file: every round button's and circle's size as drawn, for checking
    // that none is squeezed)
    if (File('$folder/measure').existsSync() || Platform.environment['NOTES_MEASURE'] != null) {
      File('$folder/$name-sizes.txt').writeAsStringSync(measureRound());
    }
    final b = boundary.currentContext?.findRenderObject() as RenderRepaintBoundary?;
    if (b == null) return;
    final img = await b.toImage(pixelRatio: ui.PlatformDispatcher.instance.views.first.devicePixelRatio);
    final png = await img.toByteData(format: ui.ImageByteFormat.png);
    if (png != null) await File('$folder/$name.png').writeAsBytes(png.buffer.asUint8List());
  }

  final script = Platform.environment['NOTES_SCRIPT'];
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
          const steps = 20;
          for (var i = 1; i <= steps; i++) {
            await frames(ms ~/ steps);
            send(PointerMoveEvent(pointer: pointer, position: Offset.lerp(a, b, i / steps)!, delta: (b - a) / steps.toDouble(), kind: PointerDeviceKind.touch));
          }
          send(PointerUpEvent(pointer: pointer, position: b, kind: PointerDeviceKind.touch));
          await frames(100);
        case 'path':
          // one finger through the points (a lasso loop): path x1 y1 x2 y2 …
          final ptsList = [for (var i = 1; i + 1 < p.length; i += 2) Offset(n(i), n(i + 1))];
          pointer++;
          send(PointerDownEvent(pointer: pointer, position: ptsList.first, kind: PointerDeviceKind.touch));
          for (var k = 1; k < ptsList.length; k++) {
            for (var i = 1; i <= 8; i++) {
              await frames(12);
              final at = Offset.lerp(ptsList[k - 1], ptsList[k], i / 8)!;
              send(PointerMoveEvent(pointer: pointer, position: at, delta: (ptsList[k] - ptsList[k - 1]) / 8, kind: PointerDeviceKind.touch));
            }
          }
          send(PointerUpEvent(pointer: pointer, position: ptsList.last, kind: PointerDeviceKind.touch));
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
        case 'quit':
          exit(0);
      }
    }
    return;
  }
  if (!File('$folder/test-snapshots').existsSync()) return;
  Timer.periodic(const Duration(milliseconds: 500), (_) async {
    final trigger = File('$folder/snap-now');
    if (!trigger.existsSync()) return;
    try {
      trigger.deleteSync();
    } catch (_) {}
    await snap('snap');
  });
}

/// The glass buttons' and capsules' clips and the circles (swipe actions, checklist circles) as
/// drawn on screen (logical pixels, after any transform): `kind w×h radius at x,y`.
String measureRound() {
  final out = StringBuffer();
  void visit(RenderObject o) {
    if (o is RenderBox && o.hasSize && o.attached) {
      String? kind;
      double r = 0;
      if (o is RenderClipRRect) {
        final br = o.borderRadius.resolve(TextDirection.ltr);
        r = br.topLeft.x;
        if (r >= 10) kind = (o.size.width - o.size.height).abs() < 1 && (r - o.size.width / 2).abs() < 1 ? 'glass-circle' : 'glass-capsule';
      } else if (o is RenderDecoratedBox && o.decoration is BoxDecoration && (o.decoration as BoxDecoration).shape == BoxShape.circle) {
        kind = 'circle';
        r = o.size.shortestSide / 2;
      }
      if (kind != null) {
        final rect = MatrixUtils.transformRect(o.getTransformTo(null), Offset.zero & o.size);
        if (rect.width > 4) {
          out.writeln('$kind ${rect.width.toStringAsFixed(2)}x${rect.height.toStringAsFixed(2)} r=${r.toStringAsFixed(1)} at ${rect.left.toStringAsFixed(0)},${rect.top.toStringAsFixed(0)}');
        }
      }
    }
    o.visitChildren(visit);
  }

  for (final v in RendererBinding.instance.renderViews) {
    visit(v);
  }
  return out.toString();
}
