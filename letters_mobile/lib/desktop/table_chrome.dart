// A selected table's chrome on the page (pg-app tables.rs chrome, borders, cell_selection,
// cell_editor): the column and row tabs (tap to select, drag across, drag a line between two
// to resize, the ⌄ or a long press for the tab's menu), the corner button (the whole table;
// drag it to move the table), the add-column / add-row buttons (tap for a stepper, drag to add
// or take off), the selected cells' outline with the autofill dot, and the cell editor.
import 'dart:math' as math;

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';

import '../engine.dart';
import 'look.dart';
import 'widgets.dart';

const _bar = 20.0, _gap = 6.0, _btn = 24.0;
const _barFill = Color(0xFFF4F4F4), _barLine = Color(0xFFC8C8C8), _label = Color(0xFF707070);
const _tint = Color(0xFFCFE0FB), _yellow = Color(0xFFF5C211);

String columnLetters(int c) {
  var s = '';
  var n = c + 1;
  while (n > 0) {
    s = String.fromCharCode(65 + (n - 1) % 26) + s;
    n = (n - 1) ~/ 26;
  }
  return s;
}

class TableChrome extends StatefulWidget {
  const TableChrome({super.key, required this.engine, required this.t, required this.toLocal, required this.zoom, required this.toView, required this.onChanged, required this.onMenu});
  final Engine engine;

  /// lt_table: the selected table.
  final Map<String, dynamic> t;

  /// Page point → this widget's point, and this widget's point → the engine's view point.
  final Offset Function(int page, double x, double y) toLocal;
  final Offset Function(Offset local) toView;
  final double zoom;
  final VoidCallback onChanged;

  /// A tab's menu: its items at a global point.
  final void Function(List<dynamic> items, Offset global) onMenu;

  @override
  State<TableChrome> createState() => _TableChromeState();
}

class _TableChromeState extends State<TableChrome> {
  // a drag going on: what, and its live value
  (bool rows, int i, double size)? _resize;
  Offset? _resizeAt;
  (bool rows, int n)? _count;
  Offset? _countAt;
  (int r1, int c1)? _fill;
  (bool rows, int lo, int hi, bool moving, double along)? _tabDrag;

  Map<String, dynamic> get t => widget.t;
  List<double> _l(String k) => [for (final v in (t[k] as List? ?? [])) (v as num).toDouble()];
  void act(String a) {
    widget.engine.action('tbl:$a');
    widget.onChanged();
  }

  @override
  Widget build(BuildContext context) {
    final look = LookScope.of(context);
    final z = widget.zoom;
    final cw = _l('colWidths'), rh = _l('rowHeights');
    final page = t['page'] as int? ?? 0;
    final tl = widget.toLocal(page, (t['x'] as num).toDouble(), (t['y'] as num).toDouble());
    final w = cw.fold(0.0, (a, b) => a + b) * z, h = rh.fold(0.0, (a, b) => a + b) * z;
    final br = tl + Offset(w, h);
    final colBar = Rect.fromLTRB(tl.dx, tl.dy - _gap - _bar, br.dx, tl.dy - _gap);
    final rowBar = Rect.fromLTRB(tl.dx - _gap - _bar, tl.dy, tl.dx - _gap, br.dy);
    final rows = t['rows'] as int? ?? 1, cols = t['cols'] as int? ?? 1;
    final sel = (t['sel'] as List?)?.map((v) => v as int).toList();
    final colEdges = [0.0];
    for (final c in cw) {
      colEdges.add(colEdges.last + c * z);
    }
    final rowEdges = [0.0];
    for (final r in rh) {
      rowEdges.add(rowEdges.last + r * z);
    }
    Rect block(int r0, int c0, int r1, int c1) => Rect.fromLTRB(tl.dx + colEdges[c0], tl.dy + rowEdges[r0], tl.dx + colEdges[c1 + 1], tl.dy + rowEdges[r1 + 1]);
    int indexAt(List<double> edges, double v) {
      for (var i = 0; i + 1 < edges.length; i++) {
        if (v < edges[i + 1]) return i;
      }
      return edges.length - 2;
    }

    final editing = t['editing'] as Map<String, dynamic>?;
    final kids = <Widget>[
      // the selected cells (cell_selection)
      if (sel != null && editing == null)
        Positioned.fill(
          child: IgnorePointer(
            child: CustomPaint(painter: _SelPainter(block(sel[0], sel[1], sel[2], sel[3]), look.accent, _fill == null ? null : block(sel[0], sel[1], _fill!.$1, _fill!.$2))),
          ),
        ),
      // the bars
      Positioned.fromRect(rect: colBar, child: _tabs(false, cols, colEdges, colBar, sel, rows, cols, look)),
      Positioned.fromRect(rect: rowBar, child: _tabs(true, rows, rowEdges, rowBar, sel, rows, cols, look)),
      // the corner: the whole table, or drag it to move the table
      _round(Offset(rowBar.center.dx, colBar.center.dy), 'o', look, onTap: () => act('all'), onDragStart: (g) {
        final v = widget.toView(g);
        act('moveStart:${v.dx}:${v.dy}');
        widget.engine.pointer(1, v.dx, v.dy);
      }, onDrag: (g) {
        final v = widget.toView(g);
        widget.engine.pointer(0, v.dx, v.dy);
        widget.onChanged();
      }, onDragEnd: (g) {
        final v = widget.toView(g);
        widget.engine.pointer(2, v.dx, v.dy);
        widget.onChanged();
      }),
      // add columns / rows
      for (final (isRows, c) in [(false, Offset(colBar.right + _gap + _btn / 2, colBar.center.dy)), (true, Offset(rowBar.center.dx, rowBar.bottom + _gap + _btn / 2))])
        _round(c, isRows ? '=' : '||', look, onTap: () => _stepper(context, isRows, c, look), onDragStart: (_) {}, onDrag: (g) {
          final cur = isRows ? rows : cols;
          final last = (isRows ? rh.last : cw.last) * z;
          final d = isRows ? g.dy - c.dy : g.dx - c.dx;
          final floor = (isRows ? t['rowFloor'] : t['colFloor']) as int? ?? 1;
          setState(() {
            _count = (isRows, (cur + (d / math.max(8, last)).round()).clamp(floor, 255));
            _countAt = g;
          });
        }, onDragEnd: (_) {
          final c = _count;
          setState(() => _count = null);
          if (c != null && c.$2 != (c.$1 ? rows : cols)) act('count:${c.$1 ? 1 : 0}:${c.$2}');
        }),
      // the autofill dots
      if (sel != null && editing == null)
        for (final down in [true, false])
          Builder(builder: (_) {
            final b = block(sel[0], sel[1], sel[2], sel[3]);
            final dot = down ? b.bottomCenter : b.centerRight;
            return Positioned(
              left: dot.dx - 9,
              top: dot.dy - 9,
              width: 18,
              height: 18,
              child: GestureDetector(
                behavior: HitTestBehavior.opaque,
                onPanUpdate: (d) {
                  final g = Offset(dot.dx - 9, dot.dy - 9) + d.localPosition;
                  setState(() => _fill = down ? (math.max(sel[2], indexAt(rowEdges, g.dy - tl.dy)), sel[3]) : (sel[2], math.max(sel[3], indexAt(colEdges, g.dx - tl.dx))));
                },
                onPanEnd: (_) {
                  final f = _fill;
                  setState(() => _fill = null);
                  if (f != null) act('autofill:${f.$1}:${f.$2}');
                },
                child: Center(child: Container(width: 9, height: 9, decoration: BoxDecoration(color: _yellow, shape: BoxShape.circle, border: Border.all(color: Colors.white)))),
              ),
            );
          }),
      if (editing != null) _editor(editing, block(editing['r'] as int, editing['c'] as int, editing['r'] as int, editing['c'] as int), z, look),
      // a resize, add or move going on: its line and tip
      Positioned.fill(child: IgnorePointer(child: CustomPaint(painter: _LivePainter(this, tl, br, colEdges, rowEdges, rowBar, look)))),
    ];
    return Stack(children: kids);
  }

  Widget _tabs(bool isRows, int n, List<double> edges, Rect bar, List<int>? sel, int rows, int cols, Look look) {
    final wholeRows = sel != null && sel[1] == 0 && sel[3] + 1 >= cols && cols > 1;
    final wholeCols = sel != null && sel[0] == 0 && sel[2] + 1 >= rows && rows > 1;
    int at(double v) {
      for (var i = 0; i + 1 < edges.length; i++) {
        if (v < edges[i + 1]) return i;
      }
      return n - 1;
    }

    int? nearLine(double v) {
      for (var i = 1; i < edges.length; i++) {
        if ((edges[i] - v).abs() < 9) return i - 1;
      }
      return null;
    }

    void selectSpan(int a, int b) => act(isRows ? 'select:$a:0:$b:${cols - 1}' : 'select:0:$a:${rows - 1}:$b');
    void menu(int i, Offset global) {
      selectSpan(i, i);
      final items = widget.engine.tableTabMenu(isRows, i);
      widget.onMenu(items, global);
    }

    return GestureDetector(
      behavior: HitTestBehavior.opaque,
      onTapUp: (d) {
        final v = isRows ? d.localPosition.dy : d.localPosition.dx;
        final i = at(v);
        // the ⌄ at the end of a column tab
        if (!isRows && d.localPosition.dx > edges[i + 1] - 20) return menu(i, d.globalPosition);
        selectSpan(i, i);
      },
      onLongPressStart: (d) => menu(at(isRows ? d.localPosition.dy : d.localPosition.dx), d.globalPosition),
      onPanStart: (d) {
        final v = isRows ? d.localPosition.dy : d.localPosition.dx;
        final line = nearLine(v);
        if (line != null) {
          _resize = (isRows, line, (isRows ? _l('rowHeights') : _l('colWidths'))[line]);
          _resizeAt = d.localPosition;
          return;
        }
        final i = at(v);
        final covered = sel != null && (isRows ? (sel[0] <= i && i <= sel[2]) : (sel[1] <= i && i <= sel[3]));
        final moving = isRows && covered && wholeRows;
        _tabDrag = (isRows, moving ? sel[0] : i, moving ? sel[2] : i, moving, v);
      },
      onPanUpdate: (d) {
        final v = isRows ? d.localPosition.dy : d.localPosition.dx;
        if (_resize != null) {
          setState(() => _resizeAt = d.localPosition);
          return;
        }
        final td = _tabDrag;
        if (td == null) return;
        if (td.$4) {
          setState(() => _tabDrag = (td.$1, td.$2, td.$3, true, v));
        } else {
          final u = at(v);
          selectSpan(math.min(td.$2, u), math.max(td.$2, u));
        }
      },
      onPanEnd: (_) {
        final r = _resize, at0 = _resizeAt;
        if (r != null && at0 != null) {
          final start = (isRows ? edges[r.$2 + 1] : edges[r.$2 + 1]);
          final delta = ((isRows ? at0.dy : at0.dx) - start) / widget.zoom;
          setState(() => _resize = null);
          final size = math.max(r.$3 + delta, isRows ? 12.0 : 20.0);
          if ((size - r.$3).abs() > 0.5) act('size:${isRows ? 1 : 0}:${r.$2}:${size.toStringAsFixed(1)}');
          return;
        }
        final td = _tabDrag;
        _tabDrag = null;
        if (td != null && td.$4) {
          final first = t['headerRows'] as int? ?? 0, end = rows - (t['footerRows'] as int? ?? 0);
          var before = first;
          for (var k = first; k <= end; k++) {
            if ((edges[k] - td.$5).abs() < (edges[before] - td.$5).abs()) before = k;
          }
          if (before < td.$2 || before > td.$3 + 1) act('move:${td.$2}:${td.$3}:$before');
          setState(() {});
        }
      },
      child: CustomPaint(painter: _BarPainter(isRows, n, edges, sel, isRows ? wholeRows : wholeCols, look)),
    );
  }

  Widget _round(Offset c, String glyph, Look look, {required VoidCallback onTap, required void Function(Offset) onDragStart, required void Function(Offset) onDrag, required void Function(Offset) onDragEnd}) {
    Offset g(Offset local) => Offset(c.dx - _btn, c.dy - _btn) + local;
    return Positioned(
      left: c.dx - _btn,
      top: c.dy - _btn,
      width: _btn * 2,
      height: _btn * 2,
      child: GestureDetector(
        behavior: HitTestBehavior.opaque,
        onTap: onTap,
        onPanStart: (d) => onDragStart(g(d.localPosition)),
        onPanUpdate: (d) => onDrag(g(d.localPosition)),
        onPanEnd: (_) => onDragEnd(_lastDrag ?? c),
        onPanDown: (d) => _lastDrag = g(d.localPosition),
        child: CustomPaint(painter: _RoundPainter(glyph, look)),
      ),
    );
  }

  Offset? _lastDrag;

  /// The add stepper: Rows / Columns and their count.
  void _stepper(BuildContext context, bool isRows, Offset at, Look look) {
    final cur = isRows ? (t['rows'] as int? ?? 1) : (t['cols'] as int? ?? 1);
    final box = context.findRenderObject() as RenderBox;
    final g = box.localToGlobal(at + const Offset(_btn, 0));
    showDialog<void>(
      context: context,
      barrierColor: Colors.transparent,
      builder: (ctx) => Stack(children: [
        Positioned(
          left: g.dx,
          top: g.dy - 18,
          child: LookScope(
            look: look,
            child: Material(
              color: look.panel,
              elevation: 8,
              shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(6), side: BorderSide(color: look.windowStroke)),
              child: Padding(
                padding: const EdgeInsets.all(6),
                child: Row(mainAxisSize: MainAxisSize.min, children: [
                  DLabel(isRows ? 'Rows' : 'Columns'),
                  const SizedBox(width: 8),
                  DStepper(value: cur.toDouble(), min: 1, max: 255, onChanged: (v) {
                    act('count:${isRows ? 1 : 0}:${v.round()}');
                    Navigator.of(ctx).pop();
                  }),
                ]),
              ),
            ),
          ),
        ),
      ]),
    );
  }

  /// The cell editor in the cell's own size, colour and alignment; Return enters it, Tab moves
  /// on, Escape cancels (tables.rs cell_editor).
  Widget _editor(Map<String, dynamic> e, Rect cell, double z, Look look) {
    final pad = [for (final p in (e['padding'] as List? ?? [4, 4, 4, 4])) (p as num).toDouble() * z];
    final size = ((e['size'] as num?)?.toDouble() ?? 11) * z;
    final align = switch (e['align']) { 'center' => TextAlign.center, 'right' => TextAlign.right, _ => TextAlign.left };
    final lines = '${e['text']}'.split('\n').length;
    final need = lines * size * 1.25 + pad[1] + pad[3];
    return Positioned(
      left: cell.left,
      top: cell.top,
      width: cell.width,
      height: math.max(cell.height, need),
      child: _CellField(
        key: ValueKey('${t['id']}:${e['r']}:${e['c']}'),
        text: e['text'] as String? ?? '',
        style: Look.font(size, e['color'] == null ? Colors.black : colorOf(e['color'])).copyWith(height: 1.25),
        align: align,
        fill: e['fill'] == null ? Colors.white : colorOf(e['fill']),
        padding: EdgeInsets.fromLTRB(pad[0], pad[1], pad[2], pad[3]),
        accent: look.accent,
        onText: (s) => widget.engine.action('tbl:text:$s'),
        onCommit: (tab, back) => act(tab ? 'commitTab:${back ? 1 : 0}' : 'commit'),
        onCancel: () => act('cancel'),
      ),
    );
  }
}

class _CellField extends StatefulWidget {
  const _CellField({super.key, required this.text, required this.style, required this.align, required this.fill, required this.padding, required this.accent, required this.onText, required this.onCommit, required this.onCancel});
  final String text;
  final TextStyle style;
  final TextAlign align;
  final Color fill, accent;
  final EdgeInsets padding;
  final void Function(String) onText;
  final void Function(bool tab, bool back) onCommit;
  final VoidCallback onCancel;
  @override
  State<_CellField> createState() => _CellFieldState();
}

class _CellFieldState extends State<_CellField> {
  late final _c = TextEditingController(text: widget.text)..selection = TextSelection.collapsed(offset: widget.text.length);
  late final _f = FocusNode(onKeyEvent: (node, ev) {
    if (ev is! KeyDownEvent) return KeyEventResult.ignored;
    final shift = HardwareKeyboard.instance.isShiftPressed;
    if (ev.logicalKey == LogicalKeyboardKey.enter && !shift) {
      widget.onCommit(false, false);
      return KeyEventResult.handled;
    }
    if (ev.logicalKey == LogicalKeyboardKey.tab) {
      widget.onCommit(true, shift);
      return KeyEventResult.handled;
    }
    if (ev.logicalKey == LogicalKeyboardKey.escape) {
      widget.onCancel();
      return KeyEventResult.handled;
    }
    return KeyEventResult.ignored;
  });

  @override
  void initState() {
    super.initState();
    WidgetsBinding.instance.addPostFrameCallback((_) => _f.requestFocus());
  }

  @override
  void dispose() {
    _c.dispose();
    _f.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) => Container(
        decoration: BoxDecoration(color: widget.fill, border: Border.all(color: widget.accent, width: 2, strokeAlign: BorderSide.strokeAlignOutside)),
        padding: widget.padding,
        child: TextField(
          controller: _c,
          focusNode: _f,
          style: widget.style,
          textAlign: widget.align,
          maxLines: null,
          cursorColor: widget.accent,
          decoration: const InputDecoration(isDense: true, border: InputBorder.none, contentPadding: EdgeInsets.zero),
          onChanged: (s) {
            // the on-screen keyboard's Return: a new paragraph enters the cell
            if (s.endsWith('\n') && !HardwareKeyboard.instance.isShiftPressed && s.length > widget.text.length) {
              widget.onText(s.substring(0, s.length - 1));
              widget.onCommit(false, false);
              return;
            }
            widget.onText(s);
          },
        ),
      );
}

class _BarPainter extends CustomPainter {
  _BarPainter(this.isRows, this.n, this.edges, this.sel, this.whole, this.look);
  final bool isRows, whole;
  final int n;
  final List<double> edges;
  final List<int>? sel;
  final Look look;

  @override
  void paint(Canvas c, Size s) {
    final r0 = Offset.zero & s;
    c.drawRRect(RRect.fromRectAndRadius(r0, const Radius.circular(4)), Paint()..color = _barFill);
    c.drawRRect(RRect.fromRectAndRadius(r0.deflate(0.5), const Radius.circular(4)), Paint()
      ..style = PaintingStyle.stroke
      ..color = _barLine);
    for (var i = 0; i < n; i++) {
      final r = isRows ? Rect.fromLTRB(0, edges[i], s.width, edges[i + 1]) : Rect.fromLTRB(edges[i], 0, edges[i + 1], s.height);
      final covered = sel != null && (isRows ? (sel![0] <= i && i <= sel![2]) : (sel![1] <= i && i <= sel![3]));
      final solid = covered && whole;
      if (covered) c.drawRRect(RRect.fromRectAndRadius(r.deflate(1), const Radius.circular(3)), Paint()..color = solid ? look.accent : _tint);
      if (i > 0) {
        final line = Paint()..color = _barLine;
        isRows ? c.drawLine(Offset(3, r.top), Offset(s.width - 3, r.top), line) : c.drawLine(Offset(r.left, 3), Offset(r.left, s.height - 3), line);
      }
      final tp = TextPainter(text: TextSpan(text: isRows ? '${i + 1}' : columnLetters(i), style: Look.font(11.5, solid ? Colors.white : _label)), textDirection: TextDirection.ltr)..layout();
      tp.paint(c, r.center - Offset(tp.width / 2, tp.height / 2));
    }
  }

  @override
  bool shouldRepaint(_BarPainter o) => true;
}

class _RoundPainter extends CustomPainter {
  _RoundPainter(this.glyph, this.look);
  final String glyph;
  final Look look;
  @override
  void paint(Canvas c, Size s) {
    final ctr = s.center(Offset.zero);
    c.drawCircle(ctr, _btn / 2, Paint()..color = Colors.white);
    c.drawCircle(ctr, _btn / 2 - 0.5, Paint()
      ..style = PaintingStyle.stroke
      ..color = _barLine);
    final ink = Paint()
      ..style = PaintingStyle.stroke
      ..strokeWidth = 1.6
      ..color = _label;
    switch (glyph) {
      case 'o':
        c.drawCircle(ctr, 4.8, ink);
      case '||':
        c.drawLine(ctr + const Offset(-2.3, -5), ctr + const Offset(-2.3, 5), ink);
        c.drawLine(ctr + const Offset(2.3, -5), ctr + const Offset(2.3, 5), ink);
      default:
        c.drawLine(ctr + const Offset(-5, -2.3), ctr + const Offset(5, -2.3), ink);
        c.drawLine(ctr + const Offset(-5, 2.3), ctr + const Offset(5, 2.3), ink);
    }
  }

  @override
  bool shouldRepaint(_RoundPainter o) => false;
}

class _SelPainter extends CustomPainter {
  _SelPainter(this.r, this.accent, this.fill);
  final Rect r;
  final Color accent;
  final Rect? fill;
  @override
  void paint(Canvas c, Size s) {
    c.drawRect(r, Paint()..color = accent.withValues(alpha: 0.10));
    c.drawRect(r, Paint()
      ..style = PaintingStyle.stroke
      ..strokeWidth = 1.5
      ..color = accent);
    for (final p in [r.topLeft, r.bottomRight]) {
      c.drawCircle(p, 3.5, Paint()..color = Colors.white);
      c.drawCircle(p, 3.5, Paint()
        ..style = PaintingStyle.stroke
        ..strokeWidth = 1.2
        ..color = accent);
    }
    if (fill != null) {
      c.drawRect(fill!, Paint()
        ..style = PaintingStyle.stroke
        ..strokeWidth = 3
        ..color = _yellow);
    }
  }

  @override
  bool shouldRepaint(_SelPainter o) => true;
}

/// A tip as tables.rs tip: white text on a dark rounded box.
void _tip(Canvas c, Offset q, String text) {
  final tp = TextPainter(text: TextSpan(text: text, style: Look.font(12, Colors.white)), textDirection: TextDirection.ltr)..layout();
  final at = q + const Offset(14, 14);
  c.drawRRect(RRect.fromRectAndRadius(Rect.fromLTWH(at.dx, at.dy, tp.width + 12, tp.height + 6), const Radius.circular(4)), Paint()..color = const Color.fromARGB(200, 0, 0, 0));
  tp.paint(c, at + const Offset(6, 3));
}

class _LivePainter extends CustomPainter {
  _LivePainter(this.s, this.tl, this.br, this.colEdges, this.rowEdges, this.rowBar, this.look);
  final _TableChromeState s;
  final Offset tl, br;
  final List<double> colEdges, rowEdges;
  final Rect rowBar;
  final Look look;
  @override
  void paint(Canvas c, Size size) {
    final blue = Paint()
      ..strokeWidth = 2
      ..color = look.accent;
    final r = s._resize, at = s._resizeAt;
    if (r != null && at != null) {
      final edges = r.$1 ? rowEdges : colEdges;
      final bar = r.$1 ? Offset(tl.dx - _gap - _bar, tl.dy) : Offset(tl.dx, tl.dy - _gap - _bar);
      final pos = r.$1 ? bar.dy + at.dy : bar.dx + at.dx;
      final newSize = math.max(r.$3 + (pos - (r.$1 ? tl.dy : tl.dx) - edges[r.$2 + 1]) / s.widget.zoom, r.$1 ? 12.0 : 20.0);
      final edge = (r.$1 ? tl.dy : tl.dx) + edges[r.$2] + newSize * s.widget.zoom;
      r.$1 ? c.drawLine(Offset(tl.dx, edge), Offset(br.dx, edge), blue) : c.drawLine(Offset(edge, tl.dy), Offset(edge, br.dy), blue);
      _tip(c, r.$1 ? Offset(br.dx, edge) : Offset(edge, br.dy), '${r.$1 ? 'Height' : 'Width'} ${newSize.round()} pt');
    }
    final n = s._count, q = s._countAt;
    if (n != null && q != null) {
      final cur = (n.$1 ? s.t['rows'] : s.t['cols']) as int? ?? 1;
      final last = (n.$1 ? rowEdges.last - rowEdges[rowEdges.length - 2] : colEdges.last - colEdges[colEdges.length - 2]);
      final edge = (n.$1 ? br.dy : br.dx) + (n.$2 - cur) * math.max(8.0, last);
      n.$1 ? c.drawLine(Offset(tl.dx, edge), Offset(br.dx, edge), blue) : c.drawLine(Offset(edge, tl.dy), Offset(edge, br.dy), blue);
      _tip(c, q, '${n.$2} ${n.$1 ? (n.$2 == 1 ? 'row' : 'rows') : (n.$2 == 1 ? 'column' : 'columns')}');
    }
    final td = s._tabDrag;
    if (td != null && td.$4) {
      c.drawLine(Offset(rowBar.left, tl.dy + td.$5), Offset(br.dx, tl.dy + td.$5), Paint()
        ..strokeWidth = 3
        ..color = look.accent);
    }
  }

  @override
  bool shouldRepaint(_LivePainter o) => true;
}
