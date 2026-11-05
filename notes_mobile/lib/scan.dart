// Scan Documents (Notes' scanner): the camera takes a page; the page's edges are found and
// shown as a yellow outline whose corners can be dragged; Keep Scan squares it up with the
// chosen filter (Color, Grayscale, Black & White, Photo) and adds it to the scan's pages
// (the tray at the foot), Retake throws it away; Save puts the pages in the note as one PDF
// ("Scanned Document"), as Notes saves scans. The image work is Android's own (MainActivity,
// Scan.kt); the desktop test build keeps pictures as they are and makes the PDF with convert.
import 'dart:io';
import 'dart:math' as math;

import 'package:file_picker/file_picker.dart';
import 'package:flutter/cupertino.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:path_provider/path_provider.dart';

import 'theme.dart';
import 'ui/glass.dart';

const _system = MethodChannel('app.bramble.notes/system');

/// A finished scan: the PDF and its pages' sizes (points).
class ScanResult {
  ScanResult(this.pdf, this.pages);
  final String pdf;
  final List<Size> pages;
}

Future<ScanResult?> scanDocuments(BuildContext context) =>
    Navigator.of(context, rootNavigator: true).push<ScanResult>(CupertinoPageRoute(fullscreenDialog: true, builder: (_) => const _Scanner()));

const _filters = [('color', 'Color'), ('grayscale', 'Grayscale'), ('bw', 'Black & White'), ('photo', 'Photo')];

class _Scanner extends StatefulWidget {
  const _Scanner();
  @override
  State<_Scanner> createState() => _ScannerState();
}

class _ScannerState extends State<_Scanner> {
  final _pages = <String>[];

  /// The picture being cropped, its size and the page's corners (0..1, TL TR BR BL).
  String? _shot;
  Size _shotSize = Size.zero;
  List<Offset> _corners = const [];
  String _filter = 'color';
  bool _busy = false, _tested = false;

  @override
  void initState() {
    super.initState();
    WidgetsBinding.instance.addPostFrameCallback((_) => _capture());
  }

  Future<void> _capture() async {
    // (the automated checks: NOTES_SCAN_IMAGE is the camera's one picture)
    var test = Platform.environment['NOTES_SCAN_IMAGE'];
    // (on Android: a scan-test.jpg in the app's external folder, with test-snapshots there)
    if (test == null && Platform.isAndroid) {
      try {
        final ext = await getExternalStorageDirectory();
        if (ext != null && File('${ext.path}/test-snapshots').existsSync() && File('${ext.path}/scan-test.jpg').existsSync())
          test = '${ext.path}/scan-test.jpg';
      } catch (_) {}
    }
    String? path = _tested ? null : test;
    _tested = test != null;
    if (test != null) {
    } else if (Platform.isAndroid) {
      final r = await _system.invokeMethod<String>('takePhoto', {'save': false});
      if (r == 'nocamera') {
        path = (await FilePicker.pickFile(type: FileType.image))?.path;
      } else {
        path = r;
      }
    } else {
      path = (await FilePicker.pickFile(type: FileType.image))?.path;
    }
    if (path == null) {
      if (_pages.isEmpty && mounted) Navigator.of(context).pop();
      return;
    }
    setState(() => _busy = true);
    final bytes = await File(path).readAsBytes();
    final img = await decodeImageFromList(bytes);
    var c = const [0.05, 0.05, 0.95, 0.05, 0.95, 0.95, 0.05, 0.95];
    if (Platform.isAndroid) {
      final r = await _system.invokeMethod<List>('detectEdges', path);
      if (r != null && r.length == 8) c = [for (final v in r) (v as num).toDouble()];
    }
    if (!mounted) return;
    setState(() {
      _busy = false;
      _shot = path;
      _shotSize = Size(img.width.toDouble(), img.height.toDouble());
      _corners = [for (var i = 0; i < 4; i++) Offset(c[i * 2], c[i * 2 + 1])];
    });
  }

  Future<void> _keep() async {
    final shot = _shot;
    if (shot == null) return;
    setState(() => _busy = true);
    final dir = Directory('${(await getTemporaryDirectory()).path}/scan')..createSync(recursive: true);
    final out = '${dir.path}/page-${DateTime.now().microsecondsSinceEpoch}.jpg';
    if (Platform.isAndroid) {
      await _system.invokeMethod('warp', {
        'path': shot,
        'corners': [
          for (final o in _corners) ...[o.dx, o.dy],
        ],
        'filter': _filter,
        'out': out,
      });
    } else {
      File(shot).copySync(out);
    }
    if (!mounted) return;
    setState(() {
      if (File(out).existsSync()) _pages.add(out);
      _shot = null;
      _busy = false;
    });
    _capture();
  }

  Future<void> _save() async {
    if (_pages.isEmpty) return;
    setState(() => _busy = true);
    final dir = Directory('${(await getTemporaryDirectory()).path}/scan-${DateTime.now().microsecondsSinceEpoch}')..createSync(recursive: true);
    final pdf = '${dir.path}/Scanned Document.pdf';
    var sizes = <Size>[];
    if (Platform.isAndroid) {
      final r = await _system.invokeMethod<List>('makePdf', {'pages': _pages, 'out': pdf});
      sizes = [for (final p in r ?? const []) Size(((p as List)[0] as num).toDouble(), (p[1] as num).toDouble())];
    } else {
      await Process.run('convert', [..._pages, pdf]);
    }
    if (!mounted) return;
    Navigator.of(context).pop(File(pdf).existsSync() ? ScanResult(pdf, sizes) : null);
  }

  @override
  Widget build(BuildContext context) {
    final p = Palette.of(context);
    final mq = MediaQuery.of(context);
    return Scaffold(
      resizeToAvoidBottomInset: false,
      backgroundColor: Colors.black,
      body: Stack(
        children: [
          Positioned.fill(
            top: mq.padding.top + 64,
            bottom: mq.padding.bottom + 190,
            child: _shot == null
                ? Center(
                    child: _busy
                        ? const CupertinoActivityIndicator(color: Colors.white, radius: 14)
                        : Text(_pages.isEmpty ? '' : 'Ready for the next page', style: Type.body(Colors.white70)),
                  )
                : _Crop(path: _shot!, size: _shotSize, corners: _corners, filter: _filter, onChanged: (c) => setState(() => _corners = c)),
          ),
          // the top: close, and the filters while cropping
          Positioned(
            top: mq.padding.top + 6,
            left: 12,
            right: 12,
            child: Row(
              children: [
                GlassButton(icon: CupertinoIcons.xmark, semantic: 'Cancel', onTap: () => Navigator.of(context).pop()),
                const Spacer(),
                if (_shot != null)
                  Builder(
                    builder: (bctx) => GlassGroup(
                      items: [
                        GlassItem(
                          icon: CupertinoIcons.circle_lefthalf_fill,
                          label: _filters.firstWhere((f) => f.$1 == _filter).$2,
                          onTap: (_) => setState(() => _filter = _filters[(_filters.indexWhere((f) => f.$1 == _filter) + 1) % _filters.length].$1),
                        ),
                      ],
                    ),
                  ),
              ],
            ),
          ),
          // the foot: the filters, Retake / Keep Scan, or the shutter, the pages and Save
          Positioned(
            left: 0,
            right: 0,
            bottom: mq.padding.bottom + 10,
            height: 180,
            child: _shot != null
                ? Column(
                    children: [
                      SizedBox(
                        height: 48,
                        child: ListView(
                          scrollDirection: Axis.horizontal,
                          padding: const EdgeInsets.symmetric(horizontal: 16),
                          children: [
                            for (final (k, l) in _filters)
                              Pressable(
                                onTap: () => setState(() => _filter = k),
                                child: Container(
                                  margin: const EdgeInsets.only(right: 8),
                                  padding: const EdgeInsets.symmetric(horizontal: 16),
                                  alignment: Alignment.center,
                                  decoration: BoxDecoration(color: _filter == k ? p.accent : const Color(0x33FFFFFF), borderRadius: BorderRadius.circular(24)),
                                  child: Text(l, style: Type.text(15, Colors.white, weight: FontWeight.w600)),
                                ),
                              ),
                          ],
                        ),
                      ),
                      const Spacer(),
                      Padding(
                        padding: const EdgeInsets.symmetric(horizontal: 16),
                        child: Row(
                          children: [
                            _TextButton(
                              'Retake',
                              onTap: _busy
                                  ? null
                                  : () {
                                      setState(() => _shot = null);
                                      _capture();
                                    },
                            ),
                            const Spacer(),
                            _TextButton('Keep Scan', bold: true, onTap: _busy ? null : _keep),
                          ],
                        ),
                      ),
                      const SizedBox(height: 18),
                    ],
                  )
                : Row(
                    children: [
                      const SizedBox(width: 20),
                      // the pages so far
                      SizedBox(
                        width: 90,
                        height: 110,
                        child: _pages.isEmpty
                            ? null
                            : Stack(
                                clipBehavior: Clip.none,
                                children: [
                                  Positioned.fill(
                                    child: ClipRRect(
                                      borderRadius: BorderRadius.circular(6),
                                      child: Image.file(File(_pages.last), fit: BoxFit.cover, cacheWidth: 200),
                                    ),
                                  ),
                                  Positioned(
                                    right: -8,
                                    top: -8,
                                    child: Container(
                                      padding: const EdgeInsets.symmetric(horizontal: 7, vertical: 2),
                                      decoration: BoxDecoration(color: p.accent, borderRadius: BorderRadius.circular(10)),
                                      child: Text('${_pages.length}', style: Type.text(13, Colors.white, weight: FontWeight.w700)),
                                    ),
                                  ),
                                ],
                              ),
                      ),
                      const Spacer(),
                      // the shutter
                      Semantics(
                        button: true,
                        label: 'Take Picture',
                        child: Pressable(
                          onTap: _busy ? null : _capture,
                          child: Container(
                            width: 76,
                            height: 76,
                            decoration: BoxDecoration(
                              shape: BoxShape.circle,
                              border: Border.all(color: Colors.white, width: 4),
                            ),
                            child: Center(
                              child: Container(
                                width: 60,
                                height: 60,
                                decoration: const BoxDecoration(shape: BoxShape.circle, color: Colors.white),
                              ),
                            ),
                          ),
                        ),
                      ),
                      const Spacer(),
                      SizedBox(
                        width: 110,
                        child: _TextButton(_pages.isEmpty ? '' : 'Save (${_pages.length})', bold: true, onTap: _pages.isEmpty || _busy ? null : _save),
                      ),
                      const SizedBox(width: 10),
                    ],
                  ),
          ),
        ],
      ),
    );
  }
}

class _TextButton extends StatelessWidget {
  const _TextButton(this.label, {this.onTap, this.bold = false});
  final String label;
  final VoidCallback? onTap;
  final bool bold;
  @override
  Widget build(BuildContext context) {
    final p = Palette.of(context);
    if (label.isEmpty) return const SizedBox.shrink();
    return Pressable(
      onTap: onTap,
      child: Container(
        height: 48,
        padding: const EdgeInsets.symmetric(horizontal: 20),
        alignment: Alignment.center,
        decoration: BoxDecoration(color: bold ? p.accent : const Color(0x33FFFFFF), borderRadius: BorderRadius.circular(24)),
        child: Text(label, style: Type.text(17, Colors.white, weight: bold ? FontWeight.w600 : FontWeight.w500)),
      ),
    );
  }
}

/// The picture with the page's outline: a yellow wash inside, round handles at the corners.
class _Crop extends StatefulWidget {
  const _Crop({required this.path, required this.size, required this.corners, required this.filter, required this.onChanged});
  final String path, filter;
  final Size size;
  final List<Offset> corners;
  final ValueChanged<List<Offset>> onChanged;
  @override
  State<_Crop> createState() => _CropState();
}

class _CropState extends State<_Crop> {
  int? _drag;

  @override
  Widget build(BuildContext context) => LayoutBuilder(
    builder: (ctx, box) {
      final s = widget.size;
      final k = math.min((box.maxWidth - 32) / s.width, box.maxHeight / s.height);
      final w = s.width * k, h = s.height * k;
      final origin = Offset((box.maxWidth - w) / 2, (box.maxHeight - h) / 2);
      Offset at(Offset c) => origin + Offset(c.dx * w, c.dy * h);
      final greys = widget.filter == 'grayscale' || widget.filter == 'bw';
      return GestureDetector(
        onPanStart: (d) {
          var best = 0;
          for (var i = 1; i < 4; i++) {
            if ((at(widget.corners[i]) - d.localPosition).distance < (at(widget.corners[best]) - d.localPosition).distance) best = i;
          }
          _drag = (at(widget.corners[best]) - d.localPosition).distance < 80 ? best : null;
        },
        onPanUpdate: (d) {
          final i = _drag;
          if (i == null) return;
          final c = [...widget.corners];
          final p = d.localPosition - origin;
          c[i] = Offset((p.dx / w).clamp(0.0, 1.0), (p.dy / h).clamp(0.0, 1.0));
          widget.onChanged(c);
        },
        onPanEnd: (_) => _drag = null,
        child: Stack(
          children: [
            Positioned(
              left: origin.dx,
              top: origin.dy,
              width: w,
              height: h,
              child: ColorFiltered(
                colorFilter: greys
                    ? const ColorFilter.matrix([0.3, 0.59, 0.11, 0, 0, 0.3, 0.59, 0.11, 0, 0, 0.3, 0.59, 0.11, 0, 0, 0, 0, 0, 1, 0])
                    : const ColorFilter.mode(Colors.transparent, BlendMode.dst),
                child: Image.file(File(widget.path), fit: BoxFit.fill, cacheWidth: 1200),
              ),
            ),
            Positioned.fill(child: CustomPaint(painter: _Outline([for (final c in widget.corners) at(c)]))),
            for (final c in widget.corners)
              Positioned(
                left: at(c).dx - 14,
                top: at(c).dy - 14,
                child: Container(
                  width: 28,
                  height: 28,
                  decoration: BoxDecoration(
                    shape: BoxShape.circle,
                    color: Colors.white,
                    border: Border.all(color: const Color(0xFFFFCC00), width: 3),
                  ),
                ),
              ),
          ],
        ),
      );
    },
  );
}

class _Outline extends CustomPainter {
  _Outline(this.pts);
  final List<Offset> pts;
  @override
  void paint(Canvas c, Size s) {
    final path = Path()..addPolygon(pts, true);
    c.drawPath(path, Paint()..color = const Color(0x40FFCC00));
    c.drawPath(
      path,
      Paint()
        ..style = PaintingStyle.stroke
        ..strokeWidth = 2.5
        ..color = const Color(0xFFFFCC00),
    );
  }

  @override
  bool shouldRepaint(_Outline o) => true;
}
