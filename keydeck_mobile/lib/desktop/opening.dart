// Opening a presentation (ffi.rs kd_open_start / kd_open_poll): the engine reads it on a thread
// of its own while the desktop's "Opening “name”…" panel shows (kn-app app.rs opening_window,
// Keynote's): 349 × 112, an empty title strip, the first slide's picture, the name, a blue
// segment sweeping along a bar, and a round × that cancels.
import 'dart:async';
import 'dart:io';
import 'dart:math' as math;

import 'package:flutter/material.dart';

import '../engine.dart';
import 'look.dart';

/// Opens `path`, the panel showing once it takes a moment (a third of a second); the error
/// (null: opened, or cancelled).
Future<String?> openWithPanel(BuildContext context, Engine e, Look look, String path) async {
  e.openStart(path);
  final overlay = Overlay.of(context, rootOverlay: true);
  final state = ValueNotifier<Map<String, dynamic>?>(null);
  OverlayEntry? entry;
  final started = DateTime.now();
  final done = Completer<String?>();
  Timer.periodic(const Duration(milliseconds: 40), (t) {
    final st = e.openPoll();
    final s = st?['state'];
    if (s == 'opening') {
      state.value = st;
      if (entry == null && DateTime.now().difference(started).inMilliseconds > 330) {
        entry = OverlayEntry(builder: (_) => _OpeningPanel(look: look, state: state, onCancel: e.openCancel));
        overlay.insert(entry!);
      }
      return;
    }
    t.cancel();
    entry?.remove();
    state.dispose();
    done.complete(s == 'failed' ? (st?['message'] as String? ?? 'it could not be read') : null);
  });
  return done.future;
}

class _OpeningPanel extends StatefulWidget {
  const _OpeningPanel({required this.look, required this.state, required this.onCancel});
  final Look look;
  final ValueNotifier<Map<String, dynamic>?> state;
  final VoidCallback onCancel;

  @override
  State<_OpeningPanel> createState() => _OpeningPanelState();
}

class _OpeningPanelState extends State<_OpeningPanel> with SingleTickerProviderStateMixin {
  // (the bar's sweep: once across every 1.6 s)
  late final _sweep = AnimationController(vsync: this, duration: const Duration(milliseconds: 1600))..repeat();
  bool _cancelled = false;

  @override
  void dispose() {
    _sweep.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final dark = !widget.look.light;
    Color g(int v) => Color.fromARGB(255, v, v, v);
    final bg = g(dark ? 0x1e : 0xf2), track = g(dark ? 0x2c : 0xd8), text = g(dark ? 0xdc : 0x22);
    const blue = Color(0xFF0A84FF);
    return LayoutBuilder(builder: (context, box) {
      // a third of the way down, centred; narrower than 349 on a small screen
      final w = math.min(349.0, box.maxWidth - 24);
      return Stack(children: [
        // (the presentation behind can't be used while it opens)
        const Positioned.fill(child: ModalBarrier(dismissible: false, color: Color(0x22000000))),
        Positioned(
          left: (box.maxWidth - w) / 2,
          top: box.maxHeight / 3 - 56,
          width: w,
          height: 112,
          child: Material(
            type: MaterialType.transparency,
            child: Container(
              decoration: BoxDecoration(
                color: bg,
                borderRadius: BorderRadius.circular(12),
                border: Border.all(color: g(dark ? 0x48 : 0xb8)),
                boxShadow: const [BoxShadow(color: Color(0x40000000), blurRadius: 18, offset: Offset(0, 4))],
              ),
              child: Column(children: [
                // the empty title strip
                Container(height: 28, decoration: BoxDecoration(border: Border(bottom: BorderSide(color: g(dark ? 0x10 : 0xd0))))),
                Expanded(
                  child: ValueListenableBuilder<Map<String, dynamic>?>(
                    valueListenable: widget.state,
                    builder: (context, st, _) {
                      final name = st?['name'] as String? ?? '';
                      final preview = st?['preview'] as String?;
                      return Row(children: [
                        const SizedBox(width: 16),
                        // the first slide
                        SizedBox(
                          width: 60,
                          height: 34,
                          child: preview == null ? const ColoredBox(color: Colors.white) : Image.file(File(preview), fit: BoxFit.cover, gaplessPlayback: true),
                        ),
                        const SizedBox(width: 18),
                        Expanded(
                          child: Column(mainAxisAlignment: MainAxisAlignment.center, crossAxisAlignment: CrossAxisAlignment.start, children: [
                            // (a long name shortened in the middle)
                            Text('Opening “$name”…', maxLines: 1, overflow: TextOverflow.ellipsis, style: Look.font(14, text, bold: true)),
                            const SizedBox(height: 6),
                            SizedBox(
                              height: 5,
                              child: AnimatedBuilder(
                                animation: _sweep,
                                builder: (context, _) => CustomPaint(painter: _Bar(_sweep.value, track, blue), size: Size.infinite),
                              ),
                            ),
                          ]),
                        ),
                        // the round ×
                        GestureDetector(
                          behavior: HitTestBehavior.opaque,
                          onTap: _cancelled
                              ? null
                              : () {
                                  setState(() => _cancelled = true);
                                  widget.onCancel();
                                },
                          child: SizedBox(
                            width: 52,
                            height: 52,
                            child: Center(
                              child: Container(
                                width: 23,
                                height: 23,
                                decoration: BoxDecoration(shape: BoxShape.circle, color: g(0x9c), border: Border.all(color: const Color(0xFF2F6FB0), width: 3)),
                                child: Icon(Icons.close, size: 11, color: dark ? g(0x2a) : Colors.white),
                              ),
                            ),
                          ),
                        ),
                      ]);
                    },
                  ),
                ),
              ]),
            ),
          ),
        ),
      ]);
    });
  }
}

/// The bar: its track, and a blue segment a third of its width sweeping across.
class _Bar extends CustomPainter {
  _Bar(this.t, this.track, this.blue);
  final double t;
  final Color track, blue;

  @override
  void paint(Canvas c, Size s) {
    final r = Radius.circular(s.height / 2);
    c.drawRRect(RRect.fromRectAndRadius(Offset.zero & s, r), Paint()..color = track);
    final seg = s.width * 0.35;
    final a = math.max(0.0, -seg + (s.width + seg) * t), b = math.min(s.width, (s.width + seg) * t);
    if (b > a) c.drawRRect(RRect.fromRectAndRadius(Rect.fromLTRB(a, 0, b, s.height), r), Paint()..color = blue);
  }

  @override
  bool shouldRepaint(_Bar o) => o.t != t;
}
