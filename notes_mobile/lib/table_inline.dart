// Tables edited in the note, as iOS Notes does: tap a cell to type in it; the selected cell's
// column gets a ⋯ pill on the table's top edge and its row a ⋮ pill on the left edge (tap one
// for Add Row Above / Below, Delete Row, or Add Column Before / After, Delete Column, with that
// row or column tinted); Return goes to the cell below (a new row from the last). Columns are as
// wide as their text (up to most of the screen, then it wraps) and fill the column together; a
// table wider than the note scrolls sideways under the finger, with momentum, iOS's rubber band
// at its ends and a soft fade on the side that has more. Drag a handle (or touch and hold it) to
// move its row or column: it lifts and follows the finger, the others slide out of its way, and
// it settles into its new place; the save says where each row and column came from, so a moved
// cell keeps its formatting (nt-ffi table_of).
import 'dart:math' as math;

import 'package:flutter/cupertino.dart';
import 'package:flutter/material.dart';
import 'package:flutter/rendering.dart';
import 'package:flutter/services.dart';

import 'theme.dart';
import 'ui/glass.dart';
import 'ui/touch_menu.dart';

class InlineTable extends StatefulWidget {
  const InlineTable({super.key, required this.rows, required this.width, required this.editable, required this.onChanged, required this.onActive, this.autofocus = false, this.base});
  final List<List<String>> rows;

  /// The table as last saved (its attachment as the engine gave it): a new one means the edits
  /// so far are stored, and moves count from the table as it now is.
  final Object? base;
  final double width;
  final bool editable, autofocus;

  /// Every edit: the table's rows of cell text (an empty list: the table is to go), and where
  /// each row and column was in the table as last saved (null: new).
  final void Function(List<List<String>> rows, List<int?> rowFrom, List<int?> colFrom) onChanged;

  /// A cell took the keyboard (this table) or the table let it go (null).
  final void Function(InlineTableState? table) onActive;

  @override
  State<InlineTable> createState() => InlineTableState();
}

class InlineTableState extends State<InlineTable> with SingleTickerProviderStateMixin {
  late List<List<TextEditingController>> _cells;
  late List<List<FocusNode>> _nodes;

  /// Where each row and column was in the table as last saved (null: added since), and each
  /// one's key (it keeps its place in the tree, and its slide, as it moves).
  late List<int?> _rowFrom, _colFrom;
  late List<GlobalKey> _rowKeys;
  late List<Object> _colKeys;

  // a row or column being moved: which ('row' / 'col'), from where, over where, the finger's
  // offset along the axis, the rows' heights or columns' widths when it lifted, and its settling
  // into place after the finger lets go
  String? _drag;
  int _dragFrom = 0, _dragTo = 0;
  final _dragOff = ValueNotifier<double>(0);
  List<double> _sizes = const [], _rowH = const [];
  late final _drop = AnimationController(vsync: this, duration: const Duration(milliseconds: 220));
  double _dropStart = 0, _dropEnd = 0;

  /// (the frame after a drop: the others take their places without sliding back)
  bool _snap = false;
  final _scroll = ScrollController();

  /// The cells' focus, hung on the screen's scope rather than under the note's own field: else
  /// the note's field thinks it still has the focus while a cell is typed in, and both talk to
  /// the keyboard at once (Android then gave a cell the note's text).
  final _scope = FocusScopeNode(debugLabel: 'table');

  /// The cell with the caret, and the row or column picked with its handle.
  (int, int)? _sel;
  String? _band;
  bool _fadeL = false, _fadeR = false;

  /// A handle's menu is open: the cell keeps its selection while the menu has the focus.
  bool _menu = false;

  static const _minCol = 96.0, _top = 14.0, _left = 10.0, _cellPad = 12.0;

  @override
  void initState() {
    super.initState();
    _build(widget.rows);
    _scroll.addListener(_edges);
    if (widget.autofocus) WidgetsBinding.instance.addPostFrameCallback((_) => _nodes.first.first.requestFocus());
  }

  @override
  void didUpdateWidget(InlineTable old) {
    super.didUpdateWidget(old);
    // (a save brings the rows back: only taken when they changed elsewhere and no one is typing)
    if (_drag != null) return;
    if (_sel == null && !_same(widget.rows)) {
      _dispose();
      _build(widget.rows);
    } else if (!identical(widget.base, old.base)) {
      _rowFrom = [for (var r = 0; r < _cells.length; r++) r];
      _colFrom = [for (var c = 0; c < _cols; c++) c];
    }
  }

  @override
  void dispose() {
    _dispose();
    _scroll.dispose();
    _scope.dispose();
    _drop.dispose();
    _dragOff.dispose();
    super.dispose();
  }

  bool _same(List<List<String>> rows) {
    if (rows.length != _cells.length) return false;
    for (var r = 0; r < rows.length; r++) {
      for (var c = 0; c < _cols; c++) {
        if ((c < rows[r].length ? rows[r][c] : '') != _cells[r][c].text) return false;
      }
    }
    return true;
  }

  void _build(List<List<String>> rows) {
    final cols = rows.fold<int>(1, (m, r) => math.max(m, r.length));
    final rs = rows.isEmpty ? [List.filled(cols, '')] : rows;
    _cells = [
      for (final r in rs) [for (var c = 0; c < cols; c++) _cell(c < r.length ? r[c] : '')],
    ];
    _nodes = [
      for (final r in _cells) [for (final _ in r) _node()],
    ];
    _rowFrom = [for (var r = 0; r < _cells.length; r++) r];
    _colFrom = [for (var c = 0; c < cols; c++) c];
    _rowKeys = [for (final _ in _cells) GlobalKey()];
    _colKeys = [for (var c = 0; c < cols; c++) Object()];
  }

  TextEditingController _cell(String t) => TextEditingController(text: t)..addListener(_changed);

  FocusNode _node() {
    late final FocusNode n;
    n = FocusNode()..addListener(() => _focusChanged(n));
    return n;
  }

  void _dispose() {
    for (final r in _cells) {
      for (final c in r) {
        c.dispose();
      }
    }
    for (final r in _nodes) {
      for (final n in r) {
        n.dispose();
      }
    }
  }

  int get _cols => _cells.isEmpty ? 0 : _cells.first.length;

  List<List<String>> get rows => [
    for (final r in _cells) [for (final c in r) c.text],
  ];

  String _last = '';
  void _changed() {
    final now = '$rows $_rowFrom $_colFrom';
    if (now == _last) return;
    _last = now;
    widget.onChanged(rows, [..._rowFrom], [..._colFrom]);
    setState(() {});
  }

  void _focusChanged(FocusNode n) {
    for (var r = 0; r < _nodes.length; r++) {
      final c = _nodes[r].indexOf(n);
      if (c < 0) continue;
      if (n.hasFocus) {
        setState(() {
          _sel = (r, c);
          _band = null;
        });
        widget.onActive(this);
      }
    }
    if (!n.hasFocus) {
      // (moving to another cell of this table isn't leaving it)
      WidgetsBinding.instance.addPostFrameCallback((_) {
        if (!mounted || _menu || _nodes.any((r) => r.any((x) => x.hasFocus))) return;
        setState(() {
          _sel = null;
          _band = null;
        });
        widget.onActive(null);
      });
    }
  }

  void _edges() {
    if (!_scroll.hasClients) return;
    final m = _scroll.position;
    final l = m.pixels > 1, r = m.pixels < m.maxScrollExtent - 1;
    if (l != _fadeL || r != _fadeR) {
      setState(() {
        _fadeL = l;
        _fadeR = r;
      });
    }
  }

  void _focus(int r, int c) {
    if (r < _nodes.length && c < _cols) _nodes[r][c].requestFocus();
  }

  void _down(int r, int c) {
    if (r + 1 >= _cells.length) addRow(r + 1);
    WidgetsBinding.instance.addPostFrameCallback((_) => _focus(r + 1, c));
  }

  // ------------------------------------------------------------ editing the grid

  void addRow(int at) {
    _cells.insert(at, [for (var c = 0; c < _cols; c++) _cell('')]);
    _nodes.insert(at, [for (var c = 0; c < _cols; c++) _node()]);
    _rowFrom.insert(at, null);
    _rowKeys.insert(at, GlobalKey());
    _changed();
  }

  void addColumn(int at) {
    for (var r = 0; r < _cells.length; r++) {
      _cells[r].insert(at, _cell(''));
      _nodes[r].insert(at, _node());
    }
    _colFrom.insert(at, null);
    _colKeys.insert(at, Object());
    _changed();
  }

  void _deleteRow(int r) {
    if (_cells.length == 1) return deleteTable();
    final gone = (_cells.removeAt(r), _nodes.removeAt(r));
    _rowFrom.removeAt(r);
    _rowKeys.removeAt(r);
    _changed();
    _focus(math.min(r, _cells.length - 1), _sel?.$2 ?? 0);
    WidgetsBinding.instance.addPostFrameCallback((_) {
      for (final c in gone.$1) {
        c.dispose();
      }
      for (final n in gone.$2) {
        n.dispose();
      }
    });
  }

  void _deleteColumn(int c) {
    if (_cols == 1) return deleteTable();
    final gone = <(TextEditingController, FocusNode)>[];
    for (var r = 0; r < _cells.length; r++) {
      gone.add((_cells[r].removeAt(c), _nodes[r].removeAt(c)));
    }
    _colFrom.removeAt(c);
    _colKeys.removeAt(c);
    _changed();
    _focus(_sel?.$1 ?? 0, math.min(c, _cols - 1));
    WidgetsBinding.instance.addPostFrameCallback((_) {
      for (final (t, n) in gone) {
        t.dispose();
        n.dispose();
      }
    });
  }

  void deleteTable() {
    FocusManager.instance.primaryFocus?.unfocus();
    widget.onActive(null);
    widget.onChanged(const [], const [], const []);
  }

  /// The cell with the caret (row, column).
  (int, int)? get selected => _sel;

  /// The table as text: a line per row, its cells between tabs.
  String asText() => [for (final r in rows) r.join('\t')].join('\n');

  void _handle(BuildContext bctx, String band) {
    final sel = _sel;
    if (sel == null) return;
    setState(() {
      _band = band;
      _menu = true;
    });
    final (r, c) = sel;
    var moved = false;
    final p = Palette.of(context);
    HapticFeedback.selectionClick();
    showTouchMenu(
      context,
      p.look,
      globalRect(bctx),
      centered: true,
      band == 'row'
          ? [
              {'label': 'Add Row Above', 'icon': CupertinoIcons.arrow_up_to_line, 'id': 'above'},
              {'label': 'Add Row Below', 'icon': CupertinoIcons.arrow_down_to_line, 'id': 'below'},
              {'separator': true},
              {'label': 'Delete Row', 'icon': CupertinoIcons.trash, 'id': 'delete', 'destructive': true},
            ]
          : [
              {'label': 'Add Column Before', 'icon': CupertinoIcons.arrow_left_to_line, 'id': 'before'},
              {'label': 'Add Column After', 'icon': CupertinoIcons.arrow_right_to_line, 'id': 'after'},
              {'separator': true},
              {'label': 'Delete Column', 'icon': CupertinoIcons.trash, 'id': 'delete', 'destructive': true},
            ],
      (i) {
        moved = true;
        _menu = false;
        switch (i['id']) {
          case 'above':
            addRow(r);
            WidgetsBinding.instance.addPostFrameCallback((_) => _focus(r, c));
          case 'below':
            addRow(r + 1);
            WidgetsBinding.instance.addPostFrameCallback((_) => _focus(r + 1, c));
          case 'before':
            addColumn(c);
            WidgetsBinding.instance.addPostFrameCallback((_) => _focus(r, c));
          case 'after':
            addColumn(c + 1);
            WidgetsBinding.instance.addPostFrameCallback((_) => _focus(r, c + 1));
          case 'delete':
            band == 'row' ? _deleteRow(r) : _deleteColumn(c);
        }
      },
    ).whenComplete(() {
      _menu = false;
      if (!mounted) return;
      setState(() => _band = null);
      // (closed without a choice: the caret goes back to its cell)
      if (!moved) _focus(r, c);
    });
  }

  // ------------------------------------------------------------ moving a row or column

  void _lift(String axis) {
    final sel = _sel;
    if (sel == null || _drag != null || _drop.isAnimating) return;
    _rowH = [for (final k in _rowKeys) (k.currentContext?.findRenderObject() as RenderBox?)?.size.height ?? 44];
    final style = Type.body(Palette.of(context).label);
    HapticFeedback.mediumImpact();
    setState(() {
      _drag = axis;
      _band = axis;
      _dragFrom = _dragTo = axis == 'row' ? sel.$1 : sel.$2;
      _sizes = axis == 'row' ? _rowH : _widths(style, widget.width - _left);
      _dragOff.value = 0;
    });
  }

  void _follow(double delta) {
    if (_drag == null || _drop.isAnimating) return;
    final before = _sizes.take(_dragFrom).fold(0.0, (a, b) => a + b);
    final after = _sizes.skip(_dragFrom + 1).fold(0.0, (a, b) => a + b);
    _dragOff.value = (_dragOff.value + delta).clamp(-before, after);
    final to = dropIndex(_sizes, _dragFrom, _dragOff.value);
    if (to != _dragTo) {
      HapticFeedback.selectionClick();
      setState(() => _dragTo = to);
    }
  }

  void _put() {
    if (_drag == null || _drop.isAnimating) return;
    _dropStart = _dragOff.value;
    _dropEnd = slotOffset(_sizes, _dragFrom, _dragTo);
    _drop.forward(from: 0).whenComplete(() {
      if (!mounted) return;
      final (from, to, axis) = (_dragFrom, _dragTo, _drag);
      setState(() {
        if (axis == 'row') {
          _cells = moved(_cells, from, to);
          _nodes = moved(_nodes, from, to);
          _rowFrom = moved(_rowFrom, from, to);
          _rowKeys = moved(_rowKeys, from, to);
          if (_sel != null) _sel = (to, _sel!.$2);
        } else {
          _cells = [for (final r in _cells) moved(r, from, to)];
          _nodes = [for (final r in _nodes) moved(r, from, to)];
          _colFrom = moved(_colFrom, from, to);
          _colKeys = moved(_colKeys, from, to);
          if (_sel != null) _sel = (_sel!.$1, to);
        }
        _drag = null;
        _band = null;
        _snap = true;
        _drop.value = 0;
      });
      if (from != to) _changed();
      WidgetsBinding.instance.addPostFrameCallback((_) {
        if (mounted) setState(() => _snap = false);
      });
    });
  }

  /// The lifted row or column's offset from its own place.
  double get _liftedOff => _drop.isAnimating || _drop.value > 0 ? _dropStart + (_dropEnd - _dropStart) * Curves.easeOutCubic.transform(_drop.value) : _dragOff.value;

  // ------------------------------------------------------------ drawing it

  /// Each column's width: its longest line (most of the screen at most), all together filling
  /// the note's column when they're narrower than it.
  List<double> _widths(TextStyle style, double room) {
    final maxCol = math.max(_minCol, room * 0.72);
    final w = <double>[];
    for (var c = 0; c < _cols; c++) {
      var m = 0.0;
      for (final r in _cells) {
        for (final line in r[c].text.split('\n')) {
          if (line.isEmpty) continue;
          final tp = TextPainter(
            text: TextSpan(text: line, style: style),
            textDirection: TextDirection.ltr,
            maxLines: 1,
          )..layout();
          m = math.max(m, tp.width);
        }
      }
      w.add((m + _cellPad * 2 + 2).clamp(_minCol, maxCol));
    }
    final sum = w.fold(0.0, (a, b) => a + b);
    if (sum < room) {
      final extra = (room - sum) / _cols;
      for (var c = 0; c < _cols; c++) {
        w[c] += extra;
      }
    }
    return w;
  }

  @override
  Widget build(BuildContext context) {
    final p = Palette.of(context);
    final style = Type.body(p.label);
    final room = widget.width - _left;
    final widths = _widths(style, room);
    final line = p.separator;
    final tint = p.accent.withValues(alpha: 0.14);
    final sel = _sel;

    final dragging = _drag;
    // a cell (`lifted`: the moving row or column's copy, all its edges drawn; `height`: as tall
    // as its row)
    Widget cell(int r, int c, {bool lifted = false, double? height}) {
      final on = sel != null && ((_band == 'row' && sel.$1 == r) || (_band == 'col' && sel.$2 == c));
      final edge = BorderSide(color: line, width: 0.8);
      final box = Container(
        width: widths[c],
        height: height,
        constraints: const BoxConstraints(minHeight: 44),
        decoration: BoxDecoration(
          color: lifted ? p.page : (on ? tint : null),
          border: Border(right: edge, bottom: edge, left: c == 0 || lifted ? edge : BorderSide.none, top: r == 0 || lifted ? edge : BorderSide.none),
        ),
        padding: const EdgeInsets.symmetric(horizontal: _cellPad, vertical: 11),
        alignment: Alignment.topLeft,
        child: widget.editable && !lifted
            ? TextField(
                controller: _cells[r][c],
                focusNode: _nodes[r][c],
                maxLines: null,
                // (a wrapping field defaults to the multiline keyboard, whose Return types a new line:
                // this one's Return goes to the cell below)
                keyboardType: TextInputType.text,
                textInputAction: TextInputAction.next,
                onSubmitted: (_) => _down(r, c),
                style: style,
                strutStyle: StrutStyle.disabled,
                cursorColor: p.accent,
                selectionControls: cupertinoTextSelectionHandleControls,
                contextMenuBuilder: (ctx, st) => CupertinoAdaptiveTextSelectionToolbar.editableText(editableTextState: st),
                decoration: const InputDecoration.collapsed(hintText: ''),
                scrollPadding: const EdgeInsets.all(80),
              )
            : Text(_cells[r][c].text, style: style),
      );
      if (lifted) return box;
      // a column moving: the others slide out of its way; its own place stays empty
      // (always there, so the cells' fields stay put when a move starts or ends)
      final isFrom = dragging == 'col' && c == _dragFrom;
      return AnimatedContainer(
        key: ValueKey(_colKeys[c]),
        duration: _snap || dragging != 'col' ? Duration.zero : const Duration(milliseconds: 200),
        curve: Curves.easeOutCubic,
        transform: Matrix4.translationValues(dragging != 'col' || isFrom ? 0 : shiftFor(_sizes, _dragFrom, _dragTo, c), 0, 0),
        child: Opacity(opacity: isFrom ? 0 : 1, child: box),
      );
    }

    // the moving row or column itself, over the grid: lifted (a shadow, a little larger), at
    // the finger
    Widget? liftedCopy() {
      if (dragging == null) return null;
      final from = _dragFrom;
      final copy = dragging == 'row'
          ? Row(
              mainAxisSize: MainAxisSize.min,
              children: [for (var c = 0; c < _cols; c++) cell(from, c, lifted: true, height: _rowH[from])],
            )
          : Column(
              mainAxisSize: MainAxisSize.min,
              children: [for (var r = 0; r < _cells.length; r++) cell(r, from, lifted: true, height: _rowH[r])],
            );
      final start = _sizes.take(from).fold(0.0, (a, b) => a + b);
      // (placed once where it lifted from, then only moved as it's painted: following the finger
      // lays nothing out again, so the note's text isn't touched while it moves)
      return Positioned(
        key: const ValueKey('lifted'),
        left: dragging == 'row' ? _left : _left + start,
        top: dragging == 'row' ? _top + start : _top,
        child: IgnorePointer(
          child: AnimatedBuilder(
            animation: Listenable.merge([_dragOff, _drop]),
            builder: (ctx, child) {
              // (it sinks back as it lands)
              final lift = 1 - Curves.easeOutCubic.transform(_drop.value);
              return Transform.translate(
                offset: dragging == 'row' ? Offset(0, _liftedOff) : Offset(_liftedOff, 0),
                child: Transform.scale(
                  scale: 1 + 0.03 * lift,
                  child: DecoratedBox(
                    decoration: BoxDecoration(
                      boxShadow: [BoxShadow(color: Color.fromRGBO(0, 0, 0, 0.28 * lift), blurRadius: 12, offset: const Offset(0, 5))],
                    ),
                    child: child,
                  ),
                ),
              );
            },
            child: RepaintBoundary(child: copy),
          ),
        ),
      );
    }

    final editing = sel != null && widget.editable && sel.$1 < _cells.length && sel.$2 < _cols;
    final grid = Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      mainAxisSize: MainAxisSize.min,
      children: [
        for (var r = 0; r < _cells.length; r++)
          AnimatedContainer(
            key: _rowKeys[r],
            duration: _snap || dragging != 'row' ? Duration.zero : const Duration(milliseconds: 200),
            curve: Curves.easeOutCubic,
            transform: Matrix4.translationValues(0, dragging == 'row' && r != _dragFrom ? shiftFor(_sizes, _dragFrom, _dragTo, r) : 0, 0),
            child: Opacity(
              opacity: dragging == 'row' && r == _dragFrom ? 0 : 1,
              child: Stack(
                children: [
                  Padding(
                    padding: const EdgeInsets.only(left: _left),
                    child: IntrinsicHeight(
                      child: Row(crossAxisAlignment: CrossAxisAlignment.stretch, children: [for (var c = 0; c < _cols; c++) cell(r, c)]),
                    ),
                  ),
                  // the selected row's handle, on the table's left edge
                  if (editing && sel.$1 == r)
                    Positioned(
                      left: 0,
                      top: 0,
                      bottom: 0,
                      width: _left * 2 + 8,
                      child: Builder(
                        builder: (bctx) => _Pill(on: _band == 'row', horizontal: false, onTap: () => _handle(bctx, 'row'), onLift: () => _lift('row'), onMove: _follow, onPut: _put),
                      ),
                    ),
                ],
              ),
            ),
          ),
      ],
    );
    final copy = liftedCopy();

    final content = Stack(
      children: [
        Padding(
          padding: const EdgeInsets.only(top: _top),
          child: grid,
        ),
        // the selected column's handle, on the table's top edge (it goes along with its column)
        if (editing)
          Positioned(
            // (keyed: the lifted copy coming and going mustn't rebuild it mid-drag)
            key: const ValueKey('col-handle'),
            left: _left + widths.take(sel.$2).fold(0.0, (a, b) => a + b) + widths[sel.$2] / 2 - 28,
            top: 0,
            width: 56,
            height: _top * 2 + 4,
            child: AnimatedBuilder(
              animation: Listenable.merge([_dragOff, _drop]),
              builder: (ctx, child) => Transform.translate(offset: Offset(dragging == 'col' ? _liftedOff : 0, 0), child: child),
              child: Builder(
                builder: (bctx) => _Pill(on: _band == 'col', horizontal: true, onTap: () => _handle(bctx, 'col'), onLift: () => _lift('col'), onMove: _follow, onPut: _put),
              ),
            ),
          ),
        // the moving row or column, over the rest
        ?copy,
        // the moving row's handle, with it
        if (dragging == 'row')
          Positioned(
            key: const ValueKey('row-handle'),
            left: 0,
            top: _top + _sizes.take(_dragFrom).fold(0.0, (a, b) => a + b),
            width: _left * 2 + 8,
            height: _rowH[_dragFrom],
            child: IgnorePointer(
              child: AnimatedBuilder(
                animation: Listenable.merge([_dragOff, _drop]),
                builder: (ctx, child) => Transform.translate(offset: Offset(0, _liftedOff), child: child),
                child: _Pill(on: true, horizontal: false, onTap: () {}),
              ),
            ),
          ),
      ],
    );

    WidgetsBinding.instance.addPostFrameCallback((_) => _edges());
    final screen = FocusScope.of(context);
    return FocusScope(
      node: _scope,
      parentNode: identical(screen, _scope) ? null : screen,
      child: SizedBox(
        width: widget.width,
        child: _EdgeFade(
          left: _fadeL,
          right: _fadeR,
          child: SingleChildScrollView(
            controller: _scroll,
            scrollDirection: Axis.horizontal,
            // iOS: momentum, and a rubber band at both ends even when it fits
            physics: const BouncingScrollPhysics(parent: AlwaysScrollableScrollPhysics()),
            child: Padding(padding: const EdgeInsets.only(right: 2), child: content),
          ),
        ),
      ),
    );
  }
}

/// iOS's handle: a small grey pill with ⋯ (a column) or ⋮ (a row); yellow while its row or
/// column is picked. Its touch area is larger than it.
class _Pill extends StatefulWidget {
  const _Pill({required this.on, required this.horizontal, required this.onTap, this.onLift, this.onMove, this.onPut});
  final bool on, horizontal;
  final VoidCallback onTap;

  /// Dragging it (or touching and holding it, then moving) moves its row or column: lifted, the
  /// finger's travel along the axis, let go.
  final VoidCallback? onLift, onPut;
  final void Function(double delta)? onMove;

  @override
  State<_Pill> createState() => _PillState();
}

class _PillState extends State<_Pill> {
  double _held = 0;

  @override
  Widget build(BuildContext context) {
    final w = widget;
    final h = w.horizontal;
    return TextFieldTapRegion(
      child: GestureDetector(
        behavior: HitTestBehavior.opaque,
        onVerticalDragStart: h || w.onLift == null ? null : (_) => w.onLift!(),
        onVerticalDragUpdate: h || w.onMove == null ? null : (d) => w.onMove!(d.delta.dy),
        onVerticalDragEnd: h || w.onPut == null ? null : (_) => w.onPut!(),
        onHorizontalDragStart: !h || w.onLift == null ? null : (_) => w.onLift!(),
        onHorizontalDragUpdate: !h || w.onMove == null ? null : (d) => w.onMove!(d.delta.dx),
        onHorizontalDragEnd: !h || w.onPut == null ? null : (_) => w.onPut!(),
        onLongPressStart: w.onLift == null
            ? null
            : (_) {
                _held = 0;
                w.onLift!();
              },
        onLongPressMoveUpdate: w.onMove == null
            ? null
            : (d) {
                final v = h ? d.offsetFromOrigin.dx : d.offsetFromOrigin.dy;
                w.onMove!(v - _held);
                _held = v;
              },
        onLongPressEnd: w.onPut == null ? null : (_) => w.onPut!(),
        child: _PillFace(on: w.on, horizontal: h, onTap: w.onTap),
      ),
    );
  }
}

class _PillFace extends StatelessWidget {
  const _PillFace({required this.on, required this.horizontal, required this.onTap});
  final bool on, horizontal;
  final VoidCallback onTap;
  @override
  Widget build(BuildContext context) {
    final p = Palette.of(context);
    return TextFieldTapRegion(
      child: GestureDetector(
        behavior: HitTestBehavior.opaque,
        onTap: onTap,
        child: Center(
          child: AnimatedContainer(
            duration: const Duration(milliseconds: 160),
            width: horizontal ? 32 : 15,
            height: horizontal ? 15 : 32,
            decoration: BoxDecoration(
              color: on ? p.accent : p.fill,
              borderRadius: BorderRadius.circular(8),
              border: Border.all(color: p.separator, width: 0.5),
            ),
            child: Icon(horizontal ? Icons.more_horiz_rounded : Icons.more_vert_rounded, size: 14, color: on ? Colors.white : p.secondary),
          ),
        ),
      ),
    );
  }
}

/// Where item `from` (of `sizes`, along the axis) lands when dragged `off` from its place: past
/// the middle of a neighbour, it takes the neighbour's place.
int dropIndex(List<double> sizes, int from, double off) {
  var to = from, acc = 0.0;
  if (off > 0) {
    while (to + 1 < sizes.length && off > acc + sizes[to + 1] / 2) {
      acc += sizes[to + 1];
      to++;
    }
  } else {
    while (to - 1 >= 0 && -off > acc + sizes[to - 1] / 2) {
      acc += sizes[to - 1];
      to--;
    }
  }
  return to;
}

/// How far item `i` slides aside while item `from` is over `to`.
double shiftFor(List<double> sizes, int from, int to, int i) {
  if (from < to && i > from && i <= to) return -sizes[from];
  if (to < from && i >= to && i < from) return sizes[from];
  return 0;
}

/// Where item `from` settles, from its own place, when it's let go over `to`.
double slotOffset(List<double> sizes, int from, int to) => from < to ? sizes.sublist(from + 1, to + 1).fold(0.0, (a, b) => a + b) : -sizes.sublist(to, from).fold(0.0, (a, b) => a + b);

/// `l` with item `from` moved to `to`.
List<T> moved<T>(List<T> l, int from, int to) {
  final x = [...l];
  x.insert(to, x.removeAt(from));
  return x;
}

/// The soft fade on the side of a wide table that has more, drawn only while there is more (a
/// mask layer costs every frame, and most tables fit).
class _EdgeFade extends SingleChildRenderObjectWidget {
  const _EdgeFade({required this.left, required this.right, super.child});
  final bool left, right;
  @override
  _RenderEdgeFade createRenderObject(BuildContext context) => _RenderEdgeFade(left, right);
  @override
  void updateRenderObject(BuildContext context, _RenderEdgeFade r) => r
    ..left = left
    ..right = right;
}

class _RenderEdgeFade extends RenderProxyBox {
  _RenderEdgeFade(this._left, this._right);
  bool _left, _right;
  set left(bool v) {
    if (v == _left) return;
    _left = v;
    markNeedsPaint();
  }

  set right(bool v) {
    if (v == _right) return;
    _right = v;
    markNeedsPaint();
  }

  @override
  bool get alwaysNeedsCompositing => child != null;

  @override
  void paint(PaintingContext context, Offset offset) {
    if (child == null) return;
    if (!_left && !_right) {
      layer = null;
      context.paintChild(child!, offset);
      return;
    }
    final r = offset & size;
    final shader = LinearGradient(
      colors: [_left ? Colors.transparent : Colors.black, Colors.black, Colors.black, _right ? Colors.transparent : Colors.black],
      stops: [0, 22 / size.width, 1 - 22 / size.width, 1],
    ).createShader(Offset.zero & size);
    final l = (layer as ShaderMaskLayer?) ?? ShaderMaskLayer();
    l
      ..shader = shader
      ..maskRect = r
      ..blendMode = BlendMode.dstIn;
    layer = l;
    context.pushLayer(l, (ctx, o) => ctx.paintChild(child!, o), offset);
  }
}
