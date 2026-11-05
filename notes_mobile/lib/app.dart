// The window's layouts, by width (iOS 26 / iPadOS 26 Notes, IPAD-SIZES.md): under 600 dp (a
// phone, a narrow window) a stack of three pages, Folders → notes → the note, pushed with iOS's
// slide and swiped back from the edge; from 600 dp the notes list and the note side by side, the
// folders sliding over from the left; from 1100 dp three columns (sidebar, list, note) while the
// sidebar toggle is on. Full Screen gives the note the window. A note column too narrow for the
// iPad bar even with its tools folded into » takes the iPhone's bars.
import 'dart:io';

import 'package:flutter/cupertino.dart';
import 'package:flutter/services.dart';
import 'package:flutter/material.dart';

import 'editor.dart';
import 'keys.dart';
import 'folders.dart';
import 'model.dart';
import 'notes_list.dart';
import 'state.dart';
import 'theme.dart';
import 'ui/dialogs.dart';
import 'ui/glass.dart';

class NotesHome extends StatefulWidget {
  const NotesHome({super.key, required this.state});
  final AppState state;
  @override
  State<NotesHome> createState() => _NotesHomeState();
}

class _NotesHomeState extends State<NotesHome> {
  AppState get s => widget.state;
  final _phone = GlobalKey<NavigatorState>();
  bool _fullScreen = false, _sidebarOver = false;

  /// The sidebar column's toggle (from 1100 dp: sidebar + list + note while it's on).
  bool _sidebarShown = true;

  static const _system = MethodChannel('app.bramble.notes/system');

  // (a hardware keyboard: the desktop's keys)
  late final _keys = NotesKeys(s, () => context, active: () => mounted && (ModalRoute.of(context)?.isCurrent ?? true), newNote: () => _newNote());

  @override
  void dispose() {
    if (s.keyHandler == _keys.handle) s.keyHandler = null;
    super.dispose();
  }

  @override
  void initState() {
    super.initState();
    s.keyHandler = _keys.handle;
    WidgetsBinding.instance.addPostFrameCallback((_) => _libraryNotice());
    if (!Platform.isAndroid) return;
    // what the app was opened for: Quick Note, New Note, something shared to Notes
    _system.setMethodCallHandler((call) async {
      if (call.method == 'intent') _handle((call.arguments as Map).cast());
    });
    WidgetsBinding.instance.addPostFrameCallback((_) async {
      final a = await _system.invokeMethod<Map>('pending');
      if (a != null) _handle(a.cast());
    });
  }

  /// The library converted to files on this open (or couldn't be): said once.
  Future<void> _libraryNotice() async {
    final m = s.libraryNotice;
    s.libraryNotice = null;
    if (m == null || !mounted) return;
    final error = m['error'] as String?;
    if (error == null) {
      final n = m['notes'] as num? ?? 0;
      s.say('Your library was updated ($n ${n == 1 ? 'note' : 'notes'}).');
      return;
    }
    await showAlert(
      context,
      title: 'Your Library Couldn\u2019t Be Updated',
      message: 'Notes kept your library as it was, and it works as before. Notes tries again the next time it opens.\n\n$error',
      actions: const [AlertAction('OK', bold: true)],
    );
  }

  /// Settings > Note Backgrounds: light pages in dark mode.
  Widget _noteTheme(BuildContext context, Widget editor) => Theme.of(context).brightness == Brightness.dark && s.settings.noteBackground == 'light'
      ? Theme(data: notesTheme(Brightness.light), child: editor)
      : editor;

  Future<void> _handle(Map<String, dynamic> a) async {
    final String? id;
    switch (a['kind']) {
      case 'quicknote':
        id = await s.quickNote();
      case 'newnote':
        s.place = s.defaultPlace;
        id = await s.newNote();
      case 'share':
        id = await s.sharedNote(subject: a['subject'] as String?, text: a['text'] as String?, files: ((a['files'] as List?) ?? const []).cast<String>());
      default:
        id = null;
    }
    if (id == null || !mounted) return;
    // the phone: straight to the note, over the list it's in
    final nav = _phone.currentState;
    if (nav != null) {
      nav.popUntil((r) => r.isFirst);
      nav.push(CupertinoPageRoute(builder: (_) => _phoneList()));
      nav.push(CupertinoPageRoute(builder: (_) => _phoneEditor()));
    }
  }

  @override
  Widget build(BuildContext context) {
    final p = Palette.of(context);
    return LayoutBuilder(
      builder: (ctx, box) {
        final w = box.maxWidth;
        final Widget layout;
        s.columns = !(w < 600 || MediaQuery.sizeOf(context).shortestSide < 600);
        if (!s.columns) {
          layout = _phoneStack();
        } else {
          // (IPAD-SIZES.md: list + note from 600 dp, the sidebar too from 1100 while it's on)
          layout = _columns(p, three: w >= 1100 && _sidebarShown, wide: w >= 1100);
        }
        return Material(
          type: MaterialType.transparency,
          child: Stack(
            children: [
              Positioned.fill(child: layout),
              // a moment's message at the foot
              Positioned(
                left: 0,
                right: 0,
                bottom: MediaQuery.of(context).padding.bottom + 80,
                child: IgnorePointer(
                  child: ValueListenableBuilder<String?>(
                    valueListenable: s.message,
                    builder: (ctx, m, _) => AnimatedOpacity(
                      opacity: m == null ? 0 : 1,
                      duration: const Duration(milliseconds: 200),
                      child: Center(
                        child: Glass(
                          radius: 20,
                          child: Padding(
                            padding: const EdgeInsets.symmetric(horizontal: 18, vertical: 11),
                            child: Text(m ?? '', style: Type.subhead(p.label)),
                          ),
                        ),
                      ),
                    ),
                  ),
                ),
              ),
            ],
          ),
        );
      },
    );
  }

  // ------------------------------------------------------------ the phone

  Widget _phoneStack() => PopScope(
    canPop: false,
    onPopInvokedWithResult: (did, _) {
      if (!did) _phone.currentState?.maybePop();
    },
    child: Navigator(
      key: _phone,
      onGenerateRoute: (_) => CupertinoPageRoute(builder: (_) => _folders(sidebar: false)),
    ),
  );

  Widget _folders({required bool sidebar, VoidCallback? onToggle}) => FoldersView(
    state: s,
    sidebar: sidebar,
    onToggleSidebar: onToggle,
    onOpen: (p) {
      s.choose(p);
      if (!sidebar) {
        _phone.currentState?.push(CupertinoPageRoute(builder: (_) => _phoneList()));
      } else if (_sidebarOver) {
        setState(() => _sidebarOver = false);
      }
    },
    onSearch: () {
      s.choose(const AllNotes());
      _phone.currentState?.push(CupertinoPageRoute(builder: (_) => _phoneList(searching: true)));
    },
    onNewNote: () => _newNote(),
  );

  Widget _phoneList({bool searching = false}) => NotesListView(
    state: s,
    searching: searching,
    onBack: () => _phone.currentState?.maybePop(),
    onOpenNote: (id) {
      s.open(id);
      _phone.currentState?.push(CupertinoPageRoute(builder: (_) => _phoneEditor()));
    },
    onNewNote: () => _newNote(),
  );

  Widget _phoneEditor() => ListenableBuilder(
    listenable: s,
    builder: (ctx, _) {
      final id = s.noteId;
      if (id == null) return ColoredBox(color: Palette.of(ctx).page);
      return _noteTheme(
        context,
        EditorView(key: const ValueKey('editor'), state: s, id: id, onBack: () => Navigator.of(ctx).maybePop(), onNewNote: () => _newNote(replace: true)),
      );
    },
  );

  Future<void> _newNote({bool replace = false}) async {
    final id = await s.newNote();
    if (id == null) return;
    final nav = _phone.currentState;
    if (nav == null) return;
    // (from Folders: the list goes under it, as Notes does)
    if (!replace) {
      nav.push(CupertinoPageRoute(builder: (_) => _phoneEditor()));
    }
  }

  // ------------------------------------------------------------ tablets

  Widget _columns(Palette p, {required bool three, required bool wide}) => ListenableBuilder(
    listenable: s,
    builder: (ctx, _) {
      final mq = MediaQuery.of(ctx);
      final id = s.noteId;
      final listW = three ? 320.0 : 340.0;
      final sideW = three ? 300.0 : 0.0;
      // a note column too narrow for the iPad toolbar (Split View, a square unfolded screen)
      // takes the iPhone's bars, as iPadOS's compact width does
      final room = mq.size.width - (_fullScreen ? 0 : sideW + listW + 0.5);
      // (the iPad bar, all it can fold away folded, is about 424 dp with its margins; narrower,
      // the note takes the iPhone's bars, as iPadOS's compact width does)
      final compact = room < 440;
      final several = s.selection.length > 1;
      final editor = id == null || several
          ? ColoredBox(
              color: p.page,
              child: Stack(
                children: [
                  Center(
                    child: Text(several ? '${s.selection.length} Notes Selected' : 'No Note Selected', style: Type.text(20, p.tertiary, weight: FontWeight.w600)),
                  ),
                  // (the note column's bar stays: full screen and compose leading, search trailing)
                  Positioned(
                    left: 12,
                    right: 12,
                    top: mq.padding.top + 6,
                    child: ToolbarRow(
                      children: [
                        GlassButton(
                          icon: _fullScreen ? CupertinoIcons.arrow_down_right_arrow_up_left : CupertinoIcons.arrow_up_left_arrow_down_right,
                          semantic: _fullScreen ? 'Exit Full Screen' : 'Enter Full Screen',
                          onTap: () => setState(() => _fullScreen = !_fullScreen),
                        ),
                        const SizedBox(width: 12),
                        GlassButton(icon: CupertinoIcons.square_pencil, semantic: 'New Note', onTap: () => s.newNote()),
                        const Spacer(),
                        GlassButton(
                          icon: CupertinoIcons.search,
                          semantic: 'Search',
                          onTap: () {
                            if (_fullScreen) setState(() => _fullScreen = false);
                            s.searchRequest.value++;
                          },
                        ),
                      ],
                    ),
                  ),
                ],
              ),
            )
          : _noteTheme(
              context,
              EditorView(
                key: const ValueKey('editor'),
                state: s,
                id: id,
                wide: !compact,
                fullScreen: _fullScreen,
                onFullScreen: () => setState(() => _fullScreen = !_fullScreen),
                onNewNote: () => s.newNote(),
              ),
            );
      final list = NotesListView(
        state: s,
        highlight: true,
        // the sidebar toggle leads the list's bar while the sidebar isn't showing: a column
        // again from 1100 dp, else the folders over the list
        leading: three
            ? null
            : GlassButton(
                icon: CupertinoIcons.sidebar_left,
                semantic: 'Show Sidebar',
                onTap: () => setState(() => wide ? _sidebarShown = true : _sidebarOver = true),
              ),
        onOpenNote: (n) => s.open(n),
        onNewNote: () => s.newNote(),
      );
      final sidebar = Padding(
        padding: EdgeInsets.fromLTRB(8, mq.padding.top + 8, 0, mq.padding.bottom + 8),
        // (no clip: the toolbar's glass buttons keep their whole shadows)
        child: MediaQuery.removePadding(context: ctx, removeTop: true, removeBottom: true, child: _folders(sidebar: true, onToggle: three ? () => setState(() => _sidebarShown = false) : null)),
      );
      // the columns slide away under Full Screen and the folders slide in over the list, on
      // iOS's push curve. Everything keeps its size and only moves (a transform, no layout),
      // each column on its own layer, so a frame repaints nothing that didn't change; the folders
      // stay built while hidden, so opening them builds nothing mid-slide.
      return ColoredBox(
        color: p.page,
        child: LayoutBuilder(
          builder: (ctx, box) {
            final colsW = sideW + listW + 0.5;
            // (the note takes the room it will have once the columns are in or out)
            final editorW = _fullScreen ? box.maxWidth : box.maxWidth - colsW;
            return TweenAnimationBuilder<double>(
              tween: Tween(end: _fullScreen ? 0 : 1),
              duration: const Duration(milliseconds: 420),
              curve: _push,
              child: RepaintBoundary(child: editor),
              builder: (ctx, cols, editorChild) {
                final shift = -colsW * (1 - cols);
                return Stack(
                  children: [
                    if (three)
                      Positioned(
                        left: 0,
                        top: 0,
                        bottom: 0,
                        width: sideW,
                        child: Transform.translate(
                          offset: Offset(shift, 0),
                          child: Offstage(offstage: cols == 0, child: RepaintBoundary(child: ColoredBox(color: p.sidebar, child: sidebar))),
                        ),
                      ),
                    Positioned(
                      left: sideW,
                      top: 0,
                      bottom: 0,
                      width: listW + 0.5,
                      child: Transform.translate(
                        offset: Offset(shift, 0),
                        child: Offstage(
                          offstage: cols == 0,
                          child: RepaintBoundary(child: Row(children: [SizedBox(width: listW, child: list), Container(width: 0.5, color: p.separator)])),
                        ),
                      ),
                    ),
                    Positioned(
                      left: _fullScreen ? 0 : colsW,
                      top: 0,
                      bottom: 0,
                      width: editorW,
                      child: Transform.translate(offset: Offset(_fullScreen ? colsW + shift : shift, 0), child: editorChild),
                    ),
                    // the folders over the list (two columns)
                    if (!three) _overlay(p, sidebar),
                  ],
                );
              },
            );
          },
        ),
      );
    },
  );

  /// The folders sliding in over the list (two columns): a dim behind them, then the folders on
  /// a layer of their own moving in from the left edge.
  Widget _overlay(Palette p, Widget sidebar) => Positioned.fill(
    child: TweenAnimationBuilder<double>(
      tween: Tween(end: _sidebarOver ? 1 : 0),
      duration: const Duration(milliseconds: 380),
      curve: _push,
      child: RepaintBoundary(
        child: DecoratedBox(
          decoration: BoxDecoration(
            color: p.sidebar,
            boxShadow: const [BoxShadow(color: Color(0x2E000000), blurRadius: 24)],
          ),
          child: sidebar,
        ),
      ),
      builder: (ctx, over, panel) => Stack(
        children: [
          if (over > 0)
            Positioned.fill(
              child: GestureDetector(
                onTap: () => setState(() => _sidebarOver = false),
                child: ColoredBox(color: Color.fromRGBO(0, 0, 0, 0.2 * over)),
              ),
            ),
          Positioned(
            left: 0,
            top: 0,
            bottom: 0,
            width: 320,
            child: Offstage(
              offstage: over == 0,
              child: Transform.translate(offset: Offset(-340 * (1 - over), 0), child: panel),
            ),
          ),
        ],
      ),
    ),
  );

  /// iOS's push: quick out of the gate, a long soft landing (a critically damped spring's shape).
  static const _push = Cubic(0.2, 0.9, 0.25, 1);
}
