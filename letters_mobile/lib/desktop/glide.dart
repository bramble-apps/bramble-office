// Windows that glide (as ChordPro presenter's floating panels): dragged by their title bar,
// let go with a flick they carry on and ease to a stop on a spring, stay on screen, and settle
// against an edge when let go close to it.
import 'dart:math' as math;

import 'package:flutter/physics.dart';
import 'package:flutter/widgets.dart';

/// What a window's title bar moves it with (DWindow, the Colors window).
class GlideHandle extends InheritedWidget {
  const GlideHandle({super.key, required this.drag, required this.release, required super.child});
  final void Function(Offset delta) drag;
  final void Function(Velocity v) release;
  static GlideHandle? of(BuildContext c) => c.dependOnInheritedWidgetOfExactType<GlideHandle>();
  @override
  bool updateShouldNotify(GlideHandle old) => false;
}

class Glide extends StatefulWidget {
  const Glide({super.key, required this.child, required this.initial, this.margin = 8});

  /// Where it opens: its top left for the area and its own size.
  final Offset Function(Size area, Size self) initial;
  final double margin;
  final Widget child;
  @override
  State<Glide> createState() => _GlideState();
}

class _GlideState extends State<Glide> with TickerProviderStateMixin {
  late final _x = AnimationController.unbounded(vsync: this);
  late final _y = AnimationController.unbounded(vsync: this);
  final _key = GlobalKey();
  Size _area = Size.zero;
  Size? _self;

  static const snapDistance = 40.0;

  @override
  void dispose() {
    _x.dispose();
    _y.dispose();
    super.dispose();
  }

  (Offset, Offset) get _bounds {
    final s = _self ?? Size.zero, m = widget.margin;
    return (Offset(m, m), Offset(math.max(m, _area.width - s.width - m), math.max(m, _area.height - s.height - m)));
  }

  Offset _clamp(Offset p) {
    final (lo, hi) = _bounds;
    return Offset(p.dx.clamp(lo.dx, hi.dx), p.dy.clamp(lo.dy, hi.dy));
  }

  void _drag(Offset d) {
    _x.stop();
    _y.stop();
    final p = _clamp(Offset(_x.value, _y.value) + d);
    _x.value = p.dx;
    _y.value = p.dy;
  }

  void _release(Velocity velocity) {
    final v = velocity.pixelsPerSecond;
    final speed = v.distance;
    final capped = speed > 4000 ? v * (4000 / speed) : v;
    final here = Offset(_x.value, _y.value);
    var t = _clamp(here + capped * 0.16);
    final (lo, hi) = _bounds;
    double snap(double t, double a, double b) => t - a < snapDistance ? a : (b - t < snapDistance ? b : t);
    t = Offset(snap(t.dx, lo.dx, hi.dx), snap(t.dy, lo.dy, hi.dy));
    final spring = SpringDescription.withDampingRatio(mass: 1, stiffness: 170, ratio: 0.9);
    _x.animateWith(SpringSimulation(spring, here.dx, t.dx, capped.dx));
    _y.animateWith(SpringSimulation(spring, here.dy, t.dy, capped.dy));
  }

  /// Its size once laid out: placed where it opens (then kept on screen as the area changes).
  void _measure() {
    final box = _key.currentContext?.findRenderObject() as RenderBox?;
    if (box == null || !box.hasSize || !mounted) return;
    final first = _self == null;
    if (box.size == _self) return;
    setState(() => _self = box.size);
    final p = first ? widget.initial(_area, box.size) : Offset(_x.value, _y.value);
    final c = _clamp(p);
    _x.value = c.dx;
    _y.value = c.dy;
  }

  @override
  Widget build(BuildContext context) => LayoutBuilder(builder: (ctx, box) {
        final area = Size(box.maxWidth, box.maxHeight);
        if (area != _area) {
          _area = area;
          if (_self != null && !_x.isAnimating) {
            final c = _clamp(Offset(_x.value, _y.value));
            _x.value = c.dx;
            _y.value = c.dy;
          }
        }
        WidgetsBinding.instance.addPostFrameCallback((_) => _measure());
        return AnimatedBuilder(
          animation: Listenable.merge([_x, _y]),
          builder: (ctx, child) => Stack(children: [
            Positioned(left: _x.value, top: _y.value, child: Opacity(opacity: _self == null ? 0 : 1, child: child)),
          ]),
          child: KeyedSubtree(key: _key, child: GlideHandle(drag: _drag, release: _release, child: widget.child)),
        );
      });
}
