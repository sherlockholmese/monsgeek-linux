# Validation plan and evidence

The owner resumed implementation and reversible setting tests after the initial
read-only research. Tests use the real release CLI against ID983/revision0108,
not a mocked HID transport. They compare complete exported state after each
change and explicit undo. An export includes the selected RGB picture, not all
inactive pictures. Physical behavior and unplug/replug persistence are unproved.

## Catalog coverage and animation pass (2026-11-14)

Support now covers the 52 model IDs listed in [KEYBOARDS.md](KEYBOARDS.md).
These additions were validated offline. No physical keyboard was configured,
calibrated or flashed in this pass. Earlier ID983 hardware records below are
historical evidence for that unit only.

Executed checks:

- `cargo test --workspace --offline`:75 passed (19 GUI,13 library,6 CLI,7 codec,
  15 feature,9 model,6 protocol). Six GPU screenshot tests are ignored by default.
- `cargo clippy --workspace --all-targets --offline -- -D warnings` and
  `cargo fmt --all --check`: clean.
- `cargo build --workspace --release --offline`: CLI and native GUI built.
- `udevadm verify udev/70-monsgeek-m1w.rules`: success; rule not installed.
- Arch PKGBUILD shell syntax and `git diff --check`: clean.
- Catalog regeneration from retained vendor files: byte-for-byte match with
  `docs/models.json`; no vendor JS execution.

The model regression suite verifies all 52 identities, matrix dimensions and
mapped physical keys, snapshot validation, complete last maps/slots, Mac Fn
defaults, family-specific packet fields,3/5 picture counts, DKS transposition,
magnetic edits/precision, cross-model snapshot/profile/picture/undo rejection,
model firmware prefix/wide headers and numbered feature reports. CLI tests use
an impossible HID path to prove modern option and magnetic previews stay offline
and infer their snapshot model. Missing advanced fields, unrepresentable travel
and magnetic-precision mismatches are refused.

The GUI test renders every page and every base/Fn/output map for every model.
Input tests verify typed actuation values, invalid-value blocking, preview
hardware guards, native pointer/Enter/Space activation, held-contact feedback,
fixed bounds and focus-loss cleanup. Animation tests sample every advertised
non-streaming simulated effect over time at different speeds across all 52
layouts, check finite colors/intensities, static frames, side effect IDs,
brightness scaling, pause/resume and reduced-motion/inactive suspension.

Two GPU tests were run explicitly with Mesa llvmpipe:

```sh
XDG_CONFIG_HOME=/tmp/monsgeek-model-qa/config \
XDG_STATE_HOME=/tmp/monsgeek-model-qa/state \
XDG_CACHE_HOME=/tmp/monsgeek-model-qa/cache \
VK_ICD_FILENAMES=/usr/share/vulkan/icd.d/lvp_icd.json \
cargo test -p monsgeek-gui --offline model_ -- --ignored --nocapture
```

They produced20 screenshots under `/tmp/monsgeek-model-*.png` and
`/tmp/monsgeek-interaction-*.png`. Inspected states include960px and1240px native
windows, light/dark, full-size and compact layouts, ISO/JIS, selected keys,
five-picture painting, side lighting, model picker open, calibration confirmation,
expanded/scrolled DKS and multiple animation frames. Inspection caught and fixed
a clipped JIS legend, misleading preview firmware text, per-key numeric clipping,
a false default Snap partner and repeated headings. No center-dot separators
remain in GUI copy. Preview motion remains an approximation of firmware timing;
headless screenshots do not establish physical LED fidelity or desktop session
behavior.

## Feature additions (2026-11-11)

The profiles, review, tester, three-picture backup, desktop integration,
diagnostics and host remapper are covered in [FEATURES.md](FEATURES.md). The
workspace suite passes 55 tests (10 GUI, 13 library, 5 CLI, 7 codec, 14 feature,
6 protocol), with three renderer-dependent screenshot tests ignored by default.
The new feature screenshot test was also run explicitly and passed, producing
ten inspected light/dark screenshots under `/tmp/monsgeek-features-*.png`.
Final `cargo fmt --all --check`, `cargo clippy --offline --workspace --all-targets
-- -D warnings`, and `cargo build --offline --release --workspace -j 4` passed.
The release CLI also exported offline diagnostics and recognized the retained
ID983 v108 vendor image by exact hash in preview mode without HID access.

New checks cover selective profile restore and offline CLI workflows, RGB
picture identity, readable per-key differences, diagnostics omissions, tester
focus loss, tap-hold interruption/timeouts, tap dance, chord fallback order,
application changes during a held action, modifier reference counts, literal
launch arguments, bounded focus queries and 5,000 mixed input events with
balanced synthetic key releases. These are software tests; they do not establish
real evdev/uinput, desktop notifications/tray, login-startup or picture-capture
behaviour on the owner's session/hardware. No such service or capture was
activated during this pass.

## Original configuration implementation results

Offline:16 Rust tests (3 CLI,7 config,6 protocol), formatter, Clippy with warnings
denied and release build passed. Tests cover public binding tuples, signed
movement,127/128/65535 delay boundaries, zero-delay extended records, exact macro
capacity/rejection, page/clear framing, asymmetric RGB/timer offsets, raw tuple
preservation, snapshot restore of slots126/127, identity and CLI apply gating.

Hardware proof runner: `scripts/validate-hardware.py`, explicitly requiring
`--allow-setting-changes --output NEW_DIRECTORY`. Every command's arguments,
exit status, stdout/stderr and elapsed time are stored in `commands.json` with
before/changed/restored snapshots. Full comparisons include all50 macro slots,
all maps and scalar settings. It stops on the first mismatch, without rollback.

### Executed successful cases

- `hardware-roundtrip-1`: asymmetric lighting, debounce, all four sleep fields,
  layer1, base and Fn remap, three-key combo, knob mute binding, mouse button and
  disabled binding. Each changed exactly the expected exported fields and undo
  exactly restored the baseline. Macro collateral state then stopped that run.
- `hardware-roundtrip-3`: macro write/clear with RGB preservation, RGB replacement,
  Windows lock and auto-OS true. Exact exported-state restoration after each.
  Auto-OS true returned a distinguishable nonzero value, not an echoed zero query.
- `hardware-roundtrip-5`: extended macro (signed movement and delay0, keyboard
  delays127/128, mouse delays1/65535), exact256-byte macro image, nonzero raw data
  after a zero gap, macro playback binding mode2, pointer scroll-down binding.
  Every case passed complete expected-state and exact restoration comparisons.
  The subsequent inactive-layer test stopped that run; see below.
- `hardware-roundtrip-6`: corrected all20-effect case, inactive base-layer3 and
  Fn-layer2 writes with temporary layer selection, and multi-setting restore
  (RGB plus debounce). Every case passed complete expected-state comparison and
  exact restoration, including the original active layer.
- `hardware-roundtrip-7`: pictures1 and2 independently selected, exported, edited
  on one slot, switched away/back and restored. Other selected-picture0 state
  stayed unchanged; each edit survived the mode switch before undo. Mixed-picture
  RGB plans and wrong-picture undo failed with their expected guard messages,
  before a backup or setting write. Both pictures' original bytes were restored.
- `final-safety`: an external lock denied `inspect`, then unlocked `inspect`
  succeeded. Deliberately stale option/RGB inputs and an existing before-state
  path were rejected. The final full snapshot is **exactly equal** to
  `.amp/in/full-before-validation.json`, with all50 macro slots empty again.
- `hardware-roundtrip-8`: full macro slot rechecked after the final preservation
  fix; macro write/clear while picture1 and then picture2 was selected. All three
  pictures were captured first and compared afterward, including inactive ones.
  All originals and prior lighting were restored. The alternate original captures
  also exactly match run7's captures.
- `final-handoff.json` and `final-handoff-result.json`: final release binary hash
  and independently compared restoration result. Export exactly matches the
  pre-test baseline, no test macros remain, and both alternate pictures match
  their saved originals. Formatter,16 tests, Clippy and release build passed
  after the final Rust change.

### Failures caught, not hidden

1. Macro slot49 write and clear changed RGB byte27 from255 to0 while macro
   readback itself matched. Run1 stopped. Reducing final-page declared length
   56→32 did **not** eliminate this in run2, so an overrun explanation is not
   established. Restoring RGB alone retained the correct macro; clearing it also
   reproduced the side effect. The first fix preserved the selected picture.
   A later `macro-picture-interaction` test while picture1 was selected still
   changed picture0 byte27. It stopped and picture0 was explicitly restored.
   Current apply temporarily selects/preserves/verifies picture0, then restores
   the prior lighting effect. The initial failure and corrected behavior are
   retained under `hardware-roundtrip-1`, `-2`, `-3`.
2. Run4 applied all20 onboard modes and two extra custom-picture choices, each
   with matching lighting readback, then the full comparison caught a changed
   RGB read image. A follow-up select0 returned the original image unchanged;
   undo restored the full baseline. Bounded selector1/2 reads were identical to0.
   This is picture selection, not evidence of destroyed original pixels. The
   corrected effect case tests all20 modes with picture0; other pictures require
   separate edit/switch/restore tests. Mixed RGB/picture-selection plans and
   wrong-picture undo are now rejected rather than guessing an inverse image.
3. Run5 requested a base-layer3 remap while layer0 was active. The firmware
   changed layer0 byte14 instead and left layer3 unchanged. The mismatch stopped
   the run; an explicit baseline restore then matched exactly. Apply now selects
   the requested layer, verifies the write there and restores the original active
   layer. Failed selection/write/restoration stops with the saved before-state.

Across these runs all23 named setting cases passed, plus two independent-picture
cases. Macro playback modes0/1 are codec-covered, not physically exercised; mode2
was stored/read back. Only Windows lock was exercised among the named option
bits. No power/sleep action, firmware update, factory reset or native streaming
was triggered. The macro/picture interaction tests exercise slot49 on revision0108,
not every slot/revision combination. Failure/disconnect recovery and power-cycle
retention remain unproved. The reproducible interaction case is `--case macro-pictures`.

### Evidence locations

All paths above are under `.amp/in/artifacts/`. The full pre-write configuration
is `.amp/in/full-before-validation.json`; initial implementation baseline is
`.amp/in/implementation-start.json`. They contained empty raw macro slots and a
custom RGB pattern; no source-default backup was substituted. Before-state and
snapshot files use0600 permissions. Raw proprietary sources stay local.

## Historical research-only results

The sections below preserve the earlier plan/read-only captures. Their statements
about untested writes describe that earlier phase, not the current state.

## Cases recorded before expanding the implementation

1. Identify the original M1W and reject other internal IDs. A shared USB ID is
   insufficient. Query only the configuration interface.
2. Read lighting, all base/Fn layers, firmware revision, and supported settings
   twice. Require stable data; preserve the original state for restoration.
3. Change lighting to an asymmetric RGB value with non-default speed/brightness;
   verify raw read-back, undo, and compare the full baseline. Check another
   setting remains unchanged.
4. Select a different vendor layer; verify and undo. Observe whether the firmware
   allows all four values, rather than assuming they are independent profiles.
5. Remap a nonessential base key and a Fn-layer key to distinct bindings; verify
   target slot and surrounding slots, undo, and compare the full baseline.
6. For each added setting, use values on both sides of relevant ranges where safe.
   Verify the requested change and original-state restoration without collateral
   changes. Respect settling times and stop on unexpected responses.
7. Round-trip a small macro containing down/up and asymmetric delays. Preserve
   the original slot bytes, restore them, and verify the entire raw slot. Test
   boundaries offline before any hardware command.
8. Round-trip custom per-key colors on two different matrix slots. Preserve all
   RGB bytes and restore them. Verify color order and unrelated slots.
9. Export the complete supported configuration, make controlled changes, restore,
   and export again. Compare settings and all stored data, not only JSON shape.
10. Exercise CLI previews, invalid input, missing write authorization, unavailable
    device, identity mismatch, existing backup path, and failed reads. Ensure
    these do not send settings writes. Check simultaneous access is rejected.
11. Run formatter, tests, Clippy, release build, and udev-rule verification.
12. Physical key behavior and persistence after an actual unplug/replug require
    the owner's participation. Do not claim them based only on read-back.

## Results

### Initial offline implementation

- Seven tests passed, including packet checksums, HID report-ID framing, original
  M1W identity rejection, correct key-slot mapping, and CLI write gating.
- 456 lighting packets matched execution of the vendor's lighting setter and
  checksum encoder byte-for-byte.
- 166 base/Fn remap previews matched all standard key positions in the vendor
  M1W matrix. This is host-code compatibility, not hardware validation.
- Release build, Clippy, formatter, and udev syntax checks passed.

### Connected hardware, read-only baseline

- `monsgeek-linux list`: expected interface found on `/dev/hidraw7`.
- `monsgeek-linux --device /dev/hidraw7 inspect`: ID983, layer0, valid lighting.
- Two `snapshot` invocations: exact JSON equality, four base and four Fn maps,
  512 bytes each. All 87 populated base slots match the vendor's default matrix.
- USB revision is 264 decimal / `0x0108`.
- No settings changes made during this baseline capture.

### Additional read-only protocol probe

Local raw capture `.amp/in/artifacts/m1w-readonly-protocol-probe.json`, timestamp
`2026-11-08T00:20:41+0800`, contains complete request/reply hex for identity,
revision, layer, lighting, options selector0, debounce, sleep, automatic OS query
and six RGB pages. A fixed GET-opcode whitelist was used; no settings writes.
The one-off capture script was removed after research. To reproduce, implement
only these documented queries with strict interface/identity checks; never sweep
unknown opcodes. Report descriptor is in the capture and protocol document.

| Query | Observation | Limit |
|---|---|---|
| `0x86`, selector0 | Bytes2..4=`10 04 00` hex | Preserve raw; getter/setter semantics disagree |
| `0x91` | Byte2=2 | Debounce setting read, no timing measurement |
| `0x92` | BT120,2.4GHz120,deepBT900,deep2.4GHz600 seconds | No writes or timer-expiry test |
| `0x8C`, six pages | 384 raw bytes, mostly zero with several `FF` bytes | Not physical color correspondence or activation proof |
| `0x85` and `0x97` | Reply exactly equals request | Cannot distinguish valid zero from echo solely from these replies |

SHA-256 baseline and repeat (identical):
`d728edad5f6eaa084077c5c2de1c5271e65005880c82bc430440ae37d4f6af66`.
SHA-256 extra probe:
`28f3092d95e07771c61c158a1a2387a443eddcbd7a4d858a3033fe10fc592074`.
Descriptor SHA-256:
`350fea62e225e3d92e9db3bbbd408fcebe386c10405454be661d3d2c3229b0b5`.

Raw local snapshots live under `.amp/in/`; they are not included in source
distributions. Do not treat source defaults as a replacement for these captures.

### Documentation verification

The matrix extractor was executed without vendor-code execution or device access.
Its output includes three512-byte/128-slot matrices from the concrete M1W class,
including the specially capitalized Mac property. All four baseline base maps
match the full extracted default. Documentation links, source hashes and capture
hashes were checked. Existing Rust tests, formatter, Clippy and release build
were rerun after documentation changes; results do not validate any new setting
write or native stream.


## Experimental Bluetooth implementation (2026-11-08)

Offline only. The macOS host's `Px` filter specifies report6 with65 payload
bytes. Tests cover complete command/page framing, report-ID filtering, split
HID report items, truncation/undersized/ambiguous descriptors and query-only
retries. CLI selection is checked without opening HID. The GUI uses the same
transport and shows a short persistent experimental warning, including during
initial reads and failures.

No Bluetooth connection, setting write, battery reading or persistence test was
performed. Hardware validation remains pending.

Verification: workspace tests, strict Clippy and GUI build pass. Headless GPU
screenshots were inspected at1240px (dark),960px (light) and960px (disconnected).
The optional `bluetooth_warning_screenshots` GUI test reproduces them without
opening a HID device. GUI logs now start empty and the disk log resets at launch.


## Experimental firmware updater (offline only)

The owner requested implementation with an experimental label. Unit tests check
boot/prepare/finish bytes, skipped prefix, partial-block padding, payload sum,
size limits, exact-file hashing, send timing, completion-marker rejection and
stop-on-failure behavior using fake USB I/O. CLI tests verify offline previews,
wireless rejection and required flashing flags/hash before HID access. GUI tests
cover the clean-USB-baseline gate and disabling edits during flashing.

No real image was uploaded, no boot command was sent and no hardware firmware
update, recovery or persistence test was performed. See [FIRMWARE.md](FIRMWARE.md)
for reference provenance and remaining limitations.

Verification passed: workspace tests, strict workspace Clippy, formatting,
release workspace build and `udevadm verify`. Headless GPU screenshots were
inspected in light/dark themes at960/1240px, including the flash confirmation
and writing state. The `firmware_screenshots` test reproduces these views
without opening HID or sending firmware commands.
