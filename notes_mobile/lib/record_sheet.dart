// Record Audio: the microphone into an m4a (the record package, as Letters records), shown as
// the sheet's big red button and the time; Done puts the recording in the note as an audio card.
import 'dart:async';

import 'package:flutter/material.dart';
import 'package:path_provider/path_provider.dart';
import 'package:record/record.dart';

import 'theme.dart';
import 'ui/glass.dart';
import 'ui/sheet.dart';

/// The recording's path, or null.
Future<String?> recordAudio(BuildContext context) => showSheet<String>(context, title: 'New Recording', fit: true, builder: (ctx) => const _Recorder());

class _Recorder extends StatefulWidget {
  const _Recorder();
  @override
  State<_Recorder> createState() => _RecorderState();
}

class _RecorderState extends State<_Recorder> {
  AudioRecorder? _rec;
  DateTime? _since;
  Timer? _tick;
  String? _problem;

  @override
  void dispose() {
    _tick?.cancel();
    _rec?.dispose();
    super.dispose();
  }

  Future<void> _start() async {
    final r = AudioRecorder();
    if (!await r.hasPermission()) {
      r.dispose();
      setState(() => _problem = 'Notes needs the microphone to record.');
      return;
    }
    final dir = await getTemporaryDirectory();
    final stamp = DateTime.now();
    final path = '${dir.path}/Recording ${stamp.year}-${stamp.month}-${stamp.day} ${stamp.hour}.${stamp.minute.toString().padLeft(2, '0')}.m4a';
    try {
      await r.start(const RecordConfig(encoder: AudioEncoder.aacLc, bitRate: 128000), path: path);
    } catch (e) {
      r.dispose();
      setState(() => _problem = '$e');
      return;
    }
    setState(() {
      _rec = r;
      _since = DateTime.now();
      _tick = Timer.periodic(const Duration(milliseconds: 250), (_) => setState(() {}));
    });
  }

  Future<void> _stop() async {
    _tick?.cancel();
    final path = await _rec?.stop();
    _rec?.dispose();
    _rec = null;
    if (mounted) Navigator.of(context).pop(path);
  }

  @override
  Widget build(BuildContext context) {
    final p = Palette.of(context);
    final t = _since == null ? 0 : DateTime.now().difference(_since!).inSeconds;
    final on = _rec != null;
    return Padding(
      padding: const EdgeInsets.fromLTRB(16, 4, 16, 16),
      child: Column(
        mainAxisSize: MainAxisSize.min,
        children: [
          Text('${t ~/ 60}:${(t % 60).toString().padLeft(2, '0')}', style: Type.text(40, p.label, weight: FontWeight.w300)),
          const SizedBox(height: 6),
          Text(_problem ?? (on ? 'Recording' : 'Tap to record'), textAlign: TextAlign.center, style: Type.subhead(_problem != null ? p.red : p.secondary)),
          const SizedBox(height: 18),
          Semantics(
            button: true,
            label: on ? 'Stop' : 'Record',
            child: Pressable(
              onTap: on ? _stop : _start,
              child: Container(
                width: 72,
                height: 72,
                decoration: BoxDecoration(
                  shape: BoxShape.circle,
                  border: Border.all(color: p.tertiary, width: 4),
                ),
                child: Center(
                  child: AnimatedContainer(
                    duration: const Duration(milliseconds: 200),
                    width: on ? 28 : 56,
                    height: on ? 28 : 56,
                    decoration: BoxDecoration(color: p.red, borderRadius: BorderRadius.circular(on ? 6 : 28)),
                  ),
                ),
              ),
            ),
          ),
        ],
      ),
    );
  }
}
