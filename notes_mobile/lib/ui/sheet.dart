// Sheets from the bottom, as Letters' (desktop/inspector.dart InspectorSheet, the Bible
// viewer's dock motion, no glass): it opens to half height (260 ms, easing out), drags up to
// full height or down to close, a flick carrying it to the next resting place; dragging
// anywhere but a list moves it, and scrolling a list up first raises it to full height. On a
// wide screen (iPad) the sheet is a centred card instead, as iPadOS shows form sheets.
import 'dart:math' as math;

import 'package:flutter/material.dart';

import '../theme.dart';
import 'glass.dart';

const kSheetRadius = 18.0;

/// Shows `builder`'s contents in a sheet; `title` and the close button head it. `fit`: rests at
/// its contents' height (short sheets: the Aa format sheet) instead of half the screen.
Future<T?> showSheet<T>(
  BuildContext context, {
  required String title,
  required WidgetBuilder builder,
  bool fit = false,
  double? height,
  List<Widget> actions = const [],
  bool dim = true,
}) {
  return Navigator.of(context, rootNavigator: true).push<T>(
    PageRouteBuilder<T>(
      opaque: false,
      barrierDismissible: true,
      barrierColor: dim ? const Color(0x33000000) : Colors.transparent,
      barrierLabel: 'Close',
      transitionDuration: const Duration(milliseconds: 260),
      reverseTransitionDuration: const Duration(milliseconds: 220),
      pageBuilder: (ctx, a, _) => _SheetHost(title: title, builder: builder, fit: fit, height: height, actions: actions, animation: a),
    ),
  );
}

class _SheetHost extends StatelessWidget {
  const _SheetHost({required this.title, required this.builder, required this.fit, required this.animation, this.height, this.actions = const []});
  final String title;
  final WidgetBuilder builder;
  final bool fit;
  final double? height;
  final Animation<double> animation;
  final List<Widget> actions;

  @override
  Widget build(BuildContext context) {
    final mq = MediaQuery.of(context);
    final wide = mq.size.width >= 700 && mq.size.height >= 600;
    final p = Palette.of(context);
    if (wide) {
      // iPad: a form sheet in the middle
      return FadeTransition(
        opacity: CurvedAnimation(parent: animation, curve: Curves.easeOut),
        child: ScaleTransition(
          scale: Tween(begin: 0.96, end: 1.0).animate(CurvedAnimation(parent: animation, curve: Curves.easeOutCubic)),
          child: Center(
            child: Padding(
              padding: EdgeInsets.only(bottom: mq.viewInsets.bottom),
              child: ConstrainedBox(
                constraints: BoxConstraints(maxWidth: 540, maxHeight: math.min(height ?? 620, mq.size.height - 80)),
                child: Material(
                  color: p.sheet,
                  borderRadius: BorderRadius.circular(kSheetRadius + 6),
                  clipBehavior: Clip.antiAlias,
                  elevation: 0,
                  child: _SheetBody(title: title, actions: actions, fit: fit, child: builder(context)),
                ),
              ),
            ),
          ),
        ),
      );
    }
    return DetentSheet(
      fit: fit,
      animation: animation,
      child: _SheetBody(title: title, actions: actions, fit: fit, child: builder(context)),
    );
  }
}

class _SheetBody extends StatelessWidget {
  const _SheetBody({required this.title, required this.child, required this.fit, this.actions = const []});
  final String title;
  final Widget child;
  final bool fit;
  final List<Widget> actions;

  @override
  Widget build(BuildContext context) {
    final p = Palette.of(context);
    final head = SizedBox(
      height: 56,
      child: Row(
        children: [
          const SizedBox(width: 20),
          ...actions,
          Expanded(
            child: Text(
              title,
              textAlign: actions.isEmpty ? TextAlign.left : TextAlign.center,
              maxLines: 1,
              overflow: TextOverflow.ellipsis,
              style: Type.text(actions.isEmpty ? 20 : 17, p.label, weight: FontWeight.w700),
            ),
          ),
          Pressable(
            onTap: () => Navigator.of(context).maybePop(),
            child: SizedBox.square(
              dimension: 48,
              child: Center(
                child: Container(
                  width: 36,
                  height: 36,
                  decoration: BoxDecoration(color: p.fill, shape: BoxShape.circle),
                  child: Icon(Icons.close_rounded, size: 22, color: p.secondary),
                ),
              ),
            ),
          ),
          const SizedBox(width: 4),
        ],
      ),
    );
    return Material(
      type: MaterialType.transparency,
      child: Column(
        mainAxisSize: fit ? MainAxisSize.min : MainAxisSize.max,
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          head,
          if (fit) child else Expanded(child: child),
        ],
      ),
    );
  }
}

/// The phone's sheet: rests at half height (or its contents' height with `fit`), drags to full
/// height, flicks or drags down shut.
class DetentSheet extends StatefulWidget {
  const DetentSheet({super.key, required this.child, required this.animation, this.fit = false});
  final Widget child;
  final Animation<double> animation;
  final bool fit;
  @override
  State<DetentSheet> createState() => _DetentSheetState();
}

class _DetentSheetState extends State<DetentSheet> with SingleTickerProviderStateMixin {
  late final _h = AnimationController.unbounded(vsync: this);
  double _available = 600;
  double? _fitHeight;
  bool _started = false, _closing = false;
  final _measure = GlobalKey();

  double get _medium => widget.fit && _fitHeight != null ? math.min(_fitHeight!, _large) : math.max(200, _available * 0.5);
  double get _large => _available - 8;

  @override
  void initState() {
    super.initState();
    widget.animation.addStatusListener(_routeStatus);
  }

  void _routeStatus(AnimationStatus s) {
    // the route closing (a tap outside, the close button): slide down
    if (s == AnimationStatus.reverse && !_closing) {
      _closing = true;
      _h.animateTo(0, duration: const Duration(milliseconds: 220), curve: Curves.easeOutCubic);
    }
  }

  @override
  void dispose() {
    widget.animation.removeStatusListener(_routeStatus);
    _h.dispose();
    super.dispose();
  }

  void _animateTo(double t) => _h.animateTo(t, duration: const Duration(milliseconds: 260), curve: Curves.easeOutCubic);

  void _drag(DragUpdateDetails d) => _h.value = (_h.value - d.delta.dy).clamp(0.0, _large);

  void _dragEnd(DragEndDetails d) {
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
    if (widget.fit && target == _large && _medium >= _large - 1) target = _medium;
    if (target == 0) {
      _closing = true;
      _h.animateTo(0, duration: const Duration(milliseconds: 220), curve: Curves.easeOutCubic).whenComplete(() {
        if (mounted) Navigator.of(context).maybePop();
      });
    } else {
      _animateTo(target);
    }
  }

  @override
  Widget build(BuildContext context) {
    final p = Palette.of(context);
    final mq = MediaQuery.of(context);
    return Padding(
      padding: EdgeInsets.only(top: mq.padding.top, bottom: mq.viewInsets.bottom),
      child: LayoutBuilder(
        builder: (ctx, box) {
          _available = box.maxHeight;
          if (!_started) {
            _started = true;
            WidgetsBinding.instance.addPostFrameCallback((_) {
              final b = _measure.currentContext?.findRenderObject() as RenderBox?;
              if (widget.fit && b != null && b.hasSize) _fitHeight = b.size.height;
              _animateTo(_medium);
            });
          }
          final sheet = Container(
            decoration: BoxDecoration(
              color: p.sheet,
              borderRadius: const BorderRadius.vertical(top: Radius.circular(kSheetRadius)),
              boxShadow: [BoxShadow(color: Color(p.light ? 0x22000000 : 0x66000000), blurRadius: 16, offset: const Offset(0, -2))],
            ),
            child: GestureDetector(
              behavior: HitTestBehavior.translucent,
              onVerticalDragUpdate: _drag,
              onVerticalDragEnd: _dragEnd,
              child: Column(
                mainAxisSize: widget.fit ? MainAxisSize.min : MainAxisSize.max,
                crossAxisAlignment: CrossAxisAlignment.stretch,
                children: [
                  // the grabber
                  SizedBox(
                    height: 14,
                    child: Center(
                      child: Container(
                        width: 36,
                        height: 5,
                        decoration: BoxDecoration(color: p.tertiary, borderRadius: BorderRadius.circular(2.5)),
                      ),
                    ),
                  ),
                  if (widget.fit)
                    Padding(
                      padding: EdgeInsets.only(bottom: mq.padding.bottom + 8),
                      child: widget.child,
                    )
                  else
                    Expanded(
                      child: NotificationListener<ScrollNotification>(
                        // a list scrolled up while the sheet rests lower: the sheet rises first
                        onNotification: (n) {
                          if (n is ScrollUpdateNotification && n.dragDetails != null && (n.scrollDelta ?? 0) > 0 && _h.value < _large - 1 && !_h.isAnimating)
                            _animateTo(_large);
                          return false;
                        },
                        child: Padding(
                          padding: EdgeInsets.only(bottom: mq.padding.bottom),
                          child: widget.child,
                        ),
                      ),
                    ),
                ],
              ),
            ),
          );
          return Stack(
            children: [
              // (measured once at its own height, for `fit`)
              if (widget.fit && _fitHeight == null)
                Offstage(
                  child: UnconstrainedBox(
                    constrainedAxis: Axis.horizontal,
                    child: KeyedSubtree(
                      key: _measure,
                      child: SizedBox(width: box.maxWidth, child: sheet),
                    ),
                  ),
                ),
              AnimatedBuilder(
                animation: _h,
                builder: (ctx, child) {
                  final h = _h.value;
                  if (h <= 0.5) return const SizedBox.shrink();
                  return Stack(
                    children: [
                      Positioned(
                        left: 0,
                        right: 0,
                        bottom: 0,
                        height: h,
                        child: ClipRect(
                          child: OverflowBox(
                            alignment: Alignment.topCenter,
                            minHeight: 0,
                            maxHeight: math.max(h, widget.fit ? (_fitHeight ?? 120) : 120),
                            child: child!,
                          ),
                        ),
                      ),
                    ],
                  );
                },
                child: widget.fit ? Align(alignment: Alignment.topCenter, child: sheet) : sheet,
              ),
            ],
          );
        },
      ),
    );
  }
}

/// A grouped card of rows (Settings, sheets), a hairline between rows.
class Card2 extends StatelessWidget {
  const Card2({super.key, required this.children, this.header, this.footer});
  final List<Widget> children;
  final String? header, footer;

  @override
  Widget build(BuildContext context) {
    final p = Palette.of(context);
    return Padding(
      padding: const EdgeInsets.fromLTRB(16, 0, 16, 18),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          if (header != null)
            Padding(
              padding: const EdgeInsets.fromLTRB(16, 4, 16, 7),
              child: Text(header!, style: Type.footnote(p.secondary)),
            ),
          ClipRRect(
            borderRadius: BorderRadius.circular(kSheetRadius - 4),
            child: ColoredBox(
              color: p.sheetCard,
              child: Column(
                children: [
                  for (final (i, c) in children.indexed) ...[
                    if (i > 0)
                      Padding(
                        padding: const EdgeInsets.only(left: 16),
                        child: Container(height: 0.5, color: p.separator),
                      ),
                    c,
                  ],
                ],
              ),
            ),
          ),
          if (footer != null)
            Padding(
              padding: const EdgeInsets.fromLTRB(16, 7, 16, 0),
              child: Text(footer!, style: Type.footnote(p.secondary)),
            ),
        ],
      ),
    );
  }
}

/// A row in a card: a label, an optional value or control at the right, a chevron when it opens something.
class CardRow extends StatelessWidget {
  const CardRow({super.key, required this.label, this.value, this.trailing, this.onTap, this.chevron = false, this.leading, this.color, this.checked = false});
  final String label;
  final String? value;
  final Widget? trailing, leading;
  final VoidCallback? onTap;
  final bool chevron, checked;
  final Color? color;

  @override
  Widget build(BuildContext context) {
    final p = Palette.of(context);
    return Pressable(
      onTap: onTap,
      child: ConstrainedBox(
        constraints: const BoxConstraints(minHeight: 50),
        child: Padding(
          padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 6),
          child: Row(
            children: [
              if (leading != null) ...[leading!, const SizedBox(width: 12)],
              Expanded(child: Text(label, style: Type.body(color ?? p.label))),
              if (value != null)
                Padding(
                  padding: const EdgeInsets.only(left: 8),
                  child: Text(value!, style: Type.body(p.secondary)),
                ),
              ?trailing,
              if (checked) Icon(Icons.check_rounded, size: 22, color: p.accent),
              if (chevron)
                Padding(
                  padding: const EdgeInsets.only(left: 6),
                  child: Icon(Icons.chevron_right_rounded, size: 22, color: p.tertiary),
                ),
            ],
          ),
        ),
      ),
    );
  }
}
