#!/usr/bin/env python3
"""GUI monkey test: random clicks, drags, typing, keys and scrolls (NM_SCRIPT) on a sample, checking
the app never panics. One instance at a time, killed after a timeout.
usage: tools/monkey.py <numdeck binary> <sample.numbers> <seed> <steps>"""
import os, random, subprocess, sys

binary, sample, seed, steps = sys.argv[1], sys.argv[2], int(sys.argv[3]), int(sys.argv[4])
rng = random.Random(seed)
W, H = 1400, 900  # window points
keys = ["Enter", "Tab", "Escape", "Delete", "Backspace", "ArrowUp", "ArrowDown", "ArrowLeft", "ArrowRight",
        "ctrl+Z", "shift+ctrl+Z", "ctrl+A", "ctrl+C", "ctrl+X", "ctrl+Plus", "ctrl+Minus", "F2", "shift+ArrowDown", "shift+Tab"]
words = ["12", "=B2+1", "hello", "=SUM(A1:B3)", "-4.5", "=XLOOKUP(1,A:A,B:B)", "=1/0", "x", "=B2*", "100%", "=IF(A1>2,\"y\",\"n\")"]
steps_out = []
for _ in range(steps):
    r = rng.random()
    x, y = rng.uniform(0, W), rng.uniform(0, H)
    if r < 0.35:
        steps_out.append(f"click {x:.0f} {y:.0f}")
    elif r < 0.45:
        steps_out.append(f"dclick {x:.0f} {y:.0f}")
    elif r < 0.6:
        steps_out.append(f"drag {x:.0f} {y:.0f} {x + rng.uniform(-300, 300):.0f} {y + rng.uniform(-300, 300):.0f}")
    elif r < 0.75:
        steps_out.append("type " + rng.choice(words))
    elif r < 0.92:
        steps_out.append("key " + rng.choice(keys))
    else:
        steps_out.append(f"scroll {x:.0f} {y:.0f} {rng.uniform(-60, 60):.0f} {rng.uniform(-60, 60):.0f} {rng.randint(1, 10)}")
env = dict(os.environ, NM_SNAP=f"/tmp/claude-1000/monkey-{seed}.png", NM_SCRIPT="; ".join(steps_out))
env = {k: v for k, v in env.items() if not k.startswith("KN_")}
try:
    p = subprocess.run([binary, sample], env=env, capture_output=True, text=True, timeout=int(os.environ.get("MONKEY_TIMEOUT", "240")))
    out = p.stdout + p.stderr
    bad = "panicked" in out or p.returncode not in (0,)
    print(f"seed {seed}: exit {p.returncode}{' PANIC' if 'panicked' in out else ''}")
    if bad:
        print("\n".join(l for l in out.splitlines() if "panicked" in l or "error" in l.lower())[:2000])
        print("SCRIPT:", env["NM_SCRIPT"][:4000])
except subprocess.TimeoutExpired:
    # subprocess.run has already killed this run's own process (never other numdeck windows)
    print(f"seed {seed}: TIMEOUT")
