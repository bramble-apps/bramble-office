// PDF pages (pdf.dart pdfPage): a page that couldn't be drawn, say its file still arriving with a
// sync, is asked for again the next time instead of staying blank. Uses pdftoppm (as the Linux
// build does); skipped without it.
import 'dart:io';

import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:notes/pdf.dart';
import 'package:notes/pdf_export.dart';

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();
  final hasPdftoppm = Process.runSync('sh', ['-c', 'command -v pdftoppm']).exitCode == 0;

  test('a page that failed is drawn once its file is there', () async {
    final tmp = Directory.systemTemp.createTempSync('pdf-pages-');
    TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger.setMockMethodCallHandler(
      const MethodChannel('plugins.flutter.io/path_provider'),
      (call) async => tmp.path,
    );
    final path = '${tmp.path}/arriving.pdf';
    expect(await pdfPage(path, 0, 300), isNull);
    final note = {
      'title': 'Brochure',
      'attachments': [],
      'body': [
        {
          'st': 'title',
          'sp': [
            {'t': 'Brochure'},
          ],
        },
      ],
    };
    File(path).writeAsBytesSync(await notePdf(note));
    final page = await pdfPage(path, 0, 300);
    expect(page, isNotNull);
    expect(File(page!).lengthSync(), greaterThan(0));
  }, skip: hasPdftoppm ? null : 'no pdftoppm');
}
