# Keyboard coverage

The configurator implements the 52 MonsGeek model IDs in the bundled official
v500.3.3 catalog. This is a catalog boundary, not a claim about every MonsGeek,
Akko, QMK or VIA keyboard ever released. Unknown internal IDs are refused.

Support beyond ID983 was reconstructed from vendor host methods, concrete
matrices, capabilities and layout geometry, then tested offline. **Only the
original M1W has the earlier hardware round-trip evidence.** None of the other
51 entries was physically validated during this implementation pass. Host
metadata and successful packet tests do not prove firmware behavior or retention.

## Exact catalog

Names can identify several hardware revisions. Prefer the numeric ID from
`monsgeek-linux models`; the live keyboard is identified before configuration.
Each row has model-specific geometry, matrix lookup, lighting controls and
snapshot/restore support. HE/TMR entries additionally have actuation controls.

Base maps include all four output planes per magnetic profile. Fn counts are
Windows/macOS maps, not additional base profiles. Picture numbers in the UI are
one-based; CLI options are zero-based. “Side” means a separate side-light codec.

| ID | Model | Family | Type | Profiles | Base maps | Fn maps | Pictures | Side |
|---|---|---|---|---|---|---|---|---|
| 969 | MG108B | YC500 | Mechanical | 4 | 4 | 4 | 3 | — |
| 983 | M1W | YC500 | Mechanical | 4 | 4 | 4 | 3 | — |
| 1200 | M3W | YC500 | Mechanical | 4 | 4 | 4 | 3 | — |
| 1215 | M7W | YC500 | Mechanical | 4 | 4 | 4 | 3 | — |
| 1308 | M5W | YC500 | Mechanical | 4 | 4 | 4 | 3 | — |
| 1419 | ICE75 | YC500 | Mechanical | 4 | 4 | 4 | 3 | — |
| 1604 | M1W HE | YC500 | Magnetic | 5 | 20 | 4 | 5 | — |
| 1674 | MG75S HE | YC500 | Magnetic | 4 | 16 | 4 | 3 | — |
| 1933 | M1 V5 | YC500 | Mechanical | 4 | 4 | 4 | 3 | — |
| 2247 | M1 V5 TMR | RY5088 | Magnetic | 4 | 16 | 4 | 5 | — |
| 2299 | FUN60 | RY5088 | Magnetic | 4 | 16 | 4 | 5 | — |
| 2304 | FUN60 PRO | RY5088 | Magnetic | 4 | 16 | 4 | 5 | — |
| 2305 | FUN60 PRO | RY5088 | Magnetic | 4 | 16 | 4 | 5 | — |
| 2306 | FUN60 MAX | RY5088 | Magnetic | 4 | 16 | 4 | 5 | — |
| 2307 | FUN60 Ultra | RY5088 | Magnetic | 4 | 16 | 4 | 5 | — |
| 2352 | FUN60 Ultra | RY5088 | Magnetic | 4 | 16 | 4 | 5 | — |
| 2381 | FUN60 Ultra | RY5088 | Magnetic | 4 | 16 | 4 | 5 | — |
| 2387 | FUN60 Ultra | RY5088 | Magnetic | 4 | 16 | 4 | 5 | — |
| 2464 | FUN60 PRO | RY5088 | Magnetic | 4 | 16 | 4 | 5 | — |
| 2536 | M1 V5 TMR | RY5088 | Magnetic | 4 | 16 | 4 | 5 | — |
| 2585 | M3 V5 | RY5088 | Magnetic | 4 | 16 | 2 | 5 | — |
| 2600 | FUN60 PRO | RY5088 | Magnetic | 4 | 16 | 4 | 5 | — |
| 2601 | M2 V5 | RY5088 | Magnetic | 4 | 16 | 2 | 5 | — |
| 2648 | FUN75 | RY5088 | Magnetic | 4 | 16 | 2 | 5 | — |
| 2679 | M1 V5 TMR | RY5088 | Magnetic | 4 | 16 | 4 | 5 | — |
| 2721 | Verve68 | RY5088 | Magnetic | 4 | 16 | 4 | 5 | Yes |
| 2739 | MG108B | RY5088 | Mechanical | 4 | 4 | 4 | 5 | — |
| 2785 | FUN60 PRO | RY5088 | Magnetic | 4 | 16 | 4 | 5 | — |
| 2811 | FUN68 | RY5088 | Magnetic | 4 | 16 | 4 | 5 | — |
| 2819 | M1 V5 HE | RY5088 | Magnetic | 4 | 16 | 4 | 5 | — |
| 2845 | M2 V5 HE | RY5088 | Magnetic | 4 | 16 | 4 | 5 | Yes |
| 2874 | M3 V5 HE | RY5088 | Magnetic | 2 | 8 | 4 | 5 | — |
| 2949 | M1 V5 TMR | RY5088 | Magnetic | 2 | 8 | 4 | 5 | — |
| 3091 | FUN68 | RY5088 | Magnetic | 4 | 16 | 4 | 5 | — |
| 3099 | Verve68 | RY5088 | Magnetic | 4 | 16 | 4 | 5 | Yes |
| 3147 | FUN68 | RY5088 | Mechanical | 4 | 4 | 4 | 5 | — |
| 3245 | MG108B | RY5088 | Mechanical | 4 | 4 | 4 | 5 | — |
| 3269 | Shine68 | RY5088 | Magnetic | 4 | 16 | 4 | 5 | Yes |
| 3270 | Shine68 | RY5088 | Magnetic | 2 | 8 | 2 | 5 | Yes |
| 3299 | FUN60 MAX | RY5088 | Magnetic | 4 | 16 | 4 | 5 | — |
| 3429 | FUN68 ISO | RY5088 | Magnetic | 4 | 16 | 4 | 5 | — |
| 3460 | Verve68T | RY5088 | Magnetic | 4 | 16 | 4 | 5 | Yes |
| 3463 | Verve68 | RY5088 | Magnetic | 4 | 16 | 4 | 5 | Yes |
| 3722 | FUN60 PRO ISO | RY5088 | Magnetic | 4 | 16 | 4 | 5 | — |
| 3853 | FUN60 Ultra TMR | RY5088 | Magnetic | 4 | 16 | 4 | 5 | — |
| 3906 | FUN60 MAX | RY5088 | Magnetic | 4 | 16 | 4 | 5 | — |
| 4088 | FUN87 HE | RY5088 | Magnetic | 4 | 16 | 4 | 5 | — |
| 4158 | M1 V5 TMR ISO | RY5088 | Magnetic | 2 | 8 | 4 | 5 | — |
| 4180 | FUN60 Pro JP | RY5088 | Magnetic | 4 | 16 | 4 | 5 | — |
| 4211 | M1 V5 TMR ISO | RY5088 | Magnetic | 4 | 16 | 4 | 5 | — |
| 4212 | M1 V5 TMR | RY5088 | Magnetic | 4 | 16 | 4 | 5 | — |
| 4294 | FUN60 PRO | RY5088 | Magnetic | 4 | 16 | 4 | 5 | — |

## Implemented capabilities

- All physical key geometries, ISO/JIS differences, numpads, actual knob positions
  and firmware-fixed knob inputs. Reset uses the correct Windows/macOS Fn matrix.
- Model-specific base/Fn maps, four magnetic outputs, typed/raw bindings, all 50
  macro slots and complete 384-byte selected-picture storage.
- Advertised onboard effects and valid direction choices, palette/color flags,
  brightness, speed inversion and three or five pictures. Side effects have their
  own namespace and speed handling.
- Magnetic actuation/release, rapid trigger, dead zones, dynamic thresholds,
  tap-hold timing, per-key modes, four-channel DKS stages, Snap pairing and
  replaceable switch types where advertised. Precision follows device revision
  and the feature report. Values that cannot be represented are rejected.
- Report rate, debounce, available wireless timers, keyboard options and automatic
  OS detection according to model capabilities. Optional keyboard/gamepad output
  appears only when the device feature response declares it.
- Explicit USB calibration and live travel. A configuration backup cannot undo
  sensor calibration. An interrupted session keeps Finish available and blocks
  Apply, reload, picture capture and firmware flashing until it is finished.
- Model-checked snapshots, selective profiles, review, undo, RGB backups,
  discovery/permissions and experimental model-targeted USB firmware parsing.

## Selecting a keyboard

```sh
monsgeek-linux models
monsgeek-linux --model 2247 keys
monsgeek-linux --model 2247 light --mode 4 --rainbow --option 2
monsgeek-linux --model 2247 advanced '{"setting":"report-rate","hz":1000}'
monsgeek-linux list
monsgeek-gui --device /dev/hidrawN
```

Offline previews default to original M1W when no model or baseline is supplied.
Snapshot-backed commands infer their model. Apply identifies the real model and
checks an explicit `--model` against it. Both apps prefer USB, receiver, then
Bluetooth; an explicit `--device` selects among simultaneously attached devices.
The GUI configures one selected keyboard per process.

**Keyboard models → Open preview** loads synthetic defaults and shows “Offline
preview”. Editing a preview never enables Apply, hardware picture capture,
calibration, live travel, backup export or firmware flashing. Reload returns to
actual device discovery. Catalog defaults are not owner backups.

## Preview and animation behavior

The preview uses each model's physical positions and dimensions. Main and side
lighting share one pausable timeline. Moving effects respond to speed, supported
directions, color and brightness. Reactive modes use deterministic simulated
presses; they do not listen to global keyboard input. Static/custom effects stay
still. Hidden and inactive previews suspend; reduced motion holds the current
frame and suppresses animation scheduling. Preview keys have immediate pointer
and keyboard contact material with a 90 ms native release, fixed bounds, visible
keyboard focus and one activation per release. The focused keyboard tester can
show actual held-key travel separately.

These are **simulations**, not recordings of firmware LED timing, diffusion or
randomness. Hardware animation fidelity remains a physical validation task.
Audio/screen streaming is excluded because its live HID codec remains unresolved;
see [STREAMING.md](STREAMING.md). Persistent RGB writes are not used as a substitute.

## Known evidence discrepancy

For M7W ID1215 the vendor geometry labels an Insert position, while its concrete
matrix has no Insert usage and retains Home at slot85. The implementation maps
that position to slot85 as a positional alias. The actual physical switch output
needs hardware confirmation. This discrepancy is not evidence of an independent
Insert remapping slot.

## Reproduction

[models.json](models.json) records source hashes, driver classes, concrete
matrices and factual geometry. Regenerate without executing the vendor app:

```sh
python3 docs/tools/extract_models.py \
  '.amp/in/vendor/extracted/MonsGeek Driver v4 500.3.3/MonsGeek Driver v4.app/Contents/Resources/app/dist/js' \
  > /tmp/models.json
cmp docs/models.json /tmp/models.json
```

The source archive provenance is in [SOURCES.md](SOURCES.md). See
[VALIDATION.md](VALIDATION.md) for executed checks and the distinction between
source-derived support, offline tests and hardware observations.
