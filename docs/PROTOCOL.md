# MonsGeek configuration protocols

Sections1–8 document original mechanical M1W observations; [section9](#9-model-aware-families-and-snapshots) describes the current catalog-wide implementation.

All byte offsets are zero-based in the **64-byte feature payload**, excluding the
HID report-ID prefix. Hexadecimal opcodes below are always prefixed `0x`. Unless
stated otherwise, fields not listed are zero and evidence is **official host code**.
Source ownership and exact research locators are in [SOURCES.md](SOURCES.md).

## 1. Identity and transport

Target USB VID:PID `3151:4015`, USB interface **2**, usage page `0xFFFF`, usage
`0x0002`. Query internal model ID before any other configuration operation:
`0x8F` reply bytes 1..4, little-endian u32, must be **983** (`D7 03 00 00`).
Do not open typing/consumer interfaces. `/dev/hidraw7` was the observed node,
not a permanent device name. Firmware/USB revision query `0x80` returned u16LE
bytes 1..2 = `08 01` (`0x0108`); this is not necessarily a marketing version.

Observed report descriptor (unnumbered 64-byte feature report):

```text
06 ff ff 09 02 a1 01 09 02 15 80 25 7f 95 40 75 08 b1 02 c0
```

HIDAPI sends `[0, payload[0..64]]` (65 bytes); zero is report ID, **not opcode**.
Linux may return 64 unnumbered bytes or a 65-byte report retaining the ID; normalize
only these two shapes. A GET query is itself sent with USB **SET_FEATURE**, followed
by GET_FEATURE. “Read-only” means no configuration-setting opcode, not no USB writes.

### Header checksums

- **Bit7**: byte 7 = `255 - (sum(bytes 0..6) & 255)`.
- **Bit8**: byte 8 = `255 - (sum(bytes 0..7) & 255)`; used by lighting writes.
- These cover the header only, not bulk payload data. Not a CRC, not two's complement.
- Vendor encoder pads short commands to at least nine bytes. Rust pads to 64.
- Do **not** apply request checksum rules to responses. Observed reply byte 7
  sometimes retains the request checksum; for lighting it is blue, for bulk data
  it is arbitrary data.

Most scalar replies retain the opcode at byte 0. Matrix, RGB and macro replies
are concatenated as raw data, without stripping an opcode/header. A matching
opcode alone does not prove success: `0x85` with layer zero and `0x97` with zero
returned the exact request in the read-only capture. Those are ambiguous echoes.

Existing Rust transport spaces sends/reads by 10 ms. Vendor macOS `vendorSleep()`
waits **2000 ms** after many writes. It is a host delay, **not a save/commit packet**.
Successful transport return is not firmware acknowledgment or persistence proof.

## 2. Command inventory

`B7` / `B8` mean checksum placement above. Arguments are subsequent payload bytes
unless otherwise specified. Reads use B7. Reversible setting-write evidence is
recorded in [VALIDATION.md](VALIDATION.md); layouts below distinguish host behavior
from deliberate Rust corrections.

| Feature | SET | GET | Request / response contract |
|---|---|---|---|
| Internal model | — | `0x8F` | Reply u32LE at 1; read observed 983 |
| USB revision | — | `0x80` | Reply u16LE at 1; read observed `0x0108` |
| Vendor layer | `0x05` | `0x85` | Layer at 1, 0..3; nonzero round trip observed |
| Keyboard options | `0x06` | `0x86` | Selector1, bitfield2, Fn3, power4; see §6 |
| Main lighting | `0x07` B8 | `0x87` | Mode1, speed2, brightness3, flags4, RGB5..7 |
| Base matrix bulk | `0x09` | `0x89` | SET layer1,length2..3,page4,data8; GET layer1,page2 |
| Fn matrix bulk | `0x10` | `0x90` | Same layout as base bulk |
| Debounce | `0x11` | `0x91` | SET value2; reply value2, read observed 2 |
| Wireless sleep | `0x12` | `0x92` | SET four u16LE at8; reply four u16LE at1 |
| Single base binding | `0x13` | `0x89` | Layer1, slot2, binding8..11; verify via full read |
| Single Fn binding | `0x15` | `0x90` | Layer1, slot2, binding8..11 |
| Macro body | `0x16` | `0x8B` | Slot1, paging differs by direction; see §4 |
| Automatic OS detection | `0x17` | `0x97` | Boolean1; enabled response and restoration observed |
| Custom RGB picture | `0x0C` | `0x8C` | Selector zero, paging differs by direction; see §5 |

Do not infer undocumented GETs by adding `0x80`, or probe every opcode. Generic
side-light methods `0x08/0x88`, magnetic features, report-rate controls, firmware
updaters and screens in shared classes are **not established original-M1W features**.

## 3. Matrix slots, layers and bindings

Each map is **512 bytes = 128 four-byte tuples**. Source-key lookup uses the
concrete original default matrix, **not the current assignment or visual position**.
On revision0108 a single-write packet targeting inactive layer3 changed active
base0 instead. Rust temporarily selects the requested layer, writes/verifies and
restores the original active layer. Base3 and Fn2 round trips passed this way.
Ordinary keyboard source usage U matches `[0,0,U,0]`; special encoded identifiers
are split into four big-endian bytes. Duplicate matches use an occurrence index.
All three concrete default matrices are preserved in [m1w-reference.json](m1w-reference.json).

Examples: Esc slot0, Caps Lock slot3, left Ctrl slot5, knob press slot6,
Fn slot65, volume-down rotation slot94, volume-up rotation slot95.
Knob center default is `[0x0A,0x0E,0,0]`, **volume/keyboard-brightness toggle**, not
mute despite metadata calling it `AudioVolumeMute`. Its encoded source identifier
is `0x0A0E0000`. Fn default is `[0x0A,1,0,0]`.

Metadata declares `layer:4`, `fnSysLayer:{win:2,mac:2}`. Do not assume four
independent profiles or derive Win/Mac switching from this alone. Reads found base
layers0..3 identical; Fn0=Fn1 and Fn2=Fn3 but the pairs differ. No switch experiment
has established their physical meaning. CLI's `profile` means vendor layer index.

### Reads and writes

- Read: `[0x89 or 0x90, layer, page, 0,0,0,0,ck,…]`, page0..7.
  Concatenate eight full raw 64-byte replies. Read observed on all layers.
- Single SET: `[0x13 or 0x15, layer, slot, 0,0,0,0,ck, B0,B1,B2,B3,…]`.
  Slot0..127. No separate save packet; public method sleeps after send.
- Bulk SET: `[0x09 or 0x10, layer, 0xF8,0x01,page,0,0,ck, data56]`, page0..8.
  **Only 504 bytes / 126 slots are transported although maps contain 512 bytes.**
  Do not use this as a lossless 128-slot restore or invent a tenth packet.
- M1W is nonmagnetic: do not multiply the layer by four as the shared class does
  for magnetic devices.
- Single Fn setter ignores the extra system argument supplied by some callers.
  Its restore-default special case uses `defaultFnMatrix`. Concrete property
  `defaultFnMACMatrix` differs in capitalization from base `defaultFnMacMatrix`;
  a declared Mac default does not establish that runtime selects it.

### Four-byte binding codec (`Xi`)

| Binding | Bytes |
|---|---|
| Ordinary / combo | `[0, skey, key, key2]`; three **usage codes, not modifier masks** |
| Disabled | `[0,0,0,0]`; reader also considers `[0,0,3,0]` forbidden |
| Consumer/media | `[3,0, consumer_usage_low, consumer_usage_high]` |
| System control | `[2,0, usage,0]` |
| Macro | `[9, playback_mode, macro_slot,0]` |
| Vendor function | Raw tuple; preserve unknown values |

Examples: Ctrl+C `[0,0,224,6]`; Ctrl+Shift+Esc `[0,224,225,41]`;
volume down `[3,0,234,0]`; up `[3,0,233,0]`; mute `[3,0,226,0]`;
play/pause `[3,0,205,0]`; calculator `[3,0,146,1]`;
system sleep `[2,0,130,0]`. Fn `[10,1,0,0]`; right Fn `[10,1,1,0]`;
profile exchange `[8,0,4,n]`, n0..3, from the generic table only.

Generic table mouse binding codes include left..forward `0xF0..0xF4`, but binding
support for every generic mouse/function entry has not been demonstrated on M1W.
Unknown existing tuples must round-trip verbatim, particularly vendor wireless,
OS, reset and lighting Fn actions. Do not replace them with disabled bindings
merely because the CLI lacks a name. Do not exercise reset-like Fn actions.

## 4. Macros

### Stored payload and event codec

Host serializer allocates **256 bytes**: repeat count u16LE at0..1, then records,
then zero padding. Input host event list alternates **event, delay**. Each delay
belongs to the preceding event. Validate length and released keys explicitly;
vendor odd-length lists can throw rather than reject cleanly.

For keyboard usage or mouse-button code K, delay d:

| Case | Record |
|---|---|
| 1 ≤ d ≤127 | `[K, (down ? 0x80 : 0) OR d]` |
| 128 ≤ d ≤65535 | `[K, down ? 0x80 : 0, d_low, d_high]` |
| Mouse move, 1 ≤ d ≤127 | `[0xF9, d, dx, dy]` in writer |
| Mouse move, d ≥128 | `[0xF9,0,dx,dy,d_low,d_high]` |

Mouse buttons left/right/middle/back/forward are `0xF0..0xF4`; keyboard reader
recognizes usages4..239. Movement coordinates are signed i8 encoded as bytes.
**Zero delay is ambiguous**: writer emits short form, reader interprets low seven
bits zero as an extended delay. **Movement short-delay mismatch**: writer emits
d directly but reader uses `d >> 1`. Rust uses the extended form for all zero
delays and movements. Raw storage round trips are tested; firmware playback and
the vendor short-movement delay interpretation remain separate uncertainties.

UI normal limit counts at most248 event bytes; shared-macro checker allows256;
actual buffer leaves **254 event bytes** after repeat count. UI checks omit repeat
bytes. Rust supports254 event bytes with checked capacity; a full256-byte stored
image passed hardware readback/clear. UI repeat-times range1..65535; recorded
delay is capped at65535. Host controller allocates **50 slots0..49** across base
and Fn bindings; this is not proof of physical capacity.

### Macro writes and readback

Actual inherited writer is `CommonKbYc500._setMacro`, **opcode0x16**:

```text
0:16  1:slot  2:page  3:38  4:last  5:00  6:00  7:checksum
8..63: 56 data bytes
```

Page starts at0; byte4=1 only on final sent page. At most five pages; page4 carries
the final32 bytes of the256-byte storage image then24 zeros. After all sends, wait
2000ms. Macro upload is separate from assigning a binding. Binding playback mode:
0 repeat-count, 1 toggle on/off, 2 play-while-pressed-style (exact physical behavior
unverified). Repeat count is stored regardless of playback mode.

**Vendor writer bug:** counts nonzero chunks rather than final nonzero chunk
index+1, then sends that many pages starting0. A zero intermediate chunk can omit
later data; all-zero payload sends no packet and reports success. Do not reuse
this count algorithm or claim empty-slot clearing is understood. Correct page
selection and deletion must be verified against firmware before enabling writes.

GET `[0x8B,slot,page,0,0,0,0,ck,…]`, page0..3, concatenates full64-byte responses
to256 bytes. Vendor stops early if **any four consecutive zero bytes anywhere in
a response**, which can stop before meaningful later data; parser also stops on
four-zero lookahead and filters zero delays. Preserve all raw pages; an incomplete
or malformed reply must not silently become an empty macro. Rust reads all four
pages for every slot, including unreferenced/empty slots. Five write pages versus four read pages is due
to56 versus64 payload bytes, not interchangeable page numbering.

**Implemented deviation and hardware finding:** Rust always sends five pages,
including zero gaps and clears, declaring length32 rather than56 on the last.
Both final-length variants stored the macro correctly but changed unrelated RGB
on revision0108; the underlying cause is unresolved. This affected picture0 even
when picture1 was selected. Apply therefore temporarily selects picture0,
captures/restores its whole map, verifies macro and RGB and restores the original
effect. Exact raw clear/full/gapped round trips are in validation.

## 5. Lighting and custom per-key RGB

### Ordinary onboard effects

SET0x07 fields: mode1; wire speed2 = `5 - UI speed`; brightness3; flags4;
red5, green6, blue7; **B8 checksum**. GET0x87 uses B7 and returns same fields.
UI speed/brightness0..4. Original M1W overrides **DAZZLE=7, NORMAL=8** (shared
YC500 has different values). Normal flags = `(option << 4) | (rainbow ? 7 : 8)`.
Host substitutes white `FFFFFF` with `FAFFFA`; retain raw readback for restoration.

| Mode | Effect | Mode | Effect |
|---|---|---|---|
| 0 | Off | 10 | Sine wave |
| 1 | Constant | 11 | Kaleidoscope |
| 2 | Breathing | 12 | Line wave |
| 3 | Neon | 13 | Custom picture |
| 4 | Wave | 14 | Laser |
| 5 | Ripple | 15 | Circle wave |
| 6 | Raindrop | 16 | Dazzling |
| 7 | Snake | 17 | Rain down |
| 8 | Press action | 18 | Meteor |
| 9 | Coverage | 19 | Press action off |

Wave option0..3; snake/kaleidoscope/line/circle option0..1; other basic modes0.
Some effects ignore fields; actual direction/color behavior still needs observation.
Shared mode table additionally names20 music3,21 screen,22 music2,23 train,
24 fireworks,66 user-color. Original-M1W `ct` metadata exposes21 effects:
modes1..19, screen21 and audio22; off0 is available in the driver table. It does
not expose20/23/24/66. Audio/screen are genuine native-service paths, but their
live HID codec remains unresolved; see [STREAMING.md](STREAMING.md).

Special setter flags: picture13 = `option << 4`, forced RGB `[0,200,200]`;
music20/22 = `(option << 4) | (rainbow ? 0 : 4)`; screen21 =0.
Those special flags are not the basic-effect7/8 flags.

### Custom RGB picture

128 slots ×3 = **384 bytes**, R,G,B at3×slot. Lookup uses the same original-default
matrix as remapping. Unspecified entries become black: upload is **whole-picture
replacement**, not sparse modification. Read/modify/write requires a complete
successful read, preserving all slots including those with unknown physical use.

SET pages0..6:

```text
0:0C  1:00  2:80  3:01  4:page  5:00  6:00  7:checksum
8..63: 56 RGB data bytes
```

Header length `0x0180` =384. Seven pages carry392 bytes; last page has48 RGB
bytes plus8 zeros. Selector fixed0; second host argument ignored. No final-page
flag or extra commit packet; sleep after all seven writes.

GET `[0x8C,0,page,0,0,0,0,ck,…]`, page0..5: concatenate six raw64-byte replies.
Read/upload/restore have been observed; physical color order still needs observation.
Vendor substitutes missing pages with black: **do not copy this behavior** into
a backup or read/modify/write path. Return an error and retain partial raw evidence.

Uploading bytes does **not** activate picture mode: select lighting mode13
separately. On revision0108 selecting picture2 changes GET_USERPIC's returned map;
selecting0 returns the original map. Changing the GET selector to1/2 did not
select those other maps. Read-only snapshots therefore contain only the selected
picture. Mixed RGB/picture-selection transactions are refused because they cannot
back up an inactive picture before editing it. Select first, export, then edit.
Independent edits to pictures1/2 survived switching away/back; exact originals
and picture0 were restored in hardware tests. Separate macro write/clear tests
with pictures1/2 selected compared all three saved pictures successfully after
the preservation fix. These tests do not establish power-cycle persistence.
Animated GIF storage and mode66 are not established original-M1W features.

## 6. Debounce, sleep and keyboard options

Debounce SET `[0x11,0,value,…]` B7, GET0x91 reply byte2. Vendor default UI
range1..10; metadata `deBounce:10` is not a measured current value. Read observed2.
Treat units as vendor debounce units until physical timing establishes milliseconds.

Sleep SET0x12 writes four u16LE values at offsets8,10,12,14 respectively:
Bluetooth sleep, 2.4GHz sleep, Bluetooth deep sleep, 2.4GHz deep sleep. GET0x92
returns those at1,3,5,7 respectively, **not the SET offsets**. Values are seconds;
model metadata sleep60..3600, deep600..3600. Observed120,120,900,600. Shared getter
has fallback defaults on a missing response; backups must report read failure
instead of manufacturing defaults. Timers configured over USB do not establish
Bluetooth/receiver configuration transport support.

Keyboard options SET0x06 / GET0x86: selector1, option bits2, Fn-related byte3,
power-save byte4. Read selector0 returned raw `[0x10,0x04,0x00]` in bytes2..4.

| Byte2 bit | Shared setter meaning | Reader agreement |
|---|---|---|
| 0 | Windows-key lock | Reads mask1 |
| 1 | Setter leaves zero | Reader uses mask2 for macOS |
| 2 | `Number(system) << 2` | **Disagrees with reader's bit1** |
| 3 | WASD/arrows exchange | Reads mask8 |
| 4 | Main LED off | Reads mask16 |
| 5 | Side LED off | Reads mask32; no M1W side-LED support established |
| 6 | Keyboard mode | Reads mask64; physical semantics unresolved |
| 7 | Keyboard lock | Reads mask128 |

Setter serializes `Number(system)` but getter returns strings `"win"`/`"mac"`;
converting those produces NaN then zero in bitwise expressions. Getter turns
byte3 into Boolean via mask1, although observed raw byte3=4. Rewriting the getter's
decoded object can therefore lose state. **No safe full option rewrite is defined.**
Resolve OS/Fn bit semantics first; preserve raw unknown bits and selector. Do not
infer all listed generic bits affect this hardware.

Automatic OS detection SET0x17 bool1; GET0x97 bool1. Its observed zero reply was
initially identical to the request. A later reversible enabled test returned a
distinct true value, then restored false. Storage/query support is now observed;
automatic switching and persistence remain physical validation questions.

## 7. Persistence, error handling and exclusions

No separate save command was found in the documented setting paths. Only macro
byte4 is a final-page marker. When firmware commits flash, whether power-cycle
retention is guaranteed, and how partially sent multipart operations recover
remain unresolved. A timed-out send may already have changed state: do not retry
or automatically rollback without inspecting it.

An experimental USB firmware updater is now implemented from the inherited
macOS host path; its separate destructive flow and wire contract are in
[FIRMWARE.md](FIRMWARE.md). No hardware flashing has been validated. Reset/erase
commands, cable battery telemetry and native streaming remain outside these
configuration tables. Original mechanical M1W has no magnetic actuation features.

## 8. Wireless transports

### 2.4 GHz receiver

Receiver `3151:4011` interface 2 (usage `ffff:0002`, unnumbered 64-byte feature
report). Status query: SET feature `[0xF7]` zero-padded, then GET feature; no
checksum and no opcode echo. Reply bytes: 0 readable, 1 keyboard battery %, 2 mouse
battery %, 3 keyboard offline (0 = online), 4 mouse offline, 5 can-send, 6 linked
devices (1 keyboard, 2 mouse, 3 both), 8 RF boot. Observed with a linked M1W:
asleep `00 00 00 01 01 01 00`, awake `00 60 00 00 01 01 01` (96 %). The vendor
host reports a fixed 100 % over USB cable; no cable battery query exists.

Configuration through the receiver uses the unchanged 64-byte packets as feature
reports, gated by status. Once per session `[0xF6, 0x0A]` routes traffic to the
keyboard (`0x05` mouse). Send: poll status until byte5 can-send = 1, wait 10 ms,
SET the packet. Reply: poll status until byte0 readable = 1, wait 10 ms, SET
`[0xFC]` (notify read), wait 10 ms, GET the reply (same layout as over cable,
including opcode echo). A write leaves no pending reply (byte0 stays 0); can-send
drops for about 70 ms. Observed on receiver `3151:4011`: identity 983, revision
`0x0108`, lighting, keymaps (6 of 8 maps equal the factory defaults slot for
slot), consecutive full snapshots byte-identical, about 100 ms per exchange
(130 ms with the vendor's 10 ms gaps; about 35 s per full snapshot). Writes:
lighting brightness, debounce and a layer-4 remap (which switches and restores
the active layer) applied with read-back in 16 s, undone in 21 s, and a final
full snapshot equal to the baseline byte for byte.

### Bluetooth LE (experimental, vendor host framing, not yet observed)

Vendor collection usage page `0xFF55` or `0xFF66`, usage `0x0202`. Output report
`[0x55, packet…]`; replies arrive as input reports `[0x55, reply…]`. `[0x77]`
requests status, answered `[0x77, battery %]`; `[0x88]` reports the keyboard
offline. The vendor host spaces sends 100 ms apart, waits 200 ms for a reply and
retries an unanswered read up to three times after the initial request. The
reference filter (`index.1970d80c.js`, character offset887863, `Px`) declares
report ID6 with **65 payload bytes**, excluding the HID report ID. That holds
the marker and the complete64-byte command. Input decoding is at1367514
(`___decodeBtInputData`); framing/retries are in the same `i6` class.

Rust reads report IDs and lengths from the descriptor, sums split items and
padding, and requires both vendor input/output reports to fit the full payload.
It checks the input report ID, rejects short command replies and partial writes,
and retries only unanswered queries. Settings writes are never retried.
Status requests share the100ms send spacing. Battery values outside0..100 are
errors. A sleeping receiver does not mask an available Bluetooth keyboard.
These are offline implementation checks, not evidence of Bluetooth hardware
support on the original M1W.

## 9. Model-aware families and snapshots

Sections1–8 preserve original-M1W evidence and framing. The current implementation
selects codecs from the 52-model [catalog](KEYBOARDS.md), rather than assuming
that every board uses ID983's addresses. USB VID/PID is never sufficient identity.
The internal ID remains the little-endian u32 at0x8F response bytes1..4. Unknown
IDs are refused, and writes recheck the identified ID.

Both families use64-byte feature payloads. USB/boot descriptors determine the
actual HID report ID, including numbered reports. The parser prefers the vendor
64-byte collection; a unique64-byte feature report can identify the vendor's
consumer-page boot interfaces. Wireless envelopes retain the complete payload.
The access rule enumerates exact catalog/receiver pairs and remains scoped to
configuration/boot interfaces; its historical filename is kept for upgrades.

### RY5088 and YC3123 configuration headers

This table describes the implemented newer family. All checksums are B7 except
main/side lighting (B8), and data begins at byte8 where specified.

| Feature | SET | GET / read layout |
|---|---|---|
| Firmware revision | — |0x8F response u16LE at7..8 |
| RF/version fallback | — |0x80 response u16LE at1..2 |
| Active profile |0x04, profile at1 |0x84, profile at1 |
| Report rate |0x03, selector0 at1, enum at2 |0x83, enum at2 |
| Debounce |0x06, value at1 |0x86, value at1 |
| Keyboard options |0x09, selector0 at1, four bytes at2..5 |0x89, bytes2..5 |
| Main lighting |0x07, existing mode/speed/brightness/flags/RGB header |0x87 |
| Side lighting |0x08, own effect namespace and direct speed |0x88 |
| Base map | Single0x0A `[profile,slot,0,0,1,output]`, binding8..11 |0x8A `[profile,255,page,output]`,8 raw64-byte pages |
| Fn map | Single0x10 `[system,index,slot]`, binding8..11 |0x90 `[system,index,255,page]`,8 raw64-byte pages |
| Macro |0x0B, five56-byte write pages (last32) |0x8B `[slot,255,page]`, four64-byte pages |
| Selected RGB |0x0C `[picture,255,page,length,final]`, data8 |0x8C `[picture,255,page]`, six64-byte pages |
| Wireless timers |0x11, four u16LE values at8..15 |0x91, values8..15 |
| Auto OS |0x17, boolean at1 |0x97, boolean at1 |

Report-rate enum order is8000,4000,2000,1000,500,250,125Hz. A model's advertised
ceiling limits selectable rates. Keyboard option bytes are Fn-map selector,
anti-mistouch, rapid-trigger stability and WASD swap. These are not the legacy
three-byte bitfields. Sleep controls/ranges follow metadata; missing controls
are not invented from generic methods.

Each map preserves all128 four-byte tuples. A magnetic profile has four binding
outputs, stored in snapshot base-map index `profile * 4 + output`. Modern headers
encode profile and output separately; Fn maps encode Windows0/macOS1 and local
Fn index independently. Legacy magnetic active-profile selection uses
`logical_profile * 4`. Source defaults and Reset use the model's concrete Fn Mac
matrix for macOS maps.

RGB writes preserve all384 bytes, correcting the newer host writer's378-byte
limit. The selected picture is read at Apply and inserted in modern headers.
Snapshots still capture one picture; an all-picture capture intentionally changes
selection and restores the prior lighting. Picture-count3/5 comes from metadata.
Palette flags/colors and main speed maximum5/4 differ by family/model. Brightness
or speed edits preserve existing raw color/flag encodings.

### Magnetic arrays, precision and calibration

GET0xE5 uses `[parameter,1,page]` and returns raw64-byte pages. SET0x65 uses
`[parameter,1,page,final]`,56 data bytes per page at8. Full arrays are preserved
when editing one key. Parameter sizes:

| Parameter | Meaning | YC500 bytes | RY5088 bytes |
|---|---|---|---|
|0/1 | Actuation/release |128 |256LE words |
|2/3 | Rapid press/release |128 |256LE words |
|4 | Dynamic threshold |128 |256LE words |
|5 | Tap-hold duration (10ms units) |128 |128 |
|6 | Bottom dead zone |128 |256LE words |
|7 | Key behavior plus RT bit7 |128 |128 |
|9 | Snap partner slot |128 |128 |
|10 | DKS, four output planes |512 |512 |
|251 | Top dead zone | — |128, revision-gated |
|252 | Switch type |128 if replaceable |128 if replaceable |

DKS readback is four channel planes. Writes transpose it into128 per-key
records: opcode0x65 parameter8, mode0, slot at3, final at4, four output bytes
at8..11. Each output byte has four2-bit stage states. It is not written as a
parameter10 bulk page. Snap edits detach previous reciprocal partners and update
both directions.

YC500 uses its legacy tenths-of-mm offset conventions. Newer precision follows
the nonzero RF version (falling back to USB) and, where present,0xE6's`AA` feature marker: precision codes0/1/2
mean100/200/1000 units per mm. The feature response also declares optional
keyboard/gamepad output mode (GET0x9D/SET0x1D). Top dead zone is gated on revision
at least0x0400. Typed edits enforce model bounds and exact representability.
Raw captures retain unknown values for lossless backups.

Calibration is an explicit USB-only sequence: SET0x1C with1, wait4s; SET0x1C
with0, wait2s; SET0x1E with1, wait2s. Finish is SET0x1E with0, wait2s. The caller
saves a fresh configuration first. This is not a reversible settings transaction:
configuration exports do not capture previous sensor calibration. Live travel
normalizes legacy calibration reports and decodes newer word arrays to mm.
Neither calibration nor new-model travel has physical validation in this pass.

### Format3 and transaction boundaries

Format3 adds `advanced` to the existing snapshot: optional RF revision, precision,
modern options, report rate, side lighting, raw magnetic arrays, legacy preset
and optional output mode. Required model fields/dimensions are validated.
Format1/2 original exports remain readable. Snapshot/profile/picture/undo paths
reject another model even at the same revision. Magnetic restores and performance
profile loads also require matching precision. Advanced changes use a full fresh
read for stale-state checks and final requested-draft comparison.

Original ID983's inactive-layer selection workaround and macro-induced
picture0 preservation are scoped to that ID. They are observed firmware behavior,
not universal protocol steps. Transactions are not atomic; failures stop without
automatic retry or a general recovery guarantee. New-family evidence is host
source and offline tests, not hardware observation.
