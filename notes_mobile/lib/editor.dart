// A note (iOS 26): the text runs under floating glass controls: back at the top left, the
// [undo][share][…] capsule at the top right and the yellow ✓ while typing; at rest the
// [checklist][paperclip][markup] capsule and the compose circle at the foot; while typing the
// accessory bar over the keyboard, whose Aa puts the Format sheet in the keyboard's place.
// On iPad the tools are in the top toolbar instead. Changes are saved a second after the last
// one, when the note is left, and when the app goes to the background; an empty new note is
// thrown away. Locked notes ask for the password; deleted ones are read only until recovered.
import 'dart:async';
import 'dart:math' as math;

import 'dart:io';

import 'package:file_picker/file_picker.dart';
import 'package:flutter/cupertino.dart';
import 'package:flutter/material.dart';
import 'package:flutter/rendering.dart';
import 'package:flutter/services.dart';

import 'attachments.dart';
import 'doc.dart';
import 'drawing.dart';
import 'export_sheet.dart';
import 'folders.dart';
import 'format.dart';
import 'lock.dart';
import 'model.dart';
import 'pdf.dart';
import 'share.dart';
import 'state.dart';
import 'record_sheet.dart';
import 'scan.dart';
import 'table_inline.dart';
import 'theme.dart';
import 'ui/dialogs.dart';
import 'ui/glass.dart';
import 'ui/touch_menu.dart';

class EditorView extends StatefulWidget {
  const EditorView({
    super.key,
    required this.state,
    required this.id,
    this.onBack,
    this.wide = false,
    this.fullScreen = false,
    this.onFullScreen,
    this.onNewNote,
  });
  final AppState state;
  final String id;
  final VoidCallback? onBack;

  /// iPad: the tools in the top toolbar.
  final bool wide, fullScreen;
  final VoidCallback? onFullScreen, onNewNote;
  @override
  State<EditorView> createState() => EditorViewState();
}

class EditorViewState extends State<EditorView> with WidgetsBindingObserver {
  AppState get s => widget.state;
  final c = NoteController();
  final _focus = FocusNode();
  final _scroll = ScrollController();
  late final TokenContext _tokens = TokenContext(attachments: {}, width: 300, onTap: _openAttachment, onLongPress: _attachmentMenu, onLink: _openLink, table: _table);

  /// The table whose cell has the caret, and a new table's id (its first cell takes the caret).
  InlineTableState? _cellTable;
  String? _newTable;

  Map<String, dynamic>? _note;
  final _fieldKey = GlobalKey();

  /// The audio attachment playing.
  String? _playing;

  /// The collapsible headings' chevrons: (paragraph, the heading line's middle).
  List<(int, double)> _chevrons = [];

  void _placeChevrons() {
    if (!mounted) return;
    final root = _fieldKey.currentContext?.findRenderObject();
    RenderEditable? re;
    void find(RenderObject o) {
      if (re != null) return;
      if (o is RenderEditable) {
        re = o;
        return;
      }
      o.visitChildren(find);
    }

    if (root == null) return;
    find(root);
    final r = re;
    if (r == null || !r.hasSize || !r.attached) return;
    final out = <(int, double)>[];
    for (final k in c.collapsibleHeadings()) {
      final caret = r.getLocalRectForCaret(TextPosition(offset: c.contentStart(k)));
      out.add((k, r.localToGlobal(caret.center, ancestor: root).dy));
    }
    final same =
        out.length == _chevrons.length &&
        [for (var i = 0; i < out.length; i++) out[i].$1 == _chevrons[i].$1 && (out[i].$2 - _chevrons[i].$2).abs() < 0.5].every((x) => x);
    if (!same) setState(() => _chevrons = out);
  }

  bool _locked = false, _dirty = false, _format = false, _loaded = false, _hadFocus = false;
  Timer? _saveTimer;
  final _adds = <Map<String, dynamic>>[];
  final _tables = <String, dynamic>{}, _drawings = <String, dynamic>{}, _views = <String, int>{};
  String? _findQuery;
  final _find = TextEditingController();
  final _with = TextEditingController();
  bool _replacing = false;

  bool get _deleted => _note?['deleted'] == true || (s.lib.note(widget.id)?.deleted ?? false);

  @override
  void initState() {
    super.initState();
    WidgetsBinding.instance.addObserver(this);
    s.editorCommand = _command;
    c.onEdit = _edited;
    c.tokenWidget = (ctx, tk, para, style) => tokenWidget(ctx, _tokens, tk, para, style);
    s.noteRevision.addListener(_reopen);
    s.syncedIn.addListener(_synced);
    s.beforeSync = saveNow;
    _focus.addListener(
      () => setState(() {
        if (!_focus.hasFocus) _format = false;
      }),
    );
    _open();
  }

  @override
  void didUpdateWidget(EditorView old) {
    super.didUpdateWidget(old);
    if (old.id != widget.id) {
      saveNow(leaving: true, id: old.id);
      _open();
    }
  }

  @override
  void dispose() {
    WidgetsBinding.instance.removeObserver(this);
    if (s.editorCommand == _command) s.editorCommand = null;
    s.noteRevision.removeListener(_reopen);
    s.syncedIn.removeListener(_synced);
    if (s.beforeSync == saveNow) s.beforeSync = null;
    saveNow(leaving: true);
    _saveTimer?.cancel();
    c.dispose();
    _focus.dispose();
    _scroll.dispose();
    _find.dispose();
    _with.dispose();
    super.dispose();
  }

  @override
  void didChangeAppLifecycleState(AppLifecycleState st) {
    if (st == AppLifecycleState.paused || st == AppLifecycleState.inactive) saveNow();
  }

  /// A sync brought notes in: this one is read again unless it's being typed in.
  void _synced() {
    if (!mounted || _focus.hasFocus || _cellTable != null || _dirty) return;
    _open();
  }

  void _reopen() {
    saveNow();
    _open();
  }

  /// Why the note couldn't be read or laid out (shown with Try Again and Copy Details).
  String? _loadError;

  /// Reads the note in; any failure is shown in the note's place and logged, never a blank page.
  void _open() {
    try {
      _loadError = null;
      _read();
    } catch (e, st) {
      logProblem('opening note ${widget.id}', e, st);
      _note = null;
      _locked = false;
      _loaded = true;
      _loadError = '$e';
      if (mounted) setState(() {});
    }
  }

  void _read() {
    final meta = s.lib.note(widget.id);
    _adds.clear();
    _tables.clear();
    _drawings.clear();
    _views.clear();
    _dirty = false;
    _format = false;
    Map<String, dynamic>? n;
    if (meta?.locked == true) {
      if (s.password == null) {
        setState(() {
          _locked = true;
          _note = null;
          _loaded = true;
        });
        return;
      }
      n = (s.call('unlock', {'id': widget.id, 'pw': s.password}) as Map?)?.cast();
    } else {
      n = (s.call('note', {'id': widget.id}) as Map?)?.cast();
      // locked meanwhile (the list not read again yet): it opens only with the password
      if (n != null && n['locked'] == true && n['unlocked'] != true) {
        n = s.password == null ? null : (s.call('unlock', {'id': widget.id, 'pw': s.password}) as Map?)?.cast();
        if (n == null) {
          setState(() {
            _locked = true;
            _note = null;
            _loaded = true;
          });
          return;
        }
      }
    }
    _locked = false;
    _note = n;
    _loaded = true;
    if (n != null) {
      _takeAttachments(n);
      c.firstStyle = s.settings.newNoteStyle;
      c.load(n['body'] as List? ?? const []);
      // an empty note starts in the New Notes Start With style
      if (c.text.isEmpty && c.paras.length == 1) c.paras.first.st = s.settings.newNoteStyle;
      // a new empty note: straight into typing
      if (c.text.isEmpty && !_deleted) WidgetsBinding.instance.addPostFrameCallback((_) => _focus.requestFocus());
    }
    if (mounted) setState(() {});
  }

  /// The note couldn't be read: why, and what to do.
  Widget _cantOpen(Palette p) {
    final why = _loadError ?? s.lastError ?? 'The note wasn\'t found in the library.';
    return Center(
      child: Padding(
        padding: const EdgeInsets.symmetric(horizontal: 32),
        child: Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            Text('This note can\'t be opened.', textAlign: TextAlign.center, style: Type.title(p.label)),
            const SizedBox(height: 8),
            Text(why, textAlign: TextAlign.center, maxLines: 6, overflow: TextOverflow.ellipsis, style: Type.footnote(p.secondary)),
            const SizedBox(height: 18),
            Row(
              mainAxisSize: MainAxisSize.min,
              children: [
                CupertinoButton(onPressed: _open, child: Text('Try Again', style: Type.body(p.accent))),
                CupertinoButton(
                  onPressed: () {
                    Clipboard.setData(ClipboardData(text: 'Notes: note ${widget.id} can\'t be opened: $why'));
                    s.say('Copied.');
                  },
                  child: Text('Copy Details', style: Type.body(p.accent)),
                ),
              ],
            ),
          ],
        ),
      ),
    );
  }

  void _takeAttachments(Map<String, dynamic> n) {
    _tokens.attachments
      ..clear()
      ..addAll({for (final a in (n['attachments'] as List? ?? const [])) (a as Map)['id'] as String: a.cast<String, dynamic>()});
  }

  Future<void> _unlock() async {
    final pw = await askPassword(context, s);
    if (pw != null) _open();
  }

  void _edited() {
    _dirty = true;
    _saveTimer?.cancel();
    _saveTimer = Timer(const Duration(seconds: 1), saveNow);
    setState(() {});
  }

  /// Writes what changed. Leaving a new note with nothing in it throws it away.
  void saveNow({bool leaving = false, String? id}) {
    _saveTimer?.cancel();
    final nid = id ?? widget.id;
    if (_note == null || _deleted) return;
    final empty = c.text.replaceAll(obj, '').trim().isEmpty && c.tokens.isEmpty;
    if (leaving && empty && _adds.isEmpty) {
      final meta = s.lib.note(nid);
      if (meta != null && meta.title.isEmpty && meta.snippet.isEmpty && !meta.locked) {
        s.call('erase', {'id': nid});
        if (s.noteId == nid) s.noteId = null;
        s.reload();
        return;
      }
    }
    if (!_dirty && _adds.isEmpty && _tables.isEmpty && _drawings.isEmpty && _views.isEmpty) return;
    final r = s.call('save', {
      'id': nid,
      'body': c.body(),
      if (_adds.isNotEmpty) 'add': [..._adds],
      if (_tables.isNotEmpty) 'tables': {..._tables},
      if (_drawings.isNotEmpty) 'drawings': {..._drawings},
      if (_views.isNotEmpty) 'views': {..._views},
    });
    _adds.clear();
    _tables.clear();
    _drawings.clear();
    _views.clear();
    _dirty = false;
    if (r is Map) {
      _note = r.cast();
      if (!leaving && mounted) setState(() => _takeAttachments(_note!));
    }
    if (leaving) s.call('close', {'id': nid});
    s.reload();
  }

  // ------------------------------------------------------------ attachments

  Future<void> _attach(BuildContext bctx) async {
    final p = Palette.of(context);
    showTouchMenu(
      context,
      p.look,
      globalRect(bctx),
      [
        {'label': 'Scan Documents', 'icon': CupertinoIcons.doc_text_viewfinder, 'id': 'scan'},
        if (Platform.isAndroid) {'label': 'Take Photo', 'icon': CupertinoIcons.camera, 'id': 'camera'},
        {'label': 'Choose Photo or Video', 'icon': CupertinoIcons.photo_on_rectangle, 'id': 'photo'},

        {'separator': true},
        {'label': 'Record Audio', 'icon': CupertinoIcons.waveform, 'id': 'audio'},
        {'label': 'Attach File', 'icon': CupertinoIcons.paperclip, 'id': 'file'},
      ],
      (i) async {
        switch (i['id']) {
          case 'photo':
            final path = (await FilePicker.pickFile(type: FileType.media))?.path;
            if (path != null) _addFile(path);
          case 'file':
            final path = (await FilePicker.pickFile())?.path;
            if (path != null) _addFile(path);
          case 'scan':
            _focus.unfocus();
            final r = await scanDocuments(context);
            if (r != null) _addFile(r.pdf, pages: r.pages);
          case 'camera':
            final path = await takePhoto(saveToPhotos: s.settings.saveToPhotos);
            if (path != null) _addFile(path);
          case 'audio':
            _focus.unfocus();
            final path = await recordAudio(context);
            if (path != null) _addFile(path);
          case 'table':
            _addTable();
          case 'drawing':
            _addDrawing();
        }
      },
    );
  }

  /// Where an attachment goes: at the caret while typing, else at the note's end.
  void _placeForInsert() {
    if (!_focus.hasFocus || !c.selection.isValid) c.selection = TextSelection.collapsed(offset: c.text.length);
  }

  Future<void> _addFile(String path, {List<Size>? pages}) async {
    _placeForInsert();
    final at = c.selection;
    final uti = (s.call('uti', {'path': path}) as Map?)?['uti'] as String? ?? 'public.data';
    final id = newId();
    // a PDF: its pages as Android reads them (the engine can't here)
    final found = uti == 'com.adobe.pdf' && (pages == null || pages.isEmpty) ? await pdfPages(path) : (pages ?? const <Size>[]);
    c.selection = at;
    _adds.add({
      'id': id,
      'file': path,
      'uti': uti,
      if (found.isNotEmpty)
        'pages': [
          for (final p in found) [p.width, p.height],
        ],
    });
    c.insertToken({'id': id, 'uti': uti, 'alt': '', 'content': ''});
    saveNow();
  }

  Future<void> _addTable() async {
    _placeForInsert();
    final id = newId();
    _adds.add({
      'id': id,
      'uti': utiTable,
      'table': {
        'rows': [
          ['', ''],
          ['', ''],
        ],
      },
    });
    _newTable = id;
    c.insertToken({'id': id, 'uti': utiTable, 'alt': '', 'content': ''});
    saveNow();
  }

  Widget _table(Map<String, dynamic> a, double width) {
    final id = a['id'] as String;
    final rows = ((a['table'] as Map?)?['rows'] as List? ?? const []).map((r) => (r as List).map((e) => '$e').toList()).toList();
    final auto = _newTable == id;
    if (auto) _newTable = null;
    // (its own layer: a row or column moving repaints the table, not the note)
    return RepaintBoundary(
      key: ValueKey('table-$id'),
      child: InlineTable(
      key: ValueKey('table-in-$id'),
      rows: rows,
      width: width,
      editable: !_deleted && !_locked && _note != null,
      autofocus: auto,
      base: a,
      onChanged: (r, rf, cf) => _tableEdited(a, r, rf, cf),
      onActive: (t) {
        if (mounted && t != _cellTable) setState(() => _cellTable = t);
      },
      ),
    );
  }

  void _tableEdited(Map<String, dynamic> a, List<List<String>> rows, List<int?> rowFrom, List<int?> colFrom) {
    final id = a['id'] as String;
    if (rows.isEmpty) {
      _cellTable = null;
      c.removeToken(id);
      saveNow();
      return;
    }
    // (where the rows and columns came from: moved cells keep their formatting)
    _tables[id] = {'rows': rows, 'rowFrom': rowFrom, 'colFrom': colFrom};
    a['table'] = {...?(a['table'] as Map?)?.cast<String, dynamic>(), 'rows': rows};
    _dirty = true;
    _saveTimer?.cancel();
    _saveTimer = Timer(const Duration(seconds: 1), saveNow);
  }

  /// The keyboard bar's table button with the caret in a cell (iOS): the table's actions.
  void _tableMenu(BuildContext bctx) {
    final t = _cellTable;
    if (t == null) return;
    final (r, col) = t.selected ?? (0, 0);
    showTouchMenu(
      context,
      Palette.of(context).look,
      globalRect(bctx),
      centered: true,
      [
        {'label': 'Add Row', 'icon': CupertinoIcons.arrow_down_to_line, 'id': 'row'},
        {'label': 'Add Column', 'icon': CupertinoIcons.arrow_right_to_line, 'id': 'col'},
        {'separator': true},
        {'label': 'Convert to Text', 'icon': CupertinoIcons.text_alignleft, 'id': 'text'},
        {'label': 'Share Table', 'icon': CupertinoIcons.share, 'id': 'share'},
        {'separator': true},
        {'label': 'Delete Table', 'icon': CupertinoIcons.trash, 'id': 'delete', 'destructive': true},
      ],
      (i) {
        switch (i['id']) {
          case 'row':
            t.addRow(r + 1);
          case 'col':
            t.addColumn(col + 1);
          case 'text':
            final text = t.asText();
            final id = (t.widget.key as ValueKey<String>).value.substring('table-in-'.length);
            FocusManager.instance.primaryFocus?.unfocus();
            _cellTable = null;
            c.removeToken(id);
            c.insertText(text);
            _focus.requestFocus();
            saveNow();
          case 'share':
            shareText(t.asText());
          case 'delete':
            t.deleteTable();
        }
      },
    );
  }

  Future<void> _addDrawing() async {
    _placeForInsert();
    final at = c.selection;
    _focus.unfocus();
    final r = await editDrawing(context, dir: s.home);
    if (r == null || r.strokes.isEmpty) return;
    final id = newId();
    c.selection = at;
    _adds.add({
      'id': id,
      'uti': utiPaper,
      'drawing': {'w': r.w, 'h': r.h, 'strokes': r.strokes},
      'fallback': r.png,
      'w': r.w,
      'h': r.h,
    });
    c.insertToken({'id': id, 'uti': utiPaper, 'alt': '', 'content': ''});
    saveNow();
  }

  Future<void> _openAttachment(Map<String, dynamic> a) async {
    if (_deleted) return _recoverAlert();
    final id = a['id'] as String;
    switch (a['kind']) {
      case 'drawing':
        final r = await editDrawing(context, dir: s.home, drawing: (a['drawing'] as Map?)?.cast() ?? {'w': a['w'], 'h': a['h'], 'strokes': []});
        if (r == null) return;
        _drawings[id] = {'w': r.w, 'h': r.h, 'strokes': r.strokes, 'fallback': r.png};
        _dirty = true;
        saveNow();
      case 'pdf':
        final f = pdfFile(a);
        if (f != null) openPdf(context, f, a['name'] as String? ?? 'PDF');
      case 'url':
        final u = a['url'] as String?;
        if (u != null) openUrl(u);
      case 'audio':
        final f = a['file'] as String?;
        if (f == null) return;
        if (_playing == id) {
          stopAudio();
          setState(() => _playing = null);
        } else {
          playAudio(f);
          setState(() => _playing = id);
        }
    }
  }

  void _attachmentMenu(BuildContext ctx, Map<String, dynamic> a) {
    final p = Palette.of(context);
    final id = a['id'] as String;
    final kind = a['kind'] as String? ?? '';
    final view = (a['view'] as num?)?.toInt() ?? 0;
    showTouchMenu(
      context,
      p.look,
      globalRect(ctx),
      [
        if (kind == 'image' || kind == 'pdf' || kind == 'scan' || kind == 'file')
          {
            'label': 'View As',
            'icon': CupertinoIcons.rectangle_grid_1x2,
            'kids': [
              for (final (v, l) in [(1, 'Small'), (2, 'Medium'), (0, 'Large')]) {'label': l, 'id': 'view', 'v': v, 'checked': view == v},
            ],
          },
        if (kind == 'pdf') {'label': 'Quick Look', 'icon': CupertinoIcons.eye, 'id': 'edit'},
        if (kind == 'drawing') {'label': 'Edit', 'icon': CupertinoIcons.pencil, 'id': 'edit'},
        {'separator': true},
        {'label': 'Delete', 'icon': CupertinoIcons.trash, 'id': 'delete', 'destructive': true},
      ],
      (i) {
        switch (i['id']) {
          case 'view':
            _views[id] = i['v'] as int;
            a['view'] = i['v'];
            setState(() {});
            saveNow();
          case 'edit':
            _openAttachment(a);
          case 'delete':
            c.removeToken(id);
            saveNow();
        }
      },
    );
  }

  void _openLink(Map<String, dynamic> tk) {
    final content = tk['content'] as String? ?? '';
    final m = RegExp(r'identifier=([^&]+)').firstMatch(content);
    if (m != null && s.lib.note(m[1]!) != null) {
      saveNow(leaving: true);
      s.open(m[1]!);
    }
  }

  /// A keyboard's menu shortcut in the note (keys.dart): Find, Add Link, the paragraph styles,
  /// checklist and lists; whether it applied.
  bool _command(String command) {
    if (!mounted || _note == null || _deleted) return false;
    switch (command) {
      case 'find':
        setState(() => _findQuery = '');
      case 'link':
        _addLink();
      case 'check':
        c.checklist();
      case 'title' || 'heading' || 'subheading' || 'body' || 'bullet' || 'dash' || 'number':
        c.paragraphStyle(command);
      default:
        return false;
    }
    return true;
  }

  Future<void> _addLink() async {
    final url = await askText(context, 'Add Link', hint: 'https://', ok: 'Done');
    if (url == null || url.trim().isEmpty) return;
    final sel = c.selection;
    if (sel.isCollapsed) {
      c.insertText(url.trim());
    }
    // the selected text becomes the link
    final a = sel.isCollapsed ? sel.start : sel.start, b = sel.isCollapsed ? sel.start + url.trim().length : sel.end;
    for (var k = a; k < b && k < c.styles.length; k++) {
      final st = c.styles[k];
      if (st.lead || st.tk != null) continue;
      c.styles[k] = CStyle(b: st.b, i: st.i, u: st.u, s: st.s, sc: st.sc, ln: url.trim(), hl: st.hl, c: st.c, f: st.f);
    }
    _edited();
    c.refresh();
  }

  void _recoverAlert() async {
    final (r, _) = await showAlert(
      context,
      title: 'Recently Deleted Note',
      message: 'Recently deleted notes can\'t be edited. To edit this note, you\'ll need to recover it.',
      actions: const [AlertAction('Cancel', cancel: true), AlertAction('Recover', bold: true)],
    );
    if (r == 1) await s.recover(widget.id);
  }

  // ------------------------------------------------------------ menus

  void _more(BuildContext bctx) {
    final p = Palette.of(context);
    final meta = s.lib.note(widget.id);
    if (meta == null) return;
    showTouchMenu(
      context,
      p.look,
      globalRect(bctx),
      [
        {
          'row': [
            // iOS 26's row: Scan, Pin, Lock
            {'label': 'Scan', 'icon': CupertinoIcons.doc_text_viewfinder, 'id': 'scan', 'enabled': _note != null && !_deleted},
            {'label': meta.pinned ? 'Unpin' : 'Pin Note', 'icon': meta.pinned ? CupertinoIcons.pin_slash : CupertinoIcons.pin, 'id': 'pin'},
            {'label': meta.locked ? 'Remove' : 'Lock', 'icon': meta.locked ? CupertinoIcons.lock_open : CupertinoIcons.lock, 'id': 'lock'},
          ],
        },
        {'label': 'Send a Copy', 'icon': CupertinoIcons.share, 'id': 'share', 'enabled': !meta.locked || _note != null},
        {'label': 'Find in Note', 'icon': CupertinoIcons.search, 'id': 'find'},
        {'label': 'Move Note', 'icon': CupertinoIcons.folder, 'id': 'move'},
        if (s.recent.where((r) => r != widget.id && s.lib.note(r) != null).isNotEmpty)
          {
            'label': 'Recent Notes',
            'icon': CupertinoIcons.clock,
            'kids': [
              for (final r in s.recent.where((r) => r != widget.id && s.lib.note(r) != null).take(8))
                {'label': s.lib.note(r)!.title.isEmpty ? 'New Note' : s.lib.note(r)!.title, 'id': 'recent', 'note': r},
            ],
          },
        {
          'label': 'Math Results',
          'icon': CupertinoIcons.function,
          'subtitle': s.settings.mathResults ? 'Insert Results' : 'Off',
          'kids': [
            {'label': 'Insert Results', 'id': 'math', 'on': true, 'checked': s.settings.mathResults},
            {'label': 'Off', 'id': 'math', 'on': false, 'checked': !s.settings.mathResults},
          ],
        },
        {
          'label': 'Lines & Grids',
          'icon': CupertinoIcons.square_grid_3x2,
          'kids': [
            for (final (k, l) in [('none', 'None'), ('lines', 'Lines'), ('grid', 'Grid')])
              {'label': l, 'id': 'lines', 'v': k, 'checked': s.settings.lines == k},
          ],
        },
        if (_tokens.attachments.isNotEmpty)
          {
            'label': 'Attachment View',
            'icon': CupertinoIcons.photo,
            'kids': [
              {'label': 'Set All to Small', 'id': 'allviews', 'v': 1},
              {'label': 'Set All to Large', 'id': 'allviews', 'v': 0},
            ],
          },
        {
          'label': 'Export as',
          'icon': CupertinoIcons.arrow_up_doc,
          'kids': [
            {'label': 'Export…', 'id': 'exportsheet'},
            {'separator': true},
            {'label': 'Markdown', 'id': 'export', 'f': 'md'},
            {'label': 'Plain Text', 'id': 'export', 'f': 'text'},
            {'label': 'HTML', 'id': 'export', 'f': 'html'},
            {'label': 'PDF', 'id': 'export', 'f': 'pdf'},
          ],
        },
        {'separator': true},
        {'label': 'Delete', 'icon': CupertinoIcons.trash, 'id': 'delete', 'destructive': true},
      ],
      (i) async {
        switch (i['id']) {
          case 'scan':
            _placeForInsert();
            final r = await scanDocuments(context);
            if (r != null) _addFile(r.pdf, pages: r.pages);
          case 'pin':
            s.pin(widget.id, !meta.pinned);
          case 'lock':
            saveNow();
            meta.locked ? await removeLock(context, s, widget.id) : await lockNote(context, s, widget.id);
          case 'share':
            saveNow();
            shareNote(s, widget.id);
          case 'export':
            saveNow();
            final r = await exportNotes(s, [widget.id], i['f'] as String, all: false, listName: '');
            if (r != null) s.say(r);
          case 'exportsheet':
            saveNow();
            if (context.mounted) showExportSheet(context, s, widget.id);
          case 'find':
            setState(() => _findQuery = '');
          case 'move':
            final to = await pickFolder(context, s, current: meta.folder);
            if (to != null) s.move(widget.id, to);
          case 'recent':
            saveNow(leaving: true);
            s.open(i['note'] as String);
          case 'math':
            s.settings.mathResults = i['on'] as bool;
            s.settings.save();
            setState(() {});
          case 'lines':
            s.settings.lines = i['v'] as String;
            s.settings.save();
            setState(() {});
          case 'allviews':
            for (final a in _tokens.attachments.values) {
              _views[a['id'] as String] = i['v'] as int;
              a['view'] = i['v'];
            }
            setState(() {});
            saveNow();
          case 'delete':
            _dirty = false;
            s.delete(widget.id);
            widget.onBack?.call();
        }
      },
    );
  }

  void _findNext(int d) {
    final q = _find.text.toLowerCase();
    if (q.isEmpty) return;
    final t = c.text.toLowerCase();
    final from = c.selection.isValid ? (d > 0 ? c.selection.end : c.selection.start - 1) : 0;
    var k = d > 0 ? t.indexOf(q, from) : t.lastIndexOf(q, from < 0 ? t.length : from);
    if (k < 0) k = d > 0 ? t.indexOf(q) : t.lastIndexOf(q);
    if (k < 0) return s.say('Not found');
    c.selection = TextSelection(baseOffset: k, extentOffset: k + q.length);
    _focus.requestFocus();
  }

  /// Replace: the match selected becomes the replacement, then on to the next one.
  void _replaceOne() {
    final q = _find.text;
    final sel = c.selection;
    if (q.isNotEmpty && sel.isValid && !sel.isCollapsed && sel.textInside(c.text).toLowerCase() == q.toLowerCase()) {
      c.value = TextEditingValue(text: c.text.replaceRange(sel.start, sel.end, _with.text), selection: TextSelection.collapsed(offset: sel.start + _with.text.length));
    }
    _findNext(1);
  }

  void _replaceAll() {
    final n = c.replaceAll(_find.text, _with.text);
    s.say(n == 0 ? 'Not found' : n == 1 ? '1 replaced' : '$n replaced');
  }

  void _openFormat() {
    SystemChannels.textInput.invokeMethod('TextInput.hide');
    setState(() => _format = true);
  }

  // ------------------------------------------------------------ the screen

  @override
  Widget build(BuildContext context) {
    final p = Palette.of(context);
    final mq = MediaQuery.of(context);
    final keyboard = mq.viewInsets.bottom > 80;
    final typing = (_focus.hasFocus || _cellTable != null) && (keyboard || _format);
    final side = widget.wide ? 28.0 : 20.0;
    c
      ..textColor = p.label
      ..leadColor = p.secondary
      ..checkColor = p.accent
      ..autoSortChecked = s.settings.autoSortChecked
      ..autoTag = s.settings.autoTag
      ..mathResults = s.settings.mathResults
      ..highlightText = p.highlightText
      ..highlightWash = p.highlightWash
      // (checklist items stand further apart, for their circles)
      ..paraStyle = (para) =>
          para.st == 'check' ? paraFont('body', s.settings.textSize, p.label).copyWith(height: 1.72) : paraFont(para.st, s.settings.textSize, p.label);
    final top = mq.padding.top + 64;
    WidgetsBinding.instance.addPostFrameCallback((_) => _placeChevrons());
    Widget body;
    if (!_loaded) {
      body = const SizedBox.shrink();
    } else if (_locked) {
      body = LockedNote(onView: _unlock);
    } else if (_note == null) {
      body = _cantOpen(p);
    } else {
      body = LayoutBuilder(
        builder: (ctx, box) {
          _tokens.width = box.maxWidth - side * 2;
          return CustomPaint(
            painter: s.settings.lines == 'none' ? null : _Lines(p, s.settings.lines == 'grid', top, _scroll),
            child: SingleChildScrollView(
              controller: _scroll,
              padding: EdgeInsets.fromLTRB(0, top, 0, 120 + (typing ? 0 : mq.padding.bottom)),
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.stretch,
                children: [
                  Padding(
                    padding: EdgeInsets.fromLTRB(side, 0, side, 8),
                    child: Text(longDate(((_note!['modified'] as num?) ?? 0).toDouble()), textAlign: TextAlign.center, style: Type.footnote(p.secondary)),
                  ),
                  Stack(
                    children: [
                      Padding(
                        key: _fieldKey,
                        padding: EdgeInsets.symmetric(horizontal: side),
                        child: Listener(
                          onPointerDown: (_) => _hadFocus = _focus.hasFocus,
                          child: TextField(
                            controller: c,
                            focusNode: _focus,
                            readOnly: _deleted,
                            maxLines: null,
                            keyboardType: TextInputType.multiline,
                            textCapitalization: TextCapitalization.sentences,
                            cursorColor: p.accent,
                            style: paraFont('body', s.settings.textSize, p.label),
                            // (each paragraph's own line height: no forced strut)
                            strutStyle: StrutStyle.disabled,
                            // iOS's caret handles and edit menu
                            selectionControls: cupertinoTextSelectionHandleControls,
                            contextMenuBuilder: (ctx, st) => CupertinoAdaptiveTextSelectionToolbar.editableText(editableTextState: st),
                            scrollPadding: EdgeInsets.only(bottom: _format ? 360 : 120),
                            decoration: const InputDecoration.collapsed(hintText: ''),
                            onTap: () {
                              if (_deleted) return _recoverAlert();
                              if (_format) setState(() => _format = false);
                              // a link tapped while not typing opens
                              final o = c.selection.baseOffset;
                              if (!_hadFocus && c.selection.isCollapsed && o >= 0) {
                                for (final k in [o, o - 1]) {
                                  if (k >= 0 && k < c.styles.length && c.styles[k].ln != null) {
                                    _focus.unfocus();
                                    openUrl(c.styles[k].ln!);
                                    break;
                                  }
                                }
                              }
                            },
                          ),
                        ),
                      ),
                      // the headings' collapse chevrons, in the margin
                      for (final (k, y) in _chevrons)
                        Positioned(
                          left: side - 26,
                          top: y - 16,
                          child: GestureDetector(
                            behavior: HitTestBehavior.opaque,
                            onTap: () => c.toggleCollapse(k),
                            child: SizedBox(
                              width: 28,
                              height: 32,
                              child: Icon(
                                k < c.paras.length && c.paras[k].col ? CupertinoIcons.chevron_right : CupertinoIcons.chevron_down,
                                size: 15,
                                color: p.accent,
                              ),
                            ),
                          ),
                        ),
                    ],
                  ),
                  // a tap under the text: the caret at its end
                  GestureDetector(
                    behavior: HitTestBehavior.opaque,
                    onTap: _deleted
                        ? _recoverAlert
                        : () {
                            c.selection = TextSelection.collapsed(offset: c.text.length);
                            _focus.requestFocus();
                          },
                    child: SizedBox(height: (box.maxHeight * 0.5).clamp(120, 600).toDouble(), width: double.infinity),
                  ),
                ],
              ),
            ),
          );
        },
      );
    }
    return ColoredBox(
      color: p.page,
      child: Stack(
        children: [
          Positioned.fill(child: body),
          // the scroll edge at the top
          Positioned(
            left: 0,
            right: 0,
            top: 0,
            height: top + 14,
            child: IgnorePointer(
              child: DecoratedBox(
                decoration: BoxDecoration(
                  gradient: LinearGradient(
                    begin: Alignment.topCenter,
                    end: Alignment.bottomCenter,
                    colors: [p.page, p.page.withValues(alpha: 0.85), p.page.withValues(alpha: 0)],
                    stops: const [0, 0.6, 1],
                  ),
                ),
              ),
            ),
          ),
          // (the bars' taps leave the text its focus)
          Positioned(
            left: 12,
            right: 12,
            top: mq.padding.top + 6,
            child: TextFieldTapRegion(child: widget.wide ? _wideBar(p, typing) : _topBar(p, typing)),
          ),
          if (_findQuery != null) Positioned(left: 12, right: 12, top: mq.padding.top + 62, child: _findBar(p)),
          if (!widget.wide && !_locked && _note != null) ..._foot(p, mq, keyboard, typing),
          if (widget.wide && _format)
            Positioned(
              right: 16,
              top: mq.padding.top + 62,
              width: 380,
              child: TextFieldTapRegion(
                child: FormatPanel(c: c, popover: true, onClose: () => setState(() => _format = false)),
              ),
            ),
        ],
      ),
    );
  }

  List<Widget> _foot(Palette p, MediaQueryData mq, bool keyboard, bool typing) {
    if (_format) {
      return [
        Positioned(
          left: 0,
          right: 0,
          bottom: mq.padding.bottom + mq.viewInsets.bottom,
          child: TextFieldTapRegion(
            child: FormatPanel(c: c, onClose: () => setState(() => _format = false)),
          ),
        ),
      ];
    }
    // (a cell's focus sits under the note's own field, so the note still "has" focus)
    if (keyboard && _cellTable != null) {
      return [
        Positioned(
          left: 10,
          right: 10,
          bottom: mq.viewInsets.bottom + 8,
          child: TextFieldTapRegion(child: _cellBar(p)),
        ),
      ];
    }
    if (keyboard && _focus.hasFocus) {
      return [
        Positioned(
          left: 10,
          right: 10,
          bottom: mq.viewInsets.bottom + 8,
          child: TextFieldTapRegion(
            child: Column(
              mainAxisSize: MainAxisSize.min,
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: [
                _suggestions(p),
                KeyboardBar(c: c, onFormat: _openFormat, onTable: _addTable, onAttach: _attach, onMarkup: _addDrawing, onLink: _addLink),
              ],
            ),
          ),
        ),
      ];
    }
    return [
      Positioned(
        left: 0,
        right: 0,
        bottom: 0,
        height: 90 + mq.padding.bottom,
        child: IgnorePointer(
          child: DecoratedBox(
            decoration: BoxDecoration(
              gradient: LinearGradient(begin: Alignment.bottomCenter, end: Alignment.topCenter, colors: [p.page, p.page.withValues(alpha: 0)]),
            ),
          ),
        ),
      ),
      Positioned(
        left: 16,
        right: 16,
        bottom: mq.padding.bottom + 8,
        child: ToolbarRow(
          children: [
            GlassGroup(
              items: [
                GlassItem(icon: CupertinoIcons.checkmark_circle, onTap: _deleted ? null : (_) => _checklistAtEnd()),
                GlassItem(icon: CupertinoIcons.paperclip, onTap: _deleted ? null : _attach),
                GlassItem(icon: markupIcon, onTap: _deleted ? null : (_) => _addDrawing()),
              ],
            ),
            const Spacer(),
            if (_deleted)
              GlassButton(icon: CupertinoIcons.folder, semantic: 'Recover', onTap: _recoverAlert)
            else
              GlassButton(icon: CupertinoIcons.square_pencil, semantic: 'New Note', onTap: widget.onNewNote),
          ],
        ),
      ),
    ];
  }

  /// The keyboard bar in a table's cell: the table button (its menu), then quick add row/column.
  Widget _cellBar(Palette p) {
    final t = _cellTable!;
    final (r, col) = t.selected ?? (0, 0);
    return Align(
      alignment: Alignment.centerLeft,
      child: GlassGroup(
        height: 52,
        items: [
          GlassItem(icon: CupertinoIcons.table, color: p.accent, onTap: _tableMenu),
          GlassItem(icon: CupertinoIcons.arrow_down_to_line, onTap: (_) => t.addRow(r + 1)),
          GlassItem(icon: CupertinoIcons.arrow_right_to_line, onTap: (_) => t.addColumn(col + 1)),
        ],
      ),
    );
  }

  /// Suggestions above the keyboard: notes for `>>` (a link to the note), tags for `#`.
  Widget _suggestions(Palette p) => ListenableBuilder(
    listenable: c,
    builder: (ctx, _) {
      final sug = c.suggesting;
      if (sug == null) return const SizedBox.shrink();
      final (kind, q, at) = sug;
      final ql = q.toLowerCase();
      final items = <(String, VoidCallback)>[];
      if (kind == 'link') {
        for (final n in s.lib.notes.where((n) => !n.deleted && n.id != widget.id && n.title.isNotEmpty && n.title.toLowerCase().contains(ql)).take(12)) {
          items.add((
            n.title,
            () => c.replaceWithToken(at, c.selection.baseOffset, {
              'id': newId(),
              'uti': utiLink,
              'alt': n.title,
              'content': 'applenotes://showNote?identifier=${n.id}',
            }),
          ));
        }
      } else {
        for (final t in s.lib.allTags.where((t) => t.toLowerCase().startsWith(ql) && t.toLowerCase() != ql).take(12)) {
          items.add(('#$t', () => c.replaceWithToken(at, c.selection.baseOffset, {'id': newId(), 'uti': utiHashtag, 'alt': '#$t', 'content': ''})));
        }
      }
      if (items.isEmpty) return const SizedBox.shrink();
      return Padding(
        padding: const EdgeInsets.only(bottom: 8),
        child: Glass(
          radius: 24,
          child: SizedBox(
            height: 48,
            child: ListView(
              scrollDirection: Axis.horizontal,
              padding: const EdgeInsets.symmetric(horizontal: 6),
              children: [
                for (final (label, pick) in items)
                  Pressable(
                    onTap: pick,
                    child: Container(
                      margin: const EdgeInsets.symmetric(horizontal: 3, vertical: 6),
                      padding: const EdgeInsets.symmetric(horizontal: 14),
                      alignment: Alignment.center,
                      decoration: BoxDecoration(color: p.fill, borderRadius: BorderRadius.circular(18)),
                      child: Row(
                        mainAxisSize: MainAxisSize.min,
                        children: [
                          if (kind == 'link')
                            Padding(
                              padding: const EdgeInsets.only(right: 6),
                              child: Icon(CupertinoIcons.doc_text, size: 16, color: p.accent),
                            ),
                          Text(label, style: Type.text(15, kind == 'tag' ? p.accent : p.label, weight: FontWeight.w500)),
                        ],
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

  /// The checklist button with the keyboard down: a checklist where the caret was (or at the end).
  void _checklistAtEnd() {
    if (!c.selection.isValid) c.selection = TextSelection.collapsed(offset: c.text.length);
    c.checklist();
    _focus.requestFocus();
  }

  Widget _topBar(Palette p, bool typing) {
    final canEdit = _note != null && !_deleted;
    return ToolbarRow(
      children: [
        if (widget.onBack != null) GlassButton(icon: CupertinoIcons.chevron_left, semantic: 'Back', onTap: widget.onBack),
        const Spacer(),
        if (_locked || _note?['locked'] == true) ...[
          GlassButton(icon: _locked ? CupertinoIcons.lock_fill : CupertinoIcons.lock_open_fill, semantic: 'Lock Now', onTap: _locked ? _unlock : s.lockNow),
          const SizedBox(width: 8),
        ],
        Builder(
          builder: (bctx) => GlassGroup(
            items: [
              if (canEdit && c.canUndo && typing) GlassItem(icon: CupertinoIcons.arrow_uturn_left, onTap: (_) => c.undo()),
              GlassItem(
                icon: CupertinoIcons.share,
                onTap: _note == null
                    ? null
                    : (_) {
                        saveNow();
                        shareNote(s, widget.id);
                      },
              ),
              GlassItem(icon: CupertinoIcons.ellipsis, onTap: _more),
            ],
          ),
        ),
        if (typing) ...[
          const SizedBox(width: 8),
          GlassButton(
            icon: CupertinoIcons.checkmark_alt,
            tinted: true,
            semantic: 'Done',
            onTap: () {
              _focus.unfocus();
              FocusManager.instance.primaryFocus?.unfocus();
              setState(() => _format = false);
              saveNow();
            },
          ),
        ],
      ],
    );
  }

  /// iPad's toolbar (IPAD-SIZES.md): [⤡][compose] (+ undo, redo while typing) at the note
  /// column's leading edge, then at the trailing edge [Aa ☑ ▦ 📎] [markup share ⋯] [🔍] (and
  /// Done while typing). As the column narrows: the search field becomes a 🔍 circle, Undo and
  /// Redo go into », then 📎, ▦ and ☑ in that order, and ⋯ merges into » once anything has;
  /// Aa, ⤡, compose, markup, share and 🔍 always stay. 12 dp between separate glass shapes.
  Widget _wideBar(Palette p, bool typing) {
    final canEdit = _note != null && !_deleted;
    const g = 12.0;
    double grp(int n) => 8 + 48.0 * n;
    final undo = typing && canEdit;
    // the stage: 0 everything; 1 search as a circle; 2 Undo/Redo in »; 3–5 📎, ▦, ☑ in »
    int tools(int st) => (3 - math.max(0, st - 2)).clamp(0, 3).toInt();
    double need(int st) {
      var w = 48 + g + 48 + g;
      if (undo && st < 2) w += grp(2) + g;
      w += grp(1 + tools(st)) + g + grp(3) + g + (st < 1 ? 200 : 48);
      if (typing) w += g + 48;
      return w;
    }

    return LayoutBuilder(
      builder: (ctx, box) {
        var st = 0;
        while (st < 5 && need(st) > box.maxWidth) {
          st++;
        }
        final n = tools(st);
        final all = <(IconData, String, void Function(BuildContext)?)>[
          (CupertinoIcons.checkmark_circle, 'Checklist', canEdit ? (_) => _checklistAtEnd() : null),
          (CupertinoIcons.table, 'Table', canEdit ? (_) => _addTable() : null),
          (CupertinoIcons.paperclip, 'Attach', canEdit ? _attach : null),
        ];
        final hidden = all.skip(n).toList();
        final undoHidden = undo && st >= 2;
        final overflow = hidden.isNotEmpty || undoHidden;
        void more(BuildContext bctx) => showTouchMenu(
          context,
          p.look,
          globalRect(bctx),
          centered: true,
          [
            if (undoHidden) ...[
              {'label': 'Undo', 'icon': CupertinoIcons.arrow_uturn_left, 'id': 'undo', 'enabled': c.canUndo},
              {'label': 'Redo', 'icon': CupertinoIcons.arrow_uturn_right, 'id': 'redo', 'enabled': c.canRedo},
              if (hidden.isNotEmpty) {'separator': true},
            ],
            for (final (icon, label, f) in hidden) {'label': label, 'icon': icon, 'id': label, 'enabled': f != null},
            {'separator': true},
            {'label': 'More', 'icon': CupertinoIcons.ellipsis_circle, 'id': 'more'},
          ],
          (i) {
            switch (i['id']) {
              case 'undo':
                c.undo();
              case 'redo':
                c.redo();
              case 'more':
                _more(bctx);
              default:
                for (final (_, label, f) in hidden) {
                  if (label == i['id']) f?.call(bctx);
                }
            }
          },
        );
        return ToolbarRow(
          children: [
            GlassButton(
              icon: widget.fullScreen ? CupertinoIcons.arrow_down_right_arrow_up_left : CupertinoIcons.arrow_up_left_arrow_down_right,
              semantic: widget.fullScreen ? 'Exit Full Screen' : 'Enter Full Screen',
              onTap: widget.onFullScreen,
            ),
            const SizedBox(width: g),
            GlassButton(icon: CupertinoIcons.square_pencil, semantic: 'New Note', onTap: widget.onNewNote),
            if (undo && !undoHidden) ...[
              const SizedBox(width: g),
              GlassGroup(
                items: [
                  GlassItem(icon: CupertinoIcons.arrow_uturn_left, onTap: c.canUndo ? (_) => c.undo() : null),
                  GlassItem(icon: CupertinoIcons.arrow_uturn_right, onTap: c.canRedo ? (_) => c.redo() : null),
                ],
              ),
            ],
            const Spacer(),
            const SizedBox(width: g),
            GlassGroup(
              items: [
                GlassItem(icon: CupertinoIcons.textformat, onTap: canEdit && _focus.hasFocus ? (_) => setState(() => _format = !_format) : null),
                for (final (icon, _, f) in all.take(n)) GlassItem(icon: icon, onTap: f),
              ],
            ),
            const SizedBox(width: g),
            GlassGroup(
              items: [
                GlassItem(icon: markupIcon, onTap: canEdit ? (_) => _addDrawing() : null),
                GlassItem(icon: CupertinoIcons.share, onTap: _note == null ? null : (_) => shareNote(s, widget.id)),
                overflow ? GlassItem(icon: CupertinoIcons.chevron_right_2, onTap: more) : GlassItem(icon: CupertinoIcons.ellipsis, onTap: _more),
              ],
            ),
            const SizedBox(width: g),
            // search: a field while there's room, else a circle (the list column searches)
            Builder(
              builder: (bctx) => st < 1
                  ? SizedBox(
                      width: 200,
                      child: Glass(
                        radius: 24,
                        child: Pressable(
                          onTap: _searchFromBar,
                          child: SizedBox(
                            height: 48,
                            child: Row(
                              children: [
                                const SizedBox(width: 14),
                                Icon(CupertinoIcons.search, size: opticalSize(CupertinoIcons.search, 20), color: p.secondary),
                                const SizedBox(width: 8),
                                Text('Search', style: Type.body(p.secondary)),
                              ],
                            ),
                          ),
                        ),
                      ),
                    )
                  : GlassButton(icon: CupertinoIcons.search, semantic: 'Search', onTap: _searchFromBar),
            ),
            if (typing) ...[
              const SizedBox(width: g),
              GlassButton(
                icon: CupertinoIcons.checkmark_alt,
                tinted: true,
                semantic: 'Done',
                onTap: () {
                  _focus.unfocus();
                  FocusManager.instance.primaryFocus?.unfocus();
                  setState(() => _format = false);
                  saveNow();
                },
              ),
            ],
          ],
        );
      },
    );
  }

  /// The note bar's search (iPad): the list column searches (out of Full Screen first).
  void _searchFromBar() {
    if (widget.fullScreen) widget.onFullScreen?.call();
    s.searchRequest.value++;
  }

  Widget _findBar(Palette p) => Glass(
    radius: 24,
    child: Column(
      mainAxisSize: MainAxisSize.min,
      children: [
        _findRow(p),
        // Find and Replace (the search icon turns it on and off): the replacement, Replace, All
        if (_replacing)
          SizedBox(
            height: 44,
            child: Row(
              children: [
                const SizedBox(width: 14),
                Icon(CupertinoIcons.arrow_2_squarepath, size: 18, color: p.secondary),
                const SizedBox(width: 8),
                Expanded(
                  child: TextField(
                    controller: _with,
                    style: Type.body(p.label),
                    cursorColor: p.accent,
                    onSubmitted: (_) => _replaceOne(),
                    decoration: InputDecoration.collapsed(hintText: 'Replace', hintStyle: Type.body(p.secondary)),
                  ),
                ),
                Pressable(
                  onTap: _replaceOne,
                  child: Padding(padding: const EdgeInsets.symmetric(horizontal: 10, vertical: 10), child: Text('Replace', style: Type.body(p.accent))),
                ),
                Pressable(
                  onTap: _replaceAll,
                  child: Padding(padding: const EdgeInsets.fromLTRB(10, 10, 16, 10), child: Text('All', style: Type.body(p.accent))),
                ),
              ],
            ),
          ),
      ],
    ),
  );

  Widget _findRow(Palette p) => SizedBox(
      height: 48,
      child: Row(
        children: [
          Pressable(
            onTap: () => setState(() => _replacing = !_replacing),
            child: SizedBox(
              width: 40,
              height: 44,
              child: Padding(
                padding: const EdgeInsets.only(left: 6),
                child: Icon(CupertinoIcons.search, size: 18, color: _replacing ? p.accent : p.secondary),
              ),
            ),
          ),
          Expanded(
            child: TextField(
              controller: _find,
              autofocus: true,
              style: Type.body(p.label),
              cursorColor: p.accent,
              textInputAction: TextInputAction.search,
              onSubmitted: (_) => _findNext(1),
              decoration: InputDecoration.collapsed(hintText: 'Find in Note', hintStyle: Type.body(p.secondary)),
            ),
          ),
          Pressable(
            onTap: () => _findNext(-1),
            child: SizedBox.square(dimension: 44, child: Icon(CupertinoIcons.chevron_up, size: 20, color: p.label)),
          ),
          Pressable(
            onTap: () => _findNext(1),
            child: SizedBox.square(dimension: 44, child: Icon(CupertinoIcons.chevron_down, size: 20, color: p.label)),
          ),
          Pressable(
            onTap: () => setState(() => _findQuery = null),
            child: SizedBox.square(dimension: 44, child: Icon(CupertinoIcons.xmark, size: 18, color: p.secondary)),
          ),
        ],
      ),
    );
}

/// Lines & Grids: ruled lines or a grid behind the text, moving with it.
class _Lines extends CustomPainter {
  _Lines(this.p, this.grid, this.top, this.scroll) : super(repaint: scroll);
  final Palette p;
  final bool grid;
  final double top;
  final ScrollController scroll;
  @override
  void paint(Canvas c, Size s) {
    final off = scroll.hasClients ? scroll.offset : 0.0;
    final step = grid ? 24.0 : 32.0;
    final paint = Paint()
      ..color = p.separator.withValues(alpha: 0.5)
      ..strokeWidth = 0.6;
    for (var y = top + 30 - off % step; y < s.height; y += step) {
      c.drawLine(Offset(0, y), Offset(s.width, y), paint);
    }
    if (grid) {
      for (var x = step / 2; x < s.width; x += step) {
        c.drawLine(Offset(x, 0), Offset(x, s.height), paint);
      }
    }
  }

  @override
  bool shouldRepaint(_Lines o) => o.grid != grid || o.p != p;
}
