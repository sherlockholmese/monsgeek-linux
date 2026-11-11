#!/usr/bin/env python3
"""Offline, hash-pinned analysis of original M1W ID983 v108.

Accepts the vendor raw-deflate download or decompressed firmwareFile.bin.
Never opens HID, invokes a flasher, or modifies the input. Optional Capstone
disassembly and Unicorn verification execute only in local analysis/emulation.
"""

import argparse
import hashlib
import json
from pathlib import Path
import re
import struct
import zlib


IMAGE_SHA256 = "7c8f0287d2d4dc916f88e68ad59e56544187faa8c5c008441a068d1922aa26c7"
DOWNLOAD_SHA256 = "99e93a620c60745bff1e01d69a75055d2e25a3cfcfc87561db7d3c0edfb4b4a6"
BASE = 0x01000200
PREFIX = 65536
MAX_FILE_BYTES = 1024 * 1024


def sha256(data):
    return hashlib.sha256(data).hexdigest()


def load_image(path):
    with path.open("rb") as source:
        data = source.read(MAX_FILE_BYTES + 1)
    if len(data) > MAX_FILE_BYTES:
        raise ValueError("input exceeds the limit for this specific image")
    input_hash = sha256(data)
    if input_hash == DOWNLOAD_SHA256:
        decoder = zlib.decompressobj(-15)
        data = decoder.decompress(data, MAX_FILE_BYTES + 1)
        if not decoder.eof or decoder.unused_data or len(data) > MAX_FILE_BYTES:
            raise ValueError("invalid or oversized raw-deflate container")
    if sha256(data) != IMAGE_SHA256:
        raise ValueError("not the researched ID983 v108 image; offsets cannot be reused")
    return data, input_hash


def decompress_scatter(image, source, size, boot=False):
    """Translate the image's two startup decompressors, not vendor JS.

    Application: 0x0102a26e. Boot: 0x01002cf0. Both use literal counts
    stored as count+1. Match lengths are count+2. Boot additionally has
    zero runs. Overlapping backreferences intentionally copy byte by byte.
    """
    pos = source - BASE
    out = bytearray()

    def take():
        nonlocal pos
        if pos >= len(image):
            raise ValueError("truncated scatter data")
        value = image[pos]
        pos += 1
        return value

    while len(out) < size:
        token = take()
        literals = token & (7 if boot else 3)
        if literals == 0:
            literals = take()
        count = token >> 4
        if count == 0:
            count = take()
        if literals == 0:
            raise ValueError("invalid literal count")
        for _ in range(literals - 1):
            out.append(take())
        if boot and not token & 8:
            out.extend(bytes(count))
        elif count or boot:
            distance = take()
            if not boot:
                distance += take() << 8 if token & 12 == 12 else (token & 12) << 6
            if not 0 < distance <= len(out):
                raise ValueError("invalid scatter backreference")
            for _ in range(count + 2):
                out.append(out[-distance])
        if len(out) > size:
            raise ValueError("scatter output exceeds declared size")
    return bytes(out), pos


def device_descriptors(ram, address=0x20000):
    result = []
    for match in re.finditer(b"\x12\x01", ram):
        offset = match.start()
        data = ram[offset:offset + 18]
        if len(data) != 18 or data[7] not in (8, 16, 32, 64):
            continue
        usb, vid, pid, version = (struct.unpack_from("<H", data, n)[0] for n in (2, 8, 10, 12))
        if not 0x100 <= usb <= 0x210:
            continue
        result.append({
            "ram_address": hex(address + offset),
            "vid": f"0x{vid:04x}", "pid": f"0x{pid:04x}",
            "device_bcd": f"0x{version:04x}", "raw": data.hex(" "),
        })
    return result


def all_offsets(data, needle):
    return [match.start() for match in re.finditer(re.escape(needle), data)]


def verify_emulation(image, expected, source, size, routine):
    from unicorn import Uc, UC_ARCH_ARM, UC_MODE_THUMB
    from unicorn.arm_const import (
        UC_ARM_REG_R0, UC_ARM_REG_R1, UC_ARM_REG_R2,
        UC_ARM_REG_SP, UC_ARM_REG_LR, UC_ARM_REG_PC,
    )

    machine = Uc(UC_ARCH_ARM, UC_MODE_THUMB)
    machine.mem_map(0x01000000, 0x80000)
    machine.mem_write(BASE, image)
    machine.mem_map(0x20000, 0x10000)
    for register, value in (
        (UC_ARM_REG_R0, source), (UC_ARM_REG_R1, 0x20000),
        (UC_ARM_REG_R2, size), (UC_ARM_REG_SP, 0x2f000),
        (UC_ARM_REG_LR, 0x01040001),
    ):
        machine.reg_write(register, value)
    machine.emu_start(routine | 1, 0x01040000, count=100000)
    if machine.reg_read(UC_ARM_REG_PC) != 0x01040000:
        raise ValueError("emulated decompressor did not return within instruction limit")
    if bytes(machine.mem_read(0x20000, size)) != expected:
        raise ValueError("Python decoder differs from original ARM routine")


def disassemble(image, destination):
    from capstone import Cs, CS_ARCH_ARM, CS_MODE_THUMB
    from capstone.arm import ARM_INS_LDR, ARM_OP_MEM, ARM_REG_PC

    decoder = Cs(CS_ARCH_ARM, CS_MODE_THUMB)
    decoder.detail = True
    # Deliberately named regions; data pools still decode as instructions.
    regions = {
        "boot": (0x200, 0x330c), "startup": (0x10200, 0x104a8),
        "vendor-handlers": (0x21ee0, 0x22e1c),
        "scatter-decoders": (0x2a008, 0x2a344),
    }
    decoder.skipdata = True
    with destination.open("w", encoding="utf-8") as output:
        output.write("Linear Thumb analysis; embedded data is NOT proven executable code.\n")
        for name, (start, end) in regions.items():
            output.write(f"\n[{name}]\n")
            for instruction in decoder.disasm(image[start:end], BASE + start):
                annotation = ""
                if (instruction.id == ARM_INS_LDR and len(instruction.operands) == 2
                        and instruction.operands[1].type == ARM_OP_MEM
                        and instruction.operands[1].mem.base == ARM_REG_PC):
                    offset = ((instruction.address + 4) & ~3) + instruction.operands[1].mem.disp - BASE
                    if 0 <= offset <= len(image) - 4:
                        value = struct.unpack_from("<I", image, offset)[0]
                        annotation = f" ; literal {value:#010x} [file {offset:#x}]"
                output.write(f"{instruction.address:08x}: {instruction.bytes.hex():10s} "
                             f"{instruction.mnemonic:8s} {instruction.op_str}{annotation}\n")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("image", type=Path)
    parser.add_argument("--out", type=Path, required=True, help="new output directory")
    parser.add_argument("--reference", type=Path,
                        default=Path(__file__).resolve().parents[1] / "m1w-reference.json")
    parser.add_argument("--disassemble", action="store_true", help="requires Capstone")
    parser.add_argument("--verify-emulation", action="store_true", help="requires Unicorn")
    args = parser.parse_args()
    image, input_hash = load_image(args.image)
    app_ram, app_end = decompress_scatter(image, 0x01033900, 0x73c)
    boot_ram, boot_end = decompress_scatter(image, 0x0100350c, 0x130, boot=True)
    reference = json.loads(args.reference.read_text())
    matrices = {}
    for name, matrix in reference["matrices"].items():
        data = bytes(value for slot in matrix["slots"] for value in slot)
        matches = all_offsets(image, data)
        matrices[name] = {
            "bytes": len(data), "sha256": sha256(data),
            "full_matches_file_offsets": [hex(offset) for offset in matches],
            "full_matches_mapped_addresses": [hex(BASE + offset) for offset in matches],
        }
    if args.verify_emulation:
        verify_emulation(image, app_ram, 0x01033900, 0x73c, 0x0102a26e)
        verify_emulation(image, boot_ram, 0x0100350c, 0x130, 0x01002cf0)
    manifest = {
        "evidence": "offline static analysis of vendor download; no hardware flash or firmware dump",
        "device_id": 983, "vendor_version": "v108",
        "metadata_url": "https://api2.rongyuan.tech:3816/api/v2/get_fw_version",
        "metadata_request": {"dev_id": 983},
        "download_url": "https://api2.rongyuan.tech:3816/download/fw_upgrade_file/983_v108",
        "download_bytes": 86253, "download_sha256": DOWNLOAD_SHA256,
        "input_sha256": input_hash,
        "image_bytes": len(image), "image_sha256": sha256(image),
        "application_file_offset": hex(PREFIX),
        "application_bytes": len(image) - PREFIX,
        "application_sha256": sha256(image[PREFIX:]),
        "application_blocks": (len(image) - PREFIX + 63) // 64,
        "application_byte_sum": sum(image[PREFIX:]),
        "analysis_mapping_base": hex(BASE), "application_write_base_literal": "0x01010200",
        "application_reset": "0x01010400", "application_main": "0x010104fc",
        "architecture": "ARM Thumb; YiChip YC3121-compatible peripheral map; exact chip marking unverified",
        "scatter": {
            "application": {"source": "0x01033900", "ram": "0x20000", "bytes": len(app_ram),
                            "source_end_file_offset": hex(app_end), "sha256": sha256(app_ram)},
            "boot": {"source": "0x0100350c", "ram": "0x20000", "bytes": len(boot_ram),
                     "source_end_file_offset": hex(boot_end), "sha256": sha256(boot_ram)},
            "original_arm_routines_match": True if args.verify_emulation else None,
        },
        "application_usb_descriptors": device_descriptors(app_ram),
        "bundled_boot_usb_descriptors": device_descriptors(boot_ram),
        "vendor_feature_descriptor": {
            "ram_address": "0x20660", "usage_page": "0xffff", "usage": 2,
            "report_bytes": 64, "type": "feature", "report_id": 0,
        },
        "host_default_matrix_matches": matrices,
        "limits": ["not a readout of the owner's flash", "not a tested recovery image",
                   "no QMK/VIA replacement implemented", "GPIO matrix pins and recovery unverified"],
    }
    # Refuse to overwrite a prior analysis or the input directory.
    args.out.mkdir(parents=True, exist_ok=False)
    (args.out / "application.bin").write_bytes(image[PREFIX:])
    (args.out / "ram-initialized.bin").write_bytes(app_ram)
    (args.out / "boot-ram-initialized.bin").write_bytes(boot_ram)
    (args.out / "analysis.json").write_text(json.dumps(manifest, indent=2) + "\n")
    if args.disassemble:
        disassemble(image, args.out / "thumb-analysis.txt")
    print(json.dumps(manifest, indent=2))


if __name__ == "__main__":
    main()
