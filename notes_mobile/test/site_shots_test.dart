// The website's screenshots of Notes for Android, drawn offscreen (no window): the app on a
// library (the site demo, nt-core examples/site_demo.rs), at phone and tablet sizes, written as
// PNGs. Skipped unless SITE_SHOTS names the output folder and SITE_LIBRARY a library copy (it's
// opened, so it changes):
//   SITE_SHOTS=/tmp/shots SITE_LIBRARY=/tmp/sitelib \
//   LD_LIBRARY_PATH=../notes-linux/target/release flutter test test/site_shots_test.dart
import 'dart:io';
import 'dart:ui' as ui;

import 'package:flutter/material.dart';
import 'package:flutter/rendering.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:notes/app.dart';
import 'package:notes/engine.dart';
import 'package:notes/main.dart';
import 'package:notes/model.dart';
import 'package:notes/state.dart';
import 'package:notes/sync.dart';

final _out = Platform.environment['SITE_SHOTS'];
final _library = Platform.environment['SITE_LIBRARY'];

Future<void> _fonts() async {
  final have = Directory('assets/fonts').listSync().map((e) => e.uri.pathSegments.last).where((f) => f.endsWith('.ttf') || f.endsWith('.otf'));
  for (final fam in ['Sfera', 'SferaDisplay', 'SferaMono']) {
    final l = FontLoader(fam);
    for (final f in have.where((f) => f.startsWith('$fam-'))) {
      l.addFont(rootBundle.load('assets/fonts/$f'));
    }
    await l.load();
  }
  // the symbols: Cupertino's (from its package) and Material's (from the SDK)
  final cupertino = FontLoader('packages/cupertino_icons/CupertinoIcons')..addFont(rootBundle.load('packages/cupertino_icons/assets/CupertinoIcons.ttf'));
  await cupertino.load();
  // (flutter_tester is bin/cache/artifacts/engine/<platform>/flutter_tester)
  final cache = File(Platform.resolvedExecutable).parent.parent.parent.parent.path;
  final material = File('$cache/artifacts/material_fonts/MaterialIcons-Regular.otf');
  expect(material.existsSync(), isTrue, reason: material.path);
  if (material.existsSync()) {
    final m = FontLoader('MaterialIcons')..addFont(Future.value(ByteData.sublistView(material.readAsBytesSync())));
    await m.load();
  }
}

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();
  final skip = _out == null || _library == null ? 'set SITE_SHOTS and SITE_LIBRARY' : null;
  final shot = GlobalKey();
  late Engine engine;
  final tmp = Directory.systemTemp.createTempSync('site-shots-');

  setUpAll(() async {
    if (skip != null) return;
    engine = Engine();
    engine.call('open', {'dir': _library});
    await _fonts();
    // (the PDF pages' cache folder)
    TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger.setMockMethodCallHandler(
      const MethodChannel('plugins.flutter.io/path_provider'),
      (call) async => tmp.path,
    );
  });

  /// The app on `s`, at `w`×`h` pixels and `dpr`.
  Future<AppState> app(WidgetTester t, {required double w, required double h, required double dpr, String appearance = 'light'}) async {
    t.view.physicalSize = Size(w, h);
    t.view.devicePixelRatio = dpr;
    t.platformDispatcher.platformBrightnessTestValue = appearance == 'dark' ? Brightness.dark : Brightness.light;
    final s = AppState(engine, tmp.path);
    s.settings.appearance = appearance;
    await t.runAsync(s.reload);
    s.sync = SyncRunner(s);
    s.place = s.defaultPlace;
    await t.pumpWidget(RepaintBoundary(key: shot, child: NotesApp(state: s)));
    await settle(t);
    return s;
  }

  Future<void> save(WidgetTester t, String name) async {
    await settle(t);
    final ro = t.renderObject<RenderRepaintBoundary>(find.byKey(shot));
    final png = await t.runAsync(() async {
      final img = await ro.toImage(pixelRatio: t.view.devicePixelRatio);
      return (await img.toByteData(format: ui.ImageByteFormat.png))!.buffer.asUint8List();
    });
    File('$_out/$name.png').writeAsBytesSync(png!);
  }

  tearDown(() {
    // (each shot sets its own size)
  });

  testWidgets('phone-list: the Spring Trip folder', (t) async {
    final s = await app(t, w: 1080, h: 2340, dpr: 2.625);
    await t.tap(find.text('Spring Trip'));
    await settle(t);
    expect(s.place, isA<InFolder>());
    await save(t, 'phone-list');
  }, skip: skip != null);

  testWidgets('phone-note: Venice & Milan, its PDF carousel and checklist', (t) async {
    await app(t, w: 1080, h: 2340, dpr: 2.625);
    await t.tap(find.text('Spring Trip'));
    await settle(t);
    await t.tap(find.text('Venice & Milan').first);
    await settle(t, rounds: 16);
    await save(t, 'phone-note');
  }, skip: skip != null);

  testWidgets('phone-dark: a note in dark mode, the glass bars', (t) async {
    await app(t, w: 1080, h: 2340, dpr: 2.625, appearance: 'dark');
    await t.tap(find.text('Spring Trip'));
    await settle(t);
    await t.tap(find.text('Packing list').first);
    await settle(t, rounds: 10);
    await save(t, 'phone-dark');
  }, skip: skip != null);

  testWidgets('tablet: sidebar, list and Tulip season with its PDF', (t) async {
    final s = await app(t, w: 2560, h: 1600, dpr: 2);
    s.choose(InFolder(s.lib.folders.firstWhere((f) => f.name == 'Spring Trip').id));
    s.open(s.lib.notes.firstWhere((n) => n.title == 'Tulip season').id);
    await settle(t, rounds: 16);
    await save(t, 'tablet');
  }, skip: skip != null);

  testWidgets('phone-sync: Settings > WebDAV Sync with a made-up account', (t) async {
    final s = await app(t, w: 1080, h: 2340, dpr: 2.625);
    // (made up: no such account; nothing is sent anywhere, the sync isn't started)
    s.call('sync_set', {'on': false, 'url': 'https://cloud.example.com/remote.php/dav/files/alex', 'user': 'alex', 'password': 'example-only', 'folder': 'Bramble Notes'});
    s.call('sync_set', {'on': true, 'url': 'https://cloud.example.com/remote.php/dav/files/alex', 'user': 'alex', 'password': 'example-only', 'folder': 'Bramble Notes'});
    showSyncSettings(t.element(find.byType(NotesHome)), s);
    await settle(t);
    await save(t, 'phone-sync');
    s.call('sync_set', {'on': false, 'url': '', 'user': '', 'password': '', 'folder': 'Bramble Notes'});
  }, skip: skip != null);
}

/// Lets real work finish (the engine's worker, PDF pages from pdftoppm, pictures decoding)
/// and the app's animations end.
Future<void> settle(WidgetTester t, {int rounds = 8}) async {
  for (var i = 0; i < rounds; i++) {
    await t.runAsync(() => Future<void>.delayed(const Duration(milliseconds: 120)));
    await t.pump(const Duration(milliseconds: 100));
  }
  for (final e in find.byType(Image).evaluate()) {
    final w = e.widget as Image;
    await t.runAsync(() => precacheImage(w.image, e));
  }
  await t.pump(const Duration(milliseconds: 400));
}
