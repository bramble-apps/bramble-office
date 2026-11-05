// The editor's text model (doc.dart): bodies in and out, typing, lists, tags, tokens, styles.
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:notes/doc.dart';

NoteController make(List body) => NoteController()..load(body);

void type(NoteController c, String s) {
  final sel = c.selection;
  c.value = TextEditingValue(text: c.text.replaceRange(sel.start, sel.end, s), selection: TextSelection.collapsed(offset: sel.start + s.length));
}

void backspace(NoteController c) {
  final o = c.selection.start;
  c.value = TextEditingValue(text: c.text.replaceRange(o - 1, o, ''), selection: TextSelection.collapsed(offset: o - 1));
}

void caretAt(NoteController c, int o) => c.selection = TextSelection.collapsed(offset: o);

List<String> texts(NoteController c) => [for (final p in c.body()) (p['sp'] as List).map((s) => s['t']).join()];
List<String> styles(NoteController c) => [for (final p in c.body()) p['st'] as String];

void main() {
  final sample = [
    {'st': 'title', 'sp': [{'t': 'Groceries'}]},
    {'st': 'check', 'ck': true, 'sp': [{'t': 'Milk'}]},
    {'st': 'check', 'sp': [{'t': 'Eggs'}]},
    {'st': 'body', 'sp': [{'t': 'Bold', 'b': true}, {'t': ' and plain'}]},
  ];

  test('a body comes back as it went in', () {
    final c = make(sample);
    expect(texts(c), ['Groceries', 'Milk', 'Eggs', 'Bold and plain']);
    expect(styles(c), ['title', 'check', 'check', 'body']);
    expect(c.body()[1]['ck'], true);
    expect(c.body()[3]['sp'], [{'t': 'Bold', 'b': true}, {'t': ' and plain'}]);
  });

  test('Return in a checklist item makes an unchecked item; twice ends the list', () {
    final c = make(sample);
    caretAt(c, c.text.indexOf('Milk') + 4);
    type(c, '\n');
    expect(styles(c), ['title', 'check', 'check', 'check', 'body']);
    expect(c.body()[2]['ck'], isNull);
    type(c, '\n');
    expect(styles(c), ['title', 'check', 'body', 'check', 'body']);
    type(c, 'x');
    expect(texts(c)[2], 'x');
  });

  test('Return after the title starts body text', () {
    final c = make([{'st': 'title', 'sp': [{'t': 'Hi'}]}]);
    caretAt(c, 2);
    type(c, '\n');
    type(c, 'there');
    expect(styles(c), ['title', 'body']);
    expect(texts(c), ['Hi', 'there']);
  });

  test('Backspace at an item\'s start ends the item, the next joins the lines', () {
    final c = make(sample);
    caretAt(c, c.text.indexOf('Eggs'));
    backspace(c);
    expect(styles(c)[2], 'body');
    expect(texts(c)[2], 'Eggs');
    backspace(c);
    expect(texts(c), ['Groceries', 'MilkEggs', 'Bold and plain']);
  });

  test('the caret never rests before a lead', () {
    final c = make(sample);
    final lead = c.text.indexOf('Eggs') - 1;
    caretAt(c, lead);
    expect(c.selection.baseOffset, lead + 1);
  });

  test('typing keeps the style before the caret; Bold with no selection is for what comes next', () {
    final c = make(sample);
    caretAt(c, c.text.indexOf('Bold') + 4);
    type(c, 'er');
    expect(c.body()[3]['sp'][0], {'t': 'Bolder', 'b': true});
    caretAt(c, c.text.length);
    c.toggle('i');
    type(c, '!');
    expect((c.body()[3]['sp'] as List).last, {'t': '!', 'i': true});
  });

  test('styles on a selection', () {
    final c = make(sample);
    final a = c.text.indexOf('plain');
    c.selection = TextSelection(baseOffset: a, extentOffset: a + 5);
    c.toggle('u');
    c.highlight(3);
    expect((c.body()[3]['sp'] as List).last, {'t': 'plain', 'u': true, 'hl': 3});
  });

  test('#word and a space becomes a tag', () {
    final c = make([{'st': 'body', 'sp': [{'t': 'see '}]}]);
    caretAt(c, 4);
    for (final ch in '#trip'.split('')) {
      type(c, ch);
    }
    type(c, ' ');
    final sp = c.body()[0]['sp'] as List;
    expect(sp[1]['tk']['alt'], '#trip');
    expect(sp[1]['tk']['uti'], utiHashtag);
    expect(sp.last['t'], ' ');
  });

  test('paragraph styles and lists over the selected lines', () {
    final c = make(sample);
    c.selection = TextSelection(baseOffset: c.text.indexOf('Milk'), extentOffset: c.text.indexOf('Bold'));
    c.paragraphStyle('bullet');
    expect(styles(c), ['title', 'bullet', 'bullet', 'bullet']);
    c.paragraphStyle('bullet');
    expect(styles(c), ['title', 'body', 'body', 'body']);
    c.indent(1);
    expect(c.body()[1]['ind'], 1);
  });

  test('a block attachment gets its own line, and goes with it', () {
    final c = make(sample);
    caretAt(c, c.text.indexOf('Groceries') + 9);
    c.insertToken({'id': 'T1', 'uti': utiTable, 'alt': '', 'content': ''});
    expect(texts(c)[1], obj);
    expect(c.body()[1]['sp'][0]['tk']['id'], 'T1');
    expect(texts(c).length, 6);
    c.removeToken('T1');
    expect(texts(c), ['Groceries', '', 'Milk', 'Eggs', 'Bold and plain']);
  });

  test('ticking an item moves it to the checklist\'s end when sorting automatically', () {
    final c = make([
      {'st': 'check', 'sp': [{'t': 'A'}]},
      {'st': 'check', 'sp': [{'t': 'B'}]},
      {'st': 'check', 'sp': [{'t': 'C'}]},
      {'st': 'body', 'sp': [{'t': 'end'}]},
    ])
      ..autoSortChecked = true;
    c.check(0);
    expect(texts(c), ['B', 'C', 'A', 'end']);
    expect(c.body()[2]['ck'], true);
  });

  test('undo puts the text and styles back', () {
    final c = make(sample);
    caretAt(c, c.text.indexOf('Eggs'));
    backspace(c);
    c.undo();
    expect(styles(c), ['title', 'check', 'check', 'body']);
    expect(texts(c), ['Groceries', 'Milk', 'Eggs', 'Bold and plain']);
  });

  test('>> and # offer suggestions, and a pick becomes a token', () {
    final c = make([{'st': 'body', 'sp': [{'t': 'see >>Gro'}]}]);
    caretAt(c, c.text.length);
    expect(c.suggesting, ('link', 'Gro', 4));
    c.replaceWithToken(4, c.text.length, {'id': 'L1', 'uti': utiLink, 'alt': 'Groceries', 'content': 'applenotes://showNote?identifier=N1'});
    final sp = c.body()[0]['sp'] as List;
    expect(sp[1]['tk']['alt'], 'Groceries');
    expect(sp.last['t'], ' ');
    type(c, '#tr');
    expect(c.suggesting, ('tag', 'tr', c.text.length - 3));
  });

  test('an expression and = gets its answer, with variables from above', () {
    final c = make([{'st': 'body', 'sp': [{'t': 'rent = 1200'}]}, {'st': 'body', 'sp': []}]);
    caretAt(c, c.text.length);
    for (final ch in 'rent*12='.split('')) {
      type(c, ch);
    }
    final sp = c.body()[1]['sp'] as List;
    expect(sp.last['tk']['alt'], '=14400');
    type(c, '\n');
    for (final ch in '7='.split('')) {
      type(c, ch);
    }
    expect((c.body()[2]['sp'] as List).last['t'], '7=');
  });
}
