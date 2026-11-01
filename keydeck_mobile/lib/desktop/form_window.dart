// A window the engine describes row by row (kn-app dlgapi.rs forms: Link, Go To Slide, Find &
// Replace, Save Theme, Export options and the other Keydeck windows Letters has no window for),
// drawn with the suite's controls. Every change goes to the engine at once
// ("dlg.<window>.<key>" with the value), buttons with null.
//
// Rows: {t: label, text, dim?} | {t: text, key, value, hint?, rows?, password?, enter?} |
// {t: radio, key, options, value} | {t: seg, key, options, value} | {t: check, key, label, value,
// enabled?} | {t: number, key, label?, value, min, max, step?, decimals?, suffix?} |
// {t: choice, key, label?, options, value} | {t: list, key, items, value?, height?} |
// {t: buttons, items: [[label, key, enabled?]]} | {t: section, text} | {t: separator}
import 'package:flutter/material.dart';

import 'look.dart';
import 'widgets.dart';
import 'windows.dart' show DTextField;

class FormBody extends StatefulWidget {
  const FormBody({super.key, required this.w, required this.set});
  final Map<String, dynamic> w;
  final void Function(String key, [Object? value]) set;

  @override
  State<FormBody> createState() => _FormBodyState();
}

class _FormBodyState extends State<FormBody> {
  final _fields = <String, TextEditingController>{};

  @override
  void dispose() {
    for (final c in _fields.values) {
      c.dispose();
    }
    super.dispose();
  }

  TextEditingController _field(String key, String value) {
    final c = _fields.putIfAbsent(key, () => TextEditingController(text: value));
    // the engine changed it (not what's being typed)
    if (c.text != value && !_typing.contains(key)) c.text = value;
    return c;
  }

  final _typing = <String>{};

  Widget _row(Map<String, dynamic> r, Look look) {
    final key = r['key'] as String? ?? '';
    switch (r['t']) {
      case 'label':
        return DLabel(r['text'] as String? ?? '', dim: r['dim'] == true, bold: r['bold'] == true);
      case 'section':
        return DSectionTitle(r['text'] as String? ?? '');
      case 'separator':
        return const DSeparator();
      case 'text':
        final c = _field(key, r['value'] as String? ?? '');
        return DTextField(
          controller: c,
          hint: r['hint'] as String?,
          rows: (r['rows'] as num?)?.toInt() ?? 1,
          width: (r['width'] as num?)?.toDouble() ?? 300,
          password: r['password'] == true,
          autofocus: r['focus'] == true,
          onChanged: (t) {
            _typing.add(key);
            widget.set(key, t);
          },
          onSubmitted: r['enter'] is String ? () => widget.set(r['enter'] as String) : null,
        );
      case 'radio':
        final opts = (r['options'] as List).cast<String>();
        final cur = (r['value'] as num?)?.toInt() ?? -1;
        return Column(crossAxisAlignment: CrossAxisAlignment.start, children: [
          for (final (i, o) in opts.indexed) DRadioRow(on: i == cur, label: o, onTap: () => widget.set(key, i)),
        ]);
      case 'seg':
        final opts = (r['options'] as List).cast<String>();
        return DSegmented(labels: opts, cur: (r['value'] as num?)?.toInt() ?? 0, onPick: (i) => widget.set(key, i));
      case 'check':
        return DCheckbox(on: r['value'] == true, label: r['label'] as String? ?? '', enabled: r['enabled'] != false, onChanged: (v) => widget.set(key, v));
      case 'number':
        final stepper = DStepper(
          value: (r['value'] as num?)?.toDouble() ?? 0,
          min: (r['min'] as num?)?.toDouble() ?? -1e9,
          max: (r['max'] as num?)?.toDouble() ?? 1e9,
          step: (r['step'] as num?)?.toDouble() ?? 1,
          decimals: (r['decimals'] as num?)?.toInt() ?? 0,
          suffix: r['suffix'] as String? ?? '',
          onChanged: (v) => widget.set(key, v),
        );
        final label = r['label'] as String?;
        return label == null ? stepper : Row(mainAxisSize: MainAxisSize.min, children: [DLabel(label), const SizedBox(width: 8), stepper]);
      case 'choice':
        final opts = (r['options'] as List).cast<String>();
        final cur = (r['value'] as num?)?.toInt() ?? 0;
        final combo = DCombo<int>(
          text: cur >= 0 && cur < opts.length ? opts[cur] : '',
          items: () => [for (final (i, o) in opts.indexed) DChoice(o, i, on: i == cur)],
          onPick: (i) => widget.set(key, i),
          width: (r['width'] as num?)?.toDouble() ?? 200,
        );
        final label = r['label'] as String?;
        return label == null ? combo : Row(mainAxisSize: MainAxisSize.min, children: [DLabel(label), const SizedBox(width: 8), combo]);
      case 'list':
        final items = (r['items'] as List).cast<String>();
        final cur = (r['value'] as num?)?.toInt();
        return Container(
          height: (r['height'] as num?)?.toDouble() ?? 200,
          width: (r['width'] as num?)?.toDouble() ?? 320,
          decoration: BoxDecoration(color: look.field, borderRadius: BorderRadius.circular(6), border: Border.all(color: look.controlStroke)),
          child: ListView.builder(
            itemCount: items.length,
            itemBuilder: (ctx, i) => GestureDetector(
              behavior: HitTestBehavior.opaque,
              onTap: () => widget.set(key, i),
              child: Container(
                color: i == cur ? look.accent : null,
                padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 6),
                child: Text(items[i], style: Look.font(13, i == cur ? Colors.white : look.text), maxLines: 2, overflow: TextOverflow.ellipsis),
              ),
            ),
          ),
        );
      case 'buttons':
        return Wrap(spacing: 8, runSpacing: 6, children: [
          for (final b in (r['items'] as List).cast<List>())
            DButton(
              onTap: b.length > 2 && b[2] == false ? null : () => widget.set(b[1] as String),
              child: Text(b[0] as String),
            ),
        ]);
    }
    return const SizedBox.shrink();
  }

  @override
  Widget build(BuildContext context) {
    final look = LookScope.of(context);
    final rows = (widget.w['form'] as List).cast<Map<String, dynamic>>();
    return ConstrainedBox(
      constraints: const BoxConstraints(maxWidth: 520),
      child: EguiColumn(children: [for (final r in rows) _row(r, look)]),
    );
  }
}

/// A radio button row (egui radio_value; a ticked row on touch).
class DRadioRow extends StatelessWidget {
  const DRadioRow({super.key, required this.on, required this.label, required this.onTap});
  final bool on;
  final String label;
  final VoidCallback onTap;
  @override
  Widget build(BuildContext context) {
    final look = LookScope.of(context);
    return GestureDetector(
      behavior: HitTestBehavior.opaque,
      onTap: onTap,
      child: SizedBox(
        height: TouchScope.of(context) ? 36 : 24,
        child: Row(mainAxisSize: MainAxisSize.min, children: [
          Container(
            width: 15,
            height: 15,
            decoration: BoxDecoration(color: look.control, shape: BoxShape.circle, border: Border.all(color: look.light ? look.controlStroke : look.gray(0x5a))),
            alignment: Alignment.center,
            child: on ? Container(width: 5, height: 5, decoration: BoxDecoration(color: look.text, shape: BoxShape.circle)) : null,
          ),
          const SizedBox(width: 6),
          Text(label, style: Look.font(13, look.text)),
        ]),
      ),
    );
  }
}
