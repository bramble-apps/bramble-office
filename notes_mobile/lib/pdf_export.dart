// Export to PDF: the desktop's File > Export… PDF (nt-app print.rs) — a note laid out on US
// Letter pages (portrait or landscape) with 54 pt margins, Notes' sizes on paper (Title 24,
// Heading 18, Subheading 15, Body 12), list labels hanging, pictures between the paragraphs (at
// most 4 inches tall), each page drawn at 200 dpi into a PDF of page pictures.
import 'dart:io';
import 'dart:math' as math;
import 'dart:typed_data';
import 'dart:ui' as ui;

import 'package:flutter/painting.dart';

const _pageW = 612.0, _pageH = 792.0, _margin = 54.0;

/// Pixels per point the pages are drawn at (200 dpi).
const _scale = 200 / 72;

const _tokenColor = Color.fromRGBO(217, 153, 0, 1);

/// What goes down the page: a paragraph's text (its label hanging), or a picture.
sealed class _Block {}

class _Text extends _Block {
  _Text(this.text, this.label, this.indent, this.height, {this.checked});
  final TextPainter text;
  final TextPainter? label;
  final double indent, height;

  /// a checklist item's box (drawn, not a font's ☐), ticked or not
  final bool? checked;
}

class _Picture extends _Block {
  _Picture(this.image, this.w, this.h);
  final ui.Image image;
  final double w, h;
}

/// The note (as nt-ffi's `note` answers it) as a PDF's bytes.
Future<Uint8List> notePdf(Map note, {bool landscape = false}) async {
  final (pw, ph) = landscape ? (_pageH, _pageW) : (_pageW, _pageH);
  final blocks = await _blocks(note, pw - 2 * _margin);
  final pages = <ui.Picture>[];
  ui.PictureRecorder? rec;
  late Canvas c;
  void newPage() {
    if (rec != null) pages.add(rec!.endRecording());
    rec = ui.PictureRecorder();
    c = Canvas(rec!, Rect.fromLTWH(0, 0, pw * _scale, ph * _scale))..scale(_scale);
    c.drawRect(Rect.fromLTWH(0, 0, pw, ph), Paint()..color = const Color(0xFFFFFFFF));
  }

  newPage();
  const top = _margin;
  final bottom = ph - _margin;
  var y = top;
  for (final b in blocks) {
    switch (b) {
      case _Text(:final text, :final label, :final indent, :final height, :final checked):
        // a paragraph that doesn't fit goes on, line by line, onto the next page
        final lines = [for (final l in text.computeLineMetrics()) (l.baseline - l.ascent, l.baseline - l.ascent + l.height)];
        final linesEnd = lines.isEmpty ? height : lines.last.$2;
        var start = 0.0;
        while (start < height) {
          final room = bottom - y;
          // the most of it, by whole lines, that fits here
          final fit = lines.map((l) => l.$2 - start).where((e) => e > 0 && e <= room).fold(0.0, math.max);
          void draw(double h) {
            c.save();
            c.clipRect(Rect.fromLTWH(0, y, pw, h + 1));
            c.translate(_margin, y - start);
            if (start == 0 && checked != null) {
              _box(c, text, indent, checked);
            } else if (start == 0) {
              label?.paint(c, Offset(indent, 0));
            }
            text.paint(c, Offset(indent + (label?.width ?? 0), 0));
            c.restore();
          }

          if (fit <= 0) {
            if (y > top) {
              newPage();
              y = top;
              continue;
            }
            // (a single line taller than the page: as much as there's room for)
            draw(room);
            start += room;
            continue;
          }
          draw(fit);
          if (start + fit >= linesEnd - 0.5) {
            // every line drawn: the space after it too, then the next block
            y += height - start;
            start = height;
          } else {
            start += fit;
            newPage();
            y = top;
          }
        }
      case _Picture(:final image, :final w, :final h):
        if (y + h > bottom && y > top) {
          newPage();
          y = top;
        }
        c.drawImageRect(
          image,
          Rect.fromLTWH(0, 0, image.width.toDouble(), image.height.toDouble()),
          Rect.fromLTWH(_margin, y, w, h),
          Paint()..filterQuality = FilterQuality.high,
        );
        y += h + 8;
    }
  }
  pages.add(rec!.endRecording());
  for (final b in blocks) {
    if (b is _Text) {
      b.text.dispose();
      b.label?.dispose();
    } else if (b is _Picture) {
      b.image.dispose();
    }
  }
  return _pdf(pages, pw, ph);
}

/// A checklist box on the first line of `text`, at `x`: an outline, or filled with a tick.
void _box(Canvas c, TextPainter text, double x, bool checked) {
  final m = text.computeLineMetrics();
  final size = (text.text?.style?.fontSize ?? 12) * 0.82;
  final mid = m.isEmpty ? size / 2 : m.first.baseline - (text.text?.style?.fontSize ?? 12) * 0.34;
  final r = RRect.fromRectAndRadius(Rect.fromLTWH(x, mid - size / 2, size, size), Radius.circular(size * 0.2));
  if (!checked) {
    c.drawRRect(r, Paint()..style = PaintingStyle.stroke..strokeWidth = 0.9..color = const Color(0xFF000000));
    return;
  }
  c.drawRRect(r, Paint()..color = const Color(0xFF000000));
  final b = r.outerRect;
  c.drawPath(
    Path()
      ..moveTo(b.left + size * 0.22, b.top + size * 0.52)
      ..lineTo(b.left + size * 0.42, b.top + size * 0.72)
      ..lineTo(b.left + size * 0.78, b.top + size * 0.3),
    Paint()..style = PaintingStyle.stroke..strokeWidth = size * 0.14..strokeCap = StrokeCap.round..strokeJoin = StrokeJoin.round..color = const Color(0xFFFFFFFF),
  );
}

/// A paragraph style's text on paper.
TextStyle _style(String st) {
  final (family, size, weight) = switch (st) {
    'title' => ('SferaDisplay', 24.0, FontWeight.w700),
    'heading' => ('SferaDisplay', 18.0, FontWeight.w700),
    'subheading' => ('Sfera', 15.0, FontWeight.w600),
    'mono' => ('SferaMono', 11.0, FontWeight.w400),
    _ => ('Sfera', 12.0, FontWeight.w400),
  };
  return TextStyle(fontFamily: family, fontSize: size, fontWeight: weight, color: const Color(0xFF000000), height: 1.25);
}

double _spaceAfter(String st) => switch (st) {
  'title' => 8,
  'heading' || 'subheading' => 5,
  _ => 3,
};

Future<List<_Block>> _blocks(Map note, double width) async {
  final out = <_Block>[];
  final atts = <Object?, Map>{for (final a in (note['attachments'] as List? ?? const []).cast<Map>()) a['id']: a};
  final numbers = <int>[];
  for (final p in (note['body'] as List? ?? const []).cast<Map>()) {
    final st = p['st'] as String? ?? 'body';
    final base = _style(st);
    final ind = (p['ind'] as num? ?? 0).toInt();
    final indent = 24.0 * ind;
    if (st == 'number') {
      while (numbers.length < ind + 1) {
        numbers.add(0);
      }
      numbers.length = ind + 1;
      numbers[ind]++;
    } else {
      numbers.clear();
    }
    final label = switch (st) {
      'bullet' => '•  ',
      'dash' => '–  ',
      'number' => '${numbers.isEmpty ? 1 : numbers.last}.  ',
      // (room for the box drawn there)
      'check' => '\u2003\u2002',
      _ => '',
    };
    final pictures = <_Picture>[];
    final spans = <TextSpan>[];
    var text = '';
    for (final s in (p['sp'] as List? ?? const []).cast<Map>()) {
      final tk = s['tk'] as Map?;
      if (tk != null) {
        final a = atts[tk['id']];
        if (a == null) {
          // tags, links to notes, dividers, math: the text Notes shows for them
          final alt = tk['alt'] as String? ?? '';
          if (alt.isNotEmpty) {
            spans.add(TextSpan(text: alt, style: base.copyWith(color: _tokenColor)));
            text += alt;
          }
        } else {
          final pic = await _picture(a, width);
          if (pic != null) {
            pictures.add(pic);
          } else {
            // files, audio, links: their name
            final name = (a['name'] as String? ?? '').isNotEmpty ? a['name'] as String : (a['url'] as String? ?? 'Attachment');
            spans.add(TextSpan(text: '[$name]', style: base.copyWith(fontStyle: FontStyle.italic)));
            text += '[$name]';
          }
        }
        continue;
      }
      final t = s['t'] as String? ?? '';
      final link = s['ln'] != null;
      final col = (s['c'] as List?)?.cast<num>();
      spans.add(
        TextSpan(
          text: t,
          style: base.copyWith(
            fontWeight: s['b'] == true ? FontWeight.w700 : null,
            fontStyle: s['i'] == true ? FontStyle.italic : null,
            decoration: TextDecoration.combine([
              if (s['u'] == true || link) TextDecoration.underline,
              if (s['s'] == true) TextDecoration.lineThrough,
            ]),
            color: link
                ? _tokenColor
                : (col != null && col.length == 4 ? Color.from(alpha: col[3].toDouble(), red: col[0].toDouble(), green: col[1].toDouble(), blue: col[2].toDouble()) : null),
          ),
        ),
      );
      text += t;
    }
    if (text.trim().isNotEmpty || pictures.isEmpty) {
      final lp = label.isEmpty ? null : (TextPainter(text: TextSpan(text: label, style: base), textDirection: TextDirection.ltr)..layout());
      final tp = TextPainter(
        text: TextSpan(style: base, children: text.isEmpty ? [TextSpan(text: ' ', style: base)] : spans),
        textDirection: TextDirection.ltr,
        textAlign: switch ((p['al'] as num? ?? 0).toInt()) {
          2 => TextAlign.center,
          3 => TextAlign.right,
          4 => TextAlign.justify,
          _ => TextAlign.left,
        },
      )..layout(maxWidth: math.max(40, width - indent - (lp?.width ?? 0)));
      out.add(_Text(tp, lp, indent, tp.height + _spaceAfter(st), checked: st == 'check' ? p['ck'] == true : null));
    }
    out.addAll(pictures);
  }
  return out;
}

/// An attachment's picture, scaled to the page's width (at most 4 inches tall): images, scans
/// (their first page), drawings (their fallback image), PDFs (their preview).
Future<_Picture?> _picture(Map a, double width) async {
  String? f(Object? v) => v is String && v.isNotEmpty && File(v).existsSync() ? v : null;
  final parts = (a['parts'] as List? ?? const []).cast<Map>();
  final fallback = f(a['fallback']);
  final path = switch (a['kind']) {
    'image' => f(a['file']) ?? f(a['preview']),
    'scan' => (parts.isEmpty ? null : f(parts.first['file']) ?? f(parts.first['preview'])) ?? f(a['preview']),
    'drawing' => (fallback != null && RegExp(r'\.(png|jpe?g)$', caseSensitive: false).hasMatch(fallback) ? fallback : null) ?? f(a['preview']),
    'pdf' => f(a['preview']),
    _ => null,
  };
  if (path == null) return null;
  try {
    final codec = await ui.instantiateImageCodec(await File(path).readAsBytes());
    final img = (await codec.getNextFrame()).image;
    final (iw, ih) = (img.width.toDouble(), img.height.toDouble());
    if (iw < 1 || ih < 1) return null;
    var w = math.max(40.0, math.min(width, iw / 2));
    var h = w * ih / iw;
    if (h > 288) {
      h = 288;
      w = h * iw / ih;
    }
    return _Picture(img, w, h);
  } catch (_) {
    return null;
  }
}

/// The pages as a PDF: each one picture, its pixels deflated.
Future<Uint8List> _pdf(List<ui.Picture> pages, double pw, double ph) async {
  final (w, h) = ((pw * _scale).round(), (ph * _scale).round());
  final out = BytesBuilder(copy: false);
  final offsets = <int>[];
  void obj(List<int> body) {
    offsets.add(out.length);
    out.add('${offsets.length} 0 obj\n'.codeUnits);
    out.add(body);
    out.add('\nendobj\n'.codeUnits);
  }

  out.add([...'%PDF-1.4\n%'.codeUnits, 0xE2, 0xE3, 0xCF, 0xD3, 10]);
  obj('<< /Type /Catalog /Pages 2 0 R >>'.codeUnits);
  int pageObj(int i) => 3 + i * 3;
  obj('<< /Type /Pages /Count ${pages.length} /Kids [${[for (var i = 0; i < pages.length; i++) '${pageObj(i)} 0 R'].join(' ')}] >>'.codeUnits);
  for (var i = 0; i < pages.length; i++) {
    final img = await pages[i].toImage(w, h);
    pages[i].dispose();
    final rgba = (await img.toByteData(format: ui.ImageByteFormat.rawRgba))!.buffer.asUint8List();
    img.dispose();
    final rgb = Uint8List(w * h * 3);
    for (var s = 0, d = 0; s < rgba.length; s += 4, d += 3) {
      rgb[d] = rgba[s];
      rgb[d + 1] = rgba[s + 1];
      rgb[d + 2] = rgba[s + 2];
    }
    final data = ZLibCodec(level: 6).encode(rgb);
    final (content, image) = (pageObj(i) + 1, pageObj(i) + 2);
    obj('<< /Type /Page /Parent 2 0 R /MediaBox [0 0 ${_n(pw)} ${_n(ph)}] /Resources << /XObject << /Im0 $image 0 R >> >> /Contents $content 0 R >>'.codeUnits);
    final draw = 'q ${_n(pw)} 0 0 ${_n(ph)} 0 0 cm /Im0 Do Q';
    obj('<< /Length ${draw.length} >>\nstream\n$draw\nendstream'.codeUnits);
    obj([
      ...'<< /Type /XObject /Subtype /Image /Width $w /Height $h /ColorSpace /DeviceRGB /BitsPerComponent 8 /Filter /FlateDecode /Length ${data.length} >>\nstream\n'.codeUnits,
      ...data,
      ...'\nendstream'.codeUnits,
    ]);
  }
  final xref = out.length;
  out.add('xref\n0 ${offsets.length + 1}\n0000000000 65535 f \n'.codeUnits);
  for (final o in offsets) {
    out.add('${o.toString().padLeft(10, '0')} 00000 n \n'.codeUnits);
  }
  out.add('trailer\n<< /Size ${offsets.length + 1} /Root 1 0 R >>\nstartxref\n$xref\n%%EOF\n'.codeUnits);
  return out.takeBytes();
}

String _n(double v) => v == v.roundToDouble() ? v.toInt().toString() : v.toStringAsFixed(2);
