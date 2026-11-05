// A folder's notes (or All Notes, a Smart Folder's, a tag's): the large title and the count,
// Pinned first, then the date groups on inset cards; each note's title, date, first line,
// folder (where it isn't the list's own) and picture. Swipe right to pin, left to share, move
// or delete; long press for the note's menu. The search field and the new-note button sit at
// the foot (iOS 26), searching every note.
import 'dart:async';
import 'dart:io';

import 'package:flutter/cupertino.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';

import 'export_sheet.dart';

import 'folders.dart';
import 'lock.dart';
import 'model.dart';
import 'share.dart';
import 'smart.dart';
import 'tags.dart';
import 'state.dart';
import 'theme.dart';
import 'ui/dialogs.dart';
import 'ui/glass.dart';
import 'ui/page.dart';
import 'ui/sheet.dart';
import 'ui/swipe.dart';
import 'ui/touch_menu.dart';

class NotesListView extends StatefulWidget {
  const NotesListView({
    super.key,
    required this.state,
    required this.onOpenNote,
    required this.onNewNote,
    this.onBack,
    this.highlight = false,
    this.leading,
    this.searching = false,
  });
  final AppState state;
  final void Function(String id) onOpenNote;
  final VoidCallback onNewNote;

  /// The back button (iPhone: to Folders).
  final VoidCallback? onBack;

  /// iPad: the open note's row stays lit.
  final bool highlight;
  final Widget? leading;

  /// Opens searching (the Folders page's search pill).
  final bool searching;
  @override
  State<NotesListView> createState() => _NotesListViewState();
}

class _NotesListViewState extends State<NotesListView> {
  AppState get s => widget.state;
  late bool _searching = widget.searching;

  /// Select Notes: the notes ticked.
  bool _selecting = false;
  final _selected = <String>{};

  /// Select Notes' bar: Move and Delete for the ticked notes (Recover in Recently Deleted).
  Widget _selectBar(Palette p, bool trash) {
    final none = _selected.isEmpty;
    Future<void> done() async {
      setState(() {
        _selecting = false;
        _selected.clear();
      });
    }

    return Row(
      mainAxisAlignment: MainAxisAlignment.spaceBetween,
      children: [
        GlassGroup(
          items: [
            GlassItem(
              label: trash ? 'Recover' : 'Move',
              onTap: none
                  ? null
                  : (_) async {
                      final to = await pickFolder(context, s, title: trash ? 'Recover To' : 'Move ${_selected.length} Notes');
                      if (to == null) return;
                      if (trash) {
                        for (final id in [..._selected]) {
                          await s.recover(id);
                        }
                      }
                      await s.moveNotes([..._selected], to);
                      done();
                    },
            ),
          ],
        ),
        GlassGroup(
          items: [
            GlassItem(
              label: 'Delete',
              color: p.red,
              onTap: none
                  ? null
                  : (_) async {
                      if (trash && !await confirm(context, 'Delete ${_selected.length} Notes?', message: 'This action cannot be undone.')) return;
                      await s.deleteNotes([..._selected]);
                      done();
                    },
            ),
          ],
        ),
      ],
    );
  }

  final _query = TextEditingController();
  final _queryFocus = FocusNode();
  List<String>? _results;
  Timer? _debounce;

  @override
  void initState() {
    super.initState();
    s.searchRequest.addListener(_searchAsked);
  }

  /// The note bar's 🔍 (iPad): search here, the field at the top of the list.
  void _searchAsked() {
    if (!widget.highlight || !mounted) return;
    setState(() => _searching = true);
    WidgetsBinding.instance.addPostFrameCallback((_) => _queryFocus.requestFocus());
  }

  @override
  void dispose() {
    s.searchRequest.removeListener(_searchAsked);
    _query.dispose();
    _queryFocus.dispose();
    _debounce?.cancel();
    super.dispose();
  }

  void _search(String q) {
    _debounce?.cancel();
    if (q.trim().isEmpty) {
      setState(() => _results = null);
      return;
    }
    _debounce = Timer(const Duration(milliseconds: 180), () async {
      try {
        final r = (await s.engine.run('search', {'q': q.trim()}) as List).cast<String>();
        if (mounted && _query.text == q) setState(() => _results = r);
      } catch (_) {}
    });
  }

  void _endSearch() {
    _queryFocus.unfocus();
    setState(() {
      _searching = false;
      _results = null;
      _query.clear();
    });
  }

  @override
  Widget build(BuildContext context) {
    final p = Palette.of(context);
    return ListenableBuilder(
      listenable: s,
      builder: (ctx, _) {
        final lib = s.lib;
        final place = s.place;
        final folder = place is InFolder ? lib.folder(place.id) : null;
        final trash = folder?.trash == true;
        final notes = lib.notesIn(place);
        final sections = _searching
            ? (_results == null
                  ? <(String, List<NoteMeta>)>[]
                  : [
                      ('Notes', [for (final id in _results!) ?lib.note(id)].where((n) => !n.deleted).toList()),
                    ])
            : groupNotes(notes, s.settings.sort, byDate: s.settings.groupByDate, reverse: s.settings.reverse);
        final showFolder = folder == null || folder.smart;
        final count = notes.length;
        return NotesPage(
          background: widget.highlight ? p.page : null,
          // (iPad's list column: a small two-line title instead of the large one)
          title: widget.highlight
              ? null
              : (_selecting
                    ? '${_selected.length} Selected'
                    : (_searching ? 'Search' : lib.title(place, tablet: MediaQuery.of(context).size.shortestSide >= 600))),
          subtitle: _searching ? null : '$count ${count == 1 ? 'Note' : 'Notes'}',
          topExtra: Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              // iPad: searching from the note bar's 🔍 opens the field here, over the results
              if (widget.highlight && _searching)
                Padding(
                  padding: const EdgeInsets.fromLTRB(12, 0, 12, 10),
                  child: SizedBox(height: 48, child: _bottom(p, trash)),
                ),
              // a tag's notes: the tags to switch between, the chosen one yellow
              if (place is WithTag && !_searching) _tagChips(p, place),
              // locked notes open: Lock Now closes them again
              if (s.password != null && notes.any((n) => n.locked) && !_searching)
                Padding(
                  padding: const EdgeInsets.fromLTRB(16, 0, 16, 12),
                  child: Align(
                    alignment: Alignment.centerLeft,
                    child: Pressable(
                      onTap: s.lockNow,
                      child: Container(
                        padding: const EdgeInsets.symmetric(horizontal: 14, vertical: 9),
                        decoration: BoxDecoration(color: p.fill, borderRadius: BorderRadius.circular(18)),
                        child: Row(
                          mainAxisSize: MainAxisSize.min,
                          children: [
                            Icon(CupertinoIcons.lock_fill, size: 15, color: p.accent),
                            const SizedBox(width: 6),
                            Text('Lock Now', style: Type.text(15, p.accent, weight: FontWeight.w600)),
                          ],
                        ),
                      ),
                    ),
                  ),
                ),
            ],
          ),
          // iPad (regular width): the title and count inline in the bar, truncated
          inlineTitle: !widget.highlight
              ? null
              : Column(
                  mainAxisSize: MainAxisSize.min,
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    Text(
                      _selecting
                          ? '${_selected.length} Selected'
                          : (_searching ? 'Search' : lib.title(place, tablet: MediaQuery.of(context).size.shortestSide >= 600)),
                      maxLines: 1,
                      overflow: TextOverflow.ellipsis,
                      style: Type.text(15, p.label, weight: FontWeight.w700),
                    ),
                    if (!_searching) Text('$count ${count == 1 ? 'Note' : 'Notes'}', maxLines: 1, style: Type.caption(p.secondary)),
                  ],
                ),
          leading: widget.leading ?? (widget.onBack == null ? null : GlassButton(icon: CupertinoIcons.chevron_left, semantic: 'Folders', onTap: widget.onBack)),
          trailing: [
            if (_selecting)
              GlassButton(
                icon: CupertinoIcons.checkmark_alt,
                tinted: true,
                semantic: 'Done',
                onTap: () => setState(() {
                  _selecting = false;
                  _selected.clear();
                }),
              )
            else
              Builder(
                builder: (bctx) => GlassButton(icon: CupertinoIcons.ellipsis, semantic: 'More', onTap: () => _more(bctx, folder)),
              ),
          ],
          // (iPad: no search pill or compose at the foot; they're the note bar's)
          bottom: _selecting ? _selectBar(p, trash) : (widget.highlight ? null : _bottom(p, trash)),
          slivers: [
            if (sections.isEmpty)
              SliverToBoxAdapter(
                child: Padding(
                  padding: const EdgeInsets.only(top: 120),
                  child: Center(
                    child: Text(
                      _searching ? (_query.text.isEmpty ? '' : 'No Results') : 'No Notes',
                      style: Type.text(20, p.secondary, weight: FontWeight.w600),
                    ),
                  ),
                ),
              ),
            if (trash && notes.isNotEmpty)
              SliverToBoxAdapter(
                child: Padding(
                  padding: const EdgeInsets.fromLTRB(32, 0, 32, 14),
                  child: Text(
                    'Notes are available here for 30 days. After that time, notes will be permanently deleted.',
                    textAlign: TextAlign.center,
                    style: Type.footnote(p.secondary),
                  ),
                ),
              ),
            for (final (title, list) in sections)
              if (s.settings.gallery && !_searching)
                SliverToBoxAdapter(child: _gallery(title, list))
              else
                ..._lazySection(p, title, list, showFolder || _searching, trash),
          ],
        );
      },
    );
  }

  /// The tag browser: tap a tag to show its notes, again to leave them out (struck through),
  /// again to let it go; with more than one chosen, Any or All of them. Long press: Rename,
  /// Delete.
  Widget _tagChips(Palette p, WithTag w) {
    final tags = s.lib.allTags;
    final inc = w.chosen, exc = w.exclude;
    void set(Set<String> i, Set<String> e, bool all) => s.choose(WithTag('', include: i, exclude: e, all: all));
    Widget chip(String label, String? tag) {
      final key = tag?.toLowerCase();
      final lit = key == null ? inc.isEmpty && exc.isEmpty : inc.contains(key);
      final out = key != null && exc.contains(key);
      return Builder(
        builder: (cctx) => Pressable(
          onTap: () {
            if (key == null) return set({}, {}, false);
            if (inc.contains(key)) {
              set({...inc}..remove(key), {...exc, key}, w.all);
            } else if (exc.contains(key)) {
              set(inc, {...exc}..remove(key), w.all);
            } else {
              set({...inc, key}, exc, w.all);
            }
          },
          onLongPress: tag == null ? null : () => tagMenu(context, s, tag, globalRect(cctx)),
          child: Container(
            padding: const EdgeInsets.symmetric(horizontal: 14, vertical: 9),
            decoration: BoxDecoration(color: lit ? p.accent : p.fill, borderRadius: BorderRadius.circular(18)),
            child: Text(
              label,
              style: Type.text(
                15,
                lit ? Colors.white : (out ? p.secondary : p.label),
                weight: FontWeight.w500,
              ).copyWith(decoration: out ? TextDecoration.lineThrough : null),
            ),
          ),
        ),
      );
    }

    String names(Set<String> keys) => [for (final k in keys) '#${tags.firstWhere((t) => t.toLowerCase() == k, orElse: () => k)}'].join(' ');
    final caption = inc.isEmpty
        ? (exc.isEmpty ? 'Showing notes with any tag.' : 'Showing tagged notes without ${names(exc)}.')
        : 'Show notes that match ${inc.length > 1 ? (w.all ? 'all' : 'any') + ' of the selected tags' : 'the selected tag'}: ${names(inc)}.${exc.isEmpty ? '' : ' Leaving out ${names(exc)}.'}';
    return Padding(
      padding: const EdgeInsets.fromLTRB(16, 2, 16, 16),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Wrap(spacing: 8, runSpacing: 8, children: [chip('All Tags', null), for (final t in tags) chip('#$t', t)]),
          const SizedBox(height: 10),
          Text(caption, style: Type.footnote(p.secondary)),
          if (inc.length > 1)
            Padding(
              padding: const EdgeInsets.only(top: 10),
              child: Container(
                padding: const EdgeInsets.all(3),
                decoration: BoxDecoration(color: p.fill, borderRadius: BorderRadius.circular(20)),
                child: Row(
                  mainAxisSize: MainAxisSize.min,
                  children: [
                    for (final (all, l) in [(false, 'Any'), (true, 'All')])
                      Pressable(
                        onTap: () => set(inc, exc, all),
                        child: Container(
                          width: 72,
                          height: 36,
                          alignment: Alignment.center,
                          decoration: BoxDecoration(color: w.all == all ? p.accent : null, borderRadius: BorderRadius.circular(18)),
                          child: Text(l, style: Type.text(15, w.all == all ? Colors.white : p.label, weight: FontWeight.w600)),
                        ),
                      ),
                  ],
                ),
              ),
            ),
        ],
      ),
    );
  }

  /// A section as Section draws it (a header, rows on a card), its rows built only as they
  /// scroll into view (a library's All Notes can be thousands).
  List<Widget> _lazySection(Palette p, String title, List<NoteMeta> list, bool showFolder, bool trash) {
    final plain = widget.highlight;
    return [
      if (title.isNotEmpty)
        SliverToBoxAdapter(
          child: Padding(
            padding: EdgeInsets.fromLTRB(plain ? 8 : 20, 6, 20, 8),
            child: Text(title, style: Type.text(20, p.label, weight: FontWeight.w700)),
          ),
        ),
      SliverPadding(
        padding: EdgeInsets.fromLTRB(plain ? 4 : 16, 0, plain ? 4 : 16, 22),
        sliver: SliverList.builder(
          itemCount: list.length,
          itemBuilder: (ctx, i) {
            final r = Radius.circular(plain ? 0 : 26);
            return ClipRRect(
              borderRadius: BorderRadius.vertical(top: i == 0 ? r : Radius.zero, bottom: i == list.length - 1 ? r : Radius.zero),
              child: ColoredBox(
                color: plain ? Colors.transparent : p.card,
                child: Column(
                  mainAxisSize: MainAxisSize.min,
                  children: [
                    if (i > 0)
                      Padding(
                        padding: EdgeInsets.only(left: plain ? 18 : 18, right: plain ? 12 : 0),
                        // (hidden beside the chosen row, as the desktop list: its highlight stands clear)
                        child: Container(height: 0.5, color: plain && widget.highlight && (_lit(list[i].id) || _lit(list[i - 1].id)) ? Colors.transparent : p.separator),
                      ),
                    _noteRow(list[i], showFolder, trash),
                  ],
                ),
              ),
            );
          },
        ),
      ),
    ];
  }

  /// Whether a note's picture is on disk (asked once per path).
  static final _exists = <String, bool>{};
  static bool _hasFile(String path) => _exists.putIfAbsent(path, () => File(path).existsSync());

  Widget _bottom(Palette p, bool trash) {
    final field = SearchPill(
      onTap: _searching ? null : () => setState(() => _searching = true),
      child: !_searching
          ? null
          : Row(
              children: [
                Expanded(
                  child: TextField(
                    controller: _query,
                    focusNode: _queryFocus,
                    autofocus: true,
                    onChanged: _search,
                    style: Type.body(p.label),
                    cursorColor: p.accent,
                    textInputAction: TextInputAction.search,
                    decoration: InputDecoration.collapsed(hintText: 'Search', hintStyle: Type.body(p.secondary)),
                  ),
                ),
                if (_query.text.isNotEmpty)
                  Pressable(
                    onTap: () {
                      _query.clear();
                      _search('');
                    },
                    child: SizedBox.square(dimension: 44, child: Icon(CupertinoIcons.xmark_circle_fill, size: 18, color: p.tertiary)),
                  ),
              ],
            ),
    );
    return Row(
      children: [
        Expanded(child: field),
        const SizedBox(width: 10),
        if (_searching)
          GlassButton(icon: CupertinoIcons.xmark, semantic: 'Cancel Search', onTap: _endSearch)
        else if (!trash)
          GlassButton(icon: CupertinoIcons.square_pencil, semantic: 'New Note', onTap: widget.onNewNote),
      ],
    );
  }

  Widget _noteRow(NoteMeta n, bool showFolder, bool trash) {
    final p = Palette.of(context);
    final picked = _selected.contains(n.id);
    final on = _selecting ? picked : widget.highlight && _lit(n.id);
    final lib = s.lib;
    final date = listDate(s.settings.sort == SortBy.created ? n.created : n.modified);
    final row = Builder(
      builder: (rctx) => Pressable(
        onTap: () => _selecting ? setState(() => picked ? _selected.remove(n.id) : _selected.add(n.id)) : _tap(n.id),
        onLongPress: _selecting ? null : () => _noteMenu(rctx, n),
        child: Container(
          margin: widget.highlight ? const EdgeInsets.symmetric(horizontal: 8, vertical: 1) : EdgeInsets.zero,
          decoration: BoxDecoration(
            color: on ? p.fill : (widget.highlight ? Colors.transparent : p.card),
            borderRadius: BorderRadius.circular(widget.highlight ? 10 : 0),
          ),
          padding: EdgeInsets.fromLTRB(widget.highlight ? 10 : 18, 11, widget.highlight ? 8 : 14, 11),
          child: Row(
            children: [
              // Select Notes: a circle to tick
              if (_selecting)
                Padding(
                  padding: const EdgeInsets.only(right: 12),
                  child: Icon(picked ? CupertinoIcons.checkmark_circle_fill : CupertinoIcons.circle, size: 26, color: picked ? p.accent : p.tertiary),
                ),
              Expanded(
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    Row(
                      children: [
                        if (n.locked)
                          Padding(
                            padding: const EdgeInsets.only(right: 4),
                            child: Icon(CupertinoIcons.lock_fill, size: 14, color: p.label),
                          ),
                        Expanded(
                          child: Text(
                            n.title.isEmpty ? 'New Note' : n.title,
                            maxLines: 1,
                            overflow: TextOverflow.ellipsis,
                            style: Type.text(17, p.label, weight: FontWeight.w600),
                          ),
                        ),
                      ],
                    ),
                    const SizedBox(height: 2),
                    Text.rich(
                      TextSpan(
                        children: [
                          TextSpan(text: date, style: Type.subhead(p.label)),
                          const TextSpan(text: '  '),
                          TextSpan(text: n.locked ? 'Locked' : (n.snippet.isEmpty ? 'No additional text' : n.snippet), style: Type.subhead(p.secondary)),
                        ],
                      ),
                      maxLines: 1,
                      overflow: TextOverflow.ellipsis,
                    ),
                    if (showFolder && lib.folder(n.folder) != null)
                      Padding(
                        padding: const EdgeInsets.only(top: 3),
                        child: Row(
                          children: [
                            Icon(CupertinoIcons.folder, size: 14, color: p.secondary),
                            const SizedBox(width: 4),
                            Flexible(
                              child: Text(lib.folder(n.folder)!.name, maxLines: 1, overflow: TextOverflow.ellipsis, style: Type.subhead(p.secondary)),
                            ),
                          ],
                        ),
                      ),
                  ],
                ),
              ),
              if (n.thumb != null && !n.locked && !n.thumb!.endsWith('.pdf') && _hasFile(n.thumb!))
                Padding(
                  padding: const EdgeInsets.only(left: 10),
                  child: ClipRRect(
                    borderRadius: BorderRadius.circular(8),
                    // (drawings' pictures are see-through: on white, as Notes shows them)
                    child: ColoredBox(
                      color: Colors.white,
                      child: Image.file(
                        File(n.thumb!),
                        width: 54,
                        height: 54,
                        fit: BoxFit.cover,
                        cacheWidth: 160,
                        errorBuilder: (_, _, _) => const SizedBox(width: 54, height: 54),
                      ),
                    ),
                  ),
                ),
            ],
          ),
        ),
      ),
    );
    if (_selecting) return row;
    if (trash) {
      return SwipeRow(
        // (by the note: a row's swipe state never passes to the note after it when one goes)
        key: ValueKey('swipe-${n.id}'),
        trailing: [
          SwipeAction(CupertinoIcons.trash_fill, p.red, () => _delete(n), label: 'Delete'),
          SwipeAction(CupertinoIcons.folder_fill, p.purple, () => _recover(n), label: 'Recover'),
        ],
        child: row,
      );
    }
    return SwipeRow(
      key: ValueKey('swipe-${n.id}'),
      leading: [
        SwipeAction(
          n.pinned ? CupertinoIcons.pin_slash_fill : CupertinoIcons.pin_fill,
          p.accent,
          () => s.pin(n.id, !n.pinned),
          label: n.pinned ? 'Unpin' : 'Pin',
        ),
      ],
      trailing: [
        SwipeAction(CupertinoIcons.trash_fill, p.red, () => _delete(n), label: 'Delete'),
        SwipeAction(CupertinoIcons.folder_fill, p.purple, () => _move(n), label: 'Move'),
        if (!n.locked) SwipeAction(CupertinoIcons.share, p.blue, () => shareNote(s, n.id), label: 'Share'),
      ],
      child: row,
    );
  }

  Widget _gallery(String title, List<NoteMeta> list) {
    final p = Palette.of(context);
    return Padding(
      padding: const EdgeInsets.fromLTRB(16, 0, 16, 18),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          if (title.isNotEmpty)
            Padding(
              padding: const EdgeInsets.fromLTRB(4, 6, 4, 10),
              child: Text(title, style: Type.text(20, p.label, weight: FontWeight.w700)),
            ),
          LayoutBuilder(
            builder: (ctx, box) {
              final cols = (box.maxWidth / 170).floor().clamp(2, 6);
              final w = (box.maxWidth - (cols - 1) * 14) / cols;
              return Wrap(
                spacing: 14,
                runSpacing: 16,
                children: [
                  for (final n in list)
                    Builder(
                      builder: (rctx) => Pressable(
                        onTap: () => widget.onOpenNote(n.id),
                        onLongPress: () => _noteMenu(rctx, n),
                        child: SizedBox(
                          width: w,
                          child: Column(
                            children: [
                              Container(
                                height: w * 1.1,
                                decoration: BoxDecoration(
                                  color: p.card,
                                  borderRadius: BorderRadius.circular(14),
                                  border: Border.all(
                                    color: s.noteId == n.id && widget.highlight ? p.accent : p.separator,
                                    width: s.noteId == n.id && widget.highlight ? 3 : 0.5,
                                  ),
                                ),
                                clipBehavior: Clip.antiAlias,
                                padding: n.thumb != null && !n.locked ? EdgeInsets.zero : const EdgeInsets.all(10),
                                child: n.locked
                                    ? Center(child: Icon(CupertinoIcons.lock_fill, size: 34, color: p.tertiary))
                                    : n.thumb != null && _hasFile(n.thumb!) && !n.thumb!.endsWith('.pdf')
                                    ? Image.file(File(n.thumb!), fit: BoxFit.cover, width: w, cacheWidth: 400)
                                    : Align(
                                        alignment: Alignment.topLeft,
                                        child: Text('${n.title}\n${n.snippet}', overflow: TextOverflow.fade, style: Type.text(10, p.label, height: 1.3)),
                                      ),
                              ),
                              const SizedBox(height: 6),
                              Text(
                                n.title.isEmpty ? 'New Note' : n.title,
                                maxLines: 1,
                                overflow: TextOverflow.ellipsis,
                                style: Type.text(14, p.label, weight: FontWeight.w600),
                              ),
                              Text(listDate(n.modified), style: Type.caption(p.secondary)),
                            ],
                          ),
                        ),
                      ),
                    ),
                ],
              );
            },
          ),
        ],
      ),
    );
  }

  /// Highlighted: the open note, or one of a keyboard's multi-selection.
  bool _lit(String id) => s.noteId == id || (s.selection.length > 1 && s.selection.contains(id));

  /// A tap; with a keyboard's Shift a range from the last note picked, with Ctrl one more or
  /// one less (the iPad list, as on a Mac).
  void _tap(String id) {
    final k = HardwareKeyboard.instance;
    if (widget.highlight && k.isShiftPressed) {
      s.pickRange(id);
    } else if (widget.highlight && (k.isControlPressed || k.isMetaPressed)) {
      s.pickToggle(id);
    } else {
      s.selection.clear();
      s.anchor = id;
      widget.onOpenNote(id);
    }
  }

  /// The row menu over a multi-selection: Pin, Move, Export and Delete for them all.
  void _selectionMenu(BuildContext rctx) {
    final p = Palette.of(context);
    final ids = s.picked;
    final notes = [for (final id in ids) ?s.lib.note(id)];
    final trash = notes.any((n) => n.deleted);
    final pin = notes.any((n) => !n.pinned);
    showTouchMenu(context, p.look, globalRect(rctx), [
      if (!trash) {'label': pin ? 'Pin ${ids.length} Notes' : 'Unpin ${ids.length} Notes', 'icon': pin ? CupertinoIcons.pin : CupertinoIcons.pin_slash, 'id': 'pin'},
      {'label': trash ? 'Recover ${ids.length} Notes' : 'Move ${ids.length} Notes', 'icon': CupertinoIcons.folder, 'id': 'move'},
      if (!trash) {'label': 'Export ${ids.length} Notes…', 'icon': CupertinoIcons.arrow_up_doc, 'id': 'export'},
      {'separator': true},
      {'label': 'Delete ${ids.length} Notes', 'icon': CupertinoIcons.trash, 'id': 'delete', 'destructive': true},
    ], (i) async {
      switch (i['id']) {
        case 'pin':
          s.pinNotes(ids, pin);
        case 'move':
          final to = await pickFolder(context, s, title: trash ? 'Recover To' : 'Move ${ids.length} Notes');
          if (to == null) return;
          if (trash) {
            for (final id in ids) {
              await s.recover(id);
            }
          }
          s.moveNotes(ids, to);
        case 'export':
          if (mounted) showExportSheet(context, s, ids.first, ids: ids);
        case 'delete':
          await deleteSelection(context, s);
      }
    });
  }

  Future<void> _delete(NoteMeta n) async {
    if (n.deleted && !await confirm(context, 'Delete Note?', message: 'This note will be deleted. This action cannot be undone.')) return;
    s.delete(n.id);
  }

  Future<void> _recover(NoteMeta n) async {
    final to = await pickFolder(context, s, title: 'Recover To');
    if (to == null) return;
    await s.recover(n.id);
    if (to != s.lib.defaultFolder?.id) await s.move(n.id, to);
  }

  Future<void> _move(NoteMeta n) async {
    final to = await pickFolder(context, s, current: n.folder);
    if (to != null) s.move(n.id, to);
  }

  void _noteMenu(BuildContext rctx, NoteMeta n) {
    if (s.selection.length > 1 && s.selection.contains(n.id)) return _selectionMenu(rctx);
    final p = Palette.of(context);
    if (n.deleted) {
      showTouchMenu(context, p.look, globalRect(rctx), [
        {'label': 'Recover', 'icon': CupertinoIcons.folder, 'id': 'recover'},
        {'separator': true},
        {'label': 'Delete', 'icon': CupertinoIcons.trash, 'id': 'delete', 'destructive': true},
      ], (i) => i['id'] == 'recover' ? _recover(n) : _delete(n));
      return;
    }
    showTouchMenu(
      context,
      p.look,
      globalRect(rctx),
      [
        {'label': n.pinned ? 'Unpin Note' : 'Pin Note', 'icon': n.pinned ? CupertinoIcons.pin_slash : CupertinoIcons.pin, 'id': 'pin'},
        {'label': n.locked ? 'Remove Lock' : 'Lock Note', 'icon': n.locked ? CupertinoIcons.lock_open : CupertinoIcons.lock, 'id': 'lock'},
        if (!n.locked) {'label': 'Send a Copy', 'icon': CupertinoIcons.share, 'id': 'share'},
        {'label': 'Move', 'icon': CupertinoIcons.folder, 'id': 'move'},
        {'separator': true},
        {'label': 'Delete', 'icon': CupertinoIcons.trash, 'id': 'delete', 'destructive': true},
      ],
      (i) {
        switch (i['id']) {
          case 'pin':
            s.pin(n.id, !n.pinned);
          case 'lock':
            n.locked ? removeLock(context, s, n.id) : lockNote(context, s, n.id);
          case 'share':
            shareNote(s, n.id);
          case 'move':
            _move(n);
          case 'delete':
            _delete(n);
        }
      },
    );
  }

  void _more(BuildContext bctx, Folder? folder) {
    final p = Palette.of(context);
    final st = s.settings;
    void set(VoidCallback f) {
      f();
      st.save();
      s.changed();
    }

    showTouchMenu(
      context,
      p.look,
      globalRect(bctx),
      [
        {
          'label': st.gallery ? 'View as List' : 'View as Gallery',
          'icon': st.gallery ? CupertinoIcons.list_bullet : CupertinoIcons.square_grid_2x2,
          'id': 'view',
        },
        {'separator': true},
        if (folder != null && !folder.smart && !folder.trash) {'label': 'Add Folder', 'icon': CupertinoIcons.folder_badge_plus, 'id': 'addfolder'},
        if (folder != null && !folder.smart && !folder.trash && !folder.isDefault)
          {'label': 'Move This Folder', 'icon': CupertinoIcons.folder, 'id': 'movefolder'},
        {'label': 'Select Notes', 'icon': CupertinoIcons.checkmark_circle, 'id': 'select'},
        {
          'label': 'Sort By',
          'icon': CupertinoIcons.arrow_up_arrow_down,
          'kids': [
            for (final (b, l) in [(SortBy.edited, 'Date Edited'), (SortBy.created, 'Date Created'), (SortBy.title, 'Title')])
              {'label': l, 'id': 'sort', 'by': b.index, 'checked': st.sort == b},
            {'separator': true},
            {'label': st.sort == SortBy.title ? 'Ascending' : 'Newest First', 'id': 'order', 'rev': false, 'checked': !st.reverse},
            {'label': st.sort == SortBy.title ? 'Descending' : 'Oldest First', 'id': 'order', 'rev': true, 'checked': st.reverse},
          ],
        },
        {'label': 'Group By Date', 'icon': CupertinoIcons.calendar, 'id': 'group', 'checked': st.groupByDate},
        {'label': 'View Attachments', 'icon': CupertinoIcons.paperclip, 'id': 'attachments'},
        if (s.place is WithTag && (s.place as WithTag).chosen.isNotEmpty) ...[
          {'separator': true},
          {'label': 'Create Smart Folder', 'icon': CupertinoIcons.gear_alt, 'id': 'tagsmart'},
        ],
        if (folder != null && folder.smart) ...[
          {'separator': true},
          {'label': 'Edit Smart Folder', 'icon': CupertinoIcons.gear_alt, 'id': 'smart'},
        ],
        if (folder != null && !folder.trash && !folder.isDefault) ...[
          {'separator': true},
          {'label': 'Rename Folder', 'icon': CupertinoIcons.pencil, 'id': 'rename'},
          {'label': 'Delete Folder', 'icon': CupertinoIcons.trash, 'id': 'deleteFolder', 'destructive': true},
        ],
        if (folder != null && folder.trash) ...[
          {'separator': true},
          {'label': 'Delete All', 'icon': CupertinoIcons.trash, 'id': 'empty', 'destructive': true},
        ],
      ],
      (i) async {
        switch (i['id']) {
          case 'select':
            setState(() {
              _selecting = true;
              _selected.clear();
            });
          case 'view':
            set(() => st.gallery = !st.gallery);
          case 'sort':
            set(() => st.sort = SortBy.values[i['by'] as int]);
          case 'order':
            set(() => st.reverse = i['rev'] as bool);
          case 'addfolder':
            final n = await askText(context, 'New Folder', message: 'Inside "${folder!.name}"', hint: 'Name');
            if (n != null && n.trim().isNotEmpty) s.createFolder(n.trim(), parent: folder.id);
          case 'movefolder':
            final to = await pickFolder(context, s, title: 'Move This Folder', exclude: folder!.id, allowTop: true);
            if (to != null) s.moveFolder(folder.id, to.isEmpty ? null : to);
          case 'attachments':
            viewAttachments(context, s, s.lib.notesIn(s.place), widget.onOpenNote);
          case 'tagsmart':
            smartFolderFromTags(context, s, s.place as WithTag);
          case 'group':
            set(() => st.groupByDate = !st.groupByDate);
          case 'smart':
            editSmartFolder(context, s, folder: folder);
          case 'rename':
            final n = await askText(context, 'Rename Folder', initial: folder!.name);
            if (n != null && n.trim().isNotEmpty) s.renameFolder(folder.id, n.trim());
          case 'deleteFolder':
            if (await confirm(context, 'Delete Folder?', message: 'Its notes will go to Recently Deleted.', ok: 'Delete Folder')) s.deleteFolder(folder!.id);
          case 'empty':
            if (await confirm(context, 'Delete All?', message: 'All notes in Recently Deleted will be permanently deleted.', ok: 'Delete All')) s.emptyTrash();
        }
      },
    );
  }
}

/// View Attachments: the pictures of the notes here as a grid; a tap shows the note.
void viewAttachments(BuildContext context, AppState s, List<NoteMeta> notes, void Function(String id) open) {
  final withPics = notes.where((n) => n.thumb != null && !n.locked && !n.thumb!.toLowerCase().endsWith('.pdf') && File(n.thumb!).existsSync()).toList();
  showSheet<void>(
    context,
    title: 'Attachments',
    builder: (ctx) {
      final p = Palette.of(ctx);
      if (withPics.isEmpty)
        return Center(
          child: Text('No Attachments', style: Type.text(20, p.secondary, weight: FontWeight.w600)),
        );
      return GridView.builder(
        padding: const EdgeInsets.fromLTRB(16, 0, 16, 16),
        gridDelegate: const SliverGridDelegateWithMaxCrossAxisExtent(maxCrossAxisExtent: 130, mainAxisSpacing: 8, crossAxisSpacing: 8),
        itemCount: withPics.length,
        itemBuilder: (c, i) => Pressable(
          onTap: () {
            Navigator.of(ctx).pop();
            open(withPics[i].id);
          },
          child: ClipRRect(
            borderRadius: BorderRadius.circular(10),
            child: ColoredBox(
              color: Colors.white,
              child: Image.file(File(withPics[i].thumb!), fit: BoxFit.cover, cacheWidth: 300),
            ),
          ),
        ),
      );
    },
  );
}

/// Delete for the selection (Delete / Backspace in the list): to Recently Deleted without
/// asking (Undo brings them back); from Recently Deleted for good, after asking.
Future<void> deleteSelection(BuildContext context, AppState s) async {
  final ids = s.picked;
  if (ids.isEmpty) return;
  final gone = ids.where((id) => s.lib.note(id)?.deleted == true).length;
  if (gone > 0) {
    final one = ids.length == 1;
    if (!await confirm(context, one ? 'Delete Note?' : 'Delete ${ids.length} Notes?', message: one ? 'This note will be deleted. This action cannot be undone.' : 'This action cannot be undone.')) return;
  }
  await s.deleteNotes(ids);
}
