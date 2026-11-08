#!/usr/bin/env python3
"""Explicitly opted-in CLI round trips on a connected original M1W.

Never run alongside another configurator. Saves every command, before-state and
full snapshot. Stops on failure without automatically retrying or rolling back.
Physical event behavior and persistence require separate owner observations.
"""
import argparse
import copy
import json
from pathlib import Path
import subprocess
import time


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--allow-setting-changes", action="store_true", required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--binary", default="./target/release/monsgeek-linux")
    parser.add_argument("--case", action="append", help="Run named cases only")
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=False, mode=0o700)
    records = []

    def run(command, expect_failure=False):
        start = time.monotonic()
        p = subprocess.run([args.binary, *command], capture_output=True, text=True)
        records.append(dict(command=command, code=p.returncode, stdout=p.stdout,
                            stderr=p.stderr, seconds=round(time.monotonic()-start, 3)))
        (args.output / "commands.json").write_text(json.dumps(records, indent=2))
        if expect_failure:
            assert p.returncode, f"unexpectedly accepted {command}"
        elif p.returncode:
            raise RuntimeError(f"command failed; inspect {args.output}/commands.json: {p.stderr}")
        return p.stdout

    def snapshot(name):
        path = args.output / f"{name}.json"
        run(["snapshot", str(path)])
        return json.loads(path.read_text())

    baseline = snapshot("baseline")
    onboard = baseline["onboard"]
    macro = {"repeat": 3, "events": [
        {"kind": "keyboard", "usage": 4, "down": True, "delay_ms": 37},
        {"kind": "keyboard", "usage": 4, "down": False, "delay_ms": 139},
    ]}
    macro_file = args.output / "macro-input.json"
    macro_file.write_text(json.dumps(macro))
    macro_raw = [3, 0, 4, 165, 4, 0, 139, 0] + [0]*248
    empty_slot = next((i for i in reversed(range(50)) if not any(onboard["macros"][i])), None)
    if empty_slot is None:
        raise RuntimeError("no empty macro slot available for validation")
    colors = copy.deepcopy(onboard["rgb"])
    colors[9:12] = [17, 83, 149]
    colors[27:30] = [211, 7, 41]
    rgb_file = args.output / "rgb-input.json"
    rgb_file.write_text(json.dumps([colors[i:i+3] for i in range(0, 384, 3)]))
    extended = {"repeat": 1, "events": [
        {"kind":"move","dx":-7,"dy":11,"delay_ms":0},
        {"kind":"keyboard","usage":6,"down":True,"delay_ms":127},
        {"kind":"keyboard","usage":6,"down":False,"delay_ms":128},
        {"kind":"mouse-button","button":244,"down":True,"delay_ms":1},
        {"kind":"mouse-button","button":244,"down":False,"delay_ms":65535},
    ]}
    extended_file = args.output / "macro-extended.json"
    extended_file.write_text(json.dumps(extended))
    extended_raw = [1,0,249,0,249,11,0,0,6,255,6,0,128,0,244,129,244,0,255,255] + [0]*236
    full_raw = [1,0] + [6,129,6,1]*62 + [249,0,1,255,0,0]
    gap_raw = [1,0,6,129,6,1] + [0]*194 + [7] + [0]*55
    full_file = args.output / "macro-full-plan.json"
    gap_file = args.output / "macro-gap-plan.json"
    full_file.write_text(json.dumps([dict(kind="macro",slot=empty_slot,data=full_raw)]))
    gap_file.write_text(json.dumps([dict(kind="macro",slot=empty_slot,data=gap_raw)]))
    effect_changes = []
    for mode in range(20):
        flags = 0 if mode == 13 else 8
        color = [0,200,200] if mode == 13 else [17,83,149]
        effect_changes.append(dict(kind="lighting",settings=dict(mode=mode,wire_speed=2,brightness=3,flags=flags,rgb=color)))
    # Other picture selections expose a different RGB map. Test their storage
    # separately, rather than misclassifying that expected read change as damage.
    effects_file = args.output / "all-effects.json"
    effects_file.write_text(json.dumps(effect_changes))
    restore_file = args.output / "restore-input-plan.json"
    restore_file.write_text(json.dumps([dict(kind="debounce",value=4),dict(kind="rgb",data=colors)]))

    # Expected state is specified independently of the program's emitted packets.
    cases = [
        ("light", ["light", "--mode", "1", "--speed", "4", "--brightness", "3", "--color", "115395"],
         lambda s: s.update(lighting=dict(mode=1, wire_speed=1, brightness=3, flags=8, rgb=[17,83,149]))),
        ("debounce", ["debounce", "3"], lambda s: s["onboard"].update(debounce=3)),
        ("sleep", ["sleep", "--bt", "181", "--receiver", "239", "--deep-bt", "901", "--deep-receiver", "1201"],
         lambda s: s["onboard"].update(sleep=dict(time_bt=181,time_24=239,deep_time_bt=901,deep_time_24=1201))),
        ("profile", ["profile", "1"], lambda s: s.update(active_layer=1)),
        ("remap", ["remap", "--from", "capslock", "--to", "101"],
         lambda s: s["keymaps"][0].__setitem__(slice(12,16), [0,0,101,0])),
        ("fn", ["remap", "--from", "capslock", "--to", "c", "--fn-layer"],
         lambda s: s["fn_keymaps"][0].__setitem__(slice(12,16), [0,0,6,0])),
        ("combo", ["bind", "--from", "capslock", "--binding", '{"kind":"keyboard","keys":[224,225,41]}'],
         lambda s: s["keymaps"][0].__setitem__(slice(12,16), [0,224,225,41])),
        ("knob", ["bind", "--from", "knob-press", "--binding", '{"kind":"consumer","usage":226}'],
         lambda s: s["keymaps"][0].__setitem__(slice(24,28), [3,0,226,0])),
        ("mouse", ["bind", "--from", "capslock", "--binding", '{"kind":"mouse","button":244}'],
         lambda s: s["keymaps"][0].__setitem__(slice(12,16), [1,0,244,0])),
        ("disabled", ["bind", "--from", "capslock", "--binding", '{"kind":"disabled"}'],
         lambda s: s["keymaps"][0].__setitem__(slice(12,16), [0,0,0,0])),
        ("macro", ["macro", str(empty_slot), "--file", str(macro_file)],
         lambda s: s["onboard"]["macros"].__setitem__(empty_slot, macro_raw)),
        ("rgb", ["rgb", "--file", str(rgb_file)], lambda s: s["onboard"].update(rgb=colors)),
        ("win-lock", ["option", "--snapshot", str(args.output/"baseline.json"), "--name", "win-lock", "--enabled", "true"],
         lambda s: s["onboard"]["options"].__setitem__(0,onboard["options"][0]|1)),
        ("auto-os", ["auto-os", "true"], lambda s: s["onboard"].update(auto_os=True)),
        ("effects", ["plan", str(effects_file)], lambda s: s.update(lighting=dict(mode=19,wire_speed=2,brightness=3,flags=8,rgb=[17,83,149]))),
        ("macro-extended", ["macro", str(empty_slot), "--file", str(extended_file)], lambda s: s["onboard"]["macros"].__setitem__(empty_slot,extended_raw)),
        ("macro-full", ["plan", str(full_file)], lambda s: s["onboard"]["macros"].__setitem__(empty_slot,full_raw)),
        ("macro-gap", ["plan", str(gap_file)], lambda s: s["onboard"]["macros"].__setitem__(empty_slot,gap_raw)),
        ("macro-bind", ["bind","--from","capslock","--binding",json.dumps(dict(kind="macro",slot=empty_slot,mode=2))], lambda s: s["keymaps"][0].__setitem__(slice(12,16),[9,2,empty_slot,0])),
        ("pointer", ["bind","--from","capslock","--binding",'{"kind":"pointer","action":"scroll-down"}'], lambda s: s["keymaps"][0].__setitem__(slice(12,16),[1,0,245,255])),
        ("layer3", ["remap","--from","capslock","--to","101","--layer","3"], lambda s: s["keymaps"][3].__setitem__(slice(12,16),[0,0,101,0])),
        ("fn-layer2", ["remap","--from","capslock","--to","c","--layer","2","--fn-layer"], lambda s: s["fn_keymaps"][2].__setitem__(slice(12,16),[0,0,6,0])),
        ("restore", ["plan",str(restore_file)], lambda s: s["onboard"].update(debounce=4,rgb=colors)),
    ]
    names = {name for name, _, _ in cases} | {"pictures", "macro-pictures"}
    if args.case and not set(args.case) <= names:
        raise RuntimeError(f"unknown case, choices: {sorted(names)}")
    for name, command, edit in cases:
        if args.case and name not in args.case:
            continue
        print(f"{name}: preview, apply, full comparison, undo, full comparison", flush=True)
        expected = copy.deepcopy(baseline)
        edit(expected)
        assert expected != baseline, f"{name}: case must use a distinct value"
        run(command)
        before = args.output / f"{name}-before.json"
        try:
            run([*command, "--apply", "--allow-experimental-writes", "--before", str(before)])
        except RuntimeError:
            snapshot(f"{name}-failed")
            raise
        changed = snapshot(f"{name}-changed")
        if changed != expected:
            fields = [k for k in changed if changed[k] != expected[k]]
            raise RuntimeError(f"{name}: unexpected collateral state in {fields}; stopped, before-state retained")
        inverse = ["restore", str(args.output/"baseline.json")] if name == "restore" else ["undo", str(before)]
        run([*inverse, "--apply", "--allow-experimental-writes", "--before", str(args.output/f"{name}-undo-before.json")])
        restored = snapshot(f"{name}-restored")
        assert restored == baseline, f"{name}: restoration mismatch; stopped"
        print(f"{name}: PASS, exact full-state restoration", flush=True)
    if not args.case or "pictures" in args.case:
        for index in [1, 2]:
            name = f"picture{index}"
            print(f"{name}: select, edit, switch away/back, undo, full comparison", flush=True)
            select = ["light", "--mode", "13", "--option", str(index)]
            select_before = args.output/f"{name}-select-before.json"
            run([*select, "--apply", "--allow-experimental-writes", "--before", str(select_before)])
            original = snapshot(f"{name}-original")
            expected = copy.deepcopy(original)
            expected["onboard"]["rgb"][9:12] = [17,83,149]
            assert original != expected, "picture edit must be distinguishable"
            paint_before = args.output/f"{name}-paint-before.json"
            run(["paint", "--snapshot", str(args.output/f"{name}-original.json"),
                 "--key", "capslock", "--color", "115395", "--apply",
                 "--allow-experimental-writes", "--before", str(paint_before)])
            assert snapshot(f"{name}-modified") == expected, "picture edit changed other fields"
            mixed = args.output/f"{name}-mixed.json"
            mixed.write_text(json.dumps([dict(kind="rgb",data=expected["onboard"]["rgb"]),
                dict(kind="lighting",settings=baseline["lighting"])]))
            rejected = args.output/f"{name}-rejected-before.json"
            run(["plan",str(mixed),"--apply","--allow-experimental-writes","--before",str(rejected)], expect_failure=True)
            assert not rejected.exists(), "mixed-picture guard ran after backup/write"
            run(["undo",str(select_before),"--apply","--allow-experimental-writes",
                 "--before",str(args.output/f"{name}-away-before.json")])
            assert snapshot(f"{name}-away") == baseline, "editing another picture changed picture0"
            run(["undo",str(paint_before),"--apply","--allow-experimental-writes",
                 "--before",str(rejected)], expect_failure=True)
            assert not rejected.exists(), "wrong-picture undo was not rejected early"
            back_before = args.output/f"{name}-back-before.json"
            run([*select,"--apply","--allow-experimental-writes","--before",str(back_before)])
            assert snapshot(f"{name}-back") == expected, "picture edit did not survive mode switch"
            run(["undo",str(paint_before),"--apply","--allow-experimental-writes",
                 "--before",str(args.output/f"{name}-undo-paint-before.json")])
            assert snapshot(f"{name}-restored") == original, "picture bytes were not restored"
            run(["undo",str(back_before),"--apply","--allow-experimental-writes",
                 "--before",str(args.output/f"{name}-undo-select-before.json")])
            assert snapshot(f"{name}-final") == baseline, "full baseline not restored"
            print(f"{name}: PASS, independent edit and exact restoration", flush=True)
    if not args.case or "macro-pictures" in args.case:
        pictures = {0: baseline}
        flags = ["--apply", "--allow-experimental-writes", "--before"]
        for index in [1, 2]:
            before = args.output/f"macro-pic{index}-capture-before.json"
            run(["light","--mode","13","--option",str(index),*flags,str(before)])
            pictures[index] = snapshot(f"macro-pic{index}-original")
            run(["undo",str(before),*flags,str(args.output/f"macro-pic{index}-capture-undo.json")])
        for index in [1, 2]:
            print(f"macro with picture{index}: write, clear, compare all three pictures", flush=True)
            before = args.output/f"macro-pic{index}-select-before.json"
            run(["light","--mode","13","--option",str(index),*flags,str(before)])
            macro_before = args.output/f"macro-pic{index}-write-before.json"
            run(["macro",str(empty_slot),"--file",str(macro_file),*flags,str(macro_before)])
            expected = copy.deepcopy(pictures[index])
            expected["onboard"]["macros"][empty_slot] = macro_raw
            assert snapshot(f"macro-pic{index}-written") == expected
            run(["undo",str(macro_before),*flags,str(args.output/f"macro-pic{index}-clear-before.json")])
            assert snapshot(f"macro-pic{index}-cleared") == pictures[index]
            run(["undo",str(before),*flags,str(args.output/f"macro-pic{index}-undo-select.json")])
            assert snapshot(f"macro-pic{index}-base-restored") == baseline
            for other in [1, 2]:
                check = args.output/f"macro-pic{index}-check{other}-before.json"
                run(["light","--mode","13","--option",str(other),*flags,str(check)])
                assert snapshot(f"macro-pic{index}-check{other}") == pictures[other]
                run(["undo",str(check),*flags,str(args.output/f"macro-pic{index}-check{other}-undo.json")])
            assert snapshot(f"macro-pic{index}-final") == baseline
            print(f"macro with picture{index}: PASS, all three pictures restored", flush=True)
    print(f"Evidence retained at {args.output}", flush=True)


if __name__ == "__main__":
    main()
