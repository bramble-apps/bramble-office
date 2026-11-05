// Swipe rows (ui/swipe.dart): after a swipe closes, a tap reaches the row again; a row's swipe
// state never passes to its neighbour when a note goes ("tapping some notes doesn't open them").
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:notes/ui/swipe.dart';

void main() {
  Widget list(List<String> ids, List<String> opened, void Function(String id) delete) => MaterialApp(
    home: Scaffold(
      body: ListView(
        children: [
          for (final id in ids)
            SwipeRow(
              key: ValueKey('swipe-$id'),
              trailing: [SwipeAction(Icons.delete, Colors.red, () => delete(id), label: 'Delete')],
              child: GestureDetector(
                behavior: HitTestBehavior.opaque,
                onTap: () => opened.add(id),
                child: SizedBox(height: 84, child: Text('note $id')),
              ),
            ),
        ],
      ),
    ),
  );

  testWidgets('a row swiped open and shut again takes taps', (t) async {
    final opened = <String>[];
    await t.pumpWidget(list(['a', 'b', 'c'], opened, (_) {}));
    await t.drag(find.text('note b'), const Offset(-120, 0));
    await t.pumpAndSettle();
    // a tap on the open row shuts it (iOS), the next one opens the note
    await t.tapAt(t.getTopRight(find.byKey(const ValueKey('swipe-b'))) + const Offset(-20, 20));
    await t.pumpAndSettle();
    await t.tap(find.text('note b'));
    await t.pumpAndSettle();
    expect(opened, ['b']);
    // shut by a touch elsewhere
    await t.drag(find.text('note c'), const Offset(-120, 0));
    await t.pumpAndSettle();
    await t.tap(find.text('note a'));
    await t.pumpAndSettle();
    await t.tap(find.text('note c'));
    await t.pumpAndSettle();
    expect(opened, ['b', 'a', 'c']);
  });

  testWidgets('delete one, then tap its neighbour: the neighbour opens', (t) async {
    final opened = <String>[];
    var ids = ['a', 'b', 'c', 'd'];
    late StateSetter set;
    await t.pumpWidget(
      StatefulBuilder(
        builder: (ctx, s) {
          set = s;
          return list(ids, opened, (id) => set(() => ids = ids.where((x) => x != id).toList()));
        },
      ),
    );
    // swipe b open and tap its Delete button
    await t.drag(find.text('note b'), const Offset(-120, 0));
    await t.pumpAndSettle();
    await t.tap(find.byIcon(Icons.delete));
    await t.pumpAndSettle();
    expect(find.text('note b'), findsNothing);
    await t.tap(find.text('note c'));
    await t.tap(find.text('note a'));
    await t.pumpAndSettle();
    expect(opened, ['c', 'a']);
    // a full swipe deleting c, then d (now where c was) opens
    await t.drag(find.text('note c'), const Offset(-700, 0));
    await t.pumpAndSettle();
    expect(find.text('note c'), findsNothing);
    await t.tap(find.text('note d'));
    await t.pumpAndSettle();
    expect(opened, ['c', 'a', 'd']);
  });
}
