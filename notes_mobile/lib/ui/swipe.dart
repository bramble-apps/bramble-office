// A row's swipe actions (iOS lists): the row follows the finger 1:1 and its buttons grow out
// from under it (circles scale up, then their names fade in); let go past halfway, or flick, to
// keep them open, which settles on a spring carrying the finger's speed. Drag far enough and the
// outermost action (Delete, Pin) takes the whole row with a tap of haptics: let go to run it.
import 'dart:math' as math;

import 'package:flutter/gestures.dart';
import 'package:flutter/material.dart';
import 'package:flutter/physics.dart';
import 'package:flutter/services.dart';

import '../theme.dart';

class SwipeAction {
  const SwipeAction(this.icon, this.color, this.onTap, {this.label});
  final IconData icon;
  final Color color;
  final VoidCallback onTap;
  final String? label;
}

class SwipeRow extends StatefulWidget {
  const SwipeRow({super.key, required this.child, this.leading = const [], this.trailing = const []});
  final Widget child;
  final List<SwipeAction> leading, trailing;
  @override
  State<SwipeRow> createState() => _SwipeRowState();
}

class _SwipeRowState extends State<SwipeRow> with TickerProviderStateMixin {
  late final _x = AnimationController.unbounded(vsync: this);

  /// 0 → 1 as the outermost action takes the row (a full swipe).
  late final _full = AnimationController(vsync: this, duration: const Duration(milliseconds: 220));
  double _width = 400;
  bool _committed = false;
  static const _button = 74.0;

  /// iOS's snap: about a 0.4 s response, just short of critical damping.
  static final _spring = SpringDescription.withDampingRatio(mass: 1, stiffness: 260, ratio: 0.92);

  /// The row open somewhere else closes when another opens.
  static _SwipeRowState? _open;

  bool _watching = false;

  /// While open, a touch anywhere else closes it (iOS), so the next swipe starts from shut.
  void _watch(bool on) {
    if (on == _watching) return;
    _watching = on;
    if (on) {
      GestureBinding.instance.pointerRouter.addGlobalRoute(_outside);
    } else {
      GestureBinding.instance.pointerRouter.removeGlobalRoute(_outside);
    }
  }

  void _outside(PointerEvent e) {
    if (e is! PointerDownEvent || !mounted) return;
    final box = context.findRenderObject() as RenderBox?;
    if (box == null || !box.hasSize) return;
    final local = box.globalToLocal(e.position);
    if (!(Offset.zero & box.size).contains(local)) _close();
  }

  @override
  void dispose() {
    _watch(false);
    if (_open == this) _open = null;
    _x.dispose();
    _full.dispose();
    super.dispose();
  }

  /// Springs to `to` and lands exactly there: a spring stops within a hair of its target
  /// (1e-5 to 1e-4), and a row left a hair open took every tap as "close" and never opened.
  void _settle(double to, [double velocity = 0]) => _x.animateWith(SpringSimulation(_spring, _x.value, to, velocity)).whenCompleteOrCancel(() {
    if (mounted && !_x.isAnimating && (_x.value - to).abs() < 1) _x.value = to;
  });

  /// Open by more than a hair (a row at rest is shut, whatever its last spring left).
  static bool _isOpen(double v) => v.abs() > 0.5;

  void _close() {
    _watch(false);
    _commit(false);
    _settle(0);
  }

  void _commit(bool on) {
    if (on == _committed) return;
    _committed = on;
    if (on) HapticFeedback.mediumImpact();
    _full.animateTo(on ? 1 : 0, curve: Curves.easeOutCubic);
  }

  void _start(DragStartDetails d) {
    _x.stop();
    if (_open != this) {
      _open?._close();
      _open = this;
    }
  }

  void _update(DragUpdateDetails d) {
    var v = _x.value + d.delta.dx;
    if (widget.leading.isEmpty) v = math.min(0, v);
    if (widget.trailing.isEmpty) v = math.max(0, v);
    v = v.clamp(-_width, _width);
    _x.value = v;
    final acts = v > 0 ? widget.leading : widget.trailing;
    if (acts.isEmpty) return;
    // (in and out with a margin, so it doesn't flicker on the line)
    final open = acts.length * _button;
    _commit(_committed ? v.abs() > _width * 0.5 : v.abs() > math.max(open + 40, _width * 0.58));
  }

  void _end(DragEndDetails d) {
    final v = _x.value, speed = d.primaryVelocity ?? 0;
    final acts = v > 0 ? widget.leading : widget.trailing;
    final open = acts.length * _button;
    if (_committed && acts.isNotEmpty) {
      // all the way: the outermost action
      _x.animateWith(SpringSimulation(_spring, v, v.sign * _width * 1.05, speed)).whenComplete(() {
        acts.first.onTap();
        if (mounted) {
          _committed = false;
          _full.value = 0;
          _x.value = 0;
        }
      });
      return;
    }
    // a flick decides by its direction; a slow release by how far it's open
    final keep = speed.abs() > 300 ? speed.sign == v.sign : v.abs() > open / 2;
    final stay = keep && acts.isNotEmpty;
    _watch(stay);
    _settle(stay ? v.sign * open : 0, speed);
  }

  @override
  Widget build(BuildContext context) => LayoutBuilder(
    builder: (ctx, box) {
      _width = box.maxWidth;
      return GestureDetector(
        onHorizontalDragStart: _start,
        onHorizontalDragUpdate: _update,
        onHorizontalDragEnd: _end,
        child: AnimatedBuilder(
          animation: Listenable.merge([_x, _full]),
          builder: (ctx, child) {
            final v = _isOpen(_x.value) ? _x.value : 0.0;
            final acts = v > 0 ? widget.leading : widget.trailing;
            final n = acts.length;
            final f = _full.value;
            // each button's share; the outermost takes the rest as the full swipe lands (the row
            // flies a little past its edge then: the buttons stop at the edge)
            final room = math.min(v.abs(), _width);
            double slot(int k) => k == 0 ? room / n + (room - room / n) * f : room / n * (1 - f);
            final shown = [for (var k = 0; k < n; k++) (acts[k], slot(k), k == 0)];
            return Stack(
              children: [
                if (v != 0 && n > 0)
                  Positioned.fill(
                    child: Row(
                      mainAxisAlignment: v > 0 ? MainAxisAlignment.start : MainAxisAlignment.end,
                      children: [
                        for (final (a, w, outer) in (v > 0 ? shown : shown.reversed))
                          _ActionButton(
                            action: a,
                            width: w,
                            // the full swipe's button keeps by the row's edge
                            align: outer ? f : 0,
                            leading: v > 0,
                            onTap: () {
                              _close();
                              a.onTap();
                            },
                          ),
                      ],
                    ),
                  ),
                Transform.translate(
                  offset: Offset(v, 0),
                  child: GestureDetector(
                    onTap: v != 0 ? _close : null,
                    behavior: v != 0 ? HitTestBehavior.opaque : HitTestBehavior.deferToChild,
                    child: AbsorbPointer(absorbing: v != 0, child: child),
                  ),
                ),
              ],
            );
          },
          child: RepaintBoundary(child: widget.child),
        ),
      );
    },
  );
}

/// iOS 26's swipe button: a coloured circle with its symbol, its name under it. Both grow with
/// the space the drag has opened (the circle scales, the name fades in after it).
class _ActionButton extends StatelessWidget {
  const _ActionButton({required this.action, required this.width, required this.align, required this.leading, required this.onTap});
  final SwipeAction action;
  final double width;

  /// 0: centred in its space; 1: against the row's edge.
  final double align;
  final bool leading;
  final VoidCallback onTap;
  static const _d = 50.0;

  @override
  Widget build(BuildContext context) {
    final p = Palette.of(context);
    final grow = ((width - 14) / _d).clamp(0.0, 1.0);
    final name = ((width - 56) / 18).clamp(0.0, 1.0);
    return GestureDetector(
      onTap: onTap,
      child: SizedBox(
        width: width,
        child: grow == 0
            ? null
            : OverflowBox(
                maxWidth: math.max(width, 74),
                alignment: Alignment.lerp(Alignment.center, leading ? Alignment.centerRight : Alignment.centerLeft, align)!,
                child: Padding(
                  padding: EdgeInsets.symmetric(horizontal: 12 * align),
                  child: Column(
                    mainAxisAlignment: MainAxisAlignment.center,
                    mainAxisSize: MainAxisSize.min,
                    children: [
                      Transform.scale(
                        scale: Curves.easeOut.transform(grow),
                        child: Opacity(
                          opacity: grow,
                          child: Container(
                            width: _d,
                            height: _d,
                            decoration: BoxDecoration(color: action.color, shape: BoxShape.circle),
                            child: Icon(action.icon, color: Colors.white, size: 23),
                          ),
                        ),
                      ),
                      if (action.label != null)
                        Opacity(
                          opacity: name,
                          child: Transform.scale(
                            scale: 0.85 + 0.15 * name,
                            child: Padding(
                              padding: const EdgeInsets.only(top: 3),
                              child: Text(action.label!, maxLines: 1, style: Type.caption(p.label)),
                            ),
                          ),
                        ),
                    ],
                  ),
                ),
              ),
      ),
    );
  }
}
