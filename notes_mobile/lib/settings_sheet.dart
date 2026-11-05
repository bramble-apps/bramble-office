// Settings (iOS Settings > Apps > Notes, here inside the app): viewing, new notes, checklists,
// lines and grids, the notes password, importing a Mac's Notes library, the library as a .bnotes
// file (export and import), the appearance.
import 'dart:io';

import 'package:file_picker/file_picker.dart';
import 'package:flutter/cupertino.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';

import 'model.dart';
import 'state.dart';
import 'theme.dart';
import 'lock.dart';
import 'sync.dart';
import 'ui/sheet.dart';

void showSettings(BuildContext context, AppState s) => showSheet<void>(
  context,
  title: 'Settings',
  builder: (ctx) => _Settings(state: s),
);

class _Settings extends StatefulWidget {
  const _Settings({required this.state});
  final AppState state;
  @override
  State<_Settings> createState() => _SettingsState();
}

class _SettingsState extends State<_Settings> {
  Settings get st => widget.state.settings;
  bool _importing = false;

  // (the Sync row follows each sync's outcome while Settings is open)
  // (on the next frame: the Sync page saves as it closes, when nothing may rebuild)
  void _synced() => WidgetsBinding.instance.addPostFrameCallback((_) {
    if (mounted) setState(() {});
  });

  @override
  void initState() {
    super.initState();
    widget.state.sync?.addListener(_synced);
  }

  @override
  void dispose() {
    widget.state.sync?.removeListener(_synced);
    super.dispose();
  }

  /// what the Library card is doing ('export', 'import'), and what it last did
  String? _library, _libraryDone;

  static const _system = MethodChannel('app.bramble.notes/system');

  /// Export Library: every note, folder and attachment as one .bnotes file (nt-core's lossless
  /// copy, the desktop's File > Export Library), where the person chooses.
  Future<void> _exportLibrary() async {
    final s = widget.state;
    setState(() => _library = 'export');
    final dir = await Directory.systemTemp.createTemp('notes-bnotes-');
    final now = DateTime.now();
    final name = 'Notes ${now.year}-${now.month.toString().padLeft(2, '0')}-${now.day.toString().padLeft(2, '0')}.bnotes';
    final file = '${dir.path}/$name';
    try {
      final r = await s.engine.run('export_bnotes', {'path': file}) as Map;
      final bool? saved;
      if (Platform.isAndroid) {
        saved = await _system.invokeMethod<bool>('saveFile', {'path': file, 'name': name, 'mime': 'application/octet-stream'});
      } else {
        final to = await FilePicker.getDirectoryPath(dialogTitle: 'Export Library To');
        if (to != null) await File(file).copy('$to/$name');
        saved = to != null;
      }
      if (saved == true) _libraryDone = 'Exported ${r['notes']} notes to $name';
    } catch (e) {
      _libraryDone = 'Export failed: $e';
    } finally {
      dir.delete(recursive: true).ignore();
    }
    if (mounted) setState(() => _library = null);
  }

  /// Import a Library: a .bnotes file's notes added to this library (ones already here stay).
  Future<void> _importLibrary() async {
    final s = widget.state;
    final path = (await FilePicker.pickFile(dialogTitle: 'A .bnotes library'))?.path;
    if (path == null) return;
    setState(() => _library = 'import');
    try {
      final r = await s.engine.run('import_bnotes', {'path': path}) as Map;
      await s.reload();
      final n = r['notes'] as num? ?? 0, skipped = r['skipped'] as num? ?? 0;
      _libraryDone = 'Imported $n ${n == 1 ? 'note' : 'notes'}${skipped > 0 ? ' ($skipped already here)' : ''}';
    } catch (e) {
      _libraryDone = 'Import failed: $e';
    }
    if (mounted) setState(() => _library = null);
  }

  void _set(VoidCallback f) {
    setState(f);
    st.save();
    widget.state.changed();
  }

  Future<void> _import() async {
    final s = widget.state;
    String? path;
    try {
      path = await FilePicker.getDirectoryPath(dialogTitle: 'The Mac\'s group.com.apple.notes folder');
    } catch (_) {}
    if (path == null) {
      path = (await FilePicker.pickFile(dialogTitle: 'NoteStore.sqlite'))?.path;
    }
    if (path == null) return;
    setState(() => _importing = true);
    try {
      await s.engine.run('import', {'path': path});
      await s.reload();
      s.say('Imported.');
    } catch (e) {
      s.say('$e');
    }
    if (mounted) setState(() => _importing = false);
  }

  @override
  Widget build(BuildContext context) {
    final p = Palette.of(context);
    final s = widget.state;
    Widget sw(bool v, ValueChanged<bool> f) => CupertinoSwitch(value: v, activeTrackColor: const Color(0xFF34C759), onChanged: (x) => _set(() => f(x)));
    return ListView(
      padding: const EdgeInsets.only(top: 4),
      children: [
        Card2(
          header: 'Viewing',
          children: [
            for (final (b, l) in [(SortBy.edited, 'Date Edited'), (SortBy.created, 'Date Created'), (SortBy.title, 'Title')])
              CardRow(label: 'Sort by $l', checked: st.sort == b, onTap: () => _set(() => st.sort = b)),
            CardRow(label: 'Group Notes by Date', trailing: sw(st.groupByDate, (v) => st.groupByDate = v)),
          ],
        ),
        Card2(
          header: 'New notes start with',
          children: [
            for (final (k, l) in [('title', 'Title'), ('heading', 'Heading'), ('subheading', 'Subheading'), ('body', 'Body')])
              CardRow(label: l, checked: st.newNoteStyle == k, onTap: () => _set(() => st.newNoteStyle = k)),
          ],
        ),
        Card2(
          header: 'Sort checked items',
          footer: 'Choose to sort checked items in checklists automatically or manually.',
          children: [
            CardRow(label: 'Manually', checked: !st.autoSortChecked, onTap: () => _set(() => st.autoSortChecked = false)),
            CardRow(label: 'Automatically', checked: st.autoSortChecked, onTap: () => _set(() => st.autoSortChecked = true)),
          ],
        ),
        Card2(
          children: [
            CardRow(label: 'Auto Convert to Tag', trailing: sw(st.autoTag, (v) => st.autoTag = v)),
            CardRow(label: 'Save to Photos', trailing: sw(st.saveToPhotos, (v) => st.saveToPhotos = v)),
            CardRow(label: 'Always Resume to Last Quick Note', trailing: sw(st.resumeQuickNote, (v) => st.resumeQuickNote = v)),
          ],
          footer: 'Quick Notes open from Quick Settings (Quick Note) and from a long press on the Notes icon.',
        ),
        Card2(
          header: 'Note backgrounds in dark mode',
          children: [
            CardRow(label: 'Dark', checked: st.noteBackground == 'dark', onTap: () => _set(() => st.noteBackground = 'dark')),
            CardRow(label: 'Light', checked: st.noteBackground == 'light', onTap: () => _set(() => st.noteBackground = 'light')),
          ],
        ),
        Card2(
          header: 'Lines & Grids',
          children: [
            for (final (k, l) in [('none', 'None'), ('lines', 'Lines'), ('grid', 'Grid')])
              CardRow(label: l, checked: st.lines == k, onTap: () => _set(() => st.lines = k)),
          ],
        ),
        Card2(
          header: 'Text size',
          children: [
            Padding(
              padding: const EdgeInsets.symmetric(horizontal: 12),
              child: Row(
                children: [
                  Text('A', style: Type.text(14, p.label)),
                  Expanded(
                    child: CupertinoSlider(
                      value: st.textSize,
                      min: 13,
                      max: 25,
                      divisions: 6,
                      activeColor: p.accent,
                      onChanged: (v) => _set(() => st.textSize = v),
                    ),
                  ),
                  Text('A', style: Type.text(24, p.label)),
                ],
              ),
            ),
          ],
        ),
        Card2(
          header: 'Appearance',
          children: [
            for (final (k, l) in [('system', 'Automatic'), ('light', 'Light'), ('dark', 'Dark')])
              CardRow(label: l, checked: st.appearance == k, onTap: () => _set(() => st.appearance = k)),
          ],
        ),
        // the desktop's Look: its colour themes and Glass
        Card2(
          header: 'Color',
          children: [
            Padding(
              padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 10),
              child: Wrap(
                spacing: 4,
                runSpacing: 4,
                children: [
                  for (final (id, name, main, accent) in colorThemes)
                    Semantics(
                      button: true,
                      selected: st.colorTheme == id,
                      label: name,
                      child: GestureDetector(
                        onTap: () => _set(() => st.colorTheme = id),
                        child: SizedBox.square(
                          dimension: 48,
                          child: CustomPaint(painter: _Swatch(Color(0xFF000000 | main), Color(0xFF000000 | accent), st.colorTheme == id, p.label)),
                        ),
                      ),
                    ),
                ],
              ),
            ),
          ],
        ),
        Card2(
          footer: 'Glass shows the toolbar buttons as Liquid Glass over the note; off, they are solid.',
          children: [CardRow(label: 'Glass Buttons', trailing: sw(st.glass, (v) => st.glass = v))],
        ),
        Card2(
          header: 'Sync',
          // (the last sync's failure, as it said it, so it's seen without opening the page)
          footer: widget.state.sync?.account['on'] == true && widget.state.sync?.account['failed'] == true
              ? syncMessage('${widget.state.sync?.account['summary']}')
              : 'Keeps your notes on a WebDAV server (Nextcloud, Koofr, a NAS), with the same account as Notes on your computer.',
          children: [
            CardRow(
              label: 'WebDAV Sync',
              value: widget.state.sync?.account['on'] != true
                  ? 'Off'
                  : (widget.state.sync?.running == true ? 'Syncing…' : (widget.state.sync?.account['failed'] == true ? 'Not Syncing' : 'On')),
              chevron: true,
              onTap: () => showSyncSettings(context, widget.state),
            ),
          ],
        ),
        Card2(
          header: 'Password',
          footer: 'Locked notes are encrypted with this password, as on a Mac, iPhone or iPad.',
          children: [
            CardRow(
              label: s.hasPassword ? 'Change Password' : 'Set Password',
              chevron: true,
              onTap: () => passwordSheet(context, s, change: s.hasPassword),
            ),
          ],
        ),
        Card2(
          header: 'Import',
          footer:
              'Copies a Mac\'s Notes library (its group.com.apple.notes folder, or the NoteStore.sqlite in it) into this one. The Mac\'s library is only read.',
          children: [CardRow(label: _importing ? 'Importing…' : 'Import from a Mac\'s Notes', chevron: !_importing, onTap: _importing ? null : _import)],
        ),
        Card2(
          header: 'Library',
          footer:
              '${_libraryDone == null ? '' : '$_libraryDone.\n\n'}A .bnotes file holds every note, folder and attachment, and opens in Notes on a computer too. Importing one adds its notes; notes already here stay as they are.',
          children: [
            CardRow(label: _library == 'export' ? 'Exporting…' : 'Export Library…', onTap: _library != null ? null : _exportLibrary),
            CardRow(label: _library == 'import' ? 'Importing…' : 'Import a Library…', onTap: _library != null ? null : _importLibrary),
          ],
        ),
        Card2(
          children: [CardRow(label: 'Notes for Android', value: '0.4.0')],
        ),
      ],
    );
  }
}

/// A colour theme's swatch (as the desktop's): its main colour, a dot of its accent, a ring when
/// chosen.
class _Swatch extends CustomPainter {
  _Swatch(this.main, this.accent, this.on, this.ring);
  final Color main, accent, ring;
  final bool on;
  @override
  void paint(Canvas c, Size s) {
    final o = s.center(Offset.zero);
    c.drawCircle(o, 15, Paint()..color = main);
    if (accent != main) c.drawCircle(o + const Offset(7.5, 7.5), 6, Paint()..color = accent);
    if (on) {
      c.drawCircle(
        o,
        19,
        Paint()
          ..style = PaintingStyle.stroke
          ..strokeWidth = 2.5
          ..color = ring,
      );
    }
  }

  @override
  bool shouldRepaint(_Swatch o) => o.main != main || o.on != on || o.ring != ring;
}
