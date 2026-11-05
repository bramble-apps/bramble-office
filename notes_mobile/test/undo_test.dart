// The library's Undo (undo.dart): names, order, and a new action clearing Redo.
import 'package:flutter_test/flutter_test.dart';
import 'package:notes/undo.dart';

void main() {
  test('undo and redo in order, by name; a new action clears redo', () async {
    final log = <String>[];
    final u = LibraryUndo();
    LibraryAction act(String name) => LibraryAction(name, undo: () async => log.add('undo $name'), redo: () async => log.add('redo $name'));
    expect(await u.undo(), isNull);
    u.record(act('Delete Note'));
    u.record(act('Move Notes'));
    expect(u.undoName, 'Move Notes');
    expect(await u.undo(), 'Move Notes');
    expect(u.undoName, 'Delete Note');
    expect(u.redoName, 'Move Notes');
    expect(await u.redo(), 'Move Notes');
    expect(await u.undo(), 'Move Notes');
    u.record(act('Pin Notes'));
    expect(u.redoName, isNull);
    expect(log, ['undo Move Notes', 'redo Move Notes', 'undo Move Notes']);
  });
}
