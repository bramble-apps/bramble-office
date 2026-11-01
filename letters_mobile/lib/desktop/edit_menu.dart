// The edit menu on the page (Pages on iPad): a rounded bar just above the selection or caret
// with the right-click menu's enabled items in a row, thin dividers between them (a wider one
// between groups), › and ‹ paging through what doesn't fit, a submenu's items in the bar with
// a ‹ back. Shown by a touch and hold, a double tap, a tap on the selection or caret, and a
// mouse's right-click; DocView closes it on a tap elsewhere, scrolling or typing.

import 'package:flutter/material.dart';

import 'look.dart';

class EditMenu extends StatefulWidget {
  const EditMenu({super.key, required this.items, required this.anchor, required this.bounds, required this.look, required this.onPick});

  /// lt_context_menu's items: {label, action, enabled, bold, checked} | {separator} |
  /// {label, note} | {label, kids}.
  final List<dynamic> items;

  /// What the bar points at (local): the selection, the caret, the objects or the touch.
  final Rect anchor;

  /// Where it may go (local): the view under the toolbar.
  final Rect bounds;
  final Look look;
  final void Function(String action) onPick;

  /// The bar's height (a 34-point touch target).
  static const height = 34.0;

  @override
  State<EditMenu> createState() => _EditMenuState();
}

/// One cell of the bar: an item, a note, a submenu, or a divider (`group`: between groups).
class _Cell {
  _Cell.entry(this.label, this.item, {this.bold = false, this.checked = false, this.note = false}) : divider = false, group = false;
  _Cell.line(this.group) : label = '', item = null, bold = false, checked = false, note = false, divider = true;
  final String label;
  final Map? item;
  final bool bold, checked, note, divider, group;
}

class _EditMenuState extends State<EditMenu> {
  /// The submenus opened (each one's items), innermost last.
  final _path = <List<dynamic>>[];
  int _page = 0;

  static const _pad = 11.0, _arrow = 32.0, _gap = 6.0;

  @override
  void didUpdateWidget(EditMenu old) {
    super.didUpdateWidget(old);
    if (!identical(old.items, widget.items)) {
      _path.clear();
      _page = 0;
    }
  }

  /// The enabled items in order: disabled ones left out, separators as group dividers (none
  /// at the ends or two together), submenus with nothing enabled left out.
  List<_Cell> _cells(List<dynamic> items) {
    final out = <_Cell>[];
    var sep = false;
    for (final i in items) {
      if (i is! Map) continue;
      _Cell? c;
      if (i['separator'] == true) {
        sep = out.isNotEmpty;
        continue;
      } else if (i['kids'] is List) {
        if (_cells(i['kids'] as List).isNotEmpty) c = _Cell.entry(i['label'] as String? ?? '', i);
      } else if (i['note'] == true) {
        c = _Cell.entry(i['label'] as String? ?? '', null, note: true);
      } else if (i['enabled'] != false) {
        c = _Cell.entry(i['label'] as String? ?? '', i, bold: i['bold'] == true, checked: i['checked'] == true);
      }
      if (c == null) continue;
      if (out.isNotEmpty) out.add(_Cell.line(sep));
      sep = false;
      out.add(c);
    }
    return out;
  }

  TextStyle _style(_Cell c) {
    final look = widget.look;
    return Look.font(13, c.note ? look.weakText : look.text, bold: c.bold);
  }

  String _text(_Cell c) => c.checked ? '✓ ${c.label}' : c.label;

  double _width(_Cell c) {
    if (c.divider) return c.group ? 1.5 : 0.5;
    final tp = TextPainter(text: TextSpan(text: _text(c), style: DefaultTextStyle.of(context).style.merge(_style(c))), textDirection: TextDirection.ltr, textScaler: MediaQuery.textScalerOf(context), maxLines: 1)..layout();
    return tp.width.ceilToDouble() + 2 * _pad;
  }

  /// The cells split into pages that fit `max` (with room for the arrows on each page).
  List<List<_Cell>> _pages(List<_Cell> cells, double max) {
    final all = cells.fold<double>(0, (w, c) => w + _width(c));
    final back = _path.isNotEmpty ? _arrow : 0.0;
    if (all + back <= max) return [cells];
    final pages = <List<_Cell>>[];
    var cur = <_Cell>[];
    var w = 0.0;
    for (final c in cells) {
      final room = max - (pages.isNotEmpty || _path.isNotEmpty ? _arrow : 0) - _arrow;
      final cw = _width(c);
      if (c.divider) {
        if (cur.isNotEmpty) {
          cur.add(c);
          w += cw;
        }
        continue;
      }
      if (cur.isNotEmpty && w + cw > room) {
        // (no divider left hanging at the page's end)
        while (cur.isNotEmpty && cur.last.divider) {
          cur.removeLast();
        }
        pages.add(cur);
        cur = [];
        w = 0;
      }
      cur.add(c);
      w += cw;
    }
    if (cur.isNotEmpty) pages.add(cur);
    return pages;
  }

  void _tap(_Cell c) {
    final i = c.item;
    if (i == null) return;
    if (i['kids'] is List) {
      setState(() {
        _path.add(i['kids'] as List);
        _page = 0;
      });
      return;
    }
    widget.onPick(i['action'] as String? ?? '');
  }

  @override
  Widget build(BuildContext context) {
    final look = widget.look;
    final b = widget.bounds.deflate(_gap);
    if (b.width <= 0 || b.height <= 0) return const SizedBox.shrink();
    final cells = _cells(_path.isEmpty ? widget.items : _path.last);
    if (cells.isEmpty) return const SizedBox.shrink();
    final pages = _pages(cells, b.width);
    final page = _page.clamp(0, pages.length - 1);
    final shown = pages[page];
    // ‹ back: the page before, or out of the submenu on its first page
    final back = page > 0 || _path.isNotEmpty;
    final next = page < pages.length - 1;
    // (from the text: Look's separator is the dark bar's own colour)
    final line = look.text.withValues(alpha: 0.22);

    Widget cell(_Cell c) {
      if (c.divider) {
        return Container(width: _width(c), margin: EdgeInsets.symmetric(vertical: c.group ? 8 : 11), color: c.group ? look.text.withValues(alpha: 0.32) : line);
      }
      return _Press(
        look: look,
        onTap: c.note ? null : () => _tap(c),
        child: Padding(
          padding: const EdgeInsets.symmetric(horizontal: _pad),
          child: Center(child: Text(_text(c), style: _style(c), maxLines: 1, softWrap: false)),
        ),
      );
    }

    Widget arrow(IconData icon, VoidCallback onTap) => _Press(
          look: look,
          onTap: onTap,
          child: SizedBox(width: _arrow, child: Icon(icon, size: 20, color: look.text)),
        );

    final row = <Widget>[
      if (back)
        arrow(Icons.chevron_left, () => setState(() {
              if (page > 0) {
                _page = page - 1;
              } else {
                _path.removeLast();
                _page = 0;
              }
            })),
      if (back) Container(width: 0.5, margin: const EdgeInsets.symmetric(vertical: 11), color: line),
      for (final c in shown) cell(c),
      if (next) Container(width: 0.5, margin: const EdgeInsets.symmetric(vertical: 11), color: line),
      if (next) arrow(Icons.chevron_right, () => setState(() => _page = page + 1)),
    ];
    var w = shown.fold<double>(0, (w, c) => w + _width(c)) + (back ? _arrow + 0.5 : 0) + (next ? _arrow + 0.5 : 0);
    w = (w.ceilToDouble() + 1).clamp(0, b.width);

    // above the anchor; below it when there's no room above; otherwise over its middle
    const h = EditMenu.height;
    final a = widget.anchor;
    double top;
    if (a.top - _gap - h >= b.top) {
      top = a.top - _gap - h;
    } else if (a.bottom + _gap + h <= b.bottom) {
      top = a.bottom + _gap;
    } else {
      top = (a.center.dy - h / 2).clamp(b.top, b.bottom - h);
    }
    final left = (a.center.dx - w / 2).clamp(b.left, b.right - w);

    return Stack(children: [
      Positioned(
        left: left,
        top: top,
        width: w,
        height: h,
        child: Container(
          decoration: BoxDecoration(
            color: look.light ? look.field : look.control,
            borderRadius: BorderRadius.circular(h / 2),
            border: Border.all(color: look.windowStroke.withValues(alpha: look.light ? 0.5 : 0.8), width: 0.5),
            boxShadow: [BoxShadow(color: Color.fromARGB(look.light ? 40 : 110, 0, 0, 0), blurRadius: 16, offset: const Offset(0, 4))],
          ),
          clipBehavior: Clip.antiAlias,
          child: Row(mainAxisSize: MainAxisSize.min, crossAxisAlignment: CrossAxisAlignment.stretch, children: row),
        ),
      ),
    ]);
  }
}

/// A cell that darkens while pressed.
class _Press extends StatefulWidget {
  const _Press({required this.look, required this.onTap, required this.child});
  final Look look;
  final VoidCallback? onTap;
  final Widget child;

  @override
  State<_Press> createState() => _PressState();
}

class _PressState extends State<_Press> {
  bool _down = false;

  @override
  Widget build(BuildContext context) => GestureDetector(
        behavior: HitTestBehavior.opaque,
        onTapDown: widget.onTap == null ? null : (_) => setState(() => _down = true),
        onTapCancel: () => setState(() => _down = false),
        onTapUp: (_) => setState(() => _down = false),
        onTap: widget.onTap,
        child: ColoredBox(color: _down ? widget.look.hover : Colors.transparent, child: widget.child),
      );
}
