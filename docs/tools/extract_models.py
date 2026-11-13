#!/usr/bin/env python3
"""Extract numeric model facts without executing proprietary JavaScript.

Usage: extract_models.py /path/to/app/dist/js > docs/models.json
Only MonsGeek catalog entries and their concrete matrices are included.
"""
import hashlib
import json
from pathlib import Path
import re
import sys
import xml.etree.ElementTree as ET


def scope(text, start, opening="{", closing="}"):
    depth, quote, escape = 0, None, False
    for i in range(start, len(text)):
        c = text[i]
        if quote:
            if escape:
                escape = False
            elif c == "\\":
                escape = True
            elif c == quote:
                quote = None
        elif c in ('"', "'", '`'):
            quote = c
        elif c == opening:
            depth += 1
        elif c == closing:
            depth -= 1
            if depth == 0:
                return text[start:i + 1]
    raise ValueError("unclosed source scope")


def literal(text):
    # Quote identifier keys outside string literals. No eval/JS runtime.
    tokens = re.findall(r'"(?:\\.|[^"\\])*"|\w+|[^\w\s]', text)
    result = []
    for i, token in enumerate(tokens):
        if re.fullmatch(r'\w+', token) and i + 1 < len(tokens) and tokens[i + 1] == ':':
            token = json.dumps(token)
        result.append(token)
    return json.loads(''.join(result))


def main():
    root = Path(sys.argv[1])
    index = (root / 'index.1970d80c.js').read_text()
    sources = {p.name: p.read_text() for p in root.glob('*.js')}
    hashes = {}
    models = []
    entries = []
    for m in re.finditer(r'\{id:\d+,vid:', index):
        entry = scope(index, m.start())
        if 'company:"MonsGeek"' in entry and 'type:"keyboard"' in entry:
            entries.append(entry)
    seen = set()
    for entry in entries:
        number = lambda key: int(re.search(r'\b' + key + r':(\d+)', entry)[1])
        string = lambda key: re.search(r'\b' + key + r':"([^"]+)"', entry)[1]
        device_id = number('id')
        if device_id in seen:
            continue
        seen.add(device_id)
        name = string('name')
        found = [(file, re.search(r'case"' + name + r'":return new (\w+)\(', src))
                 for file, src in sources.items() if 'case"' + name + '"' in src]
        found = [(file, m) for file, m in found if m]
        if len(found) != 1:
            raise ValueError(f'{name}: no unique concrete driver')
        file, dispatch = found[0]
        cls = dispatch[1]
        source = sources[file]
        start = re.search(r'class ' + cls + r' extends \w+\{', source)
        body = scope(source, source.index('{', start.start()))
        matrices = {}
        for key in ('defaultMatrix', 'defaultFnMatrix', 'defaultFnMacMatrix', 'defaultFnMACMatrix'):
            m = re.search(r'\b' + key + r'=(\[[\d,]+\])', body)
            if m:
                values = json.loads(m[1])
                assert len(values) == 512 and all(0 <= v <= 255 for v in values)
                matrices[key] = [values[i:i + 4] for i in range(0, 512, 4)]
        assert 'defaultMatrix' in matrices and 'defaultFnMatrix' in matrices, name
        legacy = file == 'b7c9f14f.js'
        # Track inherited palette overrides in this module.
        chain, current = [], cls
        while (m := re.search(r'class ' + current + r' extends (\w+)\{', source)):
            chain.append(scope(source, source.index('{', m.start())))
            current = m[1]
        dazzle, normal = (8, 7)
        for b in reversed(chain):
            if m := re.search(r'\bDAZZLE=(\d+);NORMAL=(\d+)', b):
                dazzle, normal = map(int, m.groups())
        key_layout = re.search(r'keyLayout:h\.(\w+)', entry)[1]
        model = dict(id=device_id, name=string('displayName'), driver=name,
                     vid=number('vid'), pid=number('pid'), layers=number('layer'),
                     family='yc500' if legacy else 'ry5088',
                     magnetic='magnetism:!0' in entry, sleep='sleepBT:' in entry,
                     debounce='deBounce:' in entry,
                     dazzle=dazzle, normal=normal, key_layout=key_layout,
                     source=file, source_class=cls, matrices=matrices)
        model['knob'] = json.loads(re.search(r'knobKeyCodes:(\[[^\]]+\])', entry)[1]) if 'knobKeyCodes:' in entry else []
        model['fixed_inputs'] = json.loads(re.search(r'nonModifiableKnobKeyCodes:(\[[^\]]+\])', entry)[1]) if 'nonModifiableKnobKeyCodes:' in entry else []
        model['side_light'] = 'sideLightLayout:' in entry
        model['replaceable_switches'] = 'isSwitchReplaceable:!0' in entry
        # The UI info map has factual rectangles, separate from source matrix order.
        layouts = re.findall(r'\[h\.' + key_layout + r'\]:"([^"]+)"', index)
        layout = None
        if layouts:
            locator = '"./keyboard_ui_info/' + layouts[-1] + '.ts":'
            p = index.find(locator)
            if p >= 0:
                m = re.search(r'import\("\./(\w+\.js)"\)', index[p:p + 300])
                if m:
                    text = sources[m[1]]
                    layout = literal(scope(text, text.index('{')))
                    hashes[m[1]] = hashlib.sha256((root / m[1]).read_bytes()).hexdigest()
        model['geometry'] = layout
        # Preserve literal capability data for reproducible follow-up research.
        model['catalog_entry'] = entry
        hashes[file] = hashlib.sha256((root / file).read_bytes()).hexdigest()
        models.append(model)
    hashes['index.1970d80c.js'] = hashlib.sha256((root / 'index.1970d80c.js').read_bytes()).hexdigest()
    json.dump(dict(evidence='Official host defaults and UI geometry; not owner backups or hardware validation',
                   source_sha256=hashes, models=sorted(models, key=lambda m: m['id'])), sys.stdout, indent=2)
    print()


if __name__ == '__main__':
    main()
