#!/usr/bin/env python3
"""Extract numeric model facts without executing proprietary JavaScript.

Usage: extract_models.py /path/to/app/dist/js > docs/models.json
Only MonsGeek catalog entries and their concrete matrices are included.
"""
import ast
import hashlib
import json
from pathlib import Path
import re
import sys


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
    tokens = re.findall(r'"(?:\\.|[^"\\])*"|\'(?:\\.|[^\'\\])*\'|\w+|[^\w\s]', text)
    result = []
    for i, token in enumerate(tokens):
        if token.startswith("\'"):
            token = json.dumps(ast.literal_eval(token))
        if re.fullmatch(r'\w+', token) and i + 1 < len(tokens) and tokens[i + 1] == ':':
            token = json.dumps(token)
        result.append(token)
    return json.loads(''.join(result).replace('!0', 'true').replace('!1', 'false'))


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
        palette = [16711680, 16744448, 16776960, 65280, 65535, 255, 16711935]
        for b in reversed(chain):
            if m := re.search(r'\bDAZZLE=(\d+);NORMAL=(\d+)', b):
                dazzle, normal = map(int, m.groups())
            if colors := re.search(r'COMMONCOLOR=(\{[\d:,]+\})', b):
                values = literal(colors[1])
                palette = [values[str(i)] for i in range(7)]
        key_layout = re.search(r'keyLayout:h\.(\w+)', entry)[1]
        model = dict(id=device_id, name=string('displayName'), driver=name,
                     vid=number('vid'), pid=number('pid'), layers=number('layer'),
                     family='yc500' if legacy else 'ry5088',
                     magnetic='magnetism:!0' in entry, sleep='sleepBT:' in entry,
                     debounce='deBounce:' in entry,
                     dazzle=dazzle, normal=normal, key_layout=key_layout,
                     palette=[[(color >> 16) & 255, (color >> 8) & 255, color & 255] for color in palette],
                     source=file, source_class=cls, matrices=matrices)
        model['knob'] = json.loads(re.search(r'knobKeyCodes:(\[[^\]]+\])', entry)[1]) if 'knobKeyCodes:' in entry else []
        model['fixed_inputs'] = json.loads(re.search(r'nonModifiableKnobKeyCodes:(\[[^\]]+\])', entry)[1]) if 'nonModifiableKnobKeyCodes:' in entry else []
        model['side_light'] = 'sideLightLayout:' in entry
        usb = re.search(r'type:"keyboard",vendorId:' + str(model['vid']) + r',productId:' + str(model['pid']) + r',usage:2,usagePage:65535,interfaceNumber:2([^}]*)', index)
        model['max_report_rate'] = 8000 if usb and 'reportRate:8e3' in usb[1] else 1000
        model['firmware_prefix'] = 65536 if legacy else {'p': 65536, 't': 20480, 'x': 65536, 'S': 65536}[current]
        model['firmware_wide'] = not legacy and current == 'x'
        fn = re.search(r'fnSysLayer:(\{[^}]+\})', entry)
        model['fn_counts'] = literal(fn[1]) if fn else {'win': 1}
        model['replaceable_switches'] = 'isSwitchReplaceable:!0' in entry
        switches = json.loads(re.search(r'supportedSwitchTypes:(\[[^\]]+\])', entry)[1]) if 'supportedSwitchTypes:' in entry else (["高特", "磁玉", "磁玉pro", "磁玉gaming", "天王", "万磁王"] if model['replaceable_switches'] else [])
        model['switches'] = []
        for switch in switches:
            code = int(re.search(r'e\[e\.' + re.escape(switch) + r'=(\d+)\]', index)[1])
            label = re.search(re.escape(switch) + r':\{[^}]*\ben:"([^"]+)"', index)
            model['switches'].append({'code': code, 'label': label[1] if label else switch})
        model['travel_max'] = 4.0 if legacy else 3.3
        if travel := re.search(r'travelSetting:\{travel:(\{[^}]+\})', entry):
            if maximum := re.search(r'max:([\d.]+)', travel[1]):
                model['travel_max'] = float(maximum[1])
        model['magnetic_ranges'] = {}
        travel_settings = None
        if match := re.search(r'travelSetting:\{', entry):
            travel_settings = scope(entry, entry.index('{', match.start()))
        for parameter, group, defaults in [(0, 'travel', [.1, model['travel_max'], 0]), (1, 'travel', [.1, model['travel_max'], 0]), (2, 'firePress', [0, 0, 0]), (3, 'fireLift', [0, 0, 0]), (6, 'deadzone', [0, 0, 0])]:
            bounds = defaults[:]
            if travel_settings and (match := re.search(r'\b' + group + r':(\{[^}]+\})', travel_settings)):
                for i, key in enumerate(['min', 'max', 'step']):
                    if value := re.search(r'\b' + key + r':([\d.]+)', match[1]):
                        bounds[i] = float(value[1])
            model['magnetic_ranges'][str(parameter)] = bounds
        model['sleep_ranges'] = []
        for kind in ['sleep', 'deep']:
            for radio in ['sleepBT', 'sleep24']:
                bounds = None
                if match := re.search(radio + r':\{', entry):
                    radio_settings = scope(entry, entry.index('{', match.start()))
                    if timer := re.search(r'\b' + kind + r':(\{[^}]+\})', radio_settings):
                        def seconds(key, default):
                            value = re.search(r'\b' + key + r':([\d*]+)', timer[1])
                            result = 1
                            for part in value[1].split('*') if value else [str(default)]:
                                result *= int(part)
                            return result
                        bounds = [seconds('min', 0), seconds('max', 64800)]
                model['sleep_ranges'].append(bounds)
        lights = re.search(r'\blightLayout:(\w+)', entry)[1]
        light_start = re.search(r'\b' + lights + r'=\{isRgb:', index)
        model['lighting'] = literal(scope(index, index.index('{', light_start.start())))
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
        if layout is None:
            names = {'Common108_MG108B': '108_MG108B',
                     'Common87_MonsGeek_M3V5': '87_MonsGeek_M3V5',
                     'Common68_MK902_2': '68_MK902',
                     'Common83_MG75S': '83_MG75S',
                     'Common98_MonsGeek_M2V5': '98_MonsGeek_M2V5',
                     'Common83_SG9015': '83_SG9015'}
            template = 'Keyboard_' + names[key_layout] + '_KeyMappings'
            for mapping_file, text in sources.items():
                m = re.search(template + r':\{type:"svg",str:await \w+\(\(\)=>import\("\./(\w+\.js)"\)', text)
                if not m:
                    continue
                svg_file = m[1]
                svg = sources[svg_file].split('`', 1)[1].rsplit('`', 1)[0].replace('\\`', '`')
                svg = svg.split('id="hover"')[0]
                caps = {}
                for match in re.finditer(r'<g id="#([^"]+)">((?:(?!<g id="#).)*?)<rect ([^>]+)>', svg, re.S):
                    code, _, attrs = match.groups()
                    rect = dict(re.findall(r'(\w+)="([^"]+)"', attrs))
                    caps[code] = {k: float(rect.get(k, '0')) for k in ('x', 'y', 'width', 'height')}
                    caps[code]['type'] = 'key'
                width, height = re.search(r'<svg width="([\d.]+)" height="([\d.]+)"', svg).groups()
                layout = dict(width=float(width), height=float(height), layout=caps)
                hashes[svg_file] = hashlib.sha256((root / svg_file).read_bytes()).hexdigest()
                break
            assert layout is not None, key_layout
        model['geometry'] = layout
        # Preserve literal capability data for reproducible follow-up research.
        model['catalog_entry'] = entry
        hashes[file] = hashlib.sha256((root / file).read_bytes()).hexdigest()
        models.append(model)
    hashes['index.1970d80c.js'] = hashlib.sha256((root / 'index.1970d80c.js').read_bytes()).hexdigest()
    receiver_table = scope(index, index.index('[', index.index('zp=[')), '[', ']')
    vendors = {m['vid'] for m in models}
    receivers = [[int(v), int(p)] for v, p in re.findall(r'type:"dongle",vendorId:(\d+),productId:(\d+)', receiver_table) if int(v) in vendors]
    print('{"evidence":"Official host defaults and UI geometry; not owner backups or hardware validation",')
    print('"receivers":' + json.dumps(receivers) + ',')
    print('"source_sha256":' + json.dumps(hashes, sort_keys=True) + ',"models":[')
    print(',\n'.join(json.dumps(m, separators=(',', ':')) for m in sorted(models, key=lambda m: m['id'])))
    print(']}')


if __name__ == '__main__':
    main()
