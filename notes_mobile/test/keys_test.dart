// A hardware keyboard's keys (keys.dart) and the library's Undo (state.dart) on the real engine:
// run with the Linux engine on the library path, e.g.
//   LD_LIBRARY_PATH=../notes-linux/target/release flutter test test/keys_test.dart
// (skipped when the engine can't be loaded).
import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:notes/engine.dart';
import 'package:notes/keys.dart';
import 'package:notes/model.dart';
import 'package:notes/state.dart';

void main() {
  Engine? engine;
  try {
    engine = Engine();
  } catch (_) {}
  final skip = engine == null ? 'the engine (libnotes_core.so) isn\'t on the library path' : null;

  Future<AppState> library() async {
    final dir = Directory.systemTemp.createTempSync('notes-keys-');
    engine!.call('open', {'dir': dir.path});
    engine.call('demo');
    final s = AppState(engine, dir.path)
      ..columns = true
      ..place = const AllNotes();
    await s.reload();
    return s;
  }

  test('Undo Delete Notes puts them back in their folders, pinned again', () async {
    final s = await library();
    final pinned = s.lib.notes.firstWhere((n) => n.pinned && !n.deleted);
    final other = s.lib.notes.firstWhere((n) => !n.pinned && !n.deleted);
    await s.deleteNotes([pinned.id, other.id]);
    expect(s.lib.note(pinned.id)!.deleted, isTrue);
    expect(s.undo.undoName, 'Delete Notes');
    expect(await s.undo.undo(), 'Delete Notes');
    expect(s.lib.note(pinned.id)!.deleted, isFalse);
    expect(s.lib.note(pinned.id)!.pinned, isTrue);
    expect(s.lib.note(other.id)!.folder, other.folder);
    expect(await s.undo.redo(), 'Delete Notes');
    expect(s.lib.note(other.id)!.deleted, isTrue);
  }, skip: skip);

  test('Undo Move, Pin, New Folder and Delete Folder', () async {
    final s = await library();
    final n = s.lib.notes.firstWhere((n) => !n.deleted && !n.pinned);
    final folder = (await s.createFolder('Keys Test'))!;
    expect(s.undo.undoName, 'New Folder');
    await s.moveNotes([n.id], folder);
    expect(s.lib.note(n.id)!.folder, folder);
    await s.pinNotes([n.id], true);
    expect(await s.undo.undo(), 'Pin Note');
    expect(s.lib.note(n.id)!.pinned, isFalse);
    await s.deleteFolder(folder);
    expect(s.lib.folder(folder), isNull);
    expect(s.lib.note(n.id)!.deleted, isTrue);
    expect(await s.undo.undo(), 'Delete Folder');
    final again = s.lib.folders.firstWhere((f) => f.name == 'Keys Test');
    expect(s.lib.note(n.id)!.deleted, isFalse);
    expect(s.lib.note(n.id)!.folder, again.id);
    expect(await s.undo.undo(), 'Move Note');
    expect(s.lib.note(n.id)!.folder, n.folder);
  }, skip: skip);

  testWidgets('arrows move, Shift+arrows extend, Escape ends it, Ctrl+A selects all, a Ctrl-tap takes one away', (t) async {
    final s = (await t.runAsync(library))!;
    final order = s.listOrder;
    expect(order.length, greaterThan(2));
    late BuildContext ctx;
    final keys = NotesKeys(s, () => ctx, active: () => true, newNote: () async {});
    await t.pumpWidget(
      MaterialApp(
        home: Focus(
          autofocus: true,
          onKeyEvent: (node, e) => keys.handle(e) ? KeyEventResult.handled : KeyEventResult.ignored,
          child: Builder(
            builder: (c) {
              ctx = c;
              return const SizedBox();
            },
          ),
        ),
      ),
    );
    await t.sendKeyEvent(LogicalKeyboardKey.arrowDown);
    expect(s.noteId, order[0]);
    await t.sendKeyEvent(LogicalKeyboardKey.arrowDown);
    expect(s.noteId, order[1]);
    await t.sendKeyDownEvent(LogicalKeyboardKey.shiftLeft);
    await t.sendKeyEvent(LogicalKeyboardKey.arrowDown);
    await t.sendKeyUpEvent(LogicalKeyboardKey.shiftLeft);
    expect(s.selection, {order[1], order[2]});
    expect(s.picked, [order[1], order[2]]);
    await t.sendKeyEvent(LogicalKeyboardKey.escape);
    expect(s.selection, isEmpty);
    await t.sendKeyEvent(LogicalKeyboardKey.home);
    await t.sendKeyDownEvent(LogicalKeyboardKey.shiftLeft);
    await t.sendKeyEvent(LogicalKeyboardKey.end);
    await t.sendKeyUpEvent(LogicalKeyboardKey.shiftLeft);
    expect(s.selection.length, order.length);
    await t.sendKeyEvent(LogicalKeyboardKey.arrowUp);
    expect(s.selection, isEmpty);
    expect(s.noteId, order[order.length - 2]);
    await t.sendKeyDownEvent(LogicalKeyboardKey.controlLeft);
    await t.sendKeyEvent(LogicalKeyboardKey.keyA);
    await t.sendKeyUpEvent(LogicalKeyboardKey.controlLeft);
    expect(s.selection.length, order.length);
    // a Ctrl-tap takes one away
    s.pickToggle(order[0]);
    expect(s.selection.length, order.length - 1);
    expect(s.selection.contains(order[0]), isFalse);
  }, skip: skip != null);
}
