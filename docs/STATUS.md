# Original M1W implementation status

The Rust CLI implements the reconstructed non-native configuration features of
the original mechanical M1W over USB, the receiver and experimental Bluetooth. The owner resumed implementation
after the research-only phase. This is a userspace configurator, not a kernel
driver, firmware replacement or port of the graphical application.

## Device and evidence boundary

Validated device: USB `3151:4015`, interface2, usage `ffff:0002`, internal ID983,
revision `0x0108`; observed node `/dev/hidraw7` is not a permanent name.
Readback and exact configuration restoration are tested separately from physical
behavior. Key/knob output, actual LED appearance, wireless timer expiry, physical
Win/Mac-switch relationships and unplug/replug persistence still need observation.
See [VALIDATION.md](VALIDATION.md) for executed cases and failures, not just claims.

## Feature coverage

| Feature | Implementation / evidence |
|---|---|
| Discovery, identity, firmware revision | Strict identity/interface gate; real device observed |
| Four base and four Fn maps | Complete512-byte reads; single-slot writes preserve all128 slots |
| Remapping and Fn/knob positions | Typed keyboard, combos, media/consumer, system, mouse, pointer, disabled, macro and raw/vendor bindings |
| Onboard lighting | Modes0..19; bounded speed/brightness/options; custom picture labels0..2 |
| RGB | Read/write384 bytes; single-slot edit from a stale-checked snapshot |
| Macros | All50 raw slots, semantic JSON, keyboard/buttons/signed movement, repeat/delay bounds, clear and playback bindings |
| Debounce and sleep | Vendor-range checks; four independently encoded timers |
| Keyboard options | Raw-preserving named bit edits; raw tuple import retains unknown bits |
| Auto OS detection | Distinct enabled response observed and original false value restored |
| Exports / restore / undo | Format2 onboard export, format1 compatibility, difference-only restore, multi-change undo, pre-write backup |
| GUI | `gui/` GPUI app over the same transaction code; headless view/edit tests; apply not yet exercised on hardware |
| 2.4 GHz receiver | Full reads verified on `3151:4011` (identical snapshots); Apply of lighting, debounce and an inactive-layer remap, then undo, verified with a byte-identical final read; battery and wake state |
| Bluetooth | Experimental macOS-reference transport, CLI selection, battery, GUI warning and offline framing checks; no hardware validation |
| Firmware updates | Experimental USB updater from the macOS reference; local vendor binary, exact-file hash, pre-boot configuration backup, same-port boot discovery, completion check and reconnect; offline tests only |
| Safety | Offline preview; three explicit apply flags; exclusive0600 files; advisory device-node lock; settling/readback; stop on failure |

Settings round trips have covered layer selection, base/Fn edits, combos, knob
media, mouse, disabled bindings, debounce, sleep, RGB, macros, Windows lock and
auto-OS, extended/full/gapped macros, all20 onboard modes, inactive base/Fn layers
and multi-setting restore. Storing a binding does not prove its physical
action; in particular system power/sleep actions are not triggered unattended.

## Important limits and implementation choices

- **Pictures:** GET_USERPIC returns the selected picture, despite a zero selector.
  Selecting picture2 changed the read image; selecting0 returned the untouched
  original. Queries with selector1/2 did not expose different images. A read-only
  export captures one selected384-byte picture, **not all three inactive images**.
  Independent edits to pictures1/2 survived switching away/back and were undone;
  picture0 remained untouched in those tests.
  Select and export each picture separately. Transactions combining an RGB write
  with a change of picture are refused: select first, capture, then edit/restore.
- **Macro side effect:** complete slot49 writes/clears changed an unrelated RGB
  byte in picture0 on revision0108, even while picture1 was selected. Macro apply
  explicitly selects/captures/preserves picture0, verifies macro and RGB, then
  restores prior lighting. The underlying firmware cause is unknown.
  Write/clear with pictures1 and2 selected passed comparison of all three saved
  picture maps after the final fix.
- All five macro pages are sent, including clears and zero gaps; last page declares
  its actual32-byte length. Semantic zero delays/movement use extended records
  to avoid inconsistent vendor compact encoding. Firmware playback remains unproved.
- Restore uses individual binding writes, not the vendor's504-byte bulk writer
  for512-byte maps. Existing unknown tuples/bits remain raw; source defaults are
  never substituted for owner backups.
- Single-binding writes ignore their layer field on this revision. Apply selects
  the requested layer, verifies the binding, then restores the original active
  layer. Inactive base3 and Fn2 round trips passed with all other maps unchanged.
- Multi-operation writes are not atomic. Failed sends/readback stop immediately,
  with no automatic retry or general rollback. RGB preservation is an explicit
  part of successful macro apply, not a recovery guarantee after transport failure.

## Exclusions and remaining work

Named profiles, readable change review, focused keyboard testing, complete
three-picture capture/individual restore, diagnostics export, known firmware
hash recognition, battery notifications, tray/profile controls, XDG login startup
and an opt-in evdev/uinput host remapper are implemented. The host engine supports
tap-hold, tap dance, chords, app-specific remapping and direct launch actions.
See [FEATURES.md](FEATURES.md) for offline/GUI validation and the remaining real
input/desktop/hardware checks. This work did not activate host capture, change
access rules or autostart, capture pictures from hardware, or flash firmware.

Excluded: native audio/screen streaming,
system-wide macro recording, wireless firmware updates, already-booted
firmware recovery and destructive reset. Magnetic/keyboard-screen features
belong to other models and are not original-M1W coverage gaps.

Remaining validation: owner-observed actions/LEDs, power-cycle persistence, option
semantics outside the tested Windows-lock bit, layer/physical OS-switch behavior,
and failure/disconnect recovery. Final exported state exactly matches
the pre-test baseline, with no test macros left. This feature pass did not install
rules, enable autostart, activate input remapping, commit or push changes. Usage,
JSON formats and restore caveats are in [USAGE.md](USAGE.md).
