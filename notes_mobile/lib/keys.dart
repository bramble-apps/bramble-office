// A hardware keyboard on a tablet: the desktop Notes' keys (as on a Mac, Ctrl standing for Cmd).
// The list (iPad layout, when no text has the caret): ↑ / ↓ move, Home / End, Page Up / Down,
// Shift with them extends a multi-selection, Ctrl+A selects every note, Delete / Backspace
// deletes the selection; Ctrl+Z / Ctrl+Shift+Z undo or redo the library's last action. The
// menus' shortcuts anywhere: Ctrl+N new note, Ctrl+Shift+N new folder, Ctrl+D duplicate, Ctrl+F
// find in note, Ctrl+Alt+F the list's search, Ctrl+1/2/3 list / gallery / attachments, Ctrl+P
// print, Ctrl+K link, Ctrl+Shift+T/H/J/B title / heading / subheading / body, Ctrl+Shift+L
// checklist, Ctrl+Shift+7/8/9 bulleted / dashed / numbered, Ctrl+Shift+0 / . / , zoom. Text
// keeps its own keys while it has the caret (Ctrl+A its text, Ctrl+Z its undo).
import 'dart:io';

import 'package:flutter/services.dart';
import 'package:flutter/widgets.dart';

import 'export_sheet.dart';
import 'folders.dart';
import 'notes_list.dart';
import 'pdf_export.dart';
import 'state.dart';

class NotesKeys {
  NotesKeys(this.s, this.context, {required this.active, required this.newNote});
  final AppState s;

  /// The home screen's context (sheets and alerts open from it).
  final BuildContext Function() context;

  /// Whether the home screen is in front (no sheet, menu or alert over it).
  final bool Function() active;
  final Future<void> Function() newNote;

  static const _page = 10;
  static const _system = MethodChannel('app.bramble.notes/system');

  /// Some text has the caret (the note, a field): its keys are its own.
  static bool get _inText => FocusManager.instance.primaryFocus?.context?.findAncestorStateOfType<EditableTextState>() != null;

  bool handle(KeyEvent e) {
    if (e is KeyUpEvent || !active()) return false;
    final k = HardwareKeyboard.instance;
    final ctrl = k.isControlPressed || k.isMetaPressed, shift = k.isShiftPressed, alt = k.isAltPressed;
    final key = e.logicalKey, phys = e.physicalKey;
    final text = _inText;
    if (ctrl && e is KeyDownEvent) {
      final done = _menu(key, phys, shift: shift, alt: alt, text: text);
      if (done) return true;
    }
    if (text || !s.columns) return false;
    return _list(key, ctrl: ctrl, shift: shift);
  }

  /// The menus' shortcuts; whether one ran.
  bool _menu(LogicalKeyboardKey key, PhysicalKeyboardKey phys, {required bool shift, required bool alt, required bool text}) {
    bool editor(String command) => s.editorCommand?.call(command) ?? false;
    if (shift) {
      final style = {
        LogicalKeyboardKey.keyT: 'title',
        LogicalKeyboardKey.keyH: 'heading',
        LogicalKeyboardKey.keyJ: 'subheading',
        LogicalKeyboardKey.keyB: 'body',
        LogicalKeyboardKey.keyL: 'check',
      }[key];
      if (style != null) return editor(style);
      // (by the key, not its shifted character: Shift+7 types "&")
      final list = {PhysicalKeyboardKey.digit7: 'bullet', PhysicalKeyboardKey.digit8: 'dash', PhysicalKeyboardKey.digit9: 'number'}[phys];
      if (list != null) return editor(list);
      final zoom = {PhysicalKeyboardKey.digit0: 0, PhysicalKeyboardKey.period: 1, PhysicalKeyboardKey.comma: -1}[phys];
      if (zoom != null) {
        _zoom(zoom);
        return true;
      }
      if (key == LogicalKeyboardKey.keyN) {
        showNewFolder(context(), s);
        return true;
      }
      if (key == LogicalKeyboardKey.keyZ && !text) {
        _redo();
        return true;
      }
      return false;
    }
    switch (key) {
      case LogicalKeyboardKey.keyN:
        newNote();
        return true;
      case LogicalKeyboardKey.keyD when s.noteId != null && !alt:
        s.duplicate(s.noteId!);
        return true;
      case LogicalKeyboardKey.keyF:
        if (alt || !editor('find')) s.searchRequest.value++;
        return true;
      case LogicalKeyboardKey.keyK:
        return editor('link');
      case LogicalKeyboardKey.keyP when s.noteId != null:
        _print(s.noteId!);
        return true;
      case LogicalKeyboardKey.keyZ when !text:
        _undo();
        return true;
      case LogicalKeyboardKey.keyA when !text && s.columns:
        s.pickAll();
        return true;
    }
    final view = {PhysicalKeyboardKey.digit1: 1, PhysicalKeyboardKey.digit2: 2, PhysicalKeyboardKey.digit3: 3}[phys];
    if (view != null) {
      if (view == 3) {
        viewAttachments(context(), s, s.lib.notesIn(s.place), s.pickOne);
      } else {
        s.settings.gallery = view == 2;
        s.settings.save();
        s.changed();
      }
      return true;
    }
    return false;
  }

  /// The list's keys (the iPad layout, no caret in text).
  bool _list(LogicalKeyboardKey key, {required bool ctrl, required bool shift}) {
    if (key == LogicalKeyboardKey.delete || key == LogicalKeyboardKey.backspace) {
      if (s.picked.isEmpty) return false;
      deleteSelection(context(), s);
      return true;
    }
    if (key == LogicalKeyboardKey.escape && s.selection.length > 1) {
      s.pickOne(s.noteId);
      return true;
    }
    final order = s.listOrder;
    if (order.isEmpty) return false;
    final at = s.noteId == null ? -1 : order.indexOf(s.noteId!);
    final int to;
    switch (key) {
      case LogicalKeyboardKey.arrowDown:
        to = at < 0 ? 0 : at + 1;
      case LogicalKeyboardKey.arrowUp:
        to = at < 0 ? 0 : at - 1;
      case LogicalKeyboardKey.home:
        to = 0;
      case LogicalKeyboardKey.end:
        to = order.length - 1;
      case LogicalKeyboardKey.pageDown:
        to = (at < 0 ? 0 : at) + _page;
      case LogicalKeyboardKey.pageUp:
        to = (at < 0 ? 0 : at) - _page;
      default:
        return false;
    }
    final id = order[to.clamp(0, order.length - 1)];
    if (shift) {
      s.anchor ??= s.noteId ?? id;
      s.pickRange(id);
    } else {
      s.pickOne(id);
    }
    return true;
  }

  Future<void> _undo() async {
    final name = await s.undo.undo();
    s.say(name == null ? 'Nothing to Undo' : 'Undo $name');
  }

  Future<void> _redo() async {
    final name = await s.undo.redo();
    s.say(name == null ? 'Nothing to Redo' : 'Redo $name');
  }

  /// View > Zoom In / Zoom Out / Actual Size: the notes' text size.
  void _zoom(int step) {
    final st = s.settings;
    st.textSize = step == 0 ? 17 : (st.textSize + 2 * step).clamp(13, 25).toDouble();
    st.save();
    s.changed();
  }

  /// Print (Ctrl+P): the note as the PDF export lays it out, to Android's printing.
  Future<void> _print(String id) async {
    if (!Platform.isAndroid) {
      s.say('Printing isn’t available here.');
      return;
    }
    try {
      final r = await s.engine.run('export_files', {'ids': [id], 'format': 'pdf', 'sections': 'expand'}) as Map;
      final notes = (r['notes'] as List).cast<Map>();
      if (notes.isEmpty) return;
      final dir = await Directory.systemTemp.createTemp('notes-print-');
      final name = fileStem(notes.first['title'] as String? ?? '');
      final f = await File('${dir.path}/$name.pdf').writeAsBytes(await notePdf(notes.first));
      await _system.invokeMethod('print', {'path': f.path, 'name': name});
    } catch (e) {
      s.say('Printing failed: $e');
    }
  }
}
