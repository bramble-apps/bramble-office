// The sidebar left of the pages (pg-app sidebar.rs): page thumbnails (130 tall, the page
// number under each, the current page framed in the accent colour; tap to go there, touch and
// hold for Add Page After / Delete Page or Go to Page) or the table of contents (the headings,
// indented by level, their page numbers; Insert Table of Contents at the bottom).
import 'dart:io';
import 'dart:math' as math;
import 'dart:ui' as ui;

import 'package:flutter/material.dart';

import '../engine.dart';
import 'look.dart';
import 'widgets.dart';

/// The sidebar's width: the desktop's 180; on Android the page thumbnails' just them and a little
/// round them (the table of contents keeps the room for its headings).
double sidebarWidth([String? mode]) => Platform.isAndroid && mode != 'toc' ? 124 : 180;

class Sidebar extends StatefulWidget {
  const Sidebar({super.key, required this.engine, required this.s, required this.look, required this.onChanged, required this.onMenu});
  final Engine engine;

  /// lt_sidebar.
  final Map<String, dynamic> s;
  final Look look;
  final VoidCallback onChanged;
  final void Function(List<dynamic> items, Offset global) onMenu;

  @override
  State<Sidebar> createState() => _SidebarState();
}

class _SidebarState extends State<Sidebar> {
  /// Thumbnails: page -> (content key, picture). (The key is a 64-bit hash: past 2^63 the JSON
  /// gives it as a double, so it's kept as a number and compared as one.)
  final Map<int, (num, ui.Image)> _thumbs = {};
  final Set<int> _busy = {};

  @override
  void dispose() {
    for (final t in _thumbs.values) {
      t.$2.dispose();
    }
    super.dispose();
  }

  void act(String a) {
    widget.engine.action('side:$a');
    widget.onChanged();
  }

  Future<void> _thumb(int pi, num key, double w, double h) async {
    if (_busy.contains(pi)) return;
    _busy.add(pi);
    final dpr = MediaQuery.of(context).devicePixelRatio;
    final scale = 130 / (h > w ? h : w) * dpr;
    final img = await widget.engine.renderPage(pi, scale, w, h);
    _busy.remove(pi);
    if (!mounted || img == null) return img?.dispose();
    setState(() {
      _thumbs[pi]?.$2.dispose();
      _thumbs[pi] = (key, img);
    });
  }

  @override
  Widget build(BuildContext context) {
    final look = widget.look;
    return Container(
      width: sidebarWidth(widget.s['mode'] as String?),
      decoration: BoxDecoration(color: look.sidebar, border: Border(right: BorderSide(color: look.separator))),
      child: widget.s['mode'] == 'toc' ? _toc(look) : _thumbnails(look),
    );
  }

  Widget _thumbnails(Look look) {
    final pages = [for (final p in (widget.s['pages'] as List? ?? [])) p as Map<String, dynamic>];
    final cur = widget.s['current'] as int? ?? 0;
    final layoutDoc = widget.s['layoutDoc'] == true;
    _thumbs.removeWhere((k, v) => k >= pages.length);
    return ListView.builder(
      padding: const EdgeInsets.symmetric(vertical: 10),
      itemCount: pages.length,
      itemExtent: 130 + (Platform.isAndroid ? 24 : 30),
      itemBuilder: (ctx, pi) {
        final p = pages[pi];
        final pw = (p['w'] as num).toDouble(), ph = (p['h'] as num).toDouble();
        // (130 tall, or narrower to fit the pane with 12 either side: a landscape page)
        final tw = math.min(130.0 * pw / ph, sidebarWidth() - 24);
        final th = tw * ph / pw;
        final key = p['key'] as num;
        final have = _thumbs[pi];
        if (have == null || have.$1 != key) {
          WidgetsBinding.instance.addPostFrameCallback((_) => _thumb(pi, key, pw, ph));
        }
        return GestureDetector(
          behavior: HitTestBehavior.opaque,
          onTap: () => act('go:$pi'),
          onLongPressStart: (d) => widget.onMenu(
            layoutDoc
                ? [
                    {'label': 'Add Page After', 'action': 'side:add:$pi', 'enabled': true},
                    {'label': 'Delete Page', 'action': 'side:delete:$pi', 'enabled': true},
                  ]
                : [
                    {'label': 'Go to Page', 'action': 'side:go:$pi', 'enabled': true},
                  ],
            d.globalPosition,
          ),
          child: CustomPaint(painter: _ThumbPainter(have?.$2, tw, th, pi, pi == cur, look)),
        );
      },
    );
  }

  Widget _toc(Look look) {
    final heads = [for (final h in (widget.s['headings'] as List? ?? [])) h as List];
    return Column(crossAxisAlignment: CrossAxisAlignment.stretch, children: [
      const SizedBox(height: 8),
      Padding(padding: const EdgeInsets.only(left: 8), child: Text('Table of Contents', style: Look.font(13, look.text, bold: true))),
      const SizedBox(height: 6),
      const DSeparator(),
      Expanded(
        child: ListView(children: [
          if (heads.isEmpty)
            Padding(
              padding: const EdgeInsets.only(top: 20, left: 4, right: 4),
              child: Text('Use paragraph styles in your document to populate the table of contents.', style: Look.font(9, look.textDim)),
            ),
          for (final h in heads)
            GestureDetector(
              behavior: HitTestBehavior.opaque,
              onTap: () => act('toc:${h[0]}'),
              child: SizedBox(
                height: 22,
                child: Row(children: [
                  SizedBox(width: 8 + ((h[1] as int) - 1).clamp(0, 9) * 12.0),
                  Expanded(
                    child: Text((h[2] as String).isEmpty ? '(empty heading)' : h[2] as String, maxLines: 1, overflow: TextOverflow.ellipsis, style: Look.font((h[1] as int) <= 1 ? 13 : 12, look.text)),
                  ),
                  Text('${h[3]}', style: Look.font(9, look.textDim)),
                  const SizedBox(width: 6),
                ]),
              ),
            ),
        ]),
      ),
      Padding(
        padding: const EdgeInsets.only(bottom: 8),
        child: Center(child: DButton(onTap: widget.s['canInsertToc'] == true ? () => widget.engine.run('Table of Contents > Document') : null, child: const Text('Insert Table of Contents'))),
      ),
    ]);
  }
}

class _ThumbPainter extends CustomPainter {
  _ThumbPainter(this.img, this.tw, this.th, this.pi, this.current, this.look);
  final ui.Image? img;
  final double tw, th;
  final int pi;
  final bool current;
  final Look look;
  @override
  void paint(Canvas c, Size s) {
    final tr = Rect.fromCenter(center: Offset(s.width / 2, 6 + th / 2), width: tw, height: th);
    if (current) c.drawRRect(RRect.fromRectAndRadius(tr.inflate(6), const Radius.circular(6)), Paint()..color = look.accent.withValues(alpha: 0.35));
    c.drawRect(tr, Paint()..color = Colors.white);
    if (img != null) c.drawImageRect(img!, Offset.zero & Size(img!.width.toDouble(), img!.height.toDouble()), tr, Paint()..filterQuality = FilterQuality.medium);
    c.drawRect(tr.inflate(0.25), Paint()
      ..style = PaintingStyle.stroke
      ..strokeWidth = 0.5
      ..color = look.gray(0x60));
    final tp = TextPainter(text: TextSpan(text: '${pi + 1}', style: Look.font(11, look.textDim)), textDirection: TextDirection.ltr)..layout();
    tp.paint(c, Offset(tr.left, tr.bottom + 6));
  }

  @override
  bool shouldRepaint(_ThumbPainter o) => o.img != img || o.current != current;
}

