// Keydeck for Android: Bramble Office's presentation app, its window as the desktop's (the menu
// bar, the floating toolbar, the slide navigator, the slide, the inspector at the right) made
// in Flutter. The presentations, editing logic and slide drawing are the desktop Keydeck's own
// (the Rust engine, engine.dart); nothing on screen is drawn by it but the slides' content.
import 'dart:async';
import 'dart:convert';
import 'dart:io';
import 'dart:math' as math;

import 'package:file_picker/file_picker.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:path_provider/path_provider.dart';

import 'desktop/chooser.dart';
import 'desktop/colors.dart' show appShot;
import 'desktop/inspector.dart';
import 'desktop/inspector_parts.dart';
import 'desktop/layout_grid.dart';
import 'desktop/look.dart';
import 'desktop/menubar.dart';
import 'desktop/navigator.dart';
import 'desktop/popovers.dart' show showShapePopover;
import 'desktop/scale.dart';
import 'desktop/show.dart';
import 'desktop/slide.dart';
import 'desktop/slide_view.dart';
import 'desktop/tabbar.dart';
import 'desktop/views.dart';
import 'desktop/toolbar.dart';
import 'desktop/touch_menu.dart';
import 'desktop/widgets.dart';
import 'desktop/windows.dart';
import 'engine.dart';
import 'remote/remote.dart';
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
  engine.start(home);
  // KEYDECK_OPEN=<file> (automated checks): a presentation open at the start
  final open = Platform.environment['KEYDECK_OPEN'];
  if (open != null) {
    final err = engine.open(open);
    if (err != null) stderr.writeln('KEYDECK_OPEN: $err');
  }
  runApp(RepaintBoundary(key: appShot, child: KeydeckApp(engine: engine)));
  startTestSnapshots(appShot, type: (t) => _typeHook?.call(t), key: (k) => _keyHook?.call(k));
}

class KeydeckApp extends StatelessWidget {
  const KeydeckApp({super.key, required this.engine});
  final Engine engine;

  @override
  Widget build(BuildContext context) => MaterialApp(
        title: 'Keydeck',
        debugShowCheckedModeBanner: false,
        theme: ThemeData(useMaterial3: true, fontFamily: 'Carlito', splashFactory: NoSplash.splashFactory),
        // (Settings > Scaling: everything, menus and windows too)
        builder: (ctx, child) => _testPhone(UiScaled(child: _testKeyboard(child ?? const SizedBox()))),
        home: DocScreen(engine: engine),
      );
}

/// KEYDECK_PHONE=`w`x`h` (the automated checks): the app in a box that size at the window's top
/// left, as a phone's screen.
Widget _testPhone(Widget child) {
  final m = RegExp(r'^(\d+)x(\d+)$').firstMatch(Platform.environment['KEYDECK_PHONE'] ?? '');
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

/// KEYDECK_KEYBOARD=`points` (the automated checks): an on-screen keyboard that tall, as
/// MediaQuery's viewInsets, drawn as a grey block.
Widget _testKeyboard(Widget child) {
  final k = double.tryParse(Platform.environment['KEYDECK_KEYBOARD'] ?? '');
  if (k == null) return child;
  return Builder(builder: (ctx) {
    final mq = MediaQuery.of(ctx);
    return Stack(children: [
      MediaQuery(data: mq.copyWith(viewInsets: mq.viewInsets.copyWith(bottom: k)), child: child),
      Positioned(left: 0, right: 0, bottom: 0, height: k, child: const ColoredBox(color: Color(0xFF8E8E93))),
    ]);
  });
}

/// Whether the screen is a phone's (the phone layout: Settings > Layout, Automatic: an Android
/// screen under 600 points on its short side, judged by the screen's own size, before Settings
/// > Scaling). KEYDECK_PHONE counts as one.
bool isCompact(BuildContext context, Map<String, dynamic> chrome) {
  final layout = (chrome['uiLayout'] as num?)?.toInt() ?? 0;
  if (layout == 1) return true;
  if (layout != 0) return false;
  // (the test screen's own size, as a device's before Settings > Scaling)
  final test = RegExp(r'^(\d+)x(\d+)$').firstMatch(Platform.environment['KEYDECK_PHONE'] ?? '');
  if (test != null) return math.min(double.parse(test[1]!), double.parse(test[2]!)) < 600;
  final view = View.of(context);
  return chrome['android'] == true && (view.physicalSize / view.devicePixelRatio).shortestSide < 600;
}

class DocScreen extends StatefulWidget {
  const DocScreen({super.key, required this.engine});
  final Engine engine;

  @override
  State<DocScreen> createState() => _DocScreenState();
}

class _DocScreenState extends State<DocScreen> {
  Engine get e => widget.engine;
  final _doc = GlobalKey<SlideViewState>();

  /// Inside the window's look: where popovers open from.
  final _popCtx = GlobalKey();

  Map<String, dynamic> _status = {};
  int _rev = 0;

  /// The message pill's text (the engine's message, shown for its 4 seconds).
  String? _message;
  Timer? _messageTimer, _busyTimer;

  /// The phone layout last frame (null before the first); the first frame.
  bool? _wasCompact;
  bool _firstFrame = true;
  Map<String, dynamic> _chrome = {};
  Map<String, dynamic> _nav = {};
  Map<String, dynamic> _geo = {};
  List<dynamic> _menuData = [];

  /// The theme chooser's themes and recent presentations (shown when nothing's open).
  Map<String, dynamic>? _themes;

  @override
  void initState() {
    super.initState();
    _read();
    _typeHook = (t) {
      // into a focused field of a window, else the slide
      final field = FocusManager.instance.primaryFocus?.context?.findAncestorStateOfType<EditableTextState>();
      if (field == null || field.context.findAncestorStateOfType<SlideViewState>() != null) return _doc.currentState?.typeForTest(t);
      final v = field.textEditingValue.text + t;
      field.userUpdateTextEditingValue(TextEditingValue(text: v, selection: TextSelection.collapsed(offset: v.length)), SelectionChangedCause.keyboard);
    };
    _keyHook = (k) {
      if (k.startsWith('menu:')) {
        _run(k.substring(5));
        return;
      }
      // an engine action ("action:slide:2")
      if (k.startsWith('action:')) {
        _popAction(k.substring(7));
        return;
      }
      if (k.startsWith('toolbar:')) {
        _toolbarButton(k.substring(8), Rect.fromLTWH(MediaQuery.of(context).size.width - 60, 40, 40, 30));
        return;
      }
      // an inspector control: "set:<key>=<json>"
      if (k.startsWith('set:')) {
        final kv = k.substring(4);
        final i = kv.indexOf('=');
        Object? v;
        try {
          v = jsonDecode(kv.substring(i + 1));
        } catch (_) {
          v = kv.substring(i + 1);
        }
        e.inspectorSet(kv.substring(0, i), v);
        _refreshAll();
        return;
      }
      e.key(k);
      _refreshAll();
    };
  }

  void _read() {
    _status = e.status();
    _chrome = e.chrome();
    // (after the frame: the scale is applied above this screen)
    final pct = (_chrome['uiScale'] as num?)?.toInt() ?? 0;
    if (pct != uiScale.value) WidgetsBinding.instance.addPostFrameCallback((_) => uiScale.value = pct);
    // (Android: no Window menu; minimizing, zooming and full screen are the system's there)
    _menuData = [for (final m in e.menus()) if (!(Platform.isAndroid && m['label'] == 'Window')) m];
    // Play > Keydeck Remote…: this device as the remote of a Keydeck on a computer
    for (final m in _menuData) {
      if (m['label'] == 'Play') {
        (m['kids'] as List).addAll([
          {'separator': true},
          {'label': 'Keydeck Remote…', 'command': '__remote', 'enabled': true},
        ]);
      }
    }
    _nav = e.navigator();
    _geo = e.geometry();
    if (_status['open'] != true || _status['changingTheme'] == true) {
      _themes ??= e.themes();
    } else {
      _themes = null;
    }
  }

  /// After the presentation, selection or caret changed: the chrome and menus follow, what was
  /// copied goes to the clipboard, a message shows.
  void _changed() {
    final before = _status['message'];
    final copied = e.copied();
    if (copied != null) Clipboard.setData(ClipboardData(text: copied));
    setState(() {
      _read();
      _rev++;
    });
    // background work (opening, a web video's details, pictures for a gallery): polled until done
    if (_status['busy'] == true && _busyTimer == null) {
      _busyTimer = Timer.periodic(const Duration(milliseconds: 150), (_) {
        if (e.tick() != 0) _refreshAll();
        if (e.status()['busy'] != true) {
          _busyTimer?.cancel();
          _busyTimer = null;
          _refreshAll();
        }
      });
    }
    _openUrls();
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

  /// Web pages and mail links the engine opened (a link in the show, Support Development…):
  /// the system's browser or mail app (MainActivity "open").
  void _openUrls() {
    for (final u in e.urls()) {
      const MethodChannel('app.bramble.keydeck/open').invokeMethod('url', u).catchError((_) => null);
    }
  }

  void _refreshAll() {
    final d = _doc.currentState;
    if (d != null && _status['open'] == true) {
      d.refresh();
    } else {
      _changed();
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

  /// Saves (or exports) through the system's save picker; whether it was saved.
  Future<bool> _saveAs(String ext, {bool export = false}) async {
    final dir = await getTemporaryDirectory();
    final title = (_status['title'] as String? ?? 'Untitled').replaceAll(RegExp(r'\.(key|pptx)$'), '');
    final out = '${dir.path}/$title.$ext';
    final f = File(out);
    if (f.existsSync()) f.deleteSync();
    final err = export ? e.export(out) : e.save(out);
    if (err != null || !f.existsSync()) {
      _toast("Couldn't save${err == null ? '' : ': $err'}");
      return false;
    }
    final saved = await FilePicker.saveFile(fileName: '$title.$ext', bytes: await f.readAsBytes());
    if (saved != null) _toast('Saved');
    _refreshAll();
    return saved != null;
  }

  /// A presentation with unsaved changes asked to close: Save, Don't Save or Cancel.
  Future<void> _askUnsaved() async {
    final u = e.status()['unsaved'] as Map<String, dynamic>?;
    if (u == null || !mounted) return;
    final tab = u['tab'] as int;
    final look = Look(_chrome);
    final pick = await showDialog<String>(
      context: context,
      builder: (ctx) => AlertDialog(
        backgroundColor: look.panel,
        title: Text('Do you want to save the changes to \u201c${u['name']}\u201d?', style: Look.font(17, look.text, bold: true)),
        content: Text("Your changes will be lost if you don't save them.", style: Look.font(14, look.textDim)),
        actions: [
          TextButton(onPressed: () => Navigator.of(ctx).pop('discard'), child: Text("Don't Save", style: Look.font(15, look.accent))),
          TextButton(onPressed: () => Navigator.of(ctx).pop('keep'), child: Text('Cancel', style: Look.font(15, look.accent))),
          TextButton(onPressed: () => Navigator.of(ctx).pop('save'), child: Text('Save', style: Look.font(15, look.accent, bold: true))),
        ],
      ),
    );
    switch (pick) {
      case 'discard':
        e.action('tab:discard:$tab');
      case 'save':
        e.action('tab:keep');
        e.action('tab:select:$tab');
        _refreshAll();
        if (await _saveAs(_status['pptx'] == true ? 'pptx' : 'key')) e.action('tab:discard:$tab');
      default:
        e.action('tab:keep');
    }
    _refreshAll();
  }

  int get _current => (_geo['current'] as num?)?.toInt() ?? 0;

  void _play({bool rehearse = false}) {
    _doc.currentState?.hideKeyboard();
    e.playStart(_current, rehearse: rehearse);
    _refreshAll();
  }

  /// A menu command: the ones that need Android's file picker, the show or the view's zoom
  /// here, the rest as on the desktop.
  Future<void> _run(String cmd) async {
    final doc = _doc.currentState;
    switch (cmd) {
      case 'Open…':
        return _open();
      case '__remote':
        _doc.currentState?.hideKeyboard();
        await Navigator.of(context).push(MaterialPageRoute<void>(builder: (_) => const RemoteScreen()));
        return;
      case 'Save' || 'Save…' || 'Save As…' || 'Duplicate':
        await _saveAs(_status['pptx'] == true ? 'pptx' : 'key');
        return;
      case 'PDF…' || 'Export To > PDF…':
        await _saveAs('pdf', export: true);
        return;
      case 'PowerPoint…' || 'Export To > PowerPoint…':
        await _saveAs('pptx', export: true);
        return;
      case 'HTML…' || 'Export To > HTML…':
        await _saveAs('html', export: true);
        return;
      // the slides as pictures: PNGs in a .zip (one file for Android's save picker)
      case 'Images…' || 'Export To > Images…':
        await _saveAs('zip', export: true);
        return;
      // (Android has no printing of its own here: the PDF, to print from a PDF viewer)
      case 'Print…':
        await _saveAs('pdf', export: true);
        return;
      case 'Movie…' || 'Animated GIF…' || 'Export To > Movie…' || 'Export To > Animated GIF…':
        return _toast('Movies and GIFs are made with ffmpeg, which Android doesn\u2019t have: export a PDF or images instead');
      case 'Image Gallery':
        final paths = [for (final f in await FilePicker.pickFiles(type: FileType.image)) if (f.path != null) f.path!];
        if (paths.isNotEmpty) e.action('gallery:${paths.join('\n')}');
        return _refreshAll();
      // the desktop's media browsers: Android's own pickers
      case 'Show Photo and Video Browser':
        return _popAction('media:Choose Photo or Video…');
      case 'Show Music Browser':
        return _popAction('media:Choose Music…');
      case 'Choose File' || 'Choose…':
        return _popAction('media:Choose Photo or Video…');
      // a new presentation in a tab of its own: the theme chooser shows there
      case 'New…':
        e.action('tab:new');
        return _refreshAll();
      case 'Close Tab' || 'Close Window' || 'Close All' || 'Close':
        e.action('close');
        _refreshAll();
        return _askUnsaved();
      case 'Paste' || 'Paste and Match Style':
        final c = await Clipboard.getData(Clipboard.kTextPlain);
        e.paste(c?.text ?? '');
        return _refreshAll();
      case 'Zoom In' || 'Zoom > Zoom In':
        doc?.setZoom(doc.zoom * 1.25);
        return setState(() {});
      case 'Zoom Out' || 'Zoom > Zoom Out':
        doc?.setZoom(doc.zoom / 1.25);
        return setState(() {});
      case 'Actual Size' || 'Zoom > Actual Size':
        doc?.setZoom(1);
        return setState(() {});
      case 'Fit Slide' || 'Zoom > Fit Slide' || 'Zoom to Fit':
        doc?.setZoom(null);
        return setState(() {});
      case 'Play Slideshow' || 'Play':
        return _play();
      case 'Rehearse Slideshow':
        return _play(rehearse: true);
    }
    if (cmd.startsWith('open:')) {
      final err = e.open(cmd.substring(5));
      if (err != null) _toast("Couldn't open it: $err");
    } else {
      e.run(cmd);
    }
    _refreshAll();
  }

  // ---- toolbar ----

  void _toolbarButton(String id, Rect at) {
    _doc.currentState?.hideKeyboard();
    final look = Look(_chrome);
    switch (id) {
      case '__title':
        return;
      case 'Zoom':
        _zoomMenu(at);
      case 'View' || 'TSKToolbarItemMedia':
        showTouchMenu(context, look, at, e.popover(id == 'View' ? 'View' : 'Media') as List? ?? [], (i) => _popAction(i['action'] as String? ?? ''), centered: true);
      case 'KNMacToolbarItemAddSlide':
        _layoutPicker(at);
      case 'TSKToolbarItemShape':
        final ctx0 = _popCtx.currentContext;
        if (ctx0 == null) return;
        showShapePopover(ctx0, at, (e.popover('Shape') as Map<String, dynamic>?) ?? {}).then((a) {
          if (a != null) _popAction(a);
        });
      case 'TSKToolbarItemChart' || 'TSKToolbarItemTable':
        // the Insert menu's Chart and Table lists as touch menus
        final label = const {'TSKToolbarItemChart': 'Chart', 'TSKToolbarItemTable': 'Table'}[id]!;
        showTouchMenu(context, look, at, [for (final k in _submenu(label)) _asItem(k)], (i) => _popAction(i['action'] as String? ?? ''), centered: true);
      case 'TSKToolbarItemPlay':
        _play();
      default:
        e.toolbar(id);
        _refreshAll();
    }
  }

  /// A menu bar item's data as a touch menu item (its command as the action).
  Map<String, dynamic> _asItem(dynamic k) {
    final m = k as Map<String, dynamic>;
    if (m['separator'] == true) return m;
    if (m['kids'] is List) return {'label': m['label'], 'kids': [for (final x in m['kids'] as List) _asItem(x)]};
    return {'label': m['label'], 'action': 'cmd:${m['command']}', 'enabled': m['enabled'], 'checked': m['checked']};
  }

  /// The Insert menu's submenu called `label`.
  List<dynamic> _submenu(String label) {
    for (final m in _menuData) {
      if (m['label'] != 'Insert') continue;
      for (final k in (m['kids'] as List? ?? const [])) {
        if (k['label'] == label && k['kids'] is List) return k['kids'] as List;
      }
    }
    return const [];
  }

  /// The zoom menu: fit, then the zooms (the current one ticked), under the Zoom item.
  Future<void> _zoomMenu(Rect at) async {
    const zooms = [0.25, 0.5, 0.75, 1.0, 1.25, 1.5, 2.0, 3.0, 4.0];
    final doc = _doc.currentState;
    final z = doc?.zoom ?? 1;
    await showTouchMenu(context, Look(_chrome), at, [
      {'label': 'Fit Slide', 'action': 'fit', 'checked': doc?.fitted ?? true},
      {'separator': true},
      for (final v in zooms) {'label': '${(v * 100).round()}%', 'action': '$v', 'checked': doc?.fitted == false && (v - z).abs() < 0.001},
    ], (i) {
      final v = i['action'] as String;
      doc?.setZoom(v == 'fit' ? null : double.parse(v));
      setState(() {});
    }, centered: true);
  }

  /// Add Slide: the theme's layouts as pictures, under the button.
  Future<void> _layoutPicker(Rect at) async {
    final data = e.popover('AddSlide') as Map<String, dynamic>?;
    if (data == null) return;
    final names = [for (final n in (data['layouts'] as List? ?? const [])) n as String];
    final aspect = (data['aspect'] as num?)?.toDouble() ?? 9 / 16;
    final ctx0 = _popCtx.currentContext;
    if (ctx0 == null) return;
    final pick = await showLayoutGrid(ctx0, at, e, names: names, aspect: aspect);
    if (pick != null) {
      e.action('layout:$pick');
      _refreshAll();
    }
  }

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
      case 'media:Content Hub Shapes':
        e.run('Browse Content Hub');
      case 'tbcustomize':
        e.inspectorSet('dlg.customize.open', null);
      case 'play':
        return _play();
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

  Widget _toolbar(List<dynamic> windows) {
    final z = _doc.currentState?.zoom;
    return DesktopToolbar(
      chrome: {..._chrome, 'zoom': ?z},
      onPressed: _toolbarButton,
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
        {'label': 'Customize Toolbar…', 'action': 'tbcustomize', 'enabled': true},
      ], at.bottomLeft),
    );
  }

  @override
  Widget build(BuildContext context) {
    final look = Look(_chrome);
    final open = _status['open'] == true;
    final showInspector = _status['inspector'] == true && open;
    final tbH = _chrome['showToolbar'] == false ? 0.0 : DesktopToolbar.height(_chrome);
    final windows = _status['windows'] as List? ?? const [];
    final classic = look.classic;
    final stripH = classic ? tbH : 0.0;
    final floatH = classic ? 0.0 : tbH;
    final compact = isCompact(context, _chrome);
    if (compact != _wasCompact) {
      final into = compact && _wasCompact != null;
      _wasCompact = compact;
      // into the phone form: the inspector closes (kn_ui form switched)
      if (into || (compact && _firstFrame)) {
        WidgetsBinding.instance.addPostFrameCallback((_) {
          if (_status['inspector'] == true) e.toolbar(_status['inspectorMode'] as String? ?? 'Format');
          _refreshAll();
        });
      }
    }
    _firstFrame = false;
    final size = _geo['size'] as List?;
    final slideSize = size == null ? const Size(1920, 1080) : Size((size[0] as num).toDouble(), (size[1] as num).toDouble());
    final showNav = open && _status['navigator'] == true;
    final playing = _status['playing'] == true;
    // the slide list: narrower on Android (Letters' sidebarWidth); the inspector a sheet from
    // the foot whenever the slide would be left less than 420 points beside the panels (an
    // unfolded phone at Android's 1.25 scale: about 570 points across)
    final navW = Platform.isAndroid ? 124.0 : 160.0;
    final roomW = MediaQuery.of(context).size.width - (showNav && !compact ? navW : 0) - 300;
    final sheetInsp = compact || roomW < 420;
    final navigator = Navigator2(engine: e, look: look, s: _nav, slide: slideSize, onChanged: _refreshAll, onAction: _popAction, horizontal: compact, topInset: compact ? 0 : floatH);
    return AnnotatedRegion<SystemUiOverlayStyle>(
      value: look.light ? SystemUiOverlayStyle.dark : SystemUiOverlayStyle.light,
      child: Scaffold(
        backgroundColor: look.header,
        body: Stack(children: [
          SafeArea(
            bottom: false,
            child: LookScope(
              look: look,
              child: Stack(children: [
                Column(key: _popCtx, children: [
                  // (the phone layout too: the whole bar, scrolling sideways, rather than one ☰ menu)
                  DesktopMenuBar(look: look, menus: _menuData, onCommand: _run),
                  if (stripH > 0) Container(height: stripH, color: look.header, child: _toolbar(windows)),
                  Expanded(
                    child: Stack(children: [
                      Row(crossAxisAlignment: CrossAxisAlignment.stretch, children: [
                        // the navigator: down the left under the toolbar's band (a tablet), along the foot (a phone)
                        if (!compact) SlidePane(open: showNav, width: navW, fromLeft: true, child: navigator),
                        Expanded(
                          child: Column(children: [
                            // the presentations' tabs (two or more): under the toolbar's room
                            if (_status['tabs'] != null) ...[
                              SizedBox(height: floatH),
                              DocTabBar(engine: e, s: _status['tabs'] as Map<String, dynamic>, onChanged: () {
                                _refreshAll();
                                _askUnsaved();
                              }),
                            ],
                            Expanded(
                              child: open && _status['view'] == 'LightTable'
                                  ? LightTable(engine: e, look: look, s: _nav, slide: slideSize, onChanged: _refreshAll, topInset: _status['tabs'] != null ? 0 : floatH)
                                  : open && _status['view'] == 'Outline'
                                  ? OutlineView(engine: e, look: look, revision: _rev, current: (_geo['current'] as num?)?.toInt() ?? 0, onChanged: _refreshAll, topInset: _status['tabs'] != null ? 0 : floatH)
                                  : open
                                  ? LayoutBuilder(
                                      // (the phone's sheet rests at half the room, over the filmstrip: the slide fits above it)
                                      builder: (ctx, box) => SlideView(key: _doc, engine: e, look: look, topInset: _status['tabs'] != null ? 0 : floatH, bottomInset: sheetInsp && showInspector ? (box.maxHeight + (compact && showNav ? 96 : 0)) * 0.5 - (compact && showNav ? 96 : 0) : 0, message: _message, onChanged: _changed, onMenuAction: _popAction),
                                    )
                                  : ColoredBox(color: look.canvas),
                            ),
                            if (open && _status['notes'] == true && (_status['view'] == 'Navigator' || _status['view'] == 'SlideOnly')) NotesPanel(engine: e, look: look, current: (_geo['current'] as num?)?.toInt() ?? 0, revision: _rev),
                            if (open && _status['editingLayouts'] == true)
                              Container(
                                height: 44,
                                color: look.header,
                                padding: const EdgeInsets.symmetric(horizontal: 12),
                                child: Row(children: [
                                  Expanded(child: Text('Editing slide layouts — changes apply to every slide that uses the layout', maxLines: 2, style: Look.font(13, look.textDim))),
                                  TextButton(onPressed: () {
                                    e.action('layouts:done');
                                    _refreshAll();
                                  }, child: Text('Done', style: Look.font(15, look.accent, bold: true))),
                                ]),
                              ),
                            if (compact && showNav) Container(height: 96, decoration: BoxDecoration(border: Border(top: BorderSide(color: look.separator))), child: navigator),
                          ]),
                        ),
                        if (!sheetInsp) SlidePane(open: showInspector, width: 300, child: Inspector(engine: e, look: look, topInset: floatH, revision: _rev, onChanged: _refreshAll)),
                      ]),
                      if (sheetInsp)
                        Positioned.fill(
                          child: InspectorSheet(
                            open: showInspector,
                            onClose: () {
                              e.toolbar(_status['inspectorMode'] as String? ?? 'Format');
                              _refreshAll();
                            },
                            child: Inspector(engine: e, look: look, topInset: 0, revision: _rev, onChanged: _refreshAll, sheet: true),
                          ),
                        ),
                      // File > Change Theme: the chooser over the open presentation
                      if (open && _status['changingTheme'] == true && _themes != null)
                        Positioned.fill(
                          top: floatH,
                          child: Chooser(
                            w: {
                              'title': 'Choose a new theme for this presentation',
                              'app': 'Keydeck',
                              'section': 'Themes',
                              'all': 'All Themes',
                              'templates': [for (final t in _themes!['themes'] as List) t['name']],
                              'previews': [for (final t in _themes!['themes'] as List) t['preview']],
                              'default': _themes!['default'],
                              'tile': const [176.0, 99.0],
                              'cancel': true,
                              'recent': const [],
                              'backdrop': look.canvas.toARGB32(),
                            },
                            press: (b, v) {
                              e.action(b == 'create' ? 'theme:$v' : 'theme:cancel');
                              _refreshAll();
                            },
                            onBrowse: () {},
                          ),
                        ),
                      // the theme chooser when nothing's open (welcome.rs), below the toolbar
                      if (!open && _themes != null)
                        Positioned.fill(
                          top: floatH,
                          child: Chooser(
                            w: {
                              'title': 'Choose a Theme',
                              'app': 'Keydeck',
                              'section': 'Themes',
                              'all': 'All Themes',
                              'hint': 'or open a Keynote or PowerPoint file',
                              'templates': [for (final t in _themes!['themes'] as List) t['name']],
                              'previews': [for (final t in _themes!['themes'] as List) t['preview']],
                              'default': _themes!['default'],
                              'tile': const [176.0, 99.0],
                              // (in a tab of its own, beside others: Cancel closes it)
                              'cancel': _status['tabs'] != null,
                              'recent': _themes!['recent'],
                              'backdrop': look.canvas.toARGB32(),
                            },
                            press: (b, v) {
                              if (b == 'create') {
                                final err = e.create(v as int);
                                if (err != null) _toast("Couldn't create it: $err");
                              }
                              if (b == 'cancel') e.action('close');
                              if (b == 'open') {
                                final err = e.open(v as String);
                                if (err != null) _toast("Couldn't open it: $err");
                              }
                              _refreshAll();
                            },
                            onBrowse: _open,
                          ),
                        ),
                      if (floatH > 0) Positioned(top: 0, left: 0, right: 0, height: floatH, child: _toolbar(windows)),
                    ]),
                  ),
                ]),
                // the desktop's windows (Settings, the Content Hub, Customize Toolbar) over everything
                Positioned.fill(child: Windows(engine: e, windows: windows, toolbarBottom: DesktopMenuBar.height + tbH, compact: compact, savedColors: _status['savedColors'] as String? ?? '', onChanged: _refreshAll)),
              ]),
            ),
          ),
          if (playing)
            Positioned.fill(
              child: SlideShow(
                engine: e,
                onEnd: _refreshAll,
              ),
            ),
        ]),
      ),
    );
  }
}
