// The desktop's menu bar (menus.rs menu_bar inside chrome.rs title_bar): the app menu (a page
// mark), File, Edit, Insert, Format, Arrange, View, Window, Help, each opening the desktop's
// menu (items, ticks, shortcuts, submenus and icons as the engine reports them, lt_menus) as a
// touch menu (touch_menu.dart). DesktopMenuStyle keeps egui's look for the right-click menu.
import 'package:flutter/material.dart';

import 'icons.dart';
import 'look.dart';
import 'touch_menu.dart';

/// kn_ui::icons::menu_icon: the icon Keynote shows next to some menu items.
const _menuIcons = <String, DIcon>{
  'Share…': DIcon.share, 'Print…': DIcon.print, 'Find': DIcon.search, 'Table': DIcon.table,
  'Chart': DIcon.chart, 'Text': DIcon.text, 'Shape': DIcon.shape, 'Line': DIcon.line,
  'Highlight': DIcon.highlight, 'Comment': DIcon.comment, 'Image Gallery': DIcon.gallery,
  'Web Video…': DIcon.webVideo, 'Live Video': DIcon.liveVideo, 'Show Live Video Preview': DIcon.liveVideo,
  'Record Audio': DIcon.audio, 'Choose File': DIcon.file, 'New Slide': DIcon.addSlide,
  'Bring Forward': DIcon.layerForward, 'Bring to Front': DIcon.layerFront, 'Send Backward': DIcon.layerBackward,
  'Send to Back': DIcon.layerBack, 'Show Arrange Tools': DIcon.layerFront, 'Align Objects': DIcon.alignObjects,
  'Distribute Objects': DIcon.distribute, 'Flip Horizontally': DIcon.flipH, 'Flip Vertically': DIcon.flipV,
  'Show Build Order': DIcon.list, 'Show Colors': DIcon.palette, 'Show Adjust Image': DIcon.sliders,
  'Show Music Browser': DIcon.music, 'Show Photo and Video Browser': DIcon.photo, 'Start Dictation': DIcon.mic,
  'Emoji & Symbols': DIcon.smiley, 'AutoFill': DIcon.text,
  'Undo': DIcon.undo, 'Redo': DIcon.redo, 'Find…': DIcon.search, 'Find & Replace…': DIcon.search,
};

/// The icon shown before a menu item, if it has one.
DIcon? menuIcon(String label) =>
    _menuIcons[label] ?? _menuIcons[label.replaceAll(RegExp(r'…$'), '')] ?? (label.startsWith('Undo ') ? DIcon.undo : label.startsWith('Redo ') ? DIcon.redo : null);

/// The desktop's menus as Flutter menu widgets: lt_menus' items ({label, command, enabled,
/// checked, shortcut, kids, separator}) and the right-click menu's ({label, action, enabled,
/// bold, checked, note, kids, separator}). `onPick` gets the item chosen.
class DesktopMenuStyle {
  DesktopMenuStyle(this.look);
  final Look look;

  MenuStyle menu() => MenuStyle(
        backgroundColor: WidgetStatePropertyAll(look.panel),
        surfaceTintColor: const WidgetStatePropertyAll(Colors.transparent),
        elevation: const WidgetStatePropertyAll(8),
        shadowColor: WidgetStatePropertyAll(Color.fromARGB(look.light ? 36 : 90, 0, 0, 0)),
        padding: const WidgetStatePropertyAll(EdgeInsets.all(6)),
        shape: WidgetStatePropertyAll(RoundedRectangleBorder(borderRadius: BorderRadius.circular(6), side: BorderSide(color: look.windowStroke))),
      );

  ButtonStyle item(bool checked) => ButtonStyle(
        padding: const WidgetStatePropertyAll(EdgeInsets.symmetric(horizontal: 10)),
        minimumSize: const WidgetStatePropertyAll(Size(0, 24)),
        maximumSize: const WidgetStatePropertyAll(Size(double.infinity, 24)),
        textStyle: WidgetStatePropertyAll(Look.font(13, look.text)),
        foregroundColor: WidgetStateProperty.resolveWith((s) => s.contains(WidgetState.disabled) ? look.text.withValues(alpha: 0.5) : look.text),
        backgroundColor: WidgetStateProperty.resolveWith((s) {
          if (checked) return look.accent;
          if (s.contains(WidgetState.disabled)) return Colors.transparent;
          return s.contains(WidgetState.hovered) || s.contains(WidgetState.focused) || s.contains(WidgetState.pressed) ? look.hover : Colors.transparent;
        }),
        overlayColor: const WidgetStatePropertyAll(Colors.transparent),
        shape: WidgetStatePropertyAll(RoundedRectangleBorder(borderRadius: BorderRadius.circular(6))),
        visualDensity: VisualDensity.compact,
        tapTargetSize: MaterialTapTargetSize.shrinkWrap,
      );

  List<Widget> items(List<dynamic> items, void Function(Map<String, dynamic> item) onPick) {
    // items line up past an icon column when any of them has an icon (menus.rs atoms)
    final indent = items.any((i) => menuIcon(i['label'] as String? ?? '') != null);
    return [
      for (final i in items)
        if (i['separator'] == true)
          Padding(padding: const EdgeInsets.symmetric(vertical: 6), child: Divider(height: 1, thickness: 1, color: look.separator))
        else if (i['note'] == true)
          SizedBox(height: 24, child: Padding(padding: const EdgeInsets.symmetric(horizontal: 10), child: Align(alignment: Alignment.centerLeft, child: Text(i['label'] as String? ?? '', style: Look.font(13, look.textDim)))))
        else
          // egui's menu rows: 24 tall, 6 between them
          Padding(padding: const EdgeInsets.symmetric(vertical: 3), child: SizedBox(height: 24, child: _item(i as Map<String, dynamic>, indent, onPick))),
    ];
  }

  Widget _leading(String label, bool enabled) {
    final ic = menuIcon(label);
    return SizedBox(width: 20, child: ic == null ? null : Center(child: DesktopIcon(ic, size: 16, color: enabled ? look.text : look.text.withValues(alpha: 0.5))));
  }

  Widget _item(Map<String, dynamic> i, bool indent, void Function(Map<String, dynamic>) onPick) {
    final label = i['label'] as String? ?? '';
    final kids = i['kids'] as List?;
    final enabled = i['enabled'] != false;
    if (kids != null) {
      return SubmenuButton(
        style: item(false),
        menuStyle: menu(),
        leadingIcon: indent ? _leading(label, enabled) : null,
        menuChildren: items(kids, onPick),
        child: Text(label),
      );
    }
    final checked = i['checked'] == true;
    final shortcut = i['shortcut'] as String?;
    final bold = i['bold'] == true;
    return MenuItemButton(
      style: item(checked),
      leadingIcon: indent ? _leading(label, enabled) : null,
      trailingIcon: shortcut == null ? null : Padding(padding: const EdgeInsets.only(left: 24), child: Text(shortcut, style: Look.font(13, enabled ? look.weakText : look.weakText.withValues(alpha: 0.5)))),
      onPressed: enabled ? () => onPick(i) : null,
      child: Text(label, style: Look.font(13, checked ? Colors.white : (enabled ? look.text : look.text.withValues(alpha: 0.5)), bold: bold)),
    );
  }
}

class DesktopMenuBar extends StatelessWidget {
  const DesktopMenuBar({super.key, required this.look, required this.menus, required this.onCommand, this.compact = false});
  final Look look;

  /// The phone layout: every menu under one ☰ button (menus.rs menu_bar).
  final bool compact;

  /// lt_menus: [{label, command, enabled, checked, shortcut, kids, separator}].
  final List<dynamic> menus;
  final void Function(String command) onCommand;

  /// The title bar row (30) and its panel's margins (2 above and below).
  static const height = 34.0;

  @override
  Widget build(BuildContext context) {
    final font = Look.font(13, look.text);
    void pick(Map<String, dynamic> i) => onCommand(i['command'] as String? ?? i['label'] as String? ?? '');
    return Container(
      height: height,
      color: look.header,
      padding: const EdgeInsets.fromLTRB(6, 2, 6, 2),
      alignment: Alignment.centerLeft,
      // (not enough room for every menu: the bar scrolls sideways)
      child: SingleChildScrollView(
        scrollDirection: Axis.horizontal,
        child: Row(children: [
          if (compact)
            _BarButton(look: look, items: () => [for (final m in menus) {'label': m['label'], 'kids': m['kids']}], onPick: pick, child: Text('\u2630', style: font))
          else
            for (final m in menus)
              _BarButton(
                look: look,
                items: () => m['kids'] as List? ?? [],
                onPick: pick,
                child: m['label'] == 'Letters'
                    // the app menu: a page mark instead of its name
                    ? SizedBox(width: 20, height: 20, child: DesktopIcon(DIcon.text, size: 20, color: look.text))
                    : Text(m['label'] as String? ?? '', style: font),
              ),
        ]),
      ),
    );
  }
}

/// A menu's title in the bar: filled while its menu (a touch menu) is open.
class _BarButton extends StatefulWidget {
  const _BarButton({required this.look, required this.items, required this.onPick, required this.child});
  final Look look;
  final List<dynamic> Function() items;
  final void Function(Map<String, dynamic>) onPick;
  final Widget child;
  @override
  State<_BarButton> createState() => _BarButtonState();
}

class _BarButtonState extends State<_BarButton> {
  bool _open = false, _hover = false;

  Future<void> _show() async {
    final box = context.findRenderObject() as RenderBox;
    setState(() => _open = true);
    await showTouchMenu(context, widget.look, box.localToGlobal(Offset.zero) & box.size, widget.items(), widget.onPick);
    if (mounted) setState(() => _open = false);
  }

  @override
  Widget build(BuildContext context) => MouseRegion(
        onEnter: (_) => setState(() => _hover = true),
        onExit: (_) => setState(() => _hover = false),
        child: GestureDetector(
          behavior: HitTestBehavior.opaque,
          onTap: _show,
          child: Container(
            height: 30,
            padding: const EdgeInsets.symmetric(horizontal: 9),
            alignment: Alignment.center,
            decoration: BoxDecoration(color: _open || _hover ? widget.look.hover : null, borderRadius: BorderRadius.circular(8)),
            child: widget.child,
          ),
        ),
      );
}
