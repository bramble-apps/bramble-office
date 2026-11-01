// Edit > Find / Find & Replace, as Pages on iPad and iPhone: the bar sits at the foot of the
// pages, just above the on-screen keyboard (the Scaffold shrinks the pages over it, so the
// match pg-app find.rs scrolls to stays in sight). The gear's Search Options choose Find or
// Find & Replace, Whole Words and Match Case; the search field shows "2 of 5"; ⌃ ⌄ step
// through the matches; Replace replaces one, touch and hold it for Replace All; Done closes.
// Wide (a tablet): one row, the replace field beside the search field. Narrow (a phone): the
// replace field and Replace in a second row under the search field (also any phone-layout window).
import 'package:flutter/material.dart';

import '../engine.dart';
import 'icons.dart';
import 'look.dart';
import 'widgets.dart';

class FindBar extends StatefulWidget {
  const FindBar({super.key, required this.engine, required this.s, required this.onChanged, required this.onMenu, this.compact = false});
  final Engine engine;

  /// lt_status find: {query, count, cur, case, words, replace}.
  final Map<String, dynamic> s;
  final VoidCallback onChanged;

  /// A popover menu at a global point (main.dart _menuAt; items' actions go to the engine).
  final void Function(List<dynamic> items, Offset at) onMenu;

  /// The phone layout (a phone on its side too): the iPhone's two rows at any width.
  final bool compact;
  @override
  State<FindBar> createState() => _FindBarState();
}

class _FindBarState extends State<FindBar> {
  late final _q = TextEditingController(text: widget.s['query'] as String? ?? '');
  late final _with = TextEditingController(text: widget.s['replace'] as String? ?? '');
  final _focus = FocusNode();
  final _withFocus = FocusNode();

  /// Touch targets: 44 points, as iOS.
  static const _tap = 34.0;

  @override
  void initState() {
    super.initState();
    WidgetsBinding.instance.addPostFrameCallback((_) => _focus.requestFocus());
  }

  @override
  void didUpdateWidget(FindBar old) {
    super.didUpdateWidget(old);
    // the engine's text wins when it changed there (Find & Replace reopened empty, say)
    final q = widget.s['query'] as String? ?? '';
    if (q != old.s['query'] && q != _q.text) _q.text = q;
    final w = widget.s['replace'] as String?;
    if (w != null && w != old.s['replace'] && w != _with.text) _with.text = w;
  }

  @override
  void dispose() {
    _q.dispose();
    _with.dispose();
    _focus.dispose();
    _withFocus.dispose();
    super.dispose();
  }

  void act(String a) {
    widget.engine.action('find:$a');
    widget.onChanged();
  }

  bool get _touch => const {TargetPlatform.android, TargetPlatform.iOS}.contains(Theme.of(context).platform);

  /// A popover menu opening upward from `ctx`'s widget (the bar is at the foot of the screen):
  /// menubar.dart's rows are 30 tall, separators 13, the menu's padding 6.
  void _menuAbove(BuildContext ctx, List<Map<String, dynamic>> items) {
    final box = ctx.findRenderObject() as RenderBox;
    final h = 12 + items.fold<double>(0, (a, i) => a + (i['separator'] == true ? 13 : 30));
    widget.onMenu(items, box.localToGlobal(Offset(0, -h - 4)));
  }

  /// The gear: Search Options.
  void _options(BuildContext ctx) {
    final s = widget.s;
    final rep = s['replace'] != null;
    final words = s['words'] == true, cas = s['case'] == true;
    _menuAbove(ctx, [
      {'label': 'Find', 'action': 'find:replaceClose', 'enabled': true, 'checked': !rep},
      {'label': 'Find & Replace', 'action': 'find:replaceOpen', 'enabled': true, 'checked': rep},
      {'separator': true},
      {'label': 'Whole Words', 'action': 'find:words:${words ? 0 : 1}', 'enabled': true, 'checked': words},
      {'label': 'Match Case', 'action': 'find:case:${cas ? 0 : 1}', 'enabled': true, 'checked': cas},
    ]);
  }

  /// An icon button, 44 by 44.
  Widget _icon(Widget icon, VoidCallback? onTap, String tip) {
    final look = LookScope.of(context);
    return SizedBox(
      width: _tap,
      height: _tap,
      child: Material(
        type: MaterialType.transparency,
        child: InkResponse(
          onTap: onTap,
          radius: 20,
          highlightColor: look.hover,
          child: Semantics(label: tip, button: true, child: Opacity(opacity: onTap == null ? 0.4 : 1, child: Center(child: icon))),
        ),
      ),
    );
  }

  /// A rounded field, 28 tall in a 34-point row; the search one has the magnifier and the match
  /// count. Both have a clear button.
  Widget _field(TextEditingController c, String hint, FocusNode focus, void Function(String) onChanged, {String? count, bool search = false, void Function(String)? onSubmitted}) {
    final look = LookScope.of(context);
    return SizedBox(
      height: _tap,
      child: Center(
        child: Container(
          height: 28,
          padding: EdgeInsets.only(left: search ? 8 : 12, right: 2),
          decoration: BoxDecoration(color: look.field, borderRadius: BorderRadius.circular(10), border: Border.all(color: look.fieldStroke)),
          child: Row(children: [
            if (search) ...[DesktopIcon(DIcon.search, size: 16, color: look.textDim), const SizedBox(width: 6)],
            Expanded(
              child: TextField(
                controller: c,
                focusNode: focus,
                style: Look.font(13, look.text),
                cursorColor: look.accent,
                textInputAction: search ? TextInputAction.search : TextInputAction.done,
                decoration: InputDecoration(isDense: true, border: InputBorder.none, hintText: hint, hintStyle: Look.font(13, look.textDim), contentPadding: const EdgeInsets.symmetric(vertical: 6)),
                onChanged: onChanged,
                onSubmitted: onSubmitted,
              ),
            ),
            if (count != null) Padding(padding: const EdgeInsets.only(left: 4), child: Text(count, style: Look.font(13, look.textDim))),
            ListenableBuilder(
              listenable: c,
              builder: (_, _) => c.text.isEmpty
                  ? const SizedBox(width: 8)
                  : GestureDetector(
                      behavior: HitTestBehavior.opaque,
                      onTap: () {
                        c.clear();
                        onChanged('');
                        focus.requestFocus();
                      },
                      child: SizedBox(width: 28, height: 28, child: Icon(Icons.cancel, size: 15, color: look.textDim)),
                    ),
            ),
          ]),
        ),
      ),
    );
  }

  /// A text button in the accent colour, 44 tall.
  Widget _button(String label, VoidCallback? onTap, {GestureLongPressStartCallback? onLongPress, bool bold = false}) {
    final look = LookScope.of(context);
    return GestureDetector(
      behavior: HitTestBehavior.opaque,
      onTap: onTap,
      onLongPressStart: onLongPress,
      child: Opacity(
        opacity: onTap == null ? 0.4 : 1,
        child: Container(
          height: _tap,
          constraints: const BoxConstraints(minWidth: _tap),
          padding: const EdgeInsets.symmetric(horizontal: 10),
          alignment: Alignment.center,
          child: Text(label, style: Look.font(13, look.accent, bold: bold)),
        ),
      ),
    );
  }

  @override
  Widget build(BuildContext context) {
    final look = LookScope.of(context);
    final s = widget.s;
    final q = s['query'] as String? ?? '';
    final n = s['count'] as int? ?? 0;
    final cur = s['cur'] as int? ?? 0;
    final rep = s['replace'] as String?;
    final count = q.isEmpty ? null : n == 0 ? 'Not found' : '${cur.clamp(0, n - 1) + 1} of $n';

    final gear = Builder(builder: (ctx) => _icon(Icon(Icons.settings_outlined, size: 18, color: look.text), () => _options(ctx), 'Search Options'));
    final find = _field(_q, 'Search', _focus, (t) => act('q:$t'), count: count, search: true, onSubmitted: (_) {
      act('step:1');
      // (Return on a keyboard steps on and keeps typing here; a phone's Search key hides the
      // keyboard to show the matches, as Pages on iPhone)
      if (!_touch) _focus.requestFocus();
    });
    final arrows = [
      _icon(RotatedBox(quarterTurns: 2, child: DesktopIcon(DIcon.chevronDown, size: 16, color: look.text)), n == 0 ? null : () => act('step:0'), 'Previous'),
      _icon(DesktopIcon(DIcon.chevronDown, size: 16, color: look.text), n == 0 ? null : () => act('step:1'), 'Next'),
    ];
    final done = _button('Done', () => act('close'), bold: true);
    final withField = rep == null ? null : _field(_with, 'Replace', _withFocus, (t) => act('with:$t'));
    final replace = Builder(
        builder: (ctx) => _button('Replace', n == 0 ? null : () => act('replace'),
            onLongPress: n == 0 ? null : (_) => _menuAbove(ctx, [{'label': 'Replace All', 'action': 'find:replaceAll', 'enabled': true}])));

    return LayoutBuilder(builder: (context, c) {
      final wide = !widget.compact && c.maxWidth >= 600;
      return Container(
        decoration: BoxDecoration(color: look.panel, border: Border(top: BorderSide(color: look.separator))),
        // (no keyboard up: clear of the phone's gesture bar)
        padding: EdgeInsets.fromLTRB(6, 2, 6, 2 + MediaQuery.paddingOf(context).bottom),
        child: wide
            ? Row(children: [
                gear,
                Expanded(child: find),
                if (withField != null) ...[const SizedBox(width: 8), Expanded(child: withField), replace],
                ...arrows,
                done,
              ])
            : Column(mainAxisSize: MainAxisSize.min, children: [
                Row(children: [gear, Expanded(child: find), ...arrows, done]),
                if (withField != null) Row(children: [const SizedBox(width: _tap), Expanded(child: withField), replace]),
              ]),
      );
    });
  }
}
