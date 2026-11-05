// The slim sync progress row (sync.dart SyncProgress): it waits 300 ms before showing, glides
// open, holds the finished bar 0.8 s, glides shut; scrolled down, the notes in view don't move.
// Run with the Linux engine on the library path (as keys_test.dart); skipped without it.
// ignore_for_file: invalid_use_of_protected_member, invalid_use_of_visible_for_testing_member
import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:notes/engine.dart';
import 'package:notes/state.dart';
import 'package:notes/sync.dart';

void main() {
  Engine? engine;
  try {
    engine = Engine();
  } catch (_) {}

  testWidgets('waits, glides open, holds, glides shut; keeps the view when scrolled', (t) async {
    final dir = Directory.systemTemp.createTempSync('notes-progress-');
    engine!.call('open', {'dir': dir.path});
    final s = AppState(engine, dir.path);
    final sync = SyncRunner(s);
    final scroll = ScrollController();
    double rowHeight() => t.getSize(find.byType(SyncProgress, skipOffstage: false)).height;
    await t.pumpWidget(
      MaterialApp(
        home: Scaffold(
          body: ListView(
            controller: scroll,
            children: [
              SyncProgress(sync: sync),
              for (var i = 0; i < 60; i++) SizedBox(height: 40, child: Text('row $i', key: ValueKey(i))),
            ],
          ),
        ),
      ),
    );
    expect(rowHeight(), 0);
    void set(bool on) {
      sync.running = on;
      sync.notifyListeners();
    }

    // a quick sync: never shows
    set(true);
    await t.pump(const Duration(milliseconds: 200));
    set(false);
    await t.pump(const Duration(milliseconds: 400));
    expect(rowHeight(), 0);
    // a longer one: after 300 ms it glides open
    set(true);
    await t.pump(const Duration(milliseconds: 310));
    await t.pump(const Duration(milliseconds: 100));
    final mid = rowHeight();
    await t.pump(const Duration(milliseconds: 300));
    final open = rowHeight();
    expect(mid, greaterThan(0));
    expect(open, greaterThan(mid));
    // finished: held 0.8 s, then shut
    set(false);
    await t.pump(const Duration(milliseconds: 600));
    expect(rowHeight(), open);
    await t.pump(const Duration(milliseconds: 300));
    await t.pump(const Duration(milliseconds: 300));
    expect(rowHeight(), 0);
    // scrolled down: opening above doesn't move the rows in view
    scroll.jumpTo(400);
    await t.pump();
    final before = t.getTopLeft(find.byKey(const ValueKey(20))).dy;
    set(true);
    await t.pump(const Duration(milliseconds: 310));
    for (var i = 0; i < 6; i++) {
      await t.pump(const Duration(milliseconds: 60));
      expect(t.getTopLeft(find.byKey(const ValueKey(20))).dy, closeTo(before, 0.5));
    }
    set(false);
    await t.pump(const Duration(seconds: 2));
    expect(t.getTopLeft(find.byKey(const ValueKey(20))).dy, closeTo(before, 0.5));
  }, skip: engine == null);
}
