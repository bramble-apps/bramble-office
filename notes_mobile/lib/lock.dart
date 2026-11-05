// Locked notes: the notes password (set it, change it, with a hint), locking a note with it,
// and asking for it to view one. Once given, locked notes stay open until Lock Now.
import 'package:flutter/cupertino.dart';
import 'package:flutter/material.dart';

import 'state.dart';
import 'theme.dart';
import 'ui/dialogs.dart';
import 'ui/glass.dart';
import 'ui/sheet.dart';

/// Set Password / Change Password; answers the new password.
Future<String?> passwordSheet(BuildContext context, AppState s, {bool change = false}) => showSheet<String>(
  context,
  title: change ? 'Change Password' : 'Set Password',
  fit: true,
  builder: (ctx) => _PasswordForm(state: s, change: change),
);

class _PasswordForm extends StatefulWidget {
  const _PasswordForm({required this.state, required this.change});
  final AppState state;
  final bool change;
  @override
  State<_PasswordForm> createState() => _PasswordFormState();
}

class _PasswordFormState extends State<_PasswordForm> {
  final _old = TextEditingController(), _pw = TextEditingController(), _again = TextEditingController(), _hint = TextEditingController();
  String? _problem;

  @override
  void dispose() {
    for (final c in [_old, _pw, _again, _hint]) {
      c.dispose();
    }
    super.dispose();
  }

  void _done() {
    final s = widget.state;
    if (_pw.text.isEmpty) return setState(() => _problem = 'Enter a password.');
    if (_pw.text != _again.text) return setState(() => _problem = 'The passwords don\'t match.');
    if (widget.change && !s.checkPassword(_old.text)) return setState(() => _problem = 'The old password is wrong.');
    final hint = _hint.text.trim().isEmpty ? null : _hint.text.trim();
    final r = widget.change
        ? s.call('change_password', {'old': _old.text, 'pw': _pw.text, 'hint': ?hint})
        : s.call('set_password', {'pw': _pw.text, 'hint': ?hint});
    if (r == null) return;
    s.password = _pw.text;
    Navigator.of(context).pop(_pw.text);
  }

  @override
  Widget build(BuildContext context) {
    final p = Palette.of(context);
    Widget field(TextEditingController c, String hint, {bool secure = true, bool first = false}) => Padding(
      padding: const EdgeInsets.symmetric(horizontal: 16),
      child: SizedBox(
        height: 50,
        child: Center(
          child: TextField(
            controller: c,
            autofocus: first,
            obscureText: secure,
            style: Type.body(p.label),
            cursorColor: p.accent,
            decoration: InputDecoration.collapsed(hintText: hint, hintStyle: Type.body(p.tertiary)),
          ),
        ),
      ),
    );
    return Column(
      mainAxisSize: MainAxisSize.min,
      children: [
        Padding(
          padding: const EdgeInsets.fromLTRB(24, 0, 24, 14),
          child: Text(
            'Locked notes are visible only with this password.${widget.change ? '' : ' If you forget it, the locked notes can\'t be opened.'}',
            textAlign: TextAlign.center,
            style: Type.footnote(p.secondary),
          ),
        ),
        if (widget.change) Card2(children: [field(_old, 'Old Password', first: true)]),
        Card2(
          children: [
            field(_pw, 'Password', first: !widget.change),
            field(_again, 'Verify'),
          ],
        ),
        Card2(footer: _problem, children: [field(_hint, 'Hint (recommended)', secure: false)]),
        Padding(
          padding: const EdgeInsets.fromLTRB(16, 0, 16, 8),
          child: Pressable(
            onTap: _done,
            child: Container(
              height: 50,
              alignment: Alignment.center,
              decoration: BoxDecoration(color: p.accent, borderRadius: BorderRadius.circular(25)),
              child: Text('Done', style: Type.text(17, Colors.white, weight: FontWeight.w600)),
            ),
          ),
        ),
      ],
    );
  }
}

/// The notes password (asked for once a session); null when cancelled or wrong.
Future<String?> askPassword(BuildContext context, AppState s, {String title = 'Enter Password'}) async {
  if (s.password != null) return s.password;
  final hint = s.passwordHint;
  final pw = await askText(context, title, message: hint == null ? 'Enter the notes password.' : 'Hint: $hint', hint: 'Password', ok: 'OK', secure: true);
  if (pw == null || pw.isEmpty) return null;
  if (!s.checkPassword(pw)) {
    s.say('Wrong password.');
    return null;
  }
  s.password = pw;
  return pw;
}

/// Lock Note: sets the notes password first when there's none.
Future<bool> lockNote(BuildContext context, AppState s, String id) async {
  var pw = s.hasPassword ? await askPassword(context, s) : await passwordSheet(context, s);
  if (pw == null) return false;
  return s.lock(id, pw);
}

Future<void> removeLock(BuildContext context, AppState s, String id) async {
  final pw = await askPassword(context, s);
  if (pw != null) await s.removeLock(id, pw);
}

/// What a locked note shows until it's opened.
class LockedNote extends StatelessWidget {
  const LockedNote({super.key, required this.onView});
  final VoidCallback onView;
  @override
  Widget build(BuildContext context) {
    final p = Palette.of(context);
    return Center(
      child: Column(
        mainAxisSize: MainAxisSize.min,
        children: [
          Icon(CupertinoIcons.lock_fill, size: 64, color: p.tertiary),
          const SizedBox(height: 16),
          Text('This note is locked.', style: Type.text(20, p.label, weight: FontWeight.w600)),
          const SizedBox(height: 6),
          Text('Enter the notes password to view it.', style: Type.subhead(p.secondary)),
          const SizedBox(height: 22),
          Pressable(
            onTap: onView,
            child: Container(
              height: 48,
              padding: const EdgeInsets.symmetric(horizontal: 28, vertical: 13),
              decoration: BoxDecoration(color: p.fill, borderRadius: BorderRadius.circular(24)),
              child: Text('View Note', style: Type.text(17, p.accent, weight: FontWeight.w600)),
            ),
          ),
        ],
      ),
    );
  }
}
