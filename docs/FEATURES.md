# Profiles, testing, desktop integration and host remapping

These additions extend the original M1W configurator. They do not replace its
firmware. The optional host service changes input on Linux while it is running;
onboard configuration still uses the existing identity, backup and readback gates.

## Named profiles and readable review

Open **Profiles → Save current draft**, enter a name, and save. Profiles include
the current draft, including unapplied edits. They live in
`$XDG_CONFIG_HOME/monsgeek-linux/profiles/` (default `~/.config/...`) as owner-only
JSON files. Duplicate creates another local profile; it never touches HID.

Choose which groups to load: keys, macros, lighting, performance, keyboard options,
and active layer. **Load for review** stages the selected groups, preserving all
others. Firmware revisions must match. Load keys and macros together when saved
bindings depend on the profile's macro bodies.

The **Changes** page highlights affected positions and shows before/after values,
including exact per-slot RGB colours and decoded macro events. **Apply changes**
performs the existing verified transaction and saves its before-state. The header
now leads to review rather than sending changes immediately. Undo retains the
existing behaviour of restoring the most recent applied state.

Profiles capture one RGB picture. If a profile uses a different selected picture,
first select it on the keyboard, Apply, and reload before loading its colours.
Alternatively, exclude lighting. Loading never silently substitutes another
picture's RGB data.

Offline CLI equivalents:

`profiles` (plural) manages named local files; the existing singular `profile`
command still selects an onboard layer.

```sh
monsgeek-linux profiles save Coding current-snapshot.json
monsgeek-linux profiles list
monsgeek-linux profiles duplicate /path/to/profile.json 'Coding copy'
monsgeek-linux profiles draft /path/to/profile.json current-snapshot.json draft.json \
  --only keys,macros
monsgeek-linux review current-snapshot.json draft.json
monsgeek-linux restore draft.json --current current-snapshot.json
```

The last command remains a preview. The existing explicit write flags and a new
before-state path are required to apply. Profile paths from `profiles list` are
opaque generated filenames; names are stored inside the files.

## Keyboard tester

Open **Keyboard tester → Start testing**. The focused capture panel displays key
presses/releases, modifier state, mouse buttons, scroll events, and any knob/media
events delivered to the window. The board highlights **emitted key names**, not
the original physical switch positions. Remapping Caps Lock to Ctrl therefore
shows Ctrl. Modifier sides and Fn may not be exposed by the window system, and
desktop-intercepted media shortcuts may not arrive.

Clicking elsewhere stops capture and clears held state. **Clear** resets the
in-memory history and seen keys. Tester events never enter the activity log,
diagnostics, profile files, or keyboard configuration. The tester receives input
from the window, so it does not attribute events to one particular physical board.

## Complete RGB backups

**Backups → Back up all three pictures** requires a clean draft and confirmation
of its temporary selection changes. Choose a new destination. Before changing
selection, the app saves a fresh recovery snapshot. It selects/readbacks each of
the three pictures, attempts to restore the original lighting even if a read
fails, and saves a backup only after successful restoration.

Open a backup with **Open picture backup**. To restore colours, select the desired
picture on the keyboard and reload, then choose that picture's **Load colours
for review**. The ordinary Apply/readback path writes the colours. Restore each
picture separately; the format is not an atomic all-picture restore operation.
Keep the recovery snapshot after a failed capture; restoration can fail if the
keyboard disconnects. A picture backup is not a firmware recovery image.

```sh
monsgeek-linux pictures all-pictures.json
# Only after reviewing the temporary picture-selection writes:
monsgeek-linux pictures all-pictures.json \
  --apply --allow-experimental-writes --before picture-before.json
```

## Desktop controls and diagnostics

**Desktop** provides:

- Low-battery notifications, with a threshold of 5–50%. One alert is sent per
  low-battery cycle; charging above the threshold plus five points rearms it.
  Wireless must expose a battery level. Notifications use the desktop's D-Bus
  service.
- An optional StatusNotifier tray icon, disabled by default. Enable **Show tray
  icon** in **Desktop**. Its menu opens the window, loads a
  profile **for review**, stops host remapping, or quits. Desktops without a
  StatusNotifier host can keep using the window.
- **Open at login**, which creates/removes this app's user XDG autostart entry.
  It starts the GUI, not the host input service. Nothing installs at development
  time; the toggle acts only when clicked.
- **Export diagnostics**, available offline too. Reports include application
  version, architecture, model/revision when known, transport, pending-operation
  count, and operation error titles. They omit raw keymaps, macros, input history,
  USB serials and raw error chains/personal paths.

CLI support report:

```sh
monsgeek-linux diagnostics --snapshot current-snapshot.json > diagnostics.json
```

The firmware picker also identifies the researched original-M1W v108 image by
its exact SHA-256. An unknown hash stays **Unrecognized image**; filenames and
file size do not establish a model. A hash match identifies the retained vendor
download, not a cryptographic signature or tested recovery guarantee. Flashing
remains experimental and uses its separate confirmation flow.

## Optional Linux host remapping

This is an experimental evdev/uinput service. It captures **only explicitly
selected event devices**, creates a virtual input device, and applies:

- **Tap-hold:** tap Caps Lock for Escape; hold it for Ctrl. A second key commits
  the hold before that key; otherwise the hold starts at its timeout.
- **Tap dance:** choose actions for one through four taps. The action fires after
  the final release's quiet interval, or before another unrelated key is pressed.
  Holding the source waits for release; it is not an additional hold action.
- **Chords:** two through four keys pressed within a configured window trigger
  an action. The action releases when the first member releases. Unmatched keys
  pass through, delayed only by the chord window. Chord sources cannot overlap
  other source rules or chords in this implementation.
- **Application rules:** match the focused app ID/class; a held action retains
  its original mapping through focus changes. More specific application rules
  override global rules. Missing focus information never enables an application
  rule globally.
- **Launch actions:** execute a program with an argument vector, without shell
  expansion. A held/repeating launch key starts it only once per press.

The example below is generated by the application, not activated automatically:

```sh
monsgeek-linux host devices
monsgeek-linux host example host.json
# Edit devices to list this keyboard's actual event nodes.
monsgeek-linux host check host.json
monsgeek-linux host run host.json
# The preceding command only validates. This explicitly starts capture:
monsgeek-linux host run host.json --activate
```

**Host remapping** in the GUI offers an advanced configuration editor, file
open/save, validation, device listing, and Start/Stop. Its Start dialog explains
the capture. Rules stay editable without hardware; validated status is cleared
when the editor changes. The CLI and GUI use the same engine. Closing the GUI
terminates the service it started, including unexpected parent exit. A CLI-run
service can be stopped with Ctrl+C, SIGTERM, or the physical emergency chord
**Ctrl+Alt+Esc**. No keystrokes are logged. Disconnects or dropped input events
stop the service and release synthetic keys; it does not silently re-grab a
replacement device. LED state on the virtual keyboard is not mirrored back to
the physical keyboard in this implementation.

### Configuration and permissions

Key numbers are Linux input codes, **not USB HID usages**. For example:
Esc=1, Ctrl=29, Shift=42, Alt=56, Space=57, Caps Lock=58, F11=87, F12=88,
Mute=113, Play/Pause=164, F13=183. The generated example includes tap-hold,
tap dance, an app-specific shortcut, a launch action and a J+K chord.

The service needs read access to each selected `/dev/input/eventN` and write
access to `/dev/uinput`. The existing configurator's hidraw rule does **not**
grant those permissions. For a temporary session, grant the desktop user access
to the selected nodes using your system's ACL mechanism, for example:

```sh
# Substitute the event node chosen above. This is not run by the application.
sudo setfacl -m "u:$(id -un):r" /dev/input/eventN
sudo setfacl -m "u:$(id -un):rw" /dev/uinput
```

Release all keys/buttons before starting. Choose the keyboard's keyboard and
consumer/media event nodes as needed; do not select its virtual remapper output.
The service rejects duplicate event nodes and absolute/touch/switch devices.
Permissions and device nodes can change across unplug/replug.

Focus queries support niri, Sway, Hyprland and native X11. Other Wayland desktops
need an explicitly configured `focus_command` returning an app ID on stdout.
Focus queries run on a separate worker with a time/output limit; they do not
block key processing. The cached focus can lag by a query interval, so do not use
application matching as an authorization or security boundary.

Host remapping changes emitted input, so it composes with onboard remaps: configure
rules for the keys Linux actually receives. It does not make the keyboard run QMK,
implement arbitrary firmware layers, or carry these behaviours to another host.

## Validation boundary

Deterministic tests cover timing, interrupted holds, chord fallback/order,
tap-dance completion, application changes during held actions, modifier sharing,
launch events, input validation and synthetic-key release across randomized
sequences. Profile/RGB tests cover partial restore, firmware mismatch, and picture
identity; diagnostics tests check configuration/input omission. GUI tests cover
draft staging, rendering and tester focus cleanup. Headless screenshots cover
light/dark, narrow supported windows, long names, dialogs and error states.

Physical evdev/uinput capture, notifications/tray delivery, desktop focus providers,
login startup and the three-picture capture flow still need user-session/hardware
validation. No input devices, access rules, autostart entries or keyboard firmware
were changed while implementing these features.
