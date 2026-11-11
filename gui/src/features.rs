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
    switch::Switch,
    v_flex,
};
use gpui_kit::{
    AnyElement, App, AppContext, Context, Entity, FocusHandle, InteractiveElement, IntoElement,
    KeyDownEvent, KeyUpEvent, ModifiersChangedEvent, ParentElement, SharedString, Styled,
    Subscription, Window, div, prelude::FluentBuilder, px,
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
    pub events: VecDeque<String>,
    pub host_editor: Entity<TextareaState>,
    pub host_error: Option<String>,
    pub host_process: Option<desktop::HostProcess>,
    pub host_config: Option<host::Config>,
    pub picture_backup: Option<monsgeek_linux::pictures::Backup>,
    pub operation: Option<String>,
    _blur: Subscription,
    _host_edits: Subscription,
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
        Self {
            profiles: profiles.unwrap_or_default(),
            profile_error: error,
            selection: Selection::default(),
            name: cx.new(|cx| InputState::new(window, cx).placeholder("For example, Coding")),
            source: None,
            preferences: settings::load(),
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
            picture_backup: None,
            operation: None,
            _blur: blur,
            _host_edits: host_edits,
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
        self.events.push_front(format!(
            "{}  {label}",
            if down { "Press" } else { "Release" }
        ));
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
                            desktop::Event::StopHost => { this.features.host_process = None; this.features.operation = Some("Host remapping stopped".into()); }
                            desktop::Event::Quit => { this.features.host_process = None; cx.quit(); }
                        }
                    }
                    if let Some(process) = &mut this.features.host_process {
                        match process.0.try_wait() {
                            Ok(Some(status)) => {
                                if status.success() { this.features.host_error=None; this.features.operation=Some("Host remapping stopped".into()); } else { let error=process.1.lock().unwrap_or_else(|e|e.into_inner()).clone(); this.features.host_error = Some(if error.trim().is_empty(){format!("Host remapping stopped ({status}). Check the configuration and input permissions.")}else{error.trim().to_owned()}); }
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
                Some((kind, Some(battery))) => format!("{kind} · {battery}%"),
                Some((_, None)) if !presence.usb => "Asleep".into(),
                _ if presence.usb => "USB".into(),
                _ => "Disconnected".into(),
            };
            tray.handle
                .update(move |tray| tray.title = format!("MonsGeek M1W · {status}"));
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
                this.update_in(cx, |this, window, cx| {
                    match result {
                        Ok(()) => {
                            this.features.operation = Some(
                                "Diagnostics exported without keymaps, macros, or recorded input"
                                    .into(),
                            );
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
        if self.busy.is_some()
            || !self.pending().is_ok_and(|v| v.is_empty())
            || self.baseline.is_none()
        {
            return;
        }
        let view = cx.entity();
        window.open_dialog(cx,move |dialog,_,_| {
            let capture = view.clone();
            dialog.title("Back up all RGB pictures").w(px(520.))
                .child(div().text_sm().child("This briefly selects each of the three pictures, saves their colours, and restores the original lighting effect. A recovery snapshot is saved before selection changes."))
                .footer(h_flex().w_full().justify_end().gap_2()
                    .child(Button::new("capture-cancel").ghost().label("Cancel").on_click(|_,window,cx|window.close_dialog(cx)))
                    .child(Button::new("capture-pictures").primary().label("Back up pictures").on_click(move |_,window,cx| {
                        window.close_dialog(cx);
                        capture.update(cx,|this,cx|this.choose_picture_path(window,cx));
                    })))
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
                    this.features.operation = Some("Backing up three RGB pictures…".into());
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
                        this.features.operation =
                            Some("All three pictures saved; original lighting restored".into());
                    }
                    Err(e) => {
                        this.features.operation = Some(format!("Picture backup failed: {e}"));
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
            prompt: Some("Open three-picture backup".into()),
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
                        Ok(backup) => this.features.picture_backup = Some(backup),
                        Err(e) => this.features.operation = Some(e.to_string()),
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
                Err(e) => self.features.operation = Some(e.to_string()),
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
                .child(div().text_sm().child("The selected input devices will be captured exclusively and remapped on this computer. Ctrl+Alt+Esc stops the service. Closing this app stops it too. Input and uinput permissions must already be configured."))
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

fn message(text: impl Into<SharedString>, cx: &App) -> AnyElement {
    div()
        .text_sm()
        .text_color(cx.theme().muted_foreground)
        .child(text.into())
        .into_any_element()
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
    let mut rows = vec![
        h_flex()
            .py_3()
            .gap_3()
            .child(message(
                "Save the current draft as a named configuration.",
                cx,
            ))
            .child(div().flex_1())
            .child(
                Button::new("new-profile")
                    .outline()
                    .label("Save current draft")
                    .disabled(this.draft.is_none() || this.busy.is_some())
                    .on_click(cx.listener(|this, _, window, cx| {
                        if let Some(snapshot) = this.draft.clone() {
                            this.name_profile(snapshot, String::new(), window, cx)
                        }
                    })),
            )
            .into_any_element(),
    ];
    for (index, (_, profile)) in this.features.profiles.clone().into_iter().enumerate() {
        let load = profile.clone();
        rows.push(
            h_flex()
                .py_3()
                .gap_3()
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .child(div().text_sm().child(profile.name.clone()))
                        .child(message(
                            format!("Firmware {:04X}", profile.snapshot.usb_revision),
                            cx,
                        )),
                )
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
                        .label("Load for review")
                        .disabled(!this.editable())
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.load_profile(load.clone(), window, cx)
                        })),
                )
                .into_any_element(),
        );
    }
    if this.features.profiles.is_empty() {
        rows.push(message(
            "No named profiles yet. Saved profiles stay on this computer.",
            cx,
        ));
    }
    let s = this.features.selection;
    let selections: Vec<_> = [
        ("Keys", s.keys, 0),
        ("Macros", s.macros, 1),
        ("Lighting", s.lighting, 2),
        ("Performance", s.performance, 3),
        ("Keyboard options", s.options, 4),
        ("Active layer", s.active_layer, 5),
    ]
    .into_iter()
    .map(|(label, on, index)| {
        div()
            .py_2()
            .child(
                Checkbox::new(("profile-part", index as usize))
                    .label(label)
                    .checked(on)
                    .on_click(cx.listener(move |this, on: &bool, _, cx| {
                        match index {
                            0 => this.features.selection.keys = *on,
                            1 => this.features.selection.macros = *on,
                            2 => this.features.selection.lighting = *on,
                            3 => this.features.selection.performance = *on,
                            4 => this.features.selection.options = *on,
                            _ => this.features.selection.active_layer = *on,
                        }
                        cx.notify();
                    })),
            )
            .into_any_element()
    })
    .collect();
    let mut sections = vec![
        ui::section("Named configurations", rows, cx),
        ui::section("Include when loading", selections, cx),
    ];
    if let Some(error) = &this.features.profile_error {
        sections.insert(0, message(error.clone(), cx));
    }
    sections.push(message("Lighting includes only the picture captured in the profile. Load keys and macros together when bindings reference saved macros.",cx));
    ui::page("profiles", sections).into_any_element()
}
fn review_page(this: &mut Workbench, cx: &mut Context<Workbench>) -> AnyElement {
    let rows = match (&this.baseline, &this.draft) {
        (Some(b), Some(d)) => review::rows(b, d),
        _ => Ok(Vec::new()),
    };
    let rows = match rows {
        Ok(rows) => rows,
        Err(e) => {
            return ui::page("review-error", vec![message(e.to_string(), cx)]).into_any_element();
        }
    };
    let changed: BTreeSet<_> = rows.iter().filter_map(|r| r.slot).collect();
    let mut parts = vec![board(
        "review-board",
        |slot| Look {
            legend: keys::slot_legend(slot).into(),
            action: changed.contains(&slot).then(|| "Δ".into()),
            selected: changed.contains(&slot),
            ..Default::default()
        },
        None,
        cx,
    )];
    if rows.is_empty() {
        parts.push(message(
            "No pending changes. Edit a setting or load a profile to review it here.",
            cx,
        ));
    } else {
        parts.push(
            v_flex()
                .gap_3()
                .children(rows.iter().map(|row| {
                    v_flex()
                        .py_3()
                        .gap_1()
                        .border_b_1()
                        .border_color(cx.theme().border)
                        .child(
                            div()
                                .text_sm()
                                .font_weight(gpui_kit::FontWeight::MEDIUM)
                                .child(row.setting.clone()),
                        )
                        .child(
                            h_flex()
                                .items_start()
                                .gap_4()
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .text_sm()
                                        .text_color(cx.theme().muted_foreground)
                                        .child(format!("Before: {}", row.before)),
                                )
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .text_sm()
                                        .child(format!("After: {}", row.after)),
                                ),
                        )
                }))
                .into_any_element(),
        );
        parts.push(
            h_flex()
                .justify_end()
                .gap_2()
                .child(
                    Button::new("review-apply")
                        .primary()
                        .label("Apply changes")
                        .loading(this.busy == Some(Busy::Applying))
                        .disabled(
                            this.busy.is_some()
                                || this.pending().is_err()
                                || this.features.host_process.is_some(),
                        )
                        .on_click(cx.listener(|this, _, window, cx| this.apply(window, cx))),
                )
                .into_any_element(),
        );
        parts.push(message(
            "Apply saves a before-state and verifies each write. Changes remain local until then.",
            cx,
        ));
        if this.features.host_process.is_some() {
            parts.push(message(
                "Stop host remapping before applying onboard changes.",
                cx,
            ));
        }
    }
    if let Err(e) = this.pending() {
        parts.push(
            div()
                .text_sm()
                .text_color(cx.theme().danger)
                .child(e.to_string())
                .into_any_element(),
        );
    }
    ui::page("change-review", parts).into_any_element()
}
fn tester_page(this: &mut Workbench, cx: &mut Context<Workbench>) -> AnyElement {
    let testing = this.features.testing;
    let focus = this.features.tester_focus.clone();
    let mut capture = v_flex()
        .id("tester-capture")
        .track_focus(&focus)
        .p_4()
        .gap_3()
        .rounded(cx.theme().radius_lg)
        .bg(cx.theme().muted)
        .child(message(
            if testing {
                "Listening while this panel has focus. Click elsewhere to stop."
            } else {
                "Start testing, then press keys or turn the knob. Events stay in memory."
            },
            cx,
        ))
        .child(board(
            "tester-board",
            |slot| {
                let binding = keys::default_binding(false, slot);
                let usage = binding[2];
                Look {
                    legend: keys::slot_legend(slot).into(),
                    selected: this.features.held.contains(&usage),
                    action: this.features.seen.contains(&usage).then(|| "✓".into()),
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
                this.features
                    .events
                    .push_front(format!("Scroll {:?}", event.delta));
                this.features.events.truncate(100);
                cx.notify();
            }
        }));
    for button in [
        gpui_kit::MouseButton::Left,
        gpui_kit::MouseButton::Right,
        gpui_kit::MouseButton::Middle,
    ] {
        capture = capture
            .on_mouse_down(
                button,
                cx.listener(move |this, _: &gpui_kit::MouseDownEvent, _, cx| {
                    this.features
                        .test_event(None, format!("Mouse {button:?}"), true);
                    cx.notify();
                }),
            )
            .on_mouse_up(
                button,
                cx.listener(move |this, _: &gpui_kit::MouseUpEvent, _, cx| {
                    this.features
                        .test_event(None, format!("Mouse {button:?}"), false);
                    cx.notify();
                }),
            );
    }
    let events = v_flex().gap_1().children(
        this.features
            .events
            .iter()
            .take(30)
            .map(|event| div().text_sm().child(event.clone())),
    );
    ui::page("tester",vec![h_flex().gap_2()
        .child(Button::new("tester-start").outline().label("Start testing").on_click(cx.listener(|this,_,window,cx|{this.features.testing = true;this.features.held.clear();window.focus(&this.features.tester_focus,cx);cx.notify();})))
        .child(Button::new("tester-stop").ghost().label("Stop").disabled(!testing).on_click(cx.listener(|this,_,_,cx|{this.features.testing = false;this.features.held.clear();cx.notify();})))
        .child(Button::new("tester-reset").ghost().label("Clear").on_click(cx.listener(|this,_,_,cx|{this.features.seen.clear();this.features.events.clear();this.features.held.clear();cx.notify();}))).into_any_element(),
        capture.into_any_element(),message("The board highlights emitted key names, not physical source positions. Fn and media actions intercepted by the desktop may not reach this window.",cx),events.into_any_element()]).into_any_element()
}
fn desktop_page(this: &mut Workbench, cx: &mut Context<Workbench>) -> AnyElement {
    use gpui_kit::assets::IconName as I;
    let prefs = &this.features.preferences;
    let threshold = prefs.battery_threshold.clamp(5, 50);
    let rows = vec![
        ui::icon_row(
            I::Battery,
            "Low-battery notifications",
            Switch::new("battery-alerts")
                .checked(prefs.battery_alerts)
                .on_click(cx.listener(|this, on: &bool, _, cx| {
                    this.features.preferences.battery_alerts = *on;
                    this.features.battery_latched = false;
                    this.save_preferences(cx);
                })),
        ),
        ui::icon_row(
            I::BatteryLow,
            "Notify below",
            h_flex()
                .gap_2()
                .child(
                    Button::new("battery-less")
                        .ghost()
                        .label("−")
                        .disabled(threshold <= 5)
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.features.preferences.battery_threshold = this
                                .features
                                .preferences
                                .battery_threshold
                                .saturating_sub(5)
                                .max(5);
                            this.features.battery_latched = false;
                            this.save_preferences(cx);
                        })),
                )
                .child(div().text_sm().child(format!("{threshold}%")))
                .child(
                    Button::new("battery-more")
                        .ghost()
                        .label("+")
                        .disabled(threshold >= 50)
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.features.preferences.battery_threshold =
                                (this.features.preferences.battery_threshold + 5).min(50);
                            this.features.battery_latched = false;
                            this.save_preferences(cx);
                        })),
                ),
        ),
        ui::icon_row(
            I::Monitor,
            "Show tray icon",
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
            I::Monitor,
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
    ];
    let mut parts = vec![
        ui::section("Desktop integration", rows, cx),
        message(
            "Battery levels are available over wireless links. Tray support depends on your desktop's StatusNotifier host. Open at login starts this app; host remapping still needs Start.",
            cx,
        ),
        ui::section(
            "Support",
            vec![ui::icon_row(
                I::Download,
                "Support report",
                Button::new("diagnostics-export")
                    .outline()
                    .label("Export diagnostics")
                    .on_click(
                        cx.listener(|this, _, window, cx| this.export_diagnostics(window, cx)),
                    ),
            )],
            cx,
        ),
    ];
    if let Some(error) = &this.features.desktop_error {
        parts.push(message(error.clone(), cx));
    }
    if let Some(status) = &this.features.operation {
        parts.push(message(status.clone(), cx));
    }
    ui::page("desktop", parts).into_any_element()
}
fn host_page(this: &mut Workbench, cx: &mut Context<Workbench>) -> AnyElement {
    let running = this.features.host_process.is_some();
    let mut parts = vec![
        message(
            "Tap-hold, tap dance, chords, shortcuts, and launch actions run on this computer. Application rules support niri, Sway, Hyprland, X11, or a custom focus command.",
            cx,
        ),
        h_flex()
            .gap_2()
            .child(
                Button::new("host-open")
                    .outline()
                    .label("Open configuration")
                    .disabled(running)
                    .on_click(cx.listener(|this, _, window, cx| this.host_file(true, window, cx))),
            )
            .child(
                Button::new("host-save")
                    .outline()
                    .label("Save configuration")
                    .disabled(running)
                    .on_click(cx.listener(|this, _, window, cx| this.host_file(false, window, cx))),
            )
            .child(
                Button::new("host-validate")
                    .ghost()
                    .label("Validate")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.validate_host(cx);
                    })),
            )
            .child(div().flex_1())
            .child(
                Button::new("host-start-stop")
                    .outline()
                    .label(if running {
                        "Stop remapping"
                    } else {
                        "Start remapping"
                    })
                    .on_click(cx.listener(move |this, _, window, cx| {
                        if running {
                            this.features.host_process = None;
                            cx.notify();
                        } else {
                            this.start_host(window, cx);
                        }
                    })),
            )
            .into_any_element(),
        ui::heading("Advanced configuration", cx).into_any_element(),
        Textarea::new(&this.features.host_editor)
            .h(px(360.))
            .disabled(running)
            .into_any_element(),
        message(
            "Replace the example's device path with your event device. Key numbers are Linux input codes; Caps Lock=58, Esc=1, Ctrl=29, Space=57. Commands use an argument list and are never interpreted by a shell.",
            cx,
        ),
    ];
    if let Some(error) = &this.features.host_error {
        parts.insert(
            1,
            div()
                .text_sm()
                .text_color(cx.theme().danger)
                .child(error.clone())
                .into_any_element(),
        );
    } else if let Some(config) = &this.features.host_config {
        parts.insert(
            1,
            message(
                format!(
                    "{} devices · {} rules · {} chords{}",
                    config.devices.len(),
                    config.rules.len(),
                    config.chords.len(),
                    if running {
                        " · Running"
                    } else {
                        " · Validated"
                    }
                ),
                cx,
            ),
        );
    }
    parts.push(
        Button::new("host-devices")
            .ghost()
            .label("Show available input devices")
            .on_click(cx.listener(|this, _, _, cx| {
                match host::linux::devices() {
                    Ok(devices) => {
                        this.features.operation = Some(
                            devices
                                .iter()
                                .map(|d| format!("{} — {}", d.path.display(), d.name))
                                .collect::<Vec<_>>()
                                .join("\n"),
                        )
                    }
                    Err(e) => this.features.host_error = Some(e.to_string()),
                }
                cx.notify();
            }))
            .into_any_element(),
    );
    if let Some(status) = &this.features.operation {
        parts.push(message(status.clone(), cx));
    }
    ui::page("host-remapping", parts).into_any_element()
}

pub fn picture_controls(this: &mut Workbench, cx: &mut Context<Workbench>) -> AnyElement {
    let clean = this.baseline.is_some()
        && this.busy.is_none()
        && this.pending().is_ok_and(|v| v.is_empty());
    let mut rows = vec![
        h_flex()
            .py_3()
            .gap_2()
            .child(
                Button::new("pictures-backup")
                    .outline()
                    .label("Back up all three pictures")
                    .disabled(!clean)
                    .on_click(cx.listener(|this, _, window, cx| this.capture_pictures(window, cx))),
            )
            .child(
                Button::new("pictures-open")
                    .ghost()
                    .label("Open picture backup")
                    .on_click(
                        cx.listener(|this, _, window, cx| this.open_picture_backup(window, cx)),
                    ),
            )
            .into_any_element(),
    ];
    if this.features.picture_backup.is_some() {
        for picture in 0..3 {
            rows.push(ui::icon_row(
                gpui_kit::assets::IconName::Image,
                format!("Picture {}", picture + 1),
                Button::new(("picture-restore", picture as usize))
                    .outline()
                    .small()
                    .label("Load colours for review")
                    .disabled(!this.editable())
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.restore_picture(picture, window, cx)
                    })),
            ));
        }
    }
    if let Some(status) = &this.features.operation {
        rows.push(message(status.clone(), cx));
    }
    ui::section("Complete RGB backup", rows, cx)
}
