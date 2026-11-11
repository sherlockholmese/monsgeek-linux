# Original M1W firmware and QMK/VIA feasibility

Research date: 2026-11-11. Target: **original mechanical M1W, ID983**, previously
observed as `3151:4015`, revision `0x0108`. This is not the M1W VIA, M1W HE,
M1 V5, or M1W V3. No keyboard was opened, reset, put in boot mode, or flashed
during this research. The binaries are vendor downloads, not a dump of the
owner's current flash.

**A QMK port looks technically plausible, but there is no working port from this
research.** The image contains ARM Thumb code and uses a peripheral map matching
the public YiChip YC3121 SDK. The missing work is a supported hardware platform,
board wiring, USB, persistent storage, and a verified recovery path. ARM code by
itself does not make an MCU compatible with an existing QMK target.

**VIA alone is a separate, potentially smaller project.** A host adapter could
translate the stock protocol for a modified configurator. Native compatibility
with the ordinary VIA app would require firmware changes to implement VIA's
transport and commands. Loading a layout JSON cannot supply those changes.

## Firmware acquisition

The macOS app exposes the updater's API in `index.1970d80c.js`:

- Character offset `1059267`: `getFwVersion` posts `dev_id` to `/get_fw_version`
  (verify the named anchor if inspecting a different build).
- Around `1059529`: a single-component version uses raw DEFLATE; multi-component
  versions use ZIP members such as `firmwareFile.bin`.
- Around `1064700`: firmware metadata base
  `https://api2.rongyuan.tech:3816/api/v2`, download base
  `https://api2.rongyuan.tech:3816/download`.
- Model dispatch and the inherited `upgrade_usb(file, 65536, 10, progress)` are
  established in [SOURCES.md](SOURCES.md) and [FIRMWARE.md](FIRMWARE.md).

The actual response to `{"dev_id":983}` returned `dev_id:983`, `version_str:v108`,
`file_path:fw_upgrade_file/983_v108`, company `akko`, minimum app version `2.0.5`.
Its description concerns a wired-mode wake problem after host shutdown. This
identifies the package returned for this model; it is not a signed authenticity
check or proof of compatibility with every physical PCB revision.

Local acquisition directory: `.amp/in/firmware/id983/` (Git-ignored).

| Artifact | Bytes | SHA-256 |
|---|---:|---|
| `983_v108.deflate` | 86,253 | `99e93a620c60745bff1e01d69a75055d2e25a3cfcfc87561db7d3c0edfb4b4a6` |
| `firmwareFile.bin` | 282,912 | `7c8f0287d2d4dc916f88e68ad59e56544187faa8c5c008441a068d1922aa26c7` |
| `application.bin`, file offset `0x10000` onward | 217,376 | `0127131261637cdb354f5cf3b13987f0b08846be541ce545ef10e91fa1a165bd` |

Reproduce acquisition and offline analysis from the repository root:

```sh
mkdir -p .amp/in/firmware/id983
curl --fail --show-error --silent \
  -H 'Content-Type: application/json' --data '{"dev_id":983}' \
  https://api2.rongyuan.tech:3816/api/v2/get_fw_version \
  -o .amp/in/firmware/id983/version-response.json
curl --fail --show-error --silent \
  https://api2.rongyuan.tech:3816/download/fw_upgrade_file/983_v108 \
  -o .amp/in/firmware/id983/983_v108.deflate
python3 docs/tools/analyze_firmware.py \
  .amp/in/firmware/id983/983_v108.deflate \
  --out .amp/in/firmware/id983/analysis-new
```

The analysis tool accepts either the pinned compressed file or decompressed
binary, refuses other hashes, writes only to a **new** output directory, and
never accesses HID. It exports the application payload, reconstructed startup
RAM, and JSON findings. Optional `--disassemble` requires Python Capstone;
`--verify-emulation` requires Unicorn. Both are already available in this
research environment. Raw vendor binaries and SDK files remain local rather
than being committed as project source.

## What the firmware contains

All addresses below are **mapped analysis addresses**, not file offsets. For
this package, use `address = file_offset + 0x01000200`. The app payload alone
maps at `0x01010200`. The evidence for the extra `0x200` is the bundled
bootloader's write-base literal and the startup branches and literal references
agreeing under this mapping. Do not load the complete file at `0x01000000` and
silently apply the resulting addresses to patches.

| Address / file position | Evidence |
|---|---|
| File `0..0xffff` | Bundled bootloader, its startup data, and padding; the ordinary USB updater skips this region |
| File `0x10000`, mapped `0x01010200` | Application entry/interrupt trampolines, not a conventional STM32 vector table |
| `0x01010400` | App reset loads SP=`0x30000`, calls scatter initialization, then branches to `0x010104fc` |
| `0x010104fc` | Main initialization and loops selected by a mode byte at RAM `0x20b10`; exact physical switch/mode meanings remain unproved |
| `0x0102a208` | App scatter loader; table at `0x010338e0` |
| `0x0102a26e` | App startup decompressor: reconstructs 1,852 bytes at RAM `0x20000` from flash `0x01033900` |
| `0x01002cf0` | Boot startup decompressor: reconstructs 304 bytes at RAM `0x20000` from flash `0x0100350c` |

The app also zero-initializes `0x2d70` bytes from RAM `0x2073c`. Strings include
`M1W RGB BT`, `M1W RGB BT3.0`, `MonsGeek Keyboard`, and SDK paths ending in
`yc_gpio.c`, `yc_uart.c`, `yc_timer.c`, and `yc_systick.c`.

The [public YC3121 header](https://github.com/RT-Thread/rt-thread/blob/master/bsp/yichip/yc3121-pos/libraries/core/yc3121.h)
defines USB `0xf6000`, SYSCTRL `0xf8500`, SECURE `0xf8540`, GPIO `0xf8700`.
These match firmware register literals/accesses. SP=`0x30000` and RAM starting
at `0x20000` also fit that platform. This is strong evidence of a **YC3121-compatible
platform**, not confirmation of the chip's exact marking, revision, complete
memory size, debug access, or radio architecture. A PCB photograph would resolve
the marking and expose possible debug/recovery pads.

### USB transport recovered from compressed startup data

The Python translations of **both** startup decompressors were independently
checked by emulating the original ARM routines in Unicorn. Their outputs match
byte for byte. Emulation covered these pure decompression routines, not the
keyboard, peripherals, or radio.

| Image | Descriptor location in reconstructed RAM | USB identity |
|---|---|---|
| Application | `0x2051c` | VID `3151`, PID `4015`, device revision `0108` |
| Bundled bootloader | `0x2008c` | VID `3151`, PID `4016`, descriptor revision `0729` |

This establishes `3151:4016` for the **download's bundled bootloader**. It does
not prove which bootloader is installed on the owner's keyboard, because
ordinary updates leave the prefix unsent. The existing host boot-filter table
is therefore insufficient evidence of this board's actual boot PID.

The app's vendor report descriptor at RAM `0x20660` is:

```text
06 ff ff 09 02 a1 01 09 02 15 80 25 7f 95 40 75 08 b1 02 c0
```

It declares usage page `0xffff`, usage `2`, **64-byte feature reports**, with no
numbered report ID. The reconstructed configuration descriptor has three HID
interfaces. This agrees with the project's previously observed interface 2.

### Configuration, keymaps, and firmware-side handlers

The 512-byte macOS `defaultMatrix` matches the firmware exactly at file
`0x331ac` (mapped `0x010333ac`) and at nine other positions, including the base
map region starting at `0x01040100`. Both macOS Fn defaults match full 512-byte
blocks at mapped `0x01044a80` and `0x01044c80`. These matches independently tie
the downloaded binary to the researched host model. Duplicated tables do not
by themselves establish the number of selectable layers or OS profiles.

Configuration dispatch starts at `0x01022e46`, using a report buffer at RAM
`0x21ac8`. The checksum routine `0x01022df2` checks that bytes `0..7` sum to
`0xff` modulo 256, except commands `0x07`/`0x08`, which use bytes `0..8`.
The command switch was also traced in isolated ARM emulation, with called
handlers skipped: this validates dispatch targets without performing hardware
operations.

| Command | Handler | Finding |
|---|---|---|
| `0x8f`, identify | `0x0102270c` | Writes `d7 03 00 00` into reply bytes 1..4: ID983 |
| `0x80`, version | `0x01022740` | Writes `08 01`: revision `0x0108` |
| `0x89`, base-map page read | `0x010224c6` | Source address `0x01040100 + report[1]*512 + report[2]*64` |
| `0x13`, single base binding | `0x010228c2` | Stages layer, slot, and four binding bytes; sets a pending-work flag |
| `0x15`, single Fn binding | `0x01022812` | Separate staged Fn write, also four bytes per binding |
| `0x16`, macro page | `0x0102276e` | Copies 14 words / 56 bytes per page into a staging buffer; final page triggers later work |
| `0x7f`, boot entry | `0x010220e0` | Writes/verifies a boot request at `0x01000120`, then enters the reset path |

These staged handlers explain why a successful feature-report send does not
prove that settings have been saved: other firmware tasks perform the pending
work. They do not resolve the project's observed active-layer write behavior
or macro/RGB interaction; keep the existing preservation/readback workarounds.
The physical matrix GPIOs, diode direction, scan/debounce routine, encoder
wiring, and complete key-event pipeline are not reconstructed yet. The host's
128 binding slots are not a GPIO pin map.

### Firmware updates

In the bundled bootloader, mapped `0x01000818` processes transfer data;
`0x01000876` dispatches `0xc0ba`/`0xc2ba`; `0x010007ac` prepares;
`0x01000752` verifies. The write-base literal at file `0x774` is
`0x01010200`. Data are written in 64-byte chunks and read back for comparison;
completion checks the low 24 bits of the supplied byte sum and the error count.

The downloaded app would require 3,397 blocks, with a 32-byte final block plus
32 zero padding bytes. Its unpadded byte sum is `11162067`. This agrees with
the inherited macOS transfer framing already implemented here. The visible
transfer checks are integrity/readback checks, not a demonstrated cryptographic
model-authentication scheme. No flashing or recovery was tested, and this
package is not a backup of the owner's installed firmware/configuration.

## Practical routes to QMK and VIA

QMK and VIA solve different problems: QMK runs the key-processing firmware;
VIA is a host configurator and its device protocol.

| Route | Work required | What it would deliver |
|---|---|---|
| Host adapter using stock firmware | Translate VIA-style UI operations to the existing Rust protocol; map supported keycodes/layers/macros; modify the configurator's transport | Lowest-risk route to familiar remapping while retaining stock wireless behavior. Does not run QMK or guarantee compatibility with unmodified VIA |
| VIA protocol added to stock firmware | Recover USB descriptor/endpoint handling and add VIA command, storage, and keycode translation; find patch space and a recovery method | Potential native VIA configuration while retaining most vendor firmware. A substantial binary-patching project, not a JSON-only change |
| Full QMK port on existing MCU | YiChip startup/linker, clocks, USB backend, GPIO/timers, nonvolatile storage, boot/recovery; then matrix/encoder/LED drivers | Actual programmable QMK behavior. Start wired; preserving Bluetooth/2.4 GHz requires separate radio/stack work |
| Compatible replacement PCB/controller | Verify mechanical fit and wiring; use a supported MCU | Potentially less engineering than a new MCU platform; features depend on the replacement |

The [QMK MCU documentation](https://docs.qmk.fm/compatible_microcontrollers)
does not establish a ready-made YiChip target for this image. The public
YC3121 BSP is a useful register/startup reference; it is not already a QMK USB
backend. The [official M1 V5 VIA source](https://github.com/MonsGeek/m1_v5/blob/master/us/m1_v5/keyboard.json)
specifies **WB32FQ95**, `wb32-dfu`, and different USB IDs. Its compiled firmware,
pin assignments, and boot assumptions cannot be transferred to ID983.

For native VIA, the firmware must answer VIA's protocol version and dynamic
keymap commands, and expose its required Raw HID transport. The usual QMK Raw
HID collection uses usage page `0xff60`, usage `0x61`, and 32-byte input/output
reports; this differs from the M1W feature-report channel above.
[QMK Raw HID](https://docs.qmk.fm/features/rawhid),
[VIA command definitions](https://github.com/qmk/qmk_firmware/blob/master/quantum/via.h).
VIA uses 16-bit QMK keycodes; the stock board's four-byte tuples cannot express
every QMK action. A translator must reject unsupported actions rather than
pretend that stock firmware gains tap-hold, arbitrary layer logic, or QMK combos.

The [VIA keyboard definition](https://www.caniusevia.com/docs/specification/)
describes layout and controls after protocol support exists. On a working QMK
port, the subsequent steps are `VIA_ENABLE = yes`, dynamic keymap storage, a
layout definition, and compatible encoder/layer handling. See the current
[VIA configuration workflow](https://www.caniusevia.com/docs/configuring_qmk/).
That build option does not implement a missing MCU platform.

## Recommended next milestones

1. Identify the chip marking, PCB revision, matrix connections, and reset/debug
   pads. Establish a recovery method and preserve configuration before any
   firmware experiment. The official download alone does not establish recovery.
2. Match the public SDK's startup and ROM/peripheral interfaces to this image;
   trace matrix scan and flash writes. Build a minimal wired implementation and
   verify recovery before replacing the key-processing firmware.
3. Bring up USB typing, encoder, and persistent keymaps on the exact board;
   then integrate QMK and VIA. Handle RGB and radio modes as separate milestones.

If the immediate objective is easier remapping, the existing Rust configurator
and a host-side adapter have much less unknown work. If the objective is QMK's
onboard behavior, the MCU platform and recovery milestones are necessary.

## Verification and retained evidence

- Both compressed and decompressed input paths are pinned by SHA-256.
- Both startup RAM decoders match the original routines in ARM emulation.
- App identity/revision and boot USB descriptors were extracted from the decoded
  data; base/Fn tables match the independent macOS reference.
- [m1w-firmware-v108.json](m1w-firmware-v108.json) records hashes, sizes, mapping,
  descriptor bytes, and complete table-match offsets.
- `.amp/in/firmware/id983/analysis-v1/thumb-analysis.txt` retains annotated
  disassembly. Linear disassembly includes data pools; decoded instructions are
  not automatically evidence of executable code or an established function.
- Acquisition responses, comparison source metadata, SDK header, and additional
  intermediate analysis remain under `.amp/in/firmware/`.
