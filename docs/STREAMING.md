# Audio visualization, screen color and dynamic lighting

## Support conclusion

Original-M1W metadata **does expose audio visualization and screen-color
lighting**, with genuine native-service startup paths. They are not just names
in a generic effect table. However, **the live streaming HID codec has not been
recovered**; these features are not implemented or validated on Linux.

The exact ID983 `lightLayout:ct` contains21 effects: basic modes1..19 including
custom picture13, plus screen21 and audio22. It does not expose shared-table
music20, train23, fireworks24 or user-color66. Off0 exists in the driver table.
Picture UI offers three slots. The original inherited writer selects0, while
this implementation selects, captures and restores each picture explicitly.
Independent picture storage and macro preservation later passed ID983 readback
tests; see [VALIDATION.md](VALIDATION.md). This does not resolve streaming.

Audio UI options `upright`, `separate`, `intersect` map to option indices0..2,
with RGB/dazzle and brightness0..4. Screen brightness0..4. These are **effect IDs**
within SET0x07: audio22 (`0x16`), screen21 (`0x15`). They are **not stream opcodes**
and must not be confused with macro SET0x16 or Fn SET0x15.

## Host-to-native path

All offsets in this section are zero-based decoded character positions in
`dist/js/index.1970d80c.js`; hashes and app root are in [SOURCES.md](SOURCES.md).

| Offset | Evidence |
|---|---|
| 514576..515993 | `ct`: exact original-M1W effects and UI options |
| 669414..669907 | ID983 selects `ct`; no side-light or screen hardware metadata |
| 1835480..1835840 | Keyboard initialization selects native task list `h9` |
| 1799979 | `h9=[am,d9,Hd,_9]` |
| 1783044 onward | `_9`: native keyboard settings task |
| 1783907..1784610 | Changed effect calls inherited `setLightSetting` |
| 1783206..1783358 | Stop native lighting, write effect, call `Yn` |
| 1834780..1835035 | `Yn`: normalize music names; delay startup three seconds |
| 1363370..1364770 | `Ri.startLight`: enable native feature then device lighting |
| 1271157..1271320 | Browser-only `Vt.startLight/stopLight` are empty stubs |
| 1740941..1741200 | Separate multi-device Light Sync page excluded for MonsGeek |

`Ri.startLight` enables `MUSIC_RHYTHM` or `SCREEN_LIGHT`, then selects MUSIC or
SCREEN for the device path. It polls default audio-output changes every five
seconds and restarts processing. Its microphone-change operation explicitly
reports unsupported. Stop disables both native features and selects OTHER.

`c85db53a.js`, offsets1007..1540 and3214..3300, hides audio/screen effects on
macOS and requires native/IOT SDK mode for activation. Thus this macOS bundle
contains the shared protocol/UI research needed to find a Windows-driven path,
not a working portable audio/screen engine. The multi-device Light Sync page is
separate from these individual effects and is explicitly hidden for MonsGeek.

## Recovered native API boundary

Manager gRPC-Web base `http://127.0.0.1:6015` at index offset894119. This is the
vendor's architecture, not a server started by this project.

| Method/schema | Index offset | Fields |
|---|---|---|
| `controlFeature` wrapper | 903274 | Enable/disable feature |
| `controlDeviceLight` wrapper | 904164 | Select device light processing |
| `ControlFeatureRequest` | 836913 | Protobuf1 enum feature,2 bool start,3 string session_id |
| `ControlDeviceLightInfo` | 840124 | Protobuf1 string devicePath,2 enum light,4 string session_id |
| Enum definitions | 824144..824510 | Feature music0/screen1; device music0/screen1/other2 |

Executable selection at894748 chooses bare Mach-O name on macOS and `.exe`
otherwise. That conditional is **not Linux executable support**.

Native strings were inspected without executing binaries. Offsets here are
**decimal byte positions**, unlike JS locators:

| Binary under app `iot_manager/` | Byte offset | Evidence |
|---|---|---|
| `iot_manager_rs` | 4548744 | `/iot_manager.IotManager/ControlFeature`, `/ControlDeviceLight`, `/ControlService`, `/StartClientMonitoring` route block |
| `iot_manager_rs` | ~4519855 | `/rhythm_analysis.RhythmAnalysisService/StreamData` |
| `iot_manager_rs` | 4520928 / 4520992 | `/screen.ScreenService/ListScreens`, `/StreamScreenThumbnails` |
| `iot_manager_rs` | 4521192 | Local service clients on ports3838,3839,3840,3850 |
| `common_hid_rs` | ~1797850 | `127.0.0.1:3838/hid.HidService/StartDeviceMonitoring` |
| `rhythm_service.exe` | 3540128 / 3576632 | Port3839; ListAudioDevices/GetConfiguration/Configure/StreamData |
| `rhythm_service.exe` | 3483152 | Audio device, num_bands, fft_size, gain, tilt/contrast/release factors, min/max dB, attack_frames |
| `rhythm_service.exe` | 3499376 / 3499480 | `(Loopback)`, `src\audio\capture.rs` |
| `screen_capture_service.exe` | 2737744 / 2690104 / 2690148 | Port3840; StreamScreenThumbnails/ListScreens |
| `iot_manager_rs` | 4553824 | Spectrum data and nearby thumbnail pixels/dimensions |
| `iot_manager_rs` | 5924030 / 5955443 | `RyDevice::loop_send_rhythms` / `loop_send_screen` |
| `iot_manager_rs` | 6579174 / 6747811 | `RyDevice::send_screen` / `io_ry_device::acquire_light_plugin` |
| `iot_manager_rs` | 4519151 | Failed-to-acquire-light-plugin error |

These strings locate the missing native implementation; they do not prove its
plugin accepts ID983 or reveal its packet bytes.

## Native inventory

| File | Format / size | SHA-256 |
|---|---|---|
| `common_hid_rs` | Mach-O x86_64 /2274560 | `d83bc9114d870f96f024df11d74f6fcaa4037b244eb6b3f4c47d7b27e6cff708` |
| `iot_manager_rs` | Mach-O x86_64 /7103408 | `4ed567aa3fa348fe95dfe9ae783160a01795f6c4c44a01055a86b7ea626e870f` |
| `common_hid_rs.exe` | PE x86_64 /3073600 | `619fa555b598c058babb8dc11a242f5be6aec761b586dc3694362bc17ccec97a` |
| `iot_manager_rs.exe` | PE x86_64 /7361928 | `3892f0b5bd6ad1ebdfc3f448c951cb322d7767f166916406bc30fa3e3d1f1a2d` |
| `rhythm_service.exe` | PE x86_64 /4692544 | `d40cde8e34216d77f2d07379a74edc5db0cdf2a2601c6316cc3ddad288ac9706` |
| `screen_capture_service.exe` | PE x86_64 /3823168 | `0838804a8bb24670d3765081119d6e4e5a726175eaeb6d2126fdd6a240a35a4d` |

## Remaining reverse engineering

Recover model/plugin acceptance, spectrum-to-HID and screen-to-key conversion,
stream command/report IDs, page/chunk format, sequence/checksum rules, cadence
and transport differences. Use native disassembly or owner-authorized vendor
traffic captures. Do not run opaque vendor binaries against the keyboard during
the current read-only documentation phase.

Once the wire codec is known, Rust can implement Linux capture/FFT and frame
generation separately. Linux audio-output capture and Wayland/X11 screen capture
have their own permission/platform requirements. No such implementation is
present. Do **not** repeatedly invoke persistent picture writes as a guessed
real-time transport: persistence/flash behavior is unknown.
