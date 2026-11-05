// Tags' actions (the tag browser and the Folders page's tag chips): Rename Tag and Delete Tag
// from a long press (Delete leaves the tag's text in the notes, no longer a tag), and the
// browser's Create Smart Folder from the tags chosen.
import 'package:flutter/cupertino.dart';
import 'package:flutter/material.dart';

import 'model.dart';
import 'state.dart';
import 'theme.dart';
import 'ui/dialogs.dart';
import 'ui/touch_menu.dart';

void tagMenu(BuildContext context, AppState s, String tag, Rect anchor) {
  final p = Palette.of(context);
  showTouchMenu(
    context,
    p.look,
    anchor,
    [
      {'label': 'Rename Tag', 'icon': CupertinoIcons.pencil, 'id': 'rename'},
      {'separator': true},
      {'label': 'Delete Tag', 'icon': CupertinoIcons.trash, 'id': 'delete', 'destructive': true},
    ],
    (i) async {
      if (i['id'] == 'rename') {
        final to = await askText(context, 'Rename Tag', initial: tag, hint: 'Tag', ok: 'Rename');
        final name = to?.trim().replaceAll(RegExp(r'^#+'), '').replaceAll(RegExp(r'\s+'), '');
        if (name == null || name.isEmpty || name == tag) return;
        final r = s.call('rename_tag', {'from': tag, 'to': name}) as Map?;
        await s.reload();
        if (s.place is WithTag && (s.place as WithTag).chosen.contains(tag.toLowerCase())) s.choose(WithTag(name));
        if (r != null) s.say('Renamed in ${r['notes']} ${r['notes'] == 1 ? 'note' : 'notes'}.');
      } else {
        final n = s.lib.notes.where((x) => !x.deleted && x.tags.any((t) => t.toLowerCase() == tag.toLowerCase())).length;
        if (!await confirm(
          context,
          'Delete Tag?',
          message: 'The tag will be removed from $n ${n == 1 ? 'note' : 'notes'}. Their text stays.',
          ok: 'Delete Tag',
        ))
          return;
        s.call('delete_tag', {'name': tag});
        await s.reload();
        if (s.place is WithTag && (s.place as WithTag).chosen.contains(tag.toLowerCase())) s.choose(const WithTag(''));
      }
    },
  );
}

/// The tag browser's … > Create Smart Folder: the chosen tags as its filters.
Future<void> smartFolderFromTags(BuildContext context, AppState s, WithTag t) async {
  final name = await askText(context, 'New Smart Folder', initial: t.chosen.map((c) => '#$c').join(' '), hint: 'Name', ok: 'Create');
  if (name == null || name.trim().isEmpty) return;
  s.call('create_smart', {
    'name': name.trim(),
    'any': !t.all,
    'rules': [
      for (final c in t.chosen) [0, c],
    ],
  });
  await s.reload();
  s.say('Smart Folder made.');
}
