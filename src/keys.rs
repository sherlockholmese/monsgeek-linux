//! Human-facing names for usages, bindings, effects and the physical layout.
//! Labels never replace stored tuples: unknown values remain raw in snapshots.
use std::sync::LazyLock;

use crate::{config::MacroEvent, protocol::key_slot};

/// HID keyboard usage page names.
pub fn usage_name(usage: u8) -> Option<&'static str> {
    const LETTERS: [&str; 26] = [
        "A", "B", "C", "D", "E", "F", "G", "H", "I", "J", "K", "L", "M", "N", "O", "P", "Q", "R",
        "S", "T", "U", "V", "W", "X", "Y", "Z",
    ];
    const DIGITS: [&str; 10] = ["1", "2", "3", "4", "5", "6", "7", "8", "9", "0"];
    const F: [&str; 12] = [
        "F1", "F2", "F3", "F4", "F5", "F6", "F7", "F8", "F9", "F10", "F11", "F12",
    ];
    const F13: [&str; 12] = [
        "F13", "F14", "F15", "F16", "F17", "F18", "F19", "F20", "F21", "F22", "F23", "F24",
    ];
    Some(match usage {
        4..=29 => LETTERS[usize::from(usage - 4)],
        30..=39 => DIGITS[usize::from(usage - 30)],
        40 => "Enter",
        41 => "Esc",
        42 => "Backspace",
        43 => "Tab",
        44 => "Space",
        45 => "-",
        46 => "=",
        47 => "[",
        48 => "]",
        49 => "\\",
        50 => "#",
        51 => ";",
        52 => "'",
        53 => "`",
        54 => ",",
        55 => ".",
        56 => "/",
        57 => "Caps Lock",
        58..=69 => F[usize::from(usage - 58)],
        70 => "Print Screen",
        71 => "Scroll Lock",
        72 => "Pause",
        73 => "Insert",
        74 => "Home",
        75 => "Page Up",
        76 => "Delete",
        77 => "End",
        78 => "Page Down",
        79 => "Right",
        80 => "Left",
        81 => "Down",
        82 => "Up",
        83 => "Num Lock",
        84 => "Num /",
        85 => "Num *",
        86 => "Num -",
        87 => "Num +",
        88 => "Num Enter",
        89 => "Num 1",
        90 => "Num 2",
        91 => "Num 3",
        92 => "Num 4",
        93 => "Num 5",
        94 => "Num 6",
        95 => "Num 7",
        96 => "Num 8",
        97 => "Num 9",
        98 => "Num 0",
        99 => "Num .",
        100 => "ISO \\",
        101 => "Menu",
        135 => "Ro",
        136 => "Kana",
        137 => "Yen",
        138 => "Convert",
        139 => "NonConvert",
        144 => "Hangul",
        145 => "Hanja",
        146 => "Katakana",
        147 => "Hiragana",
        148 => "Zenkaku/Hankaku",
        104..=115 => F13[usize::from(usage - 104)],
        224 => "Left Ctrl",
        225 => "Left Shift",
        226 => "Left Alt",
        227 => "Left Super",
        228 => "Right Ctrl",
        229 => "Right Shift",
        230 => "Right Alt",
        231 => "Right Super",
        _ => return None,
    })
}

/// Short keycap legend.
pub fn usage_legend(usage: u8) -> String {
    match usage {
        42 => "⌫".into(),
        57 => "Caps".into(),
        70 => "PrtSc".into(),
        73 => "Ins".into(),
        75 => "PgUp".into(),
        76 => "Del".into(),
        78 => "PgDn".into(),
        83 => "Num".into(),
        84 => "/".into(),
        85 => "*".into(),
        86 => "−".into(),
        87 => "+".into(),
        88 => "Enter".into(),
        89..=97 => format!("{}", usage - 88),
        98 => "0".into(),
        99 => ".".into(),
        138 => "Conv".into(),
        139 => "NConv".into(),
        79 => "→".into(),
        80 => "←".into(),
        81 => "↓".into(),
        82 => "↑".into(),
        224 | 228 => "Ctrl".into(),
        225 | 229 => "Shift".into(),
        226 | 230 => "Alt".into(),
        227 | 231 => "Super".into(),
        _ => usage_name(usage)
            .map(str::to_owned)
            .unwrap_or_else(|| format!("{usage:#04x}")),
    }
}

pub const CONSUMER: &[(u16, &str)] = &[
    (205, "Play/Pause"),
    (181, "Next track"),
    (182, "Previous track"),
    (183, "Stop"),
    (226, "Mute"),
    (233, "Volume up"),
    (234, "Volume down"),
    (111, "Screen brightness up"),
    (112, "Screen brightness down"),
    (387, "Media player"),
    (394, "Mail"),
    (402, "Calculator"),
    (404, "File manager"),
    (545, "Search"),
    (547, "Browser home"),
    (548, "Browser back"),
    (549, "Browser forward"),
    (551, "Browser refresh"),
    (554, "Bookmarks"),
];

pub const SYSTEM: &[(u8, &str)] = &[(129, "Power"), (130, "Sleep"), (131, "Wake")];

pub const MOUSE: &[(u8, &str)] = &[
    (0xf0, "Left click"),
    (0xf1, "Right click"),
    (0xf2, "Middle click"),
    (0xf3, "Mouse back"),
    (0xf4, "Mouse forward"),
];

pub const POINTER: &[(&str, &str)] = &[
    ("scroll-up", "Scroll up"),
    ("scroll-down", "Scroll down"),
    ("move-left", "Pointer left"),
    ("move-right", "Pointer right"),
    ("move-up", "Pointer up"),
    ("move-down", "Pointer down"),
];

pub const MACRO_MODES: [&str; 3] = ["Repeat", "Toggle", "Hold"];

pub fn consumer_name(usage: u16) -> Option<&'static str> {
    CONSUMER.iter().find(|(u, _)| *u == usage).map(|(_, n)| *n)
}

pub fn mouse_name(button: u8) -> Option<&'static str> {
    MOUSE.iter().find(|(b, _)| *b == button).map(|(_, n)| *n)
}

/// Human label for a stored four-byte binding.
pub fn binding_label(value: [u8; 4]) -> String {
    let named = |u: u8| {
        usage_name(u)
            .map(str::to_owned)
            .unwrap_or_else(|| format!("Key {u:#04x}"))
    };
    match value {
        [0, 0, 0, 0] => "Disabled".into(),
        [0, a, b, c] => [a, b, c]
            .into_iter()
            .filter(|u| *u != 0)
            .map(named)
            .collect::<Vec<_>>()
            .join(" + "),
        [3, 0, lo, hi] => {
            let usage = u16::from_le_bytes([lo, hi]);
            consumer_name(usage)
                .map(str::to_owned)
                .unwrap_or_else(|| format!("Media {usage:#06x}"))
        }
        [2, 0, u, 0] => SYSTEM
            .iter()
            .find(|(s, _)| *s == u)
            .map(|(_, n)| (*n).to_owned())
            .unwrap_or_else(|| format!("System {u:#04x}")),
        [1, 0, b @ 0xf0..=0xf4, 0] => mouse_name(b).unwrap_or("Mouse").into(),
        [1, 0, 245, 1] => "Scroll up".into(),
        [1, 0, 245, 255] => "Scroll down".into(),
        [1, 0, 246, 251] => "Pointer left".into(),
        [1, 0, 246, 5] => "Pointer right".into(),
        [1, 0, 247, 251] => "Pointer up".into(),
        [1, 0, 247, 5] => "Pointer down".into(),
        [9, mode, slot, 0] if slot < 50 && mode < 3 => {
            format!("Macro {} {}", slot + 1, MACRO_MODES[usize::from(mode)])
        }
        [8, 0, 4, n] if n < 4 => format!("Layer {}", n + 1),
        [10, 1, 0, 0] => "Fn".into(),
        [10, 1, 1, 0] => "Right Fn".into(),
        [10, 14, 0, 0] => "Volume/backlight toggle".into(),
        _ => "Vendor function".into(),
    }
}

/// Every binding the editor offers, grouped. `None` restores the vendor default.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    pub group: &'static str,
    pub label: String,
    pub value: Option<[u8; 4]>,
}

pub static TARGETS: LazyLock<Vec<Target>> = LazyLock::new(|| {
    let mut out = vec![
        Target {
            group: "General",
            label: "Default".into(),
            value: None,
        },
        Target {
            group: "General",
            label: "Disabled".into(),
            value: Some([0; 4]),
        },
        Target {
            group: "General",
            label: "Fn".into(),
            value: Some([10, 1, 0, 0]),
        },
    ];
    let keys = (4u8..=101)
        .chain(104..=115)
        .chain(135..=148)
        .chain(224..=231)
        .filter_map(|u| usage_name(u).map(|n| (u, n)));
    for (u, n) in keys {
        out.push(Target {
            group: "Keys",
            label: n.into(),
            value: Some([0, 0, u, 0]),
        });
    }
    for (u, n) in CONSUMER {
        let [lo, hi] = u.to_le_bytes();
        out.push(Target {
            group: "Media",
            label: (*n).into(),
            value: Some([3, 0, lo, hi]),
        });
    }
    for (u, n) in SYSTEM {
        out.push(Target {
            group: "System",
            label: (*n).into(),
            value: Some([2, 0, *u, 0]),
        });
    }
    for (b, n) in MOUSE {
        out.push(Target {
            group: "Mouse",
            label: (*n).into(),
            value: Some([1, 0, *b, 0]),
        });
    }
    for (action, n) in POINTER {
        let value = crate::config::Binding::Pointer {
            action: (*action).into(),
        }
        .encode()
        .expect("static pointer actions are valid");
        out.push(Target {
            group: "Mouse",
            label: (*n).into(),
            value: Some(value),
        });
    }
    for n in 0..4u8 {
        out.push(Target {
            group: "Layers",
            label: format!("Layer {}", n + 1),
            value: Some([8, 0, 4, n]),
        });
    }
    for slot in 0..50u8 {
        for (mode, name) in MACRO_MODES.iter().enumerate() {
            out.push(Target {
                group: "Macros",
                label: format!("Macro {} {name}", slot + 1),
                value: Some([9, mode as u8, slot, 0]),
            });
        }
    }
    out
});

/// Modifier usages offered in combinations.
pub const MODIFIERS: [(u8, &str); 4] =
    [(224, "Ctrl"), (225, "Shift"), (226, "Alt"), (227, "Super")];

/// Encode modifiers plus one key exactly as the vendor stores a combination.
pub fn combo(modifiers: &[u8], key: u8) -> Option<[u8; 4]> {
    let mut keys: Vec<u8> = modifiers.to_vec();
    keys.push(key);
    crate::config::Binding::Keyboard { keys }.encode().ok()
}

/// Split a stored keyboard tuple into modifiers and its final key.
pub fn split_combo(value: [u8; 4]) -> Option<(Vec<u8>, u8)> {
    let [0, a, b, c] = value else { return None };
    let keys: Vec<u8> = [a, b, c].into_iter().filter(|u| *u != 0).collect();
    let (&key, mods) = keys.split_last()?;
    mods.iter()
        .all(|m| (224..=231).contains(m))
        .then(|| (mods.to_vec(), key))
}

pub const EFFECTS: [(u8, &str); 23] = [
    (0, "Off"),
    (1, "Static"),
    (2, "Breathing"),
    (3, "Neon"),
    (4, "Wave"),
    (5, "Ripple"),
    (6, "Raindrop"),
    (7, "Snake"),
    (8, "Reactive"),
    (9, "Coverage"),
    (10, "Sine wave"),
    (11, "Kaleidoscope"),
    (12, "Line wave"),
    (13, "Custom"),
    (14, "Laser"),
    (15, "Circle wave"),
    (16, "Dazzling"),
    (17, "Rain down"),
    (18, "Meteor"),
    (19, "Reactive off"),
    (23, "Train"),
    (24, "Fireworks"),
    (25, "User color"),
];

/// Number of option values an effect accepts (directions or pictures).
pub fn effect_options(mode: u8) -> u8 {
    match mode {
        4 => 4,
        13 => 3,
        7 | 11 | 12 | 15 => 2,
        _ => 1,
    }
}

/// Short label for a macro event.
pub fn macro_event_label(event: &MacroEvent) -> String {
    match event {
        MacroEvent::Keyboard { usage, .. } => usage_name(*usage)
            .map(str::to_owned)
            .unwrap_or_else(|| format!("Key {usage:#04x}")),
        MacroEvent::MouseButton { button, .. } => mouse_name(*button).unwrap_or("Mouse").into(),
        MacroEvent::Move { dx, dy, .. } => format!("Move {dx}, {dy}"),
    }
}

/// One keycap on the original M1W. Units are 1u key pitches.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Cap {
    pub slot: u8,
    pub usage: u8,
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

pub const KNOB_PRESS: u8 = 6;
pub const KNOB_LEFT: u8 = 94;
pub const KNOB_RIGHT: u8 = 95;
pub const FN_SLOT: u8 = 65;
pub const LAYOUT_WIDTH: f32 = 16.0;
pub const LAYOUT_HEIGHT: f32 = 6.25;

pub static LAYOUT: LazyLock<Vec<Cap>> = LazyLock::new(|| {
    // (usage, width); usage 0 is Fn. Gaps are encoded as negative widths.
    let rows: [&[(i16, f32)]; 6] = [
        &[
            (41, 1.0),
            (-1, 0.25),
            (58, 1.0),
            (59, 1.0),
            (60, 1.0),
            (61, 1.0),
            (-1, 0.25),
            (62, 1.0),
            (63, 1.0),
            (64, 1.0),
            (65, 1.0),
            (-1, 0.25),
            (66, 1.0),
            (67, 1.0),
            (68, 1.0),
            (69, 1.0),
            (-1, 0.25),
            (76, 1.0),
        ],
        &[
            (53, 1.0),
            (30, 1.0),
            (31, 1.0),
            (32, 1.0),
            (33, 1.0),
            (34, 1.0),
            (35, 1.0),
            (36, 1.0),
            (37, 1.0),
            (38, 1.0),
            (39, 1.0),
            (45, 1.0),
            (46, 1.0),
            (42, 2.0),
            (74, 1.0),
        ],
        &[
            (43, 1.5),
            (20, 1.0),
            (26, 1.0),
            (8, 1.0),
            (21, 1.0),
            (23, 1.0),
            (28, 1.0),
            (24, 1.0),
            (12, 1.0),
            (18, 1.0),
            (19, 1.0),
            (47, 1.0),
            (48, 1.0),
            (49, 1.5),
            (75, 1.0),
        ],
        &[
            (57, 1.75),
            (4, 1.0),
            (22, 1.0),
            (7, 1.0),
            (9, 1.0),
            (10, 1.0),
            (11, 1.0),
            (13, 1.0),
            (14, 1.0),
            (15, 1.0),
            (51, 1.0),
            (52, 1.0),
            (40, 2.25),
            (78, 1.0),
        ],
        &[
            (225, 2.25),
            (29, 1.0),
            (27, 1.0),
            (6, 1.0),
            (25, 1.0),
            (5, 1.0),
            (17, 1.0),
            (16, 1.0),
            (54, 1.0),
            (55, 1.0),
            (56, 1.0),
            (229, 1.75),
            (82, 1.0),
            (77, 1.0),
        ],
        &[
            (224, 1.25),
            (227, 1.25),
            (226, 1.25),
            (44, 6.25),
            (230, 1.0),
            (0, 1.0),
            (228, 1.0),
            (80, 1.0),
            (81, 1.0),
            (79, 1.0),
        ],
    ];
    let mut caps = Vec::new();
    for (r, row) in rows.iter().enumerate() {
        let y = r as f32 + if r > 0 { 0.25 } else { 0.0 };
        let mut x = 0.0;
        for &(usage, w) in *row {
            if usage >= 0 {
                let usage = usage as u8;
                let slot = if usage == 0 {
                    FN_SLOT
                } else {
                    key_slot(usage).expect("layout uses mapped usages")
                };
                caps.push(Cap {
                    slot,
                    usage,
                    x,
                    y,
                    w,
                    h: 1.,
                });
            }
            x += w;
        }
    }
    caps
});

/// Concrete vendor default tuple for a base or Fn slot.
pub fn default_binding(fn_layer: bool, slot: u8) -> [u8; 4] {
    static DEFAULTS: LazyLock<[Vec<[u8; 4]>; 2]> = LazyLock::new(|| {
        let doc: serde_json::Value =
            serde_json::from_str(include_str!("../docs/m1w-reference.json"))
                .expect("reference JSON is valid");
        let read = |name: &str| -> Vec<[u8; 4]> {
            serde_json::from_value(doc["matrices"][name]["slots"].clone())
                .expect("reference matrix has 128 tuples")
        };
        [read("defaultMatrix"), read("defaultFnMatrix")]
    });
    DEFAULTS[usize::from(fn_layer)][usize::from(slot)]
}

/// Legend for a physical key, including the knob and Fn.
pub fn slot_legend(slot: u8) -> String {
    match slot {
        FN_SLOT => "Fn".into(),
        KNOB_PRESS => "".into(),
        KNOB_LEFT => "Knob ↺".into(),
        KNOB_RIGHT => "Knob ↻".into(),
        _ => LAYOUT
            .iter()
            .find(|c| c.slot == slot)
            .map(|c| usage_legend(c.usage))
            .unwrap_or_else(|| format!("Slot {slot}")),
    }
}

/// Full name of a physical key, for headings.
pub fn slot_name(slot: u8) -> String {
    match slot {
        FN_SLOT => "Fn".into(),
        KNOB_PRESS | KNOB_LEFT | KNOB_RIGHT => "Knob".into(),
        _ => LAYOUT
            .iter()
            .find(|c| c.slot == slot)
            .and_then(|c| usage_name(c.usage))
            .map(str::to_owned)
            .unwrap_or_else(|| format!("Slot {slot}")),
    }
}

/// Map a GPUI/X11 style key name to a HID usage, for macro recording.
pub fn usage_from_key_name(key: &str) -> Option<u8> {
    let key = key.to_ascii_lowercase();
    if key.len() == 1 {
        let c = key.as_bytes()[0];
        return match c {
            b'a'..=b'z' => Some(c - b'a' + 4),
            b'1'..=b'9' => Some(c - b'1' + 30),
            b'0' => Some(39),
            b'-' => Some(45),
            b'=' => Some(46),
            b'[' => Some(47),
            b']' => Some(48),
            b'\\' => Some(49),
            b';' => Some(51),
            b'\'' => Some(52),
            b'`' => Some(53),
            b',' => Some(54),
            b'.' => Some(55),
            b'/' => Some(56),
            b' ' => Some(44),
            _ => None,
        };
    }
    if let Some(n) = key.strip_prefix('f').and_then(|n| n.parse::<u8>().ok()) {
        return match n {
            1..=12 => Some(57 + n),
            13..=24 => Some(91 + n),
            _ => None,
        };
    }
    Some(match key.as_str() {
        "enter" => 40,
        "escape" => 41,
        "backspace" => 42,
        "tab" => 43,
        "space" => 44,
        "capslock" => 57,
        "printscreen" => 70,
        "scrolllock" => 71,
        "pause" => 72,
        "insert" => 73,
        "home" => 74,
        "pageup" => 75,
        "delete" => 76,
        "end" => 77,
        "pagedown" => 78,
        "right" => 79,
        "left" => 80,
        "down" => 81,
        "up" => 82,
        "menu" => 101,
        _ => return None,
    })
}
