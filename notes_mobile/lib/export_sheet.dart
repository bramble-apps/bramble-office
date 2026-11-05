// Export (the note's … menu > Export as): the desktop's File > Export… sheet in touch size, one
// card per format with its own Export button — Text; HTML with its Collapsible Section (Expand
// all, Collapse all, As shown); PDF with its Orientation and Collapsible Section (Expand all, As
// shown); Markdown with "Include additional data" (front matter: title, dates, folder, tags). For
// this note (one file) or every note in the list (each its own file, in a folder named after the
// list). nt-ffi writes the files with nt-core's export code, as the desktop does; PDF pages are
// laid out here (pdf_export.dart).
import 'dart:io';

import 'package:file_picker/file_picker.dart';
import 'package:flutter/cupertino.dart';
import 'package:flutter/services.dart';

import 'model.dart';
import 'pdf_export.dart';
import 'state.dart';
import 'theme.dart';
import 'ui/glass.dart';
import 'ui/sheet.dart';
import 'ui/touch_menu.dart';

const _system = MethodChannel('app.bramble.notes/system');

/// (`ids`: a keyboard's multi-selection, exported in place of the one note)
void showExportSheet(BuildContext context, AppState s, String id, {List<String>? ids}) => showSheet<void>(
  context,
  title: 'Export',
  builder: (ctx) => _ExportPage(state: s, id: id, ids: ids),
);

/// What the sheet remembers while the app runs (the desktop's sheet keeps its choices too).
class _Choices {
  bool all = false;
  String html = 'expand', pdf = 'expand';
  bool landscape = false, data = false;
}

final _choices = _Choices();

const _sectionNames = {'expand': 'Expand all', 'collapse': 'Collapse all', 'shown': 'As shown'};

class _ExportPage extends StatefulWidget {
  const _ExportPage({required this.state, required this.id, this.ids});
  final AppState state;
  final String id;
  final List<String>? ids;
  @override
  State<_ExportPage> createState() => _ExportPageState();
}

class _ExportPageState extends State<_ExportPage> {
  AppState get s => widget.state;

  /// The note, or the selected ones.
  List<String> get _mine => widget.ids ?? [widget.id];
  _Choices get c => _choices;
  String? _busy;

  /// What the last export did, on its button for a moment (the app's notices are under the sheet).
  (String, String)? _said;

  /// The list's notes in its order (all but the locked ones nt-ffi leaves out).
  List<String> get _listIds => [
    for (final (_, l) in groupNotes(s.lib.notesIn(s.place), s.settings.sort, byDate: s.settings.groupByDate, reverse: s.settings.reverse))
      for (final n in l)
        if (!n.deleted) n.id,
  ];

  String get _listName => s.lib.title(s.place);

  Widget _choice(Palette p, String label, String value, List<(String, String)> options, ValueChanged<String> pick) => Builder(
    builder: (bctx) => CardRow(
      label: label,
      value: options.firstWhere((o) => o.$1 == value, orElse: () => options.first).$2,
      chevron: true,
      onTap: () => showTouchMenu(context, p.look, globalRect(bctx), [
        for (final (k, l) in options) {'label': l, 'v': k, 'checked': k == value},
      ], (i) => setState(() => pick(i['v'] as String))),
    ),
  );

  Widget _button(Palette p, String kind, String label) =>
      CardRow(
        label: _busy == kind ? 'Exporting…' : (_said?.$1 == kind ? _said!.$2 : label),
        color: _said?.$1 == kind && _busy != kind ? p.secondary : p.accent,
        onTap: _busy != null ? null : () => _export(kind),
      );

  Future<void> _export(String kind) async {
    setState(() => _busy = kind);
    try {
      final r = await exportNotes(s, c.all ? _listIds : _mine, kind, all: c.all || _mine.length > 1, listName: _listName);
      _show(kind, r);
    } catch (e) {
      _show(kind, 'Export failed: $e');
    }
    if (mounted) setState(() => _busy = null);
  }

  void _show(String kind, String? message) {
    if (message == null || !mounted) return;
    setState(() => _said = (kind, message));
    Future<void>.delayed(const Duration(milliseconds: 2500), () {
      if (mounted && _said == (kind, message)) setState(() => _said = null);
    });
  }

  @override
  Widget build(BuildContext context) {
    final p = Palette.of(context);
    final n = _listIds.length;
    return ListView(
      padding: const EdgeInsets.only(top: 4, bottom: 24),
      children: [
        Card2(
          header: 'Export',
          footer: c.all ? 'Each note is its own file, in a folder named “$_listName” inside the one you choose.' : null,
          children: [
            CardRow(label: _mine.length > 1 ? 'The ${_mine.length} Selected Notes' : 'This Note', checked: !c.all, onTap: () => setState(() => c.all = false)),
            CardRow(label: 'All Notes in “$_listName” ($n)', checked: c.all, onTap: n == 0 ? null : () => setState(() => c.all = true)),
          ],
        ),
        Card2(header: 'Text', children: [_button(p, 'text', 'Export to Text')]),
        Card2(
          header: 'HTML',
          children: [
            _choice(p, 'Collapsible Section', c.html, [for (final k in ['expand', 'collapse', 'shown']) (k, _sectionNames[k]!)], (v) => c.html = v),
            _button(p, 'html', 'Export to HTML'),
          ],
        ),
        Card2(
          header: 'PDF',
          children: [
            _choice(p, 'Orientation', c.landscape ? 'l' : 'p', const [('p', 'Portrait'), ('l', 'Landscape')], (v) => c.landscape = v == 'l'),
            _choice(p, 'Collapsible Section', c.pdf, [for (final k in ['expand', 'shown']) (k, _sectionNames[k]!)], (v) => c.pdf = v),
            _button(p, 'pdf', 'Export to PDF'),
          ],
        ),
        Card2(
          header: 'Markdown',
          footer: 'Additional data: the title, when the note was created and last updated, its folder and its tags, at the top of the file.',
          children: [
            CardRow(
              label: 'Include additional data',
              trailing: CupertinoSwitch(value: c.data, activeTrackColor: const Color(0xFF34C759), onChanged: (v) => setState(() => c.data = v)),
            ),
            _button(p, 'md', 'Export to Markdown'),
          ],
        ),
      ],
    );
  }
}

/// Writes the notes `ids` as `kind` (text, html, pdf, md) with the sheet's choices and saves them
/// where the person chooses: one file, or (several, or a note with attachments) a folder. Answers
/// what to tell them, or null when they cancelled.
Future<String?> exportNotes(AppState s, List<String> ids, String kind, {required bool all, required String listName}) async {
  if (ids.isEmpty) return null;
  final c = _choices;
  final dir = await Directory.systemTemp.createTemp('notes-export-');
  try {
    final List<File> files;
    if (kind == 'pdf') {
      final r = await s.engine.run('export_files', {'ids': ids, 'format': 'pdf', 'sections': c.pdf}) as Map;
      files = [];
      final used = <String>{};
      for (final n in (r['notes'] as List).cast<Map>()) {
        final stem = fileStem(n['title'] as String? ?? '');
        var name = '$stem.pdf';
        for (var k = 2; !used.add(name); k++) {
          name = '$stem $k.pdf';
        }
        files.add(await File('${dir.path}/$name').writeAsBytes(await notePdf(n, landscape: c.landscape)));
      }
    } else {
      final r = await s.engine.run('export_files', {'ids': ids, 'format': kind, 'sections': c.html, 'data': c.data, 'dir': dir.path}) as Map;
      files = [for (final f in (r['files'] as List).cast<String>()) File(f)];
    }
    if (files.isEmpty) return 'Nothing to export.';
    final name = files.first.uri.pathSegments.last;
    final withFiles = Directory('${dir.path}/Attachments').existsSync();
    if (!all && !withFiles) {
      final saved = await FilePicker.saveFile(fileName: name, bytes: await files.first.readAsBytes(), mimeType: _mime(kind));
      return saved == null ? null : 'Exported $name';
    }
    // a folder: the list's name, or the note's (its file and its Attachments)
    final folder = fileStem(all ? listName : name.replaceAll(RegExp(r'\.[a-z]+$'), ''));
    final n = await _saveFolder(dir, folder);
    if (n == null) return null;
    return all ? 'Exported ${files.length} ${files.length == 1 ? 'note' : 'notes'} to “$folder”' : 'Exported $name';
  } finally {
    dir.delete(recursive: true).ignore();
  }
}

/// nt-core's file_stem: a file name from a title.
String fileStem(String title) {
  final s = title.replaceAll(RegExp(r'[/\\:\x00]'), '-').trim().replaceFirst(RegExp(r'^\.+'), '');
  return s.isEmpty ? 'Untitled' : (s.length > 200 ? s.substring(0, 200) : s);
}

String _mime(String kind) => switch (kind) {
  'md' => 'text/markdown',
  'html' => 'text/html',
  'pdf' => 'application/pdf',
  _ => 'text/plain',
};

/// The files in `dir` copied into a new folder `name` inside one the person picks; how many.
Future<int?> _saveFolder(Directory dir, String name) async {
  if (Platform.isAndroid) return await _system.invokeMethod<int>('saveFolder', {'dir': dir.path, 'name': name});
  final to = await FilePicker.getDirectoryPath();
  if (to == null) return null;
  var target = Directory('$to/$name');
  for (var k = 2; target.existsSync(); k++) {
    target = Directory('$to/$name $k');
  }
  var n = 0;
  await for (final e in dir.list(recursive: true)) {
    if (e is! File) continue;
    final out = File('${target.path}/${e.path.substring(dir.path.length + 1)}');
    await out.parent.create(recursive: true);
    await e.copy(out.path);
    n++;
  }
  return n;
}
