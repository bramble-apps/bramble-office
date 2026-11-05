// The library's Undo (the desktop's Edit > Undo while the note isn't being edited, Ctrl+Z /
// Ctrl+Shift+Z on a keyboard): Delete Note(s), Move Notes, Pin / Unpin Notes, New Folder and
// Delete Folder, each named as the menu names it ("Undo Delete Notes"). Inside the note, Ctrl+Z
// stays the text's undo.
import 'package:flutter/foundation.dart';

class LibraryAction {
  LibraryAction(this.name, {required this.undo, required this.redo});

  /// What it did ("Delete Note", "Move Notes"): the menu's "Undo …" / "Redo …".
  final String name;
  final Future<void> Function() undo, redo;
}

class LibraryUndo extends ChangeNotifier {
  final _done = <LibraryAction>[], _undone = <LibraryAction>[];

  /// (as many as the desktop keeps)
  static const _most = 50;

  void record(LibraryAction a) {
    _done.add(a);
    if (_done.length > _most) _done.removeAt(0);
    _undone.clear();
    notifyListeners();
  }

  String? get undoName => _done.isEmpty ? null : _done.last.name;
  String? get redoName => _undone.isEmpty ? null : _undone.last.name;

  /// Undoes the last action; its name (null: nothing to undo).
  Future<String?> undo() async {
    if (_done.isEmpty) return null;
    final a = _done.removeLast();
    await a.undo();
    _undone.add(a);
    notifyListeners();
    return a.name;
  }

  Future<String?> redo() async {
    if (_undone.isEmpty) return null;
    final a = _undone.removeLast();
    await a.redo();
    _done.add(a);
    notifyListeners();
    return a.name;
  }
}
