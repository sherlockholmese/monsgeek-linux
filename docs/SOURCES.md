# Source provenance and reproduction

## Official source

- Landing page: <https://www.monsgeek.cn/download/>.
- Download researched: <https://www.monsgeek.cn/wp-content/uploads/2026/09/MonsGeek-Driver-v4500.3.3_MAC20260923.rar>.
- Application `APPVersion.json`: `APPVersion=500.3.3`, `IOTMacVersion=217`,
  `IotManagerMacVersion=0.1.7`.
- Archive retained at `.amp/in/vendor/monsgeek-mac.rar`.
- Archive SHA-256:
  `7d74ef299cc5c2360e700ccb391b615fb8aecd6e30931499e6fa13617363a4e0`.

The macOS distribution exposes its JavaScript host implementation, which is easier
to inspect than treating a Windows executable as an opaque driver. Neither the
vendor app nor its native executables was run. This is protocol interoperability
research, not porting the proprietary app's source wholesale.

Extract the archive with `7z x`, inspect the DMG listing, then extract the full
directory `MonsGeek Driver v4 500.3.3/MonsGeek Driver v4.app/Contents/Resources/app`
from the DMG. A wildcard-only extraction failed; the full directory path worked.
Do not install or run the application to inspect it. The intermediate DMG was
removed; the original RAR and extracted files remain.

Local app root:
`.amp/in/vendor/extracted/MonsGeek Driver v4 500.3.3/MonsGeek Driver v4.app/Contents/Resources/app/`.
JavaScript bundles below are relative to its `dist/js/` directory.

| File | SHA-256 |
|---|---|
| `index.1970d80c.js` | `459954b4633f6b3a0183d05775dfef9547a1564cc9a975f1af19a56bbccb12a6` |
| `b7c9f14f.js` | `3247e9169240baae79ab472dc664297e1ad4bda658438f52c1e013c65e247b94` |
| `cb8a3a35.js` | `bf33fa80a94717f0669969dc251c4b00898714b2761f2d0a6a956ce72a9a2315` |
| `43a0ab9f.js` | `99c03503af15af4f6e7c106d8d0766eaff6ad8819b41979b533b1dde183d3448` |

## Correct implementation ownership

The original mechanical M1W is **ID983**, `yc500_m1wnosled_soc`, not a similarly
named HE, VIA, or newer M1W model. USB `3151:4015` is not unique to this model.

```text
index: device ID983 metadata
  -> b7c9f14f: dispatch yc500_m1wnosled_soc to Z
     Z (concrete default matrices)
       extends s (M1W family palette/flag overrides)
         extends cb8a3a35 V / CommonKbYc500
           extends K
             extends 43a0ab9f F / CommonKeyboard
               extends index i6 (transport/checksum)
```

Always follow overrides before copying a packet. In particular, generic macro
opcodes `0x08` and `0x0B` are overridden by **`0x16`** for this model. Binding
conversion is index `Xi`, exported as `ax`; the nearby `hr`/`aX` converter has
different ordering and is not the M1W's imported codec.

## Research locators

Bundles are minified, usually one line. Positions below are **zero-based decoded
Unicode character offsets**, not byte offsets. Read with Python UTF-8 text then
slice; verify a named anchor because another vendor release will shift positions.

| Bundle | Offset / anchor | Topic |
|---|---|---|
| index | ~669414, `id:983` | Exact model metadata, layers, sleep bounds |
| b7 | 124065, `case"yc500_m1wnosled_soc"` | Dispatch |
| b7 | 106368, `class Z extends s` | Concrete base, Fn, `defaultFnMACMatrix` |
| b7 | 39291, `class s` | Palette/DAZZLE/NORMAL overrides |
| cb | 5918, `class V extends K` | Actual inherited YC500 class |
| cb | 745 / 1117, `setSleepTime` / `getSleepTime` | Sleep layout |
| cb | ~5959, `LightList` | Lighting opcode mapping |
| cb | 9451 / 9615, `setAutoOsen` / `getAutoOsen` | Automatic OS setting |
| cb | 10188 / 10621 | Single base / Fn binding writes |
| cb | 11256, `_setMacro` | Actual macro paging/final marker |
| 43 | 283, `findIndexInDefaultMatrix` | Source-key to matrix-slot mapping |
| 43 | 1223 / 1571 / 2479 | Macro serializer / decoder / readback |
| 43 | 3572 / 3957 / 4302 / 4533 | RGB page writer / serializer / reader / mapping |
| 43 | 4831 / 6080 | Base/Fn full-matrix write framing |
| index | ~191415, `Qw` | Encoded 32-bit binding to four bytes |
| index | 223773 / 225648, `$w` / `cn` | Function/media / mouse binding tables |
| index | 463140 / 464271 | Macro UI size check / event encoder |
| index | ~1273800, `Xi` | Correct four-byte binding converter |
| index | ~1366500..1371250, `___encodeCmd`, `vendorSleep` | Checksums and host delay |
| index | ~1624753 | Macro repeat validation |
| index | 1326994 / 1753527, `maxMacro=50` | Host slot allocation |
| index | ~2161846 | Debounce UI default bounds 1..10 |

## Recreate the complete matrix reference

`tools/extract_reference.py` parses only numeric array literals inside the exact
concrete class. It does not execute JavaScript, open devices, or send packets.
Run from the repository root, with the app's `dist/js` directory as its argument:

```sh
python docs/tools/extract_reference.py \
  '.amp/in/vendor/extracted/MonsGeek Driver v4 500.3.3/MonsGeek Driver v4.app/Contents/Resources/app/dist/js' \
  > docs/m1w-reference.json
```

The JSON retains all zero/unused slots as well as special Fn/knob tuples. It is
not derived from the Rust `key_slot` table and can independently check that table.

## Experimental firmware updater

`43a0ab9f.js`, `CommonKeyboard`/class `F`, offset9839, inherits
`upgrade_usb(file, 65536, 10, progress)` into the concrete M1W driver.
`index.1970d80c.js` class `gd` starts at1373362 and contains the boot, prepare,
data and completion flow. The `uR` boot-keyboard filter table starts at887932.
[The firmware guide](FIRMWARE.md) records exact packets, implementation deviations
and unvalidated boundaries. No native updater binary was executed.

## Other research

[Sharkfin](https://github.com/dniminenn/sharkfin) was consulted as a cross-check;
its original M1W entry marked the protocol family unknown when inspected. The
temporary clone was removed. The official model dispatch and codecs, not another
project's driver, are the primary evidence.

See [VALIDATION.md](VALIDATION.md) for local capture paths and hashes. Do not
redistribute the full proprietary application as project source. A new agent on
a different checkout must acquire the archive or explicitly transfer these files.
