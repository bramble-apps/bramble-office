// iOS 26's alerts: a rounded card in the middle (large corners), a bold title and the message
// at the left, text fields, and the buttons side by side (two) or stacked; Cancel never bold, a destructive
// button red; the buttons are capsules.
import 'package:flutter/material.dart';

import '../theme.dart';
import 'glass.dart';

class AlertAction {
  const AlertAction(this.label, {this.destructive = false, this.bold = false, this.cancel = false});
  final String label;
  final bool destructive, bold, cancel;
}

/// An alert; answers the index of the button pressed (null: dismissed) and the fields' text.
Future<(int?, List<String>)> showAlert(
  BuildContext context, {
  required String title,
  String? message,
  List<AlertField> fields = const [],
  required List<AlertAction> actions,
}) async {
  final ctrls = [for (final f in fields) TextEditingController(text: f.initial)];
  final focus = [for (final _ in fields) FocusNode()];
  // the first field takes the keyboard once the alert is up
  if (focus.isNotEmpty) Future.delayed(const Duration(milliseconds: 260), () => focus.first.requestFocus());
  final r = await showGeneralDialog<int>(
    context: context,
    useRootNavigator: true,
    barrierDismissible: fields.isEmpty,
    barrierLabel: 'Close',
    barrierColor: const Color(0x33000000),
    transitionDuration: const Duration(milliseconds: 200),
    transitionBuilder: (ctx, a, _, child) => FadeTransition(
      opacity: a,
      child: ScaleTransition(
        scale: Tween(begin: 1.08, end: 1.0).animate(CurvedAnimation(parent: a, curve: Curves.easeOutCubic)),
        child: child,
      ),
    ),
    pageBuilder: (ctx, _, _) => _Alert(title: title, message: message, fields: fields, ctrls: ctrls, focus: focus, actions: actions),
  );
  final texts = [for (final c in ctrls) c.text];
  // (after the closing animation, which still shows the fields)
  Future.delayed(const Duration(milliseconds: 500), () {
    for (final c in ctrls) {
      c.dispose();
    }
    for (final f in focus) {
      f.dispose();
    }
  });
  return (r, texts);
}

class AlertField {
  const AlertField({this.hint = '', this.initial = '', this.secure = false});
  final String hint, initial;
  final bool secure;
}

class _Alert extends StatelessWidget {
  const _Alert({required this.title, required this.message, required this.fields, required this.ctrls, required this.focus, required this.actions});
  final List<FocusNode> focus;
  final String title;
  final String? message;
  final List<AlertField> fields;
  final List<TextEditingController> ctrls;
  final List<AlertAction> actions;

  @override
  Widget build(BuildContext context) {
    final p = Palette.of(context);
    final mq = MediaQuery.of(context);
    final side = actions.length == 2;
    Widget button(int i) {
      final a = actions[i];
      return Expanded(
        flex: side ? 1 : 0,
        child: Pressable(
          onTap: () => Navigator.of(context).pop(i),
          child: Container(
            height: 48,
            margin: const EdgeInsets.all(4),
            alignment: Alignment.center,
            decoration: BoxDecoration(color: p.fill, borderRadius: BorderRadius.circular(24)),
            child: Text(
              a.label,
              style: Type.text(17, a.destructive ? p.red : (a.bold ? p.accent : p.label), weight: a.bold ? FontWeight.w600 : FontWeight.w500),
            ),
          ),
        ),
      );
    }

    return Padding(
      padding: EdgeInsets.only(bottom: mq.viewInsets.bottom),
      child: Center(
        child: Material(
          color: p.sheetCard,
          borderRadius: BorderRadius.circular(30),
          elevation: 0,
          child: SizedBox(
            width: 300,
            child: Padding(
              padding: const EdgeInsets.fromLTRB(14, 20, 14, 10),
              child: Column(
                mainAxisSize: MainAxisSize.min,
                crossAxisAlignment: CrossAxisAlignment.stretch,
                children: [
                  Padding(
                    padding: const EdgeInsets.symmetric(horizontal: 8),
                    child: Text(title, style: Type.text(17, p.label, weight: FontWeight.w600)),
                  ),
                  if (message != null)
                    Padding(
                      padding: const EdgeInsets.fromLTRB(8, 6, 8, 0),
                      child: Text(message!, style: Type.subhead(p.label)),
                    ),
                  for (final (i, f) in fields.indexed)
                    Padding(
                      padding: EdgeInsets.fromLTRB(4, i == 0 ? 14 : 8, 4, 0),
                      child: Container(
                        height: 44,
                        padding: const EdgeInsets.symmetric(horizontal: 12),
                        decoration: BoxDecoration(color: p.fill, borderRadius: BorderRadius.circular(12)),
                        alignment: Alignment.centerLeft,
                        child: TextField(
                          controller: ctrls[i],
                          focusNode: focus[i],
                          obscureText: f.secure,
                          style: Type.body(p.label),
                          cursorColor: p.accent,
                          textInputAction: i == fields.length - 1 ? TextInputAction.done : TextInputAction.next,
                          onSubmitted: i == fields.length - 1
                              ? (_) => Navigator.of(context).pop(actions.indexWhere((a) => a.bold).clamp(0, actions.length - 1))
                              : null,
                          decoration: InputDecoration.collapsed(hintText: f.hint, hintStyle: Type.body(p.tertiary)),
                        ),
                      ),
                    ),
                  const SizedBox(height: 14),
                  if (side)
                    Row(children: [button(0), button(1)])
                  else ...[
                    for (var i = 0; i < actions.length; i++)
                      Row(
                        children: [
                          Expanded(child: Row(children: [button(i)])),
                        ],
                      ),
                  ],
                ],
              ),
            ),
          ),
        ),
      ),
    );
  }
}

Future<bool> confirm(BuildContext context, String title, {String? message, String ok = 'Delete', bool destructive = true}) async {
  final (r, _) = await showAlert(
    context,
    title: title,
    message: message,
    actions: [
      const AlertAction('Cancel', cancel: true),
      AlertAction(ok, destructive: destructive, bold: !destructive),
    ],
  );
  return r == 1;
}

/// Asks for one line of text (a folder's name…); null when cancelled.
Future<String?> askText(
  BuildContext context,
  String title, {
  String? message,
  String hint = '',
  String initial = '',
  String ok = 'Save',
  bool secure = false,
}) async {
  final (r, t) = await showAlert(
    context,
    title: title,
    message: message,
    fields: [AlertField(hint: hint, initial: initial, secure: secure)],
    actions: [const AlertAction('Cancel', cancel: true), AlertAction(ok, bold: true)],
  );
  return r == 1 ? t.first : null;
}
