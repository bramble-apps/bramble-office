// Folders (Notes' first screen on iPhone, the sidebar on iPad): the account's All Notes, its
// folders and subfolders (disclosure arrows), Smart Folders and Recently Deleted on one card,
// then Tags; New Folder and Edit at the top right, the search pill and a new note at the foot
// (iOS 26). Long press a folder for its menu; swipe it for Delete and Move.
import 'package:flutter/cupertino.dart';
import 'package:flutter/material.dart';

import 'model.dart';
import 'settings_sheet.dart';
import 'smart.dart';
import 'tags.dart';
import 'state.dart';
import 'sync.dart';
import 'theme.dart';
import 'ui/dialogs.dart';
import 'ui/glass.dart';
import 'ui/page.dart';
import 'ui/sheet.dart';
import 'ui/swipe.dart';
import 'ui/touch_menu.dart';

class FoldersView extends StatefulWidget {
  const FoldersView({super.key, required this.state, required this.onOpen, required this.onNewNote, this.onSearch, this.sidebar = false, this.onToggleSidebar});
  final AppState state;
  final void Function(Place p) onOpen;
  final VoidCallback onNewNote;

  /// The search pill (searching every note, in All Notes' list).
  final VoidCallback? onSearch;

  /// iPad's sidebar: the chosen place stays lit.
  final bool sidebar;

  /// iPad's sidebar column: its toggle at the end of its own bar (hides the sidebar).
  final VoidCallback? onToggleSidebar;
  @override
  State<FoldersView> createState() => _FoldersViewState();
}

class _FoldersViewState extends State<FoldersView> {
  final _expanded = <String>{};
  bool _tagsClosed = false;

  /// the account sections folded shut (by account id; '' for the only one)
  final _accountsClosed = <String>{};

  AppState get s => widget.state;

  @override
  Widget build(BuildContext context) {
    final p = Palette.of(context);
    return ListenableBuilder(
      listenable: s,
      builder: (ctx, _) {
        final lib = s.lib;
        final tablet = MediaQuery.of(context).size.shortestSide >= 600;
        // one section per account (a Mac's iCloud and On My Mac come as accounts), as on a Mac:
        // its All, its folders, its Recently Deleted; one account: the section as before
        List<Widget> rowsFor(Account? a) {
          final rows = <Widget>[];
          final all = AllNotes(a?.id);
          final normal = lib.folders.where((f) => !f.trash && !f.smart && (a == null || f.account == a.id)).length;
          if (normal > 1 || lib.notes.isEmpty) {
            rows.add(_row(all, a == null ? 'All ${_account(context)}' : 'All ${lib.accountTitle(a, tablet: tablet)}', CupertinoIcons.folder, lib.count(all)));
          }
          void add(String? parent, int depth) {
            for (final f in lib.children(parent, account: a?.id)) {
              final kids = lib.hasChildren(f.id);
              rows.add(_folderRow(f, depth, kids));
              if (kids && _expanded.contains(f.id)) add(f.id, depth + 1);
            }
          }

          add(null, 0);
          final trash = a == null ? lib.trash : lib.trashIn(a.id);
          if (trash != null && lib.count(InFolder(trash.id)) > 0) {
            rows.add(_row(InFolder(trash.id), 'Recently Deleted', CupertinoIcons.trash, lib.count(InFolder(trash.id))));
          }
          return rows;
        }

        final sections = lib.manyAccounts ? [for (final a in lib.shownAccounts) (a.id, lib.accountTitle(a, tablet: tablet), rowsFor(a))] : [('', _account(context), rowsFor(null))];
        final tags = lib.allTags;
        return NotesPage(
          title: widget.sidebar ? null : 'Folders',
          background: widget.sidebar ? p.sidebar : null,
          // (iPad's sidebar bar: Edit …… [new folder][toggle]; the iPhone's: [new folder][Edit])
          leading: widget.onToggleSidebar == null ? null : GlassGroup(items: [GlassItem(label: 'Edit', onTap: _more)]),
          trailing: [
            GlassButton(icon: CupertinoIcons.folder_badge_plus, semantic: 'New Folder', onTap: _newFolder),
            if (widget.onToggleSidebar == null)
              GlassGroup(
                items: [GlassItem(label: 'Edit', onTap: _more)],
              )
            else
              GlassButton(icon: CupertinoIcons.sidebar_left, semantic: 'Hide Sidebar', onTap: widget.onToggleSidebar),
          ],
          bottom: widget.sidebar
              ? null
              : Row(
                  children: [
                    Expanded(child: SearchPill(onTap: widget.onSearch)),
                    const SizedBox(width: 10),
                    GlassButton(icon: CupertinoIcons.square_pencil, semantic: 'New Note', onTap: widget.onNewNote),
                  ],
                ),
          slivers: [
            // (a sync running: a slim row of how far it is, over the folders)
            SliverToBoxAdapter(child: SyncProgress(sync: s.sync, padding: EdgeInsets.fromLTRB(widget.sidebar ? 12 : 20, 0, widget.sidebar ? 12 : 20, 10))),
            for (final (id, title, rows) in sections)
              SliverToBoxAdapter(
                child: Section(
                  plain: widget.sidebar,
                  header: title,
                  collapsed: _accountsClosed.contains(id),
                  onHeaderTap: () => setState(() => _accountsClosed.contains(id) ? _accountsClosed.remove(id) : _accountsClosed.add(id)),
                  children: rows,
                ),
              ),
            if (tags.isNotEmpty)
              SliverToBoxAdapter(
                child: Section(
                  plain: widget.sidebar,
                  header: 'Tags',
                  collapsed: _tagsClosed,
                  onHeaderTap: () => setState(() => _tagsClosed = !_tagsClosed),
                  children: [
                    Container(
                      width: double.infinity,
                      padding: const EdgeInsets.all(14),
                      child: Wrap(
                        spacing: 8,
                        runSpacing: 8,
                        children: [
                          _TagChip(label: 'All Tags', on: s.place == const WithTag('') && widget.sidebar, onTap: () => widget.onOpen(const WithTag(''))),
                          for (final t in tags)
                            Builder(
                              builder: (cctx) => _TagChip(
                                label: '#$t',
                                on: s.place == WithTag(t) && widget.sidebar,
                                onTap: () => widget.onOpen(WithTag(t)),
                                onLongPress: () => tagMenu(context, s, t, globalRect(cctx)),
                              ),
                            ),
                        ],
                      ),
                    ),
                  ],
                ),
              ),
          ],
        );
      },
    );
  }

  String _account(BuildContext context) => s.lib.account(tablet: MediaQuery.of(context).size.shortestSide >= 600);

  Widget _row(Place place, String name, IconData icon, int count, {int depth = 0, bool? expanded, VoidCallback? onDisclose, Folder? folder}) {
    final p = Palette.of(context);
    final on = widget.sidebar && s.place == place;
    final row = Builder(
      builder: (rctx) => Pressable(
        onTap: () => widget.onOpen(place),
        onLongPress: folder == null ? null : () => _folderMenu(rctx, folder),
        child: Container(
          // iPad's sidebar: the chosen place is a grey rounded rectangle
          margin: widget.sidebar ? const EdgeInsets.symmetric(horizontal: 6) : EdgeInsets.zero,
          decoration: BoxDecoration(color: on ? p.fill : null, borderRadius: BorderRadius.circular(widget.sidebar ? 12 : 0)),
          constraints: const BoxConstraints(minHeight: 52),
          padding: EdgeInsets.only(left: (widget.sidebar ? 8 : 14) + depth * 22.0, right: 10),
          child: Row(
            children: [
              SizedBox(width: 32, child: Icon(icon, color: p.accent, size: 24)),
              const SizedBox(width: 12),
              Expanded(
                child: Text(name, maxLines: 1, overflow: TextOverflow.ellipsis, style: Type.body(p.label)),
              ),
              Text('$count', style: Type.body(p.secondary)),
              if (expanded != null)
                GestureDetector(
                  behavior: HitTestBehavior.opaque,
                  onTap: onDisclose,
                  child: SizedBox(
                    width: 40,
                    height: 48,
                    child: AnimatedRotation(
                      turns: expanded ? 0.25 : 0,
                      duration: const Duration(milliseconds: 200),
                      child: Icon(Icons.chevron_right_rounded, color: p.accent, size: 24),
                    ),
                  ),
                )
              else
                Padding(
                  padding: const EdgeInsets.only(left: 6, right: 2),
                  child: Icon(Icons.chevron_right_rounded, color: p.tertiary, size: 22),
                ),
            ],
          ),
        ),
      ),
    );
    if (folder == null || folder.isDefault) return row;
    return SwipeRow(
      trailing: [
        SwipeAction(CupertinoIcons.trash_fill, p.red, () => _deleteFolder(folder)),
        SwipeAction(CupertinoIcons.folder_fill, p.purple, () => _moveFolder(folder)),
      ],
      child: ColoredBox(color: widget.sidebar ? Colors.transparent : p.card, child: row),
    );
  }

  Widget _folderRow(Folder f, int depth, bool kids) => _row(
    InFolder(f.id),
    f.name,
    f.smart ? CupertinoIcons.gear_alt : (f.name == 'Quick Notes' && f.parent == null ? CupertinoIcons.doc_text : CupertinoIcons.folder),
    s.lib.count(InFolder(f.id)),
    depth: depth,
    folder: f,
    expanded: kids ? _expanded.contains(f.id) : null,
    onDisclose: () => setState(() => _expanded.contains(f.id) ? _expanded.remove(f.id) : _expanded.add(f.id)),
  );

  void _more(BuildContext bctx) {
    final p = Palette.of(context);
    showTouchMenu(
      context,
      p.look,
      globalRect(bctx),
      [
        {'label': 'New Smart Folder', 'icon': CupertinoIcons.gear_alt, 'id': 'smart'},
        {'separator': true},
        {'label': 'Settings', 'icon': CupertinoIcons.gear, 'id': 'settings'},
        if (s.password != null) {'label': 'Lock Now', 'icon': CupertinoIcons.lock, 'id': 'locknow'},
      ],
      (i) {
        switch (i['id']) {
          case 'smart':
            editSmartFolder(context, s);
          case 'settings':
            showSettings(context, s);
          case 'locknow':
            s.lockNow();
        }
      },
      centered: false,
    );
  }

  Future<void> _newFolder() async {
    final made = await showSheet<bool>(
      context,
      title: 'New Folder',
      fit: true,
      builder: (ctx) => _NewFolder(state: s),
    );
    if (made == true && mounted) setState(() {});
  }

  void _folderMenu(BuildContext rctx, Folder f) {
    final p = Palette.of(context);
    showTouchMenu(
      context,
      p.look,
      globalRect(rctx),
      [
        if (f.smart) {'label': 'Edit Smart Folder', 'icon': CupertinoIcons.gear_alt, 'id': 'edit'},
        if (!f.isDefault) {'label': 'Rename', 'icon': CupertinoIcons.pencil, 'id': 'rename'},
        if (!f.isDefault && !f.smart) {'label': 'Move', 'icon': CupertinoIcons.folder, 'id': 'move'},
        if (!f.smart) {'label': 'New Subfolder', 'icon': CupertinoIcons.folder_badge_plus, 'id': 'sub'},
        if (!f.isDefault) ...[
          {'separator': true},
          {'label': 'Delete', 'icon': CupertinoIcons.trash, 'id': 'delete', 'destructive': true},
        ],
      ],
      (i) async {
        switch (i['id']) {
          case 'edit':
            editSmartFolder(context, s, folder: f);
          case 'rename':
            final n = await askText(context, 'Rename Folder', initial: f.name);
            if (n != null && n.trim().isNotEmpty) s.renameFolder(f.id, n.trim());
          case 'move':
            _moveFolder(f);
          case 'sub':
            final n = await askText(context, 'New Folder', message: 'Inside "${f.name}"', hint: 'Name', ok: 'Save');
            if (n != null && n.trim().isNotEmpty) {
              await s.createFolder(n.trim(), parent: f.id);
              setState(() => _expanded.add(f.id));
            }
          case 'delete':
            _deleteFolder(f);
        }
      },
    );
  }

  Future<void> _deleteFolder(Folder f) async {
    final n = s.lib.count(InFolder(f.id));
    final ok = await confirm(
      context,
      'Delete Folder?',
      message: f.smart ? 'The notes in it stay where they are.' : (n == 0 ? null : 'The $n ${n == 1 ? 'note' : 'notes'} in it will go to Recently Deleted.'),
      ok: 'Delete Folder',
    );
    if (ok) s.deleteFolder(f.id);
  }

  Future<void> _moveFolder(Folder f) async {
    final to = await pickFolder(context, s, title: 'Move Folder', exclude: f.id, allowTop: true);
    if (to == null) return;
    s.moveFolder(f.id, to.isEmpty ? null : to);
  }
}

class _TagChip extends StatelessWidget {
  const _TagChip({required this.label, required this.on, required this.onTap, this.onLongPress});
  final String label;
  final bool on;
  final VoidCallback onTap;
  final VoidCallback? onLongPress;
  @override
  Widget build(BuildContext context) {
    final p = Palette.of(context);
    return Pressable(
      onTap: onTap,
      onLongPress: onLongPress,
      child: Container(
        padding: const EdgeInsets.symmetric(horizontal: 14, vertical: 10),
        decoration: BoxDecoration(color: on ? p.accent : p.fill, borderRadius: BorderRadius.circular(20)),
        child: Text(label, style: Type.text(15, on ? Colors.white : p.label, weight: FontWeight.w500)),
      ),
    );
  }
}

/// New Folder (the sidebar's button, Ctrl+Shift+N).
Future<void> showNewFolder(BuildContext context, AppState s) => showSheet<bool>(context, title: 'New Folder', fit: true, builder: (ctx) => _NewFolder(state: s));

class _NewFolder extends StatefulWidget {
  const _NewFolder({required this.state});
  final AppState state;
  @override
  State<_NewFolder> createState() => _NewFolderState();
}

class _NewFolderState extends State<_NewFolder> {
  final _name = TextEditingController();

  @override
  void dispose() {
    _name.dispose();
    super.dispose();
  }

  Future<void> _save() async {
    final n = _name.text.trim();
    if (n.isEmpty) return;
    await widget.state.createFolder(n, account: widget.state.lib.manyAccounts ? widget.state.lib.accountOf(widget.state.place) : null);
    if (mounted) Navigator.of(context).pop(true);
  }

  @override
  Widget build(BuildContext context) {
    final p = Palette.of(context);
    return Column(
      mainAxisSize: MainAxisSize.min,
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
                    autofocus: true,
                    style: Type.body(p.label),
                    cursorColor: p.accent,
                    onSubmitted: (_) => _save(),
                    decoration: InputDecoration.collapsed(hintText: 'Name', hintStyle: Type.body(p.tertiary)),
                  ),
                ),
              ),
            ),
          ],
        ),
        Card2(
          children: [
            CardRow(
              label: 'Make into Smart Folder',
              leading: Icon(CupertinoIcons.gear_alt, color: p.accent),
              chevron: true,
              onTap: () async {
                final nav = Navigator.of(context);
                final name = _name.text.trim();
                nav.pop();
                await editSmartFolder(nav.context, widget.state, name: name);
              },
            ),
          ],
        ),
        Padding(
          padding: const EdgeInsets.fromLTRB(16, 0, 16, 8),
          child: Pressable(
            onTap: _save,
            child: Container(
              height: 50,
              alignment: Alignment.center,
              decoration: BoxDecoration(color: p.accent, borderRadius: BorderRadius.circular(25)),
              child: Text('Save', style: Type.text(17, Colors.white, weight: FontWeight.w600)),
            ),
          ),
        ),
      ],
    );
  }
}

/// A folder to move into (its id; '' = the top, with `allowTop`).
Future<String?> pickFolder(BuildContext context, AppState s, {String title = 'Move Note', String? exclude, String? current, bool allowTop = false}) {
  return showSheet<String>(
    context,
    title: title,
    builder: (ctx) {
      final p = Palette.of(ctx);
      final rows = <Widget>[];
      if (allowTop)
        rows.add(
          CardRow(
            label: 'Top Level',
            leading: Icon(CupertinoIcons.folder, color: p.accent),
            onTap: () => Navigator.of(ctx).pop(''),
          ),
        );
      void add(String? parent, int depth) {
        for (final f in s.lib.children(parent)) {
          if (f.smart || f.id == exclude) continue;
          rows.add(
            Padding(
              padding: EdgeInsets.only(left: depth * 22.0),
              child: CardRow(
                label: f.name,
                leading: Icon(CupertinoIcons.folder, color: p.accent),
                checked: f.id == current,
                onTap: f.id == current ? null : () => Navigator.of(ctx).pop(f.id),
              ),
            ),
          );
          add(f.id, depth + 1);
        }
      }

      add(null, 0);
      return ListView(
        padding: const EdgeInsets.only(top: 4),
        children: [Card2(children: rows)],
      );
    },
  );
}

/// iOS 26's search pill at the foot of the lists (a magnifier, "Search", the mic).
class SearchPill extends StatelessWidget {
  const SearchPill({super.key, this.onTap, this.child});
  final VoidCallback? onTap;

  /// The field itself while searching.
  final Widget? child;
  @override
  Widget build(BuildContext context) {
    final p = Palette.of(context);
    return Glass(
      radius: 24,
      child: GestureDetector(
        behavior: HitTestBehavior.opaque,
        onTap: onTap,
        child: SizedBox(
          height: 48,
          child: Row(
            children: [
              const SizedBox(width: 14),
              Icon(CupertinoIcons.search, size: 20, color: p.secondary),
              const SizedBox(width: 8),
              Expanded(child: child ?? Text('Search', style: Type.body(p.secondary))),
              if (child == null)
                Padding(
                  padding: const EdgeInsets.only(right: 14),
                  child: Icon(CupertinoIcons.mic_fill, size: 19, color: p.secondary),
                ),
            ],
          ),
        ),
      ),
    );
  }
}
