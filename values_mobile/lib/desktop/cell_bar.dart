// The bar at the foot of the sheet (app.rs cell_bar): the selected cell's reference with its
// formula and value (and comment), or several cells' quick sums (SUM, AVG, MIN, MAX, COUNT), or
// the engine's message. A tap on a single cell's bar edits the cell.
import 'package:flutter/material.dart';

import 'look.dart';

class CellBar extends StatelessWidget {
  const CellBar({super.key, required this.s, required this.look, this.message, required this.onEdit});

  /// vl_status "cellBar".
  final Map<String, dynamic>? s;
  final Look look;
  final String? message;
  final VoidCallback onEdit;

  static const height = 36.0;

  @override
  Widget build(BuildContext context) {
    final dim = Look.font(13, look.textDim);
    final text = Look.font(13, look.text);
    final kids = <Widget>[];
    final s = this.s;
    if (message != null) {
      kids.add(Expanded(child: Text(message!, maxLines: 1, overflow: TextOverflow.ellipsis, style: dim)));
    } else if (s != null && s['sums'] != null) {
      for (final p in s['sums'] as List) {
        kids.addAll([Text('${p[0]}', style: dim), const SizedBox(width: 6), Text('${p[1]}', style: text), const SizedBox(width: 16)]);
      }
    } else if (s != null && s['count'] != null) {
      kids.add(Text('${s['count']} cells', style: dim));
    } else if (s != null && s['ref'] != null) {
      final formula = s['formula'] as String?;
      kids.add(Container(
        padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 2),
        decoration: BoxDecoration(color: look.control, borderRadius: BorderRadius.circular(5)),
        child: Text('${s['ref']}', style: Look.font(12.5, look.text, bold: true)),
      ));
      kids.add(const SizedBox(width: 10));
      if (s['comment'] != null) {
        kids.addAll([Text('Comment', style: Look.font(13, const Color(0xFFB08A00))), const SizedBox(width: 6), Flexible(child: Text('${s['comment']}', maxLines: 1, overflow: TextOverflow.ellipsis, style: text)), const SizedBox(width: 12)]);
      }
      kids.add(Text(formula != null ? 'Formula' : 'Value', style: dim));
      kids.add(const SizedBox(width: 8));
      kids.add(Expanded(
        child: Text.rich(
          TextSpan(children: [
            TextSpan(text: formula ?? '${s['value']}', style: text),
            if (formula != null) TextSpan(text: '      ${s['value']}', style: dim),
          ]),
          maxLines: 1,
          overflow: TextOverflow.ellipsis,
        ),
      ));
    } else if (s != null && s['kind'] != null) {
      kids.addAll([Text('${s['kind']}', style: dim), const SizedBox(width: 8), Expanded(child: Text('${s['text']}', maxLines: 1, overflow: TextOverflow.ellipsis, style: text))]);
    }
    return GestureDetector(
      behavior: HitTestBehavior.opaque,
      onTap: s != null && s['ref'] != null ? onEdit : null,
      child: Container(
        height: height,
        padding: const EdgeInsets.symmetric(horizontal: 12),
        decoration: BoxDecoration(color: look.panel, border: Border(top: BorderSide(color: look.separator))),
        child: Row(children: kids),
      ),
    );
  }
}
