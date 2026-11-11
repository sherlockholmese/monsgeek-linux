//! Optional Linux-side remapping. The deterministic engine never performs I/O.
pub mod linux;
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum Action {
    Keys { keys: Vec<u16> },
    Launch { argv: Vec<String> },
}
impl Action {
    fn validate(&self) -> Result<()> {
        match self {
            Self::Keys { keys } => {
                ensure!(
                    keys.len() <= 8 && keys.iter().all(|k| (1..768).contains(k)),
                    "Actions accept up to eight Linux key codes (1..767)"
                );
                ensure!(
                    keys.iter().collect::<BTreeSet<_>>().len() == keys.len(),
                    "Duplicate output key"
                );
            }
            Self::Launch { argv } => ensure!(
                !argv.is_empty()
                    && argv.len() <= 32
                    && argv.iter().all(|s| !s.contains('\0') && s.len() <= 4096)
                    && !argv[0].is_empty(),
                "Launch needs a program and bounded arguments"
            ),
        }
        Ok(())
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum Behavior {
    Remap {
        action: Action,
    },
    TapHold {
        tap: Action,
        hold: Vec<u16>,
        timeout_ms: u64,
    },
    TapDance {
        actions: Vec<Action>,
        timeout_ms: u64,
    },
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rule {
    pub key: u16,
    #[serde(default)]
    pub app: Option<String>,
    pub behavior: Behavior,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Chord {
    pub keys: Vec<u16>,
    pub action: Action,
    #[serde(default)]
    pub app: Option<String>,
    pub window_ms: u64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub version: u8,
    pub devices: Vec<String>,
    #[serde(default)]
    pub rules: Vec<Rule>,
    #[serde(default)]
    pub chords: Vec<Chord>,
    /// Optional explicit executable returning the focused application ID.
    #[serde(default)]
    pub focus_command: Option<Vec<String>>,
}
fn context_matches(wanted: &Option<String>, current: &str) -> bool {
    wanted
        .as_ref()
        .is_none_or(|w| !current.is_empty() && w.eq_ignore_ascii_case(current))
}
fn valid_app(app: &Option<String>) -> bool {
    app.as_ref()
        .is_none_or(|s| !s.trim().is_empty() && s.len() <= 256 && !s.chars().any(char::is_control))
}
impl Config {
    pub fn validate(&self) -> Result<()> {
        ensure!(self.version == 1, "Unsupported host configuration version");
        ensure!(
            !self.devices.is_empty() && self.devices.len() <= 8,
            "Select 1–8 explicit /dev/input devices"
        );
        ensure!(
            self.devices
                .iter()
                .all(|s| s.starts_with("/dev/input/") && !s.contains("..") && !s.contains('\0')),
            "Input paths must be below /dev/input"
        );
        ensure!(
            self.devices.iter().collect::<BTreeSet<_>>().len() == self.devices.len(),
            "Duplicate input device"
        );
        ensure!(
            self.rules.len() <= 128 && self.chords.len() <= 32,
            "Too many rules"
        );
        let mut identities = BTreeSet::new();
        for r in &self.rules {
            ensure!(
                (1..768).contains(&r.key) && valid_app(&r.app),
                "Invalid source key or application"
            );
            ensure!(
                identities.insert((r.key, r.app.as_ref().map(|a| a.to_lowercase()))),
                "Duplicate source/application rule"
            );
            match &r.behavior {
                Behavior::Remap { action } => action.validate()?,
                Behavior::TapHold {
                    tap,
                    hold,
                    timeout_ms,
                } => {
                    tap.validate()?;
                    Action::Keys { keys: hold.clone() }.validate()?;
                    ensure!(
                        !hold.is_empty() && (50..=2000).contains(timeout_ms),
                        "Hold needs keys and a timeout of 50–2000 ms"
                    );
                }
                Behavior::TapDance {
                    actions,
                    timeout_ms,
                } => {
                    ensure!(
                        !actions.is_empty()
                            && actions.len() <= 4
                            && (50..=2000).contains(timeout_ms),
                        "Tap dance needs 1–4 actions and a timeout of 50–2000 ms"
                    );
                    for action in actions {
                        action.validate()?;
                    }
                }
            }
        }
        let mut chord_keys = BTreeSet::new();
        for chord in &self.chords {
            ensure!(
                (2..=4).contains(&chord.keys.len())
                    && chord.keys.iter().all(|k| (1..768).contains(k))
                    && valid_app(&chord.app)
                    && (10..=250).contains(&chord.window_ms),
                "Chord needs 2–4 keys and a window of 10–250 ms"
            );
            for key in &chord.keys {
                ensure!(
                    chord_keys.insert(*key) && !self.rules.iter().any(|r| r.key == *key),
                    "Chord keys cannot overlap another chord or source rule"
                );
            }
            chord.action.validate()?;
        }
        if let Some(argv) = &self.focus_command {
            Action::Launch { argv: argv.clone() }.validate()?;
        }
        Ok(())
    }
    pub fn example() -> Self {
        Self {
            version: 1,
            devices: vec!["/dev/input/by-id/REPLACE-WITH-YOUR-KEYBOARD-event-kbd".into()],
            rules: vec![
                Rule {
                    key: 58,
                    app: None,
                    behavior: Behavior::TapHold {
                        tap: Action::Keys { keys: vec![1] },
                        hold: vec![29],
                        timeout_ms: 200,
                    },
                },
                Rule {
                    key: 87,
                    app: None,
                    behavior: Behavior::TapDance {
                        actions: vec![
                            Action::Keys { keys: vec![113] },
                            Action::Keys { keys: vec![164] },
                        ],
                        timeout_ms: 220,
                    },
                },
                Rule {
                    key: 88,
                    app: Some("org.gnome.Nautilus".into()),
                    behavior: Behavior::Remap {
                        action: Action::Keys { keys: vec![29, 20] },
                    },
                },
                Rule {
                    key: 183,
                    app: None,
                    behavior: Behavior::Remap {
                        action: Action::Launch {
                            argv: vec!["xdg-open".into(), "https://example.com".into()],
                        },
                    },
                },
            ],
            chords: vec![Chord {
                keys: vec![36, 37],
                action: Action::Keys { keys: vec![1] },
                app: None,
                window_ms: 45,
            }],
            focus_command: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Output {
    Key { code: u16, value: i32 },
    Launch { argv: Vec<String> },
}
#[derive(Debug, Clone)]
enum Press {
    Held(Vec<u16>),
    HoldPending {
        tap: Action,
        hold: Vec<u16>,
        deadline: u64,
    },
    Dance {
        actions: Vec<Action>,
        timeout: u64,
        deadline: u64,
        count: usize,
        down: bool,
    },
    ChordPending {
        chord: usize,
        deadline: u64,
        order: u64,
    },
    Consumed(usize),
}
pub struct Engine {
    config: Config,
    pressed: BTreeMap<u16, Press>,
    counts: BTreeMap<u16, usize>,
    active_chords: BTreeMap<usize, Vec<u16>>,
    order: u64,
}
impl Engine {
    pub fn new(config: Config) -> Result<Self> {
        config.validate()?;
        Ok(Self {
            config,
            pressed: BTreeMap::new(),
            counts: BTreeMap::new(),
            active_chords: BTreeMap::new(),
            order: 0,
        })
    }
    fn down(&mut self, keys: &[u16], out: &mut Vec<Output>) {
        for &code in keys {
            let count = self.counts.entry(code).or_default();
            if *count == 0 {
                out.push(Output::Key { code, value: 1 });
            }
            *count += 1;
        }
    }
    fn up(&mut self, keys: &[u16], out: &mut Vec<Output>) {
        for &code in keys.iter().rev() {
            if let Some(count) = self.counts.get_mut(&code) {
                *count = count.saturating_sub(1);
                if *count == 0 {
                    out.push(Output::Key { code, value: 0 });
                    self.counts.remove(&code);
                }
            }
        }
    }
    fn action_down(&mut self, action: &Action, out: &mut Vec<Output>) -> Vec<u16> {
        match action {
            Action::Keys { keys } => {
                self.down(keys, out);
                keys.clone()
            }
            Action::Launch { argv } => {
                out.push(Output::Launch { argv: argv.clone() });
                Vec::new()
            }
        }
    }
    fn tap(&mut self, action: &Action, out: &mut Vec<Output>) {
        let held = self.action_down(action, out);
        self.up(&held, out);
    }
    pub fn tick(&mut self, now: u64) -> Vec<Output> {
        let mut out = Vec::new();
        // Preserve press order when several deferred chord keys expire together.
        let mut due: Vec<_> = self
            .pressed
            .iter()
            .filter_map(|(&key, press)| match press {
                Press::ChordPending {
                    deadline, order, ..
                } if now >= *deadline => Some((*order, key)),
                _ => None,
            })
            .collect();
        due.sort_unstable();
        for (_, key) in due {
            self.down(&[key], &mut out);
            self.pressed.insert(key, Press::Held(vec![key]));
        }
        let pending: Vec<_> = self.pressed.iter().map(|(&k, p)| (k, p.clone())).collect();
        for (key, press) in pending {
            match press {
                Press::HoldPending { hold, deadline, .. } if now >= deadline => {
                    self.down(&hold, &mut out);
                    self.pressed.insert(key, Press::Held(hold));
                }
                Press::Dance {
                    actions,
                    deadline,
                    count,
                    down: false,
                    ..
                } if now >= deadline => {
                    self.pressed.remove(&key);
                    self.tap(
                        &actions[count.saturating_sub(1).min(actions.len() - 1)],
                        &mut out,
                    );
                }
                _ => {}
            }
        }
        out
    }
    pub fn event(&mut self, code: u16, value: i32, now: u64, app: &str) -> Vec<Output> {
        let mut out = self.tick(now);
        if !matches!(value, 0..=2) {
            return out;
        }
        if value == 2 {
            if let Some(Press::Held(keys)) = self.pressed.get(&code)
                && let Some(&key) = keys.last()
            {
                out.push(Output::Key {
                    code: key,
                    value: 2,
                });
            }
            return out;
        }
        if value == 0 {
            match self.pressed.remove(&code) {
                Some(Press::Held(keys)) => self.up(&keys, &mut out),
                Some(Press::HoldPending { tap, .. }) => self.tap(&tap, &mut out),
                Some(Press::Dance {
                    actions,
                    timeout,
                    count,
                    ..
                }) => {
                    self.pressed.insert(
                        code,
                        Press::Dance {
                            actions,
                            timeout,
                            deadline: now.saturating_add(timeout),
                            count: count + 1,
                            down: false,
                        },
                    );
                }
                Some(Press::ChordPending { .. }) => {
                    self.tap(&Action::Keys { keys: vec![code] }, &mut out)
                }
                Some(Press::Consumed(group)) => {
                    if let Some(keys) = self.active_chords.remove(&group) {
                        self.up(&keys, &mut out);
                    }
                }
                None => {}
            }
            return out;
        }
        if let Some(Press::Dance { down, .. }) = self.pressed.get_mut(&code) {
            *down = true;
            return out;
        }
        if self.pressed.contains_key(&code) {
            return out;
        }
        let chord_candidate = self
            .config
            .chords
            .iter()
            .enumerate()
            .find(|(_, c)| c.keys.contains(&code) && context_matches(&c.app, app))
            .map(|(index, chord)| (index, chord.clone()));
        let mut flush: Vec<_> = self
            .pressed
            .iter()
            .filter_map(|(&key, press)| match press {
                Press::ChordPending { chord, order, .. }
                    if chord_candidate
                        .as_ref()
                        .is_none_or(|(candidate, _)| candidate != chord) =>
                {
                    Some((*order, key))
                }
                _ => None,
            })
            .collect();
        flush.sort_unstable();
        for (_, key) in flush {
            self.down(&[key], &mut out);
            self.pressed.insert(key, Press::Held(vec![key]));
        }
        // Another key commits a pending hold before that key's event. A dance
        // finishes early so its action cannot be reordered after unrelated input.
        let pending: Vec<_> = self.pressed.iter().map(|(&k, p)| (k, p.clone())).collect();
        for (key, press) in pending {
            match press {
                Press::HoldPending { hold, .. } => {
                    self.down(&hold, &mut out);
                    self.pressed.insert(key, Press::Held(hold));
                }
                Press::Dance {
                    actions,
                    count,
                    down: false,
                    ..
                } => {
                    self.pressed.remove(&key);
                    self.tap(
                        &actions[count.saturating_sub(1).min(actions.len() - 1)],
                        &mut out,
                    );
                }
                _ => {}
            }
        }
        if let Some(group) = self.pressed.values().find_map(|p| {
            if let Press::Consumed(group) = p {
                self.config.chords[*group]
                    .keys
                    .contains(&code)
                    .then_some(*group)
            } else {
                None
            }
        }) {
            self.pressed.insert(code, Press::Consumed(group));
            return out;
        }
        if let Some((index, chord)) = chord_candidate {
            self.order += 1;
            self.pressed.insert(
                code,
                Press::ChordPending {
                    chord: index,
                    deadline: now.saturating_add(chord.window_ms),
                    order: self.order,
                },
            );
            if chord.keys.iter().all(|k| matches!(self.pressed.get(k), Some(Press::ChordPending { chord, .. }) if *chord == index)) {
                let held = self.action_down(&chord.action, &mut out);
                self.active_chords.insert(index, held);
                for key in chord.keys { self.pressed.insert(key, Press::Consumed(index)); }
            }
            return out;
        }
        let rule = self
            .config
            .rules
            .iter()
            .filter(|r| r.key == code && context_matches(&r.app, app))
            .max_by_key(|r| r.app.is_some())
            .cloned();
        let press = match rule.map(|r| r.behavior) {
            Some(Behavior::Remap { action }) => Press::Held(self.action_down(&action, &mut out)),
            Some(Behavior::TapHold {
                tap,
                hold,
                timeout_ms,
            }) => Press::HoldPending {
                tap,
                hold,
                deadline: now.saturating_add(timeout_ms),
            },
            Some(Behavior::TapDance {
                actions,
                timeout_ms,
            }) => Press::Dance {
                actions,
                timeout: timeout_ms,
                deadline: now.saturating_add(timeout_ms),
                count: 0,
                down: true,
            },
            None => {
                self.down(&[code], &mut out);
                Press::Held(vec![code])
            }
        };
        self.pressed.insert(code, press);
        out
    }
    pub fn release_all(&mut self) -> Vec<Output> {
        let out = self
            .counts
            .keys()
            .map(|&code| Output::Key { code, value: 0 })
            .collect();
        self.pressed.clear();
        self.counts.clear();
        self.active_chords.clear();
        out
    }
}
