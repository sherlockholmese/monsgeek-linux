# MonsGeek implementation status

The Rust CLI and native GPUI app cover all 52 MonsGeek entries in the bundled
v500.3.3 catalog. The [exact model table](KEYBOARDS.md) defines the boundary.
This is a userspace configurator, not a kernel driver or firmware replacement.

## Evidence boundary

Original M1W ID983/revision0x0108 has earlier real USB/receiver readback and
reversible configuration tests. All other models are implemented from vendor
host evidence and tested offline. This pass did not configure, calibrate or
flash physical keyboards. No claim of 52 hardware-validated devices is made.
Physical key/knob behavior, LED fidelity, timer expiry, OS-switch semantics,
retention and disconnect recovery still need per-model validation.

## Feature coverage

| Feature | Current implementation |
|---|---|
| Discovery/identity |52-model ID allowlist, exact USB/receiver catalog, Bluetooth vendors, numbered HID feature reports |
| Keyboard preview | Per-model geometry, numpads, ISO/JIS, true knob positions, fixed inputs, keyboard navigation/contact feedback |
| Maps | Model-specific profiles/Fn maps and four outputs per magnetic profile; all128 tuples preserved |
| Binding/macros | Typed/raw keyboard, media, system, pointer, disabled, vendor functions and all50 macro slots |
| Lighting | Advertised effects/directions/palettes, model speed inversion, per-key RGB,3/5 pictures and separate side effects |
| Animation | Main/side simulations, time/speed/direction/color response, play/pause, suspended inactive frames and reduced motion |
| Magnetic | Actuation/release, rapid trigger, dead zones, timing, modes, Snap pairs, switch types and four-channel DKS |
| Sensor operations | Explicit USB calibration and live travel; calibration cannot be undone by snapshot |
| Performance/options | Model-specific report-rate ceiling, debounce, available timers, legacy/modern options and feature-gated output mode |
| Snapshots/profiles | Format3, model/revision/precision guards, selective profiles, readable per-key review, undo and saved before-state |
| Firmware | Experimental model-specific prefixes/boot headers, selected physical port and target identity checks |
| Desktop/host | Focused tester, diagnostics, tray, notifications, optional startup and evdev/uinput remapper retained |

The GUI can inspect every model offline. Preview defaults are synthetic and
cannot be applied, exported as hardware backups, calibrated or flashed. The CLI
and GUI accept an explicit device path for multiple attached interfaces. Unknown
IDs remain refused. Catalog extraction is reproducible without executing vendor JS.

## Original-M1W limits and implementation choices

The following observations concern ID983/revision0108 only. The macro/RGB and
inactive-layer workarounds are scoped to that model; they are not imposed on
every keyboard.

- **Pictures:** GET_USERPIC returns the selected picture, despite a zero selector.
  Selecting picture2 changed the read image; selecting0 returned the untouched
  original. Queries with selector1/2 did not expose different images. A read-only
  export captures one selected384-byte picture, **not all three pictures**.
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

## Exclusions and remaining validation

Native audio/screen streaming, keyboard-screen hardware, global macro recording,
destructive reset, wireless flashing and already-booted recovery remain excluded.
The unresolved streaming codec is documented in [STREAMING.md](STREAMING.md).
Catalog coverage does not imply compatibility with unrelated VIA/QMK models.

M7W ID1215 has a vendor Insert-geometry/Home-slot discrepancy described in
[KEYBOARDS.md](KEYBOARDS.md). The alias is source-derived and needs physical
confirmation. New model/protocol support, sensor operations, firmware flashing
and Bluetooth behavior require hardware validation. Existing historical ID983
round-trip records remain in [VALIDATION.md](VALIDATION.md).

No access rules were installed, host capture enabled, startup preferences changed,
firmware flashed, hardware calibrated, commits created or changes pushed during
this implementation pass.
