// Sync with a WebDAV server (Nextcloud, Koofr, a NAS), the desktop's Settings > Sync and its
// timing (nt-app sync.rs): while it's on, the library syncs when the app starts or comes back,
// 5 s after the last change and every minute, on a worker isolate (nt-ffi runs it with a Store of
// its own), and the library is read again when the sync brought anything in. The same account
// and folder as the desktop's, so the phone and the computers share their notes.
import 'dart:async';
import 'dart:io';

import 'package:flutter/cupertino.dart';
import 'package:flutter/material.dart';
import 'package:flutter/rendering.dart';
import 'package:flutter/services.dart';

import 'state.dart';
import 'theme.dart';
import 'ui/dialogs.dart';
import 'ui/glass.dart';
import 'ui/sheet.dart';
import 'ui/touch_menu.dart';

/// A sync message as the person should read it: nt-core words its errors as format errors
/// ("unreadable notes data: Couldn’t reach the service…"), which a network problem isn't.
String syncMessage(String m) => m.replaceAll('unreadable notes data: ', '');

class SyncRunner extends ChangeNotifier with WidgetsBindingObserver {
  SyncRunner(this.s);
  final AppState s;

  static const _afterChange = Duration(seconds: 5), _every = Duration(seconds: 60);

  bool running = false;

  /// How far the running sync is (nt-ffi sync_progress: message, fraction, item), polled every
  /// 250 ms while it runs; null when none is.
  Map<String, dynamic>? progress;
  Timer? _poll;
  DateTime? _changedAt, _startedAt;
  Timer? _tick;

  /// The account and the last outcome (nt-ffi sync_get).
  Map<String, dynamic> get account => (s.call('sync_get') as Map?)?.cast<String, dynamic>() ?? const {};

  void start() {
    WidgetsBinding.instance.addObserver(this);
    _tick = Timer.periodic(const Duration(seconds: 1), (_) => _due());
  }

  @override
  void dispose() {
    _tick?.cancel();
    _poll?.cancel();
    WidgetsBinding.instance.removeObserver(this);
    super.dispose();
  }

  /// The account was saved (Settings' row follows it).
  void saved() => notifyListeners();

  /// The library changed here: sync a little later.
  void changed() => _changedAt = DateTime.now();

  @override
  void didChangeAppLifecycleState(AppLifecycleState st) {
    // (back in the app: what changed elsewhere comes in straight away)
    if (st == AppLifecycleState.resumed) _startedAt = null;
    // (leaving it: what was typed goes out)
    if (st == AppLifecycleState.paused && _changedAt != null && (_startedAt == null || _changedAt!.isAfter(_startedAt!))) now();
  }

  void _due() {
    if (running) return;
    final a = account;
    if (a['on'] != true || a['ready'] != true) return;
    final t = DateTime.now();
    final st = _startedAt;
    final due = st == null || (_changedAt != null && _changedAt!.isAfter(st) && t.difference(_changedAt!) >= _afterChange) || t.difference(st) >= _every;
    if (due) now();
  }

  /// Sync Now (or Replace Server / Replace Notes Here: force "push" / "pull").
  Future<void> now({String force = ''}) async {
    if (running) return;
    s.beforeSync?.call();
    running = true;
    _startedAt = DateTime.now();
    notifyListeners();
    _awake(true);
    _native('syncStart', 'Syncing…');
    var told = DateTime(0);
    _poll = Timer.periodic(const Duration(milliseconds: 250), (_) {
      final p = (s.call('sync_progress') as Map?)?.cast<String, dynamic>();
      if (p == null && progress == null) return;
      progress = p;
      notifyListeners();
      // (the notification: about once a second)
      final now = DateTime.now();
      if (p != null && now.difference(told) >= const Duration(seconds: 1)) {
        told = now;
        _native('syncUpdate', {'text': p['message'], 'done': p['done'], 'total': p['total']});
      }
    });
    try {
      final r = await s.engine.run('sync_now', {'force': force}) as Map?;
      if (r != null && ['received', 'removed', 'copies', 'merged', 'failed'].any((k) => (r[k] as num? ?? 0) > 0)) {
        // (the worker's Store wrote the note files: this one reads them in)
        s.call('refresh');
        await s.reload(fromSync: true);
        s.syncedIn.value++;
      }
    } catch (_) {
      // (the outcome is in the status nt-core keeps; Settings shows it)
    }
    _poll?.cancel();
    progress = null;
    running = false;
    _awake(false);
    _native('syncStop');
    notifyListeners();
  }

  static const _system = MethodChannel('app.bramble.notes/system');

  /// While a sync runs the screen stays on, so a long first sync isn't cut off by the phone
  /// going to sleep while Notes is open.
  void _awake(bool on) => _native('keepAwake', on);

  /// The sync's foreground service and its notification (SyncService.kt): it goes on with the
  /// screen off or Notes in the background.
  void _native(String method, [Object? args]) {
    if (Platform.isAndroid) _system.invokeMethod(method, args).ignore();
  }

  Future<String?> test() async {
    final r = await s.engine.run('sync_test') as Map?;
    final p = r?['problem'] as String?;
    return p == null ? null : syncMessage(p);
  }
}

/// While a sync runs: what it's doing ("Uploading 312 of 1,418 · 210 MB of 812 MB"), a slim
/// bar, and (`item`) the note or file going now. It glides in (its height from nothing, with a
/// fade) once a sync has gone on 300 ms, so a quick one doesn't flicker; when the sync ends it
/// shows the bar full for 0.8 s, then glides away. Scrolled down a list, it keeps what's on
/// screen where it is while it opens or closes above.
class SyncProgress extends StatefulWidget {
  const SyncProgress({super.key, required this.sync, this.item = false, this.padding = const EdgeInsets.fromLTRB(16, 10, 16, 12)});
  final SyncRunner? sync;
  final bool item;
  final EdgeInsets padding;

  @override
  State<SyncProgress> createState() => _SyncProgressState();
}

class _SyncProgressState extends State<SyncProgress> with SingleTickerProviderStateMixin {
  late final _open = AnimationController(vsync: this, duration: const Duration(milliseconds: 280), reverseDuration: const Duration(milliseconds: 250));
  late final _size = CurvedAnimation(parent: _open, curve: Curves.easeOutCubic, reverseCurve: Curves.easeInCubic);
  Timer? _wait;

  /// The sync just ended: the bar shows full until the row goes.
  bool _finished = false;
  Map<String, dynamic>? _last;

  SyncRunner? get _sync => widget.sync;

  @override
  void initState() {
    super.initState();
    _sync?.addListener(_changed);
    // (already syncing: there at once)
    if (_sync?.running == true) _open.value = 1;
  }

  @override
  void didUpdateWidget(SyncProgress old) {
    super.didUpdateWidget(old);
    if (old.sync != widget.sync) {
      old.sync?.removeListener(_changed);
      widget.sync?.addListener(_changed);
    }
  }

  @override
  void dispose() {
    _sync?.removeListener(_changed);
    _wait?.cancel();
    _open.dispose();
    super.dispose();
  }

  void _changed() {
    final r = _sync;
    if (r == null || !mounted) return;
    if (r.running) {
      _finished = false;
      if (r.progress != null) _last = r.progress;
      if (_open.value == 0 && !_open.isAnimating && _wait == null) {
        _wait = Timer(const Duration(milliseconds: 300), () {
          _wait = null;
          if (mounted && r.running) _open.forward();
        });
      } else if (_open.status == AnimationStatus.reverse) {
        _open.forward();
      }
    } else {
      _wait?.cancel();
      _wait = null;
      if (_open.value > 0 && !_finished) {
        _finished = true;
        Future<void>.delayed(const Duration(milliseconds: 800), () {
          if (mounted && !(_sync?.running ?? false)) _open.reverse();
        });
      }
    }
    setState(() {});
  }

  @override
  Widget build(BuildContext context) {
    final p = Palette.of(context);
    final pr = _sync?.running == true ? (_sync?.progress ?? _last) : _last;
    final f = _finished ? 1.0 : (pr?['fraction'] as num?)?.toDouble();
    final what = _finished ? '' : (pr?['item'] as String? ?? '');
    final message = _finished ? 'Up to date' : (pr?['message'] as String? ?? 'Syncing…');
    return _KeepScroll(
      position: Scrollable.maybeOf(context)?.position,
      child: SizeTransition(
        sizeFactor: _size,
        alignment: Alignment.topCenter,
        child: FadeTransition(
          opacity: _size,
          child: RepaintBoundary(
            child: Padding(
              padding: widget.padding,
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.stretch,
                mainAxisSize: MainAxisSize.min,
                children: [
                  Text(message, maxLines: 1, overflow: TextOverflow.ellipsis, style: Type.footnote(p.secondary)),
                  const SizedBox(height: 6),
                  ClipRRect(
                    borderRadius: BorderRadius.circular(2),
                    child: SizedBox(
                      height: 4,
                      // (no fraction yet, Checking: the bar runs to and fro)
                      child: LinearProgressIndicator(value: f == null || f <= 0 ? null : f, backgroundColor: p.fill, color: p.accent, minHeight: 4),
                    ),
                  ),
                  if (widget.item && what.isNotEmpty) ...[
                    const SizedBox(height: 6),
                    Text(what, maxLines: 1, overflow: TextOverflow.ellipsis, style: Type.caption(p.tertiary)),
                  ],
                ],
              ),
            ),
          ),
        ),
      ),
    );
  }
}

/// While the list is scrolled down, a change in this box's height (the progress row opening or
/// closing above what's on screen) is taken off the scroll position in the same layout, so the
/// notes in view don't move. At the top, the list below glides down and up with it.
class _KeepScroll extends SingleChildRenderObjectWidget {
  const _KeepScroll({required this.position, required super.child});
  final ScrollPosition? position;

  @override
  RenderObject createRenderObject(BuildContext context) => _RenderKeepScroll()..position = position;

  @override
  void updateRenderObject(BuildContext context, _RenderKeepScroll r) => r.position = position;
}

class _RenderKeepScroll extends RenderProxyBox {
  ScrollPosition? position;
  double? _height;

  @override
  void performLayout() {
    super.performLayout();
    final h = size.height, was = _height, pos = position;
    _height = h;
    if (was != null && pos != null && h != was && pos.hasPixels && pos.pixels > 0.5) pos.correctBy(h - was);
  }
}

void showSyncSettings(BuildContext context, AppState s) => showSheet<void>(
  context,
  title: 'Sync',
  builder: (ctx) => _SyncPage(state: s),
);

class _SyncPage extends StatefulWidget {
  const _SyncPage({required this.state});
  final AppState state;
  @override
  State<_SyncPage> createState() => _SyncPageState();
}

class _SyncPageState extends State<_SyncPage> {
  AppState get s => widget.state;
  SyncRunner get sync => s.sync!;
  late final Map<String, dynamic> _a = sync.account;
  late bool _on = _a['on'] == true;
  late final _url = TextEditingController(text: _a['url'] as String? ?? '');
  late final _user = TextEditingController(text: _a['user'] as String? ?? '');
  late final _password = TextEditingController(text: _a['password'] as String? ?? '');
  late final _folder = TextEditingController(text: _a['folder'] as String? ?? 'Bramble Notes');
  bool _testing = false;

  /// Test Connection's answer: null (not asked), '' (it works) or the problem.
  String? _tested;

  @override
  void initState() {
    super.initState();
    sync.addListener(_refresh);
  }

  @override
  void dispose() {
    sync.removeListener(_refresh);
    _save();
    for (final c in [_url, _user, _password, _folder]) {
      c.dispose();
    }
    super.dispose();
  }

  void _refresh() {
    if (mounted) setState(() {});
  }

  void _save() {
    s.call('sync_set', {'on': _on, 'url': _url.text, 'user': _user.text, 'password': _password.text, 'folder': _folder.text});
    sync.saved();
  }

  bool get _ready => _url.text.trim().isNotEmpty && _user.text.trim().isNotEmpty;

  Widget _field(Palette p, String label, TextEditingController c, String hint, {bool secure = false, TextInputType? type}) => SizedBox(
    height: 52,
    child: Padding(
      padding: const EdgeInsets.symmetric(horizontal: 16),
      child: Row(
        children: [
          SizedBox(width: 104, child: Text(label, style: Type.body(p.label))),
          Expanded(
            child: TextField(
              controller: c,
              enabled: _on,
              obscureText: secure,
              autocorrect: false,
              enableSuggestions: false,
              keyboardType: type,
              style: Type.body(p.label),
              cursorColor: p.accent,
              onChanged: (_) => setState(() {
                _tested = null;
                _save();
              }),
              decoration: InputDecoration.collapsed(hintText: hint, hintStyle: Type.body(p.tertiary)),
            ),
          ),
        ],
      ),
    ),
  );

  String _last() {
    final a = sync.account;
    final last = (a['last'] as num?)?.toDouble();
    final when = last == null ? 'Never' : _date(DateTime.fromMillisecondsSinceEpoch((last * 1000).round()));
    if (sync.running) return (sync.progress?['message'] as String?) ?? 'Syncing…';
    final sum = a['summary'] as String? ?? '';
    return sum.isEmpty ? when : '$when — ${syncMessage(sum)}';
  }

  static String _date(DateTime d) {
    const m = ['January', 'February', 'March', 'April', 'May', 'June', 'July', 'August', 'September', 'October', 'November', 'December'];
    final h = d.hour % 12 == 0 ? 12 : d.hour % 12;
    return '${m[d.month - 1]} ${d.day}, ${d.year} at $h:${d.minute.toString().padLeft(2, '0')} ${d.hour < 12 ? 'AM' : 'PM'}';
  }

  Future<void> _replace(bool server) async {
    final ok = await confirm(
      context,
      server ? 'Replace Server?' : 'Replace Notes Here?',
      message: server
          ? 'The notes on the server are replaced with the notes on this phone. Other devices get these notes at their next sync.'
          : 'The notes on this phone are replaced with the notes on the server.',
      ok: 'Replace',
    );
    if (ok) sync.now(force: server ? 'push' : 'pull');
  }

  @override
  Widget build(BuildContext context) {
    final p = Palette.of(context);
    final failed = sync.account['failed'] == true;
    return ListView(
      padding: const EdgeInsets.only(top: 4, bottom: 24),
      children: [
        Card2(
          footer: 'Nextcloud, Koofr, a NAS or any WebDAV folder. Your other computers and phones sync with the same folder.',
          children: [
            CardRow(
              label: 'Sync with a WebDAV Server',
              trailing: CupertinoSwitch(
                value: _on,
                activeTrackColor: const Color(0xFF34C759),
                onChanged: (v) => setState(() {
                  _on = v;
                  _save();
                  // (straight away only once there's an address to sync with)
                  if (v && _ready) sync.now();
                }),
              ),
            ),
          ],
        ),
        Card2(
          header: 'Account',
          children: [
            Builder(
              builder: (bctx) => CardRow(
                label: 'Fill In the Address For…',
                chevron: true,
                onTap: !_on
                    ? null
                    : () => showTouchMenu(
                        context,
                        p.look,
                        globalRect(bctx),
                        [
                          for (final pr in (_a['presets'] as List? ?? const [])) {'label': (pr as List)[0], 'url': pr[1]},
                        ],
                        (i) => setState(() {
                          _url.text = i['url'] as String;
                          _save();
                        }),
                      ),
              ),
            ),
            _field(p, 'Address', _url, 'https://…', type: TextInputType.url),
            _field(p, 'User Name', _user, ''),
            _field(p, 'Password', _password, 'an app password, if the service has them', secure: true),
            _field(p, 'Folder', _folder, 'Bramble Notes'),
          ],
        ),
        Card2(
          footer: _tested == null ? null : (_tested!.isEmpty ? 'The server answered: it works.' : _tested),
          children: [
            CardRow(
              label: _testing ? 'Testing…' : 'Test Connection',
              color: p.accent,
              onTap: !_on || !_ready || _testing
                  ? null
                  : () async {
                      _save();
                      setState(() => _testing = true);
                      final r = await sync.test();
                      if (mounted) {
                        setState(() {
                          _testing = false;
                          _tested = r ?? '';
                        });
                      }
                    },
            ),
            CardRow(label: 'Sync Now', color: p.accent, onTap: !_on || !_ready || sync.running ? null : () => sync.now()),
            SyncProgress(sync: sync, item: true),
          ],
        ),
        Card2(
          header: 'Last Sync',
          children: [
            Padding(
              padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 14),
              child: SizedBox(width: double.infinity, child: Text(_last(), style: Type.body(failed ? p.red : p.label))),
            ),
          ],
        ),
        Card2(
          header: 'Start Over',
          children: [
            CardRow(label: 'Replace Server…', color: p.red, onTap: !_on || !_ready || sync.running ? null : () => _replace(true)),
            CardRow(label: 'Replace Notes Here…', color: p.red, onTap: !_on || !_ready || sync.running ? null : () => _replace(false)),
          ],
        ),
      ],
    );
  }
}
