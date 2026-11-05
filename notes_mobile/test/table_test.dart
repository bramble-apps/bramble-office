// Moving a table's rows and columns (table_inline.dart): where a dragged one lands, how the
// others slide aside, and what the save is told (the rows, and where each row and column came
// from, so the engine keeps moved cells' formatting).
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:notes/table_inline.dart';
import 'package:notes/theme.dart';

void main() {
  test('a dragged row lands past the middle of its neighbours', () {
    const h = [40.0, 60.0, 50.0, 40.0];
    expect(dropIndex(h, 0, 20), 0);
    expect(dropIndex(h, 0, 31), 1);
    expect(dropIndex(h, 0, 60 + 26), 2);
    expect(dropIndex(h, 0, 999), 3);
    expect(dropIndex(h, 3, -26), 2);
    expect(dropIndex(h, 3, -999), 0);
    // the others slide by the moving row's height; it settles on the slot it took
    expect(shiftFor(h, 0, 2, 1), -40);
    expect(shiftFor(h, 0, 2, 2), -40);
    expect(shiftFor(h, 0, 2, 3), 0);
    expect(shiftFor(h, 3, 1, 1), 40);
    expect(slotOffset(h, 0, 2), 110);
    expect(slotOffset(h, 3, 1), -110);
    expect(moved(['a', 'b', 'c'], 0, 2), ['b', 'c', 'a']);
  });

  Future<(List<List<String>>, List<int?>, List<int?>)?> drag(WidgetTester t, {required bool row}) async {
    (List<List<String>>, List<int?>, List<int?>)? out;
    await t.pumpWidget(
      MaterialApp(
        theme: notesTheme(Brightness.light),
        home: Scaffold(
          body: Center(
            child: InlineTable(
              rows: const [
                ['a', 'b', 'c'],
                ['d', 'e', 'f'],
                ['g', 'h', 'i'],
              ],
              width: 360,
              editable: true,
              onChanged: (r, rf, cf) => out = (r, rf, cf),
              onActive: (_) {},
            ),
          ),
        ),
      ),
    );
    // the caret in "a": its row and column get their handles
    await t.tap(find.text('a'));
    await t.pumpAndSettle();
    final handle = find.byIcon(row ? Icons.more_vert_rounded : Icons.more_horiz_rounded);
    expect(handle, findsOneWidget);
    // drag it two rows down (or two columns along), past the middles
    final g = await t.startGesture(t.getCenter(handle));
    for (var k = 0; k < 30; k++) {
      await g.moveBy(row ? const Offset(0, 6) : const Offset(12, 0));
      await t.pump(const Duration(milliseconds: 8));
    }
    await g.up();
    await t.pumpAndSettle();
    return out;
  }

  testWidgets('dragging the row handle moves the row', (t) async {
    final out = await drag(t, row: true);
    expect(out, isNotNull);
    expect(out!.$1, [
      ['d', 'e', 'f'],
      ['g', 'h', 'i'],
      ['a', 'b', 'c'],
    ]);
    expect(out.$2, [1, 2, 0]);
    expect(out.$3, [0, 1, 2]);
  });

  testWidgets('dragging the column handle moves the column', (t) async {
    final out = await drag(t, row: false);
    expect(out, isNotNull);
    expect(out!.$1, [
      ['b', 'c', 'a'],
      ['e', 'f', 'd'],
      ['h', 'i', 'g'],
    ]);
    expect(out.$3, [1, 2, 0]);
  });
}
