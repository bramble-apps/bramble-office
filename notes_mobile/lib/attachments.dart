// What a note's attachments and inline tokens look like in the text: pictures at the column's
// width with rounded corners (Small / Medium / Large from View As), drawings, tables (a hairline
// grid), web links' cards, files and audio as cards, scans and PDFs as their first page; tags,
// note links and math results as coloured text, divider lines across the column.
import 'dart:io';
import 'dart:math' as math;

import 'package:flutter/cupertino.dart';
import 'package:flutter/material.dart';

import 'doc.dart';
import 'drawing.dart';
import 'pdf.dart';
import 'theme.dart';

class TokenContext {
  TokenContext({required this.attachments, required this.width, required this.onTap, required this.onLongPress, required this.onLink, this.table});

  /// The note's attachments by id (nt-ffi attachment JSON).
  final Map<String, Map<String, dynamic>> attachments;

  /// The text column's width.
  double width;
  final void Function(Map<String, dynamic> attachment) onTap;
  final void Function(BuildContext context, Map<String, dynamic> attachment) onLongPress;
  final void Function(Map<String, dynamic> token) onLink;

  /// A table edited in the note (else drawn as a grid).
  final Widget Function(Map<String, dynamic> attachment, double width)? table;
}

Widget tokenWidget(BuildContext context, TokenContext tc, Map<String, dynamic> tk, PStyle para, TextStyle style) {
  final p = Palette.of(context);
  final uti = tk['uti'] as String? ?? '';
  final alt = tk['alt'] as String? ?? '';
  final size = style.fontSize ?? 17;
  switch (uti) {
    case utiHashtag:
      return Text(alt.isEmpty ? '#' : alt, style: style.copyWith(color: p.accent));
    case utiLink:
      return GestureDetector(
        onTap: () => tc.onLink(tk),
        child: Text(
          alt.isEmpty ? 'Note' : alt,
          style: style.copyWith(color: p.accent, decoration: TextDecoration.underline, decorationColor: p.accent),
        ),
      );
    case utiMath:
      return Text(alt, style: style.copyWith(color: p.accent));
    case utiDivider:
      return SizedBox(
        width: tc.width - 4,
        height: size * 1.3,
        child: Center(child: Container(height: 1, color: p.separator)),
      );
  }
  if (uti.startsWith('com.apple.notes.inlinetextattachment')) return Text(alt, style: style.copyWith(color: p.accent));
  final a = tc.attachments[tk['id']];
  final width = math.max(80.0, tc.width - (para.needsLead ? 40 : 0) - 4);
  if (a != null && a['kind'] == 'table' && a['nd'] != true && tc.table != null) {
    return Padding(padding: const EdgeInsets.symmetric(vertical: 6), child: tc.table!(a, width));
  }
  Widget body;
  if (a == null) {
    body = _card(p, CupertinoIcons.paperclip, 'Attachment', 'Not available', width);
  } else {
    body = _attachment(context, p, a, width, onMenu: tc.onLongPress);
  }
  return Builder(
    builder: (ctx) => GestureDetector(
      onTap: a == null ? null : () => tc.onTap(a),
      onLongPress: a == null ? null : () => tc.onLongPress(ctx, a),
      child: Padding(padding: const EdgeInsets.symmetric(vertical: 6), child: body),
    ),
  );
}

/// A PDF attachment's PDF on disk (the original, else Notes' fallback copy).
String? pdfFile(Map<String, dynamic> a) {
  for (final k in ['file', 'fallback']) {
    final f = a[k] as String?;
    if (f != null && f.toLowerCase().endsWith('.pdf') && File(f).existsSync()) return f;
    if (f != null && k == 'file' && a['kind'] == 'pdf' && File(f).existsSync()) return f;
  }
  return null;
}

String? _file(Map<String, dynamic> a, {bool image = true}) {
  for (final k in ['preview', 'fallback', 'file']) {
    final f = a[k] as String?;
    if (f == null || !File(f).existsSync()) continue;
    if (image && f.toLowerCase().endsWith('.pdf')) continue;
    return f;
  }
  return null;
}

Widget _attachment(BuildContext context, Palette p, Map<String, dynamic> a, double width, {void Function(BuildContext context, Map<String, dynamic> a)? onMenu}) {
  final kind = a['kind'] as String? ?? 'file';
  final view = (a['view'] as num?)?.toInt() ?? 0;
  final name = a['name'] as String? ?? '';
  if (a['nd'] == true) return _card(p, CupertinoIcons.cloud_download, name.isEmpty ? 'Attachment' : name, 'Not downloaded from iCloud', width);
  switch (kind) {
    case 'drawing':
      final d = a['drawing'] as Map?;
      final pic = _file(a);
      final dw = (d?['w'] as num?)?.toDouble() ?? (a['w'] as num?)?.toDouble() ?? 600;
      final dh = (d?['h'] as num?)?.toDouble() ?? (a['h'] as num?)?.toDouble() ?? 400;
      final h = width * (dh <= 0 || dw <= 0 ? 0.6 : dh / dw);
      final strokes = (d?['strokes'] as List?) ?? const [];
      final vector = d != null && (d['shapes'] as num? ?? 0) == 0 && strokes.isNotEmpty;
      return Container(
        width: width,
        height: h.clamp(60, 4000).toDouble(),
        decoration: BoxDecoration(
          color: p.light ? Colors.white : const Color(0xFF1C1C1E),
          borderRadius: BorderRadius.circular(12),
          border: Border.all(color: p.separator, width: 0.5),
        ),
        clipBehavior: Clip.antiAlias,
        child: vector
            ? CustomPaint(painter: InkPainter(strokes.cast<Map<String, dynamic>>(), width / dw, dark: !p.light))
            : pic != null
            ? Image.file(File(pic), fit: BoxFit.contain, width: width)
            : Center(child: Icon(CupertinoIcons.scribble, color: p.tertiary, size: 32)),
      );
    case 'table':
      final rows = ((a['table'] as Map?)?['rows'] as List? ?? const []).map((r) => (r as List).cast<String>()).toList();
      return TableGrid(rows: rows, width: width);
    case 'url':
      return _card(
        p,
        CupertinoIcons.link,
        name.isEmpty ? (a['url'] as String? ?? 'Link') : name,
        Uri.tryParse(a['url'] as String? ?? '')?.host ?? '',
        width,
        picture: _file(a),
      );
    case 'audio':
      return _card(
        p,
        CupertinoIcons.waveform,
        name.isEmpty ? 'Audio' : name,
        'Audio',
        width,
        trailing: Icon(CupertinoIcons.play_circle_fill, size: 34, color: p.accent),
      );
    case 'pdf' when pdfFile(a) != null:
      final f = pdfFile(a)!;
      if (view == 1) return _card(p, CupertinoIcons.doc_text, name.isEmpty ? 'PDF' : name, 'PDF Document', width);
      return Builder(builder: (bctx) => PdfInline(path: f, width: width, medium: view == 2, name: name.isEmpty ? 'PDF' : name, onMenu: onMenu == null ? null : (ctx) => onMenu(ctx, a)));
    case 'image' || 'scan' || 'pdf':
      final parts = (a['parts'] as List?)?.cast<Map<String, dynamic>>() ?? const [];
      final pic = kind == 'scan' && parts.isNotEmpty ? _file(parts.first) : _file(a);
      if (pic == null)
        return _card(
          p,
          kind == 'pdf' ? CupertinoIcons.doc_text : CupertinoIcons.photo,
          name.isEmpty ? (kind == 'pdf' ? 'PDF' : 'Image') : name,
          kind == 'pdf' ? ((a['pages'] as num? ?? 0) > 0 ? '${a['pages']} pages' : 'PDF Document') : '',
          width,
        );
      if (view == 1) return _card(p, CupertinoIcons.photo, name.isEmpty ? 'Image' : name, kind == 'pdf' ? 'PDF Document' : 'Image', width, picture: pic);
      final w = view == 2 ? width / 2 : width;
      final aw = (a['w'] as num?)?.toDouble() ?? 0, ah = (a['h'] as num?)?.toDouble() ?? 0;
      final img = ClipRRect(
        borderRadius: BorderRadius.circular(12),
        child: Image.file(
          File(pic),
          width: w,
          height: aw > 0 && ah > 0 ? w * ah / aw : null,
          fit: BoxFit.cover,
          cacheWidth: (w * MediaQuery.devicePixelRatioOf(context)).round(),
          errorBuilder: (_, _, _) => SizedBox(width: w, height: 80),
        ),
      );
      return kind == 'image'
          ? img
          : Stack(
              children: [
                img,
                if (parts.length > 1 || kind == 'pdf') Positioned(right: 8, bottom: 8, child: _badge(kind == 'pdf' ? 'PDF' : '${parts.length} pages')),
              ],
            );
    default:
      return _card(p, CupertinoIcons.doc, name.isEmpty ? 'File' : name, name.contains('.') ? name.split('.').last.toUpperCase() : 'Document', width);
  }
}

Widget _badge(String t) => Container(
  padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 3),
  decoration: BoxDecoration(color: const Color(0x99000000), borderRadius: BorderRadius.circular(8)),
  child: Text(t, style: Type.caption(Colors.white)),
);

Widget _card(Palette p, IconData icon, String title, String sub, double width, {String? picture, Widget? trailing}) => Container(
  width: width,
  constraints: const BoxConstraints(minHeight: 64),
  padding: const EdgeInsets.all(10),
  decoration: BoxDecoration(color: p.fill, borderRadius: BorderRadius.circular(12)),
  child: Row(
    children: [
      Container(
        width: 46,
        height: 46,
        decoration: BoxDecoration(color: p.card, borderRadius: BorderRadius.circular(8)),
        clipBehavior: Clip.antiAlias,
        child: picture != null ? Image.file(File(picture), fit: BoxFit.cover, cacheWidth: 140) : Icon(icon, color: p.secondary, size: 24),
      ),
      const SizedBox(width: 12),
      Expanded(
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Text(
              title,
              maxLines: 1,
              overflow: TextOverflow.ellipsis,
              style: Type.text(15, p.label, weight: FontWeight.w600),
            ),
            if (sub.isNotEmpty) Text(sub, maxLines: 1, overflow: TextOverflow.ellipsis, style: Type.footnote(p.secondary)),
          ],
        ),
      ),
      ?trailing,
    ],
  ),
);

/// A table as Notes draws it: a hairline grid, no header shading, cells about 44 tall.
class TableGrid extends StatelessWidget {
  const TableGrid({super.key, required this.rows, required this.width});
  final List<List<String>> rows;
  final double width;
  @override
  Widget build(BuildContext context) {
    final p = Palette.of(context);
    final cols = rows.fold<int>(0, (m, r) => math.max(m, r.length));
    if (cols == 0) return SizedBox(width: width, height: 44);
    final colW = math.max(90.0, width / cols);
    final grid = Table(
      defaultColumnWidth: FixedColumnWidth(colW),
      border: TableBorder.all(color: p.separator, width: 0.8),
      children: [
        for (final r in rows)
          TableRow(
            children: [
              for (var c = 0; c < cols; c++)
                Container(
                  constraints: const BoxConstraints(minHeight: 44),
                  padding: const EdgeInsets.symmetric(horizontal: 10, vertical: 10),
                  child: Text(c < r.length ? r[c] : '', style: Type.body(p.label)),
                ),
            ],
          ),
      ],
    );
    return SizedBox(
      width: width,
      child: SingleChildScrollView(scrollDirection: Axis.horizontal, child: grid),
    );
  }
}
