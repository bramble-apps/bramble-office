import 'dart:convert';

import 'package:flutter_test/flutter_test.dart';
import 'package:notes/export_sheet.dart';
import 'package:notes/pdf_export.dart';

Map _note(int paragraphs) => {
  'title': 'Trip',
  'attachments': [],
  'body': [
    {
      'st': 'title',
      'sp': [
        {'t': 'Trip'},
      ],
    },
    for (var i = 0; i < paragraphs; i++)
      {
        'st': i.isEven ? 'bullet' : 'check',
        'ck': i % 4 == 1,
        'sp': [
          {'t': 'Item $i with some words', 'b': i % 3 == 0},
        ],
      },
  ],
};

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();

  test('a PDF of Letter pages, more pages for a long note, turned for landscape', () async {
    final short = latin1.decode(await notePdf(_note(3)));
    expect(short.startsWith('%PDF-1.4'), isTrue);
    expect(short, contains('/Count 1 '));
    expect(short, contains('/MediaBox [0 0 612 792]'));
    expect(short.trimRight().endsWith('%%EOF'), isTrue);
    final long = latin1.decode(await notePdf(_note(120)));
    expect(RegExp(r'/Count (\d+) ').firstMatch(long)![1], isNot('1'));
    final turned = latin1.decode(await notePdf(_note(3), landscape: true));
    expect(turned, contains('/MediaBox [0 0 792 612]'));
  });

  test('file names from titles, as nt-core makes them', () {
    expect(fileStem('Plans: 2026/27'), 'Plans- 2026-27');
    expect(fileStem('  .hidden'), 'hidden');
    expect(fileStem(''), 'Untitled');
  });
}
