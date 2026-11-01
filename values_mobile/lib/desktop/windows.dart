// The desktop's small windows (pg-app dialogs.rs, app.rs dialogs): egui Windows in kn_ui's look,
// a 34-point title bar with the title centred in 18 points, a hairline under it, 6-point margins,
// 10-point corners and egui's drop shadow, centred on the window and moved by their title bar.
// What each shows comes from lt_status "windows" (dlgapi.rs); their buttons go back through
// lt_inspector_set "dlg.<window>.<button>".
import 'dart:async';
import 'dart:math' as math;
import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:file_picker/file_picker.dart';
import 'package:path_provider/path_provider.dart';
import 'package:record/record.dart';

import '../engine.dart';
import 'colors.dart';
import 'glide.dart';
import 'hub.dart';
import 'icons.dart';
import 'inspector.dart' show InspectorSheet;
import 'look.dart';
import 'scale.dart';
import 'toolbar.dart' show TbPayload, TbDragImage;
import 'widgets.dart';

/// egui's Window frame. (Content is at least 326 wide, as the desktop's windows come out.)
class DWindow extends StatefulWidget {
  const DWindow({super.key, required this.title, required this.child, this.onClose, this.onDrag, this.minWidth = 326, this.collapsible = false});
  final String title;
  final Widget child;
  final VoidCallback? onClose;
  final void Function(Offset delta)? onDrag;
  final double minWidth;

  /// The ▼ at the left of the title folds it to its title bar.
  final bool collapsible;

  @override
  State<DWindow> createState() => _DWindowState();
}

class _DWindowState extends State<DWindow> {
  bool _folded = false;

  @override
  Widget build(BuildContext context) {
    final title = widget.title, child = widget.child, onClose = widget.onClose, onDrag = widget.onDrag, minWidth = widget.minWidth;
    final look = LookScope.of(context);
    final hair = 1 / MediaQuery.of(context).devicePixelRatio;
    return Container(
      decoration: BoxDecoration(
        color: look.panel,
        borderRadius: BorderRadius.circular(10),
        border: Border.all(color: look.windowStroke, width: hair),
        boxShadow: [BoxShadow(color: Color(look.light ? 0x19000000 : 0x60000000), offset: const Offset(10, 20), blurRadius: 15)],
      ),
      child: ClipRRect(
        borderRadius: BorderRadius.circular(10),
        child: IntrinsicWidth(
          child: Column(mainAxisSize: MainAxisSize.min, crossAxisAlignment: CrossAxisAlignment.stretch, children: [
            GestureDetector(
              // (in a Glide: it moves the window, a flick carries on)
              onPanUpdate: (d) => (GlideHandle.of(context)?.drag ?? onDrag)?.call(d.delta),
              onPanEnd: (d) => GlideHandle.of(context)?.release(d.velocity),
              child: Container(
                height: 34,
                decoration: BoxDecoration(color: look.titleBar, border: Border(bottom: BorderSide(color: look.windowStroke))),
                child: Stack(children: [
                  if (widget.collapsible)
                    Positioned(
                      left: 0,
                      top: 0,
                      bottom: 0,
                      width: 34,
                      child: GestureDetector(
                        behavior: HitTestBehavior.opaque,
                        onTap: () => setState(() => _folded = !_folded),
                        child: Center(child: Text(_folded ? '\u25b6' : '\u25bc', style: Look.font(13, look.text))),
                      ),
                    ),
                  // (centred in the room left of the close button)
                  Padding(padding: EdgeInsets.only(right: onClose != null ? 28 : 0), child: Center(child: Text(title, style: Look.font(18, look.text)))),
                  if (onClose != null)
                    Positioned(
                      right: 0,
                      top: 0,
                      bottom: 0,
                      width: 34,
                      child: GestureDetector(behavior: HitTestBehavior.opaque, onTap: onClose, child: CustomPaint(painter: _CloseX(look.text))),
                    ),
                ]),
              ),
            ),
            if (!_folded)
              Padding(
                padding: const EdgeInsets.all(6),
                child: ConstrainedBox(constraints: BoxConstraints(minWidth: minWidth), child: child),
              ),
          ]),
        ),
      ),
    );
  }
}

class _CloseX extends CustomPainter {
  _CloseX(this.color);
  final Color color;
  @override
  void paint(Canvas c, Size s) {
    final m = Offset(s.width / 2 - 2, s.height / 2);
    final p = Paint()
      ..color = color
      ..strokeWidth = 1;
    c.drawLine(m + const Offset(-6, -6), m + const Offset(6, 6), p);
    c.drawLine(m + const Offset(-6, 6), m + const Offset(6, -6), p);
  }

  @override
  bool shouldRepaint(_CloseX o) => o.color != color;
}

/// egui's TextEdit in kn_ui's look: white (dark: near black), 6-point corners, the hairline
/// border until it has the keyboard; 24 tall, or `rows` lines.
class DTextField extends StatefulWidget {
  const DTextField({super.key, required this.controller, this.hint, this.width, this.rows = 1, this.password = false, this.autofocus = false, this.onSubmitted, this.onChanged, this.focusNode});
  final TextEditingController controller;
  final String? hint;
  final double? width;
  final int rows;
  final bool password, autofocus;
  final VoidCallback? onSubmitted;
  final ValueChanged<String>? onChanged;
  final FocusNode? focusNode;

  @override
  State<DTextField> createState() => _DTextFieldState();
}

class _DTextFieldState extends State<DTextField> {
  late final FocusNode _f = widget.focusNode ?? FocusNode();

  @override
  void initState() {
    super.initState();
    _f.addListener(_focus);
    // (the field takes the keyboard when its window opens, as request_focus; Flutter's
    // autofocus gives way to the page's focus)
    if (widget.autofocus) WidgetsBinding.instance.addPostFrameCallback((_) => mounted ? _f.requestFocus() : null);
  }

  void _focus() => setState(() {});

  @override
  void dispose() {
    _f.removeListener(_focus);
    if (widget.focusNode == null) _f.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final look = LookScope.of(context);
    final multi = widget.rows > 1;
    return Container(
      width: widget.width,
      height: multi ? widget.rows * 15.0 + 8 : 24,
      padding: const EdgeInsets.symmetric(horizontal: 10),
      decoration: BoxDecoration(
        color: look.field,
        borderRadius: BorderRadius.circular(6),
        border: Border.all(color: _f.hasFocus ? (look.light ? Colors.white : look.accent) : look.controlStroke),
      ),
      child: TextField(
        controller: widget.controller,
        focusNode: _f,
        obscureText: widget.password,
        maxLines: multi ? null : 1,
        expands: multi,
        textAlignVertical: multi ? TextAlignVertical.top : TextAlignVertical.center,
        style: Look.font(13, look.text).copyWith(height: multi ? 15 / 13 : 1.0),
        cursorColor: look.text,
        cursorWidth: 1.5,
        decoration: InputDecoration(
          isDense: true,
          border: InputBorder.none,
          hintText: widget.hint,
          hintStyle: Look.font(13, look.textDim),
          isCollapsed: !multi,
          contentPadding: multi ? const EdgeInsets.only(top: 7, bottom: 4) : EdgeInsets.zero,
        ),
        onChanged: (t) {
          widget.onChanged?.call(t);
          setState(() {});
        },
        onSubmitted: multi ? null : (_) => widget.onSubmitted?.call(),
      ),
    );
  }
}

/// ui.horizontal: items 8 apart.
Widget _hz(List<Widget> kids) => Row(mainAxisSize: MainAxisSize.min, children: [for (final (i, k) in kids.indexed) ...[if (i > 0) const SizedBox(width: 8), k]]);

/// The open windows over everything, each centred until moved.
class Windows extends StatefulWidget {
  const Windows({super.key, required this.engine, required this.windows, required this.onChanged, this.savedColors = '', this.titleAt, this.toolbarBottom = 86, this.compact = false, this.onPrint, this.onTitleFormat});
  final Engine engine;

  /// The phone layout: Settings and the Content Hub come up as sheets from the bottom.
  final bool compact;
  final List<dynamic> windows;

  /// The Colors window's saved colours (the app's settings).
  final String savedColors;

  /// The toolbar title's place (the title popover hangs under it).
  final Rect? titleAt;

  /// The toolbar's bottom edge (the Customize sheet drops down from it).
  final double toolbarBottom;
  final VoidCallback onChanged;

  /// Print Setup's Print (false) and Export PDF… (true): the front end's own (Android prints
  /// the exported PDF through the system's print service).
  final void Function(bool export)? onPrint;

  /// The title popover changed the format (0 Excel, 1 Excel + Values, 2 Numbers): on Android the
  /// file is the app's copy, so the new one goes where the system's picker says.
  final void Function(int format)? onTitleFormat;

  @override
  State<Windows> createState() => _WindowsState();
}

class _WindowsState extends State<Windows> {
  final _record = GlobalKey<_RecordBodyState>();

  /// Where each was moved to (from the centre).

  void _press(String id, String button, [Object? v = const <String, Object?>{}]) {
    widget.engine.inspectorSet('dlg.$id.$button', v);
    widget.onChanged();
  }

  @override
  void initState() {
    super.initState();
    WellTarget.current.addListener(_well);
  }

  @override
  void dispose() {
    WellTarget.current.removeListener(_well);
    super.dispose();
  }

  void _well() => setState(() {});

  String _savedSeen = '';

  /// The saved colours changed in the panel: into the app's settings.
  void _saveColors() {
    _savedSeen = ColorPanelState.shared.savedToString();
    widget.engine.inspectorSet('dlg.colors.saved', _savedSeen);
  }

  /// The Colors window: up and left of the window's top right corner (right_top + (-560, 80)).
  Widget _colorsAt(BoxConstraints box, String id, Widget w) => Positioned.fill(key: ValueKey('glide-$id'), child: Glide(initial: (area, _) => Offset(area.width - 560, 80), child: w));

  @override
  Widget build(BuildContext context) {
    if (widget.savedColors != _savedSeen) {
      _savedSeen = widget.savedColors;
      ColorPanelState.shared.savedFrom(widget.savedColors);
    }
    final well = WellTarget.current.value;
    return LayoutBuilder(
      builder: (ctx, box) => Stack(children: [
        if (well != null)
          _colorsAt(
            box,
            'well',
            ColorsWindow(
              color: well.color,
              onChanged: (c) {
                well.color = c;
                well.onChanged(c);
              },
              onSavedChanged: _saveColors,
              onClose: () => WellTarget.current.value = null,
            ),
          ),
        for (final w in widget.windows)
          if (w['id'] == 'colors')
            _colorsAt(
              box,
              'colors',
              ColorsWindow(
                color: _rgba(w['color'] as List),
                onChanged: (c) => widget.engine.inspectorSet('dlg.colors.color', _list(c)),
                onSavedChanged: _saveColors,
                onClose: () => _press('colors', 'close'),
                extra: DButton(
                  width: 232,
                  height: 26,
                  onTap: w['any'] == true ? () => _press('colors', 'apply', _list(_rgba(w['color'] as List))) : null,
                  child: const Text('Apply to Selection'),
                ),
              ),
            )
          else if (widget.compact && (w['id'] == 'settings' || w['id'] == 'hub' || w['id'] == 'printSetup'))
            Positioned.fill(
              key: ValueKey('sheet-${w['id']}'),
              child: InspectorSheet(
                open: true,
                onClose: () => _press(w['id'] as String, 'close'),
                child: w['id'] == 'hub'
                    ? ContentHub(w: w as Map<String, dynamic>, sheet: true, set: (k, [v]) => _press('hub', k, v))
                    : w['id'] == 'printSetup'
                    ? TouchScope(child: SingleChildScrollView(padding: const EdgeInsets.all(16), child: _PrintSetupBody(w: w as Map<String, dynamic>, set: (k, [v]) => _press('printSetup', k, v), onPrint: (export) {
                        _press('printSetup', 'close');
                        widget.onPrint?.call(export);
                      })))
                    : TouchScope(child: _SettingsBody(w: w as Map<String, dynamic>, sheet: true, set: (k, v) => _press('settings', k, v))),
              ),
            )
          else
          // the Hub and All Tabs stay in the middle; the windows glide (Fonts opens with its
          // corner up and left of the middle, as default_pos(centre - (260, 160)))
          w['id'] == 'customize'
              ? Positioned.fill(child: _CustomizeSheet(w: w as Map<String, dynamic>, top: widget.toolbarBottom + 6, set: (k, [v]) => _press('customize', k, v)))
              : w['id'] == 'docTitle'
              ? Positioned.fill(
                  child: _DocTitlePopover(
                    key: const ValueKey('docTitle'),
                    w: w as Map<String, dynamic>,
                    at: widget.titleAt ?? const Rect.fromLTWH(60, 40, 100, 40),
                    set: (k, [v]) {
                      _press('docTitle', k, v);
                      final f = w['format'] as int? ?? 2;
                      if (k == 'apply' && Platform.isAndroid && f != w['was']) widget.onTitleFormat?.call(f);
                    },
                  ),
                )
              : w['id'] == 'hub' || w['id'] == 'allTabs'
              ? Positioned.fill(child: Align(child: _window(w as Map<String, dynamic>)))
              : Positioned.fill(
                  key: ValueKey('glide-${w['id']}'),
                  child: Glide(
                    initial: w['id'] == 'fonts' ? (area, _) => Offset(area.width / 2 - 260, area.height / 2 - 160) : (area, me) => Offset((area.width - me.width) / 2, (area.height - me.height) / 2),
                    child: _window(w as Map<String, dynamic>),
                  ),
                ),
      ]),
    );
  }

  Widget _window(Map<String, dynamic> w) {
    final id = w['id'] as String;
    if (id == 'allTabs') return _AllTabs(w: w, set: (k, [v]) => _press(id, k, v));
    if (id == 'hub') return ContentHub(w: w, set: (k, [v]) => _press(id, k, v));
    final body = switch (id) {
      'link' => _LinkBody(key: ValueKey(w['title']), w: w, press: (b, [v = const {}]) => _press(id, b, v)),
      'comment' => _TextBody(key: const ValueKey('comment'), w: w, rows: 4, width: 300, hint: 'Comment', label: w['author'] as String, done: 'Done', press: (b, [v = const {}]) => _press(id, b, v)),
      'newStyle' => _TextBody(key: const ValueKey('newStyle'), w: w, width: 240, label: 'Name:', done: 'Create', enter: true, press: (b, [v = const {}]) => _press(id, b, v)),
      'equation' => _EquationBody(key: const ValueKey('equation'), w: w, engine: widget.engine, press: (b, [v = const {}]) => _press(id, b, v)),
      'goTo' => _TextBody(key: const ValueKey('goTo'), w: w, width: 120, label: 'Page (1–${w['pages']}):', done: 'Go', enter: true, always: true, press: (b, [v = const {}]) => _press(id, b, v)),
      'password' => _PasswordBody(key: const ValueKey('password'), w: w, press: (b, [v = const {}]) => _press(id, b, v)),
      'setPassword' => _SetPasswordBody(key: const ValueKey('setPassword'), w: w, press: (b, [v = const {}]) => _press(id, b, v)),
      'about' => _AboutBody(w: w),
      'settings' => TouchScope(child: _SettingsBody(w: w, set: (k, v) => _press(id, k, v))),
      'spelling' => _SpellingBody(w: w, set: (k, [v]) => _press(id, k, v)),
      'versions' => _VersionsBody(w: w, set: (k, [v]) => _press(id, k, v)),
      'fonts' => _FontsBody(w: w, set: (k, [v]) => _press(id, k, v)),
      'replaceFonts' => _ReplaceFontsBody(w: w, set: (k, [v]) => _press(id, k, v)),
      'record' => _RecordBody(key: _record, w: w, engine: widget.engine, set: (k, [v]) => _press(id, k, v)),
      'webVideo' => _WebVideoBody(w: w, set: (k, [v]) => _press(id, k, v)),
      'shortcuts' => _ShortcutsBody(w: w),
      'condHighlight' => _CondBody(w: w, set: (k, [v]) => _press(id, k, v)),
      'chartData' => _ChartDataBody(w: w, set: (k, [v]) => _press(id, k, v)),
      'valuesLink' => _ValuesLinkBody(key: const ValueKey('valuesLink'), w: w, set: (k, [v]) => _press(id, k, v)),
      'stock' => _StockBody(key: const ValueKey('stock'), w: w, set: (k, [v]) => _press(id, k, v)),
      'warnings' => _WarningsBody(w: w, set: (k, [v]) => _press(id, k, v)),
      'saveTemplate' => _SaveTemplateBody(key: const ValueKey('saveTemplate'), w: w, set: (k, [v]) => _press(id, k, v)),
      'language' => _LanguageBody(w: w, set: (k, [v]) => _press(id, k, v)),
      'mediaBrowser' => _MediaBrowserBody(key: ValueKey('media-${w['music']}'), w: w, set: (k, [v]) => _press(id, k, v)),
      // (a fixed width: the window measures its content's width, which a scroll view can't give)
      'printSetup' => SizedBox(
          width: 380,
          height: math.max(240, math.min(760, MediaQuery.of(context).size.height - 160)),
          child: SingleChildScrollView(
            child: TouchScope(child: _PrintSetupBody(w: w, set: (k, [v]) => _press(id, k, v), onPrint: (export) {
              _press(id, 'close');
              widget.onPrint?.call(export);
            })),
          ),
        ),
      'unsaved' => EguiColumn(children: [
          DLabel(w['text'] as String),
          Align(
            alignment: Alignment.centerLeft,
            child: _hz([
              DButton(onTap: () => _press(id, 'dontSave'), child: const Text("Don't Save")),
              DButton(onTap: () => _press(id, 'cancel'), child: const Text('Cancel')),
              DButton(onTap: () => _press(id, 'save'), child: const Text('Save')),
            ]),
          ),
        ]),
      _ => DLabel(id),
    };
    return DWindow(
      title: w['title'] as String? ?? '',
      onClose: switch (id) {
        'about' => () => _press(id, 'cancel'),
        'settings' || 'spelling' || 'versions' || 'fonts' || 'replaceFonts' || 'webVideo' || 'shortcuts' || 'condHighlight' || 'chartData' || 'printSetup' || 'stock' || 'warnings' || 'saveTemplate' || 'language' || 'mediaBrowser' || 'equation' => () => _press(id, 'close'),
        'record' => () => _record.currentState?.close(),
        _ => null,
      },
      minWidth: switch (id) { 'mediaBrowser' => 360, 'warnings' => 460, 'valuesLink' => 300, 'settings' => 486, 'replaceFonts' => 506, 'versions' => 340, 'shortcuts' => 446, 'chartData' => 546, 'fonts' => 0, _ => 326 },
      collapsible: id == 'shortcuts',
      child: Focus(
        onKeyEvent: (n, e) {
          if (e is KeyDownEvent && e.logicalKey == LogicalKeyboardKey.escape) {
            _press(id, 'cancel');
            return KeyEventResult.handled;
          }
          return KeyEventResult.ignored;
        },
        child: body,
      ),
    );
  }
}

typedef _Press = void Function(String button, [Map<String, Object?> v]);

Color _rgba(List c) => Color.fromARGB(c[3] as int, c[0] as int, c[1] as int, c[2] as int);
List<int> _list(Color c) => [for (final v in [c.r, c.g, c.b, c.a]) (v * 255).round()];

/// egui's radio_value: a 15-point circle, the label 4 points after; 24 tall.
class _Radio extends StatelessWidget {
  const _Radio(this.on, this.label, this.onTap);
  final bool on;
  final String label;
  final VoidCallback onTap;
  @override
  Widget build(BuildContext context) {
    final look = LookScope.of(context);
    // (touch: a row of the card, a checkmark at the right of the chosen one, as iOS)
    if (TouchScope.of(context)) {
      return GestureDetector(
        behavior: HitTestBehavior.opaque,
        onTap: onTap,
        child: SizedBox(
          height: kTouchRow - 6,
          child: Row(children: [
            Expanded(child: Text(label, style: Look.font(16, look.text))),
            if (on) Icon(Icons.check_rounded, size: 22, color: look.accent),
          ]),
        ),
      );
    }
    return GestureDetector(
      behavior: HitTestBehavior.opaque,
      onTap: onTap,
      child: SizedBox(
        height: 24,
        child: Row(mainAxisSize: MainAxisSize.min, children: [
          Container(
            width: 15,
            height: 15,
            decoration: BoxDecoration(color: look.control, shape: BoxShape.circle, border: Border.all(color: look.light ? look.controlStroke : look.gray(0x5a))),
            alignment: Alignment.center,
            child: on ? Container(width: 5, height: 5, decoration: BoxDecoration(color: look.text, shape: BoxShape.circle)) : null,
          ),
          const SizedBox(width: 4),
          Text(label, style: Look.font(13, look.text)),
        ]),
      ),
    );
  }
}

/// egui's selectable_label: the accent behind it when chosen.
class _Selectable extends StatelessWidget {
  const _Selectable(this.on, this.label, this.onTap);
  final bool on;
  final String label;
  final VoidCallback onTap;
  @override
  Widget build(BuildContext context) {
    final look = LookScope.of(context);
    return GestureDetector(
      behavior: HitTestBehavior.opaque,
      onTap: onTap,
      child: Container(
        height: 24,
        padding: const EdgeInsets.symmetric(horizontal: 10),
        decoration: BoxDecoration(color: on ? look.accent : null, borderRadius: BorderRadius.circular(6), border: on ? Border.all(color: look.controlStroke) : null),
        child: Center(widthFactor: 1, child: Text(label, style: Look.font(13, look.text))),
      ),
    );
  }
}

/// Values > Settings…: General / Rulers / Auto-Correction, each a two-column grid (16 by 8).
class _SettingsBody extends StatefulWidget {
  const _SettingsBody({required this.w, required this.set, this.sheet = false});
  final Map<String, dynamic> w;

  /// In the phone layout's sheet: as wide as the sheet, its own title and close button.
  final bool sheet;
  final void Function(String field, Object? value) set;
  @override
  State<_SettingsBody> createState() => _SettingsBodyState();
}

class _SettingsBodyState extends State<_SettingsBody> {
  static int _tab = 0;
  late final _author = TextEditingController(text: widget.w['author'] as String? ?? '');

  @override
  void dispose() {
    _author.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final look = LookScope.of(context);
    final w = widget.w;
    bool b(String k) => w[k] == true;
    Widget check(String k, String label) => DCheckbox(on: b(k), label: label, onChanged: (v) => widget.set(k, v));
    final touch = TouchScope.of(context);
    Widget small(String t) => Text(t, style: Look.font(touch ? 13 : 9, look.textDim).copyWith(height: 1.1));
    // ui.vertical: 6 points between items
    Widget col(List<Widget> kids) => TouchScope.of(context)
        ? EguiColumn(children: kids)
        : Column(crossAxisAlignment: CrossAxisAlignment.start, mainAxisSize: MainAxisSize.min, children: [for (final (i, k) in kids.indexed) ...[if (i > 0) const SizedBox(height: 6), k]]);
    final rows = <(String, Widget)>[];
    switch (_tab) {
      case 0:
        final themes = [for (final t in w['themes'] as List) t as Map<String, dynamic>];
        rows.addAll([
          ('Appearance:', TouchScope.of(context) ? col([_Radio(!b('dark'), 'Light', () => widget.set('dark', false)), _Radio(b('dark'), 'Dark', () => widget.set('dark', true))]) : Row(children: [_Radio(!b('dark'), 'Light', () => widget.set('dark', false)), const SizedBox(width: 8), _Radio(b('dark'), 'Dark', () => widget.set('dark', true))])),
          (
            'Color:',
            col([
              Wrap(spacing: touch ? 12 : 6, runSpacing: touch ? 12 : 6, children: [
                for (final t in themes)
                  GestureDetector(
                    onTap: () => widget.set('color_theme', t['id']),
                    child: CustomPaint(size: Size.square(touch ? 40 : 26), painter: _Swatch(Color(0xFF000000 | (t['main'] as int)), Color(0xFF000000 | (t['accent'] as int)), w['color_theme'] == t['id'], look.text)),
                  ),
              ]),
              small('${w['themeName']} (shared with Keydeck)'),
            ]),
          ),
          ('Buttons:', col([check('liquid_glass', 'Glass (toolbar and inspector tabs)'), check('translucent_panes', 'Translucent inspector'), check('classic_look', 'Classic look (toolbar strip)')])),
          if (w['backgrounds'] != null) (
            'Toolbar:',
            DCombo<String>(
              width: 180,
              text: [for (final x in w['backgrounds'] as List) if (x[0] == w['toolbar_background']) x[1] as String].firstOrNull ?? '',
              items: () => [for (final x in w['backgrounds'] as List) DChoice(x[1] as String, x[0] as String, on: x[0] == w['toolbar_background'])],
              onPick: (v) => widget.set('toolbar_background', v),
            ),
          ),
          (
            'Scaling:',
            DCombo<int>(
              width: 100,
              text: [for (final x in w['scales'] as List) if (x[0] == w['ui_scale']) x[1] as String].firstOrNull ?? 'Default',
              items: () => [for (final x in w['scales'] as List) DChoice(x[1] as String, x[0] as int, on: x[0] == w['ui_scale'])],
              onPick: (v) => widget.set('ui_scale', v),
            ),
          ),
          if (b('android'))
            (
              'Layout:',
              DCombo<int>(
                width: 100,
                text: [for (final x in w['layouts'] as List) if (x[0] == w['ui_layout']) x[1] as String].firstOrNull ?? 'Automatic',
                items: () => [for (final x in w['layouts'] as List) DChoice(x[1] as String, x[0] as int, on: x[0] == w['ui_layout'])],
                onPick: (v) => widget.set('ui_layout', v),
              ),
            ),
          if (!b('android') && !b('windows'))
            (
              'Window:',
              col([
                _Radio(!b('system_title_bar'), 'Menus in the title bar', () => widget.set('system_title_bar', false)),
                _Radio(b('system_title_bar'), 'System title bar', () => widget.set('system_title_bar', true)),
                small('Takes effect the next time Values starts.'),
              ]),
            ),
          ('Editing:', col([check('show_size_while_moving', 'Show size and position when moving objects'), check('formula_suggestions', 'Show suggestions when editing table cells'), if (w.containsKey('autocomplete_cells')) check('autocomplete_cells', 'Show suggestions from the column while typing')])),
          if (w.containsKey('show_chooser')) (
            'For New Documents:',
            col([
              _Radio(b('show_chooser'), 'Show Template Chooser', () => widget.set('show_chooser', true)),
              _Radio(!b('show_chooser'), 'Use template: Blank', () => widget.set('show_chooser', false)),
            ]),
          ),
          if (w.containsKey('default_font')) (
            'Default Font:',
            Row(children: [
              // (the font menu's names, or type one)
              DCombo<String>(
                width: 150,
                text: (w['default_font'] as String? ?? '').isEmpty ? 'Helvetica Neue' : w['default_font'] as String,
                search: true,
                typed: (t) => t,
                listWidth: 260,
                items: () => [for (final f in (w['fonts'] as List? ?? const [])) DChoice(f as String, f, on: f == w['default_font'])],
                onPick: (f) => widget.set('default_font', f),
              ),
              const SizedBox(width: 8),
              DStepper(value: (w['default_font_size'] as num).toDouble(), min: 6, max: 96, suffix: ' pt', onChanged: (v) => widget.set('default_font_size', v)),
            ]),
          ),
          (
            'Default Zoom:',
            Row(mainAxisSize: touch ? MainAxisSize.min : MainAxisSize.max, children: [
              if (!touch) ...[const DLabel('Zoom'), const SizedBox(width: 8)],
              DCombo<double>(
                width: 100,
                text: '${((w['default_zoom'] as num) * 100).round()}%',
                items: () => [for (final z in w['zooms'] as List) DChoice('${(z * 100).round()}%', (z as num).toDouble(), on: z == w['default_zoom'])],
                onPick: (v) => widget.set('default_zoom', v),
              ),
            ]),
          ),
          ('Author:', Align(alignment: Alignment.centerLeft, child: DTextField(controller: _author, width: 240, hint: 'Name shown on comments', onChanged: (t) => widget.set('author', t)))),
        ]);
      case 1:
        const names = ['Centimeters', 'Inches', 'Points', 'Millimeters'];
        rows.addAll([
          (
            'Ruler Units:',
            DCombo<int>(
              width: 100,
              text: names[(w['ruler_units'] as int).clamp(0, 3)],
              items: () => [for (final (i, n) in names.indexed) DChoice(n, i, on: i == w['ruler_units'])],
              onPick: (v) => widget.set('ruler_units', v),
            ),
          ),
          (
            'Alignment Guides:',
            col([
              check('guides_center', 'Show guides at object center'),
              check('guides_edges', 'Show guides at object edges'),
              check('guides_spacing', 'Show guides for relative spacing'),
              check('guides_sizing', 'Show guides for relative sizing'),
            ]),
          ),
        ]);
      default:
        rows.addAll([
          (
            'Formatting:',
            col([
              check('smart_quotes', 'Use smart quotes \u201c \u201d'),
              check('smart_dashes', 'Use smart dashes \u2014'),
              check('capitalize', 'Capitalize words automatically'),
              check('text_replacement', 'Use text replacement ((c) \u2192 \u00a9, -> \u2192 \u2192)'),
              check('detect_lists', 'Automatically detect lists'),
            ]),
          ),
          ('Spelling:', col([check('spell_check', 'Check spelling while typing'), check('grammar', 'Check grammar with spelling'), check('autocorrect', 'Correct spelling automatically')])),
        ]);
    }
    // the touch style: the tabs as a segmented control, each setting a titled card (its rows
    // with hairlines between), scrolling when taller than the window; drawn at four fifths
    if (touch) {
      // a menu (a combo box) on a row with its label; the rest under a title
      final kids = <Widget>[];
      var single = false;
      for (final (label, ctl) in rows) {
        final name = label.replaceAll(':', '');
        if (ctl is DCombo) {
          if (!single && kids.isNotEmpty) kids.add(const DSeparator());
          kids.add(Row(children: [Expanded(child: Text(name, maxLines: 1, overflow: TextOverflow.ellipsis, style: Look.font(16, look.text))), ctl]));
          single = true;
        } else {
          kids.addAll([DSectionTitle(name), ctl]);
          single = false;
        }
      }
      final body = Column(crossAxisAlignment: CrossAxisAlignment.stretch, mainAxisSize: widget.sheet ? MainAxisSize.max : MainAxisSize.min, children: [
        if (widget.sheet)
          SizedBox(
            height: 44,
            child: Row(children: [
              Expanded(child: Text('Settings', style: Look.font(20, look.text, bold: true))),
              GestureDetector(
                onTap: () => widget.set('close', null),
                child: Container(width: 36, height: 36, decoration: BoxDecoration(color: touchFill(look), shape: BoxShape.circle), child: Icon(Icons.close_rounded, size: 22, color: look.text)),
              ),
            ]),
          ),
        if (widget.sheet) const SizedBox(height: 8),
        DSegmented(labels: const ['General', 'Rulers', 'Auto-Correction'], cur: _tab, radius: 16, onPick: (i) => setState(() => _tab = i)),
        const SizedBox(height: kTouchGap),
        Flexible(fit: widget.sheet ? FlexFit.tight : FlexFit.loose, child: SingleChildScrollView(child: EguiColumn(children: kids))),
      ]);
      if (widget.sheet) return ScaledBox(k: 0.8, child: body);
      return SizedBox(
        width: math.min(486, MediaQuery.of(context).size.width - 24),
        child: ConstrainedBox(
          constraints: BoxConstraints(maxHeight: MediaQuery.of(context).size.height * 0.75),
          child: ScaledBox(k: 0.8, child: body),
        ),
      );
    }
    return SizedBox(
      width: 486,
      child: Column(crossAxisAlignment: CrossAxisAlignment.stretch, mainAxisSize: MainAxisSize.min, children: [
        Row(children: [
          for (final (i, n) in ['General', 'Rulers', 'Auto-Correction'].indexed) ...[if (i > 0) const SizedBox(width: 8), _Selectable(_tab == i, n, () => setState(() => _tab = i))],
        ]),
        const DSpace(6),
        const DSeparator(),
        const DSpace(6),
        Table(
          columnWidths: const {0: IntrinsicColumnWidth(), 1: FlexColumnWidth()},
          defaultVerticalAlignment: TableCellVerticalAlignment.middle,
          children: [
            for (final (i, (label, ctl)) in rows.indexed)
              TableRow(children: [
                Padding(padding: EdgeInsets.only(right: 16, bottom: i < rows.length - 1 ? 8 : 0), child: DLabel(label)),
                Padding(padding: EdgeInsets.only(bottom: i < rows.length - 1 ? 8 : 0), child: Align(alignment: Alignment.centerLeft, child: ctl)),
              ]),
          ],
        ),
      ]),
    );
  }
}

/// A colour theme's swatch: its main colour, the accent as a dot at the lower right, a ring
/// round the chosen one.
class _Swatch extends CustomPainter {
  _Swatch(this.main, this.accent, this.on, this.ring);
  final Color main, accent, ring;
  final bool on;
  @override
  void paint(Canvas c, Size s) {
    final m = s.center(Offset.zero);
    c.drawCircle(m, 11, Paint()..color = main);
    if (accent != main) c.drawCircle(m + const Offset(5.5, 5.5), 4.5, Paint()..color = accent);
    if (on) {
      c.drawCircle(m, 12.5, Paint()
        ..style = PaintingStyle.stroke
        ..strokeWidth = 2
        ..color = ring);
    }
  }

  @override
  bool shouldRepaint(_Swatch o) => o.main != main || o.on != on || o.ring != ring;
}

/// A label, a field, Cancel and the window's button (Comment, Create Style, Go to Page).
class _TextBody extends StatefulWidget {
  const _TextBody({super.key, required this.w, required this.label, required this.done, required this.press, this.width, this.rows = 1, this.hint, this.enter = false, this.always = false});
  final Map<String, dynamic> w;
  final String label, done;
  final _Press press;
  final double? width;
  final int rows;
  final String? hint;

  /// Enter presses the button; `always`: even with nothing typed.
  final bool enter, always;

  @override
  State<_TextBody> createState() => _TextBodyState();
}

class _TextBodyState extends State<_TextBody> {
  late final _c = TextEditingController(text: widget.w['text'] as String? ?? '');

  @override
  void dispose() {
    _c.dispose();
    super.dispose();
  }

  bool get _ok => widget.always || _c.text.trim().isNotEmpty;
  void _done() => widget.press('done', {'text': _c.text});

  @override
  Widget build(BuildContext context) => EguiColumn(children: [
        DLabel(widget.label),
        Align(
          alignment: Alignment.centerLeft,
          child: DTextField(controller: _c, width: widget.width, rows: widget.rows, hint: widget.hint, autofocus: true, onChanged: (_) => setState(() {}), onSubmitted: widget.enter && _ok ? _done : null),
        ),
        Align(
          alignment: Alignment.centerLeft,
          child: _hz([DButton(onTap: () => widget.press('cancel'), child: const Text('Cancel')), DButton(onTap: _ok ? _done : null, child: Text(widget.done))]),
        ),
      ]);
}

/// Add Link: a field for the address (or the document's bookmarks), Cancel / Done.
class _LinkBody extends StatefulWidget {
  const _LinkBody({super.key, required this.w, required this.press});
  final Map<String, dynamic> w;
  final _Press press;
  @override
  State<_LinkBody> createState() => _LinkBodyState();
}

class _LinkBodyState extends State<_LinkBody> {
  late final _c = TextEditingController(text: widget.w['text'] as String? ?? '');

  @override
  void dispose() {
    _c.dispose();
    super.dispose();
  }

  void _done() => widget.press('done', {'text': _c.text});

  @override
  Widget build(BuildContext context) {
    final look = LookScope.of(context);
    final kind = widget.w['kind'] as String;
    final ok = _c.text.trim().isNotEmpty;
    final marks = [for (final b in (widget.w['bookmarks'] as List? ?? [])) b as List];
    return EguiColumn(children: [
      if (kind == 'Bookmark') ...[
        if (marks.isEmpty) const DLabel('There are no bookmarks in this document.'),
        for (final b in marks)
          // egui's selectable_label
          GestureDetector(
            onTap: () => setState(() => _c.text = b[1] as String),
            child: Align(
              alignment: Alignment.centerLeft,
              child: Container(
                padding: const EdgeInsets.symmetric(horizontal: 4, vertical: 1),
                decoration: BoxDecoration(color: _c.text == b[1] ? look.accent : null, borderRadius: BorderRadius.circular(6)),
                child: DLabel(b[0] as String, color: _c.text == b[1] ? Colors.white : null),
              ),
            ),
          ),
      ] else
        Align(
          alignment: Alignment.centerLeft,
          child: DTextField(
            controller: _c,
            width: 300,
            autofocus: true,
            hint: switch (kind) { 'Email' => 'name@example.com', 'Phone Number' => '+1 555 123 4567', _ => 'www.example.com' },
            onChanged: (_) => setState(() {}),
            onSubmitted: ok ? _done : null,
          ),
        ),
      Align(alignment: Alignment.centerLeft, child: _hz([DButton(onTap: () => widget.press('cancel'), child: const Text('Cancel')), DButton(onTap: ok ? _done : null, child: const Text('Done'))])),
    ]);
  }
}

/// Insert > Equation: the LaTeX, its readable form in 18 points under it, Cancel / Insert.
class _EquationBody extends StatefulWidget {
  const _EquationBody({super.key, required this.w, required this.engine, required this.press});
  final Map<String, dynamic> w;
  final Engine engine;
  final _Press press;
  @override
  State<_EquationBody> createState() => _EquationBodyState();
}

class _EquationBodyState extends State<_EquationBody> {
  late final _c = TextEditingController(text: widget.w['text'] as String? ?? '');

  @override
  void dispose() {
    _c.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final look = LookScope.of(context);
    final ok = _c.text.trim().isNotEmpty;
    return EguiColumn(children: [
      const DLabel('Enter a LaTeX equation:'),
      DTextField(
        controller: _c,
        rows: 3,
        autofocus: true,
        hint: 'x^2 + y^2 = z^2',
        // (the engine keeps the source and makes the preview)
        onChanged: (t) => widget.press('edit', {'text': t}),
      ),
      SizedBox(height: 21, child: Text(widget.w['preview'] as String? ?? '', style: Look.font(18, look.text).copyWith(height: 21 / 18))),
      if (widget.w['error'] is String) DLabel(widget.w['error'] as String, color: const Color(0xFFFF453A)),
      Align(
        alignment: Alignment.centerLeft,
        child: _hz([DButton(onTap: () => widget.press('cancel'), child: const Text('Cancel')), DButton(onTap: ok ? () => widget.press('done', {'text': _c.text}) : null, child: Text(widget.w['done'] as String? ?? 'Insert'))]),
      ),
    ]);
  }
}

/// Opening a password-protected document.
class _PasswordBody extends StatefulWidget {
  const _PasswordBody({super.key, required this.w, required this.press});
  final Map<String, dynamic> w;
  final _Press press;
  @override
  State<_PasswordBody> createState() => _PasswordBodyState();
}

class _PasswordBodyState extends State<_PasswordBody> {
  final _c = TextEditingController();

  @override
  void dispose() {
    _c.dispose();
    super.dispose();
  }

  void _open() => widget.press('done', {'text': _c.text});

  @override
  Widget build(BuildContext context) {
    final hint = widget.w['hint'] as String? ?? '';
    return EguiColumn(children: [
      DLabel('“${widget.w['file']}” is password protected.'),
      if (hint.isNotEmpty) DLabel('Hint: $hint'),
      if (widget.w['wrong'] == true) const DLabel("That password isn't right.", color: Color(0xFFC03030)),
      Align(alignment: Alignment.centerLeft, child: DTextField(controller: _c, width: 240, password: true, autofocus: true, onSubmitted: _open)),
      Align(alignment: Alignment.centerLeft, child: _hz([DButton(onTap: () => widget.press('cancel'), child: const Text('Cancel')), DButton(onTap: _open, child: const Text('Open'))])),
    ]);
  }
}

/// File > Set Password: Password / Verify / Hint in a grid; Remove Password when it has one.
class _SetPasswordBody extends StatefulWidget {
  const _SetPasswordBody({super.key, required this.w, required this.press});
  final Map<String, dynamic> w;
  final _Press press;
  @override
  State<_SetPasswordBody> createState() => _SetPasswordBodyState();
}

class _SetPasswordBodyState extends State<_SetPasswordBody> {
  final _pw = TextEditingController(), _again = TextEditingController(), _hint = TextEditingController();

  @override
  void dispose() {
    _pw.dispose();
    _again.dispose();
    _hint.dispose();
    super.dispose();
  }

  bool get _ok => _pw.text.isNotEmpty && _pw.text == _again.text;
  void _set() => widget.press('done', {'password': _pw.text, 'hint': _hint.text});

  @override
  Widget build(BuildContext context) {
    Widget row(String label, Widget field) => SizedBox(height: 24, child: Row(children: [SizedBox(width: 68, child: DLabel(label)), Expanded(child: field)]));
    void changed(String _) => setState(() {});
    return EguiColumn(children: [
      row('Password:', DTextField(controller: _pw, password: true, autofocus: true, onChanged: changed, onSubmitted: _ok ? _set : null)),
      row('Verify:', DTextField(controller: _again, password: true, onChanged: changed, onSubmitted: _ok ? _set : null)),
      row('Hint:', DTextField(controller: _hint, onSubmitted: _ok ? _set : null)),
      Align(
        alignment: Alignment.centerLeft,
        child: _hz([
          DButton(onTap: () => widget.press('cancel'), child: const Text('Cancel')),
          if (widget.w['has'] == true) DButton(onTap: () => widget.press('remove'), child: const Text('Remove Password')),
          DButton(onTap: _ok ? _set : null, child: const Text('Set Password')),
        ]),
      ),
    ]);
  }
}

class _AboutBody extends StatelessWidget {
  const _AboutBody({required this.w});
  final Map<String, dynamic> w;
  @override
  Widget build(BuildContext context) {
    final look = LookScope.of(context);
    Widget wrap(String t) => SizedBox(width: 326, child: Text(t, style: Look.font(13, look.text).copyWith(height: 1.2)));
    return EguiColumn(children: [
      DLabel(w['app'] as String? ?? 'Values', bold: true),
      wrap(w['line'] as String? ?? 'Reads and writes Numbers (.numbers) spreadsheets and Excel workbooks.'),
      DLabel('Version ${w['version']}'),
      const DLabel('Looks and settings follow Keydeck and Letters.'),
    ]);
  }
}

typedef _Set = void Function(String key, [Object? value]);

/// Edit > Spelling and Grammar: the word in red, its guesses in a white list, Change / Find
/// Next / Ignore / Learn.
class _SpellingBody extends StatelessWidget {
  const _SpellingBody({required this.w, required this.set});
  final Map<String, dynamic> w;
  final _Set set;
  @override
  Widget build(BuildContext context) {
    final look = LookScope.of(context);
    if (w['done'] == true) return DLabel(w['doneText'] as String? ?? 'No more misspelled words in this document.');
    if (w['noDicts'] == true) return const DLabel('No spelling dictionaries are installed (hunspell-en-us, hunspell-fr).');
    final guesses = [for (final g in w['guesses'] as List) g as String];
    final pick = w['pick'] as int? ?? 0;
    return EguiColumn(children: [
      DLabel(w['word'] as String, bold: true, color: const Color(0xFFE01B24)),
      const DSpace(4),
      Align(
        alignment: Alignment.centerLeft,
        child: Container(
        width: 300,
        constraints: const BoxConstraints(minHeight: 118),
        padding: const EdgeInsets.all(4),
        decoration: BoxDecoration(color: look.field, borderRadius: BorderRadius.circular(6)),
        child: Column(crossAxisAlignment: CrossAxisAlignment.start, mainAxisSize: MainAxisSize.min, children: [
          if (guesses.isEmpty) DLabel('No guesses', dim: true),
          for (final (i, g) in guesses.indexed) ...[if (i > 0) const SizedBox(height: 6), _Selectable(pick == i, g, () => set('pick', i))],
        ]),
      )),
      const DSpace(6),
      Align(
        alignment: Alignment.centerLeft,
        child: _hz([
          DButton(onTap: guesses.isEmpty ? null : () => set('change'), child: const Text('Change')),
          DButton(onTap: () => set('next'), child: const Text('Find Next')),
          DButton(onTap: () => set('ignore'), child: const Text('Ignore')),
          DButton(onTap: () => set('learn'), child: const Text('Learn')),
        ]),
      ),
    ]);
  }
}

/// File > Revert To > Browse All Versions…: the kept versions, newest first, each with Restore.
class _VersionsBody extends StatelessWidget {
  const _VersionsBody({required this.w, required this.set});
  final Map<String, dynamic> w;
  final _Set set;
  @override
  Widget build(BuildContext context) {
    final all = [for (final v in w['versions'] as List) v as List];
    final look = LookScope.of(context);
    return SizedBox(
      width: 340,
      child: EguiColumn(children: [
        if (all.isEmpty) Text(w['empty'] as String? ?? 'No earlier versions yet: a version is kept each time the document is saved over.', style: Look.font(13, look.text).copyWith(height: 1.2)),
        ConstrainedBox(
          constraints: const BoxConstraints(maxHeight: 320),
          child: SingleChildScrollView(
            child: Column(mainAxisSize: MainAxisSize.min, children: [
              for (final (i, v) in all.indexed) ...[
                if (i > 0) const SizedBox(height: 6),
                Row(children: [Expanded(child: DLabel(v[0] as String)), DButton(onTap: () => set('restore', v[1]), child: const Text('Restore'))]),
              ],
            ]),
          ),
        ),
      ]),
    );
  }
}

/// Format > Font > Show Fonts: underline / strikethrough / colour / search, then Family,
/// Typeface and Size lists; a pick changes the selection at once.
class _FontsBody extends StatefulWidget {
  const _FontsBody({required this.w, required this.set});
  final Map<String, dynamic> w;
  final _Set set;
  @override
  State<_FontsBody> createState() => _FontsBodyState();
}

class _FontsBodyState extends State<_FontsBody> {
  late final _q = TextEditingController(text: widget.w['query'] as String? ?? '');

  @override
  void dispose() {
    _q.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final look = LookScope.of(context);
    final w = widget.w, set = widget.set;
    final can = w['can'] == true;
    final q = _q.text.toLowerCase();
    final names = [for (final n in w['names'] as List) if (q.isEmpty || (n as String).toLowerCase().contains(q)) n as String];
    final size = (w['size'] as num).toDouble();
    Widget toggle(String k, String label, TextDecoration deco) => DButton(
          onTap: () => set(k, !(w[k] == true)),
          fill: w[k] == true ? look.accent : null,
          child: Text(label, style: Look.font(13, look.text).copyWith(decoration: deco)),
        );
    Widget head(String t) => SizedBox(height: 16, child: Align(alignment: Alignment.centerLeft, child: DLabel(t, bold: true)));
    final c = w['color'] as List;
    final body = EguiColumn(children: [
      SizedBox(
        width: 426,
        child: Row(children: [
          toggle('underline', 'U', TextDecoration.underline),
          const SizedBox(width: 8),
          toggle('strike', 'S', TextDecoration.lineThrough),
          const SizedBox(width: 8),
          DColorWell(
            color: Color.from(alpha: 1, red: (c[0] as num).toDouble(), green: (c[1] as num).toDouble(), blue: (c[2] as num).toDouble()),
            onChanged: (col) => set('color', [col.r, col.g, col.b, 1.0]),
          ),
          const SizedBox(width: 8),
          Expanded(
            child: DTextField(
              controller: _q,
              hint: 'Search',
              onChanged: (t) {
                set('query', t);
                setState(() {});
              },
            ),
          ),
        ]),
      ),
      const DSeparator(),
      Row(crossAxisAlignment: CrossAxisAlignment.start, children: [
        SizedBox(
          width: 210,
          child: Column(crossAxisAlignment: CrossAxisAlignment.start, children: [
            head('Family'),
            const SizedBox(height: 6),
            SizedBox(
              height: 240,
              child: ListView.builder(
                padding: EdgeInsets.zero,
                itemCount: names.length,
                itemExtent: 30,
                itemBuilder: (ctx, i) => Align(alignment: Alignment.topLeft, child: _Selectable(names[i] == w['family'], names[i], () => set('family', names[i]))),
              ),
            ),
          ]),
        ),
        const SizedBox(width: 8),
        SizedBox(
          width: 120,
          child: Column(crossAxisAlignment: CrossAxisAlignment.start, children: [
            head('Typeface'),
            for (final (name, b, i) in const [('Regular', false, false), ('Italic', false, true), ('Bold', true, false), ('Bold Italic', true, true)]) ...[
              const SizedBox(height: 6),
              _Selectable(w['bold'] == b && w['italic'] == i, name, () {
                if (w['bold'] != b) set('bold', b);
                if (w['italic'] != i) set('italic', i);
              }),
            ],
          ]),
        ),
        const SizedBox(width: 8),
        SizedBox(
          width: 80,
          child: Column(crossAxisAlignment: CrossAxisAlignment.start, children: [
            head('Size'),
            const SizedBox(height: 6),
            _DragValue(value: size, min: 1, max: 1638, suffix: ' pt', onChanged: (v) => set('size', v)),
            const SizedBox(height: 6),
            SizedBox(
              height: 212,
              child: ListView(padding: EdgeInsets.zero, children: [
                for (final v in const [9, 10, 11, 12, 13, 14, 18, 24, 36, 48, 64, 72, 96, 144, 288])
                  SizedBox(height: 30, child: Align(alignment: Alignment.topLeft, child: _Selectable((size - v).abs() < 0.01, '$v', () => set('size', v)))),
              ]),
            ),
          ]),
        ),
      ]),
    ]);
    return EguiColumn(children: [
      if (!can) DLabel('Select cells or a text box to change its font.', dim: true),
      IgnorePointer(ignoring: !can, child: Opacity(opacity: can ? 1 : 0.5, child: body)),
    ]);
  }
}

/// Format > Font > Replace Fonts…: each font the document uses (⚠ when it isn't installed),
/// the family to put in its place, Replace at the lower right.
class _ReplaceFontsBody extends StatelessWidget {
  const _ReplaceFontsBody({required this.w, required this.set});
  final Map<String, dynamic> w;
  final _Set set;
  @override
  Widget build(BuildContext context) {
    final look = LookScope.of(context);
    final rows = [for (final r in w['rows'] as List) r as List];
    final families = [for (final f in w['families'] as List) f as String];
    final any = rows.any((r) => r[2] != null);
    return SizedBox(
      width: 506,
      height: 372,
      child: Column(crossAxisAlignment: CrossAxisAlignment.start, children: [
        DLabel('Fonts marked \u26a0 are not installed on this computer and are shown with a substitute.', dim: true),
        const SizedBox(height: 12),
        SizedBox(
          height: 228,
          child: SingleChildScrollView(
            child: Table(
              columnWidths: const {0: IntrinsicColumnWidth(), 1: IntrinsicColumnWidth(), 2: IntrinsicColumnWidth()},
              defaultVerticalAlignment: TableCellVerticalAlignment.middle,
              children: [
                TableRow(children: [
                  Padding(padding: const EdgeInsets.only(right: 12, bottom: 6), child: DLabel('Font', color: look.text)),
                  const SizedBox(),
                  const Padding(padding: EdgeInsets.only(bottom: 6), child: DLabel('Replace with')),
                ]),
                for (final (i, r) in rows.indexed)
                  TableRow(children: [
                    Padding(
                      padding: const EdgeInsets.only(right: 12, bottom: 6),
                      child: r[1] == true ? DLabel(r[0] as String) : DLabel('\u26a0 ${r[0]}', color: const Color(0xFFE6A03C)),
                    ),
                    const Padding(padding: EdgeInsets.only(right: 12, bottom: 6), child: DLabel('\u2192')),
                    Padding(
                      padding: const EdgeInsets.only(bottom: 6),
                      child: DCombo<String?>(
                        width: 220,
                        text: r[2] as String? ?? "Don't replace",
                        items: () => [DChoice("Don't replace", null, on: r[2] == null), for (final f in families) DChoice(f, f, on: r[2] == f)],
                        onPick: (v) => set('pick', {'i': i, 'to': v}),
                      ),
                    ),
                  ]),
              ],
            ),
          ),
        ),
        const SizedBox(height: 8),
        Align(alignment: Alignment.centerRight, child: DButton(onTap: any ? () => set('replace') : null, child: const Text('Replace'))),
      ]),
    );
  }
}

/// egui's DragValue: the number in a field; dragging sideways changes it, a tap types it.
class _DragValue extends StatefulWidget {
  const _DragValue({required this.value, required this.onChanged, this.min = -1e9, this.max = 1e9, this.suffix = ''});
  final double value, min, max;
  final String suffix;
  final void Function(double) onChanged;
  @override
  State<_DragValue> createState() => _DragValueState();
}

class _DragValueState extends State<_DragValue> {
  bool _editing = false;
  double _drag = 0;
  final _c = TextEditingController();

  @override
  void dispose() {
    _c.dispose();
    super.dispose();
  }

  String get _text => '${widget.value.toStringAsFixed(widget.value == widget.value.roundToDouble() ? 0 : 1)}${widget.suffix}';

  void _commit() {
    final v = double.tryParse(_c.text.replaceAll(widget.suffix.trim(), '').trim());
    setState(() => _editing = false);
    if (v != null) widget.onChanged(v.clamp(widget.min, widget.max));
  }

  @override
  Widget build(BuildContext context) {
    final look = LookScope.of(context);
    if (_editing) return SizedBox(width: 48, child: DTextField(controller: _c, autofocus: true, onSubmitted: _commit));
    return GestureDetector(
      onTap: () => setState(() {
        _editing = true;
        _c.text = _text;
      }),
      onHorizontalDragStart: (_) => _drag = widget.value,
      onHorizontalDragUpdate: (d) {
        _drag = (_drag + d.delta.dx).clamp(widget.min, widget.max);
        if (_drag.round() != widget.value.round()) widget.onChanged(_drag.roundToDouble());
      },
      child: Container(
        height: 24,
        padding: const EdgeInsets.symmetric(horizontal: 10),
        decoration: BoxDecoration(color: look.control, borderRadius: BorderRadius.circular(6), border: Border.all(color: look.controlStroke)),
        child: Center(widthFactor: 1, child: Text(_text, style: Look.font(13, look.text))),
      ),
    );
  }
}

/// Insert > Record Audio: Record, then the time in red and Stop and Insert. Android records
/// with the system's microphone here (the desktop's way, pw-record, is Linux's); the sound goes
/// in as the desktop's recording does.
class _RecordBody extends StatefulWidget {
  const _RecordBody({super.key, required this.w, required this.engine, required this.set});
  final Map<String, dynamic> w;
  final Engine engine;
  final _Set set;
  @override
  State<_RecordBody> createState() => _RecordBodyState();
}

class _RecordBodyState extends State<_RecordBody> {
  AudioRecorder? _rec;
  DateTime? _since;
  Timer? _tick;

  bool get _here => Platform.isAndroid;

  @override
  void dispose() {
    _tick?.cancel();
    _rec?.dispose();
    super.dispose();
  }

  Future<void> _start() async {
    if (!_here) return widget.set('start');
    final r = AudioRecorder();
    if (!await r.hasPermission()) {
      r.dispose();
      return;
    }
    final dir = await getTemporaryDirectory();
    await r.start(const RecordConfig(encoder: AudioEncoder.aacLc, bitRate: 128000), path: '${dir.path}/Recording.m4a');
    setState(() {
      _rec = r;
      _since = DateTime.now();
      _tick = Timer.periodic(const Duration(milliseconds: 250), (_) => setState(() {}));
    });
  }

  Future<void> _stop(bool insert) async {
    _tick?.cancel();
    final path = await _rec?.stop();
    _rec?.dispose();
    _rec = null;
    if (insert && path != null) widget.engine.action('insert:$path');
    widget.set('cancel');
  }

  /// The close button: a recording stops without going in.
  void close() => _here && _rec != null ? _stop(false) : widget.set('cancel');

  @override
  Widget build(BuildContext context) {
    final t = _here ? (_since == null ? null : DateTime.now().difference(_since!).inSeconds) : widget.w['seconds'] as int?;
    return EguiColumn(children: [
      if (t == null) ...[
        const DLabel('Records from the default microphone.'),
        Align(alignment: Alignment.centerLeft, child: DButton(onTap: _start, child: const Text('\u25cf  Record'))),
      ] else ...[
        DLabel('\u25cf Recording  ${t ~/ 60}:${(t % 60).toString().padLeft(2, '0')}', size: 16, color: const Color(0xFFFF453A)),
        Align(alignment: Alignment.centerLeft, child: DButton(onTap: () => _here ? _stop(true) : widget.set('stop'), child: const Text('Stop and Insert'))),
      ],
    ]);
  }
}

/// Insert > Add Web Video…: a YouTube or Vimeo link; Insert looks it up.
class _WebVideoBody extends StatefulWidget {
  const _WebVideoBody({required this.w, required this.set});
  final Map<String, dynamic> w;
  final _Set set;
  @override
  State<_WebVideoBody> createState() => _WebVideoBodyState();
}

class _WebVideoBodyState extends State<_WebVideoBody> {
  late final _c = TextEditingController(text: widget.w['text'] as String? ?? '');

  @override
  void dispose() {
    _c.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final ok = widget.w['ok'] == true;
    void go() => widget.set('done', {'text': _c.text});
    return EguiColumn(children: [
      const DLabel('Paste a YouTube or Vimeo link.'),
      DTextField(controller: _c, autofocus: true, hint: 'https://www.youtube.com/watch?v=\u2026', onChanged: (t) => widget.set('edit', {'text': t}), onSubmitted: ok ? go : null),
      Align(alignment: Alignment.centerLeft, child: DButton(onTap: ok ? go : null, child: const Text('Insert'))),
    ]);
  }
}

/// Window > Show All Tabs (egui's Modal: the window dimmed, a frame in the middle): each open
/// document as a 170 × 100 button, three to a row; Done.
class _AllTabs extends StatelessWidget {
  const _AllTabs({required this.w, required this.set});
  final Map<String, dynamic> w;
  final _Set set;
  @override
  Widget build(BuildContext context) {
    final look = LookScope.of(context);
    final tabs = [for (final t in w['tabs'] as List) t as String];
    final rows = <List<int>>[for (var i = 0; i < tabs.length; i += 3) [for (var j = i; j < i + 3 && j < tabs.length; j++) j]];
    return Stack(children: [
      Positioned.fill(child: GestureDetector(onTap: () => set('done'), child: Container(color: const Color(0x80000000)))),
      Center(
        child: Container(
          width: 560 + 24,
          padding: const EdgeInsets.all(12),
          decoration: BoxDecoration(color: look.panel, borderRadius: BorderRadius.circular(10), border: Border.all(color: look.windowStroke)),
          child: Column(crossAxisAlignment: CrossAxisAlignment.start, mainAxisSize: MainAxisSize.min, children: [
            DLabel('All Tabs', size: 16, bold: true),
            const SizedBox(height: 14),
            for (final r in rows)
              Padding(
                padding: const EdgeInsets.only(bottom: 12),
                child: Row(children: [
                  for (final i in r) ...[
                    if (i != r.first) const SizedBox(width: 12),
                    DButton(
                      width: 170,
                      height: 100,
                      fill: i == w['active'] ? look.accent : null,
                      onTap: () => set('pick', i),
                      child: Text(tabs[i], style: Look.font(13, look.text, bold: true)),
                    ),
                  ],
                ]),
              ),
            DButton(onTap: () => set('done'), child: const Text('Done')),
          ]),
        ),
      ),
    ]);
  }
}

/// Help > Keyboard Shortcuts: each section's commands with their keys at the right
/// (monospace), scrolling in a 460 × 560 window.
class _ShortcutsBody extends StatelessWidget {
  const _ShortcutsBody({required this.w});
  final Map<String, dynamic> w;
  @override
  Widget build(BuildContext context) {
    final look = LookScope.of(context);
    final mono = Look.font(13, look.text).copyWith(fontFamily: 'monospace', fontFamilyFallback: const ['DejaVuSans']);
    final kids = <Widget>[];
    for (final sec in w['sections'] as List) {
      kids.add(const SizedBox(height: 6));
      kids.add(SizedBox(height: 24, child: Align(alignment: Alignment.bottomLeft, child: DLabel(sec[0] as String, bold: true))));
      for (final r in sec[1] as List) {
        kids.add(SizedBox(
          height: 30,
          child: Row(children: [Expanded(child: DLabel(r[0] as String)), Text(r[1] as String, style: mono)]),
        ));
      }
    }
    return SizedBox(width: 446, height: 510, child: Scrollbar(child: ListView(padding: const EdgeInsets.only(right: 4), children: kids)));
  }
}

/// egui's own ComboBox (not the theme's): the choice and a ▼.
class _EguiCombo extends StatelessWidget {
  const _EguiCombo({required this.text, required this.items, required this.onPick});
  final String text;
  final List<String> items;
  final void Function(int) onPick;
  @override
  Widget build(BuildContext context) {
    final look = LookScope.of(context);
    return Builder(
      builder: (ctx) => GestureDetector(
        onTap: () async {
          final box = ctx.findRenderObject() as RenderBox;
          final r = box.localToGlobal(Offset.zero) & box.size;
          final v = await showDesktopList<int>(ctx, r, [for (final (i, t) in items.indexed) DChoice(t, i, on: t == text)]);
          if (v != null) onPick(v);
        },
        // (at least egui's combo_width, 100)
        child: Container(
          height: 24,
          constraints: const BoxConstraints(minWidth: 100),
          padding: const EdgeInsets.only(left: 10, right: 8),
          decoration: BoxDecoration(color: look.control, borderRadius: BorderRadius.circular(6), border: Border.all(color: look.controlStroke)),
          child: Row(mainAxisSize: MainAxisSize.min, mainAxisAlignment: MainAxisAlignment.spaceBetween, children: [Text(text, style: Look.font(13, look.text)), const SizedBox(width: 8), Text('\u25bc', style: Look.font(11, look.text))]),
        ),
      ),
    );
  }
}

/// Format > Conditional Highlighting…: a number rule for the selected cells.
class _CondBody extends StatefulWidget {
  const _CondBody({required this.w, required this.set});
  final Map<String, dynamic> w;
  final _Set set;
  @override
  State<_CondBody> createState() => _CondBodyState();
}

class _CondBodyState extends State<_CondBody> {
  late final _v = TextEditingController(text: widget.w['value'] as String? ?? '');

  @override
  void dispose() {
    _v.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final look = LookScope.of(context);
    final w = widget.w;
    final cmps = [for (final c in w['cmps'] as List) c as String], looks = [for (final c in w['looks'] as List) c as String];
    final ok = w['can'] == true && double.tryParse(_v.text.trim().replaceAll(',', '')) != null;
    return EguiColumn(children: [
      Text(w['label'] as String, style: Look.font(13, look.text).copyWith(fontStyle: w['italic'] == true ? FontStyle.italic : null)),
      const DSpace(4),
      Row(children: [
        const DLabel('Number'),
        const SizedBox(width: 8),
        _EguiCombo(text: cmps[w['cmp'] as int], items: cmps, onPick: (i) => widget.set('cmp', i)),
        const SizedBox(width: 8),
        DTextField(
          controller: _v,
          width: 70,
          onChanged: (t) {
            widget.set('value', t);
            setState(() {});
          },
        ),
      ]),
      Row(children: [const DLabel('Style'), const SizedBox(width: 8), _EguiCombo(text: looks[w['look'] as int], items: looks, onPick: (i) => widget.set('look', i))]),
      const DSpace(4),
      Align(
        alignment: Alignment.centerLeft,
        child: _hz([
          DButton(onTap: ok ? () => widget.set('add') : null, child: const Text('Add Rule')),
          DButton(onTap: w['hasRules'] == true ? () => widget.set('clear') : null, child: const Text('Clear All Rules')),
        ]),
      ),
    ]);
  }
}

/// Edit Chart Data (kn_ui chartdata): Plot Rows / Columns as Series, + / − Series and
/// Category, then the grid (series swatches, X / Y / Size tags, unused categories dimmed) and
/// how the chart reads it. Each edit is one undo step in the engine.
class _ChartDataBody extends StatelessWidget {
  const _ChartDataBody({required this.w, required this.set});
  final Map<String, dynamic> w;
  final _Set set;

  @override
  Widget build(BuildContext context) {
    final look = LookScope.of(context);
    final byRow = w['byRow'] == true, xy = w['xy'] == true;
    final rows = [for (final r in w['rowNames'] as List) r as String], cols = [for (final c in w['colNames'] as List) c as String];
    final cells = [for (final r in w['cells'] as List) [for (final c in r as List) c as String]];
    final colors = w['colors'] as List, roles = w['roles'] as List, used = w['used'] as List;
    Widget swatch(int i) {
      final c = i < colors.length ? colors[i] as List? : null;
      return Container(
        width: 10,
        height: 10,
        margin: const EdgeInsets.only(right: 4),
        decoration: BoxDecoration(color: c == null ? null : Color.fromARGB(c[3] as int, c[0] as int, c[1] as int, c[2] as int), borderRadius: BorderRadius.circular(2)),
      );
    }

    Widget tag(int i) {
      final t = i < roles.length ? roles[i] as String : '';
      return t.isEmpty ? const SizedBox() : Padding(padding: const EdgeInsets.only(right: 4), child: Text(t, style: Look.font(9, look.accent, bold: true)));
    }

    bool isUsed(int i) => i < used.length && used[i] == true;
    Widget cell(String text, double width, bool dim, void Function(String) done) => Padding(
          padding: const EdgeInsets.only(right: 4, bottom: 4),
          child: _GridCell(key: ValueKey('$text$width'), text: text, width: width, dim: dim, onDone: done),
        );
    final grid = Column(crossAxisAlignment: CrossAxisAlignment.start, mainAxisSize: MainAxisSize.min, children: [
      if (!byRow && xy)
        Row(children: [const SizedBox(width: 128), for (final (j, _) in cols.indexed) SizedBox(width: 114, child: tag(j))]),
      Row(children: [
        const SizedBox(width: 128),
        for (final (j, c) in cols.indexed)
          Row(children: [if (!byRow) swatch(j), cell(c, byRow ? 96 : 80, byRow && !isUsed(j), (t) => set('colName', {'i': j, 'text': t}))]),
      ]),
      for (final (r, name) in rows.indexed)
        Row(children: [
          SizedBox(
            width: 128,
            child: Row(children: [
              if (byRow) ...[swatch(r), tag(r)],
              Expanded(child: cell(name, 110, !byRow && !isUsed(r), (t) => set('rowName', {'i': r, 'text': t}))),
            ]),
          ),
          for (final (j, v) in cells[r].indexed) cell(v, 96, !(byRow ? isUsed(j) : isUsed(r)), (t) => set('cell', {'r': r, 'c': j, 'text': t})),
        ]),
    ]);
    return SizedBox(
      width: 546,
      child: EguiColumn(children: [
        Align(alignment: Alignment.centerLeft, child: SizedBox(width: 380, child: _Switch(labels: const ['Plot Rows as Series', 'Plot Columns as Series'], cur: byRow ? 0 : 1, onPick: (i) => set('dir', i)))),
        if (xy) DCheckbox(on: w['sharedX'] == true, label: 'Share X Values', onChanged: (v) => set('sharedX', v)),
        Align(
          alignment: Alignment.centerLeft,
          child: _hz([
            DButton(onTap: () => set('addSeries'), child: const Text('+ Series')),
            DButton(onTap: () => set('addCategory'), child: const Text('+ Category')),
            DButton(onTap: w['canRemoveSeries'] == true ? () => set('removeSeries') : null, child: const Text('\u2212 Series')),
            DButton(onTap: w['canRemoveCategory'] == true ? () => set('removeCategory') : null, child: const Text('\u2212 Category')),
          ]),
        ),
        const DSeparator(),
        ConstrainedBox(
          constraints: const BoxConstraints(maxHeight: 380),
          child: SingleChildScrollView(scrollDirection: Axis.vertical, child: SingleChildScrollView(scrollDirection: Axis.horizontal, child: grid)),
        ),
        Text(w['note'] as String, style: Look.font(9, look.textDim)),
      ]),
    );
  }
}

/// A chart data cell: a 22-tall field that hands its text back when it loses the keyboard.
class _GridCell extends StatefulWidget {
  const _GridCell({super.key, required this.text, required this.width, required this.dim, required this.onDone});
  final String text;
  final double width;
  final bool dim;
  final void Function(String) onDone;
  @override
  State<_GridCell> createState() => _GridCellState();
}

class _GridCellState extends State<_GridCell> {
  late final _c = TextEditingController(text: widget.text);
  final _f = FocusNode();

  @override
  void initState() {
    super.initState();
    _f.addListener(() {
      if (!_f.hasFocus && _c.text != widget.text) widget.onDone(_c.text);
      setState(() {});
    });
  }

  @override
  void dispose() {
    _c.dispose();
    _f.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final look = LookScope.of(context);
    return Container(
      width: widget.width,
      height: 22,
      padding: const EdgeInsets.symmetric(horizontal: 8),
      decoration: BoxDecoration(
        color: look.field,
        borderRadius: BorderRadius.circular(6),
        border: Border.all(color: _f.hasFocus ? look.text.withValues(alpha: 0.6) : look.controlStroke),
      ),
      child: TextField(
        controller: _c,
        focusNode: _f,
        style: Look.font(13, widget.dim ? look.textDim : look.text),
        cursorColor: look.text,
        decoration: const InputDecoration(isCollapsed: true, border: InputBorder.none, contentPadding: EdgeInsets.zero),
        textAlignVertical: TextAlignVertical.center,
        onSubmitted: (t) => widget.onDone(t),
      ),
    );
  }
}

/// kn_ui chartdata switch: a 26-tall rounded bar, the chosen one in the accent with white text.
class _Switch extends StatelessWidget {
  const _Switch({required this.labels, required this.cur, required this.onPick});
  final List<String> labels;
  final int cur;
  final void Function(int) onPick;
  @override
  Widget build(BuildContext context) {
    final look = LookScope.of(context);
    return Container(
      height: 26,
      decoration: BoxDecoration(color: look.gray(0x33), borderRadius: BorderRadius.circular(7)),
      child: Row(children: [
        for (final (i, l) in labels.indexed)
          Expanded(
            child: GestureDetector(
              behavior: HitTestBehavior.opaque,
              onTap: () => onPick(i),
              child: Container(
                margin: const EdgeInsets.all(1),
                decoration: BoxDecoration(color: i == cur ? look.accent : null, borderRadius: BorderRadius.circular(6)),
                alignment: Alignment.center,
                child: Text(l, style: Look.font(12.5, i == cur ? Colors.white : look.text)),
              ),
            ),
          ),
      ]),
    );
  }
}

/// The title's popover (chrome.rs title): Name, Where and Format of the document, in a dark
/// rounded panel under the title; a tap elsewhere closes it keeping the name typed (as
/// Numbers), Escape without.
class _DocTitlePopover extends StatefulWidget {
  const _DocTitlePopover({super.key, required this.w, required this.at, required this.set});
  final Map<String, dynamic> w;
  final Rect at;
  final _Set set;
  @override
  State<_DocTitlePopover> createState() => _DocTitlePopoverState();
}

class _DocTitlePopoverState extends State<_DocTitlePopover> {
  late final _name = TextEditingController(text: widget.w['name'] as String? ?? '');

  @override
  void dispose() {
    _name.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final look = LookScope.of(context);
    final w = widget.w;
    final places = [for (final p in w['places'] as List) p as List];
    final formats = [for (final f in (w['formats'] as List? ?? const ['Pages', 'Word', 'Word + Letters'])) f as String];
    Widget row(String label, Widget field) => Padding(
          padding: const EdgeInsets.only(bottom: 10),
          child: Row(children: [SizedBox(width: 60, child: DLabel(label)), const SizedBox(width: 10), SizedBox(width: 220, child: field)]),
        );
    return Stack(children: [
      // a tap elsewhere: closed, keeping the name typed
      Positioned.fill(child: GestureDetector(behavior: HitTestBehavior.translucent, onTapDown: (_) => widget.set('apply'))),
      Positioned(
        left: widget.at.left,
        top: widget.at.bottom + 6,
        child: Focus(
          onKeyEvent: (n, e) {
            if (e is KeyDownEvent && e.logicalKey == LogicalKeyboardKey.escape) {
              widget.set('cancel');
              return KeyEventResult.handled;
            }
            return KeyEventResult.ignored;
          },
          child: Container(
            width: 300 + 28,
            padding: const EdgeInsets.all(14),
            decoration: BoxDecoration(
              color: look.gray(0x2c),
              borderRadius: BorderRadius.circular(10),
              border: Border.all(color: look.gray(0x48)),
              boxShadow: const [BoxShadow(color: Color(0x6E000000), offset: Offset(0, 4), blurRadius: 16)],
            ),
            child: Column(crossAxisAlignment: CrossAxisAlignment.start, mainAxisSize: MainAxisSize.min, children: [
              row('Name:', DTextField(controller: _name, autofocus: true, onChanged: (t) => widget.set('edit', {'name': t}), onSubmitted: () => widget.set('apply'))),
              if (!Platform.isAndroid) row(
                'Where:',
                _EguiCombo(
                  text: w['dirName'] as String? ?? '',
                  items: [for (final p in places) '\u{1F4C1} ${p[0]}'],
                  onPick: (i) => widget.set('edit', {'dir': places[i][1]}),
                ),
              ),
              row('Format:', _EguiCombo(text: formats[(w['format'] as int? ?? 0).clamp(0, 2)], items: formats, onPick: (i) => widget.set('format', {'pick': i}))),
              Text(w['note'] as String? ?? '', style: Look.font(9, look.textDim)),
            ]),
          ),
        ),
      ),
    ]);
  }
}

/// View > Customize Toolbar… (toolbar.rs sheet): drops down from the toolbar; every item to drag
/// into it (those already in it dimmed), the default set as one strip to drag in, the display
/// mode and Done.
class _CustomizeSheet extends StatelessWidget {
  const _CustomizeSheet({required this.w, required this.top, required this.set});
  final Map<String, dynamic> w;
  final double top;
  final _Set set;

  @override
  Widget build(BuildContext context) {
    final look = LookScope.of(context);
    final items = [for (final i in w['items'] as List) i as Map<String, dynamic>];
    final defaults = [for (final d in w['defaults'] as List) d as String];
    const names = ['Icon and Text', 'Icon Only', 'Text Only'];
    final mode = ((w['mode'] as int?) ?? 1).clamp(1, 3);
    return LayoutBuilder(builder: (ctx, box) {
      final width = math.min((box.maxWidth - 60).clamp(600.0, 1000.0), box.maxWidth - 40);
      final inner = width - 40;
      final cols = math.max(4, (inner / 104).floor());
      final cellW = inner / cols;
      final gridH = math.max(160.0, box.maxHeight - top - 300);
      Widget glyph(Map<String, dynamic> it, double dim) {
        final id = it['id'] as String;
        return CustomPaint(
          size: Size(cellW, 74),
          painter: _FnP((c, s) {
            final ic = Offset(s.width / 2, 24);
            final col = look.gray(0xee).withValues(alpha: dim);
            switch (id) {
              case 'Space':
                c.drawRect(Rect.fromCenter(center: ic, width: 30, height: 30), Paint()
                  ..style = PaintingStyle.stroke
                  ..color = look.gray(0x80).withValues(alpha: dim));
              case 'FlexibleSpace':
                c.drawRect(Rect.fromCenter(center: ic, width: 52, height: 30), Paint()
                  ..style = PaintingStyle.stroke
                  ..color = look.gray(0x80));
                _paintText(c, '\u2190\u00b7\u00b7\u00b7\u2192', Look.font(10, look.gray(0xa0)), ic);
              case 'Zoom':
                c.drawRRect(RRect.fromRectAndRadius(Rect.fromCenter(center: ic, width: 64, height: 30), const Radius.circular(15)), Paint()..color = look.gray(0x40).withValues(alpha: dim));
                _paintText(c, '100%', Look.font(12.5, col), ic);
              default:
                c.drawCircle(ic, 20, Paint()..color = look.capsule.withValues(alpha: dim));
                final ir = Rect.fromCenter(center: ic, width: 22, height: 22);
                if (!drawToolbarGlyph(c, ir, id, col)) {
                  final i = iconNamed(it['icon'] as String? ?? '');
                  if (i != null) drawIcon(c, ir, i, col);
                }
            }
            _paintText(c, it['label'] as String? ?? '', Look.font(12, look.text.withValues(alpha: dim)), Offset(s.width / 2, 56));
          }),
        );
      }

      return Stack(children: [
        Positioned(
          left: (box.maxWidth - width) / 2,
          top: top,
          width: width,
          child: Container(
            padding: const EdgeInsets.all(20),
            decoration: BoxDecoration(
              color: look.gray(0x2a),
              borderRadius: BorderRadius.circular(14),
              border: Border.all(color: look.gray(0x44)),
              boxShadow: const [BoxShadow(color: Color(0x78000000), offset: Offset(0, 8), blurRadius: 28)],
            ),
            child: Column(crossAxisAlignment: CrossAxisAlignment.start, mainAxisSize: MainAxisSize.min, children: [
              Text('Drag your favorite items into the toolbar\u2026', style: Look.font(13.5, look.text)),
              const SizedBox(height: 10),
              ConstrainedBox(
                constraints: BoxConstraints(maxHeight: gridH),
                child: SingleChildScrollView(
                  child: Wrap(runSpacing: 6, children: [
                    for (final it in items)
                      Tooltip(
                        message: it['tip'] as String? ?? '',
                        waitDuration: const Duration(milliseconds: 600),
                        child: Draggable<TbPayload>(
                          data: TbPayload(id: it['id'] as String),
                          feedback: TbDragImage(look: look, id: it['id'] as String, label: it['label'] as String? ?? '', icon: it['icon'] as String? ?? ''),
                          child: glyph(it, it['used'] == true ? 0.35 : 1),
                        ),
                      ),
                  ]),
                ),
              ),
              const SizedBox(height: 14),
              Text('\u2026 or drag the default set into the toolbar.', style: Look.font(13.5, look.text)),
              const SizedBox(height: 8),
              // the default set as one strip
              Draggable<TbPayload>(
                data: const TbPayload(defaults: true),
                feedback: TbDragImage(look: look, id: 'View', label: 'Default Set', icon: 'Sidebar'),
                child: Container(
                  height: 62,
                  decoration: BoxDecoration(color: look.gray(0x33), borderRadius: BorderRadius.circular(10)),
                  child: CustomPaint(
                    size: Size(inner, 62),
                    painter: _FnP((c, s) {
                      final step = (s.width - 40) / math.max(1, defaults.length);
                      // (too narrow for the names, a phone's: the icons alone, in the middle)
                      final named = step >= 56;
                      for (final (n, id) in defaults.indexed) {
                        final m = Offset(20 + step * (n + 0.5), named ? 22 : 31);
                        final it = items.firstWhere((x) => x['id'] == id, orElse: () => const {});
                        final ir = Rect.fromCenter(center: m, width: 20, height: 20);
                        if (!drawToolbarGlyph(c, ir, id, look.gray(0xee))) {
                          final i = iconNamed(it['icon'] as String? ?? '');
                          if (i != null) drawIcon(c, ir, i, look.gray(0xee));
                        }
                        if (named) _paintText(c, it['label'] as String? ?? id, Look.font(11, look.textDim), m + const Offset(0, 25));
                      }
                    }),
                  ),
                ),
              ),
              const SizedBox(height: 14),
              const DSeparator(),
              const SizedBox(height: 6),
              Row(children: [
                const DLabel('Show'),
                const SizedBox(width: 8),
                _EguiCombo(text: names[mode - 1], items: names, onPick: (i) => set('mode', i + 1)),
                const Spacer(),
                DButton(width: 80, height: 26, fill: look.accent, onTap: () => set('done'), child: Text('Done', style: Look.font(13, Colors.white))),
              ]),
            ]),
          ),
        ),
      ]);
    });
  }
}

class _FnP extends CustomPainter {
  _FnP(this.f);
  final void Function(Canvas, Size) f;
  @override
  void paint(Canvas c, Size s) => f(c, s);
  @override
  bool shouldRepaint(_FnP o) => true;
}

/// Text centred on `at`.
void _paintText(Canvas c, String t, TextStyle style, Offset at) {
  final tp = TextPainter(text: TextSpan(text: t, style: style), textDirection: TextDirection.ltr, maxLines: 1)..layout();
  tp.paint(c, at - Offset(tp.width / 2, tp.height / 2));
}

/// File > Print…: Print Setup (fileops.rs print_setup): the sheet's paper, orientation, content
/// scale or Fit, page order, margins, page numbers and options, then Export PDF… or Print.
class _PrintSetupBody extends StatelessWidget {
  const _PrintSetupBody({required this.w, required this.set, required this.onPrint});
  final Map<String, dynamic> w;
  final _Set set;
  final void Function(bool export) onPrint;
  @override
  Widget build(BuildContext context) {
    final look = LookScope.of(context);
    bool b(String k) => w[k] == true;
    final papers = [for (final p in w['papers'] as List) p as String];
    final paper = w['paper'] as int?;
    final margins = [for (final m in w['margins'] as List) (m as num).toDouble()];
    final pages = w['pages'] as int? ?? 1;
    return SizedBox(
      width: 360,
      child: EguiColumn(children: [
        DLabel('\u201c${w['sheet']}\u201d: $pages page${pages == 1 ? '' : 's'}', bold: true),
        ...dSection('Paper'),
        DCombo<int>(text: paper == null ? 'Custom' : papers[paper], items: () => [for (final (i, p) in papers.indexed) DChoice(p, i, on: i == paper)], onPick: (i) => set('paper', i)),
        DSegmented(labels: const ['Portrait', 'Landscape'], cur: b('portrait') ? 0 : 1, onPick: (i) => set('portrait', i == 0)),
        ...dSection('Content Scale'),
        DCheckbox(on: b('autofit'), label: 'Fit to the page', onChanged: (v) => set('autofit', v)),
        if (!b('autofit')) DRow('Scale', DStepper(value: ((w['scale'] as num?) ?? 1).toDouble() * 100, step: 5, min: 10, max: 400, suffix: ' %', onChanged: (v) => set('scale', v))),
        ...dSection('Page Order'),
        DSegmented(labels: const ['Down, Over', 'Over, Down'], cur: b('downFirst') ? 0 : 1, onPick: (i) => set('downFirst', i == 0)),
        ...dSection('Margins'),
        for (final (i, n) in const ['Top', 'Left', 'Bottom', 'Right'].indexed) DRow(n, DStepper(value: margins[i], step: 6, min: 0, max: 144, suffix: ' pt', onChanged: (v) => set('margin', [i, v]))),
        ...dSection('Page Numbers'),
        DCheckbox(on: b('pageNumbers'), label: 'Show page numbers', onChanged: (v) => set('pageNumbers', v)),
        DCheckbox(on: w['startPage'] != null, label: 'Start at a page of my own', onChanged: (v) => set('startPage', v ? 1 : null)),
        if (w['startPage'] != null) DRow('Start at', DStepper(value: (w['startPage'] as num).toDouble(), min: 1, max: 9999, onChanged: (v) => set('startPage', v.round()))),
        ...dSection('Options'),
        DCheckbox(on: b('repeatHeaders'), label: 'Repeat table headers', onChanged: (v) => set('repeatHeaders', v)),
        DCheckbox(on: b('backgrounds'), label: 'Print backgrounds', onChanged: (v) => set('backgrounds', v)),
        DCheckbox(on: b('comments'), label: 'Print comments', onChanged: (v) => set('comments', v)),
        const DSpace(10),
        Row(children: [
          Expanded(child: DButton(height: 40, onTap: () => onPrint(true), child: const Text('Export PDF\u2026'))),
          const SizedBox(width: 10),
          Expanded(child: DButton(height: 40, fill: look.accent, onTap: () => onPrint(false), child: const Text('Print', style: TextStyle(color: Colors.white)))),
        ]),
      ]),
    );
  }
}


/// A label and its field, as egui's two-column Grid (spacing 10 × 8).
Widget _gridRow(String label, Widget field, {double labelW = 70, double fieldW = 200}) => Padding(
      padding: const EdgeInsets.only(bottom: 8),
      child: Row(mainAxisSize: MainAxisSize.min, children: [SizedBox(width: labelW, child: DLabel(label)), const SizedBox(width: 10), SizedBox(width: fieldW, child: field)]),
    );

/// Format > Add Link (links.rs window): Numbers' link popover: Display (text links), Link to
/// (Webpage / Email / Phone Number / Sheet), the address or sheet, Remove or Cancel, and Add
/// Link / Open Link / Compose Email / Call / Go to Sheet.
class _ValuesLinkBody extends StatefulWidget {
  const _ValuesLinkBody({super.key, required this.w, required this.set});
  final Map<String, dynamic> w;
  final _Set set;
  @override
  State<_ValuesLinkBody> createState() => _ValuesLinkBodyState();
}

class _ValuesLinkBodyState extends State<_ValuesLinkBody> {
  late final _display = TextEditingController(text: widget.w['display'] as String? ?? '');
  late final _addr = TextEditingController(text: widget.w['addr'] as String? ?? '');
  late final _subject = TextEditingController(text: widget.w['subject'] as String? ?? '');

  @override
  void dispose() {
    _display.dispose();
    _addr.dispose();
    _subject.dispose();
    super.dispose();
  }

  Map<String, Object?> get _fields => {'display': _display.text, 'addr': _addr.text, 'subject': _subject.text};

  @override
  Widget build(BuildContext context) {
    final w = widget.w;
    final kinds = [for (final k in w['kinds'] as List) k as String];
    final kind = w['kind'] as String? ?? 'Webpage';
    final sheets = [for (final n in w['sheets'] as List) n as String];
    final sheet = (w['sheet'] as int? ?? 0).clamp(0, math.max(0, sheets.length - 1)).toInt();
    final existing = w['existing'] == true;
    final ready = kind == 'Sheet' || _addr.text.trim().isNotEmpty;
    final go = switch (kind) { 'Sheet' => 'Go to Sheet', 'Email' => 'Compose Email', 'Phone Number' => 'Call', _ => existing ? 'Open Link' : 'Add Link' };
    void edit() => widget.set('edit', _fields);
    return SizedBox(
      width: 300,
      child: EguiColumn(children: [
        if (w['hasText'] == true) _gridRow('Display:', DTextField(controller: _display, onChanged: (_) => edit())),
        _gridRow('Link to:', _EguiCombo(text: kind, items: kinds, onPick: (i) => widget.set('edit', {..._fields, 'kind': kinds[i]}))),
        if (kind == 'Sheet')
          _gridRow('Sheet:', _EguiCombo(text: sheets.isEmpty ? '' : sheets[sheet], items: sheets, onPick: (i) => widget.set('edit', {..._fields, 'sheet': i})))
        else ...[
          _gridRow(
            kind == 'Email' ? 'To:' : kind == 'Phone Number' ? 'Number:' : 'Link:',
            DTextField(
              controller: _addr,
              autofocus: !existing,
              hint: switch (kind) { 'Email' => 'name@example.com', 'Phone Number' => '+1 555 123 4567', _ => 'www.example.com' },
              onChanged: (_) => setState(edit),
              onSubmitted: ready ? () => widget.set('done', _fields) : null,
            ),
          ),
          if (kind == 'Email') _gridRow('Subject:', DTextField(controller: _subject, onChanged: (_) => edit())),
        ],
        const DSpace(4),
        Row(children: [
          Expanded(child: existing ? DButton(onTap: () => widget.set('remove', _fields), child: const Text('Remove')) : DButton(onTap: () => widget.set('cancel'), child: const Text('Cancel'))),
          const SizedBox(width: 8),
          Expanded(child: DButton(onTap: ready ? () => widget.set('go', _fields) : null, child: Text(go))),
        ]),
      ]),
    );
  }
}

/// Insert > Formula > Stock Quote (stocks.rs window): Symbol and Attribute, the note on where
/// quotes come from, Refresh All, Cancel and Insert.
class _StockBody extends StatefulWidget {
  const _StockBody({super.key, required this.w, required this.set});
  final Map<String, dynamic> w;
  final _Set set;
  @override
  State<_StockBody> createState() => _StockBodyState();
}

class _StockBodyState extends State<_StockBody> {
  late final _sym = TextEditingController(text: widget.w['symbol'] as String? ?? '');

  @override
  void dispose() {
    _sym.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final look = LookScope.of(context);
    final w = widget.w;
    final attrs = [for (final a in w['attrs'] as List) a as List];
    final names = [for (final a in attrs) a[0] as String];
    final attr = (w['attr'] as int? ?? 0).clamp(0, names.length - 1);
    final busy = w['busy'] == true;
    final ok = _sym.text.trim().isNotEmpty && !busy;
    return SizedBox(
      width: 260,
      child: EguiColumn(children: [
        _gridRow('Symbol:', DTextField(controller: _sym, autofocus: true, hint: 'AAPL', onChanged: (t) => setState(() => widget.set('edit', {'symbol': t})), onSubmitted: ok ? () => widget.set('insert') : null), labelW: 70, fieldW: 160),
        _gridRow('Attribute:', _EguiCombo(text: names[attr], items: names, onPick: (i) => attrs[i][1] == true ? widget.set('edit', {'attr': i}) : null), labelW: 70, fieldW: 160),
        Text('Quotes come from Yahoo Finance; only the symbol is sent.', style: Look.font(10, look.textDim)),
        const DSpace(4),
        Row(children: [
          DButton(onTap: busy ? null : () => widget.set('refresh'), child: const Text('Refresh All')),
          if (busy) const Padding(padding: EdgeInsets.only(left: 8), child: SizedBox(width: 14, height: 14, child: CircularProgressIndicator(strokeWidth: 2))),
          const Spacer(),
          DButton(onTap: () => widget.set('cancel'), child: const Text('Cancel')),
          const SizedBox(width: 8),
          DButton(onTap: ok ? () => widget.set('insert') : null, child: const Text('Insert')),
        ]),
      ]),
    );
  }
}

/// View > Show Warnings (fonts.rs warnings_window): each warning, Replace Fonts… when a font is
/// missing, Copy.
class _WarningsBody extends StatelessWidget {
  const _WarningsBody({required this.w, required this.set});
  final Map<String, dynamic> w;
  final _Set set;
  @override
  Widget build(BuildContext context) {
    final look = LookScope.of(context);
    final list = [for (final l in w['list'] as List) l as String];
    return SizedBox(
      width: 460,
      child: EguiColumn(children: [
        if (list.isEmpty) DLabel('There are no warnings for this spreadsheet.', dim: true),
        ConstrainedBox(
          constraints: const BoxConstraints(maxHeight: 320),
          child: SingleChildScrollView(
            child: Column(crossAxisAlignment: CrossAxisAlignment.start, mainAxisSize: MainAxisSize.min, children: [
              for (final l in list) Padding(padding: const EdgeInsets.only(bottom: 6), child: Text('⚠  $l', style: Look.font(13, look.text).copyWith(height: 1.25))),
            ]),
          ),
        ),
        const DSpace(6),
        Align(
          alignment: Alignment.centerLeft,
          child: _hz([
            if (w['missingFonts'] == true) DButton(onTap: () => set('replace'), child: const Text('Replace Fonts…')),
            DButton(onTap: () => Clipboard.setData(ClipboardData(text: list.join('\n'))), child: const Text('Copy')),
          ]),
        ),
      ]),
    );
  }
}

/// File > Save as Template… (templates.rs): the name; Add to Template Chooser, or Save… as an
/// .nmbtemplate file where the system's file picker says.
class _SaveTemplateBody extends StatefulWidget {
  const _SaveTemplateBody({super.key, required this.w, required this.set});
  final Map<String, dynamic> w;
  final _Set set;
  @override
  State<_SaveTemplateBody> createState() => _SaveTemplateBodyState();
}

class _SaveTemplateBodyState extends State<_SaveTemplateBody> {
  late final _name = TextEditingController(text: widget.w['text'] as String? ?? '');

  @override
  void dispose() {
    _name.dispose();
    super.dispose();
  }

  Future<void> _saveFile() async {
    final name = _name.text.trim().isEmpty ? 'Template' : _name.text.trim();
    final dir = await getTemporaryDirectory();
    final out = '${dir.path}/$name.nmbtemplate';
    final f = File(out);
    if (f.existsSync()) f.deleteSync();
    widget.set('save', {'path': out});
    if (!f.existsSync()) return;
    await FilePicker.saveFile(fileName: '$name.nmbtemplate', bytes: await f.readAsBytes());
  }

  @override
  Widget build(BuildContext context) {
    return SizedBox(
      width: 340,
      child: EguiColumn(children: [
        Text('Add this spreadsheet to the template chooser, or save it as a template file.', style: Look.font(13, LookScope.of(context).text).copyWith(height: 1.25)),
        const DSpace(6),
        _gridRow('Name', DTextField(controller: _name, autofocus: true, onChanged: (t) => setState(() => widget.set('edit', {'text': t}))), labelW: 40, fieldW: 240),
        const DSpace(4),
        Align(
          alignment: Alignment.centerLeft,
          child: _hz([
            DButton(onTap: _name.text.trim().isEmpty ? null : () => widget.set('add', {'text': _name.text}), child: const Text('Add to Template Chooser')),
            DButton(onTap: _saveFile, child: const Text('Save…')),
          ]),
        ),
      ]),
    );
  }
}

/// File > Advanced > Language & Region… (templates.rs language_window): Language and Region,
/// what the formats look like, OK.
class _LanguageBody extends StatelessWidget {
  const _LanguageBody({required this.w, required this.set});
  final Map<String, dynamic> w;
  final _Set set;
  @override
  Widget build(BuildContext context) {
    final look = LookScope.of(context);
    final langs = [for (final l in w['langs'] as List) l as List];
    final regions = [for (final r in w['regions'] as List) r as List];
    String name(List<List> all, String code, String or) => all.firstWhere((x) => x[0] == code, orElse: () => [code, or])[1] as String;
    return SizedBox(
      width: 320,
      child: EguiColumn(children: [
        Text("Choose the language and region for this spreadsheet's formats.", style: Look.font(13, look.text).copyWith(height: 1.25)),
        const DSpace(6),
        _gridRow('Language', _EguiCombo(text: name(langs, w['lang'] as String, 'English'), items: [for (final l in langs) l[1] as String], onPick: (i) => set('edit', {'lang': langs[i][0]})), labelW: 70, fieldW: 220),
        _gridRow('Region', _EguiCombo(text: name(regions, w['region'] as String, 'United States'), items: [for (final r in regions) r[1] as String], onPick: (i) => set('edit', {'region': regions[i][0]})), labelW: 70, fieldW: 220),
        DLabel(w['sample'] as String? ?? '', dim: true),
        const DSpace(8),
        Align(alignment: Alignment.centerRight, child: DButton(onTap: () => set('ok'), child: const Text('OK'))),
      ]),
    );
  }
}

/// View > Show Photo and Video Browser / Show Music Browser (media.rs): a search field and the
/// files found (~/Pictures and ~/Videos, ~/Music); a tap puts one on the sheet.
class _MediaBrowserBody extends StatefulWidget {
  const _MediaBrowserBody({super.key, required this.w, required this.set});
  final Map<String, dynamic> w;
  final _Set set;
  @override
  State<_MediaBrowserBody> createState() => _MediaBrowserBodyState();
}

class _MediaBrowserBodyState extends State<_MediaBrowserBody> {
  late final _q = TextEditingController(text: widget.w['query'] as String? ?? '');

  @override
  void dispose() {
    _q.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final files = [for (final f in widget.w['files'] as List) f as List];
    return SizedBox(
      width: 360,
      height: 420,
      child: Column(crossAxisAlignment: CrossAxisAlignment.stretch, children: [
        DTextField(controller: _q, hint: 'Search', onChanged: (t) => widget.set('query', t)),
        const SizedBox(height: 6),
        Expanded(
          child: files.isEmpty
              ? Align(alignment: Alignment.topLeft, child: DLabel(widget.w['empty'] as String? ?? '', dim: true))
              : ListView.builder(
                  itemCount: files.length,
                  itemBuilder: (ctx, i) => Padding(padding: const EdgeInsets.only(bottom: 4), child: _Selectable(false, files[i][0] as String, () => widget.set('pick', files[i][1]))),
                ),
        ),
      ]),
    );
  }
}
