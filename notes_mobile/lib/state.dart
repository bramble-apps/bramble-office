// The app's state: the open library (as the lists show it), what's chosen (the place and the
// note), the settings, and the library's operations the screens share (new note, pin, move,
// delete, lock), each written straight through the engine and then read back.
import 'dart:convert';
import 'dart:io';

import 'package:flutter/foundation.dart';
import 'package:flutter/services.dart';

import 'doc.dart';
import 'share.dart';
import 'engine.dart';
import 'model.dart';
import 'sync.dart';
import 'undo.dart';

/// Notes' settings (iOS Settings > Apps > Notes, and the app's own).
class Settings {
  SortBy sort = SortBy.edited;

  /// Sort By > Oldest First / Descending.
  bool reverse = false;
  bool groupByDate = true;

  /// New Notes Start With: title, heading, subheading, body.
  String newNoteStyle = 'title';

  /// Sort Checked Items: move ticked items to the end of their checklist.
  bool autoSortChecked = false;

  /// Lines & Grids: none, lines, grid.
  String lines = 'none';

  /// Appearance: system, light, dark.
  String appearance = 'system';

  /// The desktop's look options: its colour theme (theme.dart colorThemes; default: Notes'
  /// yellow) and Glass (the toolbar buttons as Liquid Glass, else solid).
  String colorTheme = 'default';
  bool glass = true;

  /// Always Resume to Last Quick Note.
  bool resumeQuickNote = false;

  /// Auto Convert to Tag (#word and a space), Save to Photos (pictures taken in Notes), Note
  /// Backgrounds in dark mode (dark or light pages).
  bool autoTag = true, saveToPhotos = true;

  /// Math Results: answers inserted after "=".
  bool mathResults = true;
  String noteBackground = 'dark';

  /// Text size: the body's points (Notes' 17 at the middle stop).
  double textSize = 17;

  /// The notes list as cards (Gallery) instead of rows.
  bool gallery = false;

  File? _file;

  void load(String dir) {
    _file = File('$dir/settings.json');
    try {
      final j = jsonDecode(_file!.readAsStringSync()) as Map<String, dynamic>;
      sort = SortBy.values.firstWhere((s) => s.name == j['sort'], orElse: () => SortBy.edited);
      groupByDate = j['groupByDate'] != false;
      reverse = j['reverse'] == true;
      newNoteStyle = j['newNoteStyle'] as String? ?? 'title';
      autoSortChecked = j['autoSortChecked'] == true;
      lines = j['lines'] as String? ?? 'none';
      appearance = j['appearance'] as String? ?? 'system';
      colorTheme = j['colorTheme'] as String? ?? 'default';
      glass = j['glass'] != false;
      textSize = (j['textSize'] as num?)?.toDouble() ?? 17;
      gallery = j['gallery'] == true;
      resumeQuickNote = j['resumeQuickNote'] == true;
      autoTag = j['autoTag'] != false;
      mathResults = j['mathResults'] != false;
      saveToPhotos = j['saveToPhotos'] != false;
      noteBackground = j['noteBackground'] as String? ?? 'dark';
    } catch (_) {}
  }

  void save() {
    try {
      _file?.writeAsStringSync(
        jsonEncode({
          'sort': sort.name,
          'groupByDate': groupByDate,
          'reverse': reverse,
          'newNoteStyle': newNoteStyle,
          'autoSortChecked': autoSortChecked,
          'lines': lines,
          'appearance': appearance,
          'colorTheme': colorTheme,
          'glass': glass,
          'textSize': textSize,
          'gallery': gallery,
          'resumeQuickNote': resumeQuickNote,
          'autoTag': autoTag,
          'mathResults': mathResults,
          'saveToPhotos': saveToPhotos,
          'noteBackground': noteBackground,
        }),
      );
    } catch (_) {}
  }
}

/// Where problems are written (main.dart sets it: the app's external files folder, notes-log.txt):
/// what failed, when, and the stack, for when someone asks what went wrong.
String? problemLog;

void logProblem(String what, Object e, StackTrace? st) {
  final line = '${DateTime.now().toIso8601String()} $what: $e${st == null ? '' : '\n$st'}\n';
  debugPrint('Notes problem: $line');
  final p = problemLog;
  if (p == null) return;
  try {
    final f = File(p);
    // (kept small: the last 200 KB)
    if (f.existsSync() && f.lengthSync() > 200000) f.writeAsStringSync(f.readAsStringSync().substring(100000));
    f.writeAsStringSync(line, mode: FileMode.append);
  } catch (_) {}
}

class AppState extends ChangeNotifier {
  AppState(this.engine, this.home);
  final Engine engine;

  /// The app's support folder (the library, settings, drawings' pictures).
  final String home;
  final settings = Settings();
  Library lib = Library.empty();

  Place place = const AllNotes();
  String? noteId;

  /// Several notes picked in the list with a keyboard (Shift / Ctrl with a tap, Shift+arrows,
  /// Ctrl+A): delete, move, pin and export act on them all, and the note column says
  /// "N Notes Selected". Empty, or the open note alone: no multi-selection.
  final selection = <String>{};

  /// Where a Shift range starts (the note picked last without Shift).
  String? anchor;

  /// The library's Undo (Ctrl+Z outside the note).
  final undo = LibraryUndo();

  /// The iPad layout (list and note side by side): where the list's keys apply.
  bool columns = false;

  /// The open note's editor, for the menu shortcuts that act inside it (Ctrl+Shift+T, Ctrl+K…):
  /// answers whether it did something.
  bool Function(String command)? editorCommand;

  /// The home screen's keys (keys.dart), for the app's key Focus (main.dart).
  bool Function(KeyEvent e)? keyHandler;

  /// The notes the selection is about: the multi-selection, else the open note.
  List<String> get picked => selection.length > 1 ? [for (final id in listOrder) if (selection.contains(id)) id] : [?noteId];

  /// The list's notes in its order (pinned first, then by its sort), as the list shows them.
  List<String> get listOrder => [
    for (final (_, l) in groupNotes(lib.notesIn(place), settings.sort, byDate: settings.groupByDate, reverse: settings.reverse))
      for (final n in l) n.id,
  ];

  /// Picks `id` alone (a tap, an arrow): the open note, the anchor of a later Shift range.
  void pickOne(String? id) {
    selection.clear();
    anchor = id;
    open(id);
  }

  /// Shift: the notes from the anchor to `id`; the open note is `id`.
  void pickRange(String id) {
    final order = listOrder;
    final a = order.indexOf(anchor ?? noteId ?? id), b = order.indexOf(id);
    if (b < 0) return;
    final from = a < 0 ? b : a;
    selection
      ..clear()
      ..addAll(order.sublist(from < b ? from : b, (from < b ? b : from) + 1));
    open(id);
  }

  /// Ctrl: one more, or one less.
  void pickToggle(String id) {
    if (selection.isEmpty && noteId != null) selection.add(noteId!);
    if (!selection.add(id)) {
      selection.remove(id);
      if (noteId == id) noteId = selection.isEmpty ? null : selection.last;
    } else {
      noteId = id;
    }
    anchor = id;
    notifyListeners();
  }

  void pickAll() {
    selection
      ..clear()
      ..addAll(listOrder);
    notifyListeners();
  }

  /// The notes password once given this session: locked notes open without asking again
  /// until Lock Now (Notes keeps them open a while).
  String? password;

  /// A message for a moment at the foot of the screen.
  final message = ValueNotifier<String?>(null);

  /// Bumped when the open note was changed from outside the editor (locked, moved, recovered).
  final noteRevision = ValueNotifier<int>(0);

  /// WebDAV sync (sync.dart), and the editor's save of what's being typed (run before a sync).
  SyncRunner? sync;
  VoidCallback? beforeSync;

  /// The note bar's 🔍 on iPad widths: the list column starts a search.
  final searchRequest = ValueNotifier<int>(0);

  /// How the first open's conversion to the library as files went (nt-ffi `open`'s migration:
  /// its counts, or `error` when it stayed on the old library), for the app to say once.
  Map<String, dynamic>? libraryNotice;

  /// Bumped when a sync brought notes in (an open note not being typed in is read again).
  final syncedIn = ValueNotifier<int>(0);

  Future<void> reload({bool fromSync = false}) async {
    if (!fromSync) sync?.changed();
    try {
      lib = Library((await engine.run('library') as Map).cast());
    } catch (e) {
      say('$e');
    }
    if (place is InFolder && lib.folder((place as InFolder).id) == null) place = defaultPlace;
    selection.removeWhere((id) => lib.note(id) == null);
    notifyListeners();
  }

  /// The Notes folder.
  Place get defaultPlace {
    final d = lib.defaultFolder;
    return d == null ? const AllNotes() : InFolder(d.id);
  }

  void say(String m) {
    message.value = m;
    Future.delayed(const Duration(seconds: 3), () {
      if (message.value == m) message.value = null;
    });
  }

  /// The last error a call answered (the editor shows it when a note can't be read).
  String? lastError;

  dynamic call(String m, [Map<String, dynamic> a = const {}]) {
    try {
      lastError = null;
      return engine.call(m, a);
    } on EngineError catch (e) {
      lastError = e.message;
      logProblem('$m ${a['id'] ?? ''}', e, null);
      say(e.message);
      return null;
    }
  }

  /// Something the screens show changed (a setting).
  void changed() => notifyListeners();

  void choose(Place p) {
    place = p;
    selection.clear();
    notifyListeners();
  }

  void open(String? id) {
    noteId = id;
    if (id != null) {
      recent.remove(id);
      recent.insert(0, id);
      if (recent.length > 10) recent.removeLast();
    }
    notifyListeners();
  }

  /// Recent Notes (the note … menu): the notes opened lately, newest first.
  final recent = <String>[];

  /// A new empty note where the list is (the Notes folder from a Smart Folder, a tag or All).
  Future<String?> newNote() async {
    String? folder;
    final p = place;
    if (p is InFolder) {
      final f = lib.folder(p.id);
      if (f != null && !f.smart && !f.trash) folder = f.id;
    }
    // (an account's All: its own Notes folder)
    if (p is AllNotes && p.account != null) folder = lib.defaultIn(p.account!)?.id;
    final r = call('create', {
      'folder': ?folder,
      'body': [
        {'st': settings.newNoteStyle, 'sp': []},
      ],
    });
    if (r == null) return null;
    final id = (r as Map)['id'] as String;
    await reload();
    selection.clear();
    anchor = id;
    noteId = id;
    notifyListeners();
    return id;
  }

  /// The Quick Notes folder (made the first time it's needed, as Notes does).
  Future<String?> quickNotesFolder() async {
    final f = lib.folders.where((f) => f.name == 'Quick Notes' && f.parent == null && !f.smart).firstOrNull;
    if (f != null) return f.id;
    return createFolder('Quick Notes');
  }

  /// Quick Note (the Quick Settings tile, the launcher shortcut): the last Quick Note again when
  /// Settings says so and it's from the last five minutes, else a new one in Quick Notes.
  Future<String?> quickNote() async {
    final folder = await quickNotesFolder();
    if (folder == null) return null;
    final now = DateTime.now().millisecondsSinceEpoch / 1000;
    if (settings.resumeQuickNote) {
      final last = lib.notes.where((n) => n.folder == folder && !n.deleted && !n.locked && now - n.modified < 300).toList()
        ..sort((a, b) => b.modified.compareTo(a.modified));
      if (last.isNotEmpty) {
        place = InFolder(folder);
        noteId = last.first.id;
        notifyListeners();
        return noteId;
      }
    }
    place = InFolder(folder);
    return newNote();
  }

  /// Something shared to Notes: a new note with the text (its first line the title, or the
  /// subject's) and the files as attachments.
  Future<String?> sharedNote({String? subject, String? text, List<String> files = const []}) async {
    // a Markdown or text file: its text becomes the note (Import to Notes)
    final docs = files.where((f) => RegExp(r'\.(md|markdown|txt)$', caseSensitive: false).hasMatch(f)).toList();
    if (docs.isNotEmpty && (text == null || text.isEmpty)) {
      String? last;
      for (final d in docs) {
        final body = markdownBody(File(d).readAsStringSync());
        final r = call('create', {'body': body});
        if (r != null) last = (r as Map)['id'] as String;
      }
      files = [
        for (final f in files)
          if (!docs.contains(f)) f,
      ];
      if (files.isEmpty) {
        await reload();
        place = defaultPlace;
        noteId = last;
        notifyListeners();
        return last;
      }
    }
    final lines = [if (subject != null && subject.trim().isNotEmpty) subject.trim(), ...?text?.split('\n')];
    final body = <Map<String, dynamic>>[
      for (final (i, l) in lines.indexed)
        {
          'st': i == 0 ? 'title' : 'body',
          'sp': [
            // a shared link is a link
            if (l.isNotEmpty) {'t': l, if (RegExp(r'^https?://\S+$').hasMatch(l.trim())) 'ln': l.trim()},
          ],
        },
    ];
    if (body.isEmpty) body.add({'st': 'title', 'sp': []});
    final r = call('create', {'body': body});
    if (r == null) return null;
    final id = (r as Map)['id'] as String;
    if (files.isNotEmpty) {
      final add = <Map<String, dynamic>>[];
      for (final f in files) {
        final uti = (call('uti', {'path': f}) as Map?)?['uti'] as String? ?? 'public.data';
        final aid = newId();
        add.add({'id': aid, 'file': f, 'uti': uti});
        body.add({
          'st': 'body',
          'sp': [
            {
              't': '\uFFFC',
              'tk': {'id': aid, 'uti': uti, 'alt': '', 'content': ''},
            },
          ],
        });
      }
      body.add({'st': 'body', 'sp': []});
      call('save', {'id': id, 'body': body, 'add': add});
      call('close', {'id': id});
    }
    await reload();
    place = defaultPlace;
    noteId = id;
    notifyListeners();
    return id;
  }

  Future<void> pin(String id, bool on) => pinNotes([id], on);

  /// Pin / Unpin for several (the selection); one Undo Pin Notes.
  Future<void> pinNotes(List<String> ids, bool on) async {
    final done = [for (final id in ids) if (lib.note(id)?.pinned != on) id];
    Future<void> set(bool v) async {
      for (final id in done) {
        call('pin', {'id': id, 'on': v});
      }
      await reload();
    }

    await set(on);
    if (done.isNotEmpty) {
      final what = '${on ? 'Pin' : 'Unpin'} Note${done.length == 1 ? '' : 's'}';
      undo.record(LibraryAction(what, undo: () => set(!on), redo: () => set(on)));
    }
  }

  /// Delete: into Recently Deleted (from there: gone for good).
  Future<void> delete(String id) => deleteNotes([id]);

  /// Delete for several (the selection): to Recently Deleted as one Undo Delete Notes (back in
  /// their folders, pins kept); the ones already in Recently Deleted are erased (no undo).
  Future<void> deleteNotes(List<String> ids) async {
    final done = <(String, String, bool)>[];
    for (final id in ids) {
      final n = lib.note(id);
      if (n == null) continue;
      if (n.deleted) {
        call('erase', {'id': id});
      } else {
        call('delete', {'id': id});
        done.add((id, n.folder, n.pinned));
      }
    }
    if (noteId != null && ids.contains(noteId)) noteId = null;
    selection.removeAll(ids);
    await reload();
    if (done.isEmpty) return;
    undo.record(
      LibraryAction(
        'Delete Note${done.length == 1 ? '' : 's'}',
        undo: () async {
          for (final (id, folder, pinned) in done) {
            call('recover', {'id': id, 'folder': lib.folder(folder) == null ? null : folder});
            if (pinned) call('pin', {'id': id, 'on': true});
          }
          await reload();
          noteRevision.value++;
        },
        redo: () async {
          for (final (id, _, _) in done) {
            call('delete', {'id': id});
          }
          if (done.any((d) => d.$1 == noteId)) noteId = null;
          await reload();
        },
      ),
    );
  }

  /// Duplicate Note (Ctrl+D): a copy in its folder, opened.
  Future<String?> duplicate(String id) async {
    final r = call('duplicate', {'id': id});
    if (r == null) return null;
    await reload();
    final made = (r as Map)['id'] as String;
    pickOne(made);
    return made;
  }

  Future<void> recover(String id) async {
    call('recover', {'id': id});
    await reload();
    noteRevision.value++;
  }

  Future<void> move(String id, String folder) => moveNotes([id], folder);

  /// Move for several (the selection); one Undo Move Notes.
  Future<void> moveNotes(List<String> ids, String folder) async {
    final done = [for (final id in ids) if (lib.note(id) case final n? when n.folder != folder) (id, n.folder)];
    Future<void> put(bool back) async {
      for (final (id, from) in done) {
        call('move', {'id': id, 'folder': back ? from : folder});
      }
      await reload();
      noteRevision.value++;
    }

    await put(false);
    if (done.isNotEmpty) undo.record(LibraryAction('Move Note${done.length == 1 ? '' : 's'}', undo: () => put(true), redo: () => put(false)));
  }

  bool get hasPassword => (call('has_password') as Map?)?['on'] == true;
  String? get passwordHint => (call('has_password') as Map?)?['hint'] as String?;

  bool checkPassword(String pw) => (call('check_password', {'pw': pw}) as Map?)?['ok'] == true;

  Future<bool> lock(String id, String pw) async {
    final ok = call('lock', {'id': id, 'pw': pw}) != null;
    if (ok) password = pw;
    await reload();
    noteRevision.value++;
    return ok;
  }

  Future<void> removeLock(String id, String pw) async {
    call('remove_lock', {'id': id, 'pw': pw});
    await reload();
    noteRevision.value++;
  }

  /// Lock Now: locked notes need the password again.
  void lockNow() {
    password = null;
    noteRevision.value++;
    notifyListeners();
  }

  /// (`account`: a top-level folder in that account, the one the list is in)
  Future<String?> createFolder(String name, {String? parent, String? account}) async {
    var id = _makeFolder(name, parent, account);
    await reload();
    if (id != null) {
      undo.record(
        LibraryAction(
          'New Folder',
          undo: () async {
            if (id != null) call('delete_folder', {'id': id});
            await reload();
          },
          redo: () async {
            id = _makeFolder(name, parent != null && lib.folder(parent) == null ? null : parent, account);
            await reload();
          },
        ),
      );
    }
    return id;
  }

  /// A Smart Folder made again from its rules (Undo Delete Folder).
  String? _makeSmart(Folder f) {
    final rules = (call('smart_parse', {'json': f.query ?? ''}) as Map?)?.cast<String, dynamic>() ?? const {};
    final r = call('create_smart', {'name': f.name, ...rules}) as Map?;
    return r?['id'] as String?;
  }

  String? _makeFolder(String name, String? parent, String? account) =>
      (call('create_folder', {'name': name, 'parent': ?parent, 'account': ?account}) as Map?)?['id'] as String?;

  Future<void> renameFolder(String id, String name) async {
    call('rename_folder', {'id': id, 'name': name});
    await reload();
  }

  /// Delete Folder: its notes go to Recently Deleted, its subfolders with it; Undo Delete Folder
  /// makes them again (new ids, parents first) and puts the notes back in, pins kept.
  Future<void> deleteFolder(String id) async {
    final root = lib.folder(id);
    if (root == null) return;
    final tree = <Folder>[root];
    for (var k = 0; k < tree.length; k++) {
      tree.addAll(lib.folders.where((f) => f.parent == tree[k].id));
    }
    final ids = {for (final f in tree) f.id};
    final notes = root.smart ? <(String, String, bool)>[] : [for (final n in lib.notes) if (!n.deleted && ids.contains(n.folder)) (n.id, n.folder, n.pinned)];
    var current = id;
    Future<void> remove() async {
      call('delete_folder', {'id': current});
      if (place == InFolder(current)) place = defaultPlace;
      await reload();
    }

    await remove();
    undo.record(
      LibraryAction(
        'Delete Folder',
        undo: () async {
          final made = <String, String>{};
          for (final f in tree) {
            final parent = f.parent == null ? null : (made[f.parent] ?? f.parent);
            final nid = f.smart ? _makeSmart(f) : _makeFolder(f.name, parent, parent == null && lib.manyAccounts ? f.account : null);
            if (nid != null) made[f.id] = nid;
          }
          for (final (nid, folder, pinned) in notes) {
            call('recover', {'id': nid, 'folder': made[folder] ?? folder});
            if (pinned) call('pin', {'id': nid, 'on': true});
          }
          current = made[id] ?? id;
          await reload();
        },
        redo: remove,
      ),
    );
  }

  Future<void> moveFolder(String id, String? parent) async {
    call('move_folder', {'id': id, 'parent': ?parent});
    await reload();
  }

  Future<void> emptyTrash() async {
    call('empty_trash');
    await reload();
  }
}
