// New Smart Folder / Edit Smart Folder (Notes' sheet): a name, Include notes matching All or Any
// of the filters, and the filters themselves (Tags, Date Created, Date Edited, Checklists,
// Attachments, Folders, Quick Notes, Pinned Notes, Locked Notes), as nt-core stores them.
import 'package:flutter/cupertino.dart';
import 'package:flutter/material.dart';

import 'model.dart';
import 'state.dart';
import 'theme.dart';
import 'ui/glass.dart';
import 'ui/sheet.dart';
import 'ui/touch_menu.dart';

const _filters = ['Tags', 'Date Created', 'Date Edited', 'Checklists', 'Attachments', 'Folders', 'Quick Notes', 'Pinned Notes', 'Locked Notes'];
const _ranges = ['Today', 'Yesterday', 'Past 7 Days', 'Past 30 Days', 'Past 3 Months', 'Past Year'];

Future<void> editSmartFolder(BuildContext context, AppState s, {Folder? folder, String name = ''}) async {
  var any = false;
  var rules = <(int, String)>[];
  if (folder?.query != null) {
    final q = s.call('smart_parse', {'json': folder!.query}) as Map?;
    any = q?['any'] == true;
    rules = [for (final r in (q?['rules'] as List? ?? const [])) ((r as List)[0] as int, r[1] as String)];
  }
  if (rules.isEmpty) rules = [(0, '')];
  await showSheet<void>(
    context,
    title: folder == null ? 'New Smart Folder' : 'Edit Smart Folder',
    builder: (ctx) => _SmartSheet(state: s, folder: folder, name: folder?.name ?? name, any: any, rules: rules),
  );
}

class _SmartSheet extends StatefulWidget {
  const _SmartSheet({required this.state, required this.folder, required this.name, required this.any, required this.rules});
  final AppState state;
  final Folder? folder;
  final String name;
  final bool any;
  final List<(int, String)> rules;
  @override
  State<_SmartSheet> createState() => _SmartSheetState();
}

class _SmartSheetState extends State<_SmartSheet> {
  late final _name = TextEditingController(text: widget.name);
  late bool _any = widget.any;
  late final _rules = [...widget.rules];

  @override
  void dispose() {
    _name.dispose();
    super.dispose();
  }

  Future<void> _save() async {
    final s = widget.state;
    final name = _name.text.trim();
    if (name.isEmpty && widget.folder == null) return;
    final rules = [
      for (final r in _rules) [r.$1, r.$2],
    ];
    if (widget.folder == null) {
      s.call('create_smart', {'name': name, 'any': _any, 'rules': rules});
    } else {
      if (name.isNotEmpty && name != widget.folder!.name) s.call('rename_folder', {'id': widget.folder!.id, 'name': name});
      s.call('set_smart', {'id': widget.folder!.id, 'any': _any, 'rules': rules});
    }
    await s.reload();
    if (mounted) Navigator.of(context).pop();
  }

  String _arg((int, String) r) => switch (r.$1) {
    0 => r.$2.isEmpty ? 'Any Tag' : '#${r.$2}',
    1 || 2 => _ranges[(int.tryParse(r.$2) ?? 2).clamp(0, _ranges.length - 1)],
    5 => widget.state.lib.folder(r.$2)?.name ?? 'Choose',
    _ => '',
  };

  void _pickKind(BuildContext bctx, int i) {
    final p = Palette.of(context);
    showTouchMenu(context, p.look, globalRect(bctx), [
      for (final (k, f) in _filters.indexed) {'label': f, 'k': k, 'checked': _rules[i].$1 == k},
    ], (m) => setState(() => _rules[i] = (m['k'] as int, (m['k'] == 1 || m['k'] == 2) ? '2' : '')));
  }

  Future<void> _pickArg(BuildContext bctx, int i) async {
    final p = Palette.of(context);
    final s = widget.state;
    final r = _rules[i];
    if (r.$1 == 1 || r.$1 == 2) {
      showTouchMenu(context, p.look, globalRect(bctx), [
        for (final (k, l) in _ranges.indexed) {'label': l, 'k': k, 'checked': r.$2 == '$k'},
      ], (m) => setState(() => _rules[i] = (r.$1, '${m['k']}')));
    } else if (r.$1 == 5) {
      showTouchMenu(context, p.look, globalRect(bctx), [
        for (final f in s.lib.folders.where((f) => !f.smart && !f.trash)) {'label': f.name, 'id': f.id, 'checked': r.$2 == f.id},
      ], (m) => setState(() => _rules[i] = (5, m['id'] as String)));
    } else if (r.$1 == 0) {
      final tags = s.lib.allTags;
      showTouchMenu(context, p.look, globalRect(bctx), [
        {'label': 'Any Tag', 'id': '', 'checked': r.$2.isEmpty},
        for (final t in tags) {'label': '#$t', 'id': t, 'checked': r.$2.toLowerCase() == t.toLowerCase()},
      ], (m) => setState(() => _rules[i] = (0, m['id'] as String)));
    }
  }

  @override
  Widget build(BuildContext context) {
    final p = Palette.of(context);
    return ListView(
      padding: const EdgeInsets.only(top: 4),
      children: [
        Card2(
          children: [
            Padding(
              padding: const EdgeInsets.symmetric(horizontal: 16),
              child: SizedBox(
                height: 52,
                child: Center(
                  child: TextField(
                    controller: _name,
                    style: Type.body(p.label),
                    cursorColor: p.accent,
                    decoration: InputDecoration.collapsed(hintText: 'Name', hintStyle: Type.body(p.tertiary)),
                  ),
                ),
              ),
            ),
          ],
        ),
        Card2(
          header: 'Include notes matching',
          children: [
            CardRow(label: 'All Filters', checked: !_any, onTap: () => setState(() => _any = false)),
            CardRow(label: 'Any Filter', checked: _any, onTap: () => setState(() => _any = true)),
          ],
        ),
        Card2(
          header: 'Filters',
          children: [
            for (final (i, r) in _rules.indexed)
              SizedBox(
                height: 52,
                child: Row(
                  children: [
                    const SizedBox(width: 6),
                    Pressable(
                      onTap: _rules.length > 1 ? () => setState(() => _rules.removeAt(i)) : null,
                      child: SizedBox.square(dimension: 44, child: Icon(CupertinoIcons.minus_circle_fill, color: _rules.length > 1 ? p.red : p.tertiary)),
                    ),
                    Builder(
                      builder: (bctx) => Pressable(
                        onTap: () => _pickKind(bctx, i),
                        child: Padding(
                          padding: const EdgeInsets.symmetric(vertical: 12),
                          child: Row(
                            children: [
                              _FilterIcon(r.$1),
                              const SizedBox(width: 10),
                              Text(_filters[r.$1], style: Type.body(p.label)),
                              Icon(Icons.unfold_more_rounded, size: 18, color: p.secondary),
                            ],
                          ),
                        ),
                      ),
                    ),
                    const Spacer(),
                    if (_arg(r).isNotEmpty)
                      Builder(
                        builder: (bctx) => Pressable(
                          onTap: () => _pickArg(bctx, i),
                          child: Padding(
                            padding: const EdgeInsets.fromLTRB(8, 12, 16, 12),
                            child: Row(
                              children: [
                                Text(_arg(r), style: Type.body(p.accent)),
                                Icon(Icons.unfold_more_rounded, size: 18, color: p.accent),
                              ],
                            ),
                          ),
                        ),
                      ),
                  ],
                ),
              ),
            CardRow(
              label: 'Add Filter',
              color: p.accent,
              leading: Icon(CupertinoIcons.plus_circle_fill, color: p.accent),
              onTap: () => setState(() => _rules.add((7, ''))),
            ),
          ],
        ),
        Padding(
          padding: const EdgeInsets.fromLTRB(16, 0, 16, 16),
          child: Pressable(
            onTap: _save,
            child: Container(
              height: 50,
              alignment: Alignment.center,
              decoration: BoxDecoration(color: p.accent, borderRadius: BorderRadius.circular(25)),
              child: Text(widget.folder == null ? 'Create' : 'Done', style: Type.text(17, Colors.white, weight: FontWeight.w600)),
            ),
          ),
        ),
      ],
    );
  }
}

/// A filter's coloured rounded-square symbol (Notes' Filters sheet).
class _FilterIcon extends StatelessWidget {
  const _FilterIcon(this.kind);
  final int kind;
  @override
  Widget build(BuildContext context) {
    final (icon, color) = switch (kind) {
      0 => (CupertinoIcons.number, const Color(0xFFFF9500)),
      1 => (CupertinoIcons.calendar_badge_plus, const Color(0xFF007AFF)),
      2 => (CupertinoIcons.calendar, const Color(0xFF007AFF)),
      3 => (CupertinoIcons.checkmark_circle, const Color(0xFF34C759)),
      4 => (CupertinoIcons.paperclip, const Color(0xFF8E8E93)),
      5 => (CupertinoIcons.folder, const Color(0xFFE5A800)),
      6 => (CupertinoIcons.square_pencil, const Color(0xFFE5A800)),
      7 => (CupertinoIcons.pin_fill, const Color(0xFFFF9500)),
      _ => (CupertinoIcons.lock_fill, const Color(0xFF8E8E93)),
    };
    return Container(
      width: 30,
      height: 30,
      decoration: BoxDecoration(color: color, borderRadius: BorderRadius.circular(7)),
      child: Icon(icon, size: 18, color: Colors.white),
    );
  }
}
