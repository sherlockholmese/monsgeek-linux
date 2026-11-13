# Agent handoff: MonsGeek keyboard catalog

## Goal and current state

The owner requested full implementation across the bundled MonsGeek catalog,
model-specific previews/lighting, proper animations, polished native UI without
center-dot separators, and documentation after implementation. The CLI and GUI
now cover all 52 catalog IDs across YC500 and RY5088 (including YC3123) families.
See [KEYBOARDS.md](KEYBOARDS.md) for the exact boundary and per-model dimensions.

Only original M1W ID983/revision0108 has prior physical round-trip evidence.
Other support is vendor-source-derived and tested offline. New sensor calibration,
firmware flashing and Bluetooth were not hardware-validated. Native streaming,
screens and destructive/reset/recovery operations remain excluded. Do not turn
catalog completeness or headless screenshots into hardware compatibility claims.

The repository contains uncommitted implementation/documentation changes. Preserve
them. Do not restart model discovery or substitute source defaults for owner
backups. Read STATUS, USAGE, VALIDATION and PROTOCOL before new hardware work.
No installation, device writes, calibration, flashing, commit or push was performed
in this pass. The udev rule was verified, not installed.

## Catalog implementation

- `src/models.rs`, `docs/models.json`, `docs/tools/extract_models.py`: factual
  IDs, class family, capabilities, concrete matrices, geometry, ranges and hashes.
- `src/advanced.rs`: family-specific options/report-rate/side codecs, magnetic
  arrays and typed edits, DKS transposition, precision/range checks and pairing.
- `tests/models.rs`: all 52 defaults/geometries/maps, boundary slots, family
  packets, picture counts, cross-model/precision guards and boot/report framing.
- `gui/src/board.rs`, `effects.rs`: physical model rendering, native contact
  controls and bounded release, deterministic main/side animation simulations.
- `gui/src/app.rs`, `pages.rs`: model picker, isolated previews, typed actuation,
  outputs, model controls, calibration/live travel and per-key review integration.
- GUI accepts `--device /dev/hidrawN`; target is pinned per process. Both apps
  default to USB, receiver, then Bluetooth. Device identity is rechecked on writes.

M7W's Insert geometry maps to Home slot85 as a documented positional alias.
Do not remove the caveat without a physical test. Main/side animation timing is a
simulation; reactive preview taps are synthetic and are not global input capture.

## Ownership and interfaces

The later feature pass adds `src/profiles.rs`, `review.rs`, `pictures.rs`,
`diagnostics.rs`, `host.rs` and `host/linux.rs`; GUI integration is in
`gui/src/features.rs` and `desktop.rs`. Read [FEATURES.md](FEATURES.md) before
changing the host service, profile restore masks, complete RGB capture or desktop
preferences. The host engine is deterministic and separately tested from its
evdev/uinput transport; no real input capture was activated during development.
The GUI's host child exits with its parent and preserves explicit Start/Stop.
Profiles and picture restoration stage changes for the existing transaction
path. The named-profile CLI is plural `profiles`; the older singular `profile`
command still selects an onboard layer.

- `src/protocol.rs`: checksum/framing, identity, Lighting, serialized Change,
  scalar and multipart packet dispatch, ordinary usage-to-slot mapping.
- `src/config.rs`: typed/raw binding tuples, full source-slot lookup, semantic
  macro validation/encode/decode, macro/RGB pages, sleep timers.
- `src/device.rs`: strict HID selection, held device-node advisory lock, queries,
  raw page reads, format3 exports, inverse/difference generation, settling and
  readback, macro/RGB preservation.
- `src/main.rs`: offline preview and explicit apply gate, JSON imports/plans,
  exclusive0600 before-state files, stale-baseline/revision/picture guards.
- `scripts/validate-hardware.py`: opt-in release-CLI hardware round trips. Never
  run concurrently with another configurator. Saves commands, before/after and
  restoration snapshots; stops on mismatch, never blindly retries.
- `tests/`: CLI contract, public codecs and framing tests. Use `cargo test`,
  `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`.
- `docs/m1w-reference.json`: complete concrete host-default matrices, not owner
  backups. `docs/tools/extract_reference.py` reproduces them without executing
  the proprietary application. Acquisition/hashes/offsets: [SOURCES.md](SOURCES.md).

## Evidence and non-obvious decisions

1. All settings are preview-only by default. Applying requires
   `--apply --allow-experimental-writes --before NEW_PATH`. Backups/inverses are
   validated and saved before sends. Multi-change writes are not atomic; failures
   stop, with no general rollback/retry guarantee.
2. Original M1W has four512-byte base/Fn maps. Other models use catalog dimensions. Restore uses single-slot writes because
   the vendor's nine56-byte bulk packets cover only504 bytes. Never silently lose
   slots126/127 or replace unknown tuples with defaults.
   Revision0108 ignores the single-write layer field: a layer3 packet while
   active0 changed base0. Apply now temporarily selects the target, verifies the
   binding and restores the original layer; base3/Fn2 round trips passed.
3. Single keyboard binding is `[0,0,usage,0]`, two/three usages occupy the last
   two/three bytes. They are not modifier masks. Mouse buttons are
   `[1,0,F0..F4,0]`. Concrete knob center is `[10,14,0,0]`, not mute.
4. Macro writes always send five pages, including empty clears/gaps. Last-page
   length is32. Decoder retains raw captures and rejects malformed semantics.
   Zero delays/movement use extended records due to inconsistent vendor short
   encoding. Full254-event-byte storage, delayed movement and zero gaps have
   separate validation cases; playback timing is not established by readback.
5. Slot49 macro writes/clears changed RGB byte27 on revision0108. Final lengths
   56 and32 both showed it, so an overrun explanation is **not established**.
   A later picture1-selected test still changed picture0 byte27; preserving only
   the selected picture was insufficient. Apply now explicitly selects/captures
   picture0, restores it when changed, verifies RGB/macro and restores the prior
   lighting effect. Do not remove preservation based on host code or assume a
   successful HID send means it is unnecessary.
6. Picture selection changes GET_USERPIC's returned map even with selector0.
   Selector1/2 reads did not expose other pictures. Snapshots contain **only the
   selected picture**. Select/export/edit/undo each separately. Mixed picture
   selection and RGB transactions are refused; a raw-only RGB undo also checks
   picture identity. Do not claim a read-only export backs up all three pictures.
   Pictures1/2 passed independent single-slot edits, switch-away/back and exact
   undo, with picture0 unchanged. The separate macro/picture interaction case
   captures all three pictures and compares them after macro writes/clears;
   tests with pictures1 and2 selected passed after the final preservation fix.
7. Auto-OS true and layer1 produced distinguishable replies and were restored;
   this resolves the earlier all-zero-echo ambiguity for these settings. Physical
   OS-switch effects and persistence are still unproved. Options stay raw because
   the vendor getter/setter OS-bit and Fn-byte semantics disagree.
8. Existing captures and vendor sources live under `.amp/in/`; paths alone do
   not transfer them to another machine. Never redistribute native executables
   as project code. No vendor/native binary was executed during reconstruction.

## Remaining validation and research

- Ask the owner to observe safe key/knob/Fn actions, macro playback and LEDs and
  perform unplug/replug tests. Do not trigger power/sleep actions unattended or
  infer physical effects from stored bytes. Restore original state after each.
- Establish physical Win/Mac-switch relationships to vendor layer indices and
  raw option/OS/Fn flags. Avoid synthesizing guessed Boolean conversions.
- Characterize power-cycle retention; independent picture edits across mode
  transitions and macro preservation of inactive pictures have passed on this
  revision/slot49. Snapshot coverage must remain explicit.
- Continue transport failure/disconnect testing without inventing recoverability.
  Advisory locking cannot stop unrelated applications that ignore the lock.
- Bluetooth is implemented from the macOS reference and marked experimental.
  Hardware is unavailable for this implementation pass. Validate discovery,
  full snapshots, one reversible write and disconnect behavior when it returns.
- Audio/screen streaming stays excluded. The native service/plugin path is in
  [STREAMING.md](STREAMING.md); persistent RGB writes are not a streaming codec.

## Safe continuation

Inspect existing evidence before new hardware work. Capture actual state, choose
a distinguishable reversible value, apply once, read back, explicitly restore,
and compare all exported fields. Stop at any unexplained mismatch. Keep the
before-state even after success; source defaults are not a recovery image.
The firmware implementation and its offline checks are in [FIRMWARE.md](FIRMWARE.md).
Actual hardware flashing was not requested during implementation.
Do not install rules, flash/reset, commit, push or publish this work
without the owner's separate request. Current CLI/JSON examples are in USAGE.
