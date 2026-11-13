# Experimental USB firmware updates

The CLI and GUI implement model-targeted USB update flows from the vendor host. **This has
not been tested on hardware. Wrong firmware or interruption can brick the
keyboard.** This implementation pass did not flash a device.

Use a USB data cable and the decompressed vendor `firmwareFile.bin` supplied for
**the exact internal model ID**. Models and revisions are not interchangeable.
Supply `--model ID` for non-M1W CLI files. The GUI targets the loaded keyboard.
The [model catalog](KEYBOARDS.md) determines prefix and boot-header layout. ZIP/raw-deflate downloads must be decompressed first.
Automatic downloads, RF/receiver updates and already-booted recovery are not
implemented. No firmware is bundled.

Original M1W skips the first65536 bytes; many RY5088 models skip20480. The catalog records each value. Initial analysis of the downloaded
ID983 v108 package found a bundled bootloader and padding in this prefix; see
[the firmware and QMK/VIA research](QMK-VIA-RESEARCH.md) for acquisition, hashes,
memory mapping and the bundled bootloader's descriptors. Its presence does not
prove which bootloader is installed on a keyboard. We have not established a
model-authentication or signature schema in the prefix. The parser checks size, target-model parameters and protocol bounds,
not image authenticity. The known ID983 v108 hash is rejected for other targets;
unknown files have no established model-authentication schema. The preview explicitly reports
`image_model_verified: false`. SHA-256 pins the exact selected file; it does not
prove that the file belongs to this keyboard.

## CLI

Preview works offline and never opens HID:

```sh
monsgeek-linux --model 2247 firmware firmwareFile.bin
```

Review the file, size, block count and SHA-256. To flash, provide that exact
SHA-256 and a new configuration-backup path:

```sh
monsgeek-linux --model 2247 firmware firmwareFile.bin \
  --sha256 SHA256_FROM_PREVIEW \
  --apply --allow-experimental-writes --allow-experimental-firmware \
  --before before-firmware.json
```

`--device /dev/hidrawN` selects the normal USB configuration interface when
several boards are connected. Wireless transports are refused. Close other
configurators and keep the cable connected until completion.

Before entering boot mode, the updater verifies the selected catalog ID, resolves its physical USB
port and saves a full configuration snapshot to a new0600 file. It then accepts
only a vendor boot-keyboard candidate on that same USB port. It never selects
an unrelated boot device merely because it appeared first. The shipped access
rule includes the reference's shared boot interfaces; install the current rule
before flashing if needed. This development pass did not install it.

After transferring the image, it checks the vendor completion marker and waits
for that keyboard to return on the same port, verifies its identity again and
reads its firmware revision. A missing/malformed reply, ambiguous interface,
failed send or reconnect timeout is an error. Firmware sends are not retried.
No automatic rollback, configuration restore or recovery is attempted.

The snapshot captures only the selected RGB picture, not inactive pictures.
The saved snapshot is **configuration only**, not a copy of the old firmware or
an unbrick image. Firmware may reset or change settings. Review the new state
before restoring configuration; ordinary restore rejects revision mismatches.
After a failure, keep the backup and inspect the device before attempting any
recovery. A failure may occur after flash memory has already changed.

## GUI

Open **System**, then **Firmware (experimental)**. Select the same vendor binary.
Apply or discard pending configuration edits and reload over USB before flashing.
**Flash firmware** opens a confirmation showing the selected filename, hash and
warning. Confirming pins that exact loaded file; the file is not reread later.
The GUI saves its configuration snapshot as
`$XDG_STATE_HOME/monsgeek-linux/firmware-before-TIMESTAMP.json` and reports
progress. The current session's Logs record the image hash, backup path and
result. A successful flash reloads configuration; it does not reapply old settings.

## Recovered wire contract

Reference: macOS app version500.3.3, hashes in [SOURCES.md](SOURCES.md).

- `43a0ab9f.js`, `CommonKeyboard`/class`F`, character offset9839:
  `upgrade_usb(file, 65536, 10, progress)`. The concrete ID983 dispatch inherits
  this method; no M1W override was found.
- `index.1970d80c.js`, class`gd`, character offset1373362 onward:
  `boot_usb`, `checkIsCanStartUpgrade`, `upgradeFirmware`, `checkUpgradeIsSuccess`.
- `index.1970d80c.js`, `uR`, offset887932: shared boot-keyboard filters,
  interface0. These establish candidate framing, not M1W-specific boot identity.

The original M1W uses report ID0 with64 payload bytes. The implementation parses
the actual feature-report descriptor and preserves numbered report IDs for other
normal/boot interfaces. The table below is the original YC500 header form:

| Stage | Payload |
|---|---|
| Enter boot | `7F 55 AA 55 AA 00 00`, complement checksum at byte7 |
| Prepare | `BA C0`, u16LE block count at2, payload length at4, complement checksum at7 |
| Data |64 raw image bytes, final partial block padded with zeros; no checksum rewrite |
| Finish | `BA C2`, u16LE block count at2, u32LE byte sum at4, u32LE payload length at8; no header checksum |
| Completion | Vendor predicate: reply byte4 equals`55` |

YC3123 wide boot headers place the32-bit block count at8..11. Prepare uses B7;
finish places the32-bit sum at12..15 and does not write the ordinary byte-length
field. Their completion predicate is reply byte2=`55`, rather than byte4. The
model catalog selects this form; `tests/models.rs` verifies both header layouts.

The sum excludes the skipped prefix and final padding. Maximum65535 blocks,
4194240 payload bytes. Data sends are spaced10ms apart. Finish uses100ms before
send and100ms before read. Boot discovery and post-flash reconnect are bounded
and tied to the original USB port.

The vendor accepts any full prepare reply and tries prepare up to ten times.
The implementation retains the host's reply predicate but sends prepare only
once, to avoid repeating a potentially destructive operation after a failure.
No stronger ACK schema or actual boot VID/PID for this M1W has been observed.
The final marker and identity/revision query do not independently verify every
flash byte, image provenance, physical operation or power-cycle behavior.
