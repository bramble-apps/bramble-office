// PDFs (iOS 26 Notes' inline viewer): a PDF attachment shows its pages in the note (Large:
// the first page at the column's width; Medium: half; Small: a card), and opens full screen
// with every page, pinch to zoom, the page number, and a strip of page thumbnails. Pages are
// drawn by Android's PdfRenderer (MainActivity) or, on the desktop test build, pdftoppm.
import 'dart:async';
import 'dart:io';

import 'package:flutter/cupertino.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:path_provider/path_provider.dart';

import 'theme.dart';
import 'ui/glass.dart';

const _system = MethodChannel('app.bramble.notes/system');

final _info = <String, Future<List<Size>>>{};
final _pages = <String, Future<String?>>{};

/// A PDF's pages' sizes (points), read once per file.
Future<List<Size>> pdfPages(String path) => _info.putIfAbsent(path, () async {
  try {
    if (Platform.isAndroid) {
      final r = await _system.invokeMethod<List>('pdfInfo', path);
      return [for (final p in r ?? const []) Size(((p as List)[0] as num).toDouble(), (p[1] as num).toDouble())];
    }
    final r = await Process.run('pdfinfo', ['-f', '1', '-l', '100000', path]);
    final out = <Size>[];
    for (final m in RegExp(r'Page\s+\d+\s+size:\s+([\d.]+)\s+x\s+([\d.]+)').allMatches(r.stdout as String)) {
      out.add(Size(double.parse(m[1]!), double.parse(m[2]!)));
    }
    if (out.isEmpty) {
      final m = RegExp(r'Page size:\s+([\d.]+)\s+x\s+([\d.]+)').firstMatch(r.stdout as String);
      final n = int.tryParse(RegExp(r'Pages:\s+(\d+)').firstMatch(r.stdout as String)?[1] ?? '') ?? 0;
      if (m != null) out.addAll(List.filled(n, Size(double.parse(m[1]!), double.parse(m[2]!))));
    }
    return out;
  } catch (_) {
    return const [];
  }
});

/// Page `index` as a picture `width` pixels wide (drawn once, kept in the cache folder). A page
/// that couldn't be drawn (its file still arriving with a sync, the renderer busy) isn't kept as
/// "no picture": the next time it's shown it's asked for again, so it never stays blank.
Future<String?> pdfPage(String path, int index, int width) {
  final w = ((width + 99) ~/ 100) * 100;
  final key = '$path|$index|$w';
  return _pages.putIfAbsent(key, () => _drawPage(path, index, w).then((r) {
    if (r == null) _pages.remove(key);
    return r;
  }));
}

Future<String?> _drawPage(String path, int index, int w) => (() async {
    try {
      final dir = Directory('${(await getTemporaryDirectory()).path}/pdf-pages')..createSync(recursive: true);
      final out = '${dir.path}/${path.hashCode.toUnsigned(32)}-${File(path).lengthSync()}-$index-$w.png';
      if (File(out).existsSync()) return out;
      if (Platform.isAndroid) {
        final ok = await _system.invokeMethod<bool>('pdfRender', {'path': path, 'page': index, 'width': w, 'out': out});
        return ok == true ? out : null;
      }
      final stem = out.substring(0, out.length - 4);
      await Process.run('pdftoppm', ['-png', '-singlefile', '-f', '${index + 1}', '-l', '${index + 1}', '-scale-to-x', '$w', '-scale-to-y', '-1', path, stem]);
      return File(out).existsSync() ? out : null;
    } catch (_) {
      return null;
    }
  })();

/// One page, drawn when it's needed.
class PdfPageImage extends StatelessWidget {
  const PdfPageImage({super.key, required this.path, required this.index, required this.width, required this.aspect});
  final String path;
  final int index;
  final double width;

  /// height / width
  final double aspect;

  @override
  Widget build(BuildContext context) {
    final px = (width * MediaQuery.devicePixelRatioOf(context)).round().clamp(100, 2400);
    return SizedBox(
      width: width,
      height: width * aspect,
      child: FutureBuilder<String?>(
        future: pdfPage(path, index, px),
        builder: (ctx, s) => s.data == null ? const ColoredBox(color: Colors.white) : Image.file(File(s.data!), fit: BoxFit.fill, gaplessPlayback: true),
      ),
    );
  }
}

/// The PDF in the note, as the desktop shows one (Notes 4.13) with iOS's touch: a grey band with
/// a header ("<name> ⌄" for its menu, and Show / Hide Thumbnails) over every page side by side,
/// the next one peeking in at the right; swipe sideways to page through (it snaps a page at a
/// time, with the rubber band at the ends), "2 of 5" under them; a tap opens the full-screen
/// viewer at that page. Medium is the same viewer about half as tall; Small is a card.
class PdfInline extends StatefulWidget {
  const PdfInline({super.key, required this.path, required this.width, required this.name, this.medium = false, this.onMenu});
  final String path, name;
  final double width;
  final bool medium;

  /// The header's ⌄: the attachment's menu (View As, Delete…), anchored to it.
  final void Function(BuildContext context)? onMenu;

  @override
  State<PdfInline> createState() => _PdfInlineState();
}

class _PdfInlineState extends State<PdfInline> {
  PageController? _pages;
  double _fraction = 0;
  int _page = 0;
  bool _thumbs = false;
  final _strip = ScrollController();

  @override
  void dispose() {
    _pages?.dispose();
    _strip.dispose();
    super.dispose();
  }

  PageController _controller(double fraction) {
    if (_pages == null || (_fraction - fraction).abs() > 0.001) {
      final old = _pages;
      _pages = PageController(viewportFraction: fraction, initialPage: _page);
      _fraction = fraction;
      if (old != null) WidgetsBinding.instance.addPostFrameCallback((_) => old.dispose());
    }
    return _pages!;
  }

  void _go(int i) => _pages?.animateToPage(i, duration: const Duration(milliseconds: 380), curve: Curves.easeOutCubic);

  @override
  Widget build(BuildContext context) {
    final p = Palette.of(context);
    final name = widget.name.replaceAll(RegExp(r'\.pdf$', caseSensitive: false), '');
    return FutureBuilder<List<Size>>(
      future: pdfPages(widget.path),
      builder: (ctx, s) {
        final sizes = s.data ?? const [];
        final n = sizes.length;
        final aspect = sizes.isEmpty ? 1.294 : sizes.first.height / sizes.first.width;
        final w = widget.width;
        // a page's width: the next one peeks in when there are more
        var pageW = n > 1 ? (w - 16) * 0.84 : w - 16;
        if (widget.medium) pageW = pageW * 0.5;
        final pageH = pageW * aspect;
        final fraction = ((pageW + 12) / w).clamp(0.1, 1.0);
        final band = p.light ? const Color(0xFFEDEDF0) : const Color(0xFF2A2A2C);
        return Container(
          width: w,
          decoration: BoxDecoration(color: band, borderRadius: BorderRadius.circular(12)),
          clipBehavior: Clip.antiAlias,
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              // the header
              SizedBox(
                height: 44,
                child: Row(
                  children: [
                    Flexible(
                      child: Builder(
                        builder: (bctx) => Pressable(
                          onTap: widget.onMenu == null ? null : () => widget.onMenu!(bctx),
                          child: Padding(
                            padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 10),
                            child: Row(
                              mainAxisSize: MainAxisSize.min,
                              children: [
                                Flexible(
                                  child: Text(name, maxLines: 1, overflow: TextOverflow.ellipsis, style: Type.text(15, p.label, weight: FontWeight.w600)),
                                ),
                                const SizedBox(width: 4),
                                Icon(CupertinoIcons.chevron_down, size: 13, color: p.secondary),
                              ],
                            ),
                          ),
                        ),
                      ),
                    ),
                    const Spacer(),
                    if (n > 1)
                      Pressable(
                        onTap: () => setState(() => _thumbs = !_thumbs),
                        child: SizedBox(
                          width: 48,
                          height: 44,
                          child: Icon(CupertinoIcons.rectangle_grid_1x2, size: 19, color: _thumbs ? p.accent : p.secondary),
                        ),
                      ),
                  ],
                ),
              ),
              // the thumbnails, the current one marked
              AnimatedSize(
                duration: const Duration(milliseconds: 220),
                curve: Curves.easeOutCubic,
                child: !_thumbs || n < 2
                    ? const SizedBox(width: double.infinity)
                    : SizedBox(
                        height: 78,
                        child: ListView.builder(
                          controller: _strip,
                          scrollDirection: Axis.horizontal,
                          physics: const BouncingScrollPhysics(),
                          padding: const EdgeInsets.fromLTRB(10, 4, 10, 12),
                          itemCount: n,
                          itemBuilder: (ctx, i) => GestureDetector(
                            onTap: () => _go(i),
                            child: Padding(
                              padding: const EdgeInsets.symmetric(horizontal: 4),
                              child: AnimatedContainer(
                                duration: const Duration(milliseconds: 160),
                                decoration: BoxDecoration(
                                  border: Border.all(color: i == _page ? p.accent : Colors.transparent, width: 2),
                                  borderRadius: BorderRadius.circular(4),
                                ),
                                child: PdfPageImage(path: widget.path, index: i, width: 62 / (sizes[i].height / sizes[i].width), aspect: sizes[i].height / sizes[i].width),
                              ),
                            ),
                          ),
                        ),
                      ),
              ),
              SizedBox(
                height: pageH,
                child: n == 0
                    ? const SizedBox.shrink()
                    : PageView.builder(
                        controller: _controller(fraction),
                        padEnds: false,
                        physics: const PageScrollPhysics(parent: BouncingScrollPhysics()),
                        onPageChanged: (i) => setState(() => _page = i),
                        itemCount: n,
                        itemBuilder: (ctx, i) => Padding(
                          padding: EdgeInsets.only(left: i == 0 ? 8 : 6, right: 6),
                          child: Align(
                            alignment: Alignment.topLeft,
                            child: GestureDetector(
                              onTap: () => openPdf(context, widget.path, widget.name, page: i),
                              child: DecoratedBox(
                                decoration: const BoxDecoration(
                                  boxShadow: [BoxShadow(color: Color(0x26000000), blurRadius: 6, offset: Offset(0, 1))],
                                ),
                                child: PdfPageImage(path: widget.path, index: i, width: pageH / (sizes[i].height / sizes[i].width), aspect: sizes[i].height / sizes[i].width),
                              ),
                            ),
                          ),
                        ),
                      ),
              ),
              SizedBox(
                height: 30,
                child: Center(child: Text(n > 1 ? '${_page + 1} of $n' : (n == 1 ? '1 page' : ''), style: Type.caption(p.secondary))),
              ),
            ],
          ),
        );
      },
    );
  }
}

/// The full-screen viewer.
Future<void> openPdf(BuildContext context, String path, String name, {int page = 0}) => Navigator.of(context, rootNavigator: true).push(
  CupertinoPageRoute(
    fullscreenDialog: true,
    builder: (_) => _PdfViewer(path: path, name: name, page: page),
  ),
);

class _PdfViewer extends StatefulWidget {
  const _PdfViewer({required this.path, required this.name, this.page = 0});
  final String path, name;

  /// The page it opens at.
  final int page;
  @override
  State<_PdfViewer> createState() => _PdfViewerState();
}

class _PdfViewerState extends State<_PdfViewer> {
  final _scroll = ScrollController();
  final _zoom = TransformationController();
  bool _thumbs = false;
  int _page = 0;
  List<Size> _sizes = const [];
  final _offsets = <double>[];

  @override
  void initState() {
    super.initState();
    pdfPages(widget.path).then((s) {
      if (!mounted) return;
      setState(() => _sizes = s);
      if (widget.page > 0) {
        WidgetsBinding.instance.addPostFrameCallback((_) {
          if (mounted && widget.page < _offsets.length) _scroll.jumpTo(_offsets[widget.page] - 80);
        });
      }
    });
    _scroll.addListener(() {
      final y = _scroll.offset + 200;
      var k = 0;
      for (var i = 0; i < _offsets.length; i++) {
        if (_offsets[i] <= y) k = i;
      }
      if (k != _page) setState(() => _page = k);
    });
  }

  @override
  void dispose() {
    _scroll.dispose();
    _zoom.dispose();
    super.dispose();
  }

  void _go(int i) {
    if (i < _offsets.length) _scroll.animateTo(_offsets[i] - 80, duration: const Duration(milliseconds: 300), curve: Curves.easeOutCubic);
  }

  @override
  Widget build(BuildContext context) {
    final p = Palette.of(context);
    final mq = MediaQuery.of(context);
    final top = mq.padding.top + 66;
    return Scaffold(
      resizeToAvoidBottomInset: false,
      backgroundColor: p.light ? const Color(0xFFE5E5EA) : const Color(0xFF111111),
      body: Stack(
        children: [
          LayoutBuilder(
            builder: (ctx, box) {
              final w = box.maxWidth - 24 - (_thumbs ? 96 : 0);
              _offsets.clear();
              var y = top;
              for (final s in _sizes) {
                _offsets.add(y);
                y += w * s.height / s.width + 14;
              }
              return InteractiveViewer(
                transformationController: _zoom,
                minScale: 1,
                maxScale: 5,
                child: ListView.builder(
                  controller: _scroll,
                  padding: EdgeInsets.fromLTRB(12 + (_thumbs ? 96 : 0), top, 12, mq.padding.bottom + 40),
                  itemCount: _sizes.length,
                  itemBuilder: (ctx, i) => Padding(
                    padding: const EdgeInsets.only(bottom: 14),
                    child: DecoratedBox(
                      decoration: const BoxDecoration(
                        boxShadow: [BoxShadow(color: Color(0x33000000), blurRadius: 8, offset: Offset(0, 2))],
                      ),
                      child: PdfPageImage(path: widget.path, index: i, width: w, aspect: _sizes[i].height / _sizes[i].width),
                    ),
                  ),
                ),
              );
            },
          ),
          // the page thumbnails
          if (_thumbs)
            Positioned(
              left: 0,
              top: top,
              bottom: 0,
              width: 92,
              child: ListView.builder(
                padding: EdgeInsets.only(bottom: mq.padding.bottom + 20, left: 10, right: 6),
                itemCount: _sizes.length,
                itemBuilder: (ctx, i) => GestureDetector(
                  onTap: () => _go(i),
                  child: Padding(
                    padding: const EdgeInsets.only(bottom: 10),
                    child: Column(
                      children: [
                        Container(
                          decoration: BoxDecoration(
                            border: Border.all(color: i == _page ? p.accent : Colors.transparent, width: 3),
                            borderRadius: BorderRadius.circular(4),
                          ),
                          child: PdfPageImage(path: widget.path, index: i, width: 70, aspect: _sizes[i].height / _sizes[i].width),
                        ),
                        const SizedBox(height: 3),
                        Text('${i + 1}', style: Type.caption(p.secondary)),
                      ],
                    ),
                  ),
                ),
              ),
            ),
          Positioned(
            left: 12,
            right: 12,
            top: mq.padding.top + 6,
            child: Row(
              children: [
                GlassButton(icon: CupertinoIcons.xmark, semantic: 'Close', onTap: () => Navigator.of(context).pop()),
                const SizedBox(width: 8),
                Expanded(
                  child: Text(
                    widget.name.replaceAll(RegExp(r'\.pdf$', caseSensitive: false), ''),
                    textAlign: TextAlign.center,
                    maxLines: 1,
                    overflow: TextOverflow.ellipsis,
                    style: Type.title(p.label),
                  ),
                ),
                const SizedBox(width: 8),
                GlassButton(icon: CupertinoIcons.square_grid_2x2, semantic: 'Thumbnails', tinted: _thumbs, onTap: () => setState(() => _thumbs = !_thumbs)),
              ],
            ),
          ),
          if (_sizes.length > 1)
            Positioned(
              bottom: mq.padding.bottom + 12,
              left: 0,
              right: 0,
              child: Center(
                child: Glass(
                  radius: 16,
                  child: Padding(
                    padding: const EdgeInsets.symmetric(horizontal: 14, vertical: 7),
                    child: Text('${_page + 1} of ${_sizes.length}', style: Type.subhead(p.label)),
                  ),
                ),
              ),
            ),
        ],
      ),
    );
  }
}
