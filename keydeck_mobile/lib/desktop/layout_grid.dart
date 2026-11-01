// The theme's slide layouts as pictures (app.rs layout_picker, inspector.rs's layout card): a
// grid of touch-sized thumbnails with their names under them, the current one ringed in the
// accent with a tick. Add Slide and Format > Slide > Slide Layout both open it.
import 'dart:math' as math;
import 'dart:ui' as ui;

import 'package:flutter/material.dart';

import '../engine.dart';
import 'inspector_parts.dart' show showDesktopPopover;
import 'look.dart';
import 'widgets.dart';

/// The layouts' pictures, kept per presentation (they're of its theme).
final _pics = <String, Future<ui.Image?>>{};

Future<ui.Image?> layoutPicture(Engine e, String doc, int i, double aspect) => _pics.putIfAbsent('$doc:$i', () => e.renderLayout(i, 300, (300 * aspect).round()));

/// Opens the grid at `at`; the layout picked (its index), or null.
Future<int?> showLayoutGrid(BuildContext context, Rect at, Engine e, {required List<String> names, required double aspect, int? current, String title = ''}) {
  final screen = MediaQuery.of(context).size;
  // tiles of about 150 points (touch-sized), as many across as fit, at most four, at least two
  // (smaller on a narrow phone)
  const gap = 14.0;
  final room = math.min(screen.width - 24, 4 * (150 + gap) + 28);
  final cols = ((room - 28 + gap) / (150 + gap)).floor().clamp(2, 4);
  final width = cols * (math.min(150.0, (room - 28 - gap * (cols - 1)) / cols) + gap) - gap + 28;
  final doc = '${e.status()['path'] ?? e.status()['title']}';
  // (dropping from the top of the screen, at most four fifths of its height: every layout
  // reachable whatever opened it)
  at = Rect.fromLTWH(screen.width / 2 - 1, 56, 2, 2);
  return showDesktopPopover<int>(context, at, width: width, centered: true, maxHeight: screen.height * 0.8, child: (ctx) {
    final look = LookScope.of(ctx);
    return SingleChildScrollView(
      padding: const EdgeInsets.all(14),
      child: Column(crossAxisAlignment: CrossAxisAlignment.stretch, children: [
        if (title.isNotEmpty) Padding(padding: const EdgeInsets.only(bottom: 10), child: Text(title, textAlign: TextAlign.center, style: Look.font(15, look.text, bold: true))),
        LayoutBuilder(builder: (c, box) {
          // (the tiles from the room the popover really gives)
          final n = ((box.maxWidth + gap) / (150 + gap)).floor().clamp(2, 4);
          final tile = math.min(150.0, (box.maxWidth - gap * (n - 1)) / n - 0.5);
          return Wrap(spacing: gap, runSpacing: gap, children: [
          for (final (i, n) in names.indexed)
            GestureDetector(
              onTap: () => Navigator.of(ctx).pop(i),
              child: SizedBox(
                width: tile,
                child: Column(children: [
                  Stack(children: [
                    Container(
                      width: tile,
                      height: tile * aspect,
                      decoration: BoxDecoration(color: Colors.white, borderRadius: BorderRadius.circular(4), border: Border.all(color: i == current ? look.accent : look.gray(0x5a), width: i == current ? 3 : 1)),
                      child: ClipRRect(
                        borderRadius: BorderRadius.circular(3),
                        child: FutureBuilder<ui.Image?>(future: layoutPicture(e, doc, i, aspect), builder: (c, snap) => snap.data == null ? const SizedBox() : RawImage(image: snap.data, fit: BoxFit.fill)),
                      ),
                    ),
                    if (i == current)
                      Positioned(right: 6, bottom: 6, child: Container(width: 22, height: 22, decoration: BoxDecoration(color: look.accent, shape: BoxShape.circle), child: const Icon(Icons.check, size: 15, color: Colors.white))),
                  ]),
                  const SizedBox(height: 6),
                  Text(n, maxLines: 2, textAlign: TextAlign.center, overflow: TextOverflow.ellipsis, style: Look.font(13, i == current ? look.text : look.textDim)),
                ]),
              ),
            ),
          ]);
        }),
      ]),
    );
  });
}
