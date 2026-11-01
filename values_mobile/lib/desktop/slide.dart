// The side panes slide in and out (kn_ui theme slide: 250 ms, ease in and out), pushing the
// pages over: with Fit Width the page re-fits the room left as the pane moves.
import 'package:flutter/widgets.dart';

class SlidePane extends StatefulWidget {
  const SlidePane({super.key, required this.open, required this.width, required this.child, this.fromLeft = false});
  final bool open;
  final double width;
  final bool fromLeft;
  final Widget child;
  @override
  State<SlidePane> createState() => _SlidePaneState();
}

class _SlidePaneState extends State<SlidePane> with SingleTickerProviderStateMixin {
  late final _t = AnimationController(vsync: this, duration: const Duration(milliseconds: 250), value: widget.open ? 1 : 0);
  late final _curve = CurvedAnimation(parent: _t, curve: Curves.easeInOut);

  @override
  void didUpdateWidget(SlidePane old) {
    super.didUpdateWidget(old);
    if (old.open != widget.open) widget.open ? _t.forward() : _t.reverse();
  }

  @override
  void dispose() {
    _t.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) => AnimatedBuilder(
        animation: _curve,
        builder: (ctx, child) {
          // (shut: kept built offstage, so what it holds stays as it was for the next opening)
          if (_curve.value == 0) return Offstage(child: SizedBox(width: widget.width, child: child));
          return ClipRect(
            child: SizedBox(
              width: widget.width * _curve.value,
              child: OverflowBox(
                alignment: widget.fromLeft ? Alignment.centerRight : Alignment.centerLeft,
                minWidth: widget.width,
                maxWidth: widget.width,
                child: child,
              ),
            ),
          );
        },
        child: widget.child,
      );
}
