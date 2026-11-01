// The theme chooser (kn-app welcome.rs, kn_ui welcome.rs): the start screen below the toolbar,
// a card with a sidebar (Recent, All Themes), the recent presentations, the themes as their
// previews, and Open… / Cancel / Create. What it shows comes from kd_themes (main.dart puts it
// in the window's shape: "templates", "previews" (their pictures' files), "recent").
import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';

import 'look.dart';
import 'widgets.dart';

class Chooser extends StatefulWidget {
  const Chooser({super.key, required this.w, required this.press, required this.onBrowse});
  final Map<String, dynamic> w;

  /// A button: "create" (a template's index), "open" (a recent document's path), "cancel".
  final void Function(String button, Object? v) press;

  /// Open…: the chooser closes and the file picker opens.
  final VoidCallback onBrowse;

  @override
  State<Chooser> createState() => _ChooserState();
}

class _ChooserState extends State<Chooser> {
  String? _selected;
  int? _jump;
  final _scroll = ScrollController();
  final _content = GlobalKey(), _recentTop = GlobalKey(), _templatesTop = GlobalKey();

  @override
  void initState() {
    super.initState();
    HardwareKeyboard.instance.addHandler(_key);
  }

  @override
  void dispose() {
    HardwareKeyboard.instance.removeHandler(_key);
    _scroll.dispose();
    super.dispose();
  }

  bool _key(KeyEvent e) {
    if (e is KeyDownEvent && e.logicalKey == LogicalKeyboardKey.escape && widget.w['cancel'] == true) {
      widget.press('cancel', null);
      return true;
    }
    return false;
  }

  /// The sidebar's Recent / All Templates: its section's title at the top (scroll_to_rect of
  /// the title grown by 8).
  void _jumpTo(int j) {
    setState(() => _jump = j);
    final at = (j == 0 ? _recentTop : _templatesTop).currentContext?.findRenderObject() as RenderBox?;
    final content = _content.currentContext?.findRenderObject() as RenderBox?;
    if (at == null || content == null || !_scroll.hasClients) return;
    final y = at.localToGlobal(Offset.zero, ancestor: content).dy - 8;
    _scroll.animateTo(y.clamp(0, _scroll.position.maxScrollExtent), duration: const Duration(milliseconds: 250), curve: Curves.easeOut);
  }

  @override
  Widget build(BuildContext context) {
    final look = LookScope.of(context);
    final w = widget.w;
    final templates = [for (final t in w['templates'] as List) t as String];
    final recent = [for (final r in (w['recent'] as List? ?? const [])) r as Map<String, dynamic>];
    final tile = Size(((w['tile'] as List)[0] as num).toDouble(), ((w['tile'] as List)[1] as num).toDouble());
    final selected = _selected ?? (templates.contains(w['default']) ? w['default'] as String : (templates.isEmpty ? '' : templates.first));
    final shadow = Color(look.light ? 0x28000000 : 0x5A000000);
    final sep = look.separator;
    // the backdrop takes every press (the card's own stay on it)
    return GestureDetector(
      behavior: HitTestBehavior.opaque,
      onTap: () {},
      child: ColoredBox(
        color: Color((w['backdrop'] as int?) ?? 0xFFEBEBEB),
        child: LayoutBuilder(builder: (ctx, box) {
          final cw = (box.maxWidth - 64).clamp(320.0, 1080.0), ch = (box.maxHeight - 64).clamp(300.0, 720.0);
          final sideW = cw > 640 ? 190.0 : 0.0;
          final mainW = cw - sideW;
          // recent documents' tiles: the templates' shape, a little wider for slides
          final recentTile = tile.width > tile.height ? tile : Size(tile.width * 1.2, tile.height * 1.2);
          final current = _jump ?? (recent.isEmpty ? 1 : 0);
          return Center(
            child: Container(
              width: cw,
              height: ch,
              decoration: BoxDecoration(
                color: look.panel,
                borderRadius: BorderRadius.circular(14),
                boxShadow: [BoxShadow(color: shadow, offset: const Offset(0, 8), blurRadius: 28)],
              ),
              foregroundDecoration: BoxDecoration(borderRadius: BorderRadius.circular(14), border: Border.all(color: sep)),
              child: Stack(children: [
                // sidebar
                if (sideW > 0)
                  Positioned(
                    left: 0,
                    top: 0,
                    bottom: 0,
                    width: sideW,
                    child: Container(
                      decoration: BoxDecoration(
                        color: look.sidebar,
                        borderRadius: const BorderRadius.horizontal(left: Radius.circular(14)),
                      ),
                      padding: const EdgeInsets.fromLTRB(12, 16 + 26, 12, 16),
                      child: Column(crossAxisAlignment: CrossAxisAlignment.stretch, children: [
                        SizedBox(height: 24, child: Align(alignment: Alignment.centerLeft, child: Text(w['app'] as String? ?? '', style: Look.font(20, look.light ? const Color(0xFF000000) : const Color(0xFFFFFFFF))))),
                        const SizedBox(height: 6 + 14),
                        for (final (i, (icon, name)) in [('◷', 'Recent'), ('▦', w['all'] as String? ?? '')].indexed)
                          if (i != 0 || recent.isNotEmpty) ...[
                            _SideButton(label: '$icon  $name', on: current == i, onTap: () => _jumpTo(i)),
                            const SizedBox(height: 6),
                          ],
                      ]),
                    ),
                  ),
                // (the sidebar's edge: a line on its right side, as vline)
                if (sideW > 0) Positioned(left: sideW - 0.5, top: 0, bottom: 0, width: 1, child: ColoredBox(color: sep)),
                // header
                Positioned(
                  left: sideW + 24,
                  top: 20,
                  width: mainW - 48,
                  height: 40,
                  child: Align(alignment: Alignment.centerLeft, child: Text(w['title'] as String? ?? '', maxLines: 1, softWrap: false, style: Look.font(26, look.text, bold: true))),
                ),
                // the scrolling middle
                // (6 points of room round the tiles: the selection ring stands out 3 from its
                // tile, and the scrolling area's edge cut it off)
                Positioned(
                  left: sideW + 24 - 6,
                  top: 70 - 6,
                  right: 12,
                  bottom: 56 + 4,
                  child: ScrollConfiguration(
                    behavior: ScrollConfiguration.of(context).copyWith(scrollbars: false),
                    child: SingleChildScrollView(
                      controller: _scroll,
                      padding: const EdgeInsets.only(left: 6, top: 6),
                      child: SizedBox(
                        key: _content,
                        width: mainW - 36 - 16,
                        child: Column(crossAxisAlignment: CrossAxisAlignment.start, children: [
                          if (recent.isNotEmpty) ...[
                            _section(look, 'Recent', _recentTop),
                            SingleChildScrollView(
                              scrollDirection: Axis.horizontal,
                              clipBehavior: Clip.none,
                              child: Row(children: [
                                for (final (i, r) in recent.indexed) ...[
                                  if (i > 0) const SizedBox(width: 18),
                                  Tooltip(
                                    message: r['path'] as String? ?? '',
                                    waitDuration: const Duration(milliseconds: 600),
                                    child: _Card(
                                      size: recentTile,
                                      name: r['name'] as String? ?? '',
                                      thumb: r['thumb'] as String?,
                                      onTap: () => widget.press('open', r['path']),
                                    ),
                                  ),
                                ],
                              ]),
                            ),
                            const SizedBox(height: 6 + 14),
                          ],
                          _section(look, w['section'] as String? ?? '', _templatesTop),
                          Wrap(spacing: 18, runSpacing: 16, children: [
                            for (final (i, t) in templates.indexed)
                              _Card(
                                size: tile,
                                name: t,
                                thumb: (w['previews'] as List?)?.elementAtOrNull(i) as String?,
                                draw: (w['previews'] as List?)?.elementAtOrNull(i) == null ? (c, r) => _drawTemplate(c, r, i, look) : null,
                                selected: t == selected,
                                onTap: () => setState(() => _selected = t),
                                onDoubleTap: () => widget.press('create', i),
                              ),
                          ]),
                          const SizedBox(height: 6 + 12),
                        ]),
                      ),
                    ),
                  ),
                ),
                // footer: Open… on the left; Cancel and Create on the right
                Positioned(
                  left: sideW,
                  right: 0,
                  bottom: 0,
                  height: 56,
                  child: Container(
                    decoration: BoxDecoration(border: Border(top: BorderSide(color: sep))),
                    padding: const EdgeInsets.symmetric(horizontal: 20, vertical: 12),
                    child: Row(children: [
                      DButton(width: 90, height: 30, onTap: widget.onBrowse, child: Text('Open…', style: Look.font(14, look.text))),
                      const SizedBox(width: 8),
                      Expanded(child: Text(w['hint'] as String? ?? '', maxLines: 1, overflow: TextOverflow.clip, softWrap: false, style: Look.font(12, look.textDim))),
                      if (w['cancel'] == true) ...[
                        DButton(width: 84, height: 30, onTap: () => widget.press('cancel', null), child: Text('Cancel', style: Look.font(14, look.text))),
                        const SizedBox(width: 8),
                      ],
                      DButton(
                        width: 96,
                        height: 30,
                        fill: look.accent,
                        onTap: selected.isEmpty ? null : () => widget.press('create', templates.indexOf(selected)),
                        child: Text('Create', style: Look.font(14, const Color(0xFFFFFFFF))),
                      ),
                    ]),
                  ),
                ),
              ]),
            ),
          );
        }),
      ),
    );
  }

  /// A section's title (15 points), 4 above it and 12 under it.
  Widget _section(Look look, String title, Key key) => Padding(
        padding: const EdgeInsets.only(top: 4, bottom: 6 + 6),
        child: SizedBox(key: key, height: 18, child: Align(alignment: Alignment.centerLeft, child: Text(title, style: Look.font(15, look.text)))),
      );
}

/// The sidebar's Recent / All Templates (an egui Button, 30 tall, the accent's tint when on).
class _SideButton extends StatelessWidget {
  const _SideButton({required this.label, required this.on, required this.onTap});
  final String label;
  final bool on;
  final VoidCallback onTap;

  @override
  Widget build(BuildContext context) {
    final look = LookScope.of(context);
    return GestureDetector(
      behavior: HitTestBehavior.opaque,
      onTap: onTap,
      child: Container(
        height: 30,
        padding: const EdgeInsets.symmetric(horizontal: 10),
        alignment: Alignment.centerLeft,
        decoration: BoxDecoration(color: on ? look.accent.withValues(alpha: 0.15) : null, borderRadius: BorderRadius.circular(6)),
        child: Text(label, maxLines: 1, softWrap: false, style: Look.font(14, on ? look.accent : look.text)),
      ),
    );
  }
}

/// welcome.rs `card`: a picture (or a drawn tile) with its name under it; selected / hover rings.
class _Card extends StatefulWidget {
  const _Card({required this.size, required this.name, this.thumb, this.draw, this.selected = false, required this.onTap, this.onDoubleTap});
  final Size size;
  final String name;
  final String? thumb;
  final void Function(Canvas c, Rect r)? draw;
  final bool selected;
  final VoidCallback onTap;
  final VoidCallback? onDoubleTap;

  @override
  State<_Card> createState() => _CardState();
}

class _CardState extends State<_Card> {
  bool _hover = false;

  @override
  Widget build(BuildContext context) {
    final look = LookScope.of(context);
    final size = widget.size, name = widget.name;
    final round = BorderRadius.circular(6);
    // (a file: its name without the extension)
    final dot = name.lastIndexOf('.');
    final shown = dot > 0 ? name.substring(0, dot) : name;
    final ext = dot > 0 && dot < name.length - 1 ? name.substring(dot + 1).toUpperCase() : '…';
    // no picture (yet): its kind on the tile colour
    Widget kind() => Container(
          decoration: BoxDecoration(color: look.gray(0x30), borderRadius: round),
          alignment: Alignment.center,
          child: Text(ext, style: Look.font(16, look.textDim)),
        );
    final Widget pic;
    if (widget.thumb != null) {
      pic = Container(
        decoration: BoxDecoration(color: look.gray(0x18), borderRadius: round),
        alignment: Alignment.center,
        child: ClipRRect(
          borderRadius: round,
          child: Image.file(
            File(widget.thumb!),
            fit: BoxFit.contain,
            filterQuality: FilterQuality.medium,
            frameBuilder: (_, child, frame, sync) => frame == null && !sync ? const SizedBox.shrink() : child,
            errorBuilder: (_, _, _) => const SizedBox.shrink(),
          ),
        ),
      );
    } else if (widget.draw != null) {
      pic = Container(
        decoration: BoxDecoration(color: const Color(0xFFFFFFFF), borderRadius: round),
        child: CustomPaint(painter: _TilePainter(widget.draw!)),
      );
    } else {
      pic = kind();
    }
    final ring = widget.selected
        ? Positioned(left: -3, top: -3, width: size.width + 6, height: size.height + 6, child: _ring(look.accent, 3, 8))
        : _hover
            ? Positioned(left: -2, top: -2, width: size.width + 4, height: size.height + 4, child: _ring(look.accent.withValues(alpha: 0.6), 2, 7))
            : null;
    return MouseRegion(
      onEnter: (_) => setState(() => _hover = true),
      onExit: (_) => setState(() => _hover = false),
      child: GestureDetector(
        behavior: HitTestBehavior.opaque,
        onTap: widget.onTap,
        onDoubleTap: widget.onDoubleTap,
        child: SizedBox(
          width: size.width,
          height: size.height + 24,
          child: Stack(clipBehavior: Clip.none, children: [
            Positioned(
              left: 0,
              top: 0,
              width: size.width,
              height: size.height,
              child: Container(
                decoration: BoxDecoration(
                  borderRadius: round,
                  boxShadow: [BoxShadow(color: Color(look.light ? 0x1C000000 : 0x3C000000), offset: const Offset(0, 2), blurRadius: 8)],
                ),
                foregroundDecoration: BoxDecoration(borderRadius: round, border: Border.all(color: look.separator)),
                child: pic,
              ),
            ),
            ?ring,
            Positioned(
              left: 0,
              top: size.height + 6,
              width: size.width,
              child: Text(
                shown,
                textAlign: TextAlign.center,
                maxLines: 1,
                overflow: TextOverflow.ellipsis,
                style: Look.font(12.5, widget.selected ? look.accent : look.text),
              ),
            ),
          ]),
        ),
      ),
    );
  }

  /// A ring drawn inside its rectangle (rect_stroke … StrokeKind::Inside).
  Widget _ring(Color c, double width, double radius) => IgnorePointer(
        child: DecoratedBox(decoration: BoxDecoration(borderRadius: BorderRadius.circular(radius), border: Border.all(color: c, width: width))),
      );
}

class _TilePainter extends CustomPainter {
  _TilePainter(this.draw);
  final void Function(Canvas c, Rect r) draw;
  @override
  void paint(Canvas canvas, Size size) => draw(canvas, Offset.zero & size);
  @override
  bool shouldRepaint(_TilePainter o) => false;
}

/// dialogs.rs chooser's `draw`: a hint of each template's contents on its page.
void _drawTemplate(Canvas c, Rect tile, int i, Look look) {
  var page = tile;
  if (i == 1) {
    page = Rect.fromCenter(center: tile.center, width: tile.width - 16, height: (tile.width - 16) * 0.77);
    c.drawRRect(RRect.fromRectAndRadius(tile, const Radius.circular(6)), Paint()..color = look.gray(0x30));
    c.drawRRect(RRect.fromRectAndRadius(page, const Radius.circular(2)), Paint()..color = const Color(0xFFFFFFFF));
  }
  final k = page.width / 110;
  void line(double y, double w, double t) => c.drawRect(Rect.fromLTWH(page.left + 12 * k, page.top + y * k, w * k, t * k), Paint()..color = const Color(0xFFB0B0B0));
  switch (i) {
    case 3:
      line(14, 60, 4);
      final p = Paint()
        ..color = const Color(0xFF9CC2F0)
        ..strokeWidth = 0.6;
      for (var n = 0; n < 9; n++) {
        final y = page.top + (30 + n * 12) * k;
        c.drawLine(Offset(page.left + 8 * k, y), Offset(page.right - 8 * k, y), p);
      }
    case 4:
      c.drawRect(
        page.deflate(14 * k),
        Paint()
          ..style = PaintingStyle.stroke
          ..strokeWidth = 0.8
          ..color = const Color(0xFFC8C8C8),
      );
    case 5:
      line(16, 70, 5);
      line(30, 40, 3);
      for (var n = 0; n < 6; n++) {
        line(44 + n * 8.0, 84, 2);
      }
  }
}
