# Original MonsGeek M1W on Linux

An experimental Rust userspace configurator for the **original mechanical M1W**.
Not the M1W HE, M1 V5, VIA models, or a firmware replacement. Ordinary typing
already uses Linux's standard HID support; this program implements configuration.

**Implemented in Rust and exercised against the connected original M1W,
ID983, revision0x0108.** Reversible setting tests save the original configuration,
compare full readback, undo, and require exact restoration. Physical key behavior
and power-cycle persistence are separate checks, not inferred from readback.

Start with [the usage guide](docs/USAGE.md), [feature status](docs/STATUS.md) and
[validation evidence](docs/VALIDATION.md). Protocol/source references are in
[docs/README.md](docs/README.md). Audio/screen streaming and other host-native
services and destructive reset remain excluded. Firmware updates are
[implemented experimentally over USB](docs/FIRMWARE.md), without hardware validation.

## Implemented

- USB interface enumeration and strict identity check: `3151:4015`, interface 2,
  usage `ffff:0002`, internal device ID **983**. The USB ID is shared with other
  boards, so it is not sufficient by itself.
- Read active layer and raw lighting settings.
- Format2 configuration export: four base/Fn maps, lighting, layer, revision,
  debounce, sleep timers, raw options, auto-OS setting, RGB and all50 raw macro
  slots. This is not firmware or a disaster-recovery image. Format1 is still readable.
- Base/Fn remapping, combos, consumer/media/system/mouse/pointer actions,
  vendor/raw bindings, disabled keys, physical Fn and knob positions.
- Macro compilation/read/write/clear, repeat counts, three playback binding
  modes, keyboard/mouse events and signed movement; bounded validation.
- Onboard effects0..19 including custom-picture selection, RGB import/export,
  single-key color edits preserving all other slots.
- Experimental USB firmware flashing from a local vendor image, with offline
  preview, explicit flashing opt-in, configuration backup and progress.
- Debounce, wireless sleep/deep-sleep timers, raw-preserving keyboard options
  and automatic OS detection.
- Offline previews, ordered JSON plans, difference-only restore, multi-change
  undo, exclusive0600 before-state files, advisory device lock and readback checks.

Not implemented: host-native audio/screen/launcher services, system-wide macro
recording or destructive reset.

## Graphical configurator

`gui/` is a native GPUI application (shadcn-style GPUI Kit components) covering
the same onboard features: lighting effects and per-key custom pictures, base and
Fn remapping on all four layers including the knob, combinations, media, mouse,
layer and macro actions, macro recording and editing, debounce, wireless sleep,
keyboard options, active layer, export/open and undo.

```sh
cargo run --release -p monsgeek-gui
cargo test -p monsgeek-gui
```

It also needs the GPUI desktop libraries (Wayland/X11, xkbcommon, Vulkan,
fontconfig). Edits stay local until **Apply**. Apply opens the keyboard, refuses
if it changed since it was read, saves an owner-only before-state in
`$XDG_STATE_HOME/monsgeek-linux/` (default `~/.local/state/monsgeek-linux/`), then
writes and verifies each change exactly like the CLI's `--apply`. **Undo** restores
the newest before-state; **Backups** lists all of them. Macro recording captures
keys and mouse buttons pressed inside the macro panel while the window has focus.
Custom-picture colors are editable only once that picture is the one selected on
the keyboard, because a read captures only the selected picture.

**Logs** lists every read, apply, undo, import and export, and every failure
with its full error chain, including the changes that were pending. It stays
reachable while the keyboard is disconnected. Logs show only the current app
session (newest 1000 entries). The session is also written to
`$XDG_STATE_HOME/monsgeek-linux/gui.log` and resets at launch. Crashes are recorded
before the process exits. The chosen light or dark theme
is saved in `$XDG_CONFIG_HOME/monsgeek-linux/gui.json`; with no saved choice the
app follows the system appearance.

### Firmware (experimental)

**System > Firmware (experimental)** loads a vendor firmware file and shows a
short warning before flashing. USB only. The CLI previews it with
`monsgeek-linux firmware firmwareFile.bin`. Wrong firmware or interruption can
brick the keyboard. See [the firmware guide](docs/FIRMWARE.md) for the file
format, explicit apply flags and limits. No hardware was flashed during development.

### Wireless

Both the CLI and the GUI configure the keyboard over the USB cable, the 2.4 GHz
receiver (`3151:4011`) or Bluetooth, preferring them in that order. The command
packets are identical; only the framing differs (see
[PROTOCOL.md](docs/PROTOCOL.md#8-wireless-transports)). Over 2.4 GHz each request
takes about 130 ms, so a full read takes about 35 seconds; the GUI shows
progress, and Apply re-reads only what the changes touch (three changes applied
and verified in about 16 seconds). Wake the
keyboard with a key press if it is asleep.

When the device access rule is missing or older than the one this build ships,
the GUI offers to install it at launch (System › Access later), showing the exact
file; it asks for the password through polkit, or opens a terminal with `sudo`
when the session has no polkit agent. In an interactive terminal the CLI offers
the same before device commands (`y`, `N`, or `never` for this rule version);
`monsgeek-linux install-rule` installs it directly.

The GUI checks every two seconds for the cable, the receiver and the keyboard's
wake state, reads automatically when the keyboard appears or wakes, and shows
the battery in the sidebar. `monsgeek-linux battery` prints it from the receiver.

**Bluetooth is experimental and untested on hardware.** Pair with Linux first,
switch to Bluetooth mode and wake the keyboard. The GUI shows a persistent
warning. The CLI warns when opening Bluetooth; select it explicitly with
`--transport bluetooth` or `--device /dev/hidrawN`. USB and the receiver otherwise
take priority. Use USB if Bluetooth fails. Existing backup and readback checks
also apply over Bluetooth.

```sh
monsgeek-linux --transport bluetooth inspect
monsgeek-linux --transport bluetooth snapshot bluetooth-before.json
monsgeek-linux --transport bluetooth battery
```

## Arch Linux package

`packaging/arch/PKGBUILD` builds two packages from this source tree:

- `monsgeek-linux`: the CLI, the access rule in `/usr/lib/udev/rules.d/` and the
  docs. pacman's udev hook applies the rule to connected devices immediately.
- `monsgeek-linux-gui`: the app with its desktop entry and icon.

```sh
cd packaging/arch
makepkg -si
```

The package's rule makes the in-app and CLI rule prompts unnecessary. A copy
installed earlier in `/etc/udev/rules.d/` overrides it; the package says so on
install, and removing that copy lets upgrades take effect.

## Build and offline use

Install Rust and the libudev development files. On Arch, `rust` and `systemd`
provide these; on Debian/Ubuntu use `cargo`, `rustc`, `pkg-config`, and
`libudev-dev` (a recent Rust toolchain with edition 2024 support is needed).

```sh
cargo build --release
cargo test
cargo clippy --workspace --all-targets -- -D warnings
```

These examples are previews only and work with the keyboard disconnected:

```sh
./target/release/monsgeek-linux light --mode 1 --color ff8040
./target/release/monsgeek-linux light --mode 4 --rainbow --speed 3 --option 2
./target/release/monsgeek-linux remap --from capslock --to leftctrl --layer 0
./target/release/monsgeek-linux profile 1
```

Key arguments accept letters (`a`), common names (`capslock`, `leftctrl`,
`rightalt`, `space`, `tab`, `enter`, `esc`, arrows), or USB keyboard usage numbers
in decimal / `0x` hex. A bare `1` means **usage 1**, not the digit key, and is
rejected; the digit 1 key is usage `30` (`0x1e`). Remapping identifies physical
keys using the vendor's matrix, not their currently assigned values. `--fn-layer`
targets the Fn mapping; this does not change the physical Fn key itself.
Vendor layer indices are **0..3**, not assumed to be four independent hardware
profiles. The relationship to the keyboard's Windows/macOS modes needs testing.

### Onboard lighting

| ID | Effect | ID | Effect |
|---|---|---|---|
| 0 | Off | 10 | Sine wave |
| 1 | Constant | 11 | Kaleidoscope |
| 2 | Breathing | 12 | Line wave |
| 3 | Neon | 14 | Laser |
| 4 | Wave | 15 | Circle wave |
| 5 | Ripple | 16 | Dazzling |
| 6 | Raindrop | 17 | Rain down |
| 7 | Snake | 18 | Meteor |
| 8 | Press action | 19 | Press action off |
| 9 | Coverage | | |

Mode13 is custom picture, with options0..2. RGB export captures only the selected
picture; select and export each separately. See the usage guide before restoring
RGB across picture selections.

Brightness and UI speed are 0..4. The vendor sends wire speed `5 - UI speed`.
Option values: wave 0..3; snake, kaleidoscope, line wave, circle wave 0..1;
others 0. The actual visual directions need hardware validation. Some effects
ignore color, speed, or rainbow fields. Original M1W rainbow/custom RGB flags
are **7/8**, not the **8/7** defaults of the shared vendor class.

## First hardware validation

Use a USB **data** cable, wired mode, and initially Windows mode. Close any other
keyboard configuration software. USB is recommended for initial validation.
Bluetooth configuration is experimental. No settings writes occur with these commands:

```sh
./target/release/monsgeek-linux list
./target/release/monsgeek-linux inspect
./target/release/monsgeek-linux snapshot m1w-initial.json
```

`list` only enumerates devices. `inspect` and `snapshot` send query feature
reports but do not intentionally change stored settings. A mismatch or an
unexpected reply stops the operation. Do not bypass identity checks for another
revision. If access is denied, install the narrowly scoped rule below, then
unplug and reconnect. Do not run the entire configurator as root.

```sh
sudo install -m644 udev/70-monsgeek-m1w.rules /etc/udev/rules.d/70-monsgeek-m1w.rules
sudo udevadm control --reload-rules
```

This grants the active local login access to interface 2 on `3151:4015` (cable)
and `3151:4011` (2.4 GHz receiver), plus the Bluetooth keyboard's single hidraw
node; it does not make all HID devices world-writable. Boards sharing that ID may also match
the permission rule; the program still checks internal ID 983 before configuring.
For experimental firmware updates, the rule also grants interface0 on the
reference's shared boot-keyboard IDs. The updater requires the original USB port.

### Explicit setting writes

All three flags are mandatory. The before-state file must not exist; saving it
must succeed before a setting is sent. It contains a snapshot and the inverse
of this specific change. Files are created owner-readable/writable (0600).

```sh
./target/release/monsgeek-linux light --mode 1 --color ff8040 \
  --apply --allow-experimental-writes --before before-light.json
```

The tool waits two seconds after sending, then reads the setting back. A mismatch
is an error, not success. A write/read failure can leave the setting changed;
there is **no automatic retry or rollback**. Inspect before proceeding. The
snapshot is evidence of the previous state, not a guarantee of recovery.

Preview undo first, then explicitly apply it using a different before-state file:

```sh
./target/release/monsgeek-linux undo before-light.json
./target/release/monsgeek-linux undo before-light.json \
  --apply --allow-experimental-writes --before before-undo.json
```

Remaps temporarily select the chosen vendor layer because this firmware ignores
the write packet's layer field, then restore the original layer after verification.
Test physical behavior and persistence after reconnecting, not just readback.
Keep another keyboard available
during validation. A device-node advisory lock rejects concurrent instances of
this configurator; close unrelated keyboard software that may not honor that lock.
See [the usage guide](docs/USAGE.md) for macros, RGB, bindings, options and restore.

## Protocol provenance

Source: [MonsGeek's download page](https://www.monsgeek.cn/download/), macOS
`MonsGeek-Driver-v4500.3.3_MAC20260923.rar` (app version `500.3.3`).

Archive SHA-256:
`7d74ef299cc5c2360e700ccb391b615fb8aecd6e30931499e6fa13617363a4e0`.

The extracted host code is retained locally under `.amp/in/vendor/` for research,
not distributed as this project's source. Relevant vendor modules:

| Module | Evidence |
|---|---|
| `index.1970d80c.js` | M1W entry 983, USB interface filter, identity query, checksum encoder, lighting capability limits |
| `b7c9f14f.js` | `yc500_m1wnosled_soc` dispatch to class `Z`, base class `s` color overrides, M1W key matrix |
| `cb8a3a35.js` | `CommonKbYc500` lighting, layers, single-key and Fn writes, keymap reads |
| `43a0ab9f.js` | USB revision query, Fn reads, two-second write settling time |

Reports contain 64 payload bytes, report ID 0. HIDAPI sends an extra leading ID
byte. Normal commands store `255 - (sum(bytes[0..7]) & 255)` at byte 7; lighting
stores `255 - (sum(bytes[0..8]) & 255)` at byte 8. These are complement sums,
not two's complement. Keymap replies are raw 64-byte pages without echoed opcode.
The framing/reply shapes and reversible setting writes have been exercised on
this M1W. Macro writes required preserving the RGB picture against an observed
firmware side effect. See [docs/VALIDATION.md](docs/VALIDATION.md) for executed
cases, failed runs, fixes and remaining physical/persistence checks.

An existing project, [Sharkfin](https://github.com/dniminenn/sharkfin), also lists
the original M1W, but currently marks its protocol family unknown. It was consulted
as a cross-check; this implementation was reconstructed from the official package,
not copied from its driver. Extending an established configurator remains an option
after this keyboard's protocol has been validated.

## License

[MIT](LICENSE). The GUI embeds the Geist and Geist Mono fonts, which are under
the [SIL Open Font License 1.1](gui/assets/fonts/OFL.txt). The MonsGeek vendor
software is not part of this repository or its license.
