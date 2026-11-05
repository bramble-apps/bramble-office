// An iOS 26 screen: the content scrolls under floating glass buttons at the top and a glass
// toolbar at the foot, with the scroll edge effect (the content fades out under the bars); a
// large title that hands over to the small centred one once it scrolls away.
import 'dart:math' as math;

import 'package:flutter/material.dart';

import '../theme.dart';

class NotesPage extends StatefulWidget {
  const NotesPage({
    super.key,
    required this.slivers,
    this.title,
    this.subtitle,
    this.leading,
    this.trailing = const [],
    this.bottom,
    this.background,
    this.largeTitle = true,
    this.controller,
    this.topExtra,
    this.inlineTitle,
  });
  final List<Widget> slivers;
  final String? title, subtitle;
  final Widget? leading, bottom;
  final List<Widget> trailing;
  final Color? background;
  final bool largeTitle;
  final ScrollController? controller;

  /// Something under the large title before the content (a search field…).
  final Widget? topExtra;

  /// iPad's list column (regular width): its title and count in the bar row itself, between
  /// the leading button and the trailing ones, truncated, instead of a large title.
  final Widget? inlineTitle;
  @override
  State<NotesPage> createState() => _NotesPageState();
}

class _NotesPageState extends State<NotesPage> {
  late final _scroll = widget.controller ?? ScrollController();
  final _scrolled = ValueNotifier(0.0);

  @override
  void initState() {
    super.initState();
    _scroll.addListener(() => _scrolled.value = _scroll.hasClients ? _scroll.offset : 0);
  }

  @override
  void dispose() {
    if (widget.controller == null) _scroll.dispose();
    _scrolled.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final p = Palette.of(context);
    final mq = MediaQuery.of(context);
    final bg = widget.background ?? p.grouped;
    final top = mq.padding.top;
    const bar = 60.0;
    final bottomH = widget.bottom == null ? 0.0 : 64 + mq.padding.bottom;
    return ColoredBox(
      color: bg,
      child: Stack(
        children: [
          CustomScrollView(
            controller: _scroll,
            physics: const AlwaysScrollableScrollPhysics(parent: BouncingScrollPhysics()),
            slivers: [
              SliverToBoxAdapter(child: SizedBox(height: top + bar)),
              if (widget.largeTitle && widget.title != null)
                SliverToBoxAdapter(
                  child: Padding(
                    padding: const EdgeInsets.fromLTRB(20, 0, 20, 8),
                    child: Column(
                      crossAxisAlignment: CrossAxisAlignment.start,
                      children: [
                        Text(widget.title!, maxLines: 2, overflow: TextOverflow.ellipsis, style: Type.largeTitle(p.label)),
                        if (widget.subtitle != null)
                          Padding(
                            padding: const EdgeInsets.only(top: 2),
                            child: Text(widget.subtitle!, style: Type.subhead(p.secondary)),
                          ),
                      ],
                    ),
                  ),
                ),
              if (widget.topExtra != null) SliverToBoxAdapter(child: widget.topExtra!),
              ...widget.slivers,
              SliverToBoxAdapter(child: SizedBox(height: bottomH + 24)),
            ],
          ),
          // the scroll edge effect at the top, and the bar
          Positioned(
            left: 0,
            right: 0,
            top: 0,
            height: top + bar + 18,
            child: IgnorePointer(
              child: DecoratedBox(
                decoration: BoxDecoration(
                  gradient: LinearGradient(
                    begin: Alignment.topCenter,
                    end: Alignment.bottomCenter,
                    colors: [bg, bg.withValues(alpha: 0.86), bg.withValues(alpha: 0)],
                    stops: const [0, 0.6, 1],
                  ),
                ),
              ),
            ),
          ),
          Positioned(
            left: 0,
            right: 0,
            top: top,
            height: bar,
            child: Padding(
              padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 6),
              child: Row(
                children: [
                  if (widget.leading != null || widget.inlineTitle == null) widget.leading ?? const SizedBox(width: 48),
                  if (widget.inlineTitle != null)
                    Expanded(
                      child: Padding(
                        padding: EdgeInsets.only(left: widget.leading == null ? 8 : 12, right: 12),
                        child: widget.inlineTitle,
                      ),
                    )
                  else
                  Expanded(
                    child: ValueListenableBuilder<double>(
                      valueListenable: _scrolled,
                      builder: (ctx, off, _) {
                        final show = !widget.largeTitle || off > 44;
                        return AnimatedOpacity(
                          opacity: show && widget.title != null ? 1 : 0,
                          duration: const Duration(milliseconds: 150),
                          child: Text(
                            widget.title ?? '',
                            textAlign: TextAlign.center,
                            maxLines: 1,
                            overflow: TextOverflow.ellipsis,
                            style: Type.title(p.label),
                          ),
                        );
                      },
                    ),
                  ),
                  Row(
                    mainAxisSize: MainAxisSize.min,
                    children: [
                      for (final (i, t) in widget.trailing.indexed) ...[if (i > 0) const SizedBox(width: 12), t],
                      if (widget.trailing.isEmpty) const SizedBox(width: 48),
                    ],
                  ),
                ],
              ),
            ),
          ),
          if (widget.bottom != null) ...[
            Positioned(
              left: 0,
              right: 0,
              bottom: 0,
              height: bottomH + 20,
              child: IgnorePointer(
                child: DecoratedBox(
                  decoration: BoxDecoration(
                    gradient: LinearGradient(
                      begin: Alignment.bottomCenter,
                      end: Alignment.topCenter,
                      colors: [bg, bg.withValues(alpha: 0.86), bg.withValues(alpha: 0)],
                      stops: const [0, 0.55, 1],
                    ),
                  ),
                ),
              ),
            ),
            Positioned(
              left: 0,
              right: 0,
              bottom: math.max(mq.padding.bottom, mq.viewInsets.bottom) + 8,
              height: 56,
              child: Padding(padding: const EdgeInsets.symmetric(horizontal: 16), child: widget.bottom!),
            ),
          ],
        ],
      ),
    );
  }
}

/// An inset grouped section: an optional header, then rows on a rounded card.
class Section extends StatelessWidget {
  const Section({super.key, required this.children, this.header, this.trailingHeader, this.collapsed = false, this.onHeaderTap, this.plain = false});
  final List<Widget> children;

  /// iPad's list column and sidebar: plain rows, no card.
  final bool plain;
  final String? header;
  final Widget? trailingHeader;
  final bool collapsed;
  final VoidCallback? onHeaderTap;

  @override
  Widget build(BuildContext context) {
    final p = Palette.of(context);
    return Padding(
      padding: EdgeInsets.fromLTRB(plain ? 4 : 16, 0, plain ? 4 : 16, 22),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          if (header != null)
            GestureDetector(
              behavior: HitTestBehavior.opaque,
              onTap: onHeaderTap,
              child: Padding(
                padding: const EdgeInsets.fromLTRB(4, 6, 4, 8),
                child: Row(
                  children: [
                    Expanded(
                      child: Text(header!, style: Type.text(20, p.label, weight: FontWeight.w700)),
                    ),
                    ?trailingHeader,
                    if (onHeaderTap != null)
                      AnimatedRotation(
                        turns: collapsed ? -0.25 : 0,
                        duration: const Duration(milliseconds: 200),
                        child: Icon(Icons.keyboard_arrow_down_rounded, color: p.accent, size: 26),
                      ),
                  ],
                ),
              ),
            ),
          if (!collapsed && children.isNotEmpty)
            ClipRRect(
              borderRadius: BorderRadius.circular(plain ? 0 : 26),
              child: ColoredBox(
                color: plain ? Colors.transparent : p.card,
                child: Column(
                  children: [
                    for (final (i, c) in children.indexed) ...[
                      // (iPad's sidebar rows have no separators: only the chosen row's highlight)
                      if (i > 0 && !plain)
                        Padding(
                          padding: EdgeInsets.only(left: plain ? 18 : 58, right: plain ? 12 : 0),
                          child: Container(height: 0.5, color: p.separator),
                        ),
                      c,
                    ],
                  ],
                ),
              ),
            ),
        ],
      ),
    );
  }
}
