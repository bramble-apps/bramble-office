// Letters for Android: Bramble Office's word processor, its window as the desktop's (the menu
// bar, the floating toolbar, the pages, the inspector at the right) made in Flutter. The
// documents, editing logic and page drawing are the desktop Letters' own (the Rust engine,
// engine.dart); nothing on screen is drawn by it but the pages' content.
import 'dart:async';
import 'dart:io';

import 'package:file_picker/file_picker.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:path_provider/path_provider.dart';

import 'desktop/chooser.dart';
import 'desktop/doc_view.dart';
import 'desktop/annotations.dart';
import 'desktop/findbar.dart';
import 'desktop/inspector.dart';
import 'desktop/look.dart';
import 'desktop/menubar.dart';
import 'desktop/popovers.dart';
import 'desktop/scale.dart';
import 'desktop/sidebar.dart';
import 'desktop/slide.dart';
import 'desktop/tabbar.dart';
import 'desktop/widgets.dart';
import 'desktop/toolbar.dart';
import 'desktop/touch_menu.dart';
import 'desktop/colors.dart';
import 'desktop/windows.dart';
import 'engine.dart';
import 'test_snap.dart';


/// (automated checks: typing and keys as the on-screen keyboard's, test_snap.dart)
void Function(String)? _typeHook, _keyHook;

Future<void> main() async {
  WidgetsFlutterBinding.ensureInitialized();
  // (errors to stderr too: the automated checks read them)
  FlutterError.onError = (d) {
    FlutterError.presentError(d);
    stderr.writeln('FLUTTER ERROR: ${d.exceptionAsString()}\n${d.stack}');
  };
  final home = (await getApplicationSupportDirectory()).path;
  final engine = Engine();
  final view = WidgetsBinding.instance.platformDispatcher.views.first;
  final size = view.physicalSize / view.devicePixelRatio;
  engine.start(home, size.width, size.height);
  runApp(RepaintBoundary(key: appShot, child: LettersApp(engine: engine)));
  startTestSnapshots(appShot, type: (t) => _typeHook?.call(t), key: (k) => _keyHook?.call(k));
}

class LettersApp extends StatelessWidget {
  const LettersApp({super.key, required this.engine});
  final Engine engine;

  @override
  Widget build(BuildContext context) => MaterialApp(
        title: 'Letters',
        debugShowCheckedModeBanner: false,
        theme: ThemeData(useMaterial3: true, fontFamily: 'Carlito', splashFactory: NoSplash.splashFactory),
        // (Settings > Scaling: everything, menus and windows too)
        builder: (ctx, child) => _testPhone(UiScaled(child: _testKeyboard(child ?? const SizedBox()))),
        home: DocScreen(engine: engine),
      );
}

/// LETTERS_PHONE=`w`x`h` (the automated checks): the app in a box that size at the window's top
/// left, as a phone's screen.
Widget _testPhone(Widget child) {
  final m = RegExp(r'^(\d+)x(\d+)$').firstMatch(Platform.environment['LETTERS_PHONE'] ?? '');
  if (m == null) return child;
  final size = Size(double.parse(m[1]!), double.parse(m[2]!));
  return Builder(builder: (ctx) {
    return ColoredBox(
      color: const Color(0xFF000000),
      child: Align(
        alignment: Alignment.topLeft,
        child: SizedBox.fromSize(size: size, child: ClipRect(child: MediaQuery(data: MediaQuery.of(ctx).copyWith(size: size), child: child))),
      ),
    );
  });
}

/// LETTERS_KEYBOARD=`points` (the automated checks): an on-screen keyboard that tall, as
/// MediaQuery's viewInsets, drawn as a grey block.
Widget _testKeyboard(Widget child) {
  final k = double.tryParse(Platform.environment['LETTERS_KEYBOARD'] ?? '');
  if (k == null) return child;
  return Builder(builder: (ctx) {
    final mq = MediaQuery.of(ctx);
    return Stack(children: [
      MediaQuery(data: mq.copyWith(viewInsets: mq.viewInsets.copyWith(bottom: k)), child: child),
      Positioned(left: 0, right: 0, bottom: 0, height: k, child: const ColoredBox(color: Color(0xFF8E8E93))),
    ]);
  });
}

class DocScreen extends StatefulWidget {
  const DocScreen({super.key, required this.engine});
  final Engine engine;

  @override
  State<DocScreen> createState() => _DocScreenState();
}

class _DocScreenState extends State<DocScreen> {
  Engine get e => widget.engine;
  final _doc = GlobalKey<DocViewState>();

  Map<String, dynamic> _status = {};
  int _rev = 0;

  /// The message pill's text (the engine's message, shown for its 4 seconds).
  String? _message;
  Timer? _messageTimer, _busyTimer;

  /// The phone layout last frame (null before the first); the first frame.
  bool? _wasCompact;
  bool _firstFrame = true;
  Map<String, dynamic> _chrome = {};
  Map<String, dynamic> _side = {};
  List<dynamic> _menuData = [];

  @override
  void initState() {
    super.initState();
    _read();
    _typeHook = (t) {
      // into a focused text field (the find bar's), else the document
      final field = FocusManager.instance.primaryFocus?.context?.findAncestorStateOfType<EditableTextState>();
      if (field == null) return _doc.currentState?.typeForTest(t);
      final v = field.textEditingValue.text + t;
      field.userUpdateTextEditingValue(TextEditingValue(text: v, selection: TextSelection.collapsed(offset: v.length)), SelectionChangedCause.keyboard);
    };
    _keyHook = (k) {
      if (k.startsWith('menu:')) {
        _run(k.substring(5));
        return;
      }
      // an engine action ("key action:gallery:/a.png\n/b.png"; \n for a new line)
      if (k.startsWith('action:')) {
        e.action(k.substring(7).replaceAll(r'\n', '\n'));
        _refreshAll();
        return;
      }
      e.key(k);
      _refreshAll();
    };
  }

  void _read() {
    _status = e.status();
    for (final u in (_status['openUrls'] as List? ?? const [])) {
      _openUrl(u as String);
    }
    _chrome = e.chrome();
    // (after the frame: the scale is applied above this screen)
    final pct = (_chrome['uiScale'] as num?)?.toInt() ?? 0;
    if (pct != uiScale.value) WidgetsBinding.instance.addPostFrameCallback((_) => uiScale.value = pct);
    // (Android: no Window menu; minimizing, zooming and full screen are the system's there)
    _menuData = [for (final m in e.menus()) if (!(Platform.isAndroid && m['label'] == 'Window')) m];
    _side = e.sidebar();
  }

  /// After the document, selection or caret changed: the chrome and menus follow, what was
  /// copied goes to the clipboard, a message shows.
  void _changed() {
    final before = _status['message'];
    final copied = e.copied();
    if (copied != null) Clipboard.setData(ClipboardData(text: copied));
    setState(() {
      _read();
      _rev++;
    });
    // something finishing in the background (a web video's lookup, a recording's clock)
    if (_status['busy'] == true) {
      _busyTimer ??= Timer(const Duration(milliseconds: 250), () {
        _busyTimer = null;
        if (mounted) _refreshAll();
      });
    }
    final msg = _status['message'] as String?;
    if (msg != null && msg != before) {
      _message = msg;
      _messageTimer?.cancel();
      _messageTimer = Timer(const Duration(seconds: 4), () {
        e.action('msg:');
        if (mounted) setState(() => _message = null);
      });
    }
  }

  void _refreshAll() => _doc.currentState?.refresh();

  /// A web page in the system's browser (Android: the app's channel; Linux: xdg-open).
  Future<void> _openUrl(String url) async {
    if (Platform.isAndroid) {
      await const MethodChannel('app.bramble.letters/open').invokeMethod('url', url);
    } else if (Platform.isLinux) {
      await Process.start('xdg-open', [url], mode: ProcessStartMode.detached);
    }
  }

  /// A message in the desktop's message pill (as app.notify).
  void _toast(String s) {
    _messageTimer?.cancel();
    setState(() => _message = s);
    _messageTimer = Timer(const Duration(seconds: 4), () {
      if (mounted) setState(() => _message = null);
    });
  }

  // ---- files ----

  Future<void> _open() async {
    final f = await FilePicker.pickFile(type: FileType.any);
    if (f?.path == null) return;
    final err = e.open(f!.path!);
    if (err != null) _toast("Couldn't open it: $err");
    _refreshAll();
  }

  Future<void> _saveAs(String ext, {bool export = false}) async {
    final dir = await getTemporaryDirectory();
    final title = (_status['title'] as String? ?? 'Untitled').replaceAll(RegExp(r'\.[a-z]+$'), '');
    final out = '${dir.path}/$title.$ext';
    final f = File(out);
    if (f.existsSync()) f.deleteSync();
    if (export) {
      final err = e.export(out);
      if (err != null) return _toast("Couldn't export: $err");
    } else {
      e.save(out);
    }
    if (!f.existsSync()) return _toast("Couldn't save");
    final saved = await FilePicker.saveFile(fileName: '$title.$ext', bytes: await f.readAsBytes());
    if (saved != null) _toast('Saved');
    _refreshAll();
  }

  /// A menu command: the ones that need Android's file picker or the view's zoom here, the rest
  /// as on the desktop.
  Future<void> _run(String cmd) async {
    final doc = _doc.currentState;
    switch (cmd) {
      case 'Open…':
        return _open();
      case 'Save':
      case 'Save As…':
        return _saveAs('pages');
      case 'Export To > PDF…':
        return _saveAs('pdf', export: true);
      case 'Export To > Word…':
        return _saveAs('docx');
      case 'Export To > EPUB…':
        return _saveAs('epub', export: true);
      case 'Export To > Plain Text…':
        return _saveAs('txt', export: true);
      case 'Export To > Rich Text Format…':
        return _saveAs('rtf', export: true);
      case 'Paste' || 'Paste and Match Style':
        final c = await Clipboard.getData(Clipboard.kTextPlain);
        if (c?.text != null) e.paste(c!.text!);
        return _refreshAll();
      case 'Zoom > Zoom In':
        if (doc != null) doc.setZoom(_nextZoom(doc.zoom, 1));
        return;
      case 'Zoom > Zoom Out':
        if (doc != null) doc.setZoom(_nextZoom(doc.zoom, -1));
        return;
      case 'Zoom > Actual Size':
        doc?.setZoom(1);
        return;
    }
    if (cmd.startsWith('open:')) {
      e.open(cmd.substring(5));
    } else {
      e.run(cmd);
    }
    _refreshAll();
  }

  static const _zooms = [0.25, 0.5, 0.75, 1.0, 1.25, 1.5, 2.0, 3.0, 4.0];
  double _nextZoom(double z, int dir) {
    if (dir > 0) return _zooms.firstWhere((v) => v > z + 1e-3, orElse: () => 4);
    return _zooms.lastWhere((v) => v < z - 1e-3, orElse: () => 0.25);
  }

  // ---- toolbar ----

  /// Where the title's popover hangs from (its place under the toolbar, in the window's box).
  Rect? _titleAt;

  void _toolbarButton(String id, Rect at) {
    switch (id) {
      case '__title':
        final box = _popCtx.currentContext!.findRenderObject() as RenderBox;
        _titleAt = Rect.fromPoints(box.globalToLocal(at.topLeft), box.globalToLocal(at.bottomRight));
        e.inspectorSet('dlg.docTitle.open', null);
        _refreshAll();
      case 'Zoom':
        _zoomMenu(at);
      case 'View' || 'Insert' || 'Media':
        _doc.currentState?.hideKeyboard();
        showTouchMenu(context, Look(_chrome), at, e.popover(id) as List? ?? [], (i) => _popAction(i['action'] as String? ?? ''), centered: true);
      case 'Table':
        _popAt((ctx) => showTablePopover(ctx, at));
      case 'Shape':
        _popAt((ctx) => showShapePopover(ctx, at, (e.popover('Shape') as Map<String, dynamic>?) ?? {}));
      case 'Chart':
        _popAt((ctx) => showChartPopover(ctx, at, (e.popover('Chart') as Map<String, dynamic>?) ?? {}));
      default:
        _doc.currentState?.hideKeyboard();
        e.toolbar(id);
        _refreshAll();
    }
  }

  /// chrome.rs zoom_menu: the page fits, then the zooms (the current one ticked), under the Zoom item.
  Future<void> _zoomMenu(Rect at) async {
    const fits = ['One Page', 'Two Pages', 'Fit Width', 'Fit Page'];
    final zoom = _doc.currentState?.zoom ?? 1;
    await showTouchMenu(context, Look(_chrome), at, [
      for (final f in fits) {'label': f, 'action': 'fit:$f'},
      {'separator': true},
      for (final z in _zooms) {'label': '${(z * 100).round()}%', 'action': '$z', 'checked': (z - zoom).abs() < 0.001},
    ], (i) {
      final v = i['action'] as String;
      if (v.startsWith('fit:')) {
        e.run('Zoom > ${v.substring(4)}');
        _refreshAll();
      } else {
        _doc.currentState?.setZoom(double.parse(v));
      }
    }, centered: true);
  }

  // ---- popovers ----

  final _popCtx = GlobalKey();

  Future<void> _popAt(Future<String?> Function(BuildContext ctx) show) async {
    _doc.currentState?.hideKeyboard();
    final ctx = _popCtx.currentContext;
    if (ctx == null) return;
    final a = await show(ctx);
    if (a != null) await _popAction(a);
  }

  /// A menu-like popover (View, Insert, Media) under its toolbar item: a touch menu.
  void _menuAt(List<dynamic> items, Offset at) {
    _doc.currentState?.hideKeyboard();
    showTouchMenu(context, Look(_chrome), Rect.fromLTWH(at.dx, at.dy, 0, 0), items, (i) => _popAction(i['action'] as String? ?? ''));
  }

  /// A popover or menu item's action: picking files here first for the Media items.
  Future<void> _popAction(String a) async {
    Future<List<String>> pick(FileType t) async => [for (final f in await FilePicker.pickFiles(type: t)) if (f.path != null) f.path!];
    switch (a) {
      case 'media:Choose Photo or Video…':
        for (final p in await pick(FileType.media)) {
          e.action('insert:$p');
        }
      case 'media:Choose Music…':
        for (final p in await pick(FileType.audio)) {
          e.action('insert:$p');
        }
      case 'media:Choose File…':
        for (final p in await pick(FileType.any)) {
          e.action('insert:$p');
        }
      case 'media:Add Image Gallery…':
        final ps = await pick(FileType.image);
        if (ps.isNotEmpty) e.action('gallery:${ps.join('\n')}');
      case 'tbcustomize':
        e.inspectorSet('dlg.customize.open', null);
      default:
        if (a.startsWith('tbmode:')) {
          e.inspectorSet('dlg.customize.mode', int.parse(a.substring(7)));
          break;
        }
        if (a.startsWith('cmd:')) return _run(a.substring(4));
        e.action(a);
    }
    _refreshAll();
  }

  @override
  Widget build(BuildContext context) {
    final look = Look(_chrome);
    final showInspector = _status['inspector'] == true;
    final tbH = _chrome['showToolbar'] == false ? 0.0 : DesktopToolbar.height(_chrome);
    final windows = _status['windows'] as List? ?? const [];
    final chooser = windows.where((w) => w['id'] == 'chooser').firstOrNull as Map<String, dynamic>?;
    final tabs = _status['tabs'] as Map<String, dynamic>?;
    final find = _status['find'] as Map<String, dynamic>?;
    final notes = _status['notes'] as Map<String, dynamic>?;
    final notesBar = notes?['bar'] as Map<String, dynamic>?;
    final notesPane = notes?['pane'] as Map<String, dynamic>?;
    // the classic look: the toolbar in a strip of its own; otherwise it floats over the pages
    final classic = look.classic;
    final stripH = classic ? tbH : 0.0;
    final floatH = classic ? 0.0 : tbH;
    // the pages run up under the floating toolbar unless a bar sits between them
    final canvasTop = tabs != null || notesBar != null ? 0.0 : floatH;
    // the phone layout (Settings > Layout; Automatic: an Android window under 700 points wide)
    final layout = (_chrome['uiLayout'] as num?)?.toInt() ?? 0;
    // (Automatic here: a phone's screen, under 600 points on its short side; an unfolded
    // foldable or a tablet keeps the side panes)
    // (the screen's own size, before Settings > Scaling: a bigger scale doesn't make it a phone)
    final view = View.of(context);
    final compact = layout == 1 || (layout == 0 && _chrome['android'] == true && (view.physicalSize / view.devicePixelRatio).shortestSide < 600);
    if (compact != _wasCompact) {
      final into = compact && _wasCompact != null;
      _wasCompact = compact;
      // into the phone form: the inspector and sidebar close (kn_ui form switched)
      if (into || (compact && _firstFrame)) {
        WidgetsBinding.instance.addPostFrameCallback((_) {
          if (_status['inspector'] == true) e.run('Inspector > Hide Inspector');
          if (_side['mode'] == 'thumbs' || _side['mode'] == 'toc') e.run('Document Only');
          _refreshAll();
        });
      }
    }
    _firstFrame = false;
    return AnnotatedRegion<SystemUiOverlayStyle>(
      value: look.light ? SystemUiOverlayStyle.dark : SystemUiOverlayStyle.light,
      child: Scaffold(
        backgroundColor: look.header,
        body: SafeArea(
          bottom: false,
          child: LookScope(
            look: look,
            child: Stack(children: [
            Column(key: _popCtx, children: [
              // (the phone layout too: the whole bar, scrolling sideways, rather than one ☰ menu)
              DesktopMenuBar(look: look, menus: _menuData, onCommand: _run),
              if (stripH > 0) Container(height: stripH, color: look.header, child: DesktopToolbar(
                        chrome: _chrome,
                        onPressed: _toolbarButton,
                        titleOpen: windows.any((w) => w['id'] == 'docTitle'),
                        customizing: windows.any((w) => w['id'] == 'customize'),
                        onItems: (ids) {
                          e.inspectorSet('dlg.customize.items', {'items': ids});
                          _refreshAll();
                        },
                        onDefaults: () {
                          e.inspectorSet('dlg.customize.defaults', null);
                          _refreshAll();
                        },
                        onLongPress: (at) => _menuAt([
                          for (final (m, n) in const [(1, 'Icon and Text'), (2, 'Icon Only'), (3, 'Text Only')]) {'label': n, 'action': 'tbmode:$m', 'enabled': true, 'checked': _chrome['toolbarMode'] == m},
                          {'separator': true},
                          {'label': 'Customize Toolbar\u2026', 'action': 'tbcustomize', 'enabled': true},
                        ], at.bottomLeft),
                      )),
              Expanded(
                child: Stack(children: [
                  Row(crossAxisAlignment: CrossAxisAlignment.stretch, children: [
                    Expanded(
                      child: Column(crossAxisAlignment: CrossAxisAlignment.stretch, children: [
                        // the document tabs (two or more): under the toolbar's room, across the
                        // sidebar and the pages
                        if (tabs != null) ...[
                          SizedBox(height: floatH),
                          DocTabBar(engine: e, s: tabs, onChanged: _refreshAll),
                        ],
                        Expanded(
                          child: Row(crossAxisAlignment: CrossAxisAlignment.stretch, children: [
                            // the sidebar: under the toolbar's band, 180 wide
                            SlidePane(
                              open: _side['mode'] == 'thumbs' || _side['mode'] == 'toc',
                              width: sidebarWidth(_side['mode'] as String?),
                              fromLeft: true,
                              child: Padding(padding: EdgeInsets.only(top: tabs != null ? 0 : floatH), child: Sidebar(engine: e, s: _side, look: look, onChanged: _refreshAll, onMenu: _menuAt)),
                            ),
                            Expanded(
                              child: Column(crossAxisAlignment: CrossAxisAlignment.stretch, children: [
                                // the comments & changes bar: under the toolbar's room, over the pages
                                if (tabs == null && notesBar != null) SizedBox(height: floatH),
                                if (notesBar != null) AnnotationsBar(engine: e, s: notesBar, onChanged: _refreshAll),
                                Expanded(
                                  child: Row(crossAxisAlignment: CrossAxisAlignment.stretch, children: [
                                    Expanded(child: DocView(key: _doc, engine: e, look: look, topInset: canvasTop, ruler: _status['ruler'] as Map<String, dynamic>?, wordCount: _status['wordCount'] as String?, message: _message, screenView: _status['screenView'] == true, onChanged: _changed, onMenuAction: _popAction, onMenu: _menuAt)),
                                    // View > Show Comments Pane: at the right of the pages, under the ruler
                                    if (notesPane != null)
                                      Padding(
                                        padding: EdgeInsets.only(top: canvasTop + (_status['ruler'] != null ? 26 : 0)),
                                        child: CommentsPane(engine: e, s: notesPane, onChanged: _refreshAll),
                                      ),
                                  ]),
                                ),
                                // the find bar: under the pages, just above the on-screen keyboard (Pages on iPad and iPhone)
                                if (find != null) FindBar(engine: e, s: find, onChanged: _refreshAll, onMenu: _menuAt, compact: compact),
                              ]),
                            ),
                          ]),
                        ),
                      ]),
                    ),
                    // (it stays built while it slides out)
                    if (!compact) SlidePane(open: showInspector, width: 300, child: Inspector(engine: e, look: look, topInset: floatH, revision: _rev, onChanged: _refreshAll)),
                  ]),
                  // the template chooser over the document, below the toolbar (welcome.rs overlay)
                  // the phone layout's inspector: a sheet from the bottom (kn_ui form sheet)
                  if (compact)
                    Positioned.fill(
                      child: InspectorSheet(
                        open: showInspector,
                        onClose: () {
                          e.run('Inspector > Hide Inspector');
                          _refreshAll();
                        },
                        child: Inspector(engine: e, look: look, topInset: 0, revision: _rev, onChanged: _refreshAll, sheet: true),
                      ),
                    ),
                  if (chooser != null)
                    Positioned.fill(
                      top: floatH,
                      child: Chooser(w: chooser, press: (b, v) {
                        e.inspectorSet('dlg.chooser.$b', v);
                        _refreshAll();
                      }, onBrowse: () {
                        e.inspectorSet('dlg.chooser.browse', null);
                        _open();
                      }),
                    ),
                  if (floatH > 0) Positioned(top: 0, left: 0, right: 0, height: floatH, child: DesktopToolbar(
                        chrome: _chrome,
                        onPressed: _toolbarButton,
                        titleOpen: windows.any((w) => w['id'] == 'docTitle'),
                        customizing: windows.any((w) => w['id'] == 'customize'),
                        onItems: (ids) {
                          e.inspectorSet('dlg.customize.items', {'items': ids});
                          _refreshAll();
                        },
                        onDefaults: () {
                          e.inspectorSet('dlg.customize.defaults', null);
                          _refreshAll();
                        },
                        onLongPress: (at) => _menuAt([
                          for (final (m, n) in const [(1, 'Icon and Text'), (2, 'Icon Only'), (3, 'Text Only')]) {'label': n, 'action': 'tbmode:$m', 'enabled': true, 'checked': _chrome['toolbarMode'] == m},
                          {'separator': true},
                          {'label': 'Customize Toolbar\u2026', 'action': 'tbcustomize', 'enabled': true},
                        ], at.bottomLeft),
                      )),
                ]),
              ),
            ]),
            // the desktop's windows (dialogs) over everything
            Positioned.fill(child: Windows(engine: e, windows: [for (final w in windows) if (w['id'] != 'chooser') w], titleAt: _titleAt, toolbarBottom: DesktopMenuBar.height + tbH, compact: compact, savedColors: _status['savedColors'] as String? ?? '', onChanged: _refreshAll)),
            ]),
          ),
        ),
      ),
    );
  }
}
