// Send a Copy: the note as text through Android's share sheet (MainActivity's channel); on the
// desktop test build, to the clipboard. Web links in notes open in the system's browser.
import 'dart:io';

import 'package:flutter/services.dart';

import 'state.dart';

const _system = MethodChannel('app.bramble.notes/system');

Future<void> shareNote(AppState s, String id, {String format = 'text'}) async {
  final r = s.call('export', {'id': id, 'format': format}) as Map?;
  if (r == null) return;
  final text = r['text'] as String? ?? '';
  final title = s.lib.note(id)?.title ?? '';
  if (Platform.isAndroid) {
    await _system.invokeMethod('shareText', {'title': title, 'text': text});
  } else {
    await Clipboard.setData(ClipboardData(text: text));
    s.say('Copied to the clipboard.');
  }
}

/// Some text through the share sheet (a table's Share Table).
Future<void> shareText(String text, {String title = ''}) async {
  if (Platform.isAndroid) {
    await _system.invokeMethod('shareText', {'title': title, 'text': text});
  } else {
    await Clipboard.setData(ClipboardData(text: text));
  }
}

Future<void> openUrl(String url) async {
  if (Platform.isAndroid) {
    await _system.invokeMethod('url', url);
  } else {
    await Process.run('xdg-open', [url]);
  }
}

/// Take Photo: the system camera; the picture's path (null: cancelled, or no camera here).
Future<String?> takePhoto({bool saveToPhotos = true}) async {
  if (!Platform.isAndroid) return null;
  return await _system.invokeMethod<String>('takePhoto', {'save': saveToPhotos});
}

/// Plays an audio attachment (once; stopAudio stops it).
Future<void> playAudio(String path) async {
  if (Platform.isAndroid) await _system.invokeMethod('play', path);
}

Future<void> stopAudio() async {
  if (Platform.isAndroid) await _system.invokeMethod('stopPlaying');
}

/// Markdown (or plain text) as a note's body: # headings, - and * bullets, 1. numbers,
/// - [ ] checklists, > quotes, **bold** and *italic* (Import to Notes).
List<Map<String, dynamic>> markdownBody(String md) {
  final out = <Map<String, dynamic>>[];
  List<Map<String, dynamic>> spans(String t) {
    final sp = <Map<String, dynamic>>[];
    final re = RegExp(r'(\*\*[^*]+\*\*|\*[^*]+\*|_[^_]+_|~~[^~]+~~)');
    var i = 0;
    for (final m in re.allMatches(t)) {
      if (m.start > i) sp.add({'t': t.substring(i, m.start)});
      final g = m[0]!;
      if (g.startsWith('**')) {
        sp.add({'t': g.substring(2, g.length - 2), 'b': true});
      } else if (g.startsWith('~~')) {
        sp.add({'t': g.substring(2, g.length - 2), 's': true});
      } else {
        sp.add({'t': g.substring(1, g.length - 1), 'i': true});
      }
      i = m.end;
    }
    if (i < t.length) sp.add({'t': t.substring(i)});
    return sp;
  }

  for (final raw in md.split(RegExp(r'\r?\n'))) {
    var l = raw;
    final ind = (RegExp(r'^\s*').firstMatch(l)![0]!.replaceAll('\t', '    ').length ~/ 2).clamp(0, 6);
    l = l.trimLeft();
    Map<String, dynamic> p(String st, String t, {bool ck = false, bool q = false}) => {
      'st': st,
      'sp': spans(t),
      if (ind > 0) 'ind': ind,
      if (ck) 'ck': true,
      if (q) 'q': true,
    };
    RegExpMatch? m;
    if ((m = RegExp(r'^#\s+(.*)').firstMatch(l)) != null) {
      out.add(p(out.isEmpty ? 'title' : 'heading', m![1]!));
    } else if ((m = RegExp(r'^##\s+(.*)').firstMatch(l)) != null) {
      out.add(p('heading', m![1]!));
    } else if ((m = RegExp(r'^#{3,6}\s+(.*)').firstMatch(l)) != null) {
      out.add(p('subheading', m![1]!));
    } else if ((m = RegExp(r'^[-*+]\s+\[( |x|X)\]\s+(.*)').firstMatch(l)) != null) {
      out.add(p('check', m![2]!, ck: m[1]!.toLowerCase() == 'x'));
    } else if ((m = RegExp(r'^[-+]\s+(.*)').firstMatch(l)) != null) {
      out.add(p(raw.trimLeft().startsWith('-') ? 'dash' : 'bullet', m![1]!));
    } else if ((m = RegExp(r'^\*\s+(.*)').firstMatch(l)) != null) {
      out.add(p('bullet', m![1]!));
    } else if ((m = RegExp(r'^\d+[.)]\s+(.*)').firstMatch(l)) != null) {
      out.add(p('number', m![1]!));
    } else if ((m = RegExp(r'^>\s?(.*)').firstMatch(l)) != null) {
      out.add(p('body', m![1]!, q: true));
    } else {
      out.add(p(out.isEmpty && l.isNotEmpty ? 'title' : 'body', l));
    }
  }
  while (out.length > 1 && (out.last['sp'] as List).isEmpty) {
    out.removeLast();
  }
  return out;
}
