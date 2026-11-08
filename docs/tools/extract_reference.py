#!/usr/bin/env python3
"""Extract factual M1W matrices without executing vendor JavaScript or opening HID.

Usage: python docs/tools/extract_reference.py /path/to/app/dist/js > reference.json
Offsets are decoded Unicode character positions, not UTF-8 byte offsets.
"""
import hashlib
import json
from pathlib import Path
import re
import sys


def main():
    root = Path(sys.argv[1])
    source = root / "b7c9f14f.js"
    text = source.read_text(encoding="utf-8")
    start = text.index("class Z extends s{")
    end = text.index("}class ", start)
    matrices = {}
    for name in ("defaultMatrix", "defaultFnMatrix", "defaultFnMACMatrix"):
        match = re.search(r"\b" + name + r"=(\[[\d,]+\])", text[start:end])
        if match is None:
            raise ValueError(f"missing {name} in concrete M1W class")
        values = json.loads(match[1])
        if len(values) != 512 or any(not 0 <= n <= 255 for n in values):
            raise ValueError(f"unexpected matrix shape: {name}")
        matrices[name] = {
            "source_character_offset": start + match.start(),
            "slots": [values[n:n + 4] for n in range(0, 512, 4)],
        }
    result = {
        "evidence": "official host defaults, not firmware readback or restore data",
        "device_id": 983,
        "source": source.name,
        "source_sha256": hashlib.sha256(source.read_bytes()).hexdigest(),
        "class_character_offset": start,
        "warning": "defaultFnMACMatrix capitalization differs from base property; do not assume it is selected at runtime",
        "matrices": matrices,
    }
    json.dump(result, sys.stdout, indent=2)
    print()


if __name__ == "__main__":
    main()
