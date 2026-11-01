// The inspector's frames shared with the windows (Letters' inspector.dart): the popover frame,
// the radio button, and the phone layout's bottom sheet.
import 'dart:math' as math;

import 'package:flutter/material.dart';

import 'touch_menu.dart';
import 'widgets.dart';

/// A popover under `anchor` (egui's popup frame), holding `child`.
/// `fit`: too tall for the room, it's drawn smaller to fit rather than scrolling.
Future<T?> showDesktopPopover<T>(BuildContext context, Rect anchor, {required double width, required Widget Function(BuildContext ctx) child, double? maxHeight, bool centered = false, bool fit = false}) {
  final w = width;
  final look = LookScope.of(context);
  // (in the scaled overlay's points: Settings > Scaling)
  anchor = inOverlay(context, anchor);
  return showDialog<T>(
    context: context,
    barrierColor: Colors.transparent,
    // (placed in the whole screen's points, the system bars allowed for below)
    useSafeArea: false,
    builder: (ctx) {
      final mq = MediaQuery.of(ctx);
      final screen = mq.size;
      // (`width` with the desktop's 6-point margins; the touch frame's are 10 inside a half-point rim)
      final width = w + 9;
      // (clear of the system's navigation bar)
      final room = screen.height - mq.padding.bottom - anchor.bottom - 12;
      final above = room < 220 && anchor.top - mq.padding.top > room;
      return Stack(children: [
        Positioned(
          left: centered ? centeredX(anchor, width, screen.width, 8, math.max(8.0, screen.width - width - 8)) : anchor.left.clamp(8.0, math.max(8.0, screen.width - width - 8)),
          top: above ? null : anchor.bottom + 2,
          bottom: above ? screen.height - anchor.top + 2 : null,
          width: width,
          child: ConstrainedBox(
            constraints: BoxConstraints(maxHeight: maxHeight ?? (above ? anchor.top - mq.padding.top - 12 : room)),
            child: LookScope(
              look: look,
              // (framed like the touch menus: large corners, a soft shadow)
              child: Material(
                type: MaterialType.transparency,
                child: () {
                  Widget panel(Widget content) => Container(
                        decoration: TouchMenu.panel(look),
                        child: ClipRRect(borderRadius: BorderRadius.circular(TouchMenu.radius), child: Padding(padding: const EdgeInsets.all(10), child: content)),
                      );
                  // the whole panel smaller when it doesn't fit (its full size laid out, then shrunk)
                  if (fit) return FittedBox(fit: BoxFit.scaleDown, alignment: Alignment.topCenter, child: SizedBox(width: width, child: panel(Builder(builder: (inner) => child(inner)))));
                  return panel(SingleChildScrollView(child: Builder(builder: (inner) => child(inner))));
                }(),
              ),
            ),
          ),
        ),
      ]);
    },
  );
}

Rect rectOf(BuildContext context) {
  final box = context.findRenderObject() as RenderBox;
  return box.localToGlobal(Offset.zero) & box.size;
}

/// The phone layout's inspector: a sheet from the bottom with the Bible viewer's dock motion
/// (no glass): it opens to half height (260 ms, easing out), drags up to full height or down
/// to close, a flick carrying it to the next resting place; dragging anywhere but a list moves
/// it, and scrolling a list up first raises it to full height.
class InspectorSheet extends StatefulWidget {
  const InspectorSheet({super.key, required this.open, required this.onClose, required this.child});
  final bool open;

  /// Dragged (or flicked) down shut.
  final VoidCallback onClose;
  final Widget child;
  @override
  State<InspectorSheet> createState() => _InspectorSheetState();
}

class _InspectorSheetState extends State<InspectorSheet> with SingleTickerProviderStateMixin {
  late final _h = AnimationController.unbounded(vsync: this)..addStatusListener(_settled);
  double _available = 600;
  bool _started = false;

  /// While it's dragged or moving: the height it's laid out at (full), so only a translation
  /// changes each frame (no layout of what's in it); null at rest (laid out at its height).
  double? _layoutH;
  bool _dragging = false;

  void _moving() {
    if (_layoutH == null) setState(() => _layoutH = _large);
  }

  void _settled([AnimationStatus? s]) {
    if (!_dragging && !_h.isAnimating && _layoutH != null && mounted) setState(() => _layoutH = null);
  }

  double get _medium => math.max(200, _available * 0.5);
  double get _large => _available - 8;

  @override
  void didUpdateWidget(InspectorSheet old) {
    super.didUpdateWidget(old);
    if (old.open != widget.open) _animateTo(widget.open ? _medium : 0);
  }

  @override
  void dispose() {
    _h.dispose();
    super.dispose();
  }

  void _animateTo(double t) {
    _moving();
    _h.animateTo(t, duration: const Duration(milliseconds: 260), curve: Curves.easeOutCubic);
  }

  void _dragStart(DragStartDetails d) {
    _dragging = true;
    _moving();
  }

  void _drag(DragUpdateDetails d) => _h.value = (_h.value - d.delta.dy).clamp(0.0, _large);

  void _dragEnd(DragEndDetails d) {
    _dragging = false;
    // (let go where it rests: no animation to end, laid out at its height now)
    WidgetsBinding.instance.addPostFrameCallback((_) => _settled());
    final v = d.primaryVelocity ?? 0;
    final h = _h.value;
    double target;
    if (v > 600) {
      target = h > _medium + 20 ? _medium : 0;
    } else if (v < -600) {
      target = h < _medium - 20 ? _medium : _large;
    } else {
      target = [0.0, _medium, _large].reduce((a, b) => (a - h).abs() < (b - h).abs() ? a : b);
    }
    if (target == 0) {
      _moving();
      _h.animateTo(0, duration: const Duration(milliseconds: 220), curve: Curves.easeOutCubic).whenComplete(() {
        if (mounted && widget.open) widget.onClose();
      });
    } else {
      _animateTo(target);
    }
  }

  @override
  Widget build(BuildContext context) {
    final look = LookScope.of(context);
    return LayoutBuilder(builder: (ctx, box) {
      _available = box.maxHeight;
      if (!_started) {
        _started = true;
        if (widget.open) WidgetsBinding.instance.addPostFrameCallback((_) => _animateTo(_medium));
      }
      return AnimatedBuilder(
        animation: _h,
        builder: (ctx, child) {
          final h = _h.value;
          if (h <= 0.5) return const SizedBox.shrink();
          // moving: laid out full height once and slid (a translation of a cached layer); at
          // rest, laid out at its height (its lists end at the screen's foot)
          final lh = math.max(_layoutH ?? h, h);
          return Stack(children: [
            // (laid out at least 120 tall and clipped, so its first frames opening don't overflow)
            Positioned(
              left: 0,
              right: 0,
              bottom: 0,
              height: lh,
              child: Transform.translate(
                offset: Offset(0, lh - h),
                child: RepaintBoundary(child: ClipRect(child: OverflowBox(alignment: Alignment.topCenter, minHeight: 0, maxHeight: math.max(lh, 120), child: child!))),
              ),
            ),
          ]);
        },
        child: Container(
          decoration: BoxDecoration(
            color: touchBackground(look),
            borderRadius: const BorderRadius.vertical(top: Radius.circular(kTouchRadius)),
            boxShadow: [BoxShadow(color: Color(look.light ? 0x22000000 : 0x66000000), blurRadius: 16, offset: const Offset(0, -2))],
          ),
          // dragging anywhere but a list moves the sheet; the lists scroll as usual
          child: GestureDetector(
            behavior: HitTestBehavior.translucent,
            onVerticalDragStart: _dragStart,
            onVerticalDragUpdate: _drag,
            onVerticalDragEnd: _dragEnd,
            child: Column(crossAxisAlignment: CrossAxisAlignment.stretch, children: [
              // the grabber
              SizedBox(height: 18, child: Center(child: Container(width: 36, height: 5, decoration: BoxDecoration(color: look.weakText, borderRadius: BorderRadius.circular(2.5))))),
              Expanded(
                child: NotificationListener<ScrollNotification>(
                  // a list scrolled up while the sheet rests lower: the sheet rises first
                  onNotification: (n) {
                    if (n is ScrollUpdateNotification && n.dragDetails != null && (n.scrollDelta ?? 0) > 0 && _h.value < _large - 1 && !_h.isAnimating) _animateTo(_large);
                    return false;
                  },
                  child: Padding(padding: const EdgeInsets.fromLTRB(12, 0, 12, 12), child: widget.child),
                ),
              ),
            ]),
          ),
        ),
      );
    });
  }
}
