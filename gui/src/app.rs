//! Application state: the device baseline, the editable draft and the view
//! state of each page. Every edit changes only the draft; Apply sends the
//! difference as one verified transaction with a saved before-state.
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Instant,
};

use gpui_kit::component::{
    ActiveTheme, Disableable, Icon, IconName, Sizable, Theme, ThemeMode, TitleBar, WindowExt,
    button::{Button, ButtonVariants},
    color_picker::{ColorPickerEvent, ColorPickerState},
    h_flex,
    notification::Notification,
    select::{SelectEvent, SelectState},
    sidebar::{Sidebar, SidebarFooter, SidebarMenu, SidebarMenuItem},
    slider::{SliderEvent, SliderState, SliderValue},
    v_flex,
};
use gpui_kit::{
    AppContext, Context, Decorations, Entity, FocusHandle, Hsla, InteractiveElement, IntoElement,
    ParentElement, Render, Rgba, SharedString, StatefulInteractiveElement, Styled, Subscription,
    Window, div, prelude::FluentBuilder, px,
};
use monsgeek_linux::{
    access,
    config::{Macro, MacroEvent},
    device::{Onboard, Snapshot},
    firmware::{self, Image},
    keys::{self, EFFECTS, TARGETS},
    protocol::{Change, Lighting},
    transaction,
    transport::Kind,
};

use crate::{
    device::{self, Record},
    log::{self, Entry, Level},
    settings::{self, ThemeChoice},
    ui,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Page {
    Lighting,
    Keys,
    Macros,
    Performance,
    System,
    Backups,
    Logs,
}

impl Page {
    pub const ALL: [Page; 7] = [
        Page::Lighting,
        Page::Keys,
        Page::Macros,
        Page::Performance,
        Page::System,
        Page::Backups,
        Page::Logs,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Page::Lighting => "Lighting",
            Page::Keys => "Keys",
            Page::Macros => "Macros",
            Page::Performance => "Performance",
            Page::System => "System",
            Page::Backups => "Backups",
            Page::Logs => "Logs",
        }
    }

    fn icon(self) -> Icon {
        use gpui_kit::assets::IconName as L;
        Icon::new(match self {
            Page::Lighting => L::Lightbulb,
            Page::Keys => L::Keyboard,
            Page::Macros => L::ListVideo,
            Page::Performance => L::Gauge,
            Page::System => L::SlidersHorizontal,
            Page::Backups => L::Archive,
            Page::Logs => L::ScrollText,
        })
    }
}

pub enum Link {
    Reading,
    Ready,
    Failed(SharedString),
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Busy {
    Reading,
    Applying,
    Flashing,
}

/// Decoded lighting controls. `rgb` survives picture mode, which stores a
/// fixed color on the wire.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Light {
    pub mode: u8,
    pub speed: u8,
    pub brightness: u8,
    pub option: u8,
    pub rainbow: bool,
    pub rgb: [u8; 3],
}

impl Light {
    fn decode(l: &Lighting, last_rgb: [u8; 3]) -> Self {
        Self {
            mode: l.mode,
            speed: 5u8.saturating_sub(l.wire_speed).min(4),
            brightness: l.brightness.min(4),
            option: l.flags >> 4,
            rainbow: l.flags & 0x0f == 7,
            rgb: if l.mode == 13 {
                last_rgb
            } else if l.rgb == [250, 255, 250] {
                [255; 3]
            } else {
                l.rgb
            },
        }
    }
}

/// Live macro capture. Delays follow their event, as stored by the firmware.
pub struct Recording {
    pub last: Instant,
    pub held: Vec<u8>,
    pub modifiers: Vec<u8>,
}

pub struct Workbench {
    pub page: Page,
    pub link: Link,
    pub busy: Option<Busy>,
    pub baseline: Option<Snapshot>,
    pub draft: Option<Snapshot>,
    pub history: Vec<Record>,
    /// Latest background poll; `None` before the first one.
    pub presence: Option<device::Presence>,
    presence_error: Option<String>,
    /// Transport of the loaded baseline.
    pub transport: Option<Kind>,
    /// The running full read.
    pub progress: device::ReadProgress,
    pub read_started: Option<Instant>,
    /// Udev rule state, and whether an installation is running.
    pub access: access::Status,
    pub access_busy: bool,
    /// The last read failed for lack of permission.
    pub access_denied: bool,
    /// One automatic retry was spent on the current failure.
    retried: bool,
    pub logs: Vec<Entry>,
    pub errors_only: bool,
    pub firmware_image: Option<(PathBuf, Image)>,
    pub firmware_status: Arc<Mutex<String>>,
    // Keys
    pub layer: u8,
    pub fn_layer: bool,
    pub selected: Option<u8>,
    // Lighting
    pub last_rgb: [u8; 3],
    // Macros
    pub macro_slot: u8,
    pub macro_events: Vec<MacroEvent>,
    pub macro_repeat: u16,
    pub macro_error: Option<SharedString>,
    pub recording: Option<Recording>,
    pub record_focus: FocusHandle,
    // Control state
    pub effect: Entity<SelectState<Vec<SharedString>>>,
    pub target: Entity<SelectState<Vec<SharedString>>>,
    pub color: Entity<ColorPickerState>,
    pub brightness: Entity<SliderState>,
    pub speed: Entity<SliderState>,
    pub debounce: Entity<SliderState>,
    pub sleep: [Entity<SliderState>; 4],
    _subscriptions: Vec<Subscription>,
}

fn slider(min: f32, max: f32, cx: &mut Context<Workbench>) -> Entity<SliderState> {
    cx.new(|_| {
        SliderState::new()
            .min(min)
            .max(max)
            .step(1.)
            .default_value(min)
    })
}

fn single(value: &SliderValue) -> f32 {
    match value {
        SliderValue::Single(v) => *v,
        SliderValue::Range(_, v) => *v,
    }
}

pub fn hsla(rgb: [u8; 3]) -> Hsla {
    Rgba {
        r: f32::from(rgb[0]) / 255.,
        g: f32::from(rgb[1]) / 255.,
        b: f32::from(rgb[2]) / 255.,
        a: 1.,
    }
    .into()
}

fn rgb_of(color: Hsla) -> [u8; 3] {
    let c = color.to_rgb();
    [c.r, c.g, c.b].map(|v| (v.clamp(0., 1.) * 255.).round() as u8)
}

impl Workbench {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut this = Self::build(window, cx);
        this.access = access::status();
        // Debug builds can freeze the progress screen for visual QA.
        if let Some(done) = cfg!(debug_assertions)
            .then(|| std::env::var("M1W_PROGRESS").ok()?.parse::<usize>().ok())
            .flatten()
        {
            this.busy = Some(Busy::Reading);
            this.read_started = Some(Instant::now() - std::time::Duration::from_secs(12));
            this.progress
                .done
                .store(done, std::sync::atomic::Ordering::Relaxed);
            *this.progress.kind.lock().unwrap() = Some(
                if std::env::var("M1W_TRANSPORT").as_deref() == Ok("bluetooth") {
                    Kind::Bluetooth
                } else {
                    Kind::Dongle
                },
            );
            return this;
        }
        this.reload(window, cx);
        this.watch_presence(window, cx);
        let forced = cfg!(debug_assertions) && std::env::var_os("M1W_OFFER_ACCESS").is_some();
        if forced || access::should_offer() {
            // The dialog layer exists only once the window has rendered.
            cx.spawn_in(window, async move |this, cx| {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(400))
                    .await;
                this.update_in(cx, |this, window, cx| this.offer_access(window, cx))
                    .ok();
            })
            .detach();
        }
        this
    }

    // ---- Access rule -----------------------------------------------------

    /// Show the udev rule and offer to install it.
    pub fn offer_access(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let view = cx.entity();
        window.open_dialog(cx, move |dialog, _, cx| {
            let theme = cx.theme().clone();
            let state = view.read(cx);
            let busy = state.access_busy;
            let update = state.access == access::Status::Outdated;
            let decline = view.clone();
            let install = view.clone();
            dialog
                .w(px(640.))
                .title(if update {
                    "Update access rule"
                } else {
                    "Install access rule"
                })
                .child(
                    v_flex()
                        .gap_2()
                        .child(
                            div()
                                .text_sm()
                                .font_family(theme.mono_font_family.clone())
                                .child(access::PATH),
                        )
                        .child(
                            div()
                                .id("rule")
                                .max_h(px(260.))
                                .overflow_y_scroll()
                                .p_3()
                                .rounded(theme.radius)
                                .bg(theme.muted)
                                .text_xs()
                                .font_family(theme.mono_font_family.clone())
                                .text_color(theme.muted_foreground)
                                .child(access::RULE.trim_end().to_string()),
                        ),
                )
                .footer(
                    h_flex()
                        .w_full()
                        .justify_end()
                        .gap_2()
                        .child(
                            Button::new("rule-later")
                                .ghost()
                                .label("Not now")
                                .disabled(busy)
                                .on_click(move |_, window, cx| {
                                    decline.update(cx, |this, _| {
                                        if let Err(e) = access::decline() {
                                            this.log(Entry::error("Cannot record the choice", &e));
                                        }
                                    });
                                    window.close_dialog(cx);
                                }),
                        )
                        .child(
                            Button::new("rule-install")
                                .primary()
                                .icon(gpui_kit::assets::IconName::ShieldCheck)
                                .label(if update { "Update" } else { "Install" })
                                .loading(busy)
                                .disabled(busy)
                                .on_click(move |_, window, cx| {
                                    install.update(cx, |this, cx| this.install_access(window, cx));
                                }),
                        ),
                )
        });
    }

    /// Install with the graphical password prompt, or in a terminal when the
    /// session has no polkit agent.
    pub fn install_access(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.access_busy {
            return;
        }
        self.access_busy = true;
        cx.notify();
        let task = cx.background_spawn(async move {
            match access::install(access::Elevate::Pkexec) {
                Err(access::InstallError::NoAgent) => {
                    access::install_in_terminal().map_err(access::InstallError::Failed)?;
                    // The terminal reports its own result; wait for the file.
                    for _ in 0..180 {
                        if access::status() == access::Status::Installed {
                            return Ok(());
                        }
                        std::thread::sleep(std::time::Duration::from_secs(1));
                    }
                    Err(access::InstallError::Cancelled)
                }
                other => other,
            }
        });
        cx.spawn_in(window, async move |this, cx| {
            let result = task.await;
            this.update_in(cx, |this, window, cx| {
                this.access_busy = false;
                this.access = access::status();
                match result {
                    Ok(()) => {
                        this.log(Entry::new(
                            Level::Info,
                            "Access rule installed",
                            vec![access::PATH.into()],
                        ));
                        window.close_dialog(cx);
                        window
                            .push_notification(Notification::success("Access rule installed"), cx);
                        if this.baseline.is_none() {
                            this.access_denied = false;
                            this.reload(window, cx);
                        }
                    }
                    Err(access::InstallError::Cancelled) => {
                        this.log(Entry::info("Access rule not installed: cancelled"));
                    }
                    Err(e) => {
                        let e = anyhow::anyhow!(e);
                        this.log(Entry::error("Access rule not installed", &e));
                        window.push_notification(Notification::error(first_line(&e)), cx);
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Watch for the keyboard every two seconds: cable plugged or unplugged,
    /// wireless wake and sleep, battery. Reads automatically when it becomes
    /// reachable while nothing is loaded.
    fn watch_presence(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        cx.spawn_in(window, async move |this, cx| {
            loop {
                let idle = this.update(cx, |this, _| this.busy.is_none());
                match idle {
                    Err(_) => break,
                    Ok(false) => {}
                    Ok(true) => {
                        let result = cx.background_spawn(async { device::presence() }).await;
                        if this
                            .update_in(cx, |this, window, cx| {
                                this.presence_changed(result, window, cx)
                            })
                            .is_err()
                        {
                            break;
                        }
                    }
                }
                cx.background_executor()
                    .timer(std::time::Duration::from_secs(2))
                    .await;
            }
        })
        .detach();
    }

    fn presence_changed(
        &mut self,
        result: anyhow::Result<device::Presence>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let now = match result {
            Ok(now) => {
                self.presence_error = None;
                now
            }
            Err(e) => {
                // A busy node means another program is configuring it; skip.
                let text = format!("{e:#}");
                if !text.contains("busy") && self.presence_error.as_ref() != Some(&text) {
                    self.log(Entry::error("Keyboard status failed", &e));
                    self.presence_error = Some(text);
                }
                return;
            }
        };
        let before = self.presence;
        if before == Some(now) {
            return;
        }
        let was = before.unwrap_or(device::Presence {
            usb: false,
            wireless: None,
        });
        if now.usb != was.usb && before.is_some() {
            self.log(Entry::info(if now.usb {
                "Cable connected"
            } else {
                "Cable disconnected"
            }));
        }
        let state = |w: Option<(Kind, Option<u8>)>| w.map(|(k, b)| (k, b.is_some()));
        if state(now.wireless) != state(was.wireless) {
            let message = match now.wireless {
                Some((kind, Some(battery))) => Some(format!("{kind} online, {battery}%")),
                Some((kind, None)) => Some(format!("{kind} keyboard asleep")),
                None if was.wireless.is_some() => Some("Wireless link removed".into()),
                None => None,
            };
            if let Some(message) = message {
                self.log(Entry::info(message));
            }
        }
        self.presence = Some(now);
        // Read when the keyboard appears or wakes, and once on the first poll
        // if the launch read failed.
        let failed = matches!(self.link, Link::Failed(_));
        if now.reachable()
            && self.baseline.is_none()
            && (before.is_some() && !was.reachable() || before.is_none() && failed)
        {
            self.reload(window, cx);
        }
        cx.notify();
    }

    /// Construct without touching the device.
    fn build(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let effects: Vec<SharedString> = EFFECTS.iter().map(|(_, n)| (*n).into()).collect();
        let effect = cx.new(|cx| SelectState::new(effects, None, window, cx));
        let targets: Vec<SharedString> = TARGETS.iter().map(|t| t.label.clone().into()).collect();
        let target = cx.new(|cx| SelectState::new(targets, None, window, cx).searchable(true));
        let color = cx.new(|cx| ColorPickerState::new(window, cx).default_value(hsla([255; 3])));
        let brightness = slider(0., 4., cx);
        let speed = slider(0., 4., cx);
        let debounce = slider(1., 10., cx);
        let sleep = [
            slider(1., 60., cx),
            slider(1., 60., cx),
            slider(10., 60., cx),
            slider(10., 60., cx),
        ];

        let mut subs = vec![
            cx.subscribe_in(&effect, window, |this, _, ev: &SelectEvent<_>, _, cx| {
                let SelectEvent::Confirm(Some(name)) = ev else {
                    return;
                };
                if let Some((mode, _)) = EFFECTS.iter().find(|(_, n)| *n == name.as_ref()) {
                    let mode = *mode;
                    this.edit_light(cx, |l| l.mode = mode);
                }
            }),
            cx.subscribe_in(&target, window, |this, _, ev: &SelectEvent<_>, _, cx| {
                let SelectEvent::Confirm(Some(name)) = ev else {
                    return;
                };
                if let Some(t) = TARGETS.iter().find(|t| t.label == name.as_ref()) {
                    let value = t.value;
                    this.bind_selected(value, cx);
                }
            }),
            cx.subscribe_in(&color, window, |this, _, ev: &ColorPickerEvent, _, cx| {
                let ColorPickerEvent::Change(Some(c)) = ev else {
                    return;
                };
                let rgb = rgb_of(*c);
                this.last_rgb = rgb;
                this.edit_light(cx, |l| l.rgb = rgb);
            }),
            cx.subscribe_in(&brightness, window, |this, _, ev: &SliderEvent, _, cx| {
                let SliderEvent::Change(v) = ev else { return };
                let v = single(v) as u8;
                this.edit_light(cx, |l| l.brightness = v);
            }),
            cx.subscribe_in(&speed, window, |this, _, ev: &SliderEvent, _, cx| {
                let SliderEvent::Change(v) = ev else { return };
                let v = single(v) as u8;
                this.edit_light(cx, |l| l.speed = v);
            }),
            cx.subscribe_in(&debounce, window, |this, _, ev: &SliderEvent, _, cx| {
                let SliderEvent::Change(v) = ev else { return };
                let v = single(v) as u8;
                this.edit_onboard(cx, |o| o.debounce = v);
            }),
        ];
        for (i, s) in sleep.iter().enumerate() {
            subs.push(
                cx.subscribe_in(s, window, move |this, _, ev: &SliderEvent, _, cx| {
                    let SliderEvent::Change(v) = ev else { return };
                    let seconds = single(v) as u16 * 60;
                    this.edit_onboard(cx, |o| match i {
                        0 => o.sleep.time_bt = seconds,
                        1 => o.sleep.time_24 = seconds,
                        2 => o.sleep.deep_time_bt = seconds,
                        _ => o.sleep.deep_time_24 = seconds,
                    });
                }),
            );
        }

        // Debug builds can open a page and theme directly for visual QA.
        let page = if cfg!(debug_assertions) {
            match std::env::var("M1W_PAGE").as_deref() {
                Ok("keys") => Page::Keys,
                Ok("macros") => Page::Macros,
                Ok("performance") => Page::Performance,
                Ok("system") => Page::System,
                Ok("backups") => Page::Backups,
                Ok("logs") => Page::Logs,
                _ => Page::Lighting,
            }
        } else {
            Page::Lighting
        };
        if cfg!(debug_assertions)
            && let Ok(theme) = std::env::var("M1W_THEME")
        {
            Theme::change(
                if theme == "dark" {
                    ThemeMode::Dark
                } else {
                    ThemeMode::Light
                },
                Some(window),
                cx,
            );
        }
        Self {
            page,
            link: Link::Reading,
            busy: None,
            baseline: None,
            draft: None,
            history: device::history(),
            logs: Vec::new(),
            errors_only: false,
            firmware_image: None,
            firmware_status: Arc::new(Mutex::new(String::new())),
            presence: None,
            presence_error: None,
            transport: None,
            progress: device::ReadProgress::default(),
            read_started: None,
            access: access::Status::Installed,
            access_busy: false,
            access_denied: false,
            retried: false,
            layer: 0,
            fn_layer: false,
            selected: cfg!(debug_assertions)
                .then(|| std::env::var("M1W_SELECT").ok()?.parse().ok())
                .flatten(),
            last_rgb: [255; 3],
            macro_slot: 0,
            macro_events: Vec::new(),
            macro_repeat: 1,
            macro_error: None,
            recording: None,
            record_focus: cx.focus_handle(),
            effect,
            target,
            color,
            brightness,
            speed,
            debounce,
            sleep,
            _subscriptions: subs,
        }
    }

    /// Adopt a fresh device read as both baseline and draft.
    fn loaded(&mut self, snapshot: Snapshot, window: &mut Window, cx: &mut Context<Self>) {
        self.link = Link::Ready;
        self.baseline = Some(snapshot.clone());
        self.draft = Some(snapshot);
        self.sync_controls(window, cx);
    }

    // ---- Device ----------------------------------------------------------

    pub fn reload(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy.is_some() {
            return;
        }
        self.busy = Some(Busy::Reading);
        if self.baseline.is_none() {
            self.link = Link::Reading;
        }
        self.read_started = Some(Instant::now());
        self.progress
            .done
            .store(0, std::sync::atomic::Ordering::Relaxed);
        cx.notify();
        let progress = self.progress.clone();
        let task = cx.background_spawn(async move { device::read(progress) });
        // Repaint progress while the read runs.
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(150))
                    .await;
                let reading = this.update(cx, |this, cx| {
                    cx.notify();
                    this.busy == Some(Busy::Reading)
                });
                if !matches!(reading, Ok(true)) {
                    break;
                }
            }
        })
        .detach();
        cx.spawn_in(window, async move |this, cx| {
            let result = task.await;
            this.update_in(cx, |this, window, cx| {
                this.busy = None;
                match result {
                    Ok((snapshot, kind)) => {
                        this.retried = false;
                        this.access_denied = false;
                        this.transport = Some(kind);
                        this.log(Entry::info(format!(
                            "Read keyboard over {kind}, firmware {:X}.{:02X}",
                            snapshot.usb_revision >> 8,
                            snapshot.usb_revision & 0xff
                        )));
                        this.loaded(snapshot, window, cx);
                        if cfg!(debug_assertions)
                            && let Some(mode) = std::env::var("M1W_EFFECT")
                                .ok()
                                .and_then(|m| m.parse().ok())
                        {
                            this.edit_light(cx, |l| l.mode = mode);
                            this.sync_controls(window, cx);
                        }
                    }
                    Err(e) => {
                        this.log(Entry::error("Read failed", &e));
                        this.access_denied = access::is_permission_error(&e);
                        if this.access_denied {
                            this.access = access::status();
                        }
                        // A radio hiccup should not need a click: retry once
                        // while the keyboard is still reachable.
                        if !this.retried && this.presence.is_some_and(|p| p.reachable()) {
                            this.retried = true;
                            cx.spawn_in(window, async move |this, cx| {
                                cx.background_executor()
                                    .timer(std::time::Duration::from_secs(3))
                                    .await;
                                this.update_in(cx, |this, window, cx| {
                                    if this.baseline.is_none() {
                                        this.reload(window, cx)
                                    }
                                })
                                .ok();
                            })
                            .detach();
                        }
                        let message = first_line(&e);
                        if this.baseline.is_some() {
                            window.push_notification(Notification::error(message), cx);
                        } else {
                            this.link = Link::Failed(message);
                        }
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    pub fn apply(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (Some(baseline), Some(draft)) = (self.baseline.clone(), self.draft.clone()) else {
            return;
        };
        if self.busy.is_some() {
            return;
        }
        let changes = match self.pending() {
            Ok(changes) => changes,
            Err(e) => {
                self.log(Entry::error("Apply refused", &e));
                window.push_notification(Notification::error(first_line(&e)), cx);
                return;
            }
        };
        let summary: Vec<String> = changes.iter().map(describe).collect();
        self.busy = Some(Busy::Applying);
        cx.notify();
        let task = cx.background_spawn(async move { device::apply(baseline, draft) });
        cx.spawn_in(window, async move |this, cx| {
            let result = task.await;
            this.update_in(cx, |this, window, cx| {
                this.busy = None;
                this.history = device::history();
                match result {
                    Ok((before, snapshot)) => {
                        let mut detail = summary;
                        detail.push(format!("Before-state {}", before.display()));
                        this.log(Entry::new(
                            Level::Info,
                            plural(detail.len() - 1, "Applied 1 change", "Applied {} changes"),
                            detail,
                        ));
                        this.baseline = Some(snapshot.clone());
                        this.draft = Some(snapshot);
                        this.sync_controls(window, cx);
                        window.push_notification(Notification::success("Applied"), cx);
                    }
                    Err(e) => {
                        let mut entry = Entry::error("Apply failed", &e);
                        entry
                            .detail
                            .extend(summary.iter().map(|c| format!("Pending {c}")));
                        this.log(entry);
                        window.push_notification(
                            Notification::error(first_line(&e)).title("Apply failed"),
                            cx,
                        );
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Return the device to the state saved before the latest apply.
    pub fn undo(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(record) = self.history.first().cloned() else {
            return;
        };
        self.log(Entry::info("Undo latest apply"));
        self.restore(record.snapshot, window, cx);
        self.apply(window, cx);
    }

    /// Load a snapshot as the draft; Apply then writes only its differences.
    pub fn restore(&mut self, target: Snapshot, window: &mut Window, cx: &mut Context<Self>) {
        let Some(base) = &self.baseline else { return };
        if target.usb_revision != base.usb_revision {
            self.log(Entry::new(
                Level::Error,
                "Restore refused",
                vec![format!(
                    "Snapshot firmware {:04X}, keyboard firmware {:04X}",
                    target.usb_revision, base.usb_revision
                )],
            ));
            window.push_notification(
                Notification::error("Snapshot is from different firmware"),
                cx,
            );
            return;
        }
        let mut target = target;
        if target.onboard.is_none() {
            // Format 1 exports lack onboard settings; keep current values.
            target.onboard = base.onboard.clone();
        }
        self.draft = Some(target);
        self.sync_controls(window, cx);
        cx.notify();
    }

    pub fn discard(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.draft = self.baseline.clone();
        self.sync_controls(window, cx);
        cx.notify();
    }

    pub fn export(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(snapshot) = self.baseline.clone() else {
            return;
        };
        let dir = std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."));
        let ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or_default();
        let path = cx.prompt_for_new_path(&dir, Some(&format!("m1w-{ms}.json")));
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(path))) = path.await else {
                return;
            };
            let result = device::export(&snapshot, &path);
            this.update_in(cx, |this, window, cx| {
                let note = match result {
                    Ok(()) => {
                        this.log(Entry::new(
                            Level::Info,
                            "Exported",
                            vec![path.display().to_string()],
                        ));
                        Notification::success("Exported")
                    }
                    Err(e) => {
                        let mut entry = Entry::error("Export failed", &e);
                        entry.detail.insert(0, path.display().to_string());
                        this.log(entry);
                        Notification::error(first_line(&e))
                    }
                };
                window.push_notification(note, cx);
            })
            .ok();
        })
        .detach();
    }

    pub fn open_file(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let paths = cx.prompt_for_paths(gpui_kit::PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Open".into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(paths))) = paths.await else {
                return;
            };
            let Some(path) = paths.into_iter().next() else {
                return;
            };
            let result = device::load(&path);
            this.update_in(cx, |this, window, cx| match result {
                Ok(snapshot) => {
                    this.log(Entry::new(
                        Level::Info,
                        "Opened",
                        vec![path.display().to_string()],
                    ));
                    this.restore(snapshot, window, cx)
                }
                Err(e) => {
                    let mut entry = Entry::error("Open failed", &e);
                    entry.detail.insert(0, path.display().to_string());
                    this.log(entry);
                    window.push_notification(Notification::error(first_line(&e)), cx)
                }
            })
            .ok();
        })
        .detach();
    }

    // ---- Experimental firmware ------------------------------------------

    pub fn choose_firmware(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy.is_some() {
            return;
        }
        let paths = cx.prompt_for_paths(gpui_kit::PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Select original M1W firmwareFile.bin".into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(paths))) = paths.await else {
                return;
            };
            let Some(path) = paths.into_iter().next() else {
                return;
            };
            let load_path = path.clone();
            let result = cx
                .background_spawn(async move { Image::load(&load_path) })
                .await;
            this.update_in(cx, |this, window, cx| {
                match result {
                    Ok(image) => {
                        this.log(Entry::new(
                            Level::Info,
                            "Firmware selected",
                            vec![
                                path.display().to_string(),
                                format!("SHA-256 {}", image.info().sha256),
                            ],
                        ));
                        this.firmware_image = Some((path, image));
                    }
                    Err(error) => {
                        this.log(Entry::error("Firmware file rejected", &error));
                        window.push_notification(Notification::error(first_line(&error)), cx);
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    pub fn can_flash_firmware(&self) -> bool {
        self.busy.is_none()
            && self.transport == Some(Kind::Usb)
            && self.baseline.is_some()
            && self.firmware_image.is_some()
            && self.pending().is_ok_and(|changes| changes.is_empty())
    }

    pub fn confirm_firmware(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.can_flash_firmware() {
            return;
        }
        let Some((path, image)) = &self.firmware_image else {
            return;
        };
        let filename = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        let hash = image.info().sha256.clone();
        let view = cx.entity();
        window.open_dialog(cx, move |dialog, _, cx| {
            let flash = view.clone();
            let confirmed_hash = hash.clone();
            dialog
                .title("Flash firmware?")
                .w(px(640.))
                .child(
                    v_flex()
                        .gap_3()
                        .child(filename.clone())
                        .child(div().text_sm().child(firmware::WARNING))
                        .child(
                            div().text_sm().child(
                                "Use only original M1W firmware. The file model is unverified.",
                            ),
                        )
                        .child(
                            div()
                                .text_xs()
                                .font_family(cx.theme().mono_font_family.clone())
                                .child(hash.clone()),
                        ),
                )
                .footer(
                    h_flex()
                        .w_full()
                        .justify_end()
                        .gap_2()
                        .child(
                            Button::new("firmware-cancel")
                                .ghost()
                                .label("Cancel")
                                .on_click(|_, window, cx| window.close_dialog(cx)),
                        )
                        .child(
                            Button::new("firmware-confirm")
                                .danger()
                                .label("Flash firmware")
                                .on_click(move |_, window, cx| {
                                    window.close_dialog(cx);
                                    flash.update(cx, |this, cx| {
                                        this.flash_firmware(&confirmed_hash, window, cx)
                                    });
                                }),
                        ),
                )
        });
    }

    fn flash_firmware(&mut self, expected_hash: &str, window: &mut Window, cx: &mut Context<Self>) {
        if !self.can_flash_firmware() {
            return;
        }
        let Some((_, image)) = self.firmware_image.clone() else {
            return;
        };
        if image.info().sha256 != expected_hash {
            window.push_notification(
                Notification::error("Firmware file changed. Review it again."),
                cx,
            );
            return;
        }
        let Some(baseline) = self.baseline.clone() else {
            return;
        };
        let before = match device::firmware_backup_path() {
            Ok(path) => path,
            Err(error) => {
                self.log(Entry::error("Firmware backup failed", &error));
                window.push_notification(Notification::error(first_line(&error)), cx);
                return;
            }
        };
        self.log(Entry::new(
            Level::Info,
            "Experimental firmware update started",
            vec![
                format!("SHA-256 {}", image.info().sha256),
                format!("Configuration backup {}", before.display()),
            ],
        ));
        self.busy = Some(Busy::Flashing);
        *self
            .firmware_status
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = "Saving configuration".into();
        cx.notify();
        let status = self.firmware_status.clone();
        let task =
            cx.background_spawn(
                async move { device::flash_firmware(image, baseline, before, status) },
            );
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(150))
                    .await;
                if !matches!(
                    this.update(cx, |this, cx| {
                        cx.notify();
                        this.busy == Some(Busy::Flashing)
                    }),
                    Ok(true)
                ) {
                    break;
                }
            }
        })
        .detach();
        cx.spawn_in(window, async move |this, cx| {
            let result = task.await;
            this.update_in(cx, |this, window, cx| {
                this.busy = None;
                this.firmware_status
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .clear();
                this.baseline = None;
                this.draft = None;
                this.transport = None;
                match result {
                    Ok(outcome) => {
                        this.log(Entry::new(
                            Level::Info,
                            "Firmware completion confirmed",
                            vec![format!(
                                "Revision {:04x} -> {:04x}",
                                outcome.old_revision, outcome.new_revision
                            )],
                        ));
                        this.firmware_image = None;
                        window.push_notification(
                            Notification::success("Firmware completion confirmed"),
                            cx,
                        );
                        this.reload(window, cx);
                    }
                    Err(error) => {
                        this.log(Entry::error("Firmware update failed", &error));
                        this.link = Link::Failed(
                            "Firmware update failed. Check Logs. Do not retry automatically."
                                .into(),
                        );
                        window.push_notification(
                            Notification::error("Firmware update failed. Check Logs."),
                            cx,
                        );
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    // ---- Draft edits -----------------------------------------------------

    pub fn pending(&self) -> anyhow::Result<Vec<Change>> {
        match (&self.baseline, &self.draft) {
            (Some(b), Some(d)) => {
                let changes = b.restore_changes(d)?;
                transaction::check_picture(&changes, b)?;
                Ok(changes)
            }
            _ => Ok(Vec::new()),
        }
    }

    pub fn editable(&self) -> bool {
        !matches!(self.busy, Some(Busy::Applying | Busy::Flashing)) && self.draft.is_some()
    }

    pub fn edit(&mut self, cx: &mut Context<Self>, f: impl FnOnce(&mut Snapshot)) {
        if !self.editable() {
            return;
        }
        if let Some(d) = &mut self.draft {
            f(d);
            cx.notify();
        }
    }

    pub fn edit_onboard(&mut self, cx: &mut Context<Self>, f: impl FnOnce(&mut Onboard)) {
        self.edit(cx, |d| {
            if let Some(o) = &mut d.onboard {
                f(o)
            }
        });
    }

    pub fn light(&self) -> Option<Light> {
        self.draft
            .as_ref()
            .map(|d| Light::decode(&d.lighting, self.last_rgb))
    }

    pub fn edit_light(&mut self, cx: &mut Context<Self>, f: impl FnOnce(&mut Light)) {
        let Some(mut light) = self.light() else {
            return;
        };
        let before = light;
        f(&mut light);
        if light == before {
            return;
        }
        let max = keys::effect_options(light.mode) - 1;
        light.option = light.option.min(max);
        if let Ok(settings) = Lighting::new(
            light.mode,
            light.speed,
            light.brightness,
            light.option,
            light.rainbow,
            light.rgb,
        ) {
            self.edit(cx, |d| d.lighting = settings);
        }
    }

    /// Whether per-key colors can be edited: the device must already show the
    /// same custom picture, because a snapshot holds only the selected one.
    pub fn can_paint(&self) -> bool {
        match (&self.baseline, &self.draft) {
            (Some(b), Some(d)) => {
                b.lighting.mode == 13
                    && d.lighting.mode == 13
                    && transaction::picture(&b.lighting) == transaction::picture(&d.lighting)
            }
            _ => false,
        }
    }

    pub fn paint(&mut self, slot: u8, cx: &mut Context<Self>) {
        if !self.can_paint() {
            return;
        }
        let rgb = self.last_rgb;
        self.edit_onboard(cx, |o| {
            let i = usize::from(slot) * 3;
            o.rgb[i..i + 3].copy_from_slice(&rgb);
        });
    }

    pub fn paint_all(&mut self, cx: &mut Context<Self>) {
        if !self.can_paint() {
            return;
        }
        let rgb = self.last_rgb;
        self.edit_onboard(cx, |o| {
            for px in o.rgb.chunks_mut(3) {
                px.copy_from_slice(&rgb);
            }
        });
    }

    pub fn binding(&self, snapshot: &Snapshot, slot: u8) -> [u8; 4] {
        let maps = if self.fn_layer {
            &snapshot.fn_keymaps
        } else {
            &snapshot.keymaps
        };
        let i = usize::from(slot) * 4;
        maps[usize::from(self.layer)][i..i + 4]
            .try_into()
            .expect("validated map")
    }

    pub fn set_binding(&mut self, slot: u8, value: [u8; 4], cx: &mut Context<Self>) {
        let (layer, fn_layer) = (usize::from(self.layer), self.fn_layer);
        self.edit(cx, |d| {
            let maps = if fn_layer {
                &mut d.fn_keymaps
            } else {
                &mut d.keymaps
            };
            let i = usize::from(slot) * 4;
            maps[layer][i..i + 4].copy_from_slice(&value);
        });
    }

    fn bind_selected(&mut self, value: Option<[u8; 4]>, cx: &mut Context<Self>) {
        let Some(slot) = self.selected else { return };
        let value = value.unwrap_or_else(|| keys::default_binding(self.fn_layer, slot));
        self.set_binding(slot, value, cx);
    }

    pub fn reset_layer(&mut self, cx: &mut Context<Self>) {
        for slot in 0..128 {
            let value = keys::default_binding(self.fn_layer, slot);
            self.set_binding(slot, value, cx);
        }
    }

    pub fn select_key(&mut self, slot: Option<u8>, window: &mut Window, cx: &mut Context<Self>) {
        self.selected = slot;
        self.sync_target(window, cx);
        cx.notify();
    }

    pub fn sync_target(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let label = match (self.selected, &self.draft) {
            (Some(slot), Some(d)) => {
                let value = self.binding(d, slot);
                TARGETS
                    .iter()
                    .find(|t| t.value == Some(value))
                    .map(|t| SharedString::from(t.label.clone()))
            }
            _ => None,
        };
        self.target.update(cx, |s, cx| match label {
            Some(label) => s.set_selected_value(&label, window, cx),
            None => s.set_selected_index(None, window, cx),
        });
    }

    // ---- Macros ----------------------------------------------------------

    pub fn macro_image(&self, slot: u8) -> Option<&Vec<u8>> {
        self.draft
            .as_ref()?
            .onboard
            .as_ref()
            .map(|o| &o.macros[usize::from(slot)])
    }

    pub fn open_macro(&mut self, slot: u8, cx: &mut Context<Self>) {
        self.recording = None;
        self.macro_slot = slot;
        self.macro_error = None;
        let decoded = self.macro_image(slot).map(|d| Macro::decode(d));
        match decoded {
            Some(Ok(m)) => {
                self.macro_repeat = m.repeat;
                self.macro_events = m.events;
            }
            Some(Err(_)) if self.macro_image(slot).is_some_and(|d| d[..2] == [0, 0]) => {
                self.macro_repeat = 1;
                self.macro_events.clear();
            }
            Some(Err(e)) => {
                self.macro_repeat = 1;
                self.macro_events.clear();
                self.macro_error = Some(first_line(&e));
                self.log(Entry::error(
                    format!("Cannot decode macro {}", slot + 1),
                    &e,
                ));
            }
            None => {}
        }
        cx.notify();
    }

    /// Write the edited events into the draft. An empty list clears the slot.
    pub fn store_macro(&mut self, cx: &mut Context<Self>) {
        let slot = usize::from(self.macro_slot);
        let image = if self.macro_events.is_empty() {
            Ok(vec![0; 256])
        } else {
            Macro {
                repeat: self.macro_repeat,
                events: self.macro_events.clone(),
            }
            .encode()
        };
        match image {
            Ok(image) => {
                self.macro_error = None;
                self.edit_onboard(cx, |o| o.macros[slot] = image);
            }
            Err(e) => {
                self.macro_error = Some(first_line(&e));
                self.log(Entry::error(
                    format!("Cannot encode macro {}", slot + 1),
                    &e,
                ));
            }
        }
        cx.notify();
    }

    pub fn toggle_recording(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(rec) = self.recording.take() {
            for code in rec.held.into_iter().rev() {
                self.push_event(code, false, rec.last);
            }
            self.store_macro(cx);
        } else if self.editable() {
            self.macro_events.clear();
            self.recording = Some(Recording {
                last: Instant::now(),
                held: Vec::new(),
                modifiers: Vec::new(),
            });
            window.focus(&self.record_focus, cx);
        }
        cx.notify();
    }

    fn push_event(&mut self, code: u8, down: bool, last: Instant) {
        let now = Instant::now();
        if let Some(prev) = self.macro_events.last_mut() {
            let ms = now
                .duration_since(last)
                .as_millis()
                .min(u128::from(u16::MAX)) as u16;
            match prev {
                MacroEvent::Keyboard { delay_ms, .. }
                | MacroEvent::MouseButton { delay_ms, .. }
                | MacroEvent::Move { delay_ms, .. } => *delay_ms = ms.max(1),
            }
        }
        self.macro_events.push(if (0xf0..=0xf4).contains(&code) {
            MacroEvent::MouseButton {
                button: code,
                down,
                delay_ms: 10,
            }
        } else {
            MacroEvent::Keyboard {
                usage: code,
                down,
                delay_ms: 10,
            }
        });
    }

    pub fn record(&mut self, code: u8, down: bool, cx: &mut Context<Self>) {
        let Some(rec) = &mut self.recording else {
            return;
        };
        let held = rec.held.contains(&code);
        if held == down {
            return;
        }
        if down {
            rec.held.push(code);
        } else {
            rec.held.retain(|c| *c != code);
        }
        let last = rec.last;
        rec.last = Instant::now();
        self.push_event(code, down, last);
        cx.notify();
    }

    // ---- View sync -------------------------------------------------------

    /// Set every stateful control from the draft after it is replaced.
    pub fn sync_controls(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(d) = self.draft.clone() else { return };
        if d.lighting.mode != 13 && d.lighting.rgb != [250, 255, 250] {
            self.last_rgb = d.lighting.rgb;
        }
        let light = Light::decode(&d.lighting, self.last_rgb);
        let effect = EFFECTS
            .iter()
            .find(|(m, _)| *m == light.mode)
            .map(|(_, n)| SharedString::from(*n));
        self.effect.update(cx, |s, cx| match &effect {
            Some(name) => s.set_selected_value(name, window, cx),
            None => s.set_selected_index(None, window, cx),
        });
        self.color
            .update(cx, |s, cx| s.set_value(hsla(light.rgb), window, cx));
        let set = |e: &Entity<SliderState>, v: f32, window: &mut Window, cx: &mut Context<Self>| {
            e.update(cx, |s, cx| s.set_value(v, window, cx));
        };
        set(&self.brightness, f32::from(light.brightness), window, cx);
        set(&self.speed, f32::from(light.speed), window, cx);
        if let Some(o) = &d.onboard {
            set(&self.debounce, f32::from(o.debounce), window, cx);
            let t = &o.sleep;
            for (e, v) in self.sleep.clone().iter().zip([
                t.time_bt,
                t.time_24,
                t.deep_time_bt,
                t.deep_time_24,
            ]) {
                set(e, f32::from(v / 60), window, cx);
            }
        }
        self.sync_target(window, cx);
        let slot = self.macro_slot;
        self.open_macro(slot, cx);
    }

    pub fn set_theme(&mut self, dark: bool, window: &mut Window, cx: &mut Context<Self>) {
        Theme::change(
            if dark {
                ThemeMode::Dark
            } else {
                ThemeMode::Light
            },
            Some(window),
            cx,
        );
        let saved = settings::Settings {
            theme: Some(if dark {
                ThemeChoice::Dark
            } else {
                ThemeChoice::Light
            }),
        };
        if let Err(e) = settings::save(&saved) {
            self.log(Entry::error("Cannot save theme", &e));
        }
        cx.notify();
    }

    // ---- Logs ------------------------------------------------------------

    pub fn log(&mut self, entry: Entry) {
        log::append(&entry);
        self.logs.push(entry);
        if self.logs.len() > 1000 {
            self.logs.remove(0);
        }
    }

    pub fn copy_logs(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let text: Vec<String> = self
            .logs
            .iter()
            .filter(|e| !self.errors_only || e.level == Level::Error)
            .map(Entry::text)
            .collect();
        cx.write_to_clipboard(gpui_kit::ClipboardItem::new_string(text.join("\n")));
        window.push_notification(Notification::success("Copied"), cx);
    }

    pub fn clear_logs(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match log::clear() {
            Ok(()) => self.logs.clear(),
            Err(e) => window.push_notification(Notification::error(first_line(&e)), cx),
        }
        cx.notify();
    }
}

/// One change in a short readable form for the log.
fn describe(change: &Change) -> String {
    let text = serde_json::to_string(change).unwrap_or_else(|_| format!("{change:?}"));
    if text.len() > 240 {
        let mut end = 240;
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        format!("{}…", &text[..end])
    } else {
        text
    }
}

fn plural(n: usize, one: &str, many: &str) -> String {
    if n == 1 {
        one.into()
    } else {
        many.replace("{}", &n.to_string())
    }
}

pub fn first_line(e: &impl std::fmt::Display) -> SharedString {
    let text = e.to_string();
    let line = text.lines().next().unwrap_or_default();
    let mut chars = line.chars();
    let first: String = chars
        .next()
        .map(|c| c.to_uppercase().collect())
        .unwrap_or_default();
    format!("{first}{}", chars.as_str()).into()
}

impl Render for Workbench {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let surface = ui::Surface::of(cx);
        let pending = self.pending();
        let count = pending.as_ref().map(Vec::len).unwrap_or(0);
        let applying = matches!(self.busy, Some(Busy::Applying | Busy::Flashing));
        let reading = self.busy == Some(Busy::Reading);
        let dark = theme.is_dark();

        let status = match (&self.link, &self.baseline) {
            (_, Some(s)) => format!(
                "Firmware {:X}.{:02X}",
                s.usb_revision >> 8,
                s.usb_revision & 0xff
            ),
            (Link::Failed(_), None) => "Disconnected".into(),
            _ if reading && self.progress.kind().is_some() => "Reading".into(),
            _ => "Connecting".into(),
        };
        let presence = self.presence.unwrap_or(device::Presence {
            usb: false,
            wireless: None,
        });
        let kind = reading
            .then(|| self.progress.kind())
            .flatten()
            .or(presence.usb.then_some(Kind::Usb))
            .or(presence.wireless.map(|(k, _)| k))
            .or(self.transport.filter(|_| self.baseline.is_some()))
            .or(self.progress.kind());
        let battery = presence.wireless.map(|(_, level)| {
            use gpui_kit::assets::IconName as L;
            let (icon, text, color) = match level {
                Some(b) => (
                    match b {
                        0..=15 => L::BatteryWarning,
                        16..=40 => L::BatteryLow,
                        41..=80 => L::BatteryMedium,
                        _ => L::BatteryFull,
                    },
                    format!("{b}%"),
                    if b <= 15 {
                        theme.danger
                    } else {
                        theme.muted_foreground
                    },
                ),
                None => (L::Battery, "Asleep".to_string(), theme.muted_foreground),
            };
            h_flex()
                .gap_1()
                .text_color(color)
                .child(Icon::new(icon).small())
                .child(text)
        });

        let sidebar = Sidebar::new("nav")
            .w(px(232.))
            .border_0()
            .bg(surface.canvas)
            .header(div().h(px(4.)))
            .child(
                SidebarMenu::new().children(Page::ALL.into_iter().map(|page| {
                    SidebarMenuItem::new(page.title())
                        .icon(page.icon())
                        .active(self.page == page)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.page = page;
                            cx.notify();
                        }))
                })),
            )
            .footer(
                SidebarFooter::new().child(
                    v_flex()
                        .w_full()
                        .gap_3()
                        .child(
                            h_flex()
                                .gap_2()
                                .child(
                                    Button::new("reload")
                                        .outline()
                                        .small()
                                        .icon(IconName::RefreshCw)
                                        .tooltip("Reload")
                                        .loading(reading)
                                        .disabled(self.busy.is_some() || count > 0)
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.reload(window, cx)
                                        })),
                                )
                                .child(
                                    Button::new("theme")
                                        .outline()
                                        .small()
                                        .icon(if dark { IconName::Sun } else { IconName::Moon })
                                        .tooltip(if dark { "Light theme" } else { "Dark theme" })
                                        .on_click(cx.listener(move |this, _, window, cx| {
                                            this.set_theme(!dark, window, cx)
                                        })),
                                ),
                        )
                        .child(
                            v_flex()
                                .gap_0p5()
                                .text_xs()
                                .text_color(theme.muted_foreground)
                                .child("MonsGeek M1W")
                                .child(status)
                                .when(kind.is_some() || battery.is_some(), |d| {
                                    d.child(
                                        h_flex()
                                            .gap_2()
                                            .when_some(kind, |d, k| d.child(k.to_string()))
                                            .children(battery),
                                    )
                                }),
                        ),
                ),
            );

        let actions = h_flex()
            .gap_2()
            .when_some(pending.as_ref().err(), |this, e| {
                this.child(
                    div()
                        .text_sm()
                        .text_color(theme.danger)
                        .max_w(px(420.))
                        .truncate()
                        .child(first_line(e)),
                )
            })
            .when(
                count == 0 && !self.history.is_empty() && self.draft.is_some(),
                |this| {
                    this.child(
                        Button::new("undo")
                            .ghost()
                            .small()
                            .icon(IconName::Undo2)
                            .label("Undo")
                            .disabled(self.busy.is_some())
                            .on_click(cx.listener(|this, _, window, cx| this.undo(window, cx))),
                    )
                },
            )
            .when(count > 0 || pending.is_err(), |this| {
                this.child(
                    Button::new("discard")
                        .outline()
                        .small()
                        .label("Discard")
                        .disabled(applying)
                        .on_click(cx.listener(|this, _, window, cx| this.discard(window, cx))),
                )
            })
            .when(self.draft.is_some(), |this| {
                this.child(
                    Button::new("apply")
                        .primary()
                        .small()
                        .label(match count {
                            0 => "Apply".to_string(),
                            1 => "Apply 1 change".into(),
                            n => format!("Apply {n} changes"),
                        })
                        .loading(applying)
                        .disabled(count == 0 || self.busy.is_some())
                        .on_click(cx.listener(|this, _, window, cx| this.apply(window, cx))),
                )
            });

        let body = crate::pages::render(self, window, cx);
        let client_decorations = matches!(window.window_decorations(), Decorations::Client { .. });

        v_flex()
            .size_full()
            .bg(surface.canvas)
            .text_color(theme.foreground)
            .when(client_decorations, |d| d.child(TitleBar::new()))
            .child(
                h_flex()
                    .flex_1()
                    .min_h_0()
                    .items_start()
                    .child(div().h_full().child(sidebar))
                    .child(
                        div().flex_1().min_w_0().h_full().py_2().pr_2().child(
                            v_flex()
                                .size_full()
                                .rounded(theme.radius_lg * 1.5)
                                .border_1()
                                .border_color(theme.border)
                                .bg(surface.panel)
                                .overflow_hidden()
                                .child(ui::title(self.page.title(), actions))
                                .when_some(kind.and_then(Kind::warning), |d, warning| {
                                    d.child(ui::notice(warning, cx))
                                })
                                .child(div().flex_1().min_h_0().child(body)),
                        ),
                    ),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_kit::TestAppContext;
    use monsgeek_linux::config::SleepTimers;

    fn fixture() -> Snapshot {
        let map = |fn_layer| -> Vec<u8> {
            (0..128)
                .flat_map(|slot| keys::default_binding(fn_layer, slot))
                .collect()
        };
        let mut macros = vec![vec![0u8; 256]; 50];
        macros[2] = Macro {
            repeat: 2,
            events: vec![
                MacroEvent::Keyboard {
                    usage: 4,
                    down: true,
                    delay_ms: 20,
                },
                MacroEvent::Keyboard {
                    usage: 4,
                    down: false,
                    delay_ms: 20,
                },
            ],
        }
        .encode()
        .unwrap();
        let s = Snapshot {
            format_version: 2,
            device_id: monsgeek_linux::protocol::DEVICE_ID,
            usb_revision: 0x0108,
            active_layer: 0,
            lighting: Lighting::new(13, 2, 4, 0, false, [255; 3]).unwrap(),
            keymaps: vec![map(false); 4],
            fn_keymaps: vec![map(true); 4],
            onboard: Some(Onboard {
                debounce: 2,
                sleep: SleepTimers {
                    time_bt: 120,
                    time_24: 120,
                    deep_time_bt: 900,
                    deep_time_24: 600,
                },
                options: [0x10, 0x04, 0],
                auto_os: false,
                rgb: vec![0; 384],
                macros,
            }),
        };
        s.validate().unwrap();
        s
    }

    fn open(cx: &mut TestAppContext) -> (Entity<Workbench>, &mut gpui_kit::VisualTestContext) {
        cx.update(gpui_kit::init);
        cx.add_window_view(|window, cx| {
            let mut w = Workbench::build(window, cx);
            w.loaded(fixture(), window, cx);
            w
        })
    }

    #[gpui::test]
    fn every_page_renders(cx: &mut TestAppContext) {
        let (view, cx) = open(cx);
        for page in Page::ALL {
            for selected in [None, Some(keys::KNOB_PRESS)] {
                view.update_in(cx, |w, _, cx| {
                    w.page = page;
                    w.selected = selected;
                    cx.notify();
                });
                cx.run_until_parked();
            }
        }
    }

    #[test]
    #[ignore = "requires a headless GPU renderer; writes screenshots to /tmp"]
    fn bluetooth_warning_screenshots() {
        use gpui_kit::{
            component::{Theme, ThemeMode},
            size,
        };
        use std::sync::Arc;
        let platform = gpui_kit::platform::current_platform(true);
        let mut context = gpui::HeadlessAppContext::with_platform(
            platform.text_system(),
            Arc::new(crate::AppAssets),
            gpui_platform::current_headless_renderer,
        );
        context.update(gpui_kit::init);
        context.update(crate::use_geist);
        for (label, width, mode, failed) in [
            ("dark", 1240., ThemeMode::Dark, false),
            ("light", 960., ThemeMode::Light, false),
            ("failed", 960., ThemeMode::Dark, true),
        ] {
            let handle = context
                .open_window(size(px(width), px(820.)), |window, cx| {
                    Theme::change(mode, Some(window), cx);
                    cx.new(|cx| {
                        let mut w = Workbench::build(window, cx);
                        *w.progress.kind.lock().unwrap() = Some(Kind::Bluetooth);
                        if failed {
                            w.link = Link::Failed("Bluetooth disconnected".into());
                        } else {
                            w.transport = Some(Kind::Bluetooth);
                            w.loaded(fixture(), window, cx);
                        }
                        w
                    })
                })
                .unwrap();
            context.run_until_parked();
            context
                .update_window(handle.into(), |_, window, cx| {
                    let _ = window.draw(cx);
                })
                .unwrap();
            context
                .capture_screenshot(handle.into())
                .unwrap()
                .save(format!("/tmp/monsgeek-bluetooth-{label}.png"))
                .unwrap();
        }
    }

    #[gpui::test]
    fn firmware_requires_a_clean_usb_baseline(cx: &mut TestAppContext) {
        let (view, cx) = open(cx);
        view.update_in(cx, |w, _, cx| {
            let bytes = vec![0; firmware::PREFIX_BYTES + 65];
            w.firmware_image = Some(("firmwareFile.bin".into(), Image::parse(&bytes).unwrap()));
            assert!(!w.can_flash_firmware());
            w.transport = Some(Kind::Bluetooth);
            assert!(!w.can_flash_firmware());
            w.transport = Some(Kind::Usb);
            assert!(w.can_flash_firmware());
            w.busy = Some(Busy::Flashing);
            assert!(!w.can_flash_firmware());
            assert!(!w.editable());
            w.busy = None;
            w.edit_light(cx, |light| light.brightness = 1);
            assert!(!w.can_flash_firmware());
        });
    }

    #[test]
    #[ignore = "requires a headless GPU renderer; writes screenshots to /tmp"]
    fn firmware_screenshots() {
        use gpui_kit::size;
        use std::{cell::RefCell, rc::Rc};
        let platform = gpui_kit::platform::current_platform(true);
        let mut context = gpui::HeadlessAppContext::with_platform(
            platform.text_system(),
            Arc::new(crate::AppAssets),
            gpui_platform::current_headless_renderer,
        );
        context.update(gpui_kit::init);
        context.update(crate::use_geist);
        for (label, width, height, mode) in [
            ("dark", 1240., 1100., ThemeMode::Dark),
            ("light", 960., 1100., ThemeMode::Light),
            ("confirm", 960., 820., ThemeMode::Dark),
            ("progress", 960., 1100., ThemeMode::Light),
        ] {
            let state = Rc::new(RefCell::new(None));
            let handle = context
                .open_window(size(px(width), px(height)), |window, cx| {
                    Theme::change(mode, Some(window), cx);
                    let view = cx.new(|cx| {
                        let mut w = Workbench::build(window, cx);
                        w.loaded(fixture(), window, cx);
                        w.transport = Some(Kind::Usb);
                        w.page = Page::System;
                        let image = Image::parse(&vec![0; firmware::PREFIX_BYTES + 65]).unwrap();
                        w.firmware_image = Some(("firmwareFile.bin".into(), image));
                        if label == "progress" {
                            w.busy = Some(Busy::Flashing);
                            *w.firmware_status.lock().unwrap() = "Writing 120/240 blocks".into();
                        }
                        w
                    });
                    *state.borrow_mut() = Some(view.clone());
                    cx.new(|cx| gpui_kit::base::Root::new(view, window, cx))
                })
                .unwrap();
            context.run_until_parked();
            context.advance_clock(std::time::Duration::from_millis(500));
            context.run_until_parked();
            context
                .update_window(handle.into(), |_, window, cx| {
                    if label == "confirm" {
                        state
                            .borrow()
                            .as_ref()
                            .unwrap()
                            .update(cx, |w, cx| w.confirm_firmware(window, cx));
                    }
                    let _ = window.draw(cx);
                })
                .unwrap();
            context.run_until_parked();
            context.advance_clock(std::time::Duration::from_millis(500));
            context.run_until_parked();
            context
                .update_window(handle.into(), |_, window, cx| {
                    let _ = window.draw(cx);
                })
                .unwrap();
            context
                .capture_screenshot(handle.into())
                .unwrap()
                .save(format!("/tmp/monsgeek-firmware-{label}.png"))
                .unwrap();
        }
    }

    #[gpui::test]
    fn logs_render_while_disconnected(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let (view, cx) = cx.add_window_view(|window, cx| {
            let mut w = Workbench::build(window, cx);
            w.link = Link::Failed("No keyboard".into());
            w.page = Page::Logs;
            w.log(Entry::error("Read failed", &anyhow::anyhow!("No keyboard")));
            w
        });
        cx.run_until_parked();
        view.update_in(cx, |w, _, cx| {
            w.errors_only = true;
            cx.notify();
        });
        cx.run_until_parked();
    }

    #[gpui::test]
    fn edits_are_pending_until_discarded(cx: &mut TestAppContext) {
        let (view, cx) = open(cx);
        view.update_in(cx, |w, window, cx| {
            assert!(w.pending().unwrap().is_empty());
            // Remap Caps Lock to Left Ctrl, then add Shift.
            w.select_key(Some(3), window, cx);
            w.set_binding(3, [0, 0, 224, 0], cx);
            w.paint(4, cx);
            w.edit_onboard(cx, |o| o.options[0] |= 1);
            w.edit_light(cx, |l| l.brightness = 1);
            let changes = w.pending().unwrap();
            assert_eq!(changes.len(), 4, "{changes:?}");
            w.discard(window, cx);
            assert!(w.pending().unwrap().is_empty());
        });
    }

    #[gpui::test]
    fn picture_change_blocks_painting(cx: &mut TestAppContext) {
        let (view, cx) = open(cx);
        view.update_in(cx, |w, _, cx| {
            assert!(w.can_paint());
            w.edit_light(cx, |l| l.option = 1);
            assert!(!w.can_paint());
            w.paint(4, cx);
            assert_eq!(w.pending().unwrap().len(), 1);
        });
    }

    #[gpui::test]
    fn macro_edits_round_trip(cx: &mut TestAppContext) {
        let (view, cx) = open(cx);
        view.update_in(cx, |w, _, cx| {
            w.open_macro(2, cx);
            assert_eq!(w.macro_repeat, 2);
            assert_eq!(w.macro_events.len(), 2);
            // Removing one half of a key press is refused, not written.
            w.macro_events.remove(1);
            w.store_macro(cx);
            assert!(w.macro_error.is_some());
            assert_eq!(w.logs.last().map(|e| e.level), Some(Level::Error));
            assert!(w.pending().unwrap().is_empty());
            w.macro_events.clear();
            w.store_macro(cx);
            assert_eq!(w.pending().unwrap().len(), 1);
        });
    }
}
