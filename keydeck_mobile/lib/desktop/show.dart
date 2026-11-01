// The slideshow: the desktop's own show (kn-app play.rs: every build and transition, dynamic
// backgrounds playing, hyperlinks, self-playing timing, Loop) runs in the engine, and what it
// paints comes here as textured triangles (kn-app showapi.rs) drawn with Canvas.drawVertices.
// A tap or a swipe to the left moves on, a swipe to the right goes back, the system's Back (or
// Escape) ends it; a long press shows the controls. Rehearse Slideshow shows the presenter
// display: the slide, the next one, the notes and the timer.
import 'dart:async';
import 'dart:io';
import 'dart:typed_data';
import 'dart:ui' as ui;

import 'package:flutter/material.dart';
import 'package:flutter/scheduler.dart';
import 'package:flutter/services.dart';
import 'package:path_provider/path_provider.dart';
import 'package:record/record.dart';

import '../engine.dart';
import 'look.dart';

class SlideShow extends StatefulWidget {
  const SlideShow({super.key, required this.engine, required this.onEnd});
  final Engine engine;
  final VoidCallback onEnd;

  @override
  State<SlideShow> createState() => _SlideShowState();
}

/// One mesh of a frame: its texture, clip and triangles.
class _Mesh {
  _Mesh(this.tex, this.clip, this.vertices);
  final int tex;
  final Rect clip;
  final ui.Vertices vertices;
}

class _SlideShowState extends State<SlideShow> with SingleTickerProviderStateMixin {
  Engine get e => widget.engine;
  late final Ticker _ticker = createTicker(_tick);
  final _textures = <int, ui.Image>{};
  final _shaders = <int, ImageShader>{};
  List<_Mesh> _meshes = const [];
  final _input = <Map<String, Object>>[];
  Map<String, dynamic>? _s;

  /// The screen the show's slides are drawn on (points), and its pixels per point.
  Size _screen = Size.zero;
  double _ppp = 1;

  /// When the engine next wants a frame (null: only on input), and whether textures of the last
  /// frame are still being decoded.
  DateTime? _due = DateTime.now();
  bool _decoding = false;
  bool _ended = false;
  bool _hud = false;
  Timer? _hudTimer, _clock;
  ui.Image? _next;
  int? _nextOf;

  /// Movies and sounds playing (the engine's object id -> the player's texture id, or a
  /// negative id for a sound), on Android's MediaPlayer (MainActivity "video").
  static const _video = MethodChannel('app.bramble.keydeck/video');
  final _players = <int, int>{};
  final _starting = <int>{};

  /// Record Slideshow: the microphone, recorded here while the engine keeps the timing.
  AudioRecorder? _rec;
  bool _recStarting = false;

  Future<void> _narration(bool on) async {
    if (!on || _rec != null || _recStarting || !Platform.isAndroid) return;
    _recStarting = true;
    try {
      final r = AudioRecorder();
      if (!await r.hasPermission()) return;
      final dir = await getTemporaryDirectory();
      await r.start(const RecordConfig(encoder: AudioEncoder.aacLc, bitRate: 128000), path: '${dir.path}/narration.m4a');
      _rec = r;
    } catch (_) {
    } finally {
      _recStarting = false;
    }
  }

  /// The show ended: the sound recorded goes to the engine with the show's timing.
  Future<void> _narrationDone() async {
    final r = _rec;
    _rec = null;
    String? path;
    try {
      path = await r?.stop();
      await r?.dispose();
    } catch (_) {}
    if (path != null || e.status()['narrationWaiting'] == true) {
      e.action('narration:${path ?? ''}');
      widget.onEnd();
    }
  }

  /// The show's movies as the engine has them now: new ones start, gone ones stop.
  Future<void> _syncMovies(List movies) async {
    final want = {for (final m in movies.cast<Map<String, dynamic>>()) (m['id'] as num).toInt(): m};
    for (final id in _players.keys.where((k) => !want.containsKey(k)).toList()) {
      final t = _players.remove(id);
      try {
        await _video.invokeMethod('stop', t);
      } catch (_) {}
    }
    for (final MapEntry(key: id, value: m) in want.entries) {
      if (_players.containsKey(id) || !_starting.add(id)) continue;
      try {
        final t = await _video.invokeMethod<int>('play', {'path': m['path'], 'start': (m['start'] as num).toDouble(), 'end': (m['end'] as num).toDouble(), 'volume': (m['volume'] as num).toDouble(), 'loop': m['loop'] == true, 'video': m['video'] == true});
        if (t != null && mounted) setState(() => _players[id] = t);
      } catch (_) {
        // (no player here: the Linux build shows the poster)
      } finally {
        _starting.remove(id);
      }
    }
  }

  void _stopMovies() {
    for (final t in _players.values) {
      _video.invokeMethod('stop', t).catchError((_) => null);
    }
    _players.clear();
  }

  /// The playing movies' pictures over the slide (the show fits the slide in the screen).
  List<Widget> _movieViews(Size screen) {
    final s = _s;
    final size = s?['size'] as List?;
    if (s == null || size == null || _players.isEmpty) return const [];
    final sw = (size[0] as num).toDouble(), sh = (size[1] as num).toDouble();
    final k = (screen.width / sw < screen.height / sh) ? screen.width / sw : screen.height / sh;
    final o = Offset((screen.width - sw * k) / 2, (screen.height - sh * k) / 2);
    return [
      for (final m in (s['movies'] as List? ?? const []).cast<Map<String, dynamic>>())
        if (m['video'] == true && (_players[(m['id'] as num).toInt()] ?? -1) >= 0)
          Positioned(
            left: o.dx + (m['rect'][0] as num) * k,
            top: o.dy + (m['rect'][1] as num) * k,
            width: (m['rect'][2] as num) * k,
            height: (m['rect'][3] as num) * k,
            child: IgnorePointer(child: Transform.rotate(angle: (m['angle'] as num).toDouble() * 3.14159265 / 180, child: Texture(textureId: _players[(m['id'] as num).toInt()]!))),
          ),
    ];
  }

  @override
  void initState() {
    super.initState();
    SystemChrome.setEnabledSystemUIMode(SystemUiMode.immersiveSticky);
    HardwareKeyboard.instance.addHandler(_key);
    _ticker.start();
    _clock = Timer.periodic(const Duration(milliseconds: 500), (_) {
      if (mounted && _s?['rehearse'] == true) setState(() {});
    });
  }

  @override
  void dispose() {
    SystemChrome.setEnabledSystemUIMode(SystemUiMode.edgeToEdge);
    HardwareKeyboard.instance.removeHandler(_key);
    _stopMovies();
    _ticker.dispose();
    _hudTimer?.cancel();
    _clock?.cancel();
    for (final t in _textures.values) {
      t.dispose();
    }
    super.dispose();
  }

  bool _key(KeyEvent ev) {
    if (ev is! KeyDownEvent) return false;
    final names = {
      LogicalKeyboardKey.escape: 'Escape',
      LogicalKeyboardKey.arrowRight: 'ArrowRight',
      LogicalKeyboardKey.arrowDown: 'ArrowDown',
      LogicalKeyboardKey.arrowLeft: 'ArrowLeft',
      LogicalKeyboardKey.arrowUp: 'ArrowUp',
      LogicalKeyboardKey.space: 'Space',
      LogicalKeyboardKey.pageDown: 'PageDown',
      LogicalKeyboardKey.pageUp: 'PageUp',
      LogicalKeyboardKey.enter: 'Enter',
      LogicalKeyboardKey.backspace: 'Backspace',
      LogicalKeyboardKey.home: 'Home',
      LogicalKeyboardKey.end: 'End',
      LogicalKeyboardKey.keyR: 'R',
    };
    final k = names[ev.logicalKey];
    if (k == null) return false;
    _send({'t': 'key', 'k': k});
    return true;
  }

  void _send(Map<String, Object> ev) {
    _input.add(ev);
    _due = DateTime.now();
  }

  void _end() {
    if (_ended) return;
    _ended = true;
    _stopMovies();
    e.playStop();
    widget.onEnd();
    _narrationDone();
  }

  void _tick(Duration _) {
    if (_ended || _decoding || _screen.isEmpty) return;
    final due = _due;
    if (due == null || DateTime.now().isBefore(due)) return;
    final input = List<Map<String, Object>>.of(_input);
    _input.clear();
    final bytes = e.playFrame(_screen.width, _screen.height, _ppp, input);
    final s = e.playStatus();
    if (bytes == null || s == null) return _end();
    _s = s;
    _syncMovies(s['movies'] as List? ?? const []);
    _narration(s['narrating'] == true);
    // a hyperlink to a web page clicked in the show: the browser
    for (final u in e.urls()) {
      const MethodChannel('app.bramble.keydeck/open').invokeMethod('url', u).catchError((_) => null);
    }
    final ms = (s['repaint'] as num?)?.toInt();
    _due = ms == null ? null : DateTime.now().add(Duration(milliseconds: ms));
    _apply(bytes);
  }

  /// A frame's textures (decoded before it shows) and meshes.
  Future<void> _apply(Uint8List b) async {
    final d = ByteData.sublistView(b);
    var o = 0;
    int u32() {
      final v = d.getUint32(o, Endian.little);
      o += 4;
      return v;
    }

    int u64() {
      final v = d.getUint64(o, Endian.little);
      o += 8;
      return v;
    }

    // new pictures and freed ones take effect with the frame's meshes (the shown frame keeps
    // drawing with the old ones until then)
    final decodes = <Future<void>>[];
    final fresh = <int, ui.Image>{};
    final freed = <int>[];
    for (var n = u32(); n > 0; n--) {
      final id = u64(), w = u32(), h = u32();
      final px = Uint8List.sublistView(b, o, o + w * h * 4);
      o += w * h * 4;
      decodes.add(() async {
        final buffer = await ui.ImmutableBuffer.fromUint8List(px);
        final desc = ui.ImageDescriptor.raw(buffer, width: w, height: h, pixelFormat: ui.PixelFormat.rgba8888);
        fresh[id] = (await (await desc.instantiateCodec()).getNextFrame()).image;
      }());
    }
    for (var n = u32(); n > 0; n--) {
      freed.add(u64());
    }
    final meshes = <_Mesh>[];
    for (var n = u32(); n > 0; n--) {
      final tex = u64();
      final clip = Rect.fromLTRB(d.getFloat32(o, Endian.little), d.getFloat32(o + 4, Endian.little), d.getFloat32(o + 8, Endian.little), d.getFloat32(o + 12, Endian.little));
      o += 16;
      final nv = u32(), ni = u32();
      final pos = Float32List(nv * 2), uv = Float32List(nv * 2), col = Int32List(nv);
      for (var i = 0; i < nv; i++) {
        pos[i * 2] = d.getFloat32(o, Endian.little);
        pos[i * 2 + 1] = d.getFloat32(o + 4, Endian.little);
        uv[i * 2] = d.getFloat32(o + 8, Endian.little);
        uv[i * 2 + 1] = d.getFloat32(o + 12, Endian.little);
        col[i] = d.getInt32(o + 16, Endian.little);
        o += 20;
      }
      final idx = Uint16List(ni);
      for (var i = 0; i < ni; i++) {
        idx[i] = d.getUint16(o + i * 2, Endian.little);
      }
      o += ni * 2 + (ni.isOdd ? 2 : 0);
      meshes.add(_Mesh(tex, clip, ui.Vertices.raw(ui.VertexMode.triangles, pos, textureCoordinates: uv, colors: col, indices: idx)));
    }
    if (decodes.isNotEmpty) {
      _decoding = true;
      await Future.wait(decodes);
      _decoding = false;
    }
    if (!mounted) return;
    final old = <ui.Image>[];
    setState(() {
      for (final id in [...fresh.keys, ...freed]) {
        final o = _textures.remove(id);
        if (o != null) old.add(o);
        _shaders.remove(id);
      }
      _textures.addAll(fresh);
      _meshes = meshes;
    });
    WidgetsBinding.instance.addPostFrameCallback((_) {
      for (final o in old) {
        o.dispose();
      }
    });
    _nextPicture();
  }

  /// Rehearse: the next slide's picture for the presenter display.
  Future<void> _nextPicture() async {
    final s = _s;
    if (s == null || s['rehearse'] != true) return;
    final n = (s['next'] as num?)?.toInt();
    if (n == _nextOf) return;
    _nextOf = n;
    final size = s['size'] as List?;
    if (n == null || size == null) return setState(() => _next = null);
    const w = 640;
    final k = w / (size[0] as num);
    final img = await e.render(n, k.toDouble(), 0, 0, w, ((size[1] as num) * k).round());
    if (mounted) setState(() => _next = img);
  }

  ImageShader? _shader(int id) {
    final img = _textures[id];
    if (img == null) return null;
    return _shaders[id] ??= ImageShader(img, TileMode.clamp, TileMode.clamp, Matrix4.identity().storage, filterQuality: FilterQuality.low);
  }

  void _showHud() {
    setState(() => _hud = true);
    _hudTimer?.cancel();
    _hudTimer = Timer(const Duration(seconds: 3), () {
      if (mounted) setState(() => _hud = false);
    });
  }

  Widget _slides(BoxConstraints box) {
    final size = Size(box.maxWidth, box.maxHeight);
    if (size != _screen) {
      _screen = size;
      _due = DateTime.now();
    }
    return GestureDetector(
      behavior: HitTestBehavior.opaque,
      onTapUp: (d) => _send({'t': 'tap', 'x': d.localPosition.dx, 'y': d.localPosition.dy}),
      onLongPress: _showHud,
      onHorizontalDragEnd: (d) {
        final v = d.primaryVelocity ?? 0;
        if (v < -200) _send({'t': 'key', 'k': 'ArrowRight'});
        if (v > 200) _send({'t': 'key', 'k': 'ArrowLeft'});
      },
      child: Stack(children: [CustomPaint(size: size, painter: _MeshPainter(_meshes, _shader)), ..._movieViews(size)]),
    );
  }

  String _hms(double secs) {
    final s = secs.floor();
    String two(int v) => v.toString().padLeft(2, '0');
    return '${two(s ~/ 3600)}:${two((s ~/ 60) % 60)}:${two(s % 60)}';
  }

  /// Rehearse Slideshow: the presenter display (Keynote's): the slide and the next one side by
  /// side, the timer and clock above, the notes below.
  Widget _presenter() {
    final s = _s ?? const {};
    // Play > Customize Presenter Display… (what shows)
    final pd = (s['pd'] as Map?) ?? const {};
    bool on(String k) => pd[k] != false;
    final now = TimeOfDay.now();
    final white = Look.font(15, Colors.white);
    return SafeArea(
      child: Padding(
        padding: const EdgeInsets.all(12),
        child: Column(crossAxisAlignment: CrossAxisAlignment.stretch, children: [
          Row(children: [
            if (on('clock')) Text('${now.hour.toString().padLeft(2, '0')}:${now.minute.toString().padLeft(2, '0')}', style: Look.font(22, Colors.white)),
            const Spacer(),
            Text('Slide ${((s['slide'] as num?) ?? 0) + 1} of ${s['count'] ?? 0}', style: white),
            const Spacer(),
            if (on('timer')) GestureDetector(onTap: () => _send({'t': 'key', 'k': 'R'}), child: Text(_hms((s['timer'] as num?)?.toDouble() ?? 0), style: Look.font(22, const Color(0xFFFF9F0A)))),
            const SizedBox(width: 16),
            TextButton(onPressed: _end, child: Text('End', style: white)),
          ]),
          const SizedBox(height: 8),
          Expanded(
            flex: 3,
            child: Row(crossAxisAlignment: CrossAxisAlignment.start, children: [
              Expanded(flex: 3, child: Column(crossAxisAlignment: CrossAxisAlignment.start, children: [Text('Current', style: Look.font(13, Colors.white70)), const SizedBox(height: 4), Expanded(child: LayoutBuilder(builder: (ctx, box) => _slides(box)))])),
              const SizedBox(width: 12),
              if (on('next')) Expanded(
                flex: 2,
                child: Column(crossAxisAlignment: CrossAxisAlignment.start, children: [
                  Text('Next', style: Look.font(13, Colors.white70)),
                  const SizedBox(height: 4),
                  if (_next != null) AspectRatio(aspectRatio: _next!.width / _next!.height, child: RawImage(image: _next, fit: BoxFit.contain)) else Text(s['next'] == null ? 'End of slideshow' : '', style: white),
                ]),
              ),
            ]),
          ),
          const SizedBox(height: 8),
          if (on('notes')) Expanded(flex: 2, child: SingleChildScrollView(child: Text((s['notes'] as String?)?.trim().isNotEmpty == true ? s['notes'] as String : 'No notes', style: Look.font(18, Colors.white)))),
        ]),
      ),
    );
  }

  @override
  Widget build(BuildContext context) {
    _ppp = MediaQuery.of(context).devicePixelRatio;
    final s = _s ?? const {};
    final rehearse = s['rehearse'] == true;
    final count = (s['count'] as num?)?.toInt() ?? 0;
    final slide = (s['slide'] as num?)?.toInt() ?? 0;
    return PopScope(
      canPop: false,
      onPopInvokedWithResult: (didPop, _) {
        if (!didPop) _end();
      },
      child: ColoredBox(
        color: const Color(0xFF000000),
        child: rehearse
            ? _presenter()
            : Stack(children: [
                Positioned.fill(child: LayoutBuilder(builder: (ctx, box) => _slides(box))),
                if (_hud)
                  Positioned(
                    left: 0,
                    right: 0,
                    bottom: 24,
                    child: Center(
                      child: Container(
                        padding: const EdgeInsets.symmetric(horizontal: 6, vertical: 4),
                        decoration: BoxDecoration(color: const Color(0xCC2A2A2C), borderRadius: BorderRadius.circular(24)),
                        child: Row(mainAxisSize: MainAxisSize.min, children: [
                          IconButton(onPressed: () => _send({'t': 'key', 'k': 'ArrowLeft'}), icon: const Icon(Icons.chevron_left, color: Colors.white)),
                          Text('${slide + 1} of $count', style: Look.font(15, Colors.white)),
                          IconButton(onPressed: () => _send({'t': 'key', 'k': 'ArrowRight'}), icon: const Icon(Icons.chevron_right, color: Colors.white)),
                          const SizedBox(width: 8),
                          TextButton(onPressed: _end, child: Text('End Show', style: Look.font(15, Colors.white))),
                        ]),
                      ),
                    ),
                  ),
              ]),
      ),
    );
  }
}

class _MeshPainter extends CustomPainter {
  _MeshPainter(this.meshes, this.shader);
  final List<_Mesh> meshes;
  final ImageShader? Function(int) shader;

  @override
  void paint(Canvas canvas, Size size) {
    for (final m in meshes) {
      final sh = shader(m.tex);
      if (sh == null) continue;
      canvas.save();
      canvas.clipRect(m.clip);
      canvas.drawVertices(m.vertices, BlendMode.modulate, Paint()
        ..shader = sh
        ..filterQuality = FilterQuality.low);
      canvas.restore();
    }
  }

  @override
  bool shouldRepaint(_MeshPainter o) => !identical(o.meshes, meshes);
}
