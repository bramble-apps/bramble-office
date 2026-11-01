// Values for Android: Bramble Office's spreadsheet app, its window as the desktop's (the menu
// bar, the toolbar on its strip, the sheet tabs, the sheet, the cell bar, the inspector at the
// right) made in Flutter. The spreadsheets, editing logic and sheet drawing are the desktop
// Values' own (the Rust engine, engine.dart); nothing on screen is drawn by it but the sheet's
// content.
import 'dart:async';
import 'dart:convert';
import 'dart:io';

import 'package:file_picker/file_picker.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:path_provider/path_provider.dart';

import 'desktop/cell_bar.dart';
import 'desktop/chooser.dart';
import 'desktop/colors.dart' show appShot;
import 'desktop/findbar.dart';
import 'desktop/inspector.dart';
import 'desktop/look.dart';
import 'desktop/menubar.dart';
import 'desktop/popovers.dart';
import 'desktop/scale.dart';
import 'desktop/sheet_tabs.dart';
import 'desktop/sheet_view.dart';
import 'desktop/slide.dart';
import 'desktop/toolbar.dart';
import 'desktop/touch_menu.dart';
import 'desktop/widgets.dart';
import 'desktop/windows.dart';
import 'engine.dart';
import 'test_snap.dart';

/// (automated checks: typing and keys as a keyboard's, test_snap.dart)
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
  // (automated checks: a spreadsheet to open)
  final file = Platform.environment['VALUES_OPEN'];
  if (file != null) engine.open(file);
  runApp(RepaintBoundary(key: appShot, child: ValuesApp(engine: engine)));
  startTestSnapshots(appShot, type: (t) => _typeHook?.call(t), key: (k) => _keyHook?.call(k));
}

class ValuesApp extends StatelessWidget {
  const ValuesApp({super.key, required this.engine});
  final Engine engine;

  @override
  Widget build(BuildContext context) => MaterialApp(
        title: 'Values',
        debugShowCheckedModeBanner: false,
        theme: ThemeData(useMaterial3: true, fontFamily: 'Carlito', splashFactory: NoSplash.splashFactory),
        // (Settings > Scaling: everything, menus and windows too)
        builder: (ctx, child) => _testPhone(UiScaled(child: _testKeyboard(child ?? const SizedBox()))),
        home: SheetScreen(engine: engine),
      );
}

/// VALUES_PHONE=`w`x`h` (the automated checks): the app in a box that size at the window's top
/// left, as a phone's screen.
Widget _testPhone(Widget child) {
  final m = RegExp(r'^(\d+)x(\d+)$').firstMatch(Platform.environment['VALUES_PHONE'] ?? '');
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

/// VALUES_KEYBOARD=`points` (the automated checks): an on-screen keyboard that tall, as
/// MediaQuery's viewInsets, drawn as a grey block.
Widget _testKeyboard(Widget child) {
  final k = double.tryParse(Platform.environment['VALUES_KEYBOARD'] ?? '');
  if (k == null) return child;
  return Builder(builder: (ctx) {
    final mq = MediaQuery.of(ctx);
    return Stack(children: [
      MediaQuery(data: mq.copyWith(viewInsets: mq.viewInsets.copyWith(bottom: k)), child: child),
      Positioned(left: 0, right: 0, bottom: 0, height: k, child: const ColoredBox(color: Color(0xFF8E8E93))),
    ]);
  });
}

class SheetScreen extends StatefulWidget {
  const SheetScreen({super.key, required this.engine});
  final Engine engine;

  @override
  State<SheetScreen> createState() => _SheetScreenState();
}

class _SheetScreenState extends State<SheetScreen> {
  Engine get e => widget.engine;
  final _sheet = GlobalKey<SheetViewState>();

  Map<String, dynamic> _status = {};
  int _rev = 0;

  /// The message pill's text (the engine's message, shown for its 4 seconds).
  String? _message;
  Timer? _messageTimer;
  Timer? _busyTimer;

  /// The toolbar title's place (the title popover hangs under it).
  Rect? _titleAt;

  /// The phone layout last frame (null before the first); the first frame.
  bool? _wasCompact;
  bool _firstFrame = true;
  Map<String, dynamic> _chrome = {};
  List<dynamic> _menuData = [];

  @override
  void initState() {
    super.initState();
    _read();
    _typeHook = (t) {
      // into a focused text field (the cell editor's), else the sheet
      final field = FocusManager.instance.primaryFocus?.context?.findAncestorStateOfType<EditableTextState>();
      if (field == null) return _sheet.currentState?.typeForTest(t);
      final v = field.textEditingValue.text + t;
      field.userUpdateTextEditingValue(TextEditingValue(text: v, selection: TextSelection.collapsed(offset: v.length)), SelectionChangedCause.keyboard);
    };
    _keyHook = (k) {
      if (k.startsWith('menu:')) {
        _run(k.substring(5));
        return;
      }
      // an engine action ("key action:tbl:select:0:0:2:2")
      if (k.startsWith('action:')) {
        e.action(k.substring(7).replaceAll(r'\n', '\n'));
        _refreshAll();
        return;
      }
      // an inspector or window control ("key insp:organize.filter.add=1")
      if (k.startsWith('insp:')) {
        final eq = k.indexOf('=');
        e.inspectorSet(k.substring(5, eq), jsonDecode(k.substring(eq + 1)));
        _refreshAll();
        return;
      }
      if (k.startsWith('open:')) {
        final err = e.open(k.substring(5));
        if (err != null) _toast("Couldn't open it: $err");
        _refreshAll();
        return;
      }
      if (k.startsWith('save:')) {
        final err = e.save(k.substring(5));
        if (err != null) _toast("Couldn't save: $err");
        _refreshAll();
        return;
      }
      // (a key in the cell editor: Return / Tab / Escape as its own keys do)
      final field = FocusManager.instance.primaryFocus?.context?.findAncestorStateOfType<EditableTextState>();
      if (field != null && (k == 'Enter' || k == 'Tab' || k == 'Escape')) {
        e.action(switch (k) { 'Enter' => 'tbl:commitDown', 'Tab' => 'tbl:commitTab:0', _ => 'tbl:cancel' });
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
  }

  /// After the sheet, selection or cell changed: the chrome and menus follow, what was copied
  /// goes to the clipboard, a message shows.
  void _changed() {
    final before = _status['message'];
    final copied = e.copied();
    if (copied != null) Clipboard.setData(ClipboardData(text: copied));
    setState(() {
      _read();
      _rev++;
    });
    // something finishing in the background (a movie read, a web video's lookup, stock quotes,
    // a recording's clock)
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

  void _refreshAll() => _sheet.currentState?.refresh();

  /// A web page in the system's browser (Android: the app's channel; Linux: xdg-open).
  Future<void> _openUrl(String url) async {
    if (Platform.isAndroid) {
      await const MethodChannel('app.bramble.values/open').invokeMethod('url', url);
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

  /// Saves (or exports) to a file of the app's, then hands it to the system's file picker;
  /// true when it was saved.
  Future<bool> _saveAs(String ext, {bool export = false}) async {
    final dir = await getTemporaryDirectory();
    final title = (_status['title'] as String? ?? 'Untitled').replaceAll(RegExp(r'\.(numbers|xlsx|xlsm|csv|tsv|txt)$'), '');
    final out = '${dir.path}/$title.$ext';
    final f = File(out);
    if (f.existsSync()) f.deleteSync();
    final err = export ? e.export(out) : e.save(out);
    if (err != null) {
      _toast("Couldn't save: $err");
      return false;
    }
    if (!f.existsSync()) {
      _toast("Couldn't save");
      return false;
    }
    final saved = await FilePicker.saveFile(fileName: '$title.$ext', bytes: await f.readAsBytes());
    if (saved != null) _toast('Saved');
    _refreshAll();
    return saved != null;
  }

  /// Print Setup's Print / Export PDF…: on Android the sheet as a PDF through the system's print
  /// service; elsewhere the desktop's printer (the engine's).
  Future<void> _print(bool export) async {
    if (export) {
      await _saveAs('pdf', export: true);
      return;
    }
    if (!Platform.isAndroid) {
      e.inspectorSet('dlg.printSetup.print', null);
      _refreshAll();
      return;
    }
    final dir = await getTemporaryDirectory();
    final title = (_status['title'] as String? ?? 'Untitled').replaceAll(RegExp(r'\.(numbers|xlsx|xlsm|csv|tsv|txt)$'), '');
    final out = '${dir.path}/$title.pdf';
    final err = e.export(out);
    if (err != null) return _toast("Couldn't print: $err");
    await const MethodChannel('app.bramble.values/open').invokeMethod('print', [out, title]);
  }

  /// File > Close and Quit: unsaved changes asked about first (Don't Save / Cancel / Save).
  Future<bool> _leave() async {
    if (_status['edited'] != true) return true;
    final look = Look(_chrome);
    final choice = await showDialog<String>(
      context: context,
      builder: (ctx) => AlertDialog(
        backgroundColor: look.panel,
        title: Text('Unsaved changes', style: Look.font(17, look.text, bold: true)),
        content: Text('Do you want to save the changes to \u201c${_status['title']}\u201d?', style: Look.font(15, look.text)),
        actions: [
          TextButton(onPressed: () => Navigator.of(ctx).pop('dont'), child: Text("Don't Save", style: Look.font(15, look.accent))),
          TextButton(onPressed: () => Navigator.of(ctx).pop(), child: Text('Cancel', style: Look.font(15, look.accent))),
          TextButton(onPressed: () => Navigator.of(ctx).pop('save'), child: Text('Save', style: Look.font(15, look.accent, bold: true))),
        ],
      ),
    );
    if (choice == 'dont') return true;
    if (choice == 'save') return _saveAs(_status['excel'] == true ? 'xlsx' : 'numbers');
    return false;
  }

  Future<List<String>> _pick(FileType t) async => [for (final f in await FilePicker.pickFiles(type: t)) if (f.path != null) f.path!];

  /// A menu command: the ones that need Android's file picker or the view's zoom here, the rest
  /// as on the desktop.
  Future<void> _run(String cmd) async {
    final sv = _sheet.currentState;
    switch (cmd) {
      case 'Open…':
        return _open();
      case 'Save' || 'Save As…' || 'Duplicate':
        await _saveAs(_status['excel'] == true ? 'xlsx' : 'numbers');
        return;
      case 'Close' || 'Close All':
        if (await _leave()) e.action('close');
        return _refreshAll();
      case 'Quit Values' || 'Quit and Keep Windows':
        if (await _leave()) {
          if (Platform.isAndroid) {
            await SystemNavigator.pop();
          } else {
            exit(0);
          }
        }
        return;
      // (the desktop's own file dialog and its ~/Pictures, ~/Music: the system's picker here)
      case 'Choose File':
        for (final p in await _pick(FileType.any)) {
          e.action('insert:$p');
        }
        return _refreshAll();
      case 'Image Gallery':
        final ps = await _pick(FileType.image);
        if (ps.isNotEmpty) e.action('gallery:${ps.join('\n')}');
        return _refreshAll();
      case 'Show Photo and Video Browser' when Platform.isAndroid:
        for (final p in await _pick(FileType.media)) {
          e.action('insert:$p');
        }
        return _refreshAll();
      case 'Show Music Browser' when Platform.isAndroid:
        for (final p in await _pick(FileType.audio)) {
          e.action('insert:$p');
        }
        return _refreshAll();
      // (a copy where the system's picker says: Android's files are the app's copies)
      case 'Move To…':
        await _saveAs(_status['excel'] == true ? 'xlsx' : 'numbers');
        return;
      case 'Export To > PDF…':
        await _saveAs('pdf', export: true);
        return;
      case 'Export To > Excel…':
        await _saveAs('xlsx');
        return;
      case 'Export To > CSV…':
        await _saveAs('csv', export: true);
        return;
      case 'Export To > TSV…':
        await _saveAs('tsv', export: true);
        return;
      case 'Print Without Preview…':
        return _print(false);
      case 'Paste' || 'Paste and Match Style':
        final c = await Clipboard.getData(Clipboard.kTextPlain);
        if (c?.text != null) e.paste(c!.text!);
        return _refreshAll();
      case 'Zoom > Zoom In':
        if (sv != null) sv.setZoom(_nextZoom(sv.zoom, 1));
        return;
      case 'Zoom > Zoom Out':
        if (sv != null) sv.setZoom(_nextZoom(sv.zoom, -1));
        return;
      case 'Zoom > Actual Size':
        sv?.setZoom(1);
        return;
    }
    if (cmd.startsWith('open:')) {
      final err = e.open(cmd.substring(5));
      if (err != null) _toast("Couldn't open it: $err");
    } else {
      e.run(cmd);
    }
    _refreshAll();
  }

  // (chrome.rs ZOOMS)
  static const _zooms = [0.25, 0.5, 0.75, 1.0, 1.25, 1.5, 2.0, 3.0, 4.0];
  double _nextZoom(double z, int dir) {
    if (dir > 0) return _zooms.firstWhere((v) => v > z + 1e-3, orElse: () => 4);
    return _zooms.lastWhere((v) => v < z - 1e-3, orElse: () => 0.25);
  }

  // ---- toolbar ----

  void _toolbarButton(String id, Rect at) {
    switch (id) {
      case '__title':
        _titleAt = at;
        e.inspectorSet('dlg.docTitle.open', null);
        return _refreshAll();
      case 'Zoom':
        _zoomMenu(at);
      case 'View' || 'Insert' || 'Media':
        _sheet.currentState?.hideKeyboard();
        showTouchMenu(context, Look(_chrome), at, e.popover(id) as List? ?? [], (i) => _popAction(i['action'] as String? ?? ''), centered: true);
      case 'Table':
        _popAt((ctx) => showTablePopover(ctx, at));
      case 'Shape':
        _popAt((ctx) => showShapePopover(ctx, at, (e.popover('Shape') as Map<String, dynamic>?) ?? {}));
      case 'Chart':
        _popAt((ctx) => showChartPopover(ctx, at, (e.popover('Chart') as Map<String, dynamic>?) ?? {}));
      default:
        _sheet.currentState?.hideKeyboard();
        e.toolbar(id);
        _refreshAll();
    }
  }

  /// chrome.rs zoom_menu: the zooms (the current one ticked), under the Zoom item.
  Future<void> _zoomMenu(Rect at) async {
    final zoom = _sheet.currentState?.zoom ?? 1;
    await showTouchMenu(context, Look(_chrome), at, [
      for (final z in _zooms) {'label': '${(z * 100).round()}%', 'action': '$z', 'checked': (z - zoom).abs() < 0.001, 'enabled': true},
      {'separator': true},
      {'label': 'Zoom to Selection', 'action': 'sel', 'enabled': true},
    ], (i) {
      final v = i['action'] as String;
      if (v == 'sel') {
        e.run('Zoom > Zoom to Selection');
        _refreshAll();
      } else {
        _sheet.currentState?.setZoom(double.parse(v));
      }
    }, centered: true);
  }

  // ---- popovers ----

  final _popCtx = GlobalKey();

  Future<void> _popAt(Future<String?> Function(BuildContext ctx) show) async {
    _sheet.currentState?.hideKeyboard();
    final ctx = _popCtx.currentContext;
    if (ctx == null) return;
    final a = await show(ctx);
    if (a != null) await _popAction(a);
  }

  /// A menu of these items at a point (global).
  void _menuAt(List<dynamic> items, Offset at) {
    showTouchMenu(context, Look(_chrome), Rect.fromLTWH(at.dx, at.dy, 0, 0), items, (i) => _popAction(i['action'] as String? ?? ''));
  }

  /// A popover or menu item's action: picking files here first for the media items, a name
  /// asked for a sheet's rename.
  Future<void> _popAction(String a) async {
    switch (a) {
      case 'media:Choose…':
        return _run('Choose File');
      case 'media:Image Gallery':
        return _run('Image Gallery');
      case 'tbcustomize':
        e.inspectorSet('dlg.customize.open', null);
      default:
        if (a.startsWith('rename:')) {
          final i = int.parse(a.substring(7));
          final sheets = (_status['sheets'] as Map?)?['sheets'] as List? ?? [];
          final name = await _askName('Rename Sheet', i < sheets.length ? sheets[i]['name'] as String : '');
          if (name != null) e.action('sheet:rename:$i:$name');
          break;
        }
        if (a.startsWith('tbmode:')) {
          e.inspectorSet('dlg.customize.mode', int.parse(a.substring(7)));
          break;
        }
        if (a.startsWith('cmd:')) return _run(a.substring(4));
        e.action(a);
    }
    _refreshAll();
  }

  Future<String?> _askName(String title, String now) {
    final c = TextEditingController(text: now)..selection = TextSelection(baseOffset: 0, extentOffset: now.length);
    final look = Look(_chrome);
    return showDialog<String>(
      context: context,
      builder: (ctx) => AlertDialog(
        backgroundColor: look.panel,
        title: Text(title, style: Look.font(17, look.text, bold: true)),
        content: TextField(controller: c, autofocus: true, style: Look.font(16, look.text), onSubmitted: (v) => Navigator.of(ctx).pop(v)),
        actions: [
          TextButton(onPressed: () => Navigator.of(ctx).pop(), child: Text('Cancel', style: Look.font(15, look.accent))),
          TextButton(onPressed: () => Navigator.of(ctx).pop(c.text), child: Text('Rename', style: Look.font(15, look.accent, bold: true))),
        ],
      ),
    );
  }

  Widget _toolbar(List<dynamic> windows) => DesktopToolbar(
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
          {'label': 'Customize Toolbar…', 'action': 'tbcustomize', 'enabled': true},
        ], at.bottomLeft),
      );

  @override
  Widget build(BuildContext context) {
    final look = Look(_chrome);
    final showInspector = _status['inspector'] == true;
    final tbH = _chrome['showToolbar'] == false ? 0.0 : DesktopToolbar.height(_chrome);
    final windows = _status['windows'] as List? ?? const [];
    final chooser = windows.where((w) => w['id'] == 'chooser').firstOrNull as Map<String, dynamic>?;
    // the classic look: the toolbar in a strip of its own; otherwise it floats over the top of
    // the panes, on the header's colour (Values: nothing of the sheet shows under it)
    final classic = look.classic;
    final stripH = classic ? tbH : 0.0;
    final floatH = classic ? 0.0 : tbH;
    // the phone layout (Settings > Layout; Automatic: a phone's screen, under 600 points on its
    // short side — the screen's own size, before Settings > Scaling; an unfolded foldable or a
    // tablet keeps the side pane)
    final layout = (_chrome['uiLayout'] as num?)?.toInt() ?? 0;
    final view = View.of(context);
    final compact = layout == 1 || (layout == 0 && _chrome['android'] == true && (view.physicalSize / view.devicePixelRatio).shortestSide < 600);
    if (compact != _wasCompact) {
      final into = compact && _wasCompact != null;
      _wasCompact = compact;
      // into the phone form: the inspector closes (kn_ui form switched)
      if (into || (compact && _firstFrame)) {
        WidgetsBinding.instance.addPostFrameCallback((_) {
          if (_status['inspector'] == true) e.run('Inspector > Hide Inspector');
          _refreshAll();
        });
      }
    }
    _firstFrame = false;
    final mq = MediaQuery.of(context);
    final keyboardUp = mq.viewInsets.bottom > 0;
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
                if (stripH > 0) Container(height: stripH, color: look.header, child: _toolbar(windows)),
                Expanded(
                  child: Stack(children: [
                    Row(crossAxisAlignment: CrossAxisAlignment.stretch, children: [
                      Expanded(
                        child: Column(crossAxisAlignment: CrossAxisAlignment.stretch, children: [
                          // the toolbar's room
                          if (floatH > 0) Container(height: floatH, color: look.header),
                          SheetTabs(engine: e, s: (_status['sheets'] as Map<String, dynamic>?) ?? const {}, look: look, onChanged: _refreshAll, onMenu: _menuAt),
                          Expanded(
                            child: SheetView(key: _sheet, engine: e, look: look, message: compact ? null : _message, compact: compact, onChanged: _changed, onMenuAction: _popAction, onMenu: _menuAt),
                          ),
                          // the find bar: under the sheet, just above the keyboard (Numbers on iPad and iPhone)
                          if (_status['find'] != null) FindBar(engine: e, s: _status['find'] as Map<String, dynamic>, onChanged: _refreshAll, onMenu: _menuAt, compact: compact),
                          // the cell bar (under the keyboard while it's up: the formula bar is there)
                          if (!keyboardUp)
                            Padding(
                              padding: EdgeInsets.only(bottom: mq.padding.bottom),
                              child: CellBar(s: _status['cellBar'] as Map<String, dynamic>?, look: look, message: compact ? _message : null, onEdit: () {
                                e.action('tbl:edit:CUR');
                                _refreshAll();
                              }),
                            ),
                        ]),
                      ),
                      // (it stays built while it slides out)
                      if (!compact) SlidePane(open: showInspector, width: 300, child: Inspector(engine: e, look: look, topInset: floatH, revision: _rev, onChanged: _refreshAll)),
                    ]),
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
                    // the template chooser over the sheet, below the toolbar (welcome.rs overlay)
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
                    if (floatH > 0) Positioned(top: 0, left: 0, right: 0, height: floatH, child: _toolbar(windows)),
                  ]),
                ),
              ]),
              // the desktop's windows (dialogs) over everything
              Positioned.fill(child: Windows(engine: e, windows: [for (final w in windows) if (w['id'] != 'chooser') w], titleAt: _titleAt, toolbarBottom: DesktopMenuBar.height + tbH, compact: compact, savedColors: _status['savedColors'] as String? ?? '', onChanged: _refreshAll, onPrint: _print, onTitleFormat: (f) => _saveAs(f == 2 ? 'numbers' : 'xlsx'))),
            ]),
          ),
        ),
      ),
    );
  }
}
