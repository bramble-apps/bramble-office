// Content Hub (kn_ui contenthub.rs), laid out like the Mac's: a close button, Credits and a
// search field; round tabs (Browse, Photos, Illustrations, Backgrounds, Shapes); category
// chips; rounded tiles. Photos and illustrations are Wikimedia Commons' Featured pictures
// (searched, thumbnailed and downloaded by the engine on a worker isolate, inserted with their
// credit); shapes are the app's basic shapes, the shape library and My Shapes.
import 'dart:async';
import 'dart:io';
import 'dart:math' as math;

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:path_provider/path_provider.dart';

import '../engine.dart';
import 'look.dart';
import 'popovers.dart' show enginePath;
import 'widgets.dart';

class _Feed {
  final items = <Map<String, dynamic>>[];
  int? next;
  bool loading = false, started = false;
  String? error;
}

/// The hub's catalogue (tabs, chips, the library's shapes, icon outlines), read once.
Map<String, dynamic>? _catalog;
final Map<int, List> _libPaths = {};

class ContentHub extends StatefulWidget {
  const ContentHub({super.key, required this.w, required this.set, this.sheet = false});

  /// In the phone layout's sheet: no window frame, as big as the sheet.
  final bool sheet;

  /// dlgapi's "hub" window: the tab it opened on, the app's basic shapes and My Shapes.
  final Map<String, dynamic> w;
  final void Function(String key, [Object? value]) set;

  @override
  State<ContentHub> createState() => _ContentHubState();
}

class _ContentHubState extends State<ContentHub> with SingleTickerProviderStateMixin {
  late int _tab = widget.w['tab'] as int? ?? 0;
  final _cat = List.filled(5, 0);
  final _q = TextEditingController();
  String _searched = '';
  Timer? _typing;
  final _feeds = <String, _Feed>{};
  final _thumbs = <String, String?>{};
  final _thumbWant = <String>{};
  int _running = 0;
  String? _fetching;
  final _credits = <String>[];
  String? _note;
  Timer? _noteTimer;
  late final _open = AnimationController(vsync: this, duration: const Duration(milliseconds: 260))..forward();

  Map<String, dynamic> get cat => _catalog ??= (hubSync('catalog', '') as Map<String, dynamic>? ?? {});

  @override
  void dispose() {
    _typing?.cancel();
    _noteTimer?.cancel();
    _open.dispose();
    _q.dispose();
    super.dispose();
  }

  // ---- look (kn_ui contenthub tile colours)
  bool get _light => LookScope.of(context).light;
  Color get tileBg => _light ? const Color(0xFFF2F2F7) : const Color(0xFF3A3A3A);
  Color get tileHover => _light ? const Color(0xFFE4E4EB) : const Color(0xFF4A4A4A);
  Color get tileEdge => _light ? const Color(0xFFE2E2E8) : const Color(0xFF424242);
  Color get ink => _light ? const Color(0xFF333333) : const Color(0xFFE4E4E4);
  Color get windowBg => _light ? Colors.white : const Color(0xFF242424);

  List _cats(int tab) => switch (tab) { 1 => cat['photos'] as List, 2 => cat['illustrations'] as List, 3 => cat['backgrounds'] as List, _ => const [] };

  void _say(String s) {
    _noteTimer?.cancel();
    setState(() => _note = s);
    _noteTimer = Timer(const Duration(seconds: 6), () => mounted ? setState(() => _note = null) : null);
  }

  _Feed _feed(String words) {
    final f = _feeds.putIfAbsent(words, _Feed.new);
    if (!f.started) _more(words, 0);
    return f;
  }

  Future<void> _more(String words, int offset) async {
    final f = _feeds.putIfAbsent(words, _Feed.new);
    if (f.loading) return;
    f
      ..started = true
      ..loading = true
      ..error = null;
    final r = await hubAsync('search', '{"words": ${_json(words)}, "offset": $offset}') as Map<String, dynamic>?;
    if (!mounted) return;
    setState(() {
      f.loading = false;
      if (r == null || r['error'] != null) {
        f.error = r?['error'] as String? ?? 'Wikimedia Commons could not be reached';
      } else {
        f.items.addAll([for (final i in r['items'] as List) i as Map<String, dynamic>]);
        f.next = r['next'] as int?;
      }
    });
  }

  String _json(String s) => '"${s.replaceAll(r'\', r'\\').replaceAll('"', r'\"')}"';

  /// A thumbnail's cached file (fetched six at a time).
  String? _thumb(String url) {
    if (_thumbs.containsKey(url)) return _thumbs[url];
    _thumbWant.add(url);
    _pump();
    return null;
  }

  void _pump() {
    while (_running < 6 && _thumbWant.isNotEmpty) {
      final url = _thumbWant.last;
      _thumbWant.remove(url);
      _thumbs[url] = null;
      _running++;
      hubAsync('thumb', url).then((p) {
        _running--;
        if (!mounted) return;
        setState(() => _thumbs[url] = p as String?);
        _pump();
      });
    }
  }

  Future<void> _insert(Map<String, dynamic> item) async {
    if (_fetching != null) return;
    setState(() => _fetching = item['thumb'] as String?);
    final dir = (await getTemporaryDirectory()).path;
    final r = await hubAsync('download', '{"item": ${_encode(item)}, "dir": ${_json(dir)}}') as Map<String, dynamic>?;
    if (!mounted) return;
    setState(() => _fetching = null);
    if (r == null || r['error'] != null) return _say(r?['error'] as String? ?? 'The picture could not be downloaded');
    final credit = r['credit'] as String;
    // the credit goes with the picture: on the clipboard and in the Credits list
    Clipboard.setData(ClipboardData(text: credit));
    if (!_credits.contains(credit)) _credits.add(credit);
    widget.set('image', {'path': r['path'], 'name': r['name']});
    _say('Inserted. Its credit is on the clipboard (and under Credits).');
  }

  String _encode(Map<String, dynamic> m) => '{${m.entries.map((e) => '${_json(e.key)}: ${e.value is String ? _json(e.value as String) : e.value}').join(', ')}}';

  // ---------------------------------------------------------------- build

  @override
  Widget build(BuildContext context) {
    final look = LookScope.of(context);
    final screen = MediaQuery.of(context).size;
    final w = math.min(860.0, screen.width - 40), h = math.min(620.0, screen.height - 80);
    final searching = _searched.isNotEmpty;
    final content = Column(crossAxisAlignment: CrossAxisAlignment.stretch, children: [
      _top(look),
      const SizedBox(height: 16),
      if (!searching) ...[
        _tabs(look),
        const SizedBox(height: 16),
        if (_tab != 0) ...[_chips(look), const SizedBox(height: 14)],
      ],
      Expanded(
        child: ListView(
          key: ValueKey('$_tab ${_cat[_tab]} $searching'),
          padding: const EdgeInsets.only(right: 4),
          children: searching ? _searchPage() : switch (_tab) { 0 => _browsePage(), 4 => _shapesPage(), _ => [_pictures(_cats(_tab)[math.min(_cat[_tab], _cats(_tab).length - 1)][2] as String, 1 << 30)] },
        ),
      ),
      const SizedBox(height: 10),
      Text(_note ?? 'Photos and illustrations: Wikimedia Commons (most need a credit) · Shapes: Phosphor Icons (MIT)', style: Look.font(11, look.textDim)),
    ]);
    if (widget.sheet) return Padding(padding: const EdgeInsets.fromLTRB(4, 4, 4, 0), child: content);
    return AnimatedBuilder(
      animation: _open,
      builder: (ctx, child) {
        final t = 1 - math.pow(1 - _open.value, 3).toDouble();
        return Opacity(opacity: _open.value, child: Transform.translate(offset: Offset(0, (1 - t) * 28), child: child));
      },
      child: Container(
        width: w,
        height: h,
        padding: const EdgeInsets.all(16),
        decoration: BoxDecoration(
          color: windowBg,
          borderRadius: BorderRadius.circular(16),
          border: Border.all(color: look.windowStroke, width: 1 / MediaQuery.of(context).devicePixelRatio),
          boxShadow: [BoxShadow(color: Color(look.light ? 0x19000000 : 0x60000000), offset: const Offset(10, 20), blurRadius: 15)],
        ),
        child: content,
      ),
    );
  }

  Widget _top(Look look) {
    return SizedBox(
      height: 34,
      child: Row(children: [
        _Hover(
          builder: (hover) => GestureDetector(
            onTap: () => widget.set('close'),
            child: Container(
              width: 26,
              height: 26,
              decoration: BoxDecoration(color: hover ? tileHover : tileBg, shape: BoxShape.circle),
              child: CustomPaint(painter: _Fn((c, s) {
                final m = s.center(Offset.zero);
                final p = Paint()
                  ..color = ink
                  ..strokeWidth = 1.6;
                c.drawLine(m + const Offset(-4.5, -4.5), m + const Offset(4.5, 4.5), p);
                c.drawLine(m + const Offset(4.5, -4.5), m + const Offset(-4.5, 4.5), p);
              })),
            ),
          ),
        ),
        if (_credits.isNotEmpty) ...[
          const SizedBox(width: 8),
          Builder(
            builder: (ctx) => DButton(
              onTap: () => showDesktopList<int>(ctx, _rectOf(ctx), [for (final (i, c) in _credits.indexed) DChoice(c, i), DChoice('Copy All', -1)], width: 520).then((v) {
                if (v == -1) Clipboard.setData(ClipboardData(text: _credits.join('\n')));
              }),
              child: Text('Credits (${_credits.length})'),
            ),
          ),
        ],
        const Spacer(),
        LayoutBuilder(builder: (ctx, box) {
          final sw = (MediaQuery.of(context).size.width.clamp(0, 860) / 2 - 16).clamp(220.0, 380.0);
          return Container(
            width: sw,
            height: 34,
            decoration: BoxDecoration(color: tileBg, borderRadius: BorderRadius.circular(17)),
            child: Row(children: [
              SizedBox(
                width: 34,
                child: CustomPaint(painter: _Fn((c, s) {
                  final m = Offset(17, s.height / 2 - 1);
                  final p = Paint()
                    ..style = PaintingStyle.stroke
                    ..strokeWidth = 1.6
                    ..color = look.textDim;
                  c.drawCircle(m, 5.5, p);
                  c.drawLine(m + const Offset(4, 4), m + const Offset(8, 8), p);
                })),
              ),
              Expanded(
                child: TextField(
                  controller: _q,
                  style: Look.font(13, look.text),
                  cursorColor: look.text,
                  decoration: InputDecoration(isCollapsed: true, border: InputBorder.none, hintText: 'Search', hintStyle: Look.font(13, look.textDim)),
                  onChanged: (_) {
                    setState(() {});
                    _typing?.cancel();
                    // a search starts a moment after typing stops
                    _typing = Timer(const Duration(milliseconds: 600), () => setState(() => _searched = _q.text.trim()));
                  },
                  onSubmitted: (t) {
                    _typing?.cancel();
                    setState(() => _searched = t.trim());
                  },
                ),
              ),
              SizedBox(
                width: 30,
                child: _q.text.isEmpty
                    ? null
                    : GestureDetector(
                        onTap: () => setState(() {
                          _q.clear();
                          _searched = '';
                          _typing?.cancel();
                        }),
                        child: CustomPaint(painter: _Fn((c, s) {
                          final m = Offset(s.width - 16, s.height / 2);
                          c.drawCircle(m, 8, Paint()..color = look.textDim);
                          final p = Paint()
                            ..color = windowBg
                            ..strokeWidth = 1.4;
                          c.drawLine(m + const Offset(-3, -3), m + const Offset(3, 3), p);
                          c.drawLine(m + const Offset(3, -3), m + const Offset(-3, 3), p);
                        })),
                      ),
              ),
            ]),
          );
        }),
      ]),
    );
  }

  Rect _rectOf(BuildContext ctx) {
    final b = ctx.findRenderObject() as RenderBox;
    return b.localToGlobal(Offset.zero) & b.size;
  }

  /// A library shape's outline by name (tab and chip icons).
  List? _icon(String name) => (cat['icons'] as Map<String, dynamic>?)?[name] as List?;

  Widget _iconBox(String name, double size, Color color) => SizedBox(width: size, height: size, child: CustomPaint(painter: _Outline(_icon(name), color)));

  Widget _tabs(Look look) {
    final tabs = cat['tabs'] as List;
    return Row(mainAxisAlignment: MainAxisAlignment.center, children: [
      for (final (i, t) in tabs.indexed) ...[
        if (i > 0) const SizedBox(width: 8),
        _Hover(
          builder: (hover) => GestureDetector(
            onTap: () => setState(() => _tab = i),
            child: AnimatedContainer(
              duration: const Duration(milliseconds: 150),
              height: 38,
              padding: EdgeInsets.only(left: _tab == i ? 15.5 : 9.5, right: _tab == i ? 16 : 9.5),
              decoration: BoxDecoration(color: _tab == i ? look.accent : (hover ? tileHover : tileBg), borderRadius: BorderRadius.circular(19)),
              child: Row(mainAxisSize: MainAxisSize.min, children: [
                _iconBox(t[1] as String, 19, _tab == i ? Colors.white : ink),
                if (_tab == i) ...[const SizedBox(width: 8), Text(t[0] as String, style: Look.font(15, Colors.white))],
              ]),
            ),
          ),
        ),
      ],
    ]);
  }

  static const _libIcons = {
    'Activities': 'Soccer ball', 'Arrows': 'Arrow right', 'Communication': 'Chat', 'Media': 'Music note', 'Nature': 'Leaf', 'Objects': 'Cube', //
    'People': 'User', 'Places': 'House', 'Symbols': 'Heart', 'Technology': 'Cpu', 'Work': 'Briefcase',
  };

  List<(String, String)> get _chipList {
    if (_tab == 4) {
      return [
        ('All', 'Star'),
        ('Basic', 'Square'),
        for (final c in cat['libCategories'] as List) (c as String, _libIcons[c] ?? 'Shapes'),
        if ((widget.w['mine'] as List).isNotEmpty) ('My Shapes', 'User'),
      ];
    }
    return [for (final c in _cats(_tab)) (c[0] as String, c[1] as String)];
  }

  Widget _chips(Look look) => SizedBox(
        height: 32,
        child: ListView(scrollDirection: Axis.horizontal, children: [
          for (final (i, (name, icon)) in _chipList.indexed) ...[
            if (i > 0) const SizedBox(width: 8),
            _Hover(
              builder: (hover) => GestureDetector(
                onTap: () => setState(() => _cat[_tab] = i),
                child: Container(
                  height: 32,
                  padding: const EdgeInsets.only(left: 14, right: 14),
                  decoration: BoxDecoration(
                    color: _cat[_tab] == i ? look.accent : (hover ? tileHover : windowBg),
                    borderRadius: BorderRadius.circular(16),
                    border: _cat[_tab] == i ? null : Border.all(color: tileEdge),
                  ),
                  child: Row(mainAxisSize: MainAxisSize.min, children: [
                    _iconBox(icon, 16, _cat[_tab] == i ? Colors.white : ink),
                    const SizedBox(width: 8),
                    Text(name, style: Look.font(14, _cat[_tab] == i ? Colors.white : ink)),
                  ]),
                ),
              ),
            ),
          ],
        ]),
      );

  Widget _header(String title, String icon, [(int, int)? showAll]) {
    final look = LookScope.of(context);
    return Padding(
      padding: const EdgeInsets.symmetric(vertical: 6),
      child: SizedBox(
        height: 28,
        child: Row(children: [
          _iconBox(icon, 20, look.textDim),
          const SizedBox(width: 8),
          Text(title, style: Look.font(19, look.text)),
          const Spacer(),
          if (showAll != null)
            GestureDetector(
              onTap: () => setState(() {
                _tab = showAll.$1;
                _cat[_tab] = showAll.$2;
              }),
              child: Padding(padding: const EdgeInsets.only(right: 4), child: Text('Show All', style: Look.font(14, look.accent))),
            ),
        ]),
      ),
    );
  }

  List<Widget> _browsePage() => [
        _header('Shapes', 'Shapes', (4, 0)),
        _shapeTiles([for (var i = 0; i < (widget.w['basic'] as List).length; i++) i], const [], 1),
        for (final (tab, title, icon, c, rows) in [(1, 'Photos', 'Image', 3, 2), (2, 'Illustrations', 'Paint brush', 0, 1), (3, 'Backgrounds', 'Squares four', 0, 1)]) ...[
          const SizedBox(height: 10),
          _header(title, icon, (tab, c)),
          _pictures(_cats(tab)[c][2] as String, rows),
        ],
      ];

  List<Widget> _shapesPage() {
    final libCats = [for (final c in cat['libCategories'] as List) c as String];
    final lib = cat['lib'] as List;
    final basic = [for (var i = 0; i < (widget.w['basic'] as List).length; i++) i];
    final mine = [for (var i = 0; i < (widget.w['mine'] as List).length; i++) i];
    List<int> inCat(String c) => [for (final (i, s) in lib.indexed) if (s[0] == c) i];
    final chip = _cat[4];
    if (chip == 0) {
      return [
        _header('Basic', 'Square'),
        _shapeTiles(basic, const [], 1 << 30),
        for (final c in libCats) ...[const SizedBox(height: 8), _header(c, 'Shapes'), _libTiles(inCat(c))],
        if (mine.isNotEmpty) ...[const SizedBox(height: 8), _header('My Shapes', 'User'), _shapeTiles(const [], mine, 1 << 30)],
      ];
    }
    if (chip == 1) return [_shapeTiles(basic, const [], 1 << 30)];
    if (chip - 2 < libCats.length) return [_libTiles(inCat(libCats[chip - 2]))];
    if (mine.isEmpty) return [DLabel('Format > Shapes and Lines > Save to My Shapes adds shapes here.', dim: true)];
    return [_shapeTiles(const [], mine, 1 << 30)];
  }

  List<Widget> _searchPage() {
    final lower = _searched.toLowerCase();
    final basicList = widget.w['basic'] as List, mineList = widget.w['mine'] as List;
    final basic = [for (final (i, s) in basicList.indexed) if ((s['name'] as String).toLowerCase().contains(lower)) i];
    final mine = [for (final (i, s) in mineList.indexed) if ((s['name'] as String).toLowerCase().contains(lower)) i];
    final lib = [for (final i in (hubSync('libSearch', _searched) as List? ?? const [])) i as int];
    return [
      if (basic.isNotEmpty || mine.isNotEmpty || lib.isNotEmpty) ...[
        _header('Shapes', 'Shapes'),
        _shapeTiles(basic, mine, 1 << 30),
        _libTiles(lib),
        const SizedBox(height: 10),
      ],
      _header('Photos and Illustrations', 'Image'),
      _pictures(hubSync('query', _searched) as String, 1 << 30),
    ];
  }

  /// Square tiles, as many a row as fit at 88 or more (three at least), 10 apart.
  Widget _squares(int n, int rows, Widget Function(int i, double size) tile) => LayoutBuilder(builder: (ctx, box) {
        const gap = 10.0;
        final cols = math.max(3, ((box.maxWidth + gap) / (88 + gap)).floor());
        final size = (box.maxWidth - gap * (cols - 1)) / cols - 0.5;
        final count = math.min(n, cols * rows);
        return Wrap(spacing: gap, runSpacing: gap, children: [for (var i = 0; i < count; i++) SizedBox(width: size, height: size, child: tile(i, size))]);
      });

  Widget _tile({required VoidCallback onTap, VoidCallback? onHold, required Widget child}) => _Hover(
        builder: (hover) => GestureDetector(
          onTap: onTap,
          onLongPress: onHold,
          child: Container(
            decoration: BoxDecoration(color: hover ? tileHover : tileBg, borderRadius: BorderRadius.circular(14), border: Border.all(color: tileEdge)),
            child: child,
          ),
        ),
      );

  Widget _shapeTiles(List<int> basic, List<int> mine, int rows) {
    final all = [for (final i in basic) (false, i), for (final i in mine) (true, i)];
    return _squares(all.length, rows, (k, size) {
      final (isMine, i) = all[k];
      final s = (widget.w[isMine ? 'mine' : 'basic'] as List)[i] as Map<String, dynamic>;
      return Builder(
        builder: (ctx) => _tile(
          onTap: () => widget.set('app', s['key']),
          onHold: isMine
              ? () => showDesktopList<String>(ctx, _rectOf(ctx), const [DChoice('Insert', 'app'), DChoice('Delete from My Shapes', 'remove')]).then((v) {
                    if (v != null) widget.set(v, s['key']);
                  })
              : null,
          child: CustomPaint(painter: _Glyph(s['glyph'], s['name'] as String, ink)),
        ),
      );
    });
  }

  Widget _libTiles(List<int> list) => _squares(list.length, 1 << 30, (k, size) {
        final i = list[k];
        final path = _libPaths[i] ??= hubSync('lib', '$i') as List? ?? const [];
        return _tile(
          onTap: () => widget.set('lib', i),
          child: Padding(padding: EdgeInsets.all(size * 0.24), child: CustomPaint(size: Size.infinite, painter: _Outline(path, ink))),
        );
      });

  /// Pictures in justified rows (as on the Mac), `rows` rows at most; more load at the end.
  Widget _pictures(String words, int rows) {
    final look = LookScope.of(context);
    final f = _feed(words);
    if (f.error != null) {
      return Row(children: [
        DLabel(f.error!, dim: true),
        const SizedBox(width: 8),
        DButton(onTap: () => _more(words, 0), child: const Text('Try Again')),
      ]);
    }
    if (f.items.isEmpty) {
      return Row(children: [
        if (f.loading) ...[const SizedBox(width: 16, height: 16, child: CircularProgressIndicator(strokeWidth: 2)), const SizedBox(width: 8)],
        DLabel(f.loading ? 'Loading…' : 'Nothing found', dim: true),
      ]);
    }
    return LayoutBuilder(builder: (ctx, box) {
      const gap = 10.0, target = 150.0;
      final w = box.maxWidth - 4;
      double aspect(Map<String, dynamic> it) {
        final iw = (it['w'] as num).toDouble(), ih = (it['h'] as num).toDouble();
        return iw == 0 || ih == 0 ? 1.4 : (iw / ih).clamp(0.4, 3.0);
      }

      final lines = <(List<int>, double)>[];
      var cur = <int>[];
      var sum = 0.0;
      for (final (i, it) in f.items.indexed) {
        cur.add(i);
        sum += aspect(it);
        if (sum * target + gap * (cur.length - 1) >= w) {
          lines.add((cur, (w - gap * (cur.length - 1)) / sum));
          cur = [];
          sum = 0;
        }
      }
      final lastFull = f.next == null || rows != 1 << 30;
      if (cur.isNotEmpty && (lastFull || lines.isEmpty)) lines.add((cur, target));
      final shown = lines.take(rows).toList();
      final kids = <Widget>[];
      for (final (line, h) in shown) {
        kids.add(Row(children: [
          for (final (k, i) in line.indexed) ...[
            if (k > 0) const SizedBox(width: gap),
            _picture(f.items[i], aspect(f.items[i]) * h, h, look),
          ],
        ]));
        kids.add(const SizedBox(height: gap));
      }
      if (rows == 1 << 30) {
        if (f.loading) {
          kids.add(const Align(alignment: Alignment.centerLeft, child: SizedBox(width: 16, height: 16, child: CircularProgressIndicator(strokeWidth: 2))));
        } else if (f.next != null) {
          // the end of the list in sight: the next page
          WidgetsBinding.instance.addPostFrameCallback((_) => _more(words, f.next!));
        }
      }
      return Column(crossAxisAlignment: CrossAxisAlignment.start, children: kids);
    });
  }

  Widget _picture(Map<String, dynamic> it, double w, double h, Look look) {
    final url = it['thumb'] as String;
    final file = _thumb(url);
    final busy = _fetching == url;
    final author = it['author'] as String? ?? '';
    return Tooltip(
      message: '${it['title']}\n${author.isEmpty ? '' : '$author · '}${it['licence']}',
      child: _Hover(
        builder: (hover) => GestureDetector(
          onTap: () => _insert(it),
          child: SizedBox(
            width: w,
            height: h,
            child: Stack(fit: StackFit.expand, children: [
              ClipRRect(
                borderRadius: BorderRadius.circular(10),
                child: Container(color: tileBg, child: file == null ? null : Image.file(File(file), fit: BoxFit.cover, gaplessPlayback: true)),
              ),
              if (hover) DecoratedBox(decoration: BoxDecoration(borderRadius: BorderRadius.circular(10), border: Border.all(color: look.accent, width: 3))),
              if (busy)
                DecoratedBox(
                  decoration: BoxDecoration(color: const Color(0x5A000000), borderRadius: BorderRadius.circular(10)),
                  child: const Center(child: SizedBox(width: 28, height: 28, child: CircularProgressIndicator(color: Colors.white, strokeWidth: 2.5))),
                ),
            ]),
          ),
        ),
      ),
    );
  }
}

/// A hover state for the desktop's hover looks (a pointer on Linux; nothing on touch).
class _Hover extends StatefulWidget {
  const _Hover({required this.builder});
  final Widget Function(bool hover) builder;
  @override
  State<_Hover> createState() => _HoverState();
}

class _HoverState extends State<_Hover> {
  bool _on = false;
  @override
  Widget build(BuildContext context) => MouseRegion(onEnter: (_) => setState(() => _on = true), onExit: (_) => setState(() => _on = false), child: widget.builder(_on));
}

class _Fn extends CustomPainter {
  _Fn(this.f);
  final void Function(Canvas, Size) f;
  @override
  void paint(Canvas c, Size s) => f(c, s);
  @override
  bool shouldRepaint(_Fn o) => true;
}

/// An outline [w, h, els] filled (non-zero) to fit the box, 4/128 in from its edges, as
/// kn_ui shapelib's icons.
class _Outline extends CustomPainter {
  _Outline(this.o, this.color);
  final List? o;
  final Color color;
  @override
  void paint(Canvas c, Size s) {
    final o = this.o;
    if (o == null || o.length < 3) return;
    final w = (o[0] as num).toDouble(), h = (o[1] as num).toDouble();
    final side = math.min(s.width, s.height);
    final pad = side * 4 / 128;
    final k = (side - 2 * pad) / math.max(1, math.max(w, h));
    final r = Rect.fromCenter(center: s.center(Offset.zero), width: w * k, height: h * k);
    final p = enginePath(o[2] as List, Rect.fromLTWH(r.left, r.top, r.width, r.height), w: w, h: h);
    c.drawPath(p, Paint()..color = color);
  }

  @override
  bool shouldRepaint(_Outline o) => o.o != this.o || o.color != color;
}

/// The app's own shapes on their tiles (contenthub draw_glyph).
class _Glyph extends CustomPainter {
  _Glyph(this.g, this.name, this.ink);
  final dynamic g;
  final String name;
  final Color ink;
  @override
  void paint(Canvas c, Size s) {
    final r = Offset.zero & s;
    final inner = r.deflate(s.width * 0.24);
    final line = Paint()
      ..color = ink
      ..strokeWidth = 1.3
      ..style = PaintingStyle.stroke;
    void text(String t, double size, double maxW) {
      final tp = TextPainter(text: TextSpan(text: t, style: Look.font(size, ink)), textDirection: TextDirection.ltr, textAlign: TextAlign.center)..layout(maxWidth: maxW);
      tp.paint(c, r.center - Offset(tp.width / 2, tp.height / 2));
    }

    switch (g) {
      case 'text':
        text('Text', (s.width * 0.2).clamp(13, 18), s.width);
      case 'line' || 'arrow':
        final a = inner.bottomLeft, b = inner.topRight;
        c.drawLine(a, b, line);
        if (g == 'arrow') {
          final d = (b - a) / (b - a).distance;
          final n = Offset(-d.dy, d.dx);
          final back = b - d * 9;
          c.drawPath(Path()..addPolygon([b, back + n * 3.5, back - n * 3.5], true), Paint()..color = ink);
        }
      case 'curve':
        final path = Path();
        for (var i = 0; i <= 24; i++) {
          final a = math.pi / 2 * i / 24;
          final p = Offset(inner.left + (1 - math.cos(a)) * inner.width, inner.top + (1 - math.sin(a)) * inner.height);
          i == 0 ? path.moveTo(p.dx, p.dy) : path.lineTo(p.dx, p.dy);
        }
        c.drawPath(path, line);
        c.drawCircle(inner.bottomLeft, 2.5, Paint()..color = ink);
        c.drawCircle(inner.topRight, 2.5, Paint()..color = ink);
      case 'label':
        text(name, 12, s.width - 12);
      case final List o:
        c.save();
        c.translate(inner.left, inner.top);
        _Outline(o, ink).paint(c, inner.size);
        c.restore();
    }
  }

  @override
  bool shouldRepaint(_Glyph o) => o.g != g || o.ink != ink;
}
