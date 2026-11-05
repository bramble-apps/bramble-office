// Notes for Android: Bramble Office's notes, made after Apple Notes on iPhone and iPad (iOS 26),
// on the desktop Notes' engine (engine.dart: nt-core's library in Apple's NoteStore format,
// so a Mac's Notes library imports as it is). Everything on screen is Flutter's.
import 'dart:async';
import 'dart:io';

import 'package:flutter/cupertino.dart';
import 'package:flutter/gestures.dart';
import 'package:flutter/material.dart';
import 'package:path_provider/path_provider.dart';

import 'app.dart';
import 'engine.dart';
import 'model.dart';
import 'state.dart';
import 'sync.dart';
import 'test_snap.dart';
import 'theme.dart';

final appShot = GlobalKey();

Future<void> main() async {
  WidgetsFlutterBinding.ensureInitialized();
  // touches resampled to the frame's time, so drags follow the finger evenly at 90–120 Hz
  GestureBinding.instance.resamplingEnabled = true;
  // (errors to stderr too: the automated checks read them)
  FlutterError.onError = (d) {
    FlutterError.presentError(d);
    stderr.writeln('FLUTTER ERROR: ${d.exceptionAsString()}\n${d.stack}');
    logProblem('Flutter', d.exception, d.stack);
  };
  WidgetsBinding.instance.platformDispatcher.onError = (e, st) {
    logProblem('uncaught', e, st);
    return true;
  };
  try {
    final ext = Platform.isAndroid ? await getExternalStorageDirectory() : null;
    if (ext != null) problemLog = '${ext.path}/notes-log.txt';
  } catch (_) {}
  // the library: the app's own folder (NOTES_LIBRARY=<dir> for the automated checks)
  final home = (await getApplicationSupportDirectory()).path;
  final dir = Platform.environment['NOTES_LIBRARY'] ?? '$home/library';
  final engine = Engine();
  final state = AppState(engine, home);
  state.settings.load(home);
  await _open(engine, state, dir);
  // sample notes for test builds (NOTES_DEMO=1, or a `demo` file in the app's external folder)
  var demo = Platform.environment['NOTES_DEMO'] == '1';
  try {
    final ext = await getExternalStorageDirectory();
    if (ext != null && File('${ext.path}/demo').existsSync()) demo = true;
  } catch (_) {}
  if (demo) {
    try {
      engine.call('demo');
    } catch (_) {}
  }
  // (the automated checks: NOTES_IMPORT=<a Mac's NoteStore.sqlite or its folder>)
  final imp = Platform.environment['NOTES_IMPORT'];
  if (imp != null) {
    try {
      stderr.writeln('import: ${engine.call('import', {'path': imp})}');
    } catch (e) {
      stderr.writeln('import failed: $e');
    }
  }
  await state.reload();
  state.sync = SyncRunner(state)..start();
  state.place = state.defaultPlace;
  // (the automated checks: NOTES_OPEN=<a note's title> opens it)
  final open = Platform.environment['NOTES_OPEN'];
  if (open != null) {
    state.place = const AllNotes();
    state.noteId = state.lib.notes.where((n) => n.title == open && !n.deleted).firstOrNull?.id;
  }
  FocusManager.instance.addListener(_keepKeysFocus);
  WidgetsBinding.instance.addPostFrameCallback((_) => _keepKeysFocus());
  runApp(
    RepaintBoundary(
      key: appShot,
      child: NotesApp(state: state),
    ),
  );
  startTestSnapshots(
    appShot,
    type: (t) => _edit((v) => v.replaced(v.selection, t)),
    key: (k) {
      if (k == 'enter') {
        FocusManager.instance.primaryFocus?.context?.findAncestorStateOfType<EditableTextState>()?.performAction(TextInputAction.done);
      }
      if (k == 'backspace') {
        _edit(
          (v) => v.selection.isCollapsed && v.selection.start > 0
              ? v.replaced(TextRange(start: v.selection.start - 1, end: v.selection.start), '')
              : v.replaced(v.selection, ''),
        );
      }
    },
  );
}

/// Opens the library on a worker. The first open of an old-layout library converts it to the
/// library as files (nt-core docs/FORMAT.md, ~12 s for 1,200 notes): meanwhile a screen says how
/// far it is ("Updating your library… 120 of 1186"), and how it went waits for the app (an alert
/// if it stayed on the old library).
Future<void> _open(Engine engine, AppState state, String dir) async {
  String? tmp;
  try {
    tmp = (await getTemporaryDirectory()).path;
  } catch (_) {}
  final opening = engine.run('open', {'dir': dir, 'tmp': ?tmp});
  final message = ValueNotifier<String?>(null);
  final poll = Timer.periodic(const Duration(milliseconds: 250), (_) {
    try {
      final m = (engine.call('opening') as Map)['message'] as String?;
      if (m != null && message.value == null) runApp(_Opening(message));
      if (m != null) message.value = m;
    } catch (_) {}
  });
  try {
    final r = await opening as Map;
    final m = r['migration'] as Map?;
    if (m != null) state.libraryNotice = m.cast<String, dynamic>();
  } catch (e) {
    stderr.writeln('Notes: $e');
  } finally {
    poll.cancel();
  }
}

/// The screen while the library converts.
class _Opening extends StatelessWidget {
  const _Opening(this.message);
  final ValueNotifier<String?> message;

  @override
  Widget build(BuildContext context) {
    final dark = WidgetsBinding.instance.platformDispatcher.platformBrightness == Brightness.dark;
    final fg = dark ? const Color(0xFFFFFFFF) : const Color(0xFF000000);
    return Directionality(
      textDirection: TextDirection.ltr,
      child: ColoredBox(
        color: dark ? const Color(0xFF000000) : const Color(0xFFF2F2F7),
        child: Center(
          child: Column(
            mainAxisSize: MainAxisSize.min,
            children: [
              CupertinoActivityIndicator(radius: 14, color: fg.withValues(alpha: 0.6)),
              const SizedBox(height: 18),
              ValueListenableBuilder(
                valueListenable: message,
                builder: (ctx, m, _) => Padding(
                  padding: const EdgeInsets.symmetric(horizontal: 32),
                  child: Text(m ?? '', textAlign: TextAlign.center, style: Type.text(17, fg)),
                ),
              ),
            ],
          ),
        ),
      ),
    );
  }
}

/// The app's key Focus (keys.dart): it takes the focus whenever nothing else has it (at the
/// start, after a field lets go), so a keyboard's first key reaches the list rather than
/// moving the focus somewhere.
final _keysFocus = FocusNode(debugLabel: 'keys');

void _keepKeysFocus() {
  final fm = FocusManager.instance;
  final p = fm.primaryFocus;
  if ((p == null || p == fm.rootScope) && _keysFocus.context != null) _keysFocus.requestFocus();
}

/// The automated checks' typing: as the keyboard's, into the focused field.
void _edit(TextEditingValue Function(TextEditingValue v) f) {
  final st = FocusManager.instance.primaryFocus?.context?.findAncestorStateOfType<EditableTextState>();
  if (st == null) return;
  st.userUpdateTextEditingValue(f(st.textEditingValue), SelectionChangedCause.keyboard);
}

class NotesApp extends StatelessWidget {
  const NotesApp({super.key, required this.state});
  final AppState state;

  @override
  Widget build(BuildContext context) => ListenableBuilder(
    listenable: state,
    builder: (ctx, _) {
      Palette.colorTheme = state.settings.colorTheme;
      Palette.glassOn = state.settings.glass;
      return MaterialApp(
      title: 'Notes',
      debugShowCheckedModeBanner: false,
      // iOS's scrolling everywhere: momentum and the rubber band, no Android stretch
      scrollBehavior: const _IosScroll(),
      theme: notesTheme(Brightness.light),
      darkTheme: notesTheme(Brightness.dark),
      themeMode: switch (state.settings.appearance) {
        'light' => ThemeMode.light,
        'dark' => ThemeMode.dark,
        _ => ThemeMode.system,
      },
      // (a hardware keyboard's keys, keys.dart: here, inside the app's own shortcuts, so the
      // list's arrows come before Flutter's moving of focus between fields)
      builder: (ctx, child) => _testPhone(
        _testKeyboard(
          Focus(
            focusNode: _keysFocus,
            autofocus: true,
            skipTraversal: true,
            onKeyEvent: (node, e) => (state.keyHandler?.call(e) ?? false) ? KeyEventResult.handled : KeyEventResult.ignored,
            child: child ?? const SizedBox(),
          ),
        ),
      ),
      // (the main screen's glass buttons share one backdrop blur; sheets and full-screen pages
      // above it, which cover it, keep their own)
      home: BackdropGroup(child: NotesHome(state: state)),
    );
    },
  );
}

/// NOTES_PHONE=`w`x`h` (the automated checks): the app in a box that size at the window's top
/// left, as a phone's screen.
Widget _testPhone(Widget child) {
  final m = RegExp(r'^(\d+)x(\d+)$').firstMatch(Platform.environment['NOTES_PHONE'] ?? '');
  if (m == null) return child;
  final size = Size(double.parse(m[1]!), double.parse(m[2]!));
  return Builder(
    builder: (ctx) {
      return ColoredBox(
        color: const Color(0xFF000000),
        child: Align(
          alignment: Alignment.topLeft,
          child: SizedBox.fromSize(
            size: size,
            child: ClipRect(
              child: MediaQuery(
                data: MediaQuery.of(ctx).copyWith(size: size, padding: const EdgeInsets.only(top: 24, bottom: 16)),
                child: child,
              ),
            ),
          ),
        ),
      );
    },
  );
}

/// NOTES_KEYBOARD=`points` (the automated checks): an on-screen keyboard that tall while a text
/// field has the focus, as MediaQuery's viewInsets, drawn as a grey block.
Widget _testKeyboard(Widget child) {
  final k0 = double.tryParse(Platform.environment['NOTES_KEYBOARD'] ?? '');
  if (k0 == null) return child;
  return ListenableBuilder(
    listenable: FocusManager.instance,
    builder: (ctx, _) {
      final typing = FocusManager.instance.primaryFocus?.context?.findAncestorStateOfType<EditableTextState>() != null;
      final k = typing ? k0 : 0.0;
      final mq = MediaQuery.of(ctx);
      return Stack(
        children: [
          MediaQuery(
            data: mq.copyWith(viewInsets: mq.viewInsets.copyWith(bottom: k)),
            child: child,
          ),
          Positioned(
            left: 0,
            right: 0,
            bottom: 0,
            height: k,
            child: const ColoredBox(color: Color(0xFF8E8E93)),
          ),
        ],
      );
    },
  );
}

class _IosScroll extends MaterialScrollBehavior {
  const _IosScroll();
  @override
  ScrollPhysics getScrollPhysics(BuildContext context) => const BouncingScrollPhysics(parent: RangeMaintainingScrollPhysics());
  @override
  Widget buildOverscrollIndicator(BuildContext context, Widget child, ScrollableDetails details) => child;
}
