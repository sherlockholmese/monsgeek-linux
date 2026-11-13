//! Profiles, change review, local input testing and optional desktop services.
use crate::{
    app::{Busy, Page, Workbench, first_line},
    board::{Look, board},
    desktop, device,
    log::{Entry, Level},
    settings, ui,
};
use gpui_kit::component::{
    ActiveTheme, Disableable, Sizable, WindowExt,
    button::{Button, ButtonVariants},
    checkbox::Checkbox,
    h_flex,
    input::{Input, InputEvent, InputState, Textarea, TextareaState},
    notification::Notification,
    slider::{Slider, SliderEvent, SliderState, SliderValue},
    switch::Switch,
    v_flex,
};
use gpui_kit::{
    AnyElement, AppContext, Context, Entity, FocusHandle, InteractiveElement, IntoElement,
    KeyDownEvent, KeyUpEvent, ModifiersChangedEvent, ParentElement, Styled, Subscription, Window,
    div, prelude::FluentBuilder, px,
};
use monsgeek_linux::{
    device::Snapshot,
    diagnostics, host,
    profiles::{self, Profile, Selection},
    review,
};
use std::{
    collections::{BTreeSet, VecDeque},
    path::PathBuf,
    time::Duration,
};
// Key names remain owned by the core registry.
use monsgeek_linux::keys;

pub struct Features {
    pub calibrating: bool,
    pub live_travel: bool,
    pub calibration_status: String,
    pub travel: Vec<f32>,
    pub profiles: Vec<(PathBuf, Profile)>,
    pub profile_error: Option<String>,
    pub selection: Selection,
    pub name: Entity<InputState>,
    source: Option<Snapshot>,
    pub preferences: settings::Settings,
    pub autostart: bool,
    pub tray: Option<desktop::Bridge>,
    pub desktop_error: Option<String>,
    pub battery_latched: bool,
    pub testing: bool,
    pub tester_focus: FocusHandle,
    pub held: BTreeSet<u8>,
    pub seen: BTreeSet<u8>,
    /// Newest first: what was received, and whether it went down or up.
    pub events: VecDeque<(String, Option<&'static str>)>,
    pub host_editor: Entity<TextareaState>,
    pub host_error: Option<String>,
    pub host_process: Option<desktop::HostProcess>,
    pub host_config: Option<host::Config>,
    pub host_devices: Result<Vec<(String, PathBuf)>, String>,
    pub picture_backup: Option<monsgeek_linux::pictures::Backup>,
    pub picture_error: Option<String>,
    pub battery: Entity<SliderState>,
    _blur: Subscription,
    _host_edits: Subscription,
    _battery: Subscription,
}

fn input_devices() -> Result<Vec<(String, PathBuf)>, String> {
    let mut devices: Vec<_> = host::linux::devices()
        .map_err(|e| e.to_string())?
        .into_iter()
        .map(|d| (d.name, d.path))
        .collect();
    devices.sort();
    Ok(devices)
}
impl Features {
    pub fn new(window: &mut Window, cx: &mut Context<Workbench>) -> Self {
        let profiles = profiles::directory().and_then(|dir| profiles::list(&dir));
        let error = profiles.as_ref().err().map(ToString::to_string);
        let tester_focus = cx.focus_handle();
        let blur = cx.on_blur(&tester_focus, window, |this, _, cx| {
            this.features.testing = false;
            this.features.held.clear();
            cx.notify();
        });
        let host_editor = cx.new(|cx| {
            TextareaState::new(window, cx).default_value(
                serde_json::to_string_pretty(&host::Config::example()).unwrap_or_default(),
            )
        });
        let host_edits = cx.subscribe(&host_editor, |this, _, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                this.features.host_config = None;
                this.features.host_error = None;
                cx.notify();
            }
        });
        let preferences = settings::load();
        let battery = cx.new(|_| {
            SliderState::new()
                .min(5.)
                .max(50.)
                .step(5.)
                .default_value(f32::from(preferences.battery_threshold))
        });
        let battery_changes = cx.subscribe(&battery, |this, _, event: &SliderEvent, cx| {
            let SliderEvent::Change(value) = event else {
                return;
            };
            let value = match value {
                SliderValue::Single(v) | SliderValue::Range(_, v) => *v,
            } as u8;
            if value != this.features.preferences.battery_threshold {
                this.features.preferences.battery_threshold = value;
                this.features.battery_latched = false;
                this.save_preferences(cx);
            }
        });
        Self {
            calibrating: false,
            live_travel: false,
            calibration_status: String::new(),
            travel: Vec::new(),
            profiles: profiles.unwrap_or_default(),
            profile_error: error,
            selection: Selection::default(),
            name: cx.new(|cx| InputState::new(window, cx).placeholder("For example, Coding")),
            source: None,
            preferences,
            autostart: desktop::autostart_enabled(),
            tray: None,
            desktop_error: None,
            battery_latched: false,
            testing: false,
            tester_focus,
            held: BTreeSet::new(),
            seen: BTreeSet::new(),
            events: VecDeque::new(),
            host_editor,
            host_error: None,
            host_process: None,
            host_config: None,
            host_devices: input_devices(),
            picture_backup: None,
            picture_error: None,
            battery,
            _blur: blur,
            _host_edits: host_edits,
            _battery: battery_changes,
        }
    }
    pub fn test_event(&mut self, usage: Option<u8>, label: String, down: bool) {
        if !self.testing {
            return;
        }
        if let Some(usage) = usage {
            if down {
                self.held.insert(usage);
                self.seen.insert(usage);
            } else {
                self.held.remove(&usage);
            }
        }
        self.log_event(label, Some(if down { "Down" } else { "Up" }));
    }
    pub fn log_event(&mut self, label: String, kind: Option<&'static str>) {
        self.events.push_front((label, kind));
        self.events.truncate(100);
    }
}

impl Workbench {
    pub fn refresh_profiles(&mut self) {
        match profiles::directory().and_then(|dir| profiles::list(&dir)) {
            Ok(profiles) => {
                self.features.profiles = profiles;
                self.features.profile_error = None;
            }
            Err(error) => self.features.profile_error = Some(error.to_string()),
        }
        if let Some(tray) = &self.features.tray {
            let profiles = self
                .features
                .profiles
                .iter()
                .map(|(p, v)| (p.clone(), v.name.clone()))
                .collect();
            tray.handle.update(move |tray| tray.profiles = profiles);
        }
    }
    pub fn name_profile(
        &mut self,
        snapshot: Snapshot,
        suggested: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.features.source = Some(snapshot);
        self.features
            .name
            .update(cx, |state, cx| state.set_value(suggested, window, cx));
        self.features.profile_error = None;
        let view = cx.entity();
        window.open_dialog(cx, move |dialog, _, cx| {
            let state = view.read(cx);
            let save = view.clone();
            dialog
                .title("Save profile")
                .w(px(480.))
                .child(
                    v_flex()
                        .gap_2()
                        .child(div().text_sm().child("Name"))
                        .child(Input::new(&state.features.name))
                        .when_some(state.features.profile_error.clone(), |d, e| {
                            d.child(div().text_sm().text_color(cx.theme().danger).child(e))
                        }),
                )
                .footer(
                    h_flex()
                        .w_full()
                        .justify_end()
                        .gap_2()
                        .child(
                            Button::new("profile-cancel")
                                .ghost()
                                .label("Cancel")
                                .on_click(|_, window, cx| window.close_dialog(cx)),
                        )
                        .child(
                            Button::new("profile-save")
                                .primary()
                                .label("Save profile")
                                .on_click(move |_, window, cx| {
                                    save.update(cx, |this, cx| {
                                        let result = (|| -> anyhow::Result<()> {
                                            let name =
                                                this.features.name.read(cx).value().to_string();
                                            let snapshot =
                                                this.features.source.clone().ok_or_else(|| {
                                                    anyhow::anyhow!("No configuration to save")
                                                })?;
                                            profiles::save(
                                                &profiles::directory()?,
                                                &Profile::new(name, snapshot)?,
                                            )?;
                                            Ok(())
                                        })();
                                        match result {
                                            Ok(()) => {
                                                this.refresh_profiles();
                                                this.features.source = None;
                                                window.close_dialog(cx);
                                                window.push_notification(
                                                    Notification::success("Profile saved"),
                                                    cx,
                                                );
                                            }
                                            Err(e) => {
                                                this.features.profile_error = Some(e.to_string());
                                                cx.notify();
                                            }
                                        }
                                    });
                                }),
                        ),
                )
        });
    }
    pub fn load_profile(&mut self, profile: Profile, window: &mut Window, cx: &mut Context<Self>) {
        let Some(current) = self.draft.as_ref() else {
            self.page = Page::Profiles;
            self.features.profile_error = Some("Connect and read the keyboard first".into());
            cx.notify();
            return;
        };
        match profile.draft(current, self.features.selection) {
            Ok(draft) => {
                self.draft = Some(draft);
                self.features.profile_error = None;
                self.sync_controls(window, cx);
                self.page = Page::Review;
            }
            Err(error) => {
                self.page = Page::Profiles;
                self.features.profile_error = Some(error.to_string());
            }
        }
        cx.notify();
    }
    pub fn start_desktop(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.features.preferences.tray {
            self.enable_tray(window, cx);
        }
        cx.spawn_in(window, async move |this,cx| {
            loop {
                cx.background_executor().timer(Duration::from_millis(100)).await;
                if this.update_in(cx, |this,window,cx| {
                    let events: Vec<_> = this.features.tray.as_ref().map(|b| b.events.try_iter().collect()).unwrap_or_default();
                    for event in events {
                        match event {
                            desktop::Event::Open => { window.activate_window(); cx.activate(true); }
                            desktop::Event::Profile(path) => {
                                window.activate_window();
                                if this.busy.is_none() && let Ok(profile) = profiles::load(&path) { this.load_profile(profile,window,cx); }
                            }
                            desktop::Event::StopHost => { this.features.host_process = None; cx.notify(); }
                            desktop::Event::Quit => { this.features.host_process = None; cx.quit(); }
                        }
                    }
                    if let Some(process) = &mut this.features.host_process {
                        match process.0.try_wait() {
                            Ok(Some(status)) => {
                                if status.success() { this.features.host_error=None; } else { let error=process.1.lock().unwrap_or_else(|e|e.into_inner()).clone(); this.features.host_error = Some(if error.trim().is_empty(){format!("Host remapping stopped ({status}). Check the configuration and input permissions.")}else{error.trim().to_owned()}); }
                                this.features.host_process = None;
                                cx.notify();
                            }
                            Err(e) => { this.features.host_error = Some(e.to_string()); this.features.host_process = None; cx.notify(); }
                            _ => {}
                        }
                    }
                }).is_err() { break; }
            }
        }).detach();
    }
    pub fn enable_tray(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let profiles = self
            .features
            .profiles
            .iter()
            .map(|(p, v)| (p.clone(), v.name.clone()))
            .collect();
        let task = cx.background_spawn(async move { desktop::tray(profiles) });
        cx.spawn_in(window, async move |this, cx| {
            let result = task.await;
            this.update_in(cx, |this, _, cx| {
                match result {
                    Ok(bridge) if this.features.preferences.tray => {
                        this.features.tray = Some(bridge)
                    }
                    Ok(_) => {}
                    Err(e) => this.features.desktop_error = Some(format!("Tray unavailable: {e}")),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }
    pub fn save_preferences(&mut self, cx: &mut Context<Self>) {
        if let Err(e) = settings::save(&self.features.preferences) {
            self.features.desktop_error = Some(e.to_string());
        }
        cx.notify();
    }
    pub fn check_battery(&mut self, presence: device::Presence, cx: &mut Context<Self>) {
        let threshold = self.features.preferences.battery_threshold.clamp(5, 50);
        if let Some((_, Some(battery))) = presence.wireless {
            if battery > threshold + 5 {
                self.features.battery_latched = false;
            }
            if self.features.preferences.battery_alerts
                && !presence.usb
                && battery <= threshold
                && !self.features.battery_latched
            {
                self.features.battery_latched = true;
                cx.background_spawn(async move {
                    if let Err(error) = desktop::notify(battery) {
                        crate::log::append(&Entry::error(
                            "Battery notification unavailable",
                            &error,
                        ));
                    }
                })
                .detach();
            }
        }
        if let Some(tray) = &self.features.tray {
            let status = match presence.wireless {
                Some((kind, Some(battery))) => format!("{kind}, {battery}%"),
                Some((_, None)) if !presence.usb => "Asleep".into(),
                _ if presence.usb => "USB".into(),
                _ => "Disconnected".into(),
            };
            tray.handle
                .update(move |tray| tray.title = format!("MonsGeek ({status})"));
        }
    }
    pub fn export_diagnostics(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let report = diagnostics::report(
            self.baseline.as_ref(),
            self.transport,
            self.pending().map(|v| v.len()).unwrap_or(0),
            self.logs
                .iter()
                .rev()
                .filter(|e| e.level == Level::Error)
                .map(|e| e.message.clone())
                .collect(),
        );
        let dir = device::state_dir().unwrap_or_else(|_| PathBuf::from("."));
        let prompt = cx.prompt_for_new_path(&dir, Some("m1w-diagnostics.json"));
        cx.spawn_in(window, async move |this, cx| {
            if let Ok(Ok(Some(path))) = prompt.await {
                let result = monsgeek_linux::transaction::save_new(&path, &report);
                this.update_in(cx, |_, window, cx| {
                    match result {
                        Ok(()) => {
                            window.push_notification(
                                Notification::success("Diagnostics exported"),
                                cx,
                            );
                        }
                        Err(e) => window.push_notification(Notification::error(first_line(&e)), cx),
                    }
                    cx.notify();
                })
                .ok();
            }
        })
        .detach();
    }
    pub fn capture_pictures(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.previewing
            || self.features.calibrating
            || self.busy.is_some()
            || !self.pending().is_ok_and(|v| v.is_empty())
            || self.baseline.is_none()
        {
            return;
        }
        let view = cx.entity();
        window.open_dialog(cx, move |dialog, _, _| {
            let capture = view.clone();
            dialog
                .title("Back up all pictures")
                .w(px(480.))
                .child(div().text_sm().child(
                    "Each picture is selected briefly, then the current lighting is restored.",
                ))
                .footer(
                    h_flex()
                        .w_full()
                        .justify_end()
                        .gap_2()
                        .child(
                            Button::new("capture-cancel")
                                .ghost()
                                .label("Cancel")
                                .on_click(|_, window, cx| window.close_dialog(cx)),
                        )
                        .child(
                            Button::new("capture-pictures")
                                .primary()
                                .label("Back up all")
                                .on_click(move |_, window, cx| {
                                    window.close_dialog(cx);
                                    capture.update(cx, |this, cx| {
                                        this.choose_picture_path(window, cx)
                                    });
                                }),
                        ),
                )
        });
    }
    fn choose_picture_path(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let dir = device::state_dir().unwrap_or_else(|_| PathBuf::from("."));
        let prompt = cx.prompt_for_new_path(&dir, Some("m1w-all-pictures.json"));
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(path))) = prompt.await else {
                return;
            };
            let ready = this
                .update_in(cx, |this, _, cx| {
                    if this.busy.is_some() || !this.pending().is_ok_and(|v| v.is_empty()) {
                        return false;
                    }
                    this.busy = Some(Busy::Capturing);
                    this.features.picture_error = None;
                    cx.notify();
                    true
                })
                .unwrap_or(false);
            if !ready {
                return;
            }
            let result = cx
                .background_spawn(async move {
                    let api = hidapi::HidApi::new()?;
                    let keyboard = monsgeek_linux::device::Keyboard::open(&api, None)?;
                    let stamp = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)?
                        .as_nanos();
                    let journal = device::state_dir()?.join(format!("picture-before-{stamp}.json"));
                    monsgeek_linux::pictures::capture(&keyboard, &path, &journal)
                })
                .await;
            this.update_in(cx, |this, window, cx| {
                this.busy = None;
                match result {
                    Ok(backup) => {
                        this.features.picture_backup = Some(backup);
                        window.push_notification(Notification::success("Pictures backed up"), cx);
                    }
                    Err(e) => {
                        this.features.picture_error =
                            Some(format!("Backup failed. {}", first_line(&e)));
                        this.log(Entry::error("Picture backup failed", &e));
                    }
                }
                this.reload(window, cx);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }
    pub fn open_picture_backup(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let prompt = cx.prompt_for_paths(gpui_kit::PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Open picture backup".into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            if let Ok(Ok(Some(paths))) = prompt.await
                && let Some(path) = paths.first()
            {
                let result = (|| -> anyhow::Result<_> {
                    let backup: monsgeek_linux::pictures::Backup =
                        serde_json::from_slice(&std::fs::read(path)?)?;
                    backup.validate()?;
                    Ok(backup)
                })();
                this.update_in(cx, |this, _, cx| {
                    match result {
                        Ok(backup) => {
                            this.features.picture_backup = Some(backup);
                            this.features.picture_error = None;
                        }
                        Err(e) => this.features.picture_error = Some(first_line(&e).to_string()),
                    }
                    cx.notify();
                })
                .ok();
            }
        })
        .detach();
    }
    pub fn restore_picture(&mut self, picture: u8, window: &mut Window, cx: &mut Context<Self>) {
        if !self.editable() {
            return;
        }
        if let (Some(backup), Some(current)) = (&self.features.picture_backup, &self.draft) {
            match backup.picture_draft(current, picture) {
                Ok(draft) => {
                    self.draft = Some(draft);
                    self.sync_controls(window, cx);
                    self.page = Page::Review;
                }
                Err(e) => self.features.picture_error = Some(first_line(&e).to_string()),
            }
        }
        cx.notify();
    }
    pub fn validate_host(&mut self, cx: &mut Context<Self>) -> Option<host::Config> {
        let result = (|| -> anyhow::Result<_> {
            let config: host::Config =
                serde_json::from_str(&self.features.host_editor.read(cx).value())?;
            config.validate()?;
            Ok(config)
        })();
        match result {
            Ok(config) => {
                self.features.host_error = None;
                self.features.host_config = Some(config.clone());
                cx.notify();
                Some(config)
            }
            Err(e) => {
                self.features.host_error = Some(e.to_string());
                cx.notify();
                None
            }
        }
    }
    pub fn host_file(&mut self, open: bool, window: &mut Window, cx: &mut Context<Self>) {
        if open {
            let prompt = cx.prompt_for_paths(gpui_kit::PathPromptOptions {
                files: true,
                directories: false,
                multiple: false,
                prompt: Some("Open host configuration".into()),
            });
            cx.spawn_in(window, async move |this, cx| {
                if let Ok(Ok(Some(paths))) = prompt.await
                    && let Some(path) = paths.first()
                {
                    let result = (|| -> anyhow::Result<_> {
                        let config: host::Config = serde_json::from_slice(&std::fs::read(path)?)?;
                        config.validate()?;
                        Ok(config)
                    })();
                    this.update_in(cx, |this, window, cx| match result {
                        Ok(config) => {
                            let text = serde_json::to_string_pretty(&config).unwrap_or_default();
                            this.features
                                .host_editor
                                .update(cx, |s, cx| s.set_value(text, window, cx));
                            this.features.host_config = Some(config);
                            this.features.host_error = None;
                            cx.notify();
                        }
                        Err(e) => {
                            this.features.host_error = Some(e.to_string());
                            cx.notify();
                        }
                    })
                    .ok();
                }
            })
            .detach();
        } else if let Some(config) = self.validate_host(cx) {
            let dir = device::state_dir().unwrap_or_else(|_| PathBuf::from("."));
            let prompt = cx.prompt_for_new_path(&dir, Some("m1w-host.json"));
            cx.spawn_in(window, async move |this, cx| {
                if let Ok(Ok(Some(path))) = prompt.await {
                    let result = monsgeek_linux::transaction::save_new(&path, &config);
                    this.update_in(cx, |this, window, cx| match result {
                        Ok(()) => window.push_notification(
                            Notification::success("Host configuration saved"),
                            cx,
                        ),
                        Err(e) => {
                            this.features.host_error = Some(e.to_string());
                            cx.notify();
                        }
                    })
                    .ok();
                }
            })
            .detach();
        }
    }
    pub fn start_host(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.features.host_process.is_some() {
            return;
        }
        let Some(config) = self.validate_host(cx) else {
            return;
        };
        let view = cx.entity();
        window.open_dialog(cx,move |dialog,_,_| {
            let start=view.clone();let config=config.clone();
            dialog.title("Start host remapping").w(px(520.))
                .child(div().text_sm().child("The configured input devices are captured exclusively. Press Ctrl+Alt+Esc to stop."))
                .footer(h_flex().w_full().justify_end().gap_2()
                    .child(Button::new("host-cancel").ghost().label("Cancel").on_click(|_,window,cx|window.close_dialog(cx)))
                    .child(Button::new("host-start-confirm").primary().label("Start remapping").on_click(move |_,window,cx| {
                        let config=config.clone();
                        start.update(cx,|this,cx| {
                            let result=(||->anyhow::Result<_>{
                                let path=device::state_dir()?.join(format!("host-session-{}.json",std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_nanos()));
                                monsgeek_linux::transaction::save_new(&path,&config)?;
                                let sibling=std::env::current_exe()?.with_file_name("monsgeek-linux");
                                let binary=if sibling.is_file(){sibling}else{PathBuf::from("monsgeek-linux")};
                                desktop::spawn_host(&binary,&path)
                            })();
                            match result {Ok(process)=>{this.features.host_process=Some(process);this.features.host_error=None;window.close_dialog(cx);},Err(e)=>this.features.host_error=Some(e.to_string())}
                            cx.notify();
                        });
                    })))
        });
    }
}

pub fn render(
    this: &mut Workbench,
    _window: &mut Window,
    cx: &mut Context<Workbench>,
) -> Option<AnyElement> {
    Some(match this.page {
        Page::Profiles => profiles_page(this, cx),
        Page::Review => review_page(this, cx),
        Page::Tester => tester_page(this, cx),
        Page::Desktop => desktop_page(this, cx),
        Page::Host => host_page(this, cx),
        _ => return None,
    })
}

fn profiles_page(this: &mut Workbench, cx: &mut Context<Workbench>) -> AnyElement {
    let theme = cx.theme().clone();
    let current = this.baseline.as_ref().map(|b| b.usb_revision);
    let save = Button::new("new-profile")
        .outline()
        .small()
        .icon(gpui_kit::assets::IconName::Plus)
        .label("Save profile")
        .disabled(this.draft.is_none() || this.busy.is_some() || this.previewing)
        .on_click(cx.listener(|this, _, window, cx| {
            if let Some(snapshot) = this.draft.clone() {
                this.name_profile(snapshot, String::new(), window, cx)
            }
        }));
    let rows: Vec<AnyElement> = this
        .features
        .profiles
        .clone()
        .into_iter()
        .enumerate()
        .map(|(index, (_, profile))| {
            let load = profile.clone();
            let revision = profile.snapshot.usb_revision;
            let same_model = this
                .baseline
                .as_ref()
                .is_none_or(|b| b.device_id == profile.snapshot.device_id);
            h_flex()
                .min_h(px(52.))
                .py_2()
                .gap_4()
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_sm()
                        .font_weight(gpui_kit::FontWeight::MEDIUM)
                        .child(profile.name.clone()),
                )
                .when(!same_model, |d| {
                    d.child(
                        div().text_sm().text_color(theme.muted_foreground).child(
                            profile
                                .snapshot
                                .model()
                                .map(|m| m.name.clone())
                                .unwrap_or_else(|_| "Different model".into()),
                        ),
                    )
                })
                // Firmware only matters when it differs from the keyboard's.
                .when(current.is_some_and(|c| c != revision), |d| {
                    d.child(
                        div()
                            .flex_shrink_0()
                            .text_sm()
                            .text_color(theme.muted_foreground)
                            .child(crate::app::firmware_label(revision)),
                    )
                })
                .child(
                    h_flex()
                        .flex_shrink_0()
                        .gap_2()
                        .child(
                            Button::new(("duplicate-profile", index))
                                .ghost()
                                .small()
                                .label("Duplicate")
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.name_profile(
                                        profile.snapshot.clone(),
                                        format!("{} copy", profile.name),
                                        window,
                                        cx,
                                    )
                                })),
                        )
                        .child(
                            Button::new(("load-profile", index))
                                .outline()
                                .small()
                                .label("Load")
                                .disabled(
                                    !this.editable() || !same_model || current != Some(revision),
                                )
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.load_profile(load.clone(), window, cx)
                                })),
                        ),
                )
                .into_any_element()
        })
        .collect();
    let s = this.features.selection;
    let parts = h_flex()
        .flex_wrap()
        .py_4()
        .gap_x_6()
        .gap_y_3()
        .children(
            [
                ("Keys", s.keys),
                ("Macros", s.macros),
                ("Lighting", s.lighting),
                ("Performance", s.performance),
                ("Keyboard options", s.options),
                ("Active layer", s.active_layer),
            ]
            .into_iter()
            .enumerate()
            .map(|(index, (label, on))| {
                Checkbox::new(("profile-part", index))
                    .label(label)
                    .checked(on)
                    .on_click(cx.listener(move |this, on: &bool, _, cx| {
                        let s = &mut this.features.selection;
                        *[
                            &mut s.keys,
                            &mut s.macros,
                            &mut s.lighting,
                            &mut s.performance,
                            &mut s.options,
                            &mut s.active_layer,
                        ][index] = *on;
                        cx.notify();
                    }))
            }),
        )
        .into_any_element();
    let mut sections = vec![ui::heading_row("Saved", save, cx)];
    if rows.is_empty() {
        sections.push(ui::placeholder("No saved profiles", 96., cx));
    } else {
        sections.push(ui::section("", rows, cx));
    }
    if let Some(error) = &this.features.profile_error {
        sections.push(ui::error(error.clone(), cx));
    }
    sections.push(ui::section("Load includes", vec![parts], cx));
    ui::page("profiles", sections).into_any_element()
}

fn review_page(this: &mut Workbench, cx: &mut Context<Workbench>) -> AnyElement {
    let theme = cx.theme().clone();
    let rows = match (&this.baseline, &this.draft) {
        (Some(b), Some(d)) => review::rows(b, d),
        _ => Ok(Vec::new()),
    };
    let rows = match rows {
        Ok(rows) => rows,
        Err(e) => {
            return ui::page("review-error", vec![ui::error(first_line(&e), cx)])
                .into_any_element();
        }
    };
    if rows.is_empty() {
        return ui::page(
            "change-review",
            vec![ui::placeholder("No pending changes", 96., cx)],
        )
        .into_any_element();
    }
    let changed: BTreeSet<_> = rows.iter().filter_map(|r| r.slot).collect();
    let mut parts = Vec::new();
    if !changed.is_empty() {
        parts.push(board(
            "review-board",
            this.model(),
            |slot| Look {
                legend: this.model().slot_legend(slot).into(),
                selected: changed.contains(&slot),
                ..Default::default()
            },
            None,
            cx,
        ));
    }
    let cell = || div().flex_1().min_w_0().text_sm();
    let line = || h_flex().min_h(px(44.)).py_2().gap_4().items_start();
    let mut ledger = vec![
        line()
            .text_color(theme.muted_foreground)
            .child(cell().child("Setting"))
            .child(cell().child("Before"))
            .child(cell().child("After"))
            .into_any_element(),
    ];
    ledger.extend(rows.iter().map(|row| {
        line()
            .child(
                cell().child(
                    h_flex()
                        .flex_wrap()
                        .gap_x_2()
                        .child(
                            div()
                                .font_weight(gpui_kit::FontWeight::MEDIUM)
                                .child(row.setting.clone()),
                        )
                        .when_some(row.scope.clone(), |d, scope| {
                            d.child(div().text_color(theme.muted_foreground).child(scope))
                        }),
                ),
            )
            .child(
                cell()
                    .text_color(theme.muted_foreground)
                    .child(row.before.clone()),
            )
            .child(cell().child(row.after.clone()))
            .into_any_element()
    }));
    parts.push(ui::section("", ledger, cx));
    if this.features.host_process.is_some() {
        parts.push(ui::status("Stop host remapping to apply changes", cx));
    }
    if let Err(e) = this.pending() {
        parts.push(ui::error(first_line(&e), cx));
    }
    ui::page("change-review", parts).into_any_element()
}

fn tester_page(this: &mut Workbench, cx: &mut Context<Workbench>) -> AnyElement {
    let theme = cx.theme().clone();
    let testing = this.features.testing;
    let focus = this.features.tester_focus.clone();
    let mut capture = div()
        .id("tester-capture")
        .track_focus(&focus)
        .child(board(
            "tester-board",
            this.model(),
            |slot| {
                let usage = this.model().default_binding(false, 0, slot)[2];
                let held = this.features.held.contains(&usage);
                let seen = this.features.seen.contains(&usage);
                Look {
                    legend: this.model().slot_legend(slot).into(),
                    pressed: held,
                    fill: if held {
                        Some(theme.primary)
                    } else if seen {
                        Some(theme.selection)
                    } else {
                        None
                    },
                    ..Default::default()
                }
            },
            None,
            cx,
        ))
        .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
            if this.features.testing {
                let name = event.keystroke.key.to_string();
                this.features
                    .test_event(keys::usage_from_key_name(&name), name, true);
                cx.stop_propagation();
                cx.notify();
            }
        }))
        .on_key_up(cx.listener(|this, event: &KeyUpEvent, _, cx| {
            if this.features.testing {
                let name = event.keystroke.key.to_string();
                this.features
                    .test_event(keys::usage_from_key_name(&name), name, false);
                cx.stop_propagation();
                cx.notify();
            }
        }))
        .on_modifiers_changed(cx.listener(|this, event: &ModifiersChangedEvent, _, cx| {
            if !this.features.testing {
                return;
            }
            let m = event.modifiers;
            for (down, usage) in [
                (m.control, 224),
                (m.shift, 225),
                (m.alt, 226),
                (m.platform, 227),
            ] {
                if this.features.held.contains(&usage) != down {
                    this.features.test_event(
                        Some(usage),
                        keys::usage_name(usage).unwrap_or("Modifier").into(),
                        down,
                    );
                }
            }
            cx.notify();
        }))
        .on_scroll(cx.listener(|this, event: &gpui_kit::ScrollEvent, _, cx| {
            if this.features.testing {
                let up = event.delta.pixel_delta(px(16.)).y > px(0.);
                this.features
                    .log_event(if up { "Scroll up" } else { "Scroll down" }.into(), None);
                cx.notify();
            }
        }));
    for (button, label) in [
        (gpui_kit::MouseButton::Left, "Left button"),
        (gpui_kit::MouseButton::Right, "Right button"),
        (gpui_kit::MouseButton::Middle, "Middle button"),
    ] {
        capture = capture
            .on_mouse_down(
                button,
                cx.listener(move |this, _: &gpui_kit::MouseDownEvent, _, cx| {
                    this.features.test_event(None, label.into(), true);
                    cx.notify();
                }),
            )
            .on_mouse_up(
                button,
                cx.listener(move |this, _: &gpui_kit::MouseUpEvent, _, cx| {
                    this.features.test_event(None, label.into(), false);
                    cx.notify();
                }),
            );
    }
    let empty = this.features.events.is_empty() && this.features.seen.is_empty();
    let toolbar = h_flex()
        .gap_2()
        .child(
            Button::new("tester-toggle")
                .outline()
                .small()
                .label(if testing {
                    "Stop testing"
                } else {
                    "Start testing"
                })
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.features.held.clear();
                    this.features.testing = !testing;
                    if !testing {
                        window.focus(&this.features.tester_focus, cx);
                    }
                    cx.notify();
                })),
        )
        .child(
            Button::new("tester-reset")
                .ghost()
                .small()
                .label("Clear")
                .disabled(empty)
                .on_click(cx.listener(|this, _, _, cx| {
                    this.features.seen.clear();
                    this.features.events.clear();
                    this.features.held.clear();
                    cx.notify();
                })),
        );
    let mut parts = vec![toolbar.into_any_element(), capture.into_any_element()];
    if !this.features.events.is_empty() {
        let rows = this
            .features
            .events
            .iter()
            .take(30)
            .map(|(label, kind)| {
                h_flex()
                    .min_h(px(40.))
                    .gap_4()
                    .child(div().flex_1().text_sm().child(label.clone()))
                    .children(kind.map(|kind| {
                        div()
                            .text_sm()
                            .text_color(theme.muted_foreground)
                            .child(kind)
                    }))
                    .into_any_element()
            })
            .collect();
        parts.push(ui::section("Events", rows, cx));
    }
    ui::page("tester", parts).into_any_element()
}

fn desktop_page(this: &mut Workbench, cx: &mut Context<Workbench>) -> AnyElement {
    use gpui_kit::assets::IconName as I;
    let prefs = &this.features.preferences;
    let alerts = prefs.battery_alerts;
    let battery = ui::section(
        "Battery",
        vec![
            ui::icon_row(
                I::BellRing,
                "Low-battery notifications",
                Switch::new("battery-alerts")
                    .checked(alerts)
                    .on_click(cx.listener(|this, on: &bool, _, cx| {
                        this.features.preferences.battery_alerts = *on;
                        this.features.battery_latched = false;
                        this.save_preferences(cx);
                    })),
            ),
            ui::icon_row(
                I::BatteryLow,
                "Notify at",
                ui::valued(
                    Slider::new(&this.features.battery).disabled(!alerts),
                    format!("{}%", prefs.battery_threshold.clamp(5, 50)),
                    cx,
                ),
            ),
        ],
        cx,
    );
    let startup = ui::section(
        "Startup",
        vec![
            ui::icon_row(
                I::PanelTop,
                "Tray icon",
                Switch::new("tray-enabled")
                    .checked(prefs.tray)
                    .on_click(cx.listener(|this, on: &bool, window, cx| {
                        this.features.preferences.tray = *on;
                        this.features.desktop_error = None;
                        if *on {
                            this.enable_tray(window, cx);
                        } else {
                            this.features.tray = None;
                        }
                        this.save_preferences(cx);
                    })),
            ),
            ui::icon_row(
                I::LogIn,
                "Open at login",
                Switch::new("autostart")
                    .checked(this.features.autostart)
                    .on_click(cx.listener(|this, on: &bool, _, cx| {
                        match std::env::current_exe()
                            .map_err(anyhow::Error::from)
                            .and_then(|exe| desktop::set_autostart(*on, &exe))
                        {
                            Ok(()) => this.features.autostart = *on,
                            Err(e) => this.features.desktop_error = Some(e.to_string()),
                        }
                        cx.notify();
                    })),
            ),
        ],
        cx,
    );
    let mut parts = vec![battery, startup];
    if let Some(error) = &this.features.desktop_error {
        parts.push(ui::error(error.clone(), cx));
    }
    ui::page("desktop", parts).into_any_element()
}

fn host_page(this: &mut Workbench, cx: &mut Context<Workbench>) -> AnyElement {
    use gpui_kit::assets::IconName as I;
    let theme = cx.theme().clone();
    let running = this.features.host_process.is_some();
    let files = h_flex()
        .gap_2()
        .child(
            Button::new("host-open")
                .outline()
                .small()
                .icon(I::FolderOpen)
                .label("Open")
                .disabled(running)
                .on_click(cx.listener(|this, _, window, cx| this.host_file(true, window, cx))),
        )
        .child(
            Button::new("host-save")
                .outline()
                .small()
                .icon(I::Save)
                .label("Save")
                .disabled(running)
                .on_click(cx.listener(|this, _, window, cx| this.host_file(false, window, cx))),
        );
    let state: AnyElement = if let Some(error) = &this.features.host_error {
        div()
            .flex_1()
            .min_w_0()
            .child(ui::error(error.clone(), cx))
            .into_any_element()
    } else {
        let text = match (&this.features.host_config, running) {
            (_, true) => Some("Running".to_string()),
            (Some(config), false) => {
                let count = |n: usize, one: &str, many: &str| {
                    format!("{n} {}", if n == 1 { one } else { many })
                };
                Some(format!(
                    "Valid with {}, {}, {}",
                    count(config.devices.len(), "device", "devices"),
                    count(config.rules.len(), "rule", "rules"),
                    count(config.chords.len(), "chord", "chords"),
                ))
            }
            _ => None,
        };
        div()
            .flex_1()
            .min_w_0()
            .children(text.map(|t| ui::status(t, cx)))
            .into_any_element()
    };
    let run = h_flex()
        .gap_2()
        .child(state)
        .child(
            Button::new("host-validate")
                .ghost()
                .small()
                .label("Validate")
                .disabled(running)
                .on_click(cx.listener(|this, _, _, cx| {
                    this.validate_host(cx);
                })),
        )
        .child(if running {
            Button::new("host-start-stop")
                .outline()
                .small()
                .icon(I::CircleStop)
                .label("Stop remapping")
                .on_click(cx.listener(|this, _, _, cx| {
                    this.features.host_process = None;
                    cx.notify();
                }))
        } else {
            Button::new("host-start-stop")
                .primary()
                .small()
                .icon(I::Play)
                .label("Start remapping")
                .on_click(cx.listener(|this, _, window, cx| this.start_host(window, cx)))
        });
    let refresh = Button::new("host-devices")
        .ghost()
        .small()
        .icon(gpui_kit::assets::IconName::RefreshCw)
        .label("Refresh")
        .on_click(cx.listener(|this, _, _, cx| {
            this.features.host_devices = input_devices();
            cx.notify();
        }));
    let devices = match &this.features.host_devices {
        Err(e) => ui::error(e.clone(), cx),
        Ok(list) if list.is_empty() => ui::placeholder("No input devices", 96., cx),
        Ok(list) => ui::section(
            "",
            list.iter()
                .enumerate()
                .map(|(i, (name, path))| {
                    let copy = path.display().to_string();
                    h_flex()
                        .min_h(px(52.))
                        .py_2()
                        .gap_4()
                        .child(div().flex_1().min_w_0().text_sm().truncate().child(
                            if name.is_empty() {
                                "Unnamed device".to_string()
                            } else {
                                name.clone()
                            },
                        ))
                        .child(
                            div()
                                .flex_shrink_0()
                                .text_sm()
                                .font_family(theme.mono_font_family.clone())
                                .text_color(theme.muted_foreground)
                                .child(copy.clone()),
                        )
                        .child(
                            Button::new(("copy-device", i))
                                .ghost()
                                .small()
                                .icon(I::Copy)
                                .tooltip("Copy path")
                                .on_click(cx.listener(move |_, _, window, cx| {
                                    cx.write_to_clipboard(gpui_kit::ClipboardItem::new_string(
                                        copy.clone(),
                                    ));
                                    window.push_notification(Notification::success("Copied"), cx);
                                })),
                        )
                        .into_any_element()
                })
                .collect(),
            cx,
        ),
    };
    ui::page(
        "host-remapping",
        vec![
            ui::heading_row("Configuration", files, cx),
            Textarea::new(&this.features.host_editor)
                .h(px(360.))
                .font_family(theme.mono_font_family.clone())
                .disabled(running)
                .into_any_element(),
            run.into_any_element(),
            ui::heading_row("Input devices", refresh, cx),
            devices,
        ],
    )
    .into_any_element()
}

pub fn picture_controls(this: &mut Workbench, cx: &mut Context<Workbench>) -> AnyElement {
    let clean = !this.previewing
        && this.baseline.is_some()
        && this.busy.is_none()
        && this.pending().is_ok_and(|v| v.is_empty());
    let actions = h_flex()
        .gap_2()
        .child(
            Button::new("pictures-backup")
                .outline()
                .small()
                .icon(gpui_kit::assets::IconName::Download)
                .label("Back up all")
                .loading(this.busy == Some(Busy::Capturing))
                .disabled(!clean)
                .on_click(cx.listener(|this, _, window, cx| this.capture_pictures(window, cx))),
        )
        .child(
            Button::new("pictures-open")
                .outline()
                .small()
                .icon(gpui_kit::assets::IconName::Upload)
                .label("Open")
                .on_click(cx.listener(|this, _, window, cx| this.open_picture_backup(window, cx))),
        );
    let mut parts = vec![ui::heading_row("Pictures", actions, cx)];
    if this.features.picture_backup.is_some() {
        parts.push(ui::section(
            "",
            (0..this.model().picture_count())
                .map(|picture| {
                    h_flex()
                        .min_h(px(52.))
                        .gap_4()
                        .child(
                            div()
                                .flex_1()
                                .text_sm()
                                .font_weight(gpui_kit::FontWeight::MEDIUM)
                                .child(format!("Picture {}", picture + 1)),
                        )
                        .child(
                            Button::new(("picture-restore", picture as usize))
                                .ghost()
                                .small()
                                .label("Restore")
                                .disabled(!this.editable())
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.restore_picture(picture, window, cx)
                                })),
                        )
                        .into_any_element()
                })
                .collect(),
            cx,
        ));
    }
    if let Some(error) = &this.features.picture_error {
        parts.push(ui::error(error.clone(), cx));
    }
    v_flex().gap_3().children(parts).into_any_element()
}
