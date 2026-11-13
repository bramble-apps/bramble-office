use super::*;
use crate::js::Interp;
use crate::midifx::{build, Ctx, Ev, Msg};

fn eval(src: &str) -> String {
    let mut it = Interp::new(Box::new(()));
    match it.eval_source(src) {
        Ok(v) => it.show(&v),
        Err(e) => it.describe(&e),
    }
}

#[test]
fn language_basics() {
    assert_eq!(eval("1 + 2 * 3"), "7");
    assert_eq!(eval("'a' + 1 + 2"), "a12");
    assert_eq!(eval("var x = 5; x += 2; x++; x"), "8");
    assert_eq!(eval("function f(a, b = 3) { return a * b; } f(2)"), "6");
    assert_eq!(eval("var add = (a, b) => a + b; add(2, 3)"), "5");
    assert_eq!(eval("var s = 0; for (var i = 0; i < 10; i++) { if (i == 5) continue; if (i == 8) break; s += i; } s"), "23");
    assert_eq!(eval("var o = {a: 1, b: {c: [1, 2, 3]}}; o.b.c.length + o.a"), "4");
    assert_eq!(eval("var a = [3, 1, 2]; a.sort(); a.join('-')"), "1-2-3");
    assert_eq!(eval("[10, 9, 1].sort()"), "[1,10,9]");
    assert_eq!(eval("[10, 9, 1].sort(function (a, b) { return a - b; })"), "[1,9,10]");
    assert_eq!(eval("[1, 2, 3].map(x => x * 2).filter(x => x > 2).reduce((a, b) => a + b, 0)"), "10");
    assert_eq!(eval("typeof undefinedThing"), "undefined");
    assert_eq!(eval("typeof function () {}"), "function");
    assert_eq!(eval("var n = 0; while (n < 3) n++; do { n += 10; } while (false); n"), "13");
    assert_eq!(eval("var r; switch (2) { case 1: r = 'one'; break; case 2: r = 'two'; case 3: r += '!'; break; default: r = 'd'; } r"), "two!");
    assert_eq!(eval("var r; try { null.x; } catch (e) { r = e instanceof TypeError; } r"), "true");
    assert_eq!(eval("var r; try { throw new Error('boom'); } catch (e) { r = e.message; } r"), "boom");
    assert_eq!(eval("var f = 0; try { f = 1; } finally { f = 2; } f"), "2");
    assert_eq!(eval("function C(v) { this.v = v; } C.prototype.get = function () { return this.v; }; var c = new C(4); c.get() + (c instanceof C ? 1 : 0)"), "5");
    assert_eq!(eval("var k = []; for (var p in {a: 1, b: 2}) k.push(p); k.join()"), "a,b");
    assert_eq!(eval("var t = 0; for (var v of [1, 2, 3]) t += v; t"), "6");
    assert_eq!(eval("var name = 'x'; `n=${name}, ${1 + 1}`"), "n=x, 2");
    assert_eq!(eval("(0.1 + 0.2).toFixed(2) + ' ' + Math.max(1, 5, 3) + ' ' + parseInt('42px') + ' ' + (7 % 3)"), "0.30 5 42 1");
    assert_eq!(eval("'Hello'.toUpperCase().slice(1, 3) + 'a,b'.split(',').length"), "EL2");
    assert_eq!(eval("1 === 1.0 && '1' == 1 && null == undefined && null !== undefined"), "true");
    assert_eq!(eval("function mk() { var n = 0; return function () { return ++n; }; } var g = mk(); g(); g()"), "2");
    assert_eq!(eval("JSON.stringify({a: [1, 'x'], b: true})"), "{\"a\":[1,\"x\"],\"b\":true}");
    assert_eq!(eval("x = 3; x"), "3");
    assert_eq!(eval("0.5 + 0.25"), "0.75");
    // hoisting: a function is callable before its declaration
    assert_eq!(eval("f(); function f() { return 9; } f()"), "9");
    // errors name the line
    assert_eq!(eval("var a = 1;\n\nnope();"), "ReferenceError: Can't find variable: nope (line 3)");
}

#[test]
fn syntax_errors_carry_the_line() {
    let e = crate::js::check("function f() {\n  var x = ;\n}").expect("an error");
    assert_eq!(e.line, 2);
    assert!(crate::js::check("var ok = [1, 2];\nfunction HandleMIDI(e) { e.send(); }").is_none());
    assert!(crate::js::check("var s = 'open").is_some());
}

#[test]
fn runaway_scripts_stop() {
    let mut it = Interp::new(Box::new(()));
    it.budget = 10_000;
    match it.eval_source("while (true) {}") {
        Err(crate::js::Abort::Halt(m)) => assert!(m.contains("too long")),
        _ => panic!("should stop"),
    }
    // and it can't be caught
    it.budget = 10_000;
    assert!(matches!(it.eval_source("try { for (;;) {} } catch (e) {}"), Err(crate::js::Abort::Halt(_))));
    assert_eq!(eval("function r(n) { return r(n + 1); } var k; try { r(0) } catch (e) { k = e.name; } k"), "RangeError");
}

// ---- the unit

fn unit(script: &str) -> Box<dyn crate::midifx::MidiFx> {
    build("Scripter", &load_script(script)).expect("scripter")
}

fn ctx(from: f64, to: f64, playing: bool) -> Ctx {
    Ctx { from, to, bpm: 120.0, playing }
}

fn run(u: &mut Box<dyn crate::midifx::MidiFx>, input: &[Ev], c: Ctx) -> Vec<Ev> {
    let mut out = Vec::new();
    u.process(input, &c, &mut out);
    out
}

fn view(u: &dyn crate::midifx::MidiFx) -> View {
    (*u.status().script.expect("view")).clone()
}

fn on(tick: f64, pitch: u8, vel: u8) -> Ev {
    Ev::new(tick, Msg::On { ch: 0, pitch, vel })
}

#[test]
fn script_lives_in_the_values() {
    let v = with_script(&[1.0, 2.0], "var a = 'é';");
    assert_eq!(v.len(), SCRIPT_AT + 12);
    assert_eq!(script_of(&v), "var a = 'é';");
    assert_eq!(v[0], 1.0);
    assert_eq!(script_of(&defaults()), DEFAULT_SCRIPT);
    // an old slot without a script gets the default one
    assert_eq!(script_of(&[64.0; 4]), DEFAULT_SCRIPT);
}

#[test]
fn handle_midi_and_pass_through() {
    // no HandleMIDI: events pass through unchanged
    let mut u = unit("var x = 1;");
    let out = run(&mut u, &[on(10.0, 60, 90)], ctx(0.0, 100.0, true));
    assert_eq!(out, vec![on(10.0, 60, 90)]);
    // HandleMIDI gets one event object; not sending it drops it
    let mut u = unit("function HandleMIDI(e) { if (e.pitch != 61) e.send(); }");
    let out = run(&mut u, &[on(10.0, 60, 90), on(20.0, 61, 90)], ctx(0.0, 100.0, true));
    assert_eq!(out, vec![on(10.0, 60, 90)]);
    // tutorial 2 logs without sending
    let mut u = unit(TUTORIALS[1].1);
    let out = run(&mut u, &[on(0.0, 60, 100)], ctx(0.0, 100.0, true));
    assert!(out.is_empty());
    assert_eq!(view(&*u).console.last().map(String::as_str), Some("[NoteOn channel:1 pitch:60 [C3] velocity:100]"));
}

#[test]
fn event_types_and_ranges() {
    let mut u = unit(
        "function HandleMIDI(e) {\n\
         var c = new ControlChange; c.number = 300; c.value = -5; c.channel = 20; c.send();\n\
         var b = new PitchBend; b.value = 9000; b.send();\n\
         var p = new ProgramChange; p.number = 5; p.send();\n\
         var a = new ChannelPressure; a.value = 50; a.send();\n\
         var pp = new PolyPressure; pp.pitch = 64; pp.value = 70; pp.send();\n\
         var z = new NoteOn(e); z.velocity = 0; z.send();\n\
         }",
    );
    let out: Vec<Msg> = run(&mut u, &[on(0.0, 62, 80)], ctx(0.0, 100.0, true)).into_iter().map(|e| e.msg).collect();
    assert_eq!(
        out,
        vec![
            Msg::Cc { ch: 15, num: 127, val: 0 },
            Msg::Bend { ch: 0, val: 8191 },
            Msg::Program { ch: 0, num: 5 },
            Msg::Pressure { ch: 0, val: 50 },
            Msg::PolyAt { ch: 0, pitch: 64, val: 70 },
            // a NoteOn with velocity 0 is a note off
            Msg::Off { ch: 0, pitch: 62 },
        ]
    );
    assert_eq!(eval_api("var e = new NoteOn; e.pitch = 64; e.toString()"), "[NoteOn channel:1 pitch:64 [E3] velocity:100]");
    assert_eq!(eval_api("var e = new ControlChange; e.number = 7; e.value = 3; String(e)"), "[ControlChange channel:1 number:7 [Volume] value:3]");
    assert_eq!(eval_api("new NoteOff instanceof Event"), "true");
}

fn eval_api(src: &str) -> String {
    let mut it = new_interp();
    match it.eval_source(src) {
        Ok(v) => it.show(&v),
        Err(e) => it.describe(&e),
    }
}

#[test]
fn midi_object() {
    assert_eq!(eval_api("MIDI.noteNumber('C3') + ' ' + MIDI.noteNumber('B#2') + ' ' + MIDI.noteNumber('Db3')"), "60 60 undefined");
    assert_eq!(eval_api("MIDI.noteName(61) + ' ' + MIDI.ccName(64)"), "C#3 Sustain");
    assert_eq!(eval_api("[MIDI.normalizeStatus(5), MIDI.normalizeStatus(300), MIDI.normalizeChannel(0), MIDI.normalizeChannel(17), MIDI.normalizeData(-3), MIDI.normalizeData(200)].join()"), "128,239,1,16,0,127");
    // tutorial 11: CC 20 sends all notes off on all 16 channels
    let mut u = unit(TUTORIALS[10].1);
    let out = run(&mut u, &[Ev::new(5.0, Msg::Cc { ch: 0, num: 20, val: 127 })], ctx(0.0, 100.0, true));
    assert_eq!(out.len(), 16);
    assert!(out.iter().all(|e| matches!(e.msg, Msg::Cc { num: 123, .. })));
}

#[test]
fn delayed_sends() {
    // tutorial 3: the note, then an octave up 100 ms later (120 bpm: 1920 ticks a second)
    let mut u = unit(TUTORIALS[2].1);
    let out = run(&mut u, &[on(0.0, 60, 100)], ctx(0.0, 100.0, true));
    assert_eq!(out, vec![on(0.0, 60, 100)]);
    let out = run(&mut u, &[], ctx(100.0, 400.0, true));
    assert_eq!(out, vec![on(192.0, 72, 100)]);
    // sendAtBeat / sendAfterBeats on the song's beats (beat 1 = tick 0)
    let mut u = unit("function HandleMIDI(e) { e.sendAtBeat(3); e.pitch = 50; e.sendAfterBeats(0.5); }");
    let out = run(&mut u, &[on(0.0, 60, 100)], ctx(0.0, 600.0, true));
    assert_eq!(out, vec![on(480.0, 50, 100)]);
    let out = run(&mut u, &[], ctx(600.0, 2000.0, true));
    assert_eq!(out, vec![on(1920.0, 60, 100)]);
}

#[test]
fn beat_pos_and_timing_info() {
    // with NeedsTimingInfo: beatPos is the event's beat; send() == sendAtBeat(beatPos)
    let mut u = unit("var NeedsTimingInfo = true;\nfunction HandleMIDI(e) { Trace(e.beatPos); e.beatPos += 1; e.send(); }");
    let out = run(&mut u, &[on(480.0, 60, 100)], ctx(0.0, 1000.0, true));
    assert!(out.is_empty());
    assert_eq!(view(&*u).console.last().map(String::as_str), Some("1.5"));
    let out = run(&mut u, &[], ctx(1000.0, 2000.0, true));
    assert_eq!(out, vec![on(1440.0, 60, 100)]);
    // without it beatPos is 0 and changing it only warns
    let mut u = unit("function HandleMIDI(e) { Trace(e.beatPos); e.beatPos = 3; e.send(); }");
    let out = run(&mut u, &[on(480.0, 60, 100)], ctx(0.0, 1000.0, true));
    assert_eq!(out, vec![on(480.0, 60, 100)]);
    let c = view(&*u).console;
    assert_eq!(c[0], "0");
    assert!(c[1].contains("NeedsTimingInfo"));
    // a NoteOff made from a NoteOn doesn't take its beatPos
    let mut u = unit("var NeedsTimingInfo = true;\nfunction HandleMIDI(e) { var o = new NoteOff(e); Trace(o.beatPos + ' ' + o.pitch); var n = new NoteOn(e); Trace(n.beatPos); }");
    run(&mut u, &[on(960.0, 64, 100)], ctx(0.0, 1000.0, true));
    let c = view(&*u).console;
    assert_eq!(&c[c.len() - 2..], &["0 64".to_string(), "2".to_string()]);
    // GetTimingInfo
    set_transport(Transport { meter: (3, 4), cycling: true, cycle: (960.0, 4800.0) });
    let mut u = unit("var NeedsTimingInfo = true;\nfunction ProcessMIDI() { var t = GetTimingInfo(); Trace([t.playing, t.blockStartBeat, t.blockEndBeat, t.blockLength, t.tempo, t.meterNumerator, t.meterDenominator, t.cycling, t.leftCycleBeat, t.rightCycleBeat].join()); }");
    run(&mut u, &[], ctx(960.0, 1440.0, true));
    assert_eq!(view(&*u).console.last().map(String::as_str), Some("true,2,2.5,0.5,120,3,4,true,2,6"));
    set_transport(Transport::default());
    // needs NeedsTimingInfo
    let mut u = unit("function ProcessMIDI() { Trace(typeof GetTimingInfo()); }");
    run(&mut u, &[], ctx(0.0, 100.0, true));
    let c = view(&*u).console;
    assert!(c[0].contains("NeedsTimingInfo"));
    assert_eq!(c[1], "undefined");
}

#[test]
fn process_midi_runs_every_block() {
    let mut u = unit("var n = 0; function ProcessMIDI() { n++; } function HandleMIDI(e) { Trace(n); }");
    for k in 0..3 {
        run(&mut u, &[], ctx(k as f64 * 100.0, k as f64 * 100.0 + 100.0, true));
    }
    run(&mut u, &[on(300.0, 60, 1)], ctx(300.0, 400.0, true));
    assert_eq!(view(&*u).console.last().map(String::as_str), Some("3"));
    // tutorial 5 prints the tempo while playing only
    let mut u = unit(TUTORIALS[4].1);
    run(&mut u, &[], ctx(0.0, 100.0, false));
    assert!(view(&*u).console.is_empty());
    run(&mut u, &[], ctx(0.0, 100.0, true));
    assert_eq!(view(&*u).console, vec!["tempo: 120".to_string()]);
}

#[test]
fn plugin_parameters() {
    let src = "var PluginParameters = [\n\
        {name:'Basic'},\n\
        {name:'Lin', type:'lin', minValue:-10, maxValue:10, numberOfSteps:20, defaultValue:3, unit:'st'},\n\
        {name:'Log', type:'log', minValue:20, maxValue:20000, defaultValue:440},\n\
        {name:'Push', type:'momentary'},\n\
        {name:'Pick', type:'menu', valueStrings:['A', 'B', 'C'], defaultValue:2},\n\
        {name:'Header', type:'text'},\n\
        {name:'Hidden', hidden:true, disableAutomation:true}];";
    let (ps, err) = read_params(src);
    assert!(err.is_none());
    assert_eq!(ps.len(), 7);
    let b = &ps[0];
    assert_eq!((b.kind, b.min, b.max, b.default, b.unit.as_str()), (PKind::Lin, 0.0, 1.0, 0.0, ""));
    assert_eq!((ps[1].min, ps[1].max, ps[1].default, ps[1].steps), (-10.0, 10.0, 3.0, Some(20)));
    assert_eq!(ps[1].text(3.0), "3 st");
    assert_eq!(ps[1].snap(3.4), 3.0);
    assert_eq!(ps[2].kind, PKind::Log);
    assert_eq!(ps[3].kind, PKind::Momentary);
    assert_eq!((ps[4].kind, ps[4].max, ps[4].default), (PKind::Menu, 2.0, 2.0));
    assert_eq!(ps[4].text(1.0), "B");
    assert_eq!(ps[5].kind, PKind::Text);
    assert!(ps[6].hidden && ps[6].no_automation);
    // name is required; a menu needs valueStrings
    assert!(read_params("var PluginParameters = [{type:'lin'}];").1.is_some());
    assert!(read_params("var PluginParameters = [{name:'M', type:'menu'}];").1.unwrap().1.contains("valueStrings"));
    // values start at the defaults
    let v = load_script(src);
    assert_eq!(&v[..5], &[0.0, 3.0, 440.0, 0.0, 2.0]);
}

#[test]
fn get_set_parameter_and_parameter_changed() {
    // tutorial 6: the Velocity slider (1…127, default 80) sets every note's velocity
    let (ps, _) = read_params(TUTORIALS[5].1);
    assert_eq!((ps[0].min, ps[0].max, ps[0].default), (1.0, 127.0, 80.0));
    let mut u = unit(TUTORIALS[5].1);
    assert_eq!(run(&mut u, &[on(0.0, 60, 30)], ctx(0.0, 100.0, true)), vec![on(0.0, 60, 80)]);
    let mut v = load_script(TUTORIALS[5].1);
    v[0] = 110.0;
    u.load(&v);
    assert_eq!(run(&mut u, &[on(0.0, 60, 30)], ctx(0.0, 100.0, true)), vec![on(0.0, 60, 110)]);
    // ParameterChanged: once per parameter when loaded, then on each change (index, value)
    let src = "var PluginParameters = [{name:'A'}, {name:'B', maxValue:10}];\nfunction ParameterChanged(i, v) { Trace(i + ':' + v); }";
    let mut v = load_script(src);
    let mut u = build("Scripter", &v).unwrap();
    assert_eq!(view(&*u).console, vec!["0:0".to_string(), "1:0".to_string()]);
    v[1] = 4.0;
    u.load(&v);
    assert_eq!(view(&*u).console.last().map(String::as_str), Some("1:4"));
    // GetParameter needs the exact name; SetParameter takes an index or a name
    let src = "var PluginParameters = [{name:'Gain', maxValue:10}];\nfunction HandleMIDI(e) { Trace(GetParameter('Gain') + ' ' + GetParameter('gain')); SetParameter('Gain', 7); SetParameter(0, GetParameter(0) + 1); }";
    let mut u = unit(src);
    run(&mut u, &[on(0.0, 60, 1)], ctx(0.0, 100.0, true));
    let w = view(&*u);
    assert_eq!(w.console.last().map(String::as_str), Some("0 undefined"));
    assert_eq!(w.values[0], 8.0);
    assert_eq!(w.set_serial, 2);
}

#[test]
fn update_plugin_parameters_hides_controls() {
    let mut v = load_script(TUTORIALS[13].1);
    let mut u = build("Scripter", &v).unwrap();
    assert!(view(&*u).params[1].hidden);
    v[0] = 1.0;
    u.load(&v);
    assert!(!view(&*u).params[1].hidden);
    assert_eq!(run(&mut u, &[on(0.0, 60, 100)], ctx(0.0, 100.0, true)), vec![on(0.0, 72, 100)]);
}

#[test]
fn reset_on_transport_start_and_bypass() {
    let mut u = unit("var n = 0; function Reset() { Trace('reset ' + n); n = 0; } function HandleMIDI(e) { n++; }");
    run(&mut u, &[on(0.0, 60, 1)], ctx(0.0, 100.0, false));
    run(&mut u, &[], ctx(0.0, 100.0, true));
    assert_eq!(view(&*u).console, vec!["reset 1".to_string()]);
    // still playing: no second Reset
    run(&mut u, &[on(100.0, 60, 1)], ctx(100.0, 200.0, true));
    assert_eq!(view(&*u).console.len(), 1);
    let mut out = Vec::new();
    u.bypassed(&mut out);
    assert_eq!(view(&*u).console.last().map(String::as_str), Some("reset 1"));
}

#[test]
fn console_errors_and_thinning() {
    let mut u = unit("var a = 1;\nfunction HandleMIDI(e) {\n  e.nope();\n}");
    run(&mut u, &[on(0.0, 60, 1)], ctx(0.0, 100.0, true));
    let w = view(&*u);
    assert_eq!(w.error.as_ref().map(|e| e.0), Some(3));
    assert!(w.console.last().unwrap().contains("(line 3)"));
    // a syntax error stops the script and names its line
    let u = unit("var a = 1;\nvar b = ;\n");
    assert_eq!(view(&*u).error.as_ref().map(|e| e.0), Some(2));
    // too much output at once is thinned
    let mut u = unit("function HandleMIDI(e) { for (var i = 0; i < 500; i++) Trace(i); }");
    run(&mut u, &[on(0.0, 60, 1)], ctx(0.0, 100.0, true));
    let c = view(&*u).console;
    assert_eq!(c.len(), 41);
    assert!(c[40].contains("460 more lines"));
    // Trace takes any value, joined by string concatenation
    let mut u = unit("Trace('n=' + 3 + ' ' + true); Trace([1, 2]); Trace({a: 1});");
    run(&mut u, &[], ctx(0.0, 1.0, true));
    assert_eq!(view(&*u).console, vec!["n=3 true".to_string(), "[1,2]".to_string(), "{\"a\":1}".to_string()]);
}

#[test]
fn interactive_console() {
    let mut u = unit("var x = 21;");
    u.command("eval:x * 2");
    assert_eq!(view(&*u).console.last().map(String::as_str), Some("42"));
    u.command("eval:Trace(MIDI.noteName(60))");
    assert_eq!(view(&*u).console.last().map(String::as_str), Some("C3"));
    u.command("eval:nope(");
    assert!(view(&*u).console.last().unwrap().starts_with("SyntaxError"));
    u.command("clear");
    assert!(view(&*u).console.is_empty());
}

#[test]
fn rerun_keeps_matching_values() {
    let a = "var PluginParameters = [{name:'A', maxValue:10, defaultValue:1}, {name:'B', maxValue:10, defaultValue:2}];";
    let b = "var PluginParameters = [{name:'A', maxValue:10, defaultValue:1}, {name:'C', maxValue:10, defaultValue:5}];";
    let mut v = load_script(a);
    v[0] = 9.0;
    v[1] = 9.0;
    let w = rerun_values(&v, a, b);
    assert_eq!(&w[..2], &[9.0, 5.0]);
    assert_eq!(script_of(&w), b);
}

#[test]
fn every_bundled_script_runs_clean() {
    for (name, src) in TUTORIALS.iter().chain(FACTORY.iter()) {
        let (_, err) = read_params(src);
        assert!(err.is_none(), "{name}: {err:?}");
        let mut u = unit(src);
        let mut v = load_script(src);
        for x in v.iter_mut().take(4) {
            *x = x.max(0.0);
        }
        u.load(&v);
        let mut t = 0.0;
        for k in 0..20 {
            let input = if k % 5 == 0 { vec![on(t, 60, 100), Ev::new(t + 1.0, Msg::Off { ch: 0, pitch: 60 })] } else { vec![] };
            run(&mut u, &input, Ctx { from: t, to: t + 240.0, bpm: 120.0, playing: true });
            t += 240.0;
        }
        assert!(view(&*u).error.is_none(), "{name}: {:?}", view(&*u).error);
    }
}

#[test]
fn simple_arpeggiator_plays_held_notes() {
    let src = FACTORY.iter().find(|f| f.0 == "Simple Arpeggiator").unwrap().1;
    let mut u = unit(src);
    run(&mut u, &[], ctx(0.0, 10.0, true));
    run(&mut u, &[on(10.0, 64, 100), on(10.0, 60, 100)], ctx(10.0, 20.0, true));
    let out = run(&mut u, &[], ctx(20.0, 1000.0, true));
    let ons: Vec<(f64, u8)> = out.iter().filter_map(|e| if let Msg::On { pitch, .. } = e.msg { Some((e.tick, pitch)) } else { None }).collect();
    assert_eq!(ons, vec![(240.0, 60), (480.0, 64), (720.0, 60), (960.0, 64)]);
}

#[test]
fn target_event_sends_the_chosen_controller() {
    let mut v = load_script(TUTORIALS[14].1);
    v[0] = 74.0;
    let mut u = build("Scripter", &v).unwrap();
    let out = run(&mut u, &[on(0.0, 60, 127)], ctx(0.0, 10.0, true));
    assert_eq!(out[0].msg, Msg::Cc { ch: 0, num: 74, val: 127 });
    assert_eq!(view(&*u).params[0].strings[74], "MIDI CC 74 (Cutoff)");
}

#[test]
fn detune_bends_before_the_note() {
    let mut v = load_script(TUTORIALS[3].1);
    let (ps, _) = read_params(TUTORIALS[3].1);
    assert_eq!((ps[0].min, ps[0].max, ps[0].default), (-100.0, 100.0, 0.0));
    v[0] = 50.0;
    let mut u = build("Scripter", &v).unwrap();
    let out: Vec<Msg> = run(&mut u, &[on(0.0, 60, 100)], ctx(0.0, 10.0, true)).into_iter().map(|e| e.msg).collect();
    assert_eq!(out, vec![Msg::Bend { ch: 0, val: 2048 }, Msg::On { ch: 0, pitch: 60, vel: 100 }]);
}
