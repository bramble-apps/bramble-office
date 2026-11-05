// Formatting: the Format sheet (Aa), non-modal, in the keyboard's place (iPad: under the
// toolbar's Aa), so the text can still be selected while it's open: the paragraph styles as
// chips shown in their own type, B I U S, the highlight and its colour, the lists, indents and
// block quote. And the keyboard's accessory bar (iPhone): a glass capsule that scrolls
// sideways and changes with the selection (nothing selected: Aa and the tools; a selection
// on one line: B I U S, highlight, link, lists; across lines: outdent, indent, move up and
// down, block quote, lists).
import 'package:flutter/cupertino.dart';
import 'package:flutter/material.dart';

import 'doc.dart';
import 'theme.dart';
import 'ui/glass.dart';
import 'ui/touch_menu.dart';

const highlightNames = ['Purple', 'Pink', 'Orange', 'Mint', 'Blue'];

/// The paragraph styles' fonts (Notes: Title 28 bold, Heading 22 bold, Subheading 17 bold, Body
/// 17, Monostyled 15.5 mono), scaled with the text size setting.
TextStyle paraFont(String st, double textSize, Color color) {
  final k = textSize / 17;
  return switch (st) {
    'title' => TextStyle(fontFamily: 'SferaDisplay', fontSize: 28 * k, fontWeight: FontWeight.w700, height: 1.22, letterSpacing: 0.38, color: color),
    'heading' => TextStyle(fontFamily: 'SferaDisplay', fontSize: 22 * k, fontWeight: FontWeight.w700, height: 1.27, letterSpacing: -0.26, color: color),
    'subheading' => TextStyle(fontFamily: 'Sfera', fontSize: 17 * k, fontWeight: FontWeight.w700, height: 1.3, letterSpacing: -0.43, color: color),
    'mono' => TextStyle(fontFamily: 'SferaMono', fontSize: 15.5 * k, height: 1.4, color: color),
    _ => TextStyle(fontFamily: 'Sfera', fontSize: 17 * k, height: 1.32, letterSpacing: -0.43, color: color),
  };
}

class FormatPanel extends StatelessWidget {
  const FormatPanel({super.key, required this.c, required this.onClose, this.popover = false});
  final NoteController c;
  final VoidCallback onClose;

  /// iPad: shown as a popover (no gaps to the screen's edges to keep).
  final bool popover;

  @override
  Widget build(BuildContext context) {
    final p = Palette.of(context);
    return ListenableBuilder(
      listenable: c,
      builder: (ctx, _) {
        final para = c.currentPara;
        final cs = c.currentStyle;
        Widget seg(List<(IconData, bool, VoidCallback, String)> items) => Container(
          height: 48,
          decoration: BoxDecoration(color: p.fill, borderRadius: BorderRadius.circular(24)),
          padding: const EdgeInsets.all(3),
          child: Row(
            mainAxisSize: MainAxisSize.min,
            children: [
              for (final (icon, on, tap, label) in items)
                Semantics(
                  button: true,
                  label: label,
                  selected: on,
                  child: Pressable(
                    onTap: tap,
                    child: Container(
                      width: 48,
                      height: 42,
                      decoration: BoxDecoration(color: on ? p.accent : null, borderRadius: BorderRadius.circular(21)),
                      child: Icon(icon, size: 21, color: on ? Colors.white : p.label),
                    ),
                  ),
                ),
            ],
          ),
        );
        final hl = cs.hl;
        return Container(
          margin: popover ? EdgeInsets.zero : const EdgeInsets.fromLTRB(8, 0, 8, 8),
          decoration: BoxDecoration(
            color: p.sheetCard,
            borderRadius: BorderRadius.circular(popover ? 26 : 34),
            boxShadow: [BoxShadow(color: p.glassShadow, blurRadius: 24, offset: const Offset(0, 6))],
          ),
          padding: const EdgeInsets.fromLTRB(0, 4, 0, 16),
          child: Column(
            mainAxisSize: MainAxisSize.min,
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              Row(
                children: [
                  const SizedBox(width: 20),
                  Expanded(
                    child: Text('Format', style: Type.text(17, p.label, weight: FontWeight.w700)),
                  ),
                  Pressable(
                    onTap: onClose,
                    child: SizedBox.square(
                      dimension: 52,
                      child: Center(
                        child: Container(
                          width: 34,
                          height: 34,
                          decoration: BoxDecoration(color: p.fill, shape: BoxShape.circle),
                          child: Icon(CupertinoIcons.xmark, size: 17, color: p.secondary),
                        ),
                      ),
                    ),
                  ),
                  const SizedBox(width: 6),
                ],
              ),
              // the styles, in their own type
              SizedBox(
                height: 60,
                child: ListView(
                  scrollDirection: Axis.horizontal,
                  padding: const EdgeInsets.symmetric(horizontal: 14),
                  children: [
                    for (final (st, name) in [
                      ('title', 'Title'),
                      ('heading', 'Heading'),
                      ('subheading', 'Subheading'),
                      ('body', 'Body'),
                      ('mono', 'Monostyled'),
                    ])
                      _Visible(
                        // (the chosen style scrolled into view)
                        show: para.st == st,
                        child: Padding(
                          padding: const EdgeInsets.only(right: 6),
                          child: Pressable(
                            onTap: () => c.paragraphStyle(st),
                            child: Container(
                              padding: const EdgeInsets.symmetric(horizontal: 14),
                              alignment: Alignment.center,
                              decoration: BoxDecoration(color: para.st == st ? p.accent : null, borderRadius: BorderRadius.circular(30)),
                              child: Text(name, style: paraFont(st, 17, para.st == st ? Colors.white : p.label).copyWith(height: 1.0)),
                            ),
                          ),
                        ),
                      ),
                  ],
                ),
              ),
              const SizedBox(height: 10),
              SingleChildScrollView(
                scrollDirection: Axis.horizontal,
                padding: const EdgeInsets.symmetric(horizontal: 16),
                child: Row(
                  children: [
                    seg([
                      (CupertinoIcons.bold, cs.b, () => c.toggle('b'), 'Bold'),
                      (CupertinoIcons.italic, cs.i, () => c.toggle('i'), 'Italic'),
                      (CupertinoIcons.underline, cs.u, () => c.toggle('u'), 'Underline'),
                      (CupertinoIcons.strikethrough, cs.s, () => c.toggle('s'), 'Strikethrough'),
                    ]),
                    const SizedBox(width: 12),
                    Container(
                      height: 48,
                      decoration: BoxDecoration(color: p.fill, borderRadius: BorderRadius.circular(24)),
                      padding: const EdgeInsets.all(3),
                      child: Row(
                        children: [
                          Pressable(
                            onTap: () => c.highlight(hl == null ? _lastHighlight : null),
                            child: Container(
                              width: 48,
                              height: 42,
                              decoration: BoxDecoration(color: hl != null ? p.accent : null, borderRadius: BorderRadius.circular(21)),
                              child: Icon(CupertinoIcons.pencil_outline, size: 21, color: hl != null ? Colors.white : p.label),
                            ),
                          ),
                          Builder(
                            builder: (bctx) => Pressable(
                              onTap: () => showTouchMenu(
                                context,
                                p.look,
                                globalRect(bctx),
                                [
                                  for (var k = 1; k <= 5; k++) {'label': highlightNames[k - 1], 'k': k, 'checked': (hl ?? _lastHighlight) == k},
                                ],
                                (m) {
                                  _lastHighlight = m['k'] as int;
                                  c.highlight(_lastHighlight);
                                },
                              ),
                              child: SizedBox(
                                width: 48,
                                height: 42,
                                child: Center(
                                  child: Container(
                                    width: 22,
                                    height: 22,
                                    decoration: BoxDecoration(shape: BoxShape.circle, color: p.highlightText(hl ?? _lastHighlight)),
                                  ),
                                ),
                              ),
                            ),
                          ),
                        ],
                      ),
                    ),
                  ],
                ),
              ),
              const SizedBox(height: 12),
              SingleChildScrollView(
                scrollDirection: Axis.horizontal,
                padding: const EdgeInsets.symmetric(horizontal: 16),
                child: Row(
                  children: [
                    seg([
                      (CupertinoIcons.list_bullet, para.st == 'bullet', () => c.paragraphStyle('bullet'), 'Bulleted List'),
                      (CupertinoIcons.list_dash, para.st == 'dash', () => c.paragraphStyle('dash'), 'Dashed List'),
                      (CupertinoIcons.list_number, para.st == 'number', () => c.paragraphStyle('number'), 'Numbered List'),
                    ]),
                    const SizedBox(width: 12),
                    seg([
                      (CupertinoIcons.decrease_indent, false, () => c.indent(-1), 'Decrease Indent'),
                      (CupertinoIcons.increase_indent, false, () => c.indent(1), 'Increase Indent'),
                    ]),
                    const SizedBox(width: 12),
                    seg([(CupertinoIcons.text_quote, para.q, c.blockQuote, 'Block Quote')]),
                  ],
                ),
              ),
            ],
          ),
        );
      },
    );
  }
}

/// The highlight colour last chosen (Purple at first).
int _lastHighlight = 1;

/// The keyboard's accessory bar.
class KeyboardBar extends StatelessWidget {
  const KeyboardBar({
    super.key,
    required this.c,
    required this.onFormat,
    required this.onTable,
    required this.onAttach,
    required this.onMarkup,
    required this.onLink,
  });
  final NoteController c;
  final VoidCallback onFormat, onTable, onMarkup, onLink;
  final void Function(BuildContext context) onAttach;

  @override
  Widget build(BuildContext context) {
    final p = Palette.of(context);
    return ListenableBuilder(
      listenable: c,
      builder: (ctx, _) {
        final sel = c.selection;
        final (a, b) = c.selectedParas();
        final cs = c.currentStyle;
        final para = c.currentPara;
        final items = <(IconData, bool, void Function(BuildContext), String)>[];
        void add(IconData i, bool on, void Function(BuildContext) f, String l) => items.add((i, on, f, l));
        if (sel.isValid && !sel.isCollapsed && a != b) {
          add(CupertinoIcons.decrease_indent, false, (_) => c.indent(-1), 'Decrease Indent');
          add(CupertinoIcons.increase_indent, false, (_) => c.indent(1), 'Increase Indent');
          add(CupertinoIcons.text_quote, para.q, (_) => c.blockQuote(), 'Block Quote');
          add(CupertinoIcons.list_bullet, para.st == 'bullet', (_) => c.paragraphStyle('bullet'), 'Bulleted List');
          add(CupertinoIcons.checkmark_circle, para.st == 'check', (_) => c.checklist(), 'Checklist');
        } else if (sel.isValid && !sel.isCollapsed) {
          add(CupertinoIcons.textformat, false, (_) => onFormat(), 'Format');
          add(CupertinoIcons.bold, cs.b, (_) => c.toggle('b'), 'Bold');
          add(CupertinoIcons.italic, cs.i, (_) => c.toggle('i'), 'Italic');
          add(CupertinoIcons.underline, cs.u, (_) => c.toggle('u'), 'Underline');
          add(CupertinoIcons.strikethrough, cs.s, (_) => c.toggle('s'), 'Strikethrough');
          add(CupertinoIcons.pencil_outline, cs.hl != null, (_) => c.highlight(cs.hl == null ? _lastHighlight : null), 'Highlight');
          add(CupertinoIcons.link, false, (_) => onLink(), 'Add Link');
          add(CupertinoIcons.list_bullet, para.st == 'bullet', (_) => c.paragraphStyle('bullet'), 'Bulleted List');
        } else {
          add(CupertinoIcons.textformat, false, (_) => onFormat(), 'Format');
          add(CupertinoIcons.checkmark_circle, para.st == 'check', (_) => c.checklist(), 'Checklist');
          add(CupertinoIcons.table, false, (_) => onTable(), 'Table');
          add(CupertinoIcons.paperclip, false, onAttach, 'Attach');
          add(markupIcon, false, (_) => onMarkup(), 'Markup');
          add(CupertinoIcons.bold, cs.b, (_) => c.toggle('b'), 'Bold');
          add(CupertinoIcons.italic, cs.i, (_) => c.toggle('i'), 'Italic');
          add(CupertinoIcons.underline, cs.u, (_) => c.toggle('u'), 'Underline');
          add(CupertinoIcons.strikethrough, cs.s, (_) => c.toggle('s'), 'Strikethrough');
          add(CupertinoIcons.list_bullet, para.st == 'bullet', (_) => c.paragraphStyle('bullet'), 'Bulleted List');
          add(CupertinoIcons.decrease_indent, false, (_) => c.indent(-1), 'Decrease Indent');
          add(CupertinoIcons.increase_indent, false, (_) => c.indent(1), 'Increase Indent');
        }
        return Glass(
          radius: 26,
          child: SizedBox(
            height: 52,
            child: SingleChildScrollView(
              scrollDirection: Axis.horizontal,
              padding: const EdgeInsets.symmetric(horizontal: 4),
              child: Row(
                children: [
                  for (final (icon, on, f, label) in items)
                    Builder(
                      builder: (bctx) => Semantics(
                        button: true,
                        label: label,
                        selected: on,
                        child: Pressable(
                          onTap: () => f(bctx),
                          child: SizedBox(
                            width: 50,
                            height: 52,
                            child: Center(
                              child: Container(
                                width: 42,
                                height: 42,
                                decoration: BoxDecoration(color: on ? p.accent : null, shape: BoxShape.circle),
                                child: glyphIcon(icon, 22, on ? Colors.white : p.label),
                              ),
                            ),
                          ),
                        ),
                      ),
                    ),
                ],
              ),
            ),
          ),
        );
      },
    );
  }
}

/// Brings its child into view in the scrolling row once it's shown chosen.
class _Visible extends StatefulWidget {
  const _Visible({required this.show, required this.child});
  final bool show;
  final Widget child;
  @override
  State<_Visible> createState() => _VisibleState();
}

class _VisibleState extends State<_Visible> {
  @override
  void initState() {
    super.initState();
    _reveal();
  }

  @override
  void didUpdateWidget(_Visible old) {
    super.didUpdateWidget(old);
    if (widget.show && !old.show) _reveal();
  }

  void _reveal() {
    if (!widget.show) return;
    WidgetsBinding.instance.addPostFrameCallback((_) {
      if (mounted) Scrollable.ensureVisible(context, alignment: 0.5, duration: const Duration(milliseconds: 200));
    });
  }

  @override
  Widget build(BuildContext context) => widget.child;
}
