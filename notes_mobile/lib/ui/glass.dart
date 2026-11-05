// Liquid Glass (iOS 26): toolbar buttons and capsules float over the content, which shows
// through them blurred under the glass's tint, with a lit rim along the top and a soft shadow.
// Buttons are 48 dp (the suite's touch size); a tinted one (Done) is filled with the accent.
import 'dart:ui' as ui;

import 'package:flutter/material.dart';

import '../theme.dart';

class Glass extends StatelessWidget {
  const Glass({super.key, required this.child, this.radius = 24, this.tint});
  final Widget child;
  final double radius;

  /// A coloured glass (the accent behind Done) instead of the clear one.
  final Color? tint;

  @override
  Widget build(BuildContext context) {
    final p = Palette.of(context);
    final r = BorderRadius.circular(radius);
    return DecoratedBox(
      decoration: BoxDecoration(
        borderRadius: r,
        // (a short drop: a long one under a circle reads as an oval)
        boxShadow: [BoxShadow(color: p.glassShadow, blurRadius: 16, offset: const Offset(0, 2))],
      ),
      child: ClipRRect(
        borderRadius: r,
        // (Settings > Glass off: solid buttons, no blur)
        child: Palette.glassOn
            // (grouped: the main screen's glass shares one blur of what's behind it, main.dart's
            // BackdropGroup, rather than each button blurring the screen again every frame)
            ? BackdropFilter.grouped(
                filter: ui.ImageFilter.blur(sigmaX: 14, sigmaY: 14),
                child: CustomPaint(
                  foregroundPainter: _Rim(radius, p, tint != null),
                  child: ColoredBox(color: tint ?? p.glass, child: child),
                ),
              )
            : ColoredBox(color: tint ?? p.glass, child: child),
      ),
    );
  }
}

/// The rim: lit along the top edge, a hairline all round.
class _Rim extends CustomPainter {
  _Rim(this.radius, this.p, this.tinted);
  final double radius;
  final Palette p;
  final bool tinted;
  @override
  void paint(Canvas c, Size s) {
    final rr = RRect.fromRectAndRadius(Offset.zero & s, Radius.circular(radius)).deflate(0.5);
    c.drawRRect(
      rr,
      Paint()
        ..style = PaintingStyle.stroke
        ..strokeWidth = 1
        ..shader = ui.Gradient.linear(
          Offset.zero,
          Offset(0, s.height),
          [p.glassRim, p.glassEdge, p.glassRim.withValues(alpha: p.light ? 0.3 : 0.08)],
          [0, 0.5, 1],
        ),
    );
    if (!tinted) return;
    c.drawRRect(
      rr,
      Paint()
        ..style = PaintingStyle.stroke
        ..strokeWidth = 1
        ..color = const Color(0x33FFFFFF),
    );
  }

  @override
  bool shouldRepaint(_Rim o) => o.p != p || o.radius != radius || o.tinted != tinted;
}

/// A round glass button with a symbol (48 dp).
class GlassButton extends StatelessWidget {
  const GlassButton({super.key, required this.icon, required this.onTap, this.tinted = false, this.size = 48, this.iconSize = 22, this.color, this.semantic});
  final IconData icon;
  final VoidCallback? onTap;
  final bool tinted;
  final double size, iconSize;
  final Color? color;
  final String? semantic;

  @override
  Widget build(BuildContext context) {
    final p = Palette.of(context);
    return Semantics(
      button: true,
      label: semantic,
      child: Glass(
        radius: size / 2,
        tint: tinted ? p.accent : null,
        child: Pressable(
          onTap: onTap,
          child: SizedBox.square(
            dimension: size,
            child: glyphIcon(icon, iconSize, tinted ? Colors.white : (color ?? (onTap == null ? p.tertiary : p.label))),
          ),
        ),
      ),
    );
  }
}

/// A glass capsule of symbol buttons (48 dp each), as iOS 26 groups a toolbar's buttons.
class GlassGroup extends StatelessWidget {
  const GlassGroup({super.key, required this.items, this.height = 48});
  final List<GlassItem> items;
  final double height;

  @override
  Widget build(BuildContext context) {
    final p = Palette.of(context);
    return Glass(
      radius: height / 2,
      child: Padding(
        padding: const EdgeInsets.symmetric(horizontal: 4),
        child: Row(
          mainAxisSize: MainAxisSize.min,
          children: [
            for (final i in items)
              Builder(
                builder: (ctx) => Pressable(
                  onTap: i.onTap == null ? null : () => i.onTap!(ctx),
                  child: SizedBox(
                    width: i.label == null ? height : null,
                    height: height,
                    child: Padding(
                      padding: EdgeInsets.symmetric(horizontal: i.label == null ? 0 : 14),
                      child: Center(
                        child: i.label != null
                            ? Text(i.label!, style: Type.text(17, i.onTap == null ? p.tertiary : (i.color ?? p.label), weight: FontWeight.w500))
                            : glyphIcon(i.icon!, 22, i.onTap == null ? p.tertiary : (i.color ?? p.label)),
                      ),
                    ),
                  ),
                ),
              ),
          ],
        ),
      ),
    );
  }
}

class GlassItem {
  const GlassItem({this.icon, this.label, required this.onTap, this.color});
  final IconData? icon;
  final String? label;

  /// Gets the button's context (to anchor a menu under it).
  final void Function(BuildContext context)? onTap;
  final Color? color;
}

/// Dims while pressed (iOS's buttons), then taps.
class Pressable extends StatefulWidget {
  const Pressable({super.key, required this.child, required this.onTap, this.onLongPress});
  final Widget child;
  final VoidCallback? onTap, onLongPress;
  @override
  State<Pressable> createState() => _PressableState();
}

class _PressableState extends State<Pressable> {
  bool _down = false;
  @override
  Widget build(BuildContext context) => GestureDetector(
    behavior: HitTestBehavior.opaque,
    onTapDown: widget.onTap == null ? null : (_) => setState(() => _down = true),
    onTapCancel: () => setState(() => _down = false),
    onTapUp: widget.onTap == null ? null : (_) => setState(() => _down = false),
    onTap: widget.onTap,
    onLongPress: widget.onLongPress,
    child: AnimatedOpacity(
      opacity: _down ? 0.45 : 1,
      duration: Duration(milliseconds: _down ? 0 : 160),
      child: widget.child,
    ),
  );
}

/// Where a widget is on screen (to anchor a menu to it).
Rect globalRect(BuildContext context) {
  final box = context.findRenderObject() as RenderBox;
  return box.localToGlobal(Offset.zero) & box.size;
}

/// A toolbar's row: `children` as a Row (a Spacer between its ends) when they fit; on a screen
/// too narrow for them (a square unfolded inner screen, Undo and Redo showing), it scrolls
/// sideways instead of squeezing its buttons or pushing them off the edge.
class ToolbarRow extends StatelessWidget {
  const ToolbarRow({super.key, required this.children});
  final List<Widget> children;

  @override
  Widget build(BuildContext context) => LayoutBuilder(
    // clipped at its ends (what scrolls off doesn't draw over the next column) but not above
    // and below, where the buttons' shadows fall
    builder: (ctx, box) => ClipRect(
      clipper: const _SidesOnly(),
      child: SingleChildScrollView(
        scrollDirection: Axis.horizontal,
        // (iOS's scrolling: momentum and the rubber band, only when there's more than fits)
        physics: const BouncingScrollPhysics(),
        clipBehavior: Clip.none,
        child: ConstrainedBox(
          constraints: BoxConstraints(minWidth: box.maxWidth),
          child: IntrinsicWidth(child: Row(children: children)),
        ),
      ),
    ),
  );
}

class _SidesOnly extends CustomClipper<Rect> {
  const _SidesOnly();
  @override
  Rect getClip(Size size) => Rect.fromLTRB(0, -40, size.width, size.height + 40);
  @override
  bool shouldReclip(_SidesOnly old) => false;
}

/// Toolbar symbols drawn at one optical size: each glyph scaled so its largest side matches the
/// checklist circle's (0.86 of the em), within ±15%, from the glyphs' ink measured in the icon
/// fonts. Share and Table fill more of their box than the circle, the Material markup pen less;
/// unscaled, a bar's icons look mismatched though they're the same point size.
const _optical = <String, double>{
  'CupertinoIcons:62418': 1.150, // chevron_left
  'CupertinoIcons:62570': 1.089, // ellipsis
  'CupertinoIcons:62666': 0.905, // share
  'CupertinoIcons:63282': 0.958, // paperclip
  'CupertinoIcons:63580': 0.908, // textformat
  'CupertinoIcons:63556': 0.869, // table
  'CupertinoIcons:62342': 1.075, // expand
  'CupertinoIcons:62333': 1.024, // collapse
  'CupertinoIcons:62788': 1.085, // undo
  'CupertinoIcons:62793': 1.085, // redo
  'CupertinoIcons:63446': 0.869, // sidebar_left
  'CupertinoIcons:63096': 0.860, // folder_badge_plus
  'CupertinoIcons:62629': 1.075, // search
  'CupertinoIcons:63227': 0.950, // lock_open_fill
  'CupertinoIcons:62516': 0.894, // folder
  'CupertinoIcons:63492': 1.113, // square_grid_2x2
  'CupertinoIcons:62660': 0.920, // trash
  'CupertinoIcons:62419': 1.150, // chevron_right
};

double opticalSize(IconData? icon, double size) => icon == null ? size : size * (_optical['${icon.fontFamily}:${icon.codePoint}'] ?? 1);

/// Markup (Apple's pencil.tip.crop.circle), drawn as the desktop Notes draws it (nt-app
/// icons.rs Glyph::Markup) rather than taken from a font: Material's pen was heavier than the
/// Cupertino glyphs beside it. Used as an IconData, so the bars can hold it like any symbol.
const markupIcon = IconData(0xF8FF, fontFamily: 'BrambleGlyphs');

/// A toolbar symbol at the bars' one optical size (the markup pen drawn, the rest from fonts).
Widget glyphIcon(IconData icon, double size, Color color) =>
    icon == markupIcon ? MarkupGlyph(size: size, color: color) : Icon(icon, size: opticalSize(icon, size), color: color);

class MarkupGlyph extends StatelessWidget {
  const MarkupGlyph({super.key, required this.size, required this.color});
  final double size;
  final Color color;
  // (its own size, centred, in whatever box holds it: a tight one, like the keyboard bar's 42 dp
  // circles, would otherwise stretch it, where an Icon keeps its size)
  @override
  Widget build(BuildContext context) => Align(
    widthFactor: 1,
    heightFactor: 1,
    child: SizedBox.square(
      dimension: size,
      child: CustomPaint(painter: _Markup(color)),
    ),
  );
}

class _Markup extends CustomPainter {
  _Markup(this.color);
  final Color color;

  @override
  void paint(Canvas c, Size sz) {
    final size = sz.shortestSide;
    // the Cupertino glyphs' stroke (1.5 dp at 22), and the ring's outer edge at 0.86 of the box
    // as theirs; the drawing is the desktop's 24-unit grid around (12, 12)
    final w = 1.5 * size / 22;
    final u = (0.86 * size - w) / 17;
    final o = sz.center(Offset.zero);
    Offset at(double x, double y) => o + Offset((x - 12) * u, (y - 12) * u);
    final stroke = Paint()
      ..style = PaintingStyle.stroke
      ..strokeWidth = w
      ..strokeCap = StrokeCap.round
      ..strokeJoin = StrokeJoin.round
      ..color = color;
    c.drawCircle(o, 8.5 * u, stroke);
    // the nib: its sides ease from the circle's foot into a rounded point, a band across it
    final l0 = at(8.3, 20.3), lc = at(9.9, 11), tip = at(12, 6.5), rc = at(14.1, 11), r0 = at(15.7, 20.3);
    c.drawPath(
      Path()
        ..moveTo(l0.dx, l0.dy)
        ..quadraticBezierTo(lc.dx, lc.dy, tip.dx, tip.dy)
        ..quadraticBezierTo(rc.dx, rc.dy, r0.dx, r0.dy),
      stroke,
    );
    c.drawLine(at(9.6, 15), at(14.4, 15), stroke);
    // the point, inked
    final a = at(12, 6.6), b = at(13.15, 10.2), d = at(10.85, 10.2);
    c.drawPath(
      Path()
        ..moveTo(a.dx, a.dy)
        ..lineTo(b.dx, b.dy)
        ..lineTo(d.dx, d.dy)
        ..close(),
      Paint()
        ..color = color
        ..style = PaintingStyle.fill,
    );
  }

  @override
  bool shouldRepaint(_Markup o) => o.color != color;
}
