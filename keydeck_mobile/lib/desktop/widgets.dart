// The desktop inspector's controls, drawn as kn_ui and egui draw them (theme.rs: combo,
// checkbox, stepper, disclosure, toggle; objinsp.rs: tabs, seg_small, icon_segments, section;
// colors.rs: color_well), at the desktop's sizes: 24-point controls, 6-point corners, 13-point
// text, 6 points between the items of a column.
import 'dart:math' as math;

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';

import 'colors.dart';
import 'glass.dart';
import 'icons.dart';
import 'look.dart';
import 'touch_menu.dart';

/// The look, for the controls below it.
class LookScope extends InheritedWidget {
  const LookScope({super.key, required this.look, required super.child});
  final Look look;
  static Look of(BuildContext context) => context.dependOnInheritedWidgetOfExactType<LookScope>()!.look;
  @override
  bool updateShouldNotify(LookScope old) => old.look.light != look.light || old.look.accent != look.accent || old.look.glass != look.glass || old.look.translucent != look.translucent || old.look.panel != look.panel;
}

/// The touch inspector (iPad Pages' Format panel): its controls take their touch shapes
/// (50-point rows, pill steppers, switches) and its columns group into rounded cards. Outside
/// it (dialogs, bars) the controls keep the desktop's shapes.
class TouchScope extends InheritedWidget {
  const TouchScope({super.key, required super.child});
  static bool of(BuildContext context) => context.dependOnInheritedWidgetOfExactType<TouchScope>() != null;
  @override
  bool updateShouldNotify(TouchScope old) => false;
}

/// Inside a card: a nested column lays out plain rows, not cards of its own.
class _InCard extends InheritedWidget {
  const _InCard({required super.child});
  static bool of(BuildContext context) => context.getInheritedWidgetOfExactType<_InCard>() != null;
  @override
  bool updateShouldNotify(_InCard old) => false;
}

/// The touch inspector's sizes: a row, a card's corners, a card's inner margin, the gap
/// between cards.
const kTouchRow = 50.0, kTouchRadius = 18.0, kTouchPad = 14.0, kTouchGap = 12.0;

/// The touch inspector's colours: the pane behind the cards, a card, a control's fill on a card.
Color touchBackground(Look look) => look.light ? Color.lerp(look.panel, Colors.black, 0.05)! : look.panel;
Color touchCard(Look look) => look.light ? Color.lerp(look.panel, Colors.white, 0.85)! : Color.lerp(look.panel, Colors.white, 0.08)!;
Color touchFill(Look look) => look.light ? Color.lerp(touchCard(look), Colors.black, 0.075)! : Color.lerp(touchCard(look), Colors.white, 0.10)!;

/// A rounded card of rows, a hairline between them. `tight`: a segmented row alone in its
/// card (the card is its track, the segments nearly to its edges).
class DCard extends StatelessWidget {
  const DCard({super.key, required this.children, this.tight = false});
  final List<Widget> children;
  final bool tight;
  @override
  Widget build(BuildContext context) {
    final look = LookScope.of(context);
    return Container(
      decoration: BoxDecoration(color: touchCard(look), borderRadius: BorderRadius.circular(kTouchRadius)),
      padding: EdgeInsets.symmetric(horizontal: tight ? 4 : kTouchPad),
      child: _InCard(child: _TouchRows(children: children)),
    );
  }
}

class _TouchRows extends StatelessWidget {
  const _TouchRows({required this.children});
  final List<Widget> children;
  @override
  Widget build(BuildContext context) {
    final look = LookScope.of(context);
    return Column(crossAxisAlignment: CrossAxisAlignment.stretch, mainAxisSize: MainAxisSize.min, children: [
      for (final (i, c) in children.indexed) ...[
        if (i > 0) Container(height: 0.7, color: look.separator),
        // (each row at least a row tall, its content centred in it and as wide as the card)
        ConstrainedBox(
          constraints: const BoxConstraints(minHeight: kTouchRow),
          child: Padding(padding: const EdgeInsets.symmetric(vertical: 3), child: Row(children: [Expanded(child: c)])),
        ),
      ],
    ]);
  }
}

/// A section's title: over its card in the touch inspector, bold in the column on the desktop.
class DSectionTitle extends StatelessWidget {
  const DSectionTitle(this.title, {super.key, this.size = 14, this.bold = true});
  final String title;
  final double size;
  final bool bold;
  @override
  Widget build(BuildContext context) {
    if (!TouchScope.of(context)) return DLabel(title, size: size, bold: bold);
    final look = LookScope.of(context);
    if (_InCard.of(context)) return Text(title, maxLines: 1, overflow: TextOverflow.ellipsis, style: Look.font(16, look.text, bold: true));
    return Padding(
      padding: const EdgeInsets.fromLTRB(kTouchPad, 0, kTouchPad, 7),
      child: Text(title, maxLines: 1, overflow: TextOverflow.ellipsis, style: Look.font(14, look.textDim).copyWith(height: 1.2)),
    );
  }
}

/// A small dim note (9-point on the desktop): under its card in the touch inspector.
class DNote extends StatelessWidget {
  const DNote(this.text, {super.key, this.align = TextAlign.start});
  final String text;
  final TextAlign align;
  @override
  Widget build(BuildContext context) {
    final look = LookScope.of(context);
    if (!TouchScope.of(context)) return Text(text, textAlign: align, style: Look.font(9, look.textDim));
    return Padding(
      padding: EdgeInsets.fromLTRB(kTouchPad, 6, kTouchPad, _InCard.of(context) ? 6 : 0),
      child: Text(text, textAlign: align, style: Look.font(12.5, look.textDim).copyWith(height: 1.25)),
    );
  }
}

/// egui's vertical layout: 6 points after each item (an explicit space is just itself).
/// In the touch inspector: cards of rows, a new card after each separator, section title
/// or disclosure, the titles and notes between the cards.
class EguiColumn extends StatelessWidget {
  const EguiColumn({super.key, required this.children});
  final List<Widget> children;

  Widget _touch(BuildContext context) {
    final rows = [for (final c in children) if (c is! DSpace && c is! DSeparator) c];
    if (_InCard.of(context)) return _TouchRows(children: rows);
    final out = <Widget>[];
    var card = <Widget>[];
    // (a gap before a card or a title unless a title is just over it)
    void gap() {
      if (out.isNotEmpty && out.last is! DSectionTitle) out.add(const SizedBox(height: kTouchGap));
    }

    void close() {
      if (card.isEmpty) return;
      gap();
      out.add(DCard(tight: card.length == 1 && (card.first is DIconSegments || card.first is DSegmented), children: card));
      card = [];
    }

    for (final c in children) {
      if (c is DSpace) continue;
      if (c is DSeparator) {
        close();
      } else if (c is DSectionTitle) {
        close();
        if (out.isNotEmpty) out.add(const SizedBox(height: 10));
        out.add(c);
      } else if (c is DNote) {
        close();
        out.add(c);
      } else if (c is DDisclosure) {
        close();
        card.add(c);
      } else {
        card.add(c);
      }
    }
    close();
    // (room under the last card)
    out.add(const SizedBox(height: 16));
    return Column(crossAxisAlignment: CrossAxisAlignment.stretch, mainAxisSize: MainAxisSize.min, children: out);
  }

  @override
  Widget build(BuildContext context) {
    if (TouchScope.of(context)) return _touch(context);
    // (a negative space takes from the gap after the item before it: add_space(4) after a
    // control that already left 6)
    final out = <Widget>[];
    var gap = 0.0;
    void flush() {
      if (gap > 0) out.add(SizedBox(height: gap));
      gap = 0;
    }

    for (final c in children) {
      if (c is DSpace) {
        gap = math.max(0, gap + c.h);
        continue;
      }
      flush();
      out.add(c);
      gap = 6;
    }
    // (no spacing after the last item: egui leaves it out of a nested ui's size; an explicit
    // add_space at the end stays)
    gap = math.max(0, gap - 6);
    flush();
    return Column(crossAxisAlignment: CrossAxisAlignment.stretch, mainAxisSize: MainAxisSize.min, children: out);
  }
}

/// ui.add_space.
class DSpace extends StatelessWidget {
  const DSpace(this.h, {super.key});
  final double h;
  @override
  Widget build(BuildContext context) => SizedBox(height: math.max(0, h));
}

/// ui.separator(): a hairline in 6 points.
class DSeparator extends StatelessWidget {
  const DSeparator({super.key});
  @override
  Widget build(BuildContext context) => SizedBox(height: 6, child: Center(child: Container(height: 1, color: LookScope.of(context).separator)));
}

/// objinsp::section: a space, a line, the title in bold.
List<Widget> dSection(String title) => [const DSpace(6), const DSeparator(), DSectionTitle(title), const DSpace(2)];

class DLabel extends StatelessWidget {
  const DLabel(this.text, {super.key, this.size = 13, this.bold = false, this.dim = false, this.color});
  final String text;
  final double size;
  final bool bold, dim;
  final Color? color;
  @override
  Widget build(BuildContext context) {
    final look = LookScope.of(context);
    final sz = TouchScope.of(context) ? size + 3 : size;
    return Text(text, maxLines: 1, overflow: TextOverflow.ellipsis, style: Look.font(sz, color ?? (dim ? look.textDim : look.text), bold: bold).copyWith(height: 1.3));
  }
}

/// textinsp row: a label at the left, the control at the right.
class DRow extends StatelessWidget {
  const DRow(this.label, this.trailing, {super.key, this.enabled = true});
  final String label;
  final Widget trailing;
  final bool enabled;
  @override
  Widget build(BuildContext context) => Opacity(
        opacity: enabled ? 1 : 0.5,
        child: IgnorePointer(
          ignoring: !enabled,
          child: SizedBox(
            height: TouchScope.of(context) ? 44 : 26,
            child: Row(children: [Expanded(child: DLabel(label)), if (TouchScope.of(context)) const SizedBox(width: 8), trailing]),
          ),
        ),
      );
}

/// A pressable area with the desktop's hover look while a finger is on it.
class _Press extends StatefulWidget {
  const _Press({required this.onTap, required this.builder});
  final VoidCallback? onTap;
  final Widget Function(bool down) builder;
  @override
  State<_Press> createState() => _PressState();
}

class _PressState extends State<_Press> {
  bool _down = false;
  @override
  Widget build(BuildContext context) => GestureDetector(
        behavior: HitTestBehavior.opaque,
        onTapDown: widget.onTap == null ? null : (_) => setState(() => _down = true),
        onTapCancel: () => setState(() => _down = false),
        onTapUp: widget.onTap == null
            ? null
            : (_) {
                setState(() => _down = false);
                widget.onTap!();
              },
        child: widget.builder(_down),
      );
}

/// egui's Button in kn_ui's look: the control fill, 6-point corners, 24 tall.
class DButton extends StatelessWidget {
  const DButton({super.key, required this.child, this.onTap, this.width, this.height = 24, this.radius = 6, this.fill});
  final Widget child;
  final VoidCallback? onTap;
  final double? width;
  final double height, radius;
  final Color? fill;
  @override
  Widget build(BuildContext context) {
    final look = LookScope.of(context);
    if (TouchScope.of(context)) {
      // (touch: at least 44 × 40, rounder, larger text; in a card with no fill of its own it
      // has none, its text in the accent colour, as iPad's action rows)
      final plain = fill == null && _InCard.of(context);
      return Opacity(
        opacity: onTap == null ? 0.5 : 1,
        child: _Press(
          onTap: onTap,
          builder: (down) => Container(
            width: width == null ? null : math.max(width!, 44),
            height: math.max(height, 40),
            constraints: const BoxConstraints(minWidth: 44),
            padding: const EdgeInsets.symmetric(horizontal: 12),
            decoration: BoxDecoration(color: down ? look.hover : (plain ? null : (fill ?? touchFill(look))), borderRadius: BorderRadius.circular(12)),
            child: Center(widthFactor: width == null ? 1 : null, child: DefaultTextStyle(style: Look.font(16, plain ? look.accent : look.text), child: IconTheme(data: IconThemeData(color: plain ? look.accent : look.text), child: child))),
          ),
        ),
      );
    }
    // (disabled: faded, as egui fades a disabled widget)
    return Opacity(
      opacity: onTap == null ? 0.5 : 1,
      child: _Press(
      onTap: onTap,
      builder: (down) => Container(
        width: width,
        height: height,
        padding: const EdgeInsets.symmetric(horizontal: 10),
        decoration: BoxDecoration(
          color: down ? look.hover : (fill ?? look.control),
          borderRadius: BorderRadius.circular(radius),
          border: Border.all(color: look.controlStroke),
        ),
        // (as wide as its text unless given a width)
        child: Center(widthFactor: width == null ? 1 : null, child: DefaultTextStyle(style: Look.font(13, look.text), child: child)),
      ),
    ));
  }
}

/// The ⌃⌄ chevrons of a pop-up button (theme::combo's icon).
class _Chevrons extends CustomPainter {
  _Chevrons(this.color);
  final Color color;
  @override
  void paint(Canvas canvas, Size size) {
    final c = size.center(Offset.zero);
    final p = Paint()
      ..style = PaintingStyle.stroke
      ..strokeWidth = 1.4
      ..color = color;
    const w = 3.5, h = 2.5;
    canvas.drawPath(Path()..addPolygon([Offset(c.dx - w, c.dy - 1.5), Offset(c.dx, c.dy - 1.5 - h), Offset(c.dx + w, c.dy - 1.5)], false), p);
    canvas.drawPath(Path()..addPolygon([Offset(c.dx - w, c.dy + 1.5), Offset(c.dx, c.dy + 1.5 + h), Offset(c.dx + w, c.dy + 1.5)], false), p);
  }

  @override
  bool shouldRepaint(_Chevrons o) => o.color != color;
}

/// The touch rows' chevron: › (or ⌄ when `down`).
class TouchChevron extends CustomPainter {
  TouchChevron(this.color, {this.down = false});
  final Color color;
  final bool down;
  @override
  void paint(Canvas canvas, Size size) {
    final c = size.center(Offset.zero);
    final p = Paint()
      ..style = PaintingStyle.stroke
      ..strokeWidth = 2
      ..strokeCap = StrokeCap.round
      ..strokeJoin = StrokeJoin.round
      ..color = color;
    final pts = down ? [c + const Offset(-5, -2.5), c + const Offset(0, 2.5), c + const Offset(5, -2.5)] : [c + const Offset(-2.5, -5), c + const Offset(2.5, 0), c + const Offset(-2.5, 5)];
    canvas.drawPath(Path()..addPolygon(pts, false), p);
  }

  @override
  bool shouldRepaint(TouchChevron o) => o.color != color || o.down != down;
}

/// A choice in a pop-up: its label, whether it's the current one, its value.
class DChoice<T> {
  const DChoice(this.label, this.value, {this.on = false, this.style});
  final String label;
  final T value;
  final bool on;
  final TextStyle? style;
}

/// Opens a list under `anchor` as a touch menu (touch_menu.dart: a rounded panel, 44-point
/// rows, the current one ticked); the value picked.
/// `search`: a field over the list; typing narrows it (names starting with what's typed first),
/// Return picks the first one left, or what's typed as `typed` makes it when nothing's left.
Future<T?> showDesktopList<T>(BuildContext context, Rect anchor, List<DChoice<T>> items, {double? width, double maxHeight = 400, Widget? header, bool search = false, T Function(String text)? typed}) {
  final all = items;
  final query = ValueNotifier('');
  final look = LookScope.of(context);
  final overlay = Overlay.of(context, rootOverlay: true).context.findRenderObject() as RenderBox?;
  final at = overlay == null ? anchor : Rect.fromPoints(overlay.globalToLocal(anchor.topLeft), overlay.globalToLocal(anchor.bottomRight));
  final ticks = items.any((it) => it.on);
  return showGeneralDialog<T>(
    context: context,
    useRootNavigator: true,
    barrierDismissible: true,
    barrierLabel: 'Close list',
    barrierColor: Colors.transparent,
    transitionDuration: const Duration(milliseconds: 140),
    transitionBuilder: (ctx, a, _, child) => FadeTransition(opacity: CurvedAnimation(parent: a, curve: Curves.easeOut), child: child),
    pageBuilder: (ctx, _, _) {
      final mq = MediaQuery.of(ctx);
      final screen = mq.size;
      final w = math.min(math.max(width ?? at.width, TouchMenu.minWidth), screen.width - 16);
      final want = all.length * TouchMenu.row + TouchMenu.pad * 2 + (header == null ? 0 : 34) + (search ? 46 : 0);
      // (above the on-screen keyboard, when the search field brings it up)
      final below = screen.height - math.max(mq.padding.bottom, mq.viewInsets.bottom) - 8 - at.bottom - 6;
      final above = at.top - 6 - mq.padding.top - 8;
      // (taller than the list wants only when the caller asked for more room)
      final h = math.min(math.max(maxHeight, 5 * TouchMenu.row), math.min(want, math.max(below, above)));
      final top = h <= below || below >= above ? at.bottom + 6 : at.top - 6 - h;
      final left = at.left.clamp(8.0, math.max(8.0, screen.width - w - 8));
      return Stack(children: [
        Positioned(
          left: left.toDouble(),
          top: top,
          width: w,
          height: h,
          child: LookScope(
            look: look,
            child: Material(
              type: MaterialType.transparency,
              child: Container(
                decoration: TouchMenu.panel(look),
                child: ClipRRect(
                  borderRadius: BorderRadius.circular(TouchMenu.radius),
                  child: Padding(
                    padding: const EdgeInsets.all(TouchMenu.pad),
                    child: Column(children: [
                      ?header,
                      if (search)
                        Padding(
                          padding: const EdgeInsets.only(bottom: 6),
                          child: _SearchField(
                            look: look,
                            onChanged: (t) => query.value = t,
                            onSubmitted: (t) {
                              final left = _narrowed(all, t);
                              if (left.isNotEmpty) return Navigator.of(ctx).pop(left.first.value);
                              if (typed != null && t.trim().isNotEmpty) Navigator.of(ctx).pop(typed(t.trim()));
                            },
                          ),
                        ),
                      Expanded(
                        child: ValueListenableBuilder<String>(
                          valueListenable: query,
                          builder: (ctx2, q, _) => ListView(padding: EdgeInsets.zero, children: [
                          for (final it in _narrowed(all, q))
                            _Press(
                              onTap: () => Navigator.of(ctx).pop(it.value),
                              builder: (down) => Container(
                                height: TouchMenu.row,
                                padding: const EdgeInsets.symmetric(horizontal: 12),
                                decoration: BoxDecoration(color: down ? TouchMenu.highlight(look) : null, borderRadius: BorderRadius.circular(TouchMenu.rowRadius)),
                                child: Row(children: [
                                  if (ticks) SizedBox(width: 36, child: it.on ? Align(alignment: Alignment.centerLeft, child: Icon(Icons.check_rounded, size: 22, color: look.text)) : null),
                                  // (a font list draws each name in its font, at the touch size)
                                  Expanded(child: Text(it.label, maxLines: 1, overflow: TextOverflow.ellipsis, style: (it.style ?? TouchMenu.label(look)).copyWith(color: look.text, fontSize: 17))),
                                ]),
                              ),
                            ),
                          ]),
                        ),
                      ),
                    ]),
                  ),
                ),
              ),
            ),
          ),
        ),
      ]);
    },
  );
}

/// `items` whose labels hold `q` (any case), those starting with it first.
List<DChoice<T>> _narrowed<T>(List<DChoice<T>> items, String q) {
  final t = q.trim().toLowerCase();
  if (t.isEmpty) return items;
  final hits = [for (final it in items) if (it.label.toLowerCase().contains(t)) it];
  return [...hits.where((it) => it.label.toLowerCase().startsWith(t)), ...hits.where((it) => !it.label.toLowerCase().startsWith(t))];
}

/// A list's search field: a rounded fill, a magnifier, the keyboard up at once.
class _SearchField extends StatelessWidget {
  const _SearchField({required this.look, required this.onChanged, required this.onSubmitted});
  final Look look;
  final void Function(String) onChanged, onSubmitted;
  @override
  Widget build(BuildContext context) => Container(
        height: 40,
        padding: const EdgeInsets.symmetric(horizontal: 10),
        decoration: BoxDecoration(color: look.text.withValues(alpha: look.light ? 0.06 : 0.10), borderRadius: BorderRadius.circular(TouchMenu.rowRadius)),
        child: Row(children: [
          Icon(Icons.search_rounded, size: 20, color: look.textDim),
          const SizedBox(width: 6),
          Expanded(
            child: TextField(
              autofocus: true,
              onChanged: onChanged,
              onSubmitted: onSubmitted,
              textInputAction: TextInputAction.done,
              cursorColor: look.accent,
              style: Look.font(16, look.text),
              decoration: InputDecoration(isDense: true, border: InputBorder.none, hintText: 'Search', hintStyle: Look.font(16, look.textDim)),
            ),
          ),
        ]),
      );
}

Rect _rectOf(BuildContext context) {
  final box = context.findRenderObject() as RenderBox;
  return box.localToGlobal(Offset.zero) & box.size;
}

/// theme::combo: a pop-up button showing the current choice, its list under it.
class DCombo<T> extends StatelessWidget {
  const DCombo({super.key, required this.text, required this.items, required this.onPick, this.width, this.listWidth, this.maxHeight = 400, this.search = false, this.typed});
  final String text;
  final List<DChoice<T>> Function() items;
  final void Function(T value) onPick;

  /// A field over the list to type in (a font's name): see showDesktopList.
  final bool search;
  final T Function(String text)? typed;
  final double? width, listWidth;
  final double maxHeight;
  @override
  Widget build(BuildContext context) {
    final look = LookScope.of(context);
    if (TouchScope.of(context)) {
      // touch: a row's value and a chevron (iPad Pages' Format rows), no box; given a width
      // it's the value at a row's right, dim
      return Builder(
        builder: (ctx) => _Press(
          onTap: () async {
            final v = await showDesktopList<T>(ctx, _rectOf(ctx), items(), width: listWidth ?? math.max(_rectOf(ctx).width, 180), maxHeight: maxHeight, search: search, typed: typed);
            if (v != null) onPick(v);
          },
          builder: (down) => Opacity(
            opacity: down ? 0.5 : 1,
            child: SizedBox(
              width: width,
              height: 44,
              child: Row(children: [
                Expanded(child: Text(text, maxLines: 1, overflow: TextOverflow.ellipsis, textAlign: width == null ? TextAlign.start : TextAlign.end, style: Look.font(16, width == null ? look.text : look.textDim))),
                const SizedBox(width: 8),
                CustomPaint(size: const Size(8, 14), painter: TouchChevron(look.textDim)),
              ]),
            ),
          ),
        ),
      );
    }
    return Builder(
      builder: (ctx) => _Press(
        onTap: () async {
          final v = await showDesktopList<T>(ctx, _rectOf(ctx), items(), width: listWidth, maxHeight: maxHeight, search: search, typed: typed);
          if (v != null) onPick(v);
        },
        builder: (down) => Container(
          width: width,
          height: 24,
          padding: const EdgeInsets.only(left: 8, right: 4),
          decoration: BoxDecoration(color: down ? look.hover : look.control, borderRadius: BorderRadius.circular(6), border: Border.all(color: look.controlStroke)),
          child: Row(children: [
            Expanded(child: Text(text, maxLines: 1, overflow: TextOverflow.ellipsis, style: Look.font(13, look.text))),
            SizedBox(width: 16, height: 24, child: CustomPaint(painter: _Chevrons(look.text))),
          ]),
        ),
      ),
    );
  }
}

/// objinsp segmented: tabs (radius 16) or seg_small (7), 32 tall, then 10 points.
class DSegmented extends StatelessWidget {
  const DSegmented({super.key, required this.labels, required this.cur, required this.onPick, this.radius = 7, this.after = 10});
  final List<String> labels;
  final int cur;
  final void Function(int) onPick;
  final double radius;

  /// The space segmented() leaves after itself (0 in a row: there it's room at the right).
  final double after;
  Widget _touch(BuildContext context, Look look) {
    // touch: 40 tall, the tabs a full pill, seg_small 12-point corners; no space after it in a card
    final r = radius >= 12 ? 20.0 : 12.0;
    final glass = look.glass && radius >= 12;
    return Column(mainAxisSize: MainAxisSize.min, children: [
      CustomPaint(
        painter: glass ? _FnPainter((c, s) => paintGlass(c, Offset.zero & s, r, look)) : null,
        child: Container(
          height: 40,
          // (in a card the card is the track: only the chosen segment is filled)
          decoration: glass || _InCard.of(context) ? null : BoxDecoration(color: touchFill(look), borderRadius: BorderRadius.circular(r)),
          child: Row(children: [
            for (var i = 0; i < labels.length; i++)
              Expanded(
                child: _Press(
                  onTap: () => onPick(i),
                  builder: (down) {
                    final sel = cur == i;
                    return Container(
                      margin: const EdgeInsets.all(3),
                      decoration: BoxDecoration(color: sel ? look.accent : (down ? look.hover : null), borderRadius: BorderRadius.circular(r - 3)),
                      alignment: Alignment.center,
                      child: Text(labels[i], maxLines: 1, overflow: TextOverflow.ellipsis, style: Look.font(15, sel ? Colors.white : look.text)),
                    );
                  },
                ),
              ),
          ]),
        ),
      ),
      SizedBox(height: _InCard.of(context) ? 0 : math.min(after, 12)),
    ]);
  }

  @override
  Widget build(BuildContext context) {
    final look = LookScope.of(context);
    if (TouchScope.of(context)) return _touch(context, look);
    return Column(mainAxisSize: MainAxisSize.min, children: [
      CustomPaint(
        painter: look.glass && radius >= 12 ? _FnPainter((c, s) => paintGlass(c, Offset.zero & s, radius, look)) : null,
        child: Container(
        height: 32,
        // (the inspector's tabs on liquid glass: no lens, the panel under them is plain)
        decoration: look.glass && radius >= 12 ? null : BoxDecoration(color: look.gray(0x33), borderRadius: BorderRadius.circular(radius)),
        child: Row(children: [
          for (var i = 0; i < labels.length; i++)
            Expanded(
              child: _Press(
                onTap: () => onPick(i),
                builder: (down) {
                  final sel = cur == i;
                  return Stack(children: [
                    if (i > 0 && !sel && cur != i - 1) Positioned(left: 0, top: 8, bottom: 8, child: Container(width: 1, color: look.gray(0x55))),
                    Positioned.fill(
                      child: Container(
                        margin: EdgeInsets.all(sel ? 1 : 2),
                        decoration: BoxDecoration(color: sel ? look.accent : (down ? look.gray(0x40) : null), borderRadius: BorderRadius.circular(radius - (sel ? 1 : 2))),
                        alignment: Alignment.center,
                        child: Text(labels[i], maxLines: 1, style: Look.font(14, sel ? Colors.white : look.text)),
                      ),
                    ),
                  ]);
                },
              ),
            ),
        ]),
      )),
      SizedBox(height: after),
    ]);
  }
}

/// objinsp::icon_segments: 30 tall, radius 7, an icon drawn in each part.
class DIconSegments extends StatelessWidget {
  const DIconSegments({super.key, required this.n, required this.selected, required this.draw, required this.onPick, this.height = 30});
  final int n;
  final int? selected;
  final void Function(Canvas c, Rect r, int i, Color col) draw;
  final void Function(int) onPick;
  final double height;
  @override
  Widget build(BuildContext context) {
    final look = LookScope.of(context);
    final touch = TouchScope.of(context);
    // (touch, in a card: the card is the track, no lines between the parts, only the chosen
    // one filled, as iPad Pages' alignment row)
    final flat = touch && _InCard.of(context);
    return Container(
      height: touch ? 44 : height,
      decoration: flat ? null : BoxDecoration(color: touch ? touchFill(look) : look.gray(0x33), borderRadius: BorderRadius.circular(touch ? 14 : 7)),
      child: Row(children: [
        for (var i = 0; i < n; i++)
          Expanded(
            child: _Press(
              onTap: () => onPick(i),
              builder: (down) {
                final sel = selected == i;
                return Stack(children: [
                  if (!flat && i > 0 && !sel && selected != i - 1) Positioned(left: 0, top: touch ? 12 : 8, bottom: touch ? 12 : 8, child: Container(width: 1, color: touch ? look.separator : look.gray(0x55))),
                  Positioned.fill(
                    child: Container(
                      margin: flat ? const EdgeInsets.symmetric(horizontal: 3, vertical: 4) : EdgeInsets.all(touch ? 3 : (sel ? 1 : 2)),
                      decoration: BoxDecoration(color: sel ? look.accent : (down ? (touch ? look.hover : look.gray(0x40)) : null), borderRadius: BorderRadius.circular(touch ? 11 : (sel ? 6 : 5))),
                      child: CustomPaint(painter: _FnPainter((c, s) => draw(c, Offset.zero & s, i, sel ? Colors.white : look.text))),
                    ),
                  ),
                ]);
              },
            ),
          ),
      ]),
    );
  }
}

class _FnPainter extends CustomPainter {
  _FnPainter(this.f);
  final void Function(Canvas, Size) f;
  @override
  void paint(Canvas canvas, Size size) => f(canvas, size);
  @override
  bool shouldRepaint(_FnPainter o) => true;
}

/// theme::checkbox: a 17-point rounded square (the accent with a tick when on), its label.
class DCheckbox extends StatelessWidget {
  const DCheckbox({super.key, required this.on, required this.label, required this.onChanged, this.bold = false, this.enabled = true, this.size = 13});
  final bool on, bold, enabled;
  final double size;
  final String label;
  final void Function(bool) onChanged;
  @override
  Widget build(BuildContext context) {
    final look = LookScope.of(context);
    if (TouchScope.of(context)) {
      // touch: the label, then a switch (at the row's right when the row has a width)
      final text = Text(label, maxLines: 1, overflow: TextOverflow.ellipsis, style: Look.font(16, enabled ? look.text : look.textDim, bold: bold));
      return Opacity(
        opacity: enabled ? 1 : 0.5,
        child: GestureDetector(
          behavior: HitTestBehavior.opaque,
          onTap: enabled ? () => onChanged(!on) : null,
          child: SizedBox(
            height: 44,
            child: LayoutBuilder(
              builder: (ctx, box) => Row(mainAxisSize: box.hasBoundedWidth ? MainAxisSize.max : MainAxisSize.min, children: [
                if (box.hasBoundedWidth) Expanded(child: text) else text,
                const SizedBox(width: 10),
                IgnorePointer(child: DToggle(on: on, onChanged: (_) {})),
              ]),
            ),
          ),
        ),
      );
    }
    return GestureDetector(
      behavior: HitTestBehavior.opaque,
      onTap: enabled ? () => onChanged(!on) : null,
      child: SizedBox(
        height: 20,
        child: Row(mainAxisSize: MainAxisSize.min, children: [
          const SizedBox(width: 0.5),
          CustomPaint(size: const Size(17, 17), painter: _CheckPainter(on, enabled, look)),
          const SizedBox(width: 8.5),
          Flexible(child: Text(label, maxLines: 1, overflow: TextOverflow.ellipsis, style: Look.font(size, enabled ? look.text : look.textDim, bold: bold))),
        ]),
      ),
    );
  }
}

class _CheckPainter extends CustomPainter {
  _CheckPainter(this.on, this.enabled, this.look);
  final bool on, enabled;
  final Look look;
  @override
  void paint(Canvas canvas, Size size) {
    final b = Offset.zero & size;
    final rr = RRect.fromRectAndRadius(b, const Radius.circular(4));
    if (on) {
      canvas.drawRRect(rr, Paint()..color = enabled ? look.accent : const Color(0xFF5A5A5A));
      canvas.drawPath(
          Path()..addPolygon([Offset(b.left + 4, b.center.dy), Offset(b.left + 7.5, b.bottom - 4.5), Offset(b.right - 4, b.top + 4)], false),
          Paint()
            ..style = PaintingStyle.stroke
            ..strokeWidth = 2
            ..color = Colors.white);
    } else {
      canvas.drawRRect(rr, Paint()..color = look.gray(0x3a));
      canvas.drawRRect(rr.deflate(0.5), Paint()
        ..style = PaintingStyle.stroke
        ..strokeWidth = 1
        ..color = look.gray(0x5a));
    }
  }

  @override
  bool shouldRepaint(_CheckPainter o) => o.on != on || o.enabled != enabled;
}

/// theme::toggle: a 38 × 22 pill, its knob gliding (0.2 s).
class DToggle extends StatelessWidget {
  const DToggle({super.key, required this.on, required this.onChanged});
  final bool on;
  final void Function(bool) onChanged;
  @override
  Widget build(BuildContext context) {
    final look = LookScope.of(context);
    // (touch: iPad's 51 × 31 switch)
    final touch = TouchScope.of(context);
    final k = touch ? 27.0 : 18.0;
    return GestureDetector(
      onTap: () => onChanged(!on),
      child: AnimatedContainer(
        duration: const Duration(milliseconds: 200),
        width: touch ? 51 : 38,
        height: touch ? 31 : 22,
        decoration: BoxDecoration(color: on ? look.accent : (touch ? touchFill(look) : look.gray(0x55)), borderRadius: BorderRadius.circular(touch ? 15.5 : 11)),
        child: AnimatedAlign(
          duration: const Duration(milliseconds: 200),
          alignment: on ? Alignment.centerRight : Alignment.centerLeft,
          child: Container(
            margin: const EdgeInsets.all(2),
            width: k,
            height: k,
            decoration: BoxDecoration(color: Colors.white, shape: BoxShape.circle, boxShadow: touch ? const [BoxShadow(color: Color(0x33000000), blurRadius: 3, offset: Offset(0, 1))] : null),
          ),
        ),
      ),
    );
  }
}

/// theme::disclosure: a space, a line, "⌄ Title" in bold (28 tall), an optional swatch.
class DDisclosure extends StatelessWidget {
  const DDisclosure({super.key, required this.title, required this.open, required this.onToggle, this.swatch, this.swatchNone = false});
  final String title;
  final bool open, swatchNone;
  final Color? swatch;
  final VoidCallback onToggle;
  @override
  Widget build(BuildContext context) {
    final look = LookScope.of(context);
    if (TouchScope.of(context)) {
      // touch: a card's first row: the title, its swatch, ⌄ open or › shut at the right
      return GestureDetector(
        behavior: HitTestBehavior.opaque,
        onTap: onToggle,
        child: SizedBox(
          height: 44,
          child: Row(children: [
            Expanded(child: Text(title, maxLines: 1, overflow: TextOverflow.ellipsis, style: Look.font(16, look.text, bold: true))),
            if (swatch != null || swatchNone) ...[
              Container(
                width: 56,
                height: 26,
                decoration: BoxDecoration(color: swatchNone ? Colors.white : swatch, borderRadius: BorderRadius.circular(13), border: Border.all(color: look.separator)),
                child: swatchNone ? CustomPaint(painter: _FnPainter((c, s) => c.drawLine(Offset(8, s.height - 6), Offset(s.width - 8, 6), Paint()
                  ..strokeWidth = 1.5
                  ..color = const Color(0xFFE03030)))) : null,
              ),
              const SizedBox(width: 12),
            ],
            CustomPaint(size: const Size(14, 14), painter: TouchChevron(look.textDim, down: open)),
          ]),
        ),
      );
    }
    return Column(mainAxisSize: MainAxisSize.min, crossAxisAlignment: CrossAxisAlignment.stretch, children: [
      const SizedBox(height: 4),
      const DSeparator(),
      const SizedBox(height: 6),
      GestureDetector(
        behavior: HitTestBehavior.opaque,
        onTap: onToggle,
        child: SizedBox(
          height: 28,
          child: CustomPaint(painter: _FnPainter((c, s) {
            final ctr = Offset(6, s.height / 2);
            final p = Paint()
              ..style = PaintingStyle.stroke
              ..strokeWidth = 1.4
              ..color = look.textDim;
            final pts = open
                ? [ctr + const Offset(-3.5, -2), ctr + const Offset(0, 2), ctr + const Offset(3.5, -2)]
                : [ctr + const Offset(-2, -3.5), ctr + const Offset(2, 0), ctr + const Offset(-2, 3.5)];
            c.drawPath(Path()..addPolygon(pts, false), p);
            final tp = TextPainter(text: TextSpan(text: title, style: Look.font(14, look.text, bold: true)), textDirection: TextDirection.ltr)..layout();
            tp.paint(c, Offset(20, s.height / 2 - tp.height / 2));
            if (swatch != null || swatchNone) {
              final r = Rect.fromLTRB(s.width - 92, 3, s.width, s.height - 3);
              c.drawRRect(RRect.fromRectAndRadius(r, const Radius.circular(6)), Paint()..color = swatchNone ? Colors.white : swatch!);
              if (swatchNone) {
                c.drawLine(Offset(r.left + 4, r.bottom - 3), Offset(r.right - 4, r.top + 3), Paint()
                  ..strokeWidth = 1.5
                  ..color = const Color(0xFFE03030));
              }
            }
          })),
        ),
      ),
    ]);
  }
}

/// theme::stepper: an 64 × 24 number field and ⌃⌄ arrows (82 × 26 in all). Tapping the field
/// edits it in place, as egui's DragValue.
class DStepper extends StatefulWidget {
  const DStepper({super.key, required this.value, required this.onChanged, this.step = 1, this.min = -1e9, this.max = 1e9, this.suffix = '', this.decimals = 0});
  final double value, step, min, max;
  final String suffix;
  final int decimals;
  final void Function(double) onChanged;
  @override
  State<DStepper> createState() => _DStepperState();
}

class _DStepperState extends State<DStepper> {
  bool _editing = false;
  final _c = TextEditingController();
  final _f = FocusNode();

  String _fmt(double x) {
    final s = widget.decimals == 0 || (x - x.roundToDouble()).abs() < 1e-9 ? x.toStringAsFixed(0) : x.toStringAsFixed(widget.decimals);
    return '$s${widget.suffix}';
  }

  void _commit() {
    final t = _c.text.replaceAll(RegExp(r'[^0-9.,\-]'), '').replaceAll(',', '.');
    final v = double.tryParse(t);
    setState(() => _editing = false);
    if (v != null && (v - widget.value).abs() > 1e-9) widget.onChanged(v.clamp(widget.min, widget.max));
  }

  @override
  void dispose() {
    _c.dispose();
    _f.dispose();
    super.dispose();
  }

  void _edit() {
    setState(() {
      _editing = true;
      _c.text = _fmt(widget.value);
      _c.selection = TextSelection(baseOffset: 0, extentOffset: _c.text.length);
    });
    _f.requestFocus();
  }

  /// Touch: the value in a pill (tap to type it), then − | + in another (iPad Pages' Size row).
  Widget _touch(Look look) {
    final fill = touchFill(look);
    Widget half(double dir) => Expanded(
          child: _Press(
            onTap: () => widget.onChanged((widget.value + widget.step * dir).clamp(widget.min, widget.max)),
            builder: (down) => Container(
              decoration: BoxDecoration(color: down ? look.hover : null, borderRadius: BorderRadius.horizontal(left: Radius.circular(dir < 0 ? 17 : 0), right: Radius.circular(dir > 0 ? 17 : 0))),
              child: CustomPaint(painter: _FnPainter((c, s) {
                final ctr = s.center(Offset.zero);
                final p = Paint()
                  ..strokeWidth = 1.8
                  ..strokeCap = StrokeCap.round
                  ..color = look.text;
                c.drawLine(ctr - const Offset(6, 0), ctr + const Offset(6, 0), p);
                if (dir > 0) c.drawLine(ctr - const Offset(0, 6), ctr + const Offset(0, 6), p);
              })),
            ),
          ),
        );
    return SizedBox(
      height: 44,
      child: Row(mainAxisSize: MainAxisSize.min, children: [
        GestureDetector(
          behavior: HitTestBehavior.opaque,
          onTap: _edit,
          child: Center(
            child: Container(
              constraints: const BoxConstraints(minWidth: 62),
              height: 34,
              padding: const EdgeInsets.symmetric(horizontal: 10),
              alignment: Alignment.center,
              decoration: BoxDecoration(color: fill, borderRadius: BorderRadius.circular(17), border: _editing ? Border.all(color: look.accent, width: 1.5) : null),
              child: _editing
                  ? SizedBox(
                      width: 56,
                      child: TextField(
                        controller: _c,
                        focusNode: _f,
                        textAlign: TextAlign.center,
                        keyboardType: const TextInputType.numberWithOptions(decimal: true, signed: true),
                        style: Look.font(15, look.text),
                        cursorColor: look.accent,
                        decoration: const InputDecoration(isDense: true, border: InputBorder.none, contentPadding: EdgeInsets.zero),
                        onSubmitted: (_) => _commit(),
                        onTapOutside: (_) => _commit(),
                        inputFormatters: [FilteringTextInputFormatter.allow(RegExp(r'[0-9.,\- a-z%]'))],
                      ),
                    )
                  : Text(_fmt(widget.value), maxLines: 1, style: Look.font(15, look.text)),
            ),
          ),
        ),
        const SizedBox(width: 6),
        Container(
          width: 84,
          height: 34,
          decoration: BoxDecoration(color: fill, borderRadius: BorderRadius.circular(17)),
          child: Row(crossAxisAlignment: CrossAxisAlignment.stretch, children: [half(-1), Center(child: Container(width: 1, height: 18, color: look.separator)), half(1)]),
        ),
      ]),
    );
  }

  @override
  Widget build(BuildContext context) {
    final look = LookScope.of(context);
    if (TouchScope.of(context)) return _touch(look);
    return SizedBox(
      width: 82,
      height: 26,
      child: Row(children: [
        GestureDetector(
          onTap: () {
            setState(() {
              _editing = true;
              _c.text = _fmt(widget.value);
              _c.selection = TextSelection(baseOffset: 0, extentOffset: _c.text.length);
            });
            _f.requestFocus();
          },
          child: Container(
            width: 64,
            height: 24,
            alignment: Alignment.center,
            decoration: BoxDecoration(color: look.control, borderRadius: BorderRadius.circular(6), border: Border.all(color: _editing ? look.accent : look.controlStroke)),
            child: _editing
                ? TextField(
                    controller: _c,
                    focusNode: _f,
                    textAlign: TextAlign.center,
                    keyboardType: const TextInputType.numberWithOptions(decimal: true, signed: true),
                    style: Look.font(13, look.text),
                    cursorColor: look.accent,
                    decoration: const InputDecoration(isDense: true, border: InputBorder.none, contentPadding: EdgeInsets.zero),
                    onSubmitted: (_) => _commit(),
                    onTapOutside: (_) => _commit(),
                    inputFormatters: [FilteringTextInputFormatter.allow(RegExp(r'[0-9.,\- a-z%]'))],
                  )
                : Text(_fmt(widget.value), style: Look.font(13, look.text)),
          ),
        ),
        const SizedBox(width: 2),
        SizedBox(
          width: 14,
          height: 24,
          child: Column(children: [
            for (final dir in [1.0, -1.0])
              Expanded(
                child: _Press(
                  onTap: () => widget.onChanged((widget.value + widget.step * dir).clamp(widget.min, widget.max)),
                  builder: (down) => CustomPaint(
                    size: const Size(14, 12),
                    painter: _FnPainter((c, s) {
                      final ctr = s.center(Offset.zero);
                      final dy = 2.0 * dir;
                      c.drawPath(
                          Path()..addPolygon([Offset(ctr.dx - 4, ctr.dy + dy), Offset(ctr.dx, ctr.dy - dy), Offset(ctr.dx + 4, ctr.dy + dy)], false),
                          Paint()
                            ..style = PaintingStyle.stroke
                            ..strokeWidth = 1.3
                            ..color = down ? look.text : look.textDim);
                    }),
                  ),
                ),
              ),
          ]),
        ),
      ]),
    );
  }
}

/// kn_ui colors.rs presets: twelve greys, then twelve hues in five shades.
List<Color> colorPresets() {
  final out = <Color>[for (var i = 0; i < 12; i++) Color.fromARGB(255, (255 * (1 - i / 11)).round(), (255 * (1 - i / 11)).round(), (255 * (1 - i / 11)).round())];
  const hues = [0.58, 0.53, 0.47, 0.38, 0.28, 0.17, 0.13, 0.08, 0.02, 0.95, 0.85, 0.75];
  for (final (s, v) in [(1.0, 0.45), (1.0, 0.7), (0.9, 0.95), (0.55, 1.0), (0.25, 1.0)]) {
    for (final h in hues) {
      out.add(HSVColor.fromAHSV(1, h * 360, s, v).toColor());
    }
  }
  return out;
}

/// colors.rs color_well: the swatch (its presets popover) joined to a colour-wheel button.
class DColorWell extends StatelessWidget {
  const DColorWell({super.key, required this.color, required this.onChanged, this.width = 64, this.onWheel});
  final Color color;
  final double width;
  final void Function(Color) onChanged;
  final VoidCallback? onWheel;
  @override
  Widget build(BuildContext context) {
    final look = LookScope.of(context);
    return Builder(builder: (ctx) {
      // the well's colour changed elsewhere (undo, a preset): the Colors window follows it
      final t = WellTarget.current.value;
      if (t != null && identical(t.owner, ctx) && t.color != color) {
        t.color = color;
        WidgetsBinding.instance.addPostFrameCallback((_) => WellTarget.current.poke());
      }
      // the wheel: the Colors window edits this well's colour
      final wheel = onWheel ?? () => WellTarget.current.value = WellTarget(ctx, color, onChanged);
      if (TouchScope.of(ctx)) {
        // touch (iPad Pages' Text Color row): a wide swatch pill and a chevron; its presets
        // popover holds the wheel too
        return _Press(
          onTap: () async {
            final c = await _presets(ctx, _rectOf(ctx), color, wheel: wheel);
            if (c != null) onChanged(c.withValues(alpha: color.a));
          },
          builder: (down) => Opacity(
            opacity: down ? 0.6 : 1,
            child: SizedBox(
              height: 44,
              child: Row(mainAxisSize: MainAxisSize.min, children: [
                Container(
                  width: math.max(width, 88),
                  height: 30,
                  decoration: BoxDecoration(borderRadius: BorderRadius.circular(15), border: Border.all(color: look.separator)),
                  child: ClipRRect(borderRadius: BorderRadius.circular(15), child: CustomPaint(size: Size.infinite, painter: _FnPainter((c, s) => _swatch(c, s, color)))),
                ),
                const SizedBox(width: 10),
                CustomPaint(size: const Size(8, 14), painter: TouchChevron(look.textDim)),
              ]),
            ),
          ),
        );
      }
      const h = 24.0, r = 4.0;
      return SizedBox(
        width: width,
        height: h,
        child: Container(
          // (one capsule in the control fill, no outline: color_well paints weak_bg_fill only)
          decoration: BoxDecoration(color: look.control, borderRadius: BorderRadius.circular(6)),
          child: Row(children: [
            Expanded(
              child: GestureDetector(
                onTap: () async {
                  final c = await _presets(ctx, _rectOf(ctx), color);
                  if (c != null) onChanged(c.withValues(alpha: color.a));
                },
                child: Container(
                  margin: const EdgeInsets.all(3),
                  decoration: BoxDecoration(borderRadius: BorderRadius.circular(r), border: Border.all(color: const Color.fromARGB(60, 0, 0, 0))),
                  child: ClipRRect(
                    borderRadius: BorderRadius.circular(r),
                    // (a see-through colour over a checkerboard, as colors.rs checker_rounded)
                    child: CustomPaint(size: Size.infinite, painter: _FnPainter((c, s) => _swatch(c, s, color))),
                  ),
                ),
              ),
            ),
            GestureDetector(
              // the wheel button
              onTap: wheel,
              child: SizedBox(width: 24, height: h, child: CustomPaint(painter: _FnPainter((c, s) => _wheel(c, s.center(Offset.zero), 8)))),
            ),
          ]),
        ),
      );
    });
  }

  /// A swatch: a see-through colour over a checkerboard (colors.rs checker_rounded).
  static void _swatch(Canvas c, Size s, Color color) {
    if (color.a < 1) {
      const k = 4.0;
      for (var y = 0.0; y < s.height; y += k) {
        for (var x = 0.0; x < s.width; x += k) {
          final dark = ((x / k).floor() + (y / k).floor()).isOdd;
          c.drawRect(Rect.fromLTWH(x, y, k, k), Paint()..color = dark ? const Color(0xFFC8C8C8) : Colors.white);
        }
      }
    }
    c.drawRect(Offset.zero & s, Paint()..color = color);
  }

  /// The colour wheel's icon.
  static void _wheel(Canvas c, Offset ctr, double wr) {
    for (var k = 0; k < 24; k++) {
      final a0 = k / 24 * 2 * math.pi, a1 = (k + 1) / 24 * 2 * math.pi;
      c.drawPath(Path()..addPolygon([ctr, ctr + Offset(math.cos(a0), -math.sin(a0)) * wr, ctr + Offset(math.cos(a1), -math.sin(a1)) * wr], true), Paint()..color = HSVColor.fromAHSV(1, k / 24 * 360, 0.85, 1).toColor());
    }
    c.drawCircle(ctr, wr, Paint()
      ..style = PaintingStyle.stroke
      ..strokeWidth = 1
      ..color = const Color.fromARGB(60, 0, 0, 0));
  }

  /// The presets popover; given `wheel` (touch), a Colors… row under the swatches opens it.
  Future<Color?> _presets(BuildContext ctx, Rect anchor, Color cur, {VoidCallback? wheel}) {
    final look = LookScope.of(ctx);
    final ps = colorPresets();
    // (touch: 22-point swatches)
    final k = wheel != null ? 22.0 : 16.0;
    return showDialog<Color>(
      context: ctx,
      barrierColor: Colors.transparent,
      builder: (d) {
        final screen = MediaQuery.of(d).size;
        final w = 12 * k + 11 * 3 + 12.0, h = 6 * k + 5 * 3 + 12.0 + (wheel != null ? 50 : 0);
        var top = anchor.bottom + 2;
        if (top + h > screen.height) top = anchor.top - h - 2;
        return Stack(children: [
          Positioned(
            left: (anchor.right - w).clamp(8.0, screen.width - w - 8),
            top: top,
            child: Material(
              color: look.panel,
              elevation: 8,
              shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(6), side: BorderSide(color: look.windowStroke)),
              child: Padding(
                padding: const EdgeInsets.all(6),
                child: SizedBox(
                  width: w - 12,
                  height: h - 12,
                  child: Wrap(spacing: 3, runSpacing: 3, children: [
                    if (wheel != null)
                      GestureDetector(
                        behavior: HitTestBehavior.opaque,
                        onTap: () {
                          Navigator.of(d).pop();
                          wheel();
                        },
                        child: SizedBox(
                          width: w - 12,
                          height: 44,
                          child: Row(children: [
                            const SizedBox(width: 6),
                            CustomPaint(size: const Size(22, 22), painter: _FnPainter((c, s) => _wheel(c, s.center(Offset.zero), 11))),
                            const SizedBox(width: 12),
                            Text('Colors…', style: Look.font(16, look.text)),
                          ]),
                        ),
                      ),
                    for (final c in ps)
                      GestureDetector(
                        onTap: () => Navigator.of(d).pop(c),
                        child: Container(
                          width: k,
                          height: k,
                          decoration: BoxDecoration(
                            color: c,
                            borderRadius: BorderRadius.circular(2),
                            border: Border.all(color: c.toARGB32() & 0xFFFFFF == cur.toARGB32() & 0xFFFFFF ? look.accent : const Color.fromARGB(50, 0, 0, 0), width: c.toARGB32() & 0xFFFFFF == cur.toARGB32() & 0xFFFFFF ? 2 : 1),
                          ),
                        ),
                      ),
                  ]),
                ),
              ),
            ),
          ),
        ]);
      },
    );
  }
}

/// Colours from the engine: [r, g, b, a] in 0..1.
Color colorOf(dynamic v, [Color fallback = Colors.transparent]) {
  if (v is! List || v.length < 3) return fallback;
  double f(int i) => i < v.length ? (v[i] as num).toDouble() : 1;
  return Color.from(alpha: f(3), red: f(0), green: f(1), blue: f(2));
}

List<double> colorTo(Color c) => [c.r, c.g, c.b, c.a];

/// Draws a desktop icon in a box (for buttons).
class DIconBox extends StatelessWidget {
  const DIconBox(this.icon, {super.key, this.size = 16, this.color});
  final DIcon icon;
  final double size;
  final Color? color;
  @override
  Widget build(BuildContext context) => DesktopIcon(icon, size: size, color: color ?? LookScope.of(context).text);
}
