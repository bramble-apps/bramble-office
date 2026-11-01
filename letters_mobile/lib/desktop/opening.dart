// The "Opening “name”…" panel shown while a document is read (Keydeck's kn-app opening_window,
// after Keynote's): 349 × 112, an empty title strip, then a page, the name, a bar with a blue
// segment sweeping across it, and a round × to cancel.
import 'package:flutter/widgets.dart';

import 'look.dart';
import 'widgets.dart';

class OpeningPanel extends StatefulWidget {
  const OpeningPanel({super.key, required this.name, required this.onCancel});

  /// The file's name ('' while the system's picker is still copying it).
  final String name;
  final VoidCallback onCancel;

  @override
  State<OpeningPanel> createState() => _OpeningPanelState();
}

class _OpeningPanelState extends State<OpeningPanel> with SingleTickerProviderStateMixin {
  late final _t = AnimationController(vsync: this, duration: const Duration(milliseconds: 1600))..repeat();

  @override
  void dispose() {
    _t.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final look = LookScope.of(context);
    final dark = !look.light;
    final bg = Color(dark ? 0xFF1E1E1E : 0xFFF2F2F2), track = Color(dark ? 0xFF2C2C2C : 0xFFD8D8D8), text = Color(dark ? 0xFFDCDCDC : 0xFF222222);
    final label = widget.name.isEmpty ? 'Opening…' : 'Opening “${widget.name}”…';
    // a third of the way down, as on the desktop; never wider than the screen
    return LayoutBuilder(builder: (ctx, box) {
      final w = box.maxWidth - 24 < 349 ? box.maxWidth - 24 : 349.0;
      return Stack(children: [
        // (the document underneath takes no presses meanwhile)
        const Positioned.fill(child: AbsorbPointer()),
        Positioned(
          left: (box.maxWidth - w) / 2,
          top: box.maxHeight / 3 - 56,
          width: w,
          height: 112,
          child: Container(
            decoration: BoxDecoration(
              color: bg,
              borderRadius: BorderRadius.circular(12),
              border: Border.all(color: Color(dark ? 0xFF484848 : 0xFFB8B8B8)),
              boxShadow: const [BoxShadow(color: Color(0x50000000), offset: Offset(0, 8), blurRadius: 28)],
            ),
            child: Column(children: [
              const SizedBox(height: 27),
              Container(height: 1, color: Color(dark ? 0xFF101010 : 0xFFD0D0D0)),
              Expanded(
                child: Row(children: [
                  const SizedBox(width: 16),
                  // the first page
                  Container(width: 26, height: 34, color: const Color(0xFFFFFFFF)),
                  const SizedBox(width: 18),
                  Expanded(
                    child: Column(mainAxisAlignment: MainAxisAlignment.center, crossAxisAlignment: CrossAxisAlignment.stretch, children: [
                      // (a long name is cut short to fit before the button)
                      Text(label, maxLines: 1, overflow: TextOverflow.ellipsis, style: Look.font(14, text, bold: true)),
                      const SizedBox(height: 6),
                      SizedBox(
                        height: 5,
                        child: AnimatedBuilder(animation: _t, builder: (_, _) => CustomPaint(painter: _Sweep(_t.value, track))),
                      ),
                    ]),
                  ),
                  const SizedBox(width: 14),
                  // round cancel button
                  GestureDetector(
                    behavior: HitTestBehavior.opaque,
                    onTap: widget.onCancel,
                    child: SizedBox(
                      width: 40,
                      height: 40,
                      child: CustomPaint(painter: _Cancel(dark)),
                    ),
                  ),
                  const SizedBox(width: 6),
                ]),
              ),
            ]),
          ),
        ),
      ]);
    });
  }
}

/// The bar: a blue segment sweeping left to right over and over.
class _Sweep extends CustomPainter {
  _Sweep(this.t, this.track);
  final double t;
  final Color track;

  @override
  void paint(Canvas c, Size s) {
    final r = const Radius.circular(2.5);
    c.drawRRect(RRect.fromRectAndRadius(Offset.zero & s, r), Paint()..color = track);
    final seg = s.width * 0.35;
    final a = (-seg + (s.width + seg) * t).clamp(0.0, s.width);
    final b = ((s.width + seg) * t).clamp(0.0, s.width);
    if (b > a) c.drawRRect(RRect.fromRectAndRadius(Rect.fromLTRB(a, 0, b, s.height), r), Paint()..color = const Color(0xFF0A84FF));
  }

  @override
  bool shouldRepaint(_Sweep o) => o.t != t || o.track != track;
}

class _Cancel extends CustomPainter {
  _Cancel(this.dark);
  final bool dark;

  @override
  void paint(Canvas c, Size s) {
    final o = s.center(Offset.zero);
    c.drawCircle(o, 11.5, Paint()
      ..style = PaintingStyle.stroke
      ..strokeWidth = 3
      ..color = const Color(0xFF2F6FB0));
    c.drawCircle(o, 9, Paint()..color = const Color(0xFF9C9C9C));
    const k = 3.5;
    final p = Paint()
      ..strokeWidth = 2
      ..color = dark ? const Color(0xFF2A2A2A) : const Color(0xFFFFFFFF);
    c.drawLine(o + const Offset(-k, -k), o + const Offset(k, k), p);
    c.drawLine(o + const Offset(-k, k), o + const Offset(k, -k), p);
  }

  @override
  bool shouldRepaint(_Cancel o) => o.dark != dark;
}
