// Keydeck Remote (Keynote Remote): this phone or tablet controls a slideshow playing in Keydeck
// on a computer on the same network (kn-app remote.rs). The computers are found on their own (a
// UDP broadcast to port 51737, answered by every Keydeck with Settings > Remotes enabled); a
// computer not linked yet shows a four-digit code here and on its screen, and links when it's
// confirmed there. Linked, it plays, moves and ends the show; this shows the current and next
// slides, the presenter notes, a timer or the clock, the slides to jump to, and draws with a
// laser pointer or coloured highlights on the slide as it plays there.
import 'dart:async';
import 'dart:convert';
import 'dart:io';
import 'dart:math' as math;
import 'dart:typed_data';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:path_provider/path_provider.dart';

import '../desktop/look.dart';

const _discoveryPort = 51737;

/// A computer running Keydeck that answered.
class Desk {
  Desk(this.id, this.name, this.address, this.port);
  final String id, name;
  final InternetAddress address;
  final int port;
  DateTime seen = DateTime.now();
}

/// The remote's side of the wire: finding computers, the one connection, what it sends.
class RemoteLink extends ChangeNotifier {
  String deviceId = '', deviceName = 'Keydeck Remote';

  /// Linked computers: id -> {name, token}.
  Map<String, dynamic> links = {};
  final desks = <String, Desk>{};
  Desk? desk;
  Socket? _sock;
  RawDatagramSocket? _udp;
  Timer? _ask;
  File? _file;

  /// welcome, linked (this connection), the code shown for linking, the latest state, slides.
  bool connected = false, linked = false, disabled = false;
  String? code;
  Map<String, dynamic> state = {};
  Uint8List? current, next;
  List<Map<String, dynamic>> thumbs = [];
  DateTime? showStarted;

  Future<void> start() async {
    final dir = await getApplicationSupportDirectory();
    _file = File('${dir.path}/remote.json');
    try {
      final v = jsonDecode(await _file!.readAsString()) as Map<String, dynamic>;
      deviceId = v['id'] as String? ?? '';
      links = (v['links'] as Map<String, dynamic>?) ?? {};
    } catch (_) {}
    if (deviceId.isEmpty) {
      final r = math.Random.secure();
      deviceId = List.generate(16, (_) => r.nextInt(16).toRadixString(16)).join();
      _save();
    }
    try {
      deviceName = Platform.isAndroid ? (await const MethodChannel('app.bramble.keydeck/device').invokeMethod<String>('name') ?? deviceName) : Platform.localHostname;
    } catch (_) {}
    _udp = await RawDatagramSocket.bind(InternetAddress.anyIPv4, 0);
    _udp!.broadcastEnabled = true;
    _udp!.listen((ev) {
      if (ev != RawSocketEvent.read) return;
      final d = _udp!.receive();
      if (d == null) return;
      try {
        final v = jsonDecode(utf8.decode(d.data)) as Map<String, dynamic>;
        if (v['keydeck'] == null) return;
        final id = v['id'] as String;
        final known = desks[id];
        if (known == null) {
          desks[id] = Desk(id, v['name'] as String? ?? 'Keydeck', d.address, (v['port'] as num?)?.toInt() ?? 51738);
          // the computer this was linked to last, as soon as it's found
          if (desk == null && links.containsKey(id)) connect(desks[id]!);
          notifyListeners();
        } else {
          known.seen = DateTime.now();
        }
      } catch (_) {}
    });
    _ask = Timer.periodic(const Duration(seconds: 1), (_) => _broadcast());
    _broadcast();
  }

  Future<void> _save() async {
    await _file?.writeAsString(jsonEncode({'id': deviceId, 'links': links}));
  }

  /// "Is there a Keydeck?" to everyone on each network this is on.
  Future<void> _broadcast() async {
    final msg = utf8.encode('KDRM?${jsonEncode({'id': deviceId, 'name': deviceName})}');
    final targets = <InternetAddress>{InternetAddress('255.255.255.255')};
    try {
      for (final ni in await NetworkInterface.list(type: InternetAddressType.IPv4)) {
        for (final a in ni.addresses) {
          final p = a.rawAddress;
          targets.add(InternetAddress.fromRawAddress(Uint8List.fromList([p[0], p[1], p[2], 255])));
        }
      }
    } catch (_) {}
    for (final t in targets) {
      try {
        _udp?.send(msg, t, _discoveryPort);
      } catch (_) {}
    }
    // computers not heard from for a while go from the list (not the one in use)
    final stale = [for (final d in desks.values) if (d != desk && DateTime.now().difference(d.seen).inSeconds > 6) d.id];
    if (stale.isNotEmpty) {
      desks.removeWhere((k, _) => stale.contains(k));
      notifyListeners();
    }
  }

  Future<void> connect(Desk d) async {
    await _sock?.close();
    desk = d;
    connected = linked = disabled = false;
    code = null;
    state = {};
    current = next = null;
    thumbs = [];
    notifyListeners();
    try {
      final s = await Socket.connect(d.address, d.port, timeout: const Duration(seconds: 4));
      s.setOption(SocketOption.tcpNoDelay, true);
      _sock = s;
      send({'type': 'hello', 'id': deviceId, 'name': deviceName, 'token': (links[d.id] as Map?)?['token']});
      s.cast<List<int>>().transform(utf8.decoder).transform(const LineSplitter()).listen(_line, onDone: _closed, onError: (_) => _closed());
    } catch (_) {
      _closed();
    }
  }

  void _closed() {
    _sock = null;
    connected = false;
    linked = false;
    notifyListeners();
  }

  void send(Map<String, Object?> m) {
    try {
      _sock?.write('${jsonEncode(m)}\n');
    } catch (_) {}
  }

  void _line(String line) {
    final m = jsonDecode(line) as Map<String, dynamic>;
    switch (m['type']) {
      case 'welcome':
        connected = true;
        linked = m['linked'] == true;
        // linked before but no longer known there: forgotten here too
        if (!linked) links.remove(desk?.id);
      case 'disabled':
        disabled = true;
      case 'code':
        code = m['code'] as String?;
      case 'codeCancel':
        code = null;
      case 'linked':
        code = null;
        linked = true;
        if (desk != null) links[desk!.id] = {'name': desk!.name, 'token': m['token']};
        _save();
      case 'unlinked':
        linked = false;
        links.remove(desk?.id);
        _save();
      case 'state':
        final wasPlaying = state['playing'] == true;
        state = m;
        current = m['current'] == null ? null : base64Decode(m['current'] as String);
        next = m['next'] == null ? null : base64Decode(m['next'] as String);
        if (m['playing'] == true && !wasPlaying) showStarted = DateTime.now();
        if (m['playing'] != true) showStarted = null;
      case 'thumbs':
        thumbs = [for (final t in (m['slides'] as List)) t as Map<String, dynamic>];
    }
    notifyListeners();
  }

  void forget(String id) {
    links.remove(id);
    _save();
    notifyListeners();
  }

  void cmd(String c, [int? slide]) => send({'type': 'cmd', 'cmd': c, 'slide': ?slide});

  @override
  void dispose() {
    _ask?.cancel();
    _udp?.close();
    _sock?.destroy();
    super.dispose();
  }
}

/// The remote, full screen: the intro the first time, then the computers to choose from (and
/// linking), then the controls.
class RemoteScreen extends StatefulWidget {
  const RemoteScreen({super.key});

  @override
  State<RemoteScreen> createState() => _RemoteScreenState();
}

class _RemoteScreenState extends State<RemoteScreen> {
  final link = RemoteLink();
  bool _intro = true, _devices = false;

  @override
  void initState() {
    super.initState();
    link.addListener(() {
      if (mounted) setState(() {});
    });
    link.start().then((_) {
      if (mounted) setState(() => _intro = link.links.isEmpty);
    });
  }

  @override
  void dispose() {
    link.dispose();
    super.dispose();
  }

  static const _bg = Color(0xFF000000), _panel = Color(0xFF1C1C1E), _text = Color(0xFFFFFFFF), _dim = Color(0xFF98989F), _accent = Color(0xFF0A84FF);

  @override
  Widget build(BuildContext context) {
    final Widget body;
    if (_intro) {
      body = _introPage();
    } else if (_devices || link.desk == null || !link.connected || !link.linked) {
      body = _devicesPage();
    } else {
      body = _Controls(link: link, onDevices: () => setState(() => _devices = true), onClose: () => Navigator.of(context).pop());
    }
    return Scaffold(backgroundColor: _bg, body: SafeArea(child: body));
  }

  Widget _introPage() => Center(
        child: ConstrainedBox(
          constraints: const BoxConstraints(maxWidth: 440),
          child: Padding(
            padding: const EdgeInsets.all(28),
            child: Column(mainAxisSize: MainAxisSize.min, children: [
              const Icon(Icons.settings_remote_outlined, size: 84, color: _accent),
              const SizedBox(height: 22),
              Text('Keydeck Remote', style: Look.font(28, _text, bold: true)),
              const SizedBox(height: 14),
              Text('Use this device to control a slideshow playing in Keydeck on a computer on the same network: move through the slides, see the next slide and your presenter notes, and point or draw on the slide.', textAlign: TextAlign.center, style: Look.font(16, _dim)),
              const SizedBox(height: 28),
              _button('Continue', () => setState(() => _intro = false)),
              const SizedBox(height: 8),
              TextButton(onPressed: () => Navigator.of(context).pop(), child: Text('Not Now', style: Look.font(16, _accent))),
            ]),
          ),
        ),
      );

  Widget _button(String label, VoidCallback? onTap, {Color color = _accent}) => SizedBox(
        width: 260,
        height: 50,
        child: FilledButton(
          style: FilledButton.styleFrom(backgroundColor: color, shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(25))),
          onPressed: onTap,
          child: Text(label, style: Look.font(17, _text, bold: true)),
        ),
      );

  /// Manage Devices: the computers found (linked ones with a green dot, the one in use ticked)
  /// and, for one not linked, how to link it and its code.
  Widget _devicesPage() {
    final d = link.desk;
    final found = link.desks.values.toList()..sort((a, b) => a.name.compareTo(b.name));
    Widget status;
    if (d == null) {
      status = Text(found.isEmpty ? 'Looking for Keydeck on this network…\nOn the computer, open Keydeck, choose Keydeck > Settings, click Remotes, then select Enable.' : 'Choose the computer to control.', textAlign: TextAlign.center, style: Look.font(15, _dim));
    } else if (link.disabled) {
      status = Text('Remotes are turned off on “${d.name}”. In Keydeck there, choose Settings > Remotes, then select Enable.', textAlign: TextAlign.center, style: Look.font(15, _dim));
    } else if (!link.connected) {
      status = Text('Connecting to “${d.name}”…', textAlign: TextAlign.center, style: Look.font(15, _dim));
    } else if (link.code != null) {
      status = Column(children: [
        Text(link.code!.split('').join(' '), style: Look.font(46, _text, bold: true)),
        const SizedBox(height: 10),
        Text('Make sure this code matches the one on “${d.name}”, then click Confirm there.', textAlign: TextAlign.center, style: Look.font(15, _dim)),
      ]);
    } else if (!link.linked) {
      status = Text('On “${d.name}”, in Keydeck choose Settings > Remotes, then click Link next to “${link.deviceName}”.', textAlign: TextAlign.center, style: Look.font(15, _dim));
    } else {
      status = _button('Done', () => setState(() => _devices = false));
    }
    return Center(
      child: ConstrainedBox(
        constraints: const BoxConstraints(maxWidth: 520),
        child: Padding(
          padding: const EdgeInsets.all(20),
          child: Column(children: [
            Row(children: [
              IconButton(onPressed: () => Navigator.of(context).pop(), icon: const Icon(Icons.close, color: _text)),
              Expanded(child: Text('Keydeck Remote', textAlign: TextAlign.center, style: Look.font(18, _text, bold: true))),
              const SizedBox(width: 48),
            ]),
            const SizedBox(height: 16),
            Material(
              color: _panel,
              borderRadius: BorderRadius.circular(14),
              clipBehavior: Clip.antiAlias,
              child: Column(children: [
                for (final (i, x) in found.indexed) ...[
                  if (i > 0) const Divider(height: 1, color: Color(0xFF38383A), indent: 48),
                  ListTile(
                    leading: Icon(Icons.circle, size: 12, color: link.links.containsKey(x.id) ? const Color(0xFF34C759) : const Color(0xFF48484A)),
                    title: Text(x.name, style: Look.font(17, _text)),
                    trailing: x == d ? const Icon(Icons.check, color: _accent) : null,
                    onTap: () => link.connect(x),
                    onLongPress: link.links.containsKey(x.id) ? () => link.forget(x.id) : null,
                  ),
                ],
                if (found.isEmpty) const Padding(padding: EdgeInsets.all(22), child: SizedBox(width: 22, height: 22, child: CircularProgressIndicator(strokeWidth: 2, color: _dim))),
              ]),
            ),
            const SizedBox(height: 28),
            status,
          ]),
        ),
      ),
    );
  }
}

/// What the remote shows while linked: the layouts (current, next, notes), the timer, the slide
/// number (tap: the slides to jump to), Play, the drawing tools.
class _Controls extends StatefulWidget {
  const _Controls({required this.link, required this.onDevices, required this.onClose});
  final RemoteLink link;
  final VoidCallback onDevices, onClose;

  @override
  State<_Controls> createState() => _ControlsState();
}

/// Layouts (Keynote's): current + next, current, current + notes, next + notes, notes.
const _layouts = ['Current and Next', 'Current', 'Current and Notes', 'Next and Notes', 'Notes Only'];

class _ControlsState extends State<_Controls> {
  RemoteLink get link => widget.link;
  int _layout = 0;
  bool _clock = false, _draw = false, _invert = false, _laser = true;
  double _notesSize = 20;
  Color _color = const Color(0xFFFF3B30);
  int _markId = 0;
  List<Offset> _points = [];

  /// The highlights drawn on this slide (colour, points), as they show on the computer.
  final _marks = <(Color, List<Offset>)>[];
  String _shownAt = '';
  Timer? _tick;

  static const _colors = [Color(0xFFFF3B30), Color(0xFFFF9500), Color(0xFFFFCC00), Color(0xFF007AFF), Color(0xFF34C759), Color(0xFFAF52DE), Color(0xFFFFFFFF), Color(0xFF000000)];

  @override
  void initState() {
    super.initState();
    _tick = Timer.periodic(const Duration(seconds: 1), (_) {
      if (mounted) setState(() {});
    });
  }

  @override
  void dispose() {
    _tick?.cancel();
    super.dispose();
  }

  String _time() {
    if (_clock || link.showStarted == null) {
      final n = TimeOfDay.now();
      return '${n.hourOfPeriod == 0 ? 12 : n.hourOfPeriod}:${n.minute.toString().padLeft(2, '0')} ${n.period == DayPeriod.am ? 'AM' : 'PM'}';
    }
    final d = DateTime.now().difference(link.showStarted!);
    String two(int v) => v.toString().padLeft(2, '0');
    return '${two(d.inHours)}:${two(d.inMinutes % 60)}:${two(d.inSeconds % 60)}';
  }

  /// A slide's picture as large as fits (its label over it).
  Widget _pic(Uint8List? img, {String? label}) {
    final aspect = (link.state['aspect'] as num?)?.toDouble() ?? 9 / 16;
    return LayoutBuilder(builder: (ctx, box) {
      final room = box.maxHeight - (label == null ? 0 : 24);
      final w = math.min(box.maxWidth, room.isFinite ? room / aspect : box.maxWidth);
      return Column(mainAxisSize: MainAxisSize.min, crossAxisAlignment: CrossAxisAlignment.start, children: [
        if (label != null) SizedBox(height: 24, child: Text(label, style: Look.font(13, const Color(0xFF98989F)))),
        SizedBox(
          width: w,
          height: w * aspect,
          child: img == null
              ? Container(color: const Color(0xFF1C1C1E), alignment: Alignment.center, child: Text(label == 'Next' ? 'End of slideshow' : '', style: Look.font(15, const Color(0xFF98989F))))
              : Image.memory(img, gaplessPlayback: true, fit: BoxFit.contain),
        ),
      ]);
    });
  }

  Widget _notes() {
    final text = link.state['notes'] as String? ?? '';
    final fg = _invert ? Colors.black : Colors.white, bg = _invert ? Colors.white : const Color(0xFF1C1C1E);
    return Container(
      color: bg,
      padding: const EdgeInsets.all(16),
      child: SingleChildScrollView(child: Text(text.isEmpty ? 'No presenter notes' : text, style: Look.font(_notesSize, text.isEmpty ? fg.withValues(alpha: 0.5) : fg))),
    );
  }

  /// The slide area: a tap or a swipe to the left advances, a swipe to the right goes back,
  /// two fingers swiped down end the show.
  Widget _gestures(Widget child) {
    var twoDown = 0.0, travel = Offset.zero;
    var fingers = 0;
    return GestureDetector(
      behavior: HitTestBehavior.opaque,
      onTap: () => link.cmd('next'),
      onScaleStart: (d) {
        twoDown = 0;
        travel = Offset.zero;
        fingers = d.pointerCount;
      },
      onScaleUpdate: (d) {
        fingers = math.max(fingers, d.pointerCount);
        if (d.pointerCount >= 2) twoDown += d.focalPointDelta.dy;
        travel += d.focalPointDelta;
      },
      onScaleEnd: (d) {
        if (fingers >= 2) {
          if (twoDown > 120) link.cmd('stop');
          return;
        }
        final v = d.velocity.pixelsPerSecond.dx;
        if (travel.dx < -60 || v < -300) link.cmd('next');
        if (travel.dx > 60 || v > 300) link.cmd('prev');
      },
      child: child,
    );
  }

  Widget _layoutBody(BoxConstraints box) {
    final wide = box.maxWidth > box.maxHeight;
    final cur = _pic(link.current), nxt = _pic(link.next, label: 'Next');
    Widget split(Widget a, Widget b, {int fa = 3, int fb = 2}) => wide
        ? Row(crossAxisAlignment: CrossAxisAlignment.start, children: [Expanded(flex: fa, child: a), const SizedBox(width: 16), Expanded(flex: fb, child: b)])
        : Column(children: [Expanded(child: a), const SizedBox(height: 16), Expanded(child: b)]);
    return switch (_layout) {
      1 => _gestures(Center(child: cur)),
      2 => split(_gestures(cur), _notes(), fa: 1, fb: 1),
      3 => split(nxt, _notes(), fa: 1, fb: 1),
      4 => _gestures(_notes()),
      _ => wide ? split(_gestures(Align(alignment: Alignment.topCenter, child: cur)), Align(alignment: Alignment.topCenter, child: nxt)) : Column(children: [Expanded(flex: 3, child: _gestures(Center(child: cur))), const SizedBox(height: 16), Expanded(flex: 2, child: Center(child: nxt))]),
    };
  }

  Future<void> _pickSlide() async {
    link.send({'type': 'thumbs'});
    await showModalBottomSheet<void>(
      context: context,
      backgroundColor: const Color(0xFF1C1C1E),
      isScrollControlled: true,
      builder: (ctx) => ListenableBuilder(
        listenable: link,
        builder: (ctx, _) {
          final aspect = (link.state['aspect'] as num?)?.toDouble() ?? 9 / 16;
          var n = 0;
          return SizedBox(
            height: MediaQuery.of(ctx).size.height * 0.75,
            child: link.thumbs.isEmpty
                ? const Center(child: CircularProgressIndicator(color: Color(0xFF98989F)))
                : GridView.extent(
                    padding: const EdgeInsets.all(16),
                    maxCrossAxisExtent: 200,
                    mainAxisSpacing: 14,
                    crossAxisSpacing: 14,
                    childAspectRatio: 1 / aspect * 0.85,
                    children: [
                      for (final t in link.thumbs)
                        if (t['skipped'] != true)
                          GestureDetector(
                            onTap: () {
                              link.cmd('goto', t['index'] as int);
                              Navigator.of(ctx).pop();
                            },
                            child: Column(children: [
                              Expanded(child: t['image'] == null ? const SizedBox() : Image.memory(base64Decode(t['image'] as String), fit: BoxFit.contain)),
                              const SizedBox(height: 4),
                              Text('${++n}', style: Look.font(13, (t['index'] == link.state['slide']) ? const Color(0xFF0A84FF) : const Color(0xFF98989F))),
                            ]),
                          ),
                    ],
                  ),
          );
        },
      ),
    );
  }

  Future<void> _layoutMenu() async {
    await showModalBottomSheet<void>(
      context: context,
      backgroundColor: const Color(0xFF1C1C1E),
      builder: (ctx) => StatefulBuilder(
        builder: (ctx, set) => SafeArea(
          child: Column(mainAxisSize: MainAxisSize.min, children: [
            for (final (i, n) in _layouts.indexed)
              ListTile(
                title: Text(n, style: Look.font(17, Colors.white)),
                trailing: i == _layout ? const Icon(Icons.check, color: Color(0xFF0A84FF)) : null,
                onTap: () {
                  setState(() => _layout = i);
                  Navigator.of(ctx).pop();
                },
              ),
            const Divider(color: Color(0xFF38383A)),
            ListTile(
              title: Text('Presenter Notes', style: Look.font(17, Colors.white)),
              trailing: Row(mainAxisSize: MainAxisSize.min, children: [
                IconButton(onPressed: () => set(() => setState(() => _notesSize = math.max(12, _notesSize - 2))), icon: const Icon(Icons.text_decrease, color: Colors.white)),
                IconButton(onPressed: () => set(() => setState(() => _notesSize = math.min(48, _notesSize + 2))), icon: const Icon(Icons.text_increase, color: Colors.white)),
              ]),
            ),
            SwitchListTile(value: _invert, onChanged: (v) => set(() => setState(() => _invert = v)), title: Text('Invert Colors', style: Look.font(17, Colors.white))),
          ]),
        ),
      ),
    );
  }

  /// Drawing mode: the slide large, the laser or a colour under the finger, arrows at its sides.
  Widget _drawing() {
    final aspect = (link.state['aspect'] as num?)?.toDouble() ?? 9 / 16;
    return Column(children: [
      Expanded(
        child: Center(
          child: AspectRatio(
            aspectRatio: 1 / aspect,
            child: LayoutBuilder(builder: (ctx, box) {
              List<double> norm(Offset p) => [(p.dx / box.maxWidth).clamp(0.0, 1.0), (p.dy / box.maxHeight).clamp(0.0, 1.0)];
              void sendStroke() => link.send({'type': 'draw', 'id': _markId, 'color': '#${(_color.toARGB32() & 0xFFFFFF).toRadixString(16).padLeft(6, '0')}', 'points': [for (final p in _points) norm(p)]});
              return Stack(children: [
                Positioned.fill(child: link.current == null ? const ColoredBox(color: Color(0xFF1C1C1E)) : Image.memory(link.current!, gaplessPlayback: true, fit: BoxFit.fill)),
                Positioned.fill(
                  child: GestureDetector(
                    onPanStart: (d) {
                      if (_laser) return;
                      _markId++;
                      _points = [d.localPosition];
                      _marks.add((_color, _points));
                      sendStroke();
                      setState(() {});
                    },
                    onPanUpdate: (d) {
                      if (_laser) {
                        final p = norm(d.localPosition);
                        link.send({'type': 'laser', 'x': p[0], 'y': p[1]});
                        setState(() => _points = [d.localPosition]);
                        return;
                      }
                      _points.add(d.localPosition);
                      sendStroke();
                      setState(() {});
                    },
                    onPanEnd: (_) {
                      if (_laser) {
                        link.send({'type': 'laser'});
                        setState(() => _points = []);
                      }
                    },
                    child: CustomPaint(painter: _Ink(_marks, _laser ? _points : const [])),
                  ),
                ),
                Positioned(left: 4, top: 0, bottom: 0, child: Center(child: _round(Icons.chevron_left, () => link.cmd('prev')))),
                Positioned(right: 4, top: 0, bottom: 0, child: Center(child: _round(Icons.chevron_right, () => link.cmd('next')))),
              ]);
            }),
          ),
        ),
      ),
      const SizedBox(height: 12),
      // the tools: laser pointer, the colours, undo, Done
      Container(
        padding: const EdgeInsets.symmetric(horizontal: 10, vertical: 8),
        decoration: BoxDecoration(color: const Color(0xFF2C2C2E), borderRadius: BorderRadius.circular(30)),
        child: SingleChildScrollView(
          scrollDirection: Axis.horizontal,
          child: Row(mainAxisSize: MainAxisSize.min, children: [
            _swatch(const Color(0xFFFF3B30), _laser, () => setState(() => _laser = true), laser: true),
            for (final c in _colors) _swatch(c, !_laser && _color == c, () => setState(() {
                  _laser = false;
                  _color = c;
                })),
            IconButton(
              onPressed: () {
                link.send({'type': 'undo'});
                if (_marks.isNotEmpty) setState(() => _marks.removeLast());
              },
              icon: const Icon(Icons.undo, color: Colors.white),
            ),
            TextButton(
              onPressed: () {
                link.send({'type': 'laser'});
                setState(() => _draw = false);
              },
              child: Text('Done', style: Look.font(17, const Color(0xFF0A84FF), bold: true)),
            ),
          ]),
        ),
      ),
      const SizedBox(height: 10),
    ]);
  }

  Widget _round(IconData i, VoidCallback onTap) => GestureDetector(
        onTap: onTap,
        child: Container(width: 40, height: 40, decoration: const BoxDecoration(color: Color(0x88000000), shape: BoxShape.circle), child: Icon(i, color: Colors.white)),
      );

  Widget _swatch(Color c, bool on, VoidCallback onTap, {bool laser = false}) => GestureDetector(
        onTap: onTap,
        child: Container(
          margin: const EdgeInsets.symmetric(horizontal: 5),
          width: 34,
          height: 34,
          decoration: BoxDecoration(shape: BoxShape.circle, border: Border.all(color: on ? Colors.white : Colors.transparent, width: 2.5)),
          padding: const EdgeInsets.all(3),
          child: Container(
            decoration: BoxDecoration(color: c, shape: BoxShape.circle, border: c == Colors.black ? Border.all(color: const Color(0xFF636366)) : null),
            child: laser ? Center(child: Container(width: 9, height: 9, decoration: const BoxDecoration(color: Colors.white, shape: BoxShape.circle))) : null,
          ),
        ),
      );

  @override
  Widget build(BuildContext context) {
    final s = link.state;
    final playing = s['playing'] == true;
    // a new slide or build: the highlights go (as on the computer)
    final at = '${s['slide']}:${s['step']}';
    if (at != _shownAt) {
      _shownAt = at;
      _marks.clear();
    }
    final topBar = Padding(
      padding: const EdgeInsets.fromLTRB(4, 4, 4, 8),
      child: Row(children: [
        IconButton(
          tooltip: playing ? 'End Slideshow' : 'Close',
          onPressed: () => playing ? link.cmd('stop') : widget.onClose(),
          icon: const Icon(Icons.close, color: Colors.white),
        ),
        if (s.isNotEmpty)
          TextButton(onPressed: _pickSlide, child: Text('${s['number']} of ${s['visible']}', style: Look.font(16, Colors.white, bold: true))),
        Expanded(
          child: GestureDetector(
            onTap: () => setState(() => _clock = !_clock),
            child: Text(_time(), textAlign: TextAlign.center, style: Look.font(18, playing && !_clock ? const Color(0xFFFF9F0A) : Colors.white)),
          ),
        ),
        IconButton(tooltip: 'Devices', onPressed: widget.onDevices, icon: const Icon(Icons.devices_outlined, color: Colors.white)),
        if (playing) IconButton(tooltip: 'Layout', onPressed: _layoutMenu, icon: const Icon(Icons.view_quilt_outlined, color: Colors.white)),
        if (playing) IconButton(tooltip: 'Highlighter', onPressed: () => setState(() => _draw = true), icon: Icon(Icons.edit_outlined, color: _draw ? const Color(0xFF0A84FF) : Colors.white)),
      ]),
    );
    Widget body;
    if (!playing) {
      body = Column(children: [
        Text(s['title'] as String? ?? link.desk?.name ?? '', style: Look.font(17, const Color(0xFF98989F))),
        const SizedBox(height: 14),
        Expanded(child: Center(child: _pic(link.current))),
        const SizedBox(height: 26),
        SizedBox(
          width: 220,
          height: 54,
          child: FilledButton.icon(
            style: FilledButton.styleFrom(backgroundColor: const Color(0xFF0A84FF), shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(27))),
            onPressed: s.isEmpty ? null : () => link.cmd('play'),
            icon: const Icon(Icons.play_arrow, color: Colors.white),
            label: Text('Play', style: Look.font(18, Colors.white, bold: true)),
          ),
        ),
      ]);
    } else if (_draw) {
      body = _drawing();
    } else {
      body = LayoutBuilder(builder: (ctx, box) => _layoutBody(box));
    }
    return Column(children: [
      topBar,
      Expanded(child: Padding(padding: const EdgeInsets.fromLTRB(16, 0, 16, 12), child: body)),
    ]);
  }
}

/// The highlights drawn (and the laser's dot) over the slide on the remote.
class _Ink extends CustomPainter {
  _Ink(this.marks, this.laser);
  final List<(Color, List<Offset>)> marks;
  final List<Offset> laser;

  @override
  void paint(Canvas canvas, Size size) {
    for (final (c, pts) in marks) {
      final p = Paint()
        ..color = c
        ..strokeWidth = 4
        ..strokeCap = StrokeCap.round
        ..strokeJoin = StrokeJoin.round
        ..style = PaintingStyle.stroke;
      if (pts.length == 1) {
        canvas.drawCircle(pts.first, 2.5, p..style = PaintingStyle.fill);
      } else {
        canvas.drawPath(Path()..addPolygon(pts, false), p);
      }
    }
    if (laser.isNotEmpty) {
      canvas.drawCircle(laser.last, 16, Paint()..color = const Color(0x55FF3B30));
      canvas.drawCircle(laser.last, 8, Paint()..color = const Color(0xFFFF3B30));
    }
  }

  @override
  bool shouldRepaint(_Ink o) => true;
}
