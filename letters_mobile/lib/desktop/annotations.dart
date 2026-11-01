// Comments & Changes (pg-app notes.rs): the annotation bar under the toolbar (the comments
// pane button, ‹ › through comments and changes, "N Comments & Changes", + Comment, Highlight,
// Accept / Reject and the all-changes menu, the Track Changes switch) and the comments pane at
// the right of the pages (View > Show Comments Pane).
import 'package:flutter/material.dart';

import '../engine.dart';
import 'look.dart';
import 'widgets.dart';

/// notes.rs annotation_bar: 30 tall, the panel colour, 10 × 3 inside.
class AnnotationsBar extends StatelessWidget {
  const AnnotationsBar({super.key, required this.engine, required this.s, required this.onChanged});
  final Engine engine;

  /// lt_status notes.bar: {pane, label, canComment, canHighlight, review, here, tracking}.
  final Map<String, dynamic> s;
  final VoidCallback onChanged;

  void _act(String a) {
    engine.action(a);
    onChanged();
  }

  @override
  Widget build(BuildContext context) {
    final look = LookScope.of(context);
    bool b(String k) => s[k] == true;
    // ui.separator() in a row: 8 of spacing either side of its 6
    Widget sep() => SizedBox(width: 22, height: 24, child: Center(child: Container(width: 1, color: look.separator)));
    const gap = SizedBox(width: 8);
    return Container(
      height: 30,
      decoration: BoxDecoration(color: look.panel, border: Border(bottom: BorderSide(color: look.separator, width: 1))),
      padding: const EdgeInsets.symmetric(horizontal: 10, vertical: 3),
      child: Row(children: [
        DButton(onTap: () => _act('notes:pane'), child: Text(b('pane') ? '◧' : '▢')),
        gap,
        DButton(onTap: () => _act('notes:step:0'), child: const Text('‹')),
        gap,
        DButton(onTap: () => _act('notes:step:1'), child: const Text('›')),
        gap,
        Text(s['label'] as String? ?? '', style: Look.font(13, look.textDim)),
        sep(),
        DButton(onTap: b('canComment') ? () => _act('cmd:Comment') : null, child: const Text('+ Comment')),
        gap,
        DButton(onTap: b('canHighlight') ? () => _act('cmd:Highlight') : null, child: const Text('Highlight')),
        if (b('review')) ...[
          sep(),
          DButton(onTap: b('here') ? () => _act('notes:accept') : null, child: const Text('Accept')),
          gap,
          DButton(onTap: b('here') ? () => _act('notes:reject') : null, child: const Text('Reject')),
          gap,
          Builder(
            builder: (ctx) => DButton(
              onTap: () async {
                final box = ctx.findRenderObject() as RenderBox;
                final at = box.localToGlobal(Offset.zero) & box.size;
                final v = await showDesktopList<String>(ctx, at, const [DChoice('Accept All Changes', 'acceptAll'), DChoice('Reject All Changes', 'rejectAll')], width: 150);
                if (v != null) _act('notes:$v');
              },
              child: const Text('⌄'),
            ),
          ),
        ],
        const Spacer(),
        Text('Track Changes', style: Look.font(13, look.text)),
        gap,
        DToggle(on: b('tracking'), onChanged: (v) => _act('notes:track:${v ? 1 : 0}')),
      ]),
    );
  }
}

/// notes.rs comments_pane: 260 wide, the sidebar's colour, 10 inside; a card for each comment
/// and change (tap one to go to its text).
class CommentsPane extends StatelessWidget {
  const CommentsPane({super.key, required this.engine, required this.s, required this.onChanged});
  final Engine engine;

  /// lt_status notes.pane: {cards: [{who, color, date, quote, what, comment}]}.
  final Map<String, dynamic> s;
  final VoidCallback onChanged;

  void _act(String a) {
    engine.action(a);
    onChanged();
  }

  @override
  Widget build(BuildContext context) {
    final look = LookScope.of(context);
    final cards = s['cards'] as List? ?? const [];
    Widget small(String t, {bool italic = false}) => Text(t, style: Look.font(10, look.textDim).copyWith(height: 1.3, fontStyle: italic ? FontStyle.italic : null));
    Widget smallButton(String t, VoidCallback onTap) => GestureDetector(
          onTap: onTap,
          child: Container(
            height: 17,
            padding: const EdgeInsets.symmetric(horizontal: 9),
            decoration: BoxDecoration(color: look.control, borderRadius: BorderRadius.circular(6), border: Border.all(color: look.controlStroke)),
            alignment: Alignment.center,
            child: Text(t, style: Look.font(13, look.text)),
          ),
        );
    return Container(
      width: 260,
      decoration: BoxDecoration(color: look.sidebar, border: Border(left: BorderSide(color: look.separator))),
      padding: const EdgeInsets.all(10),
      child: Column(crossAxisAlignment: CrossAxisAlignment.stretch, children: [
        DLabel('Comments & Changes', size: 14, bold: true),
        const SizedBox(height: 6),
        const DSeparator(),
        const SizedBox(height: 6),
        Expanded(
          child: ListView(padding: EdgeInsets.zero, children: [
            if (cards.isEmpty) DLabel('No comments or changes in this document.', dim: true),
            for (var i = 0; i < cards.length; i++) ...[
              GestureDetector(
                behavior: HitTestBehavior.opaque,
                onTap: () => _act('notes:go:$i'),
                child: Container(
                  padding: const EdgeInsets.all(8),
                  decoration: BoxDecoration(color: look.gray(0x30), borderRadius: BorderRadius.circular(8)),
                  child: EguiColumn(children: [
                    Row(children: [
                      SizedBox(width: 10, height: 10, child: Center(child: Container(width: 9, height: 9, decoration: BoxDecoration(color: colorOf(cards[i]['color']), shape: BoxShape.circle)))),
                      const SizedBox(width: 8),
                      Flexible(child: SizedBox(height: 22, child: Align(alignment: Alignment.centerLeft, widthFactor: 1, child: DLabel(cards[i]['who'] as String? ?? '', size: 14, bold: true)))),
                      if (cards[i]['date'] != null) ...[const SizedBox(width: 8), small(cards[i]['date'] as String)],
                    ]),
                    if (cards[i]['quote'] != null) small('“${cards[i]['quote']}”', italic: true),
                    Text(cards[i]['what'] as String? ?? '', style: Look.font(13, look.text).copyWith(height: 1.3)),
                    if (cards[i]['comment'] != null)
                      Row(children: [
                        smallButton('Edit', () => _act('notes:edit:${cards[i]['comment']}')),
                        const SizedBox(width: 8),
                        smallButton('Delete', () => _act('notes:delete:${cards[i]['comment']}')),
                      ]),
                  ]),
                ),
              ),
              const SizedBox(height: 12),
            ],
          ]),
        ),
      ]),
    );
  }
}
