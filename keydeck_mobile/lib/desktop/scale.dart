// Settings > Scaling (kn_ui scale.rs): the whole interface at the chosen pixels per point,
// as the desktop's zoom factor (175% = 1.75 pixels a point; Default = the system's, on
// Android one and a half times that). Applied
// round the Navigator, so menus, popovers and windows scale with the rest.
import 'dart:io';

import 'package:flutter/rendering.dart';
import 'package:flutter/widgets.dart';

/// The chosen percent (0 = the system's), from the engine's lt_chrome "uiScale".
final uiScale = ValueNotifier<int>(0);

class UiScaled extends StatelessWidget {
  const UiScaled({super.key, required this.child});
  final Widget child;

  @override
  Widget build(BuildContext context) => ValueListenableBuilder<int>(
        valueListenable: uiScale,
        builder: (ctx, pct, _) {
          final mq = MediaQuery.of(ctx);
          // (Default: the system's size; on Android a quarter bigger, the desktop's controls
          // being small for fingers)
          final k = pct <= 0 ? (Platform.isAndroid ? 1.25 : 1.0) : (pct / 100) / mq.devicePixelRatio;
          if ((k - 1).abs() < 1e-3) return child;
          EdgeInsets div(EdgeInsets e) => e / k;
          return LayoutBuilder(builder: (ctx, box) {
            final inner = Size(box.maxWidth / k, box.maxHeight / k);
            return ClipRect(
              child: OverflowBox(
                alignment: Alignment.topLeft,
                minWidth: inner.width,
                maxWidth: inner.width,
                minHeight: inner.height,
                maxHeight: inner.height,
                child: Transform.scale(
                  scale: k,
                  alignment: Alignment.topLeft,
                  child: MediaQuery(
                    data: mq.copyWith(
                      size: inner,
                      devicePixelRatio: mq.devicePixelRatio * k,
                      padding: div(mq.padding),
                      viewPadding: div(mq.viewPadding),
                      viewInsets: div(mq.viewInsets),
                      systemGestureInsets: div(mq.systemGestureInsets),
                    ),
                    child: child,
                  ),
                ),
              ),
            );
          });
        },
      );
}

/// Lays its child out `k` times bigger than the room it has and draws it `k` times smaller (the
/// touch-sized inspector at the size of the rest of the window); touches are mapped through.
class ScaledBox extends SingleChildRenderObjectWidget {
  const ScaledBox({super.key, required this.k, super.child});
  final double k;
  @override
  RenderObject createRenderObject(BuildContext context) => RenderScaled(k);
  @override
  void updateRenderObject(BuildContext context, RenderScaled renderObject) => renderObject.k = k;
}

class RenderScaled extends RenderProxyBox {
  RenderScaled(this._k);
  double _k;
  set k(double v) {
    if (v == _k) return;
    _k = v;
    markNeedsLayout();
  }

  Matrix4 get _m => Matrix4.diagonal3Values(_k, _k, 1);

  @override
  void performLayout() {
    final c = constraints;
    double up(double v) => v.isFinite ? v / _k : v;
    final kid = child;
    if (kid == null) {
      size = c.smallest;
      return;
    }
    kid.layout(BoxConstraints(minWidth: c.minWidth / _k, maxWidth: up(c.maxWidth), minHeight: c.minHeight / _k, maxHeight: up(c.maxHeight)), parentUsesSize: true);
    size = c.constrain(kid.size * _k);
  }

  // (widths scaled; no heights: the content holds LayoutBuilders, which can't be measured that way)
  @override
  double computeMinIntrinsicWidth(double height) => (child?.getMinIntrinsicWidth(height.isFinite ? height / _k : height) ?? 0) * _k;
  @override
  double computeMaxIntrinsicWidth(double height) => (child?.getMaxIntrinsicWidth(height.isFinite ? height / _k : height) ?? 0) * _k;
  @override
  double computeMinIntrinsicHeight(double width) => 0;
  @override
  double computeMaxIntrinsicHeight(double width) => 0;

  @override
  void paint(PaintingContext context, Offset offset) {
    final kid = child;
    if (kid == null) return;
    context.pushTransform(needsCompositing, offset, _m, (ctx, o) => ctx.paintChild(kid, o));
  }

  @override
  bool hitTestChildren(BoxHitTestResult result, {required Offset position}) =>
      child != null && result.addWithPaintTransform(transform: _m, position: position, hitTest: (r, p) => child!.hitTest(r, position: p));

  @override
  void applyPaintTransform(RenderBox child, Matrix4 transform) => transform.multiply(_m);
}
