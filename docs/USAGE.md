# Using the MonsGeek configurator

The CLI and native GUI support the [52 catalog model IDs](KEYBOARDS.md) over USB,
receiver and experimental Bluetooth. Hardware validation remains limited to the
original M1W. Firmware updates have a separate experimental flow in
[FIRMWARE.md](FIRMWARE.md). Audio/screen streaming and destructive reset are excluded.

## Model selection and previews

```sh
monsgeek-linux models
monsgeek-linux --model 2247 keys
monsgeek-linux --model 2247 light --mode 4 --option 2 --rainbow
monsgeek-linux --device /dev/hidrawN snapshot keyboard.json
monsgeek-gui --device /dev/hidrawN
```

An offline command without a model or baseline defaults to ID983. Names shared
by several revisions are ambiguous: use a numeric ID. Snapshot-backed edits infer
their model and reject a conflicting `--model`. Apply identifies the connected
model before encoding changes and verifies any explicit selection. Use `--device`
when several configuration interfaces of the preferred transport are present.

The GUI's **Keyboard models** picker opens isolated sample layouts and lighting.
It cannot apply settings or export synthetic data as a hardware backup. Reload
returns to hardware discovery. All other examples below use original M1W defaults
unless a model is supplied. Run any command with `--help` for its arguments.

## Bluetooth (experimental)

Pair with Linux, select Bluetooth mode and wake the keyboard. USB and receiver
connections take priority unless `--transport bluetooth` or `--device` selects
Bluetooth. The CLI warns on open; the GUI keeps the warning visible.
Bluetooth has not been tested on hardware. Use USB if it fails.

```sh
monsgeek-linux --transport bluetooth inspect
monsgeek-linux --transport bluetooth snapshot bluetooth-before.json
monsgeek-linux --transport bluetooth battery
```

The same preview, backup, identity and readback checks apply. A failed write may
already have changed a setting. Stop and inspect over USB before trying again.

## Safety and backups

Every change previews by default, without opening HID. Applying requires all
three flags: `--apply --allow-experimental-writes --before NEW_FILE`. Each apply
saves the complete captured before-state and the inverse changes before sending
anything, waits for firmware settling, and checks readback. Files are0600, never
overwritten. A device-node advisory lock prevents simultaneous instances of this
tool; it cannot force unrelated programs to cooperate. Close other configurators.

```sh
cargo build --release
./target/release/monsgeek-linux list
./target/release/monsgeek-linux inspect
./target/release/monsgeek-linux snapshot original.json
```

Snapshot format3 contains the identified model/revision, all model-specific
512-byte base/Fn maps, raw lighting, active profile, supported onboard settings,
384 selected-picture RGB bytes and all 50 raw 256-byte macro slots. `advanced`
holds report rate, four-byte options, side lighting, magnetic arrays/precision,
legacy magnetic preset and optional output mode when available. Magnetic base
maps include four outputs per profile. Format1/2 original-M1W exports remain
readable. A snapshot is configuration, **not firmware, inactive pictures or prior
sensor calibration**. Restore, profiles and undo refuse another model or firmware
revision; magnetic restoration also requires matching precision.

```sh
./target/release/monsgeek-linux light --mode 1 --color 115395 \
  --apply --allow-experimental-writes --before before-light.json
./target/release/monsgeek-linux undo before-light.json
./target/release/monsgeek-linux undo before-light.json \
  --apply --allow-experimental-writes --before before-undo.json
./target/release/monsgeek-linux restore original.json
```

`restore` without `--current` reads the device to calculate differences, but
does not write unless all apply flags are supplied. For a fully offline preview,
provide `--current current-snapshot.json`. Applying that preview rejects a stale
current snapshot. Restore uses single-slot writes rather than the vendor's
truncated504-byte bulk writer. Macros precede bindings; layer/lighting come last.
Restore is not atomic. On error, stop and inspect the saved state; no automatic
retry or rollback is attempted. Undo refuses a different firmware revision.

## Keys, combos, media, mouse and knob

Source names include letters, key names from the original README, decimal/hex
keyboard usages, `knob-left`, `knob-press`, `knob-right`, `fn`, and `slot:N`
(0..127). `keys` lists the full concrete default matrices. These identify original
physical positions, not current bindings. `--layer` selects a zero-based map within the model's dimensions. On magnetic
boards, base-map index is `profile * 4 + output`; `profile` itself still selects
a logical profile. Fn indices enumerate Windows maps, then macOS maps. Original
M1W revision0108 ignores the packet's layer field, so only ID983 uses temporary
layer selection before single-binding writes. OS-switch semantics need separate
physical validation.

```sh
./target/release/monsgeek-linux remap --from capslock --to leftctrl
./target/release/monsgeek-linux bind --from capslock \
  --binding '{"kind":"keyboard","keys":[224,225,41]}'
./target/release/monsgeek-linux bind --from knob-press \
  --binding '{"kind":"consumer","usage":226}'
./target/release/monsgeek-linux bind --from capslock --fn-layer \
  --binding '{"kind":"macro","slot":0,"mode":0}'
```

Binding JSON types:

| `kind` | Fields / meaning |
|---|---|
| `keyboard` | `keys:[usage,…]`, one to three usage codes, not modifier masks |
| `consumer` | `usage:u16`; volume down234/up233/mute226/play-pause205/previous182/next181/calculator402 |
| `system` | `usage:u8`; power129/sleep130/wake131; do not trigger these in unattended tests |
| `mouse` | `button:240..244` for left/right/middle/back/forward |
| `pointer` | `action`: scroll-up/down, move-left/right/up/down, wheel-left/right/forward/back |
| `macro` | `slot:0..49`, `mode`:0 repeat count,1 toggle,2 while-pressed-style |
| `disabled` | No fields |
| `function` | `value:[b0,b1,b2,b3]`, vendor function tuple |
| `raw` | `value:[b0,b1,b2,b3]`, lossless unknown tuple |

Examples: Fn function `[10,1,0,0]`, original knob center `[10,14,0,0]`, profile
exchange `[8,0,4,index]`. Generic table membership is not proof every firmware
implements a function. Raw/function bindings deliberately do not whitelist all
vendor tuples, so users can preserve existing Fn actions; do not use unknown
reset/bootloader-like actions. Binding a macro does not upload its body: upload
first, then bind. Direct raw plans are an expert interface.

## Macros

```sh
./target/release/monsgeek-linux macro 0
./target/release/monsgeek-linux macro 0 --file macro.json
./target/release/monsgeek-linux macro 0 --clear
```

Macro files have repeat1..65535 and a list of events; each delay follows its event:

```json
{
  "repeat": 3,
  "events": [
    {"kind":"keyboard","usage":4,"down":true,"delay_ms":37},
    {"kind":"keyboard","usage":4,"down":false,"delay_ms":139}
  ]
}
```

Also supported: `mouse-button` with `button:240..244`, `down`, `delay_ms`; `move`
with signed8-bit `dx`,`dy` and `delay_ms`. Delays are0..65535. All key/button
presses must be balanced and released at the end. Maximum254 encoded event bytes
after the repeat count; overflow is rejected, never truncated. Zero delays and
mouse movements use explicit extended records to avoid vendor short-codec bugs.
Read commands retain the raw256-byte image even when semantic decode fails.
Five write pages are always sent, including all-zero clears and intermediate zero
chunks. This differs deliberately from the vendor's defective nonzero-chunk count.
No global keystroke recorder is included; create/edit/import semantic JSON instead.

## RGB and onboard effects

Effects and direction choices are validated against the selected model. Most
boards expose modes0..19; some add23/24. Brightness/UI speed are0..4. Main wire
speed is inverted with model-specific maximum5 (YC500) or4 (RY5088). Picture13
has three or five slots according to the catalog. RGB reads return the selected
384-byte map: selecting another picture changes that read. Select a picture first,
save its snapshot, then edit/upload and undo while that same picture is selected.
Repeat the export for each picture you want to retain. A single snapshot does not
export all pictures. Mixed RGB/picture-selection transactions are refused rather than
writing one picture's backup into another. For a restore that needs another
picture, select that picture in a separate transaction before restoring.

```sh
./target/release/monsgeek-linux rgb
./target/release/monsgeek-linux rgb --file colors.json
./target/release/monsgeek-linux paint --snapshot original.json --key capslock --color 115395
./target/release/monsgeek-linux light --mode 13 --option 0 --brightness 4
```

`rgb --file` requires exactly128 RGB triples, each component0..255. This replaces
all colors. `paint` edits just one slot from a saved snapshot, preserving all
others; apply rejects a changed RGB baseline or different picture. Uploading
colors and selecting picture mode are separate operations. A `plan` can combine
them only when picture selection is unchanged. Do not use persistent writes for
real-time streaming. On original ID983 only, macro writes temporarily select picture0, preserve that
entire map against an observed device side effect, then restore the prior effect
after verification. Expect a brief lighting change. Retain before-state files
and separate picture exports if any operation fails; failure recovery is not
guaranteed by the in-memory preservation step.

## Debounce, timers and options

```sh
./target/release/monsgeek-linux debounce 3
./target/release/monsgeek-linux sleep --bt 181 --receiver 239 --deep-bt 901 --deep-receiver 1201
./target/release/monsgeek-linux option --snapshot original.json --name win-lock --enabled true
```

Debounce1..10 vendor units where supported. Timer availability and bounds follow
the catalog; do not apply original-M1W timer ranges to every board. Timers affect
wireless operation and can be configured over a supported transport. Options preserve
all raw bits except the named field. Names: win-lock, wasd-swap, led-off,
keyboard-lock, power-save. Capture a new baseline after changing options before
editing another option, or combine explicit raw changes in a plan. Stale option
baselines are rejected. OS and Fn-system flags are not guessed from the vendor's
inconsistent Boolean/string conversion; inspect/edit existing raw tuples only
if you understand their semantics. `auto-os true|false` has passed a distinct
enabled readback and restoration test on ID983; automatic switching behavior still requires
a physical OS-switch test. Consult current validation.

## Magnetic settings, side lights and modern options

Capture a current snapshot before editing one key. These commands preserve all
other raw values and work offline until the ordinary apply flags are supplied:

```sh
monsgeek-linux magnetic --snapshot keyboard.json --from a \
  --setting actuation --value 1.5
monsgeek-linux magnetic --snapshot keyboard.json --from a \
  --setting rapid-trigger --value 1
monsgeek-linux magnetic --snapshot keyboard.json --from a \
  --setting snap-pair --partner d
monsgeek-linux --model 2247 advanced '{"setting":"report-rate","hz":1000}'
monsgeek-linux --model 2845 advanced '{"setting":"side-lighting","settings":{"mode":2,"wire_speed":2,"brightness":3,"flags":8,"rgb":[0,200,255]}}'
```

Magnetic setting names are actuation, release, rapid-press, rapid-release,
bottom-dead-zone, top-dead-zone, dynamic-threshold, tap-hold-ms, switch-type,
mode, rapid-trigger and snap-pair. Modes are Normal0, Dynamic2, Tap-hold3,
Hold toggle4 and Tap toggle5; pairing sets Snap7 on both keys. Travel uses mm,
tap-hold uses ms, switch-type uses the model's code. Precision/ranges are checked
before encoding, including rejection of values that would silently round.

The GUI's **Actuation** page offers typed numeric values and steppers, key mode,
rapid trigger, available dead zones, switch type and Snap pairing. Dynamic mode
adds four output bindings and four stages per output (Off, Tap, End hold,
Start hold). Errors stay beside the input and block Apply until corrected.

`advanced` accepts a tagged setting object: `report-rate` (`hz`), `options`
(`bytes:[b0,b1,b2,b3]`), `side-lighting` (`settings`), `magnetic`
(`parameter`,`data`), `magnetic-mode` (`value`, legacy HE) or `output-mode`
(`value`, only when captured as supported). Raw array editing is an expert
interface. The GUI labels modern option fields separately from legacy bitfields.
The named `option --name wasd-swap` routes correctly for modern boards; the other
legacy named bits are rejected on that family. Reuse a fresh baseline for each
named option edit.

### Live travel and calibration

```sh
monsgeek-linux travel
monsgeek-linux --model 2247 calibrate start
```

`travel` queries the real keyboard. Modern results use mm; legacy magnetic
reports use a normalized calibration range, displayed as percentages in the GUI.
`calibrate start` without apply is a preview. Starting/finishing requires USB and
`--apply --allow-experimental-writes --before NEW_PATH` for each step. After
starting, press every key fully and hold it for at least one second, then run
`calibrate finish` with another before-state file. A configuration backup cannot
restore prior sensor calibration. GUI start has an explicit confirmation and
keeps Finish available after an interrupted attempt. Calibration was not run
on hardware during development.

## Ordered plans and configuration import

`plan changes.json` previews an ordered array of serialized `Change` objects.
Use `changes` from any preview as its file content. A multi-operation plan is
backed up before any send and stops at first error, retaining partial state.
Raw macro/RGB data lengths, slot indices and typed timer ranges are checked before
hardware access. No plan can flash firmware; arbitrary feature-report opcodes
are not accepted. Only the documented Change variants are supported.

See [VALIDATION.md](VALIDATION.md) for executed checks versus physical behavior
and persistence still requiring owner participation.
