// A note's body as the editor holds it: one text (paragraphs joined by '\n', U+FFFC for each
// attachment or inline token) with a style for every character and one for every paragraph,
// as nt-core's model has them (nt-ffi's body JSON in and out). The editor is a single
// TextField over this controller, so selection, the keyboard and the edit menu work across
// paragraphs; list bullets, checkboxes, numbers and indents are drawn by a "lead" U+FFFC at the
// start of the paragraphs that have one (never stored: it comes and goes with the paragraph's
// style, and the caret never rests before it). Backspace at the start of a list item ends the
// list there; Return in an empty item ends it too, as in Notes.
import 'dart:math' as math;

import 'package:flutter/material.dart';

import 'math.dart';

const obj = '￼';
const utiHashtag = 'com.apple.notes.inlinetextattachment.hashtag';
const utiLink = 'com.apple.notes.inlinetextattachment.link';
const utiDivider = 'com.apple.notes.inlinetextattachment.dividerline';
const utiMath = 'com.apple.notes.inlinetextattachment.calculateresult';
const utiTable = 'com.apple.notes.table';
const utiPaper = 'com.apple.paper';

final _rand = math.Random.secure();

/// A new upper-case dashed UUID (Notes' identifiers).
String newId() {
  String hex(int n) => List.generate(n, (_) => _rand.nextInt(16).toRadixString(16)).join().toUpperCase();
  final v = (8 + _rand.nextInt(4)).toRadixString(16).toUpperCase();
  return '${hex(8)}-${hex(4)}-4${hex(3)}-$v${hex(3)}-${hex(12)}';
}

/// A character's style (nt-core's Span without its text).
@immutable
class CStyle {
  const CStyle({this.b = false, this.i = false, this.u = false, this.s = false, this.sc = 0, this.ln, this.hl, this.c, this.f, this.tk, this.lead = false});
  final bool b, i, u, s;
  final int sc;
  final String? ln;
  final int? hl;
  final List<double>? c;
  final Map<String, dynamic>? f;

  /// an attachment or inline token: {id, uti, alt, content}
  final Map<String, dynamic>? tk;

  /// the paragraph's lead (bullet, checkbox, number, indent): not stored
  final bool lead;

  static const plain = CStyle();
  static const leadMark = CStyle(lead: true);

  factory CStyle.fromJson(Map<String, dynamic> j) => CStyle(
    b: j['b'] == true,
    i: j['i'] == true,
    u: j['u'] == true,
    s: j['s'] == true,
    sc: (j['sc'] as num?)?.toInt() ?? 0,
    ln: j['ln'] as String?,
    hl: (j['hl'] as num?)?.toInt(),
    c: (j['c'] as List?)?.map((e) => (e as num).toDouble()).toList(),
    f: (j['f'] as Map?)?.cast(),
    tk: (j['tk'] as Map?)?.cast(),
  );

  Map<String, dynamic> toJson(String text) => {
    't': text,
    if (b) 'b': true,
    if (i) 'i': true,
    if (u) 'u': true,
    if (s) 's': true,
    if (sc != 0) 'sc': sc,
    'ln': ?ln,
    'hl': ?hl,
    'c': ?c,
    'f': ?f,
    'tk': ?tk,
  };

  /// The style a character typed after this one gets (no token, no lead).
  CStyle get typed => tk == null && !lead ? this : CStyle(b: b, i: i, u: u, s: s, sc: sc, hl: hl, c: c, f: f);

  CStyle copyWith({bool? b, bool? i, bool? u, bool? s, int? hl, bool clearHl = false}) => CStyle(
    b: b ?? this.b,
    i: i ?? this.i,
    u: u ?? this.u,
    s: s ?? this.s,
    sc: sc,
    ln: ln,
    hl: clearHl ? null : (hl ?? this.hl),
    c: c,
    f: f,
    tk: tk,
    lead: lead,
  );

  bool get(String k) => switch (k) {
    'b' => b,
    'i' => i,
    'u' => u,
    's' => s,
    _ => false,
  };
  CStyle set(String k, bool on) => switch (k) {
    'b' => copyWith(b: on),
    'i' => copyWith(i: on),
    'u' => copyWith(u: on),
    _ => copyWith(s: on),
  };

  /// Same run (tokens and leads are never merged).
  bool sameRun(CStyle o) =>
      tk == null &&
      o.tk == null &&
      !lead &&
      !o.lead &&
      b == o.b &&
      i == o.i &&
      u == o.u &&
      s == o.s &&
      sc == o.sc &&
      ln == o.ln &&
      hl == o.hl &&
      _listEq(c, o.c) &&
      _mapEq(f, o.f);
}

bool _listEq(List<double>? a, List<double>? b) =>
    a == null ? b == null : (b != null && a.length == b.length && [for (var k = 0; k < a.length; k++) a[k] == b[k]].every((x) => x));
bool _mapEq(Map? a, Map? b) => a == null ? b == null : (b != null && a.length == b.length && a.keys.every((k) => a[k] == b[k]));

/// A paragraph's style (nt-core's Paragraph without its spans).
class PStyle {
  PStyle({this.st = 'body', this.ind = 0, this.al = 0, this.q = false, this.ck = false, this.cid = '', this.sn, this.id = '', this.col = false, this.dir});
  String st;
  int ind, al;
  bool q, ck, col;
  String cid, id;
  int? sn, dir;

  factory PStyle.fromJson(Map<String, dynamic> j) => PStyle(
    st: j['st'] as String? ?? 'body',
    ind: (j['ind'] as num?)?.toInt() ?? 0,
    al: (j['al'] as num?)?.toInt() ?? 0,
    q: j['q'] == true,
    ck: j['ck'] == true,
    cid: j['cid'] as String? ?? '',
    sn: (j['sn'] as num?)?.toInt(),
    id: j['id'] as String? ?? '',
    col: j['col'] == true,
    dir: (j['dir'] as num?)?.toInt(),
  );

  Map<String, dynamic> toJson(List<Map<String, dynamic>> spans) => {
    'st': st,
    'sp': spans,
    if (ind != 0) 'ind': ind,
    if (al != 0) 'al': al,
    if (q) 'q': true,
    if (ck) 'ck': true,
    if (cid.isNotEmpty) 'cid': cid,
    'sn': ?sn,
    if (id.isNotEmpty) 'id': id,
    if (col) 'col': true,
    'dir': ?dir,
  };

  PStyle copy() => PStyle(st: st, ind: ind, al: al, q: q, ck: ck, cid: cid, sn: sn, id: id, col: col, dir: dir);

  /// The paragraph after this one when Return splits it: lists go on (unchecked), titles and
  /// headings are followed by body text.
  PStyle next() => PStyle(st: const {'title', 'heading', 'subheading'}.contains(st) ? 'body' : st, ind: ind, al: al, q: q, dir: dir);

  bool get isList => const {'bullet', 'dash', 'number', 'check'}.contains(st);
  bool get needsLead => isList || ind > 0 || q;
}

class _Snapshot {
  _Snapshot(this.text, this.styles, this.paras, this.sel);
  final String text;
  final List<CStyle> styles;
  final List<PStyle> paras;
  final TextSelection sel;
}

/// The editor's controller: the text with its character and paragraph styles.
class NoteController extends TextEditingController {
  NoteController();

  List<CStyle> _styles = [];
  List<PStyle> _paras = [PStyle(st: 'title')];

  /// What's typed next when nothing is selected (Bold pressed with no selection), until the caret moves.
  CStyle? typing;

  /// Builds the widget for a token (an attachment, a tag, a link…), given its {id, uti, alt, content}.
  Widget Function(BuildContext context, Map<String, dynamic> tk, PStyle para, TextStyle style)? tokenWidget;

  /// The paragraphs' fonts (the note's text size setting).
  TextStyle Function(PStyle p)? paraStyle;

  /// The page's colours for the leads (checkbox, bullets) and highlights.
  Color leadColor = Colors.grey, checkColor = Colors.amber, textColor = Colors.black;
  Color Function(int hl)? highlightText, highlightWash;

  /// The text and the styles changed (to save, after a pause).
  VoidCallback? onEdit;

  /// Settings > Auto Convert to Tag.
  bool autoTag = true;

  /// Math Results > Insert Results: "2+3=" gets its answer.
  bool mathResults = true;

  /// Settings > Sort Checked Items > Automatically: a ticked item moves to its checklist's end.
  bool autoSortChecked = false;

  /// The style a new note's first line gets.
  String firstStyle = 'title';

  final _undo = <_Snapshot>[], _redo = <_Snapshot>[];
  DateTime _lastSnap = DateTime(0);
  String _lastKind = '';
  bool _internal = false;

  List<PStyle> get paras => _paras;
  List<CStyle> get styles => _styles;

  // ------------------------------------------------------------ loading and saving

  void load(List body) {
    final b = StringBuffer();
    final st = <CStyle>[];
    final ps = <PStyle>[];
    for (final (k, raw) in body.indexed) {
      final pj = (raw as Map).cast<String, dynamic>();
      final p = PStyle.fromJson(pj);
      if (k > 0) {
        b.write('\n');
        st.add(CStyle.plain);
      }
      ps.add(p);
      if (p.needsLead) {
        b.write(obj);
        st.add(CStyle.leadMark);
      }
      for (final sp in (pj['sp'] as List? ?? const [])) {
        final sj = (sp as Map).cast<String, dynamic>();
        final cs = CStyle.fromJson(sj);
        if (cs.tk != null) {
          b.write(obj);
          st.add(cs);
        } else {
          final t = (sj['t'] as String? ?? '').replaceAll(obj, '').replaceAll('\n', ' ');
          b.write(t);
          st.addAll(List.filled(t.length, cs));
        }
      }
    }
    if (ps.isEmpty) ps.add(PStyle(st: firstStyle));
    _styles = st;
    _paras = ps;
    _undo.clear();
    _redo.clear();
    _internal = true;
    super.value = TextEditingValue(text: b.toString(), selection: const TextSelection.collapsed(offset: 0));
    _internal = false;
  }

  /// The body as nt-ffi takes it.
  List<Map<String, dynamic>> body() {
    final out = <Map<String, dynamic>>[];
    final t = text;
    var a = 0;
    for (final p in _paras) {
      var e = t.indexOf('\n', a);
      if (e < 0) e = t.length;
      final spans = <Map<String, dynamic>>[];
      var k = a;
      while (k < e) {
        final s = _styles[k];
        if (s.lead) {
          k++;
          continue;
        }
        if (s.tk != null) {
          spans.add(s.toJson(obj));
          k++;
          continue;
        }
        var j = k + 1;
        while (j < e && _styles[j].sameRun(s)) {
          j++;
        }
        spans.add(s.toJson(t.substring(k, j)));
        k = j;
      }
      out.add(p.toJson(spans));
      a = e + 1;
    }
    return out;
  }

  /// The first line's text (the note's title as it's typed).
  String get titleText {
    final t = text;
    final e = t.indexOf('\n');
    return (e < 0 ? t : t.substring(0, e)).replaceAll(obj, '').trim();
  }

  // ------------------------------------------------------------ geometry of the text

  /// The paragraph holding offset `o`.
  int paraAt(int o) {
    final t = text;
    var k = 0;
    for (var i = 0; i < o && i < t.length; i++) {
      if (t.codeUnitAt(i) == 10) k++;
    }
    return k;
  }

  /// Paragraph `k`'s range [start, end) in the text (end at its '\n').
  (int, int) paraRange(int k) {
    final t = text;
    var a = 0;
    for (var i = 0; i < k; i++) {
      final n = t.indexOf('\n', a);
      if (n < 0) return (t.length, t.length);
      a = n + 1;
    }
    var e = t.indexOf('\n', a);
    if (e < 0) e = t.length;
    return (a, e);
  }

  /// Where paragraph `k`'s own text starts (after its lead).
  int contentStart(int k) {
    final (a, e) = paraRange(k);
    return a < e && _styles[a].lead ? a + 1 : a;
  }

  /// The paragraphs the selection touches.
  (int, int) selectedParas() {
    final s = selection.isValid ? selection : const TextSelection.collapsed(offset: 0);
    return (paraAt(s.start), paraAt(s.end));
  }

  PStyle get currentPara => _paras[math.min(_paras.length - 1, paraAt(selection.isValid ? selection.start : 0))];

  /// The style at the caret (what the Aa sheet shows lit).
  CStyle get currentStyle {
    if (typing != null) return typing!;
    final s = selection.isValid ? selection : const TextSelection.collapsed(offset: 0);
    if (!s.isCollapsed) {
      final t = text;
      final chars = [
        for (var k = s.start; k < s.end; k++)
          if (!_styles[k].lead && _styles[k].tk == null && t.codeUnitAt(k) != 10) _styles[k],
      ];
      if (chars.isEmpty) return CStyle.plain;
      return CStyle(
        b: chars.every((c) => c.b),
        i: chars.every((c) => c.i),
        u: chars.every((c) => c.u),
        s: chars.every((c) => c.s),
        hl: chars.every((c) => c.hl == chars.first.hl) ? chars.first.hl : null,
      );
    }
    return _styleBefore(s.start);
  }

  CStyle _styleBefore(int o) {
    final t = text;
    for (var k = o - 1; k >= 0; k--) {
      if (t.codeUnitAt(k) == 10) break;
      final s = _styles[k];
      if (s.lead) break;
      if (s.tk != null && s.tk!['uti'] != utiHashtag) continue;
      return s.typed;
    }
    if (o < t.length && t.codeUnitAt(o) != 10 && !_styles[o].lead) return _styles[o].typed;
    return CStyle.plain;
  }

  // ------------------------------------------------------------ editing

  @override
  set value(TextEditingValue v) {
    if (_internal) {
      super.value = v;
      return;
    }
    final old = text;
    if (v.text == old) {
      if (v.selection != selection) typing = null;
      super.value = v.copyWith(selection: _fixSelection(v.selection, selection));
      return;
    }
    _applyEdit(old, v);
  }

  void _snapshot(String kind) {
    final now = DateTime.now();
    if (kind != 'type' || _lastKind != 'type' || now.difference(_lastSnap).inMilliseconds > 1200) {
      _undo.add(_Snapshot(text, [..._styles], [for (final p in _paras) p.copy()], selection));
      if (_undo.length > 200) _undo.removeAt(0);
      _redo.clear();
    }
    _lastSnap = now;
    _lastKind = kind;
  }

  bool get canUndo => _undo.isNotEmpty;
  bool get canRedo => _redo.isNotEmpty;

  void undo() => _restore(_undo, _redo);
  void redo() => _restore(_redo, _undo);

  void _restore(List<_Snapshot> from, List<_Snapshot> to) {
    if (from.isEmpty) return;
    to.add(_Snapshot(text, [..._styles], [for (final p in _paras) p.copy()], selection));
    final s = from.removeLast();
    _styles = [...s.styles];
    _paras = [for (final p in s.paras) p.copy()];
    _lastKind = '';
    _set(TextEditingValue(text: s.text, selection: s.sel));
  }

  void _set(TextEditingValue v) {
    _internal = true;
    super.value = v;
    _internal = false;
    onEdit?.call();
  }

  void _applyEdit(String old, TextEditingValue v) {
    var n = v.text;
    var sel = v.selection;
    // the change: what's common at both ends stays
    var p = 0;
    final maxP = math.min(old.length, n.length);
    while (p < maxP && old.codeUnitAt(p) == n.codeUnitAt(p)) {
      p++;
    }
    if (sel.isValid && sel.isCollapsed) p = math.min(p, math.max(0, sel.baseOffset - math.max(0, n.length - old.length)));
    var s = 0;
    while (s < old.length - p && s < n.length - p && old.codeUnitAt(old.length - 1 - s) == n.codeUnitAt(n.length - 1 - s)) {
      s++;
    }
    if (sel.isValid && sel.isCollapsed) s = math.min(s, n.length - sel.baseOffset);
    final removed = old.substring(p, old.length - s);
    var inserted = n.substring(p, n.length - s);
    // (no object characters come in from the keyboard or a paste)
    if (inserted.contains(obj)) {
      final clean = inserted.replaceAll(obj, '');
      final d = inserted.length - clean.length;
      inserted = clean;
      n = n.substring(0, p) + clean + n.substring(n.length - s);
      sel = TextSelection.collapsed(offset: math.max(p, (sel.isValid ? sel.extentOffset : p) - d));
    }
    if (removed.isEmpty && inserted.isEmpty) return;
    _snapshot(inserted == '\n' || removed.contains('\n') ? 'line' : (removed.isEmpty && inserted.length == 1 ? 'type' : 'edit'));

    final k0 = paraAt(p);
    // Backspace over a list item's lead: the item ends (body text, no indent)
    if (inserted.isEmpty && removed.length == 1 && _styles[p].lead) {
      final para = _paras[k0];
      if (para.isList) {
        para.st = 'body';
        para.ck = false;
      } else if (para.ind > 0) {
        para.ind--;
      } else {
        para.q = false;
      }
      _set(
        _normalize(
          TextEditingValue(
            text: old,
            selection: TextSelection.collapsed(offset: p + 1),
          ),
        ),
      );
      return;
    }
    // Return in an empty list item: the list ends
    if (removed.isEmpty && inserted == '\n') {
      final para = _paras[k0];
      final (a, e) = paraRange(k0);
      final empty = e - a == (para.needsLead ? 1 : 0);
      if (para.isList && empty) {
        para.st = 'body';
        para.ck = false;
        para.ind = 0;
        _set(
          _normalize(
            TextEditingValue(
              text: old,
              selection: TextSelection.collapsed(offset: e),
            ),
          ),
        );
        return;
      }
    }

    // the paragraphs: those the removed text joined become one (the first's style), then the
    // inserted text's lines split it
    final r = '\n'.allMatches(removed).length;
    final base = _paras[k0];
    final ins = '\n'.allMatches(inserted).length;
    final splitAtStart = removed.isEmpty && inserted == '\n' && p == contentStart(k0) && p < paraRange(k0).$2;
    _paras.removeRange(k0 + 1, k0 + 1 + r);
    final added = <PStyle>[];
    if (splitAtStart) {
      // Return at an item's start: the item moves down, an empty one of its kind stays above
      added.add(base.copy());
      base
        ..id = ''
        ..ck = false
        ..cid = '';
    } else {
      for (var i = 0; i < ins; i++) {
        added.add(i == 0 ? base.next() : added.last.copy());
      }
    }
    _paras.insertAll(k0 + 1, added);

    // the characters' styles
    final st = typing ?? _styleBefore(p);
    _styles.replaceRange(p, old.length - s, List.filled(inserted.length, st.typed));
    var next = TextEditingValue(
      text: n,
      selection: sel.isValid ? sel : TextSelection.collapsed(offset: p + inserted.length),
      composing: v.composing,
    );
    if (autoTag && (inserted == ' ' || inserted == '\n')) next = _makeTag(next, p);
    if (mathResults && inserted.endsWith('=') && !inserted.contains('\n')) next = _makeResult(next, p + inserted.length - 1);
    _set(_normalize(next));
  }

  /// An expression ending in the "=" just typed: the "=" becomes the answer (a math token
  /// showing "=14"), with the variables the lines above set.
  TextEditingValue _makeResult(TextEditingValue v, int at) {
    final t = v.text;
    final k = paraAt(at);
    final (a, _) = paraRange(k);
    final expr = t.substring(a, at).replaceAll(obj, '');
    if (expr.trim().isEmpty || RegExp(r'^\s*[A-Za-z_][A-Za-z_0-9]*\s*$').hasMatch(expr) && !expr.contains(RegExp(r'\d'))) {
      // ("x =" starts setting a variable)
      final vars = variables([for (var i = 0; i < k; i++) _plainLine(t, i)]);
      if (!vars.containsKey(expr.trim())) return v;
    }
    final r = evaluate(expr, variables([for (var i = 0; i < k; i++) _plainLine(t, i)]));
    if (r == null) return v;
    final tk = {'id': newId(), 'uti': utiMath, 'alt': '=${formatResult(r)}', 'content': ''};
    _styles[at] = CStyle(tk: tk, b: _styles[at].b, i: _styles[at].i);
    return TextEditingValue(text: t.substring(0, at) + obj + t.substring(at + 1), selection: v.selection);
  }

  String _plainLine(String t, int k) {
    var a = 0;
    for (var i = 0; i < k; i++) {
      final n = t.indexOf('\n', a);
      if (n < 0) return '';
      a = n + 1;
    }
    var e = t.indexOf('\n', a);
    if (e < 0) e = t.length;
    return t.substring(a, e).replaceAll(obj, '');
  }

  /// "#word" followed by a space or Return becomes a tag (Settings > Tags: Notes makes them as you type).
  TextEditingValue _makeTag(TextEditingValue v, int at) {
    final t = v.text;
    var k = at;
    while (k > 0) {
      final c = t[k - 1];
      if (c == '#') break;
      if (!RegExp(r'[\p{L}\p{N}_\-]', unicode: true).hasMatch(c)) return v;
      k--;
    }
    if (k == 0 || t[k - 1] != '#' || k == at) return v;
    final hash = k - 1;
    if (hash > 0 && !RegExp(r'\s').hasMatch(t[hash - 1]) && !_styles[hash - 1].lead) return v;
    final word = t.substring(k, at);
    if (RegExp(r'^\d+$').hasMatch(word)) return v;
    final tk = {'id': newId(), 'uti': utiHashtag, 'alt': '#$word', 'content': ''};
    final before = _styles[hash];
    _styles.replaceRange(hash, at, [CStyle(tk: tk, b: before.b, i: before.i)]);
    final shift = at - hash - 1;
    final o = v.selection.extentOffset - shift;
    return TextEditingValue(
      text: t.substring(0, hash) + obj + t.substring(at),
      selection: TextSelection.collapsed(offset: o),
    );
  }

  /// Puts a lead at the start of each paragraph that needs one (and only there), keeping the
  /// selection where it was in the text.
  TextEditingValue _normalize(TextEditingValue v) {
    var t = v.text;
    var base = v.selection.isValid ? v.selection.baseOffset : 0, ext = v.selection.isValid ? v.selection.extentOffset : 0;
    final sb = StringBuffer();
    final ns = <CStyle>[];
    var k = 0, pos = 0;
    var atStart = true;
    // walk the text, dropping misplaced leads and adding missing ones
    final shifts = <(int, int)>[]; // (old offset, +1 inserted / -1 removed)
    while (pos <= t.length) {
      if (atStart) {
        final want = k < _paras.length && _paras[k].needsLead;
        final has = pos < t.length && _styles[pos].lead;
        if (want) {
          sb.write(obj);
          ns.add(CStyle.leadMark);
          if (has) {
            pos++;
          } else {
            shifts.add((pos, 1));
          }
        } else if (has) {
          shifts.add((pos, -1));
          pos++;
        }
        atStart = false;
        continue;
      }
      if (pos == t.length) break;
      final c = t.codeUnitAt(pos);
      if (_styles[pos].lead) {
        shifts.add((pos, -1));
        pos++;
        continue;
      }
      sb.writeCharCode(c);
      ns.add(_styles[pos]);
      pos++;
      if (c == 10) {
        k++;
        atStart = true;
      }
    }
    while (_paras.length > k + 1) {
      _paras.removeLast();
    }
    while (_paras.length < k + 1) {
      _paras.add(PStyle());
    }
    int map(int o) {
      var r = o;
      for (final (at, d) in shifts) {
        if (d > 0 && at <= o) r += 1;
        if (d < 0 && at < o) r -= 1;
      }
      return r;
    }

    base = map(base);
    ext = map(ext);
    t = sb.toString();
    _styles = ns;
    final sel = _fixSelection(
      TextSelection(baseOffset: base.clamp(0, t.length), extentOffset: ext.clamp(0, t.length)),
      null,
      text: t,
    );
    return TextEditingValue(text: t, selection: sel);
  }

  /// The caret never rests before a lead: it goes after it (or, moving left from there, to
  /// the end of the line above).
  TextSelection _fixSelection(TextSelection s, TextSelection? was, {String? text}) {
    if (!s.isValid) return s;
    final len = (text ?? this.text).length;
    // a caret under a collapsed heading goes to the heading's end
    if (s.isCollapsed && text == null && _paras.any((p) => p.col)) {
      final hidden = hiddenParas();
      var k = paraAt(s.baseOffset);
      if (hidden.contains(k)) {
        while (k > 0 && hidden.contains(k)) {
          k--;
        }
        return TextSelection.collapsed(offset: paraRange(k).$2);
      }
    }
    int fix(int o, int? from) {
      if (o < len && o < _styles.length && _styles[o].lead) {
        if (from != null && from == o + 1 && o > 0) return o - 1;
        return o + 1;
      }
      return o;
    }

    if (s.isCollapsed) return TextSelection.collapsed(offset: fix(s.baseOffset, was?.isCollapsed == true ? was!.baseOffset : null), affinity: s.affinity);
    return TextSelection(baseOffset: fix(s.baseOffset, null), extentOffset: fix(s.extentOffset, null));
  }

  // ------------------------------------------------------------ commands (the Aa sheet, the toolbar)

  /// Redraw after the styles were changed directly.
  void refresh() => notifyListeners();

  void _changed() {
    _set(_normalize(value));
    notifyListeners();
  }

  /// Bold, Italic, Underline, Strikethrough on the selection (or for what's typed next).
  void toggle(String key) {
    final s = selection;
    if (!s.isValid) return;
    if (s.isCollapsed) {
      final cur = currentStyle;
      typing = cur.set(key, !cur.get(key));
      notifyListeners();
      return;
    }
    _snapshot('format');
    final on = !currentStyle.get(key);
    for (var k = s.start; k < s.end; k++) {
      if (!_styles[k].lead) _styles[k] = _styles[k].set(key, on);
    }
    _changed();
  }

  /// A highlight colour (1…5) on the selection, or none.
  void highlight(int? hl) {
    final s = selection;
    if (!s.isValid) return;
    if (s.isCollapsed) {
      typing = currentStyle.copyWith(hl: hl, clearHl: hl == null);
      notifyListeners();
      return;
    }
    _snapshot('format');
    for (var k = s.start; k < s.end; k++) {
      if (!_styles[k].lead) _styles[k] = _styles[k].copyWith(hl: hl, clearHl: hl == null);
    }
    _changed();
  }

  /// A paragraph style for the paragraphs the selection touches (a list style again: body text).
  void paragraphStyle(String st) {
    _snapshot('format');
    final (a, b) = selectedParas();
    final all = [for (var k = a; k <= b; k++) _paras[k].st == st].every((x) => x);
    for (var k = a; k <= b; k++) {
      final p = _paras[k];
      if (all && p.isList) {
        p.st = 'body';
      } else {
        p.st = st;
      }
      if (p.st != 'check') p.ck = false;
    }
    _changed();
  }

  void indent(int d) {
    _snapshot('format');
    final (a, b) = selectedParas();
    for (var k = a; k <= b; k++) {
      _paras[k].ind = (_paras[k].ind + d).clamp(0, 8);
    }
    _changed();
  }

  void blockQuote() {
    _snapshot('format');
    final (a, b) = selectedParas();
    final on = !_paras[a].q;
    for (var k = a; k <= b; k++) {
      _paras[k].q = on;
    }
    _changed();
  }

  static int? _level(String st) => switch (st) {
    'title' => 0,
    'heading' => 1,
    'subheading' => 2,
    _ => null,
  };

  /// The paragraphs a collapsed heading hides (until a heading of its level or above).
  Set<int> hiddenParas() {
    final out = <int>{};
    int? hideBelow;
    for (var k = 0; k < _paras.length; k++) {
      final l = _level(_paras[k].st);
      if (hideBelow != null) {
        if (l != null && l <= hideBelow) {
          hideBelow = null;
        } else {
          out.add(k);
          continue;
        }
      }
      if (l != null && _paras[k].col) hideBelow = l;
    }
    return out;
  }

  /// Headings that can collapse (something under them), by paragraph.
  List<int> collapsibleHeadings() {
    final hidden = hiddenParas();
    final out = <int>[];
    for (var k = 0; k < _paras.length - 1; k++) {
      final l = _level(_paras[k].st);
      if (l == null || hidden.contains(k)) continue;
      final next = _level(_paras[k + 1].st);
      if (next == null || next > l) out.add(k);
    }
    return out;
  }

  /// Collapses or expands heading `k`; the caret leaves what's hidden.
  void toggleCollapse(int k) {
    if (k >= _paras.length || _level(_paras[k].st) == null) return;
    _snapshot('format');
    _paras[k].col = !_paras[k].col;
    final hidden = hiddenParas();
    if (selection.isValid && hidden.contains(paraAt(selection.baseOffset))) selection = TextSelection.collapsed(offset: paraRange(k).$2);
    _changed();
  }

  /// Ticks or unticks checklist item `k`.
  void check(int k) {
    if (k >= _paras.length || _paras[k].st != 'check') return;
    _snapshot('check');
    _paras[k].ck = !_paras[k].ck;
    if (autoSortChecked && _paras[k].ck) {
      var j = k;
      while (j + 1 < _paras.length && _paras[j + 1].st == 'check') {
        j++;
      }
      if (j > k) _moveAfter(k, j);
    }
    _changed();
  }

  /// Moves paragraph `k` to just after paragraph `to` (further down).
  void _moveAfter(int k, int to) {
    final t = text;
    final (a, e) = paraRange(k);
    final (_, e2) = paraRange(to);
    final seg = t.substring(a, e), segS = _styles.sublist(a, e);
    final mid = t.substring(e + 1, e2), midS = _styles.sublist(e + 1, e2);
    final p = _paras.removeAt(k);
    _paras.insert(to, p);
    _styles = [..._styles.sublist(0, a), ...midS, CStyle.plain, ...segS, ..._styles.sublist(e2)];
    final sel = selection.isValid ? selection.baseOffset.clamp(0, t.length) : 0;
    _internal = true;
    super.value = TextEditingValue(
      text: t.substring(0, a) + mid + '\n' + seg + t.substring(e2),
      selection: TextSelection.collapsed(offset: sel),
    );
    _internal = false;
  }

  /// The checklist button: the paragraphs become a checklist (or stop being one).
  void checklist() => paragraphStyle('check');

  /// Inserts a token at the caret: a block attachment gets a line of its own.
  void insertToken(Map<String, dynamic> tk, {bool block = true}) {
    _snapshot('edit');
    final s = selection.isValid ? selection : TextSelection.collapsed(offset: text.length);
    var t = text;
    var o = s.start;
    _styles.removeRange(s.start, s.end);
    t = t.substring(0, s.start) + t.substring(s.end);
    final k = paraAt(o);
    final pieces = StringBuffer();
    final st = <CStyle>[];
    var paraBreaks = <PStyle>[];
    if (block) {
      final (a, e) = paraRange(k);
      final atStart = o <= (a < e && _styles[a].lead ? a + 1 : a);
      if (!atStart) {
        pieces.write('\n');
        st.add(CStyle.plain);
        paraBreaks.add(PStyle(st: 'body'));
      }
      pieces.write(obj);
      st.add(CStyle(tk: tk));
      final atEnd = o >= e;
      pieces.write('\n');
      st.add(CStyle.plain);
      paraBreaks.add(atEnd ? PStyle(st: 'body') : _paras[k].copy());
      if (atStart && _paras[k].needsLead) {
        // the attachment's own line has no bullet
        paraBreaks = [_paras[k].copy(), ...paraBreaks];
        _paras[k] = PStyle(st: 'body');
        paraBreaks.removeAt(0);
      }
    } else {
      pieces.write(obj);
      st.add(CStyle(tk: tk));
    }
    final ins = pieces.toString();
    _styles.insertAll(o, st);
    _paras.insertAll(k + 1, paraBreaks);
    t = t.substring(0, o) + ins + t.substring(o);
    o += ins.length;
    _set(
      _normalize(
        TextEditingValue(
          text: t,
          selection: TextSelection.collapsed(offset: o),
        ),
      ),
    );
    notifyListeners();
  }

  /// What's being typed for a suggestion: `>>` and a note title's start (a note link), or `#`
  /// and a tag's start; (kind, query, where it starts).
  (String, String, int)? get suggesting {
    final s = selection;
    if (!s.isValid || !s.isCollapsed) return null;
    final t = text;
    final start = paraRange(paraAt(s.baseOffset)).$1;
    final before = t.substring(start, s.baseOffset);
    final link = RegExp(r'>>([^>\uFFFC]{0,40})$').firstMatch(before);
    if (link != null) return ('link', link[1]!, start + link.start);
    final tag = RegExp(r'(?:^|\s)#([\p{L}\p{N}_\-]{0,30})$', unicode: true).firstMatch(before);
    if (tag != null) return ('tag', tag[1]!, s.baseOffset - tag[1]!.length - 1);
    return null;
  }

  /// Puts a token in place of text [a, b) (a suggestion picked), then `after`.
  void replaceWithToken(int a, int b, Map<String, dynamic> tk, {String after = ' '}) {
    _snapshot('edit');
    final t = text;
    final st = _styleBefore(a);
    _styles.replaceRange(a, b, [CStyle(tk: tk, b: st.b, i: st.i), ...List.filled(after.length, st.typed)]);
    _set(
      _normalize(
        TextEditingValue(
          text: t.substring(0, a) + obj + after + t.substring(b),
          selection: TextSelection.collapsed(offset: a + 1 + after.length),
        ),
      ),
    );
    notifyListeners();
  }

  /// Inserts plain text at the caret (the keyboard's own typing comes through `value`).
  void insertText(String s) {
    final sel = selection.isValid ? selection : TextSelection.collapsed(offset: text.length);
    value = TextEditingValue(
      text: text.replaceRange(sel.start, sel.end, s),
      selection: TextSelection.collapsed(offset: sel.start + s.length),
    );
  }

  /// Every token in the text (attachments in order).
  Iterable<Map<String, dynamic>> get tokens => _styles.where((s) => s.tk != null).map((s) => s.tk!);

  /// Removes the token with this id (an attachment deleted from its menu).
  void removeToken(String id) {
    final k = _styles.indexWhere((s) => s.tk?['id'] == id);
    if (k < 0) return;
    _snapshot('edit');
    final t = text;
    var a = k, b = k + 1;
    // its line goes with it when it was alone on it
    if (b < t.length && t.codeUnitAt(b) == 10 && (a == 0 || t.codeUnitAt(a - 1) == 10)) b++;
    final r = '\n'.allMatches(t.substring(a, b)).length;
    final pk = paraAt(a);
    _paras.removeRange(pk + 1, math.min(_paras.length, pk + 1 + r));
    _styles.removeRange(a, b);
    _set(
      _normalize(
        TextEditingValue(
          text: t.substring(0, a) + t.substring(b),
          selection: TextSelection.collapsed(offset: a),
        ),
      ),
    );
    notifyListeners();
  }

  // ------------------------------------------------------------ drawing the text

  @override
  TextSpan buildTextSpan({required BuildContext context, TextStyle? style, required bool withComposing}) {
    final t = text;
    if (_styles.length != t.length) _styles = List.filled(t.length, CStyle.plain);
    final kids = <InlineSpan>[];
    final base = style ?? const TextStyle();
    var a = 0;
    // numbered lists count per indent level, restarting where the list breaks
    final counters = <int, int>{};
    final hidden = hiddenParas();
    const gone = TextStyle(fontSize: 0.01, height: 0.01, color: Color(0x00000000), letterSpacing: 0);
    for (var k = 0; k < _paras.length && a <= t.length; k++) {
      final p = _paras[k];
      var e = t.indexOf('\n', a);
      if (e < 0) e = t.length;
      // under a collapsed heading: there, but takes no room
      if (hidden.contains(k)) {
        var i = a;
        while (i < e) {
          final s = _styles[i];
          if (s.lead || s.tk != null) {
            kids.add(const WidgetSpan(child: SizedBox.shrink()));
            i++;
            continue;
          }
          var j = i + 1;
          while (j < e && !_styles[j].lead && _styles[j].tk == null) {
            j++;
          }
          kids.add(TextSpan(text: t.substring(i, j), style: gone));
          i = j;
        }
        if (e < t.length) kids.add(const TextSpan(text: '\n', style: gone));
        a = e + 1;
        continue;
      }
      final ps = (paraStyle?.call(p) ?? base).merge(TextStyle(color: textColor));
      if (p.st == 'number') {
        counters.removeWhere((lvl, _) => lvl > p.ind);
        counters[p.ind] = (counters[p.ind] ?? ((p.sn ?? 1) - 1)) + 1;
      } else {
        counters.clear();
      }
      var i = a;
      while (i < e) {
        final s = _styles[i];
        if (s.lead) {
          kids.add(
            WidgetSpan(
              alignment: PlaceholderAlignment.middle,
              child: _Lead(p: p, number: counters[p.ind] ?? 1, size: ps.fontSize ?? 17, color: leadColor, check: checkColor, onCheck: () => check(k)),
            ),
          );
          i++;
          continue;
        }
        if (s.tk != null) {
          final w = tokenWidget?.call(context, s.tk!, p, ps);
          final inline = (s.tk!['uti'] as String? ?? '').startsWith('com.apple.notes.inlinetextattachment');
          kids.add(
            WidgetSpan(
              alignment: inline ? PlaceholderAlignment.baseline : PlaceholderAlignment.bottom,
              baseline: TextBaseline.alphabetic,
              style: ps,
              child: w ?? const SizedBox(width: 1, height: 1),
            ),
          );
          i++;
          continue;
        }
        var j = i + 1;
        while (j < e && _styles[j].sameRun(s)) {
          j++;
        }
        kids.add(TextSpan(text: t.substring(i, j), style: _charStyle(ps, s, p)));
        i = j;
      }
      if (e < t.length) kids.add(TextSpan(text: '\n', style: ps));
      a = e + 1;
    }
    return TextSpan(style: base, children: kids);
  }

  TextStyle _charStyle(TextStyle ps, CStyle s, PStyle p) {
    final deco = <TextDecoration>[if (s.u || s.ln != null) TextDecoration.underline, if (s.s) TextDecoration.lineThrough];
    var st = ps.copyWith(
      fontWeight: s.b ? FontWeight.w700 : null,
      fontStyle: s.i ? FontStyle.italic : null,
      decoration: deco.isEmpty ? null : TextDecoration.combine(deco),
      decorationColor: s.ln != null ? checkColor : null,
      color: s.c != null && s.c!.length == 4 ? Color.from(alpha: s.c![3], red: s.c![0], green: s.c![1], blue: s.c![2]) : (s.ln != null ? checkColor : null),
      fontFeatures: s.sc != 0 ? [s.sc > 0 ? const FontFeature.superscripts() : const FontFeature.subscripts()] : null,
    );
    if (s.hl != null) st = st.copyWith(color: highlightText?.call(s.hl!), backgroundColor: highlightWash?.call(s.hl!));
    if (p.st == 'check' && p.ck) st = st.copyWith(color: leadColor);
    return st;
  }
}

/// A paragraph's lead: its indent, then the bullet, dash, number or checkbox, or a quote's bar.
class _Lead extends StatelessWidget {
  const _Lead({required this.p, required this.number, required this.size, required this.color, required this.check, required this.onCheck});
  final PStyle p;
  final int number;
  final double size;
  final Color color, check;
  final VoidCallback onCheck;

  @override
  Widget build(BuildContext context) {
    final indent = p.ind * 28.0;
    final h = size * 1.25;
    Widget mark;
    switch (p.st) {
      case 'check':
        final d = size * 1.3;
        mark = GestureDetector(
          behavior: HitTestBehavior.opaque,
          onTap: onCheck,
          child: SizedBox(
            // (wide enough for the circle at any text size: a big style's circle stays round)
            width: math.max(34, d + 6),
            height: math.max(size * 1.85, d + 4),
            child: Center(
              child: Container(
                width: d,
                height: d,
                decoration: BoxDecoration(
                  shape: BoxShape.circle,
                  color: p.ck ? check : null,
                  border: p.ck ? null : Border.all(color: color.withValues(alpha: 0.6), width: 1.4),
                ),
                child: p.ck ? Icon(Icons.check_rounded, size: d * 0.72, color: Colors.white) : null,
              ),
            ),
          ),
        );
      case 'bullet':
        mark = SizedBox(
          width: 26,
          height: h,
          child: Center(
            child: Container(
              width: size * 0.36,
              height: size * 0.36,
              decoration: BoxDecoration(shape: BoxShape.circle, color: check),
            ),
          ),
        );
      case 'dash':
        mark = SizedBox(
          width: 26,
          height: h,
          child: Center(
            child: Container(width: size * 0.5, height: 1.6, color: check),
          ),
        );
      case 'number':
        mark = SizedBox(
          width: 30,
          height: h,
          child: Align(
            alignment: Alignment.centerRight,
            child: Padding(
              padding: const EdgeInsets.only(right: 6),
              child: Text(
                '$number.',
                style: TextStyle(fontFamily: 'Sfera', fontSize: size, color: check, height: 1.0),
              ),
            ),
          ),
        );
      default:
        mark = const SizedBox.shrink();
    }
    return Row(
      mainAxisSize: MainAxisSize.min,
      children: [
        if (p.q)
          Container(
            width: 3,
            height: h + 4,
            margin: const EdgeInsets.only(right: 12),
            decoration: BoxDecoration(color: color.withValues(alpha: 0.5), borderRadius: BorderRadius.circular(1.5)),
          ),
        SizedBox(width: indent),
        mark,
        if (p.isList) const SizedBox(width: 2),
      ],
    );
  }
}
