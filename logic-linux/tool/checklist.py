#!/usr/bin/env python3
"""Regenerates the item-by-item part of CHECKLIST.md (below the GENERATED marker) from the app's
data files (Logic's menus, right-click menus, local menus, key commands) and what the source
handles: menus.rs IMPLEMENTED (menu bar and local menus), context.rs IMPLEMENTED (right-click
menus) and app.rs key_command (key commands). Run from logic-linux: python3 tool/checklist.py"""
import re, os, collections

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
APP = os.path.join(ROOT, 'crates/lg-app')
MARK = '<!-- GENERATED: tool/checklist.py rewrites everything below -->'

def src(name):
    p = os.path.join(APP, 'src', name)
    return open(p).read() if os.path.exists(p) else ''

def const_list(text, const):
    i = text.find(const)
    if i < 0:
        return set()
    j = text.find('];', i)
    return set(re.findall(r'"((?:[^"\\]|\\.)*)"', text[i:j]))

def handled_keys():
    t = src('app.rs')
    i = t.find('pub fn key_command')
    if i < 0:
        return set()
    j = t.find('\n    pub fn ', i + 10)
    names = set(re.findall(r'"((?:[^"\\]|\\.)*)"', t[i:j]))
    return names | set(re.findall(r'"((?:[^"\\]|\\.)*)"', src('edit.rs')))

def menu_tree(path):
    """[(section, [(depth, label, key, disabled)])]"""
    secs, cur = [], None
    for l in open(path):
        l = l.rstrip('\n')
        if l.startswith('#') or not l.strip():
            continue
        m = re.match(r'=== (.*) ===', l)
        if m:
            cur = (m.group(1), [])
            secs.append(cur)
            continue
        s = l.strip()
        if s == '---' or cur is None:
            continue
        depth = (len(l) - len(l.lstrip(' '))) // 4
        key = ''
        km = re.search(r'\s+\[(.+)\]$', s)
        if km:
            key, s = km.group(1), s[:km.start()]
        dis = '(disabled)' in s
        s = s.replace('(disabled)', '').strip().rstrip(' ▸').lstrip('✓ ').strip()
        cur[1].append((depth, s, key))
    return secs

def section(out, title, secs, done):
    tot = sum(len(it) for _, it in secs)
    n = sum(1 for _, it in secs for x in it if x[1] in done)
    out.append(f'\n## {title}: {n} of {tot}\n')
    for name, items in secs:
        k = sum(1 for x in items if x[1] in done)
        out.append(f'\n<details><summary>{name} — {k}/{len(items)}</summary>\n')
        for d, lab, key in items:
            out.append(f"{'  ' * d}- [{'x' if lab in done else ' '}] {lab}{f'  `{key}`' if key else ''}")
        out.append('\n</details>')
    return n, tot

def main():
    menus_done = const_list(src('menus.rs'), 'IMPLEMENTED')
    ctx_done = const_list(src('context.rs'), 'IMPLEMENTED')
    keys_done = handled_keys()
    menus_done = menus_done | keys_done
    out = [MARK, '\n# Item by item\n', 'Generated from `crates/lg-app/data/*.txt` (Logic Pro 13, from the research) against the source. `[x]` = works in Studio.']
    totals = []
    totals.append(('Menu bar', *section(out, 'Menu bar', menu_tree(os.path.join(APP, 'data/menus.txt')), menus_done)))
    totals.append(('Local menus', *section(out, 'Local menus (Edit / Functions / View in each area)', menu_tree(os.path.join(APP, 'data/local-menus.txt')), menus_done)))
    totals.append(('Right-click menus', *section(out, 'Right-click menus', menu_tree(os.path.join(APP, 'data/context-menus.txt')), ctx_done | menus_done)))
    groups = collections.OrderedDict()
    for l in open(os.path.join(APP, 'data/commands.txt')):
        if l.startswith('#'):
            continue
        a, c, k = (l.rstrip('\n').split('\t') + ['', ''])[:3]
        groups.setdefault(a, []).append((c, k))
    tot = sum(len(v) for v in groups.values())
    n = sum(1 for v in groups.values() for c, _ in v if c in keys_done)
    totals.append(('Key commands', n, tot))
    out.append(f'\n## Key commands: {n} of {tot}\n')
    for a, v in groups.items():
        k = sum(1 for c, _ in v if c in keys_done)
        out.append(f'\n<details><summary>{a} — {k}/{len(v)}</summary>\n')
        for c, key in v:
            out.append(f"- [{'x' if c in keys_done else ' '}] {c}{f'  `{key}`' if key else ''}")
        out.append('\n</details>')
    summary = ['\n## Totals\n', '| Area | Done | Of | % |', '|---|---:|---:|---:|']
    for name, n, tot in totals:
        summary.append(f'| {name} | {n} | {tot} | {100 * n // max(tot, 1)}% |')
    out[2:2] = summary
    path = os.path.join(ROOT, 'CHECKLIST.md')
    head = open(path).read().split(MARK)[0] if os.path.exists(path) else '# Bramble Studio — 1-1 checklist\n\n'
    open(path, 'w').write(head + '\n'.join(out) + '\n')
    globals()['head'] = head
    for t in totals:
        print('%-18s %4d / %4d' % t)
    write_progress(totals, groups, keys_done, open(path).read().split(MARK)[0])


def write_progress(totals, groups, keys_done, head):
    """progress/*.json for the parity dashboard: the totals, key commands by group, and the
    hand audit's features (section, item, status)."""
    import json
    out = os.path.join(ROOT, 'progress')
    os.makedirs(out, exist_ok=True)
    json.dump([{'area': a, 'done': n, 'total': t, 'percent': round(100 * n / max(t, 1), 1)} for a, n, t in totals], open(os.path.join(out, 'areas.json'), 'w'), indent=1)
    rows = []
    for a, v in groups.items():
        n = sum(1 for c, _ in v if c in keys_done)
        rows.append({'group': a, 'done': n, 'total': len(v), 'percent': round(100 * n / max(len(v), 1), 1)})
    json.dump(rows, open(os.path.join(out, 'key-groups.json'), 'w'), indent=1)
    feats = []
    section = ''
    for line in head.splitlines():
        m = re.match(r'## (\d+)\. (.*)', line)
        if m:
            section = m.group(2).strip()
            continue
        m = re.match(r'- \[(x|~| )\] (.*)', line)
        if m and section:
            status = {'x': 'Done', '~': 'Partial', ' ': 'Not started'}[m.group(1)]
            text = m.group(2).strip()
            feats.append({'area': section, 'status': status, 'feature': text})
    json.dump(feats, open(os.path.join(out, 'features.json'), 'w'), indent=1)

main()
