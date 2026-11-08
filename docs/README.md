# Original M1W reverse-engineering handoff

Start with [USAGE.md](USAGE.md) and [STATUS.md](STATUS.md). The owner resumed
implementation and connected-device validation after the reverse-engineering pass.
Rust is a suitable implementation language: the configuration protocol is USB HID
userspace traffic, not a Linux kernel driver. Normal typing already works via HID.

| Document | Contents |
|---|---|
| [USAGE.md](USAGE.md) | CLI commands, JSON formats, backup/undo/restore and safe use |
| [PROTOCOL.md](PROTOCOL.md) | Transport, opcodes, exact field layouts, binding/macro/RGB codecs, vendor defects |
| [FIRMWARE.md](FIRMWARE.md) | Experimental USB firmware updater, file format, explicit flashing flags and unvalidated limits |
| [STREAMING.md](STREAMING.md) | Model-specific audio/screen UI, native service calls, missing live HID codec |
| [SOURCES.md](SOURCES.md) | Reproducible source acquisition, hashes, inheritance, research locators |
| [HANDOFF.md](HANDOFF.md) | Next implementation tasks, ownership boundaries, validation gates and open questions |
| [STATUS.md](STATUS.md) | What exists, what was observed, what remains unimplemented |
| [VALIDATION.md](VALIDATION.md) | Offline checks, real setting round trips, failures/fixes and remaining physical tests |
| [m1w-reference.json](m1w-reference.json) | All 128 slots of each concrete vendor default matrix, machine-readable |

## Evidence vocabulary

- **Host**: reconstructed from the official macOS app, including its inherited
  model-specific methods. Establishes what that app sends/expects, not firmware support.
- **Read observed**: a query on the attached original M1W returned the recorded
  bytes. Does not establish a setting can be changed or persists after power loss.
- **Offline matched**: Rust packet output compared with official host code.
- **Round-trip observed**: real release CLI applied a distinguishable value,
  checked full exported state, undid it and exactly restored the baseline.
- **Unresolved**: ambiguous host behavior, unsupported-looking echoes, native
  service internals, or device behavior requiring a controlled write/physical test.

This is a detailed host-protocol reconstruction, **not a complete firmware
disassembly or a fully hardware-validated port**. Explicit remaining research is
part of the handoff. Do not convert a generic inherited capability into a claim
that the original M1W supports it.

Raw captures and proprietary vendor sources remain local under `.amp/in/`.
Transfer them explicitly if handing off to a different machine; these relative
paths alone do not transfer evidence. The default matrices are host references,
not a substitute for backing up the owner's actual settings.
