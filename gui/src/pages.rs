//! Page compositions. Each page edits the draft through `Workbench`.
use std::{rc::Rc, time::SystemTime};

use gpui_kit::component::{
    ActiveTheme, Disableable, Icon, IconName, Sizable,
    button::{Button, ButtonVariants, Toggle, ToggleVariants},
    color_picker::ColorPicker,
    empty::{Empty, EmptyContent, EmptyHeader, EmptyMedia, EmptyMediaVariant, EmptyTitle},
    h_flex,
    input::NumberInput,
    progress::Progress,
    scroll::ScrollableElement,
    select::Select,
    slider::Slider,
    switch::Switch,
    tab::{Tab, TabBar},
    v_flex,
};
use gpui_kit::{
    AnyElement, App, Context, InteractiveElement, IntoElement, KeyDownEvent, KeyUpEvent,
    ModifiersChangedEvent, MouseButton, MouseDownEvent, MouseUpEvent, ParentElement, SharedString,
    StatefulInteractiveElement, Styled, Window, div, prelude::FluentBuilder, px,
};
use monsgeek_linux::{
    config::{Macro, MacroEvent},
    keys::{self, MODIFIERS},
};

use crate::{
    app::{Link, Page, Workbench, hsla},
    board::{Look, OnPick, board, board_lit},
    effects, ui,
};

pub fn render(
    this: &mut Workbench,
    window: &mut Window,
    cx: &mut Context<Workbench>,
) -> AnyElement {
    if let Some(page) = crate::features::render(this, window, cx) {
        return page;
    }
    if this.page == Page::Logs {
        return logs(this, cx);
    }
    if this.page == Page::Models {
        return models_page(this, cx);
    }
    if this.draft.is_none() {
        return match &this.link {
            Link::Failed(message) => disconnected(message.clone(), this, cx),
            _ => loading(this, cx),
        };
    }
    match this.page {
        Page::Lighting => lighting(this, window, cx),
        Page::Models => unreachable!(),
        Page::Magnetic => magnetic(this, cx),
        Page::Keys => keys_page(this, window, cx),
        Page::Macros => macros(this, window, cx),
        Page::Performance => performance(this, cx),
        Page::System => system(this, cx),
        Page::Backups => backups(this, cx),
        Page::Logs => unreachable!(),
        Page::Profiles | Page::Review | Page::Tester | Page::Desktop | Page::Host => unreachable!(),
    }
}

fn models_page(this: &mut Workbench, cx: &mut Context<Workbench>) -> AnyElement {
    let model = monsgeek_linux::models::get(this.preview_model).expect("selected model");
    let can_open = this.busy.is_none()
        && !this.features.calibrating
        && this.pending().is_ok_and(|p| p.is_empty());
    ui::page(
        "models",
        vec![
            ui::section(
                "",
                vec![
                    ui::icon_row(
                        gpui_kit::assets::IconName::Keyboard,
                        "Model",
                        div().w(px(300.)).child(
                            Select::new(&this.model_select)
                                .placeholder("Choose a keyboard")
                                .search_placeholder("Search models")
                                .menu_max_h(px(360.)),
                        ),
                    ),
                    h_flex()
                        .py_3()
                        .gap_3()
                        .child(
                            Button::new("preview-model")
                                .primary()
                                .label("Open preview")
                                .disabled(!can_open)
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    if let Ok(model) =
                                        monsgeek_linux::models::get(this.preview_model)
                                    {
                                        this.loaded(model.example_snapshot(), window, cx);
                                        this.previewing = true;
                                        this.transport = None;
                                        this.page = Page::Lighting;
                                        cx.notify();
                                    }
                                })),
                        )
                        .child(
                            div().text_sm().child(
                                "Preview layouts and lighting without connecting a keyboard.",
                            ),
                        )
                        .into_any_element(),
                ],
                cx,
            ),
            board(
                "model-board",
                model,
                |slot| Look {
                    legend: model.slot_legend(slot).into(),
                    ..Default::default()
                },
                None,
                cx,
            ),
        ],
    )
    .into_any_element()
}

fn magnetic(this: &mut Workbench, cx: &mut Context<Workbench>) -> AnyElement {
    let model = this.model();
    let Some(settings) = this
        .draft
        .as_ref()
        .and_then(|d| d.advanced.clone())
        .filter(|s| !s.magnetic.is_empty())
    else {
        return ui::page(
            "no-actuation",
            vec![ui::placeholder(
                "Actuation settings are unavailable",
                120.,
                cx,
            )],
        )
        .into_any_element();
    };
    let entity = cx.entity();
    let keyboard = board(
        "actuation-board",
        model,
        |slot| Look {
            legend: model.slot_legend(slot).into(),
            action: if !model
                .layout()
                .iter()
                .any(|c| c.slot == slot && c.usage != 0)
            {
                None
            } else if this.features.live_travel {
                this.features.travel.get(usize::from(slot)).map(|v| {
                    if model.family == monsgeek_linux::models::Family::Yc500 {
                        format!("{:.0}%", v * 100.).into()
                    } else {
                        format!("{v:.2}").into()
                    }
                })
            } else {
                settings
                    .value(model, 0, slot)
                    .map(|v| format!("{v:.2}").into())
            },
            selected: this.selected == Some(slot),
            ..Default::default()
        },
        Some(Rc::new(move |slot, window, cx| {
            entity.update(cx, |this, cx| this.select_key(Some(slot), window, cx));
        })),
        cx,
    );
    let can_finish = !this.previewing
        && this.busy.is_none()
        && this.transport == Some(monsgeek_linux::transport::Kind::Usb);
    let can_calibrate = can_finish && this.pending().is_ok_and(|p| p.is_empty());
    let calibration = ui::section(
        "Sensor calibration",
        vec![
            h_flex()
                .py_3()
                .gap_3()
                .child(
                    Button::new("live-travel")
                        .outline()
                        .small()
                        .label(if this.features.live_travel {
                            "Stop live view"
                        } else {
                            "Live travel"
                        })
                        .disabled(this.previewing || this.busy.is_some())
                        .on_click(
                            cx.listener(|this, _, window, cx| this.toggle_travel(window, cx)),
                        ),
                )
                .child(
                    Button::new("calibrate-begin")
                        .outline()
                        .small()
                        .label("Calibrate switches")
                        .disabled(!can_calibrate || this.features.calibrating)
                        .on_click(
                            cx.listener(|this, _, window, cx| this.confirm_calibration(window, cx)),
                        ),
                )
                .child(
                    Button::new("calibrate-finish")
                        .outline()
                        .small()
                        .label("Finish calibration")
                        .disabled(!can_finish)
                        .loading(this.busy == Some(crate::app::Busy::Calibrating))
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.run_calibration(false, window, cx)
                        })),
                )
                .into_any_element(),
            div()
                .when(!this.features.calibration_status.is_empty(), |d| {
                    d.py_2()
                        .text_sm()
                        .child(this.features.calibration_status.clone())
                })
                .into_any_element(),
        ],
        cx,
    );
    let Some(slot) = this.selected.filter(|s| {
        model
            .layout()
            .iter()
            .any(|cap| cap.slot == *s && cap.usage != 0)
    }) else {
        return ui::page(
            "actuation",
            vec![
                keyboard,
                calibration,
                ui::placeholder("Select a key to edit its actuation", 96., cx),
            ],
        )
        .into_any_element();
    };
    let raw_mode = settings
        .magnetic
        .get(&7)
        .map_or(0, |m| m[usize::from(slot)]);
    let mode = raw_mode & 127;
    let rapid = raw_mode & 128 != 0;
    let mut rows = vec![
        ui::icon_row(
            gpui_kit::assets::IconName::Gauge,
            "Key behavior",
            segmented(
                "magnetic-mode",
                ["Normal", "Dynamic", "Tap-hold", "Hold toggle", "Tap toggle"],
                [0, 2, 3, 4, 5].iter().position(|v| *v == mode).unwrap_or(0),
                false,
                move |this, index, window, cx| {
                    this.edit(cx, |d| {
                        if let Some(s) = &mut d.advanced {
                            let _ = s.set_mode(slot, [0, 2, 3, 4, 5][index], rapid);
                        }
                    });
                    this.sync_magnetic(window, cx);
                },
                cx,
            ),
        ),
        ui::icon_row(
            gpui_kit::assets::IconName::Zap,
            "Rapid trigger",
            Switch::new("rapid-trigger")
                .checked(rapid)
                .on_click(cx.listener(move |this, on: &bool, _, cx| {
                    this.edit(cx, |d| {
                        if let Some(s) = &mut d.advanced
                            && let Some(m) = s.magnetic.get_mut(&7)
                        {
                            m[usize::from(slot)] =
                                (m[usize::from(slot)] & 127) | if *on { 128 } else { 0 };
                        }
                    });
                })),
        ),
    ];
    for (parameter, label) in [
        (0, "Actuation"),
        (1, "Release"),
        (2, "Rapid press"),
        (3, "Rapid release"),
        (6, "Bottom dead zone"),
        (251, "Top dead zone"),
        (4, "Dynamic threshold"),
        (5, "Tap-hold time"),
    ] {
        if matches!(parameter, 2 | 3) && !rapid
            || parameter == 4 && mode != 2
            || parameter == 5 && mode != 3
        {
            continue;
        }
        let Some(_) = settings.value(model, parameter, slot) else {
            continue;
        };
        rows.push(ui::icon_row(
            gpui_kit::assets::IconName::ArrowLeftRight,
            label,
            div().w(px(208.)).child(
                NumberInput::new(&this.magnetic_inputs[&parameter])
                    .small()
                    .accessibility_label(label)
                    .suffix(
                        div()
                            .text_sm()
                            .child(if parameter == 5 { "ms" } else { "mm" }),
                    ),
            ),
        ));
        if let Some(error) = this.magnetic_errors.get(&parameter) {
            rows.push(ui::error(error.clone(), cx));
        }
    }
    if mode == 2
        && let Some(data) = settings.magnetic.get(&10)
    {
        for output in 0..4usize {
            let value = data[output * 128 + usize::from(slot)];
            rows.push(ui::icon_row(
                gpui_kit::assets::IconName::Keyboard,
                format!("Output {}", output + 1),
                Button::new(SharedString::from(format!("dynamic-binding-{output}")))
                    .outline()
                    .small()
                    .label("Edit binding")
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.layer = (this.layer / 4) * 4 + output as u8;
                        this.fn_layer = false;
                        this.page = Page::Keys;
                        this.select_key(Some(slot), window, cx);
                    })),
            ));
            for stage in 0..4usize {
                rows.push(ui::icon_row(
                    gpui_kit::assets::IconName::ArrowRight,
                    [
                        "Press: first threshold",
                        "Press: bottom",
                        "Release: first threshold",
                        "Release: top",
                    ][stage],
                    segmented(
                        SharedString::from(format!("dynamic-stage-{output}-{stage}")),
                        ["Off", "Tap", "End hold", "Start hold"],
                        usize::from((value >> (stage * 2)) & 3),
                        false,
                        move |this, state, _, cx| {
                            this.edit(cx, |d| {
                                if let Some(s) = &mut d.advanced
                                    && let Some(data) = s.magnetic.get_mut(&10)
                                {
                                    let i = output * 128 + usize::from(slot);
                                    data[i] = (data[i] & !(3 << (stage * 2)))
                                        | ((state as u8) << (stage * 2));
                                }
                            });
                        },
                        cx,
                    ),
                ));
            }
        }
    }
    if !model.switches.is_empty() {
        rows.push(ui::icon_row(
            gpui_kit::assets::IconName::Keyboard,
            "Switch type",
            div().w(px(240.)).child(
                Select::new(&this.switch_select)
                    .placeholder("Choose a switch")
                    .menu_max_h(px(320.)),
            ),
        ));
    }
    rows.push(ui::icon_row(
        gpui_kit::assets::IconName::ArrowLeftRight,
        "Snap pair",
        div().w(px(240.)).child(
            Select::new(&this.snap_select)
                .placeholder("Choose a partner")
                .search_placeholder("Search keys")
                .menu_max_h(px(320.)),
        ),
    ));
    ui::page(
        "actuation",
        vec![
            keyboard,
            ui::section(model.slot_name(slot), rows, cx),
            calibration,
        ],
    )
    .into_any_element()
}

fn loading(this: &Workbench, cx: &App) -> AnyElement {
    use monsgeek_linux::device::{EXCHANGE_BYTES, Phase, SNAPSHOT_EXCHANGES, snapshot_phase};
    let total = this
        .progress
        .total
        .load(std::sync::atomic::Ordering::Relaxed);
    let total = if total == 0 {
        SNAPSHOT_EXCHANGES
    } else {
        total
    };
    let theme = cx.theme();
    let kind = this.progress.kind();
    let done = this
        .progress
        .done
        .load(std::sync::atomic::Ordering::Relaxed)
        .min(total);
    let fraction = done as f32 / total as f32;
    let phase = match (kind, snapshot_phase(done)) {
        (None, _) => "Opening".to_string(),
        (_, Phase::Firmware) => "Firmware".into(),
        (_, Phase::Keymap { layer, fn_layer }) => format!(
            "Layer {} {} keys",
            layer + 1,
            if fn_layer { "Fn" } else { "base" }
        ),
        (_, Phase::Settings) => "Settings".into(),
        (_, Phase::Picture) => "Custom picture".into(),
        (_, Phase::Macro(slot)) => format!("Macro {} of 50", slot + 1),
        (_, Phase::Done) => "Finishing".into(),
    };
    let phase = {
        let actual = this
            .progress
            .phase
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        if actual.is_empty() {
            phase
        } else {
            actual.clone()
        }
    };
    let kb = |n: usize| format!("{:.1}", (n * EXCHANGE_BYTES) as f32 / 1024.);
    let elapsed = this.read_started.map(|t| t.elapsed().as_secs_f32());
    let left = match elapsed {
        Some(e) if (8..total).contains(&done) => {
            let s = (e / done as f32 * (total - done) as f32).ceil() as u32;
            Some(if s < 60 {
                format!("{s} s left")
            } else {
                format!("{} min {} s left", s / 60, s % 60)
            })
        }
        _ => None,
    };
    let line = |left: AnyElement, right: AnyElement| {
        h_flex()
            .w_full()
            .justify_between()
            .gap_4()
            .child(left)
            .child(right)
    };
    let muted = |text: String| {
        div()
            .text_sm()
            .text_color(theme.muted_foreground)
            .child(text)
            .into_any_element()
    };
    div()
        .size_full()
        .flex()
        .items_center()
        .justify_center()
        .child(
            v_flex()
                .w(px(440.))
                .gap_3()
                .child(line(
                    div()
                        .text_base()
                        .font_weight(gpui_kit::FontWeight::MEDIUM)
                        .child(match kind {
                            Some(k) => format!("Reading over {k}"),
                            None => "Connecting".into(),
                        })
                        .into_any_element(),
                    div()
                        .text_base()
                        .font_weight(gpui_kit::FontWeight::MEDIUM)
                        .child(format!("{}%", (fraction * 100.).floor() as u32))
                        .into_any_element(),
                ))
                .child(Progress::new("read").value(fraction * 100.))
                .child(line(
                    muted(phase),
                    muted(format!("{} of {} KB", kb(done), kb(total))),
                ))
                .child(line(
                    muted(format!("{done} of {total} requests")),
                    muted(left.unwrap_or_default()),
                )),
        )
        .into_any_element()
}

fn disconnected(
    message: SharedString,
    this: &Workbench,
    cx: &mut Context<Workbench>,
) -> AnyElement {
    let reading = this.busy.is_some();
    div()
        .size_full()
        .flex()
        .items_center()
        .justify_center()
        .child(
            Empty::new()
                .header(
                    EmptyHeader::new()
                        .media(
                            EmptyMedia::new()
                                .with_variant(EmptyMediaVariant::Icon)
                                .child(Icon::new(gpui_kit::assets::IconName::Unplug)),
                        )
                        .title(EmptyTitle::new().child("Keyboard not connected"))
                        .description(
                            gpui_kit::component::empty::EmptyDescription::new()
                                .child(div().max_w(px(420.)).child(message)),
                        ),
                )
                .content(
                    EmptyContent::new().child(
                        h_flex()
                            .gap_2()
                            .when(this.access_denied, |d| {
                                d.child(
                                    Button::new("access")
                                        .primary()
                                        .icon(gpui_kit::assets::IconName::ShieldCheck)
                                        .label("Install access rule")
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.offer_access(window, cx)
                                        })),
                                )
                            })
                            .child(
                                Button::new("retry")
                                    .outline()
                                    .icon(IconName::RefreshCw)
                                    .label("Retry")
                                    .loading(reading)
                                    .on_click(
                                        cx.listener(|this, _, window, cx| this.reload(window, cx)),
                                    ),
                            ),
                    ),
                ),
        )
        .into_any_element()
}

/// A single-choice segmented control.
fn segmented(
    id: impl Into<gpui_kit::ElementId>,
    labels: impl IntoIterator<Item = impl Into<SharedString>>,
    current: usize,
    disabled: bool,
    on_pick: impl Fn(&mut Workbench, usize, &mut Window, &mut Context<Workbench>) + 'static,
    cx: &mut Context<Workbench>,
) -> impl IntoElement {
    TabBar::new(id)
        .segmented()
        .small()
        .selected_index(current)
        .children(
            labels
                .into_iter()
                .map(|label| Tab::new().label(label).disabled(disabled)),
        )
        .on_click(cx.listener(move |this, ix: &usize, window, cx| {
            if *ix != current {
                on_pick(this, *ix, window, cx);
                cx.notify();
            }
        }))
}

// ---- Lighting -------------------------------------------------------------

fn lighting(this: &mut Workbench, window: &mut Window, cx: &mut Context<Workbench>) -> AnyElement {
    let Some(light) = this.light() else {
        return div().into_any_element();
    };
    let draft = this.draft.clone().expect("draft present");
    let mode = light.mode;
    let off = mode == 0;
    let picture = mode == 13;
    let can_paint = this.can_paint();
    let model = this.model();
    let layout = model.layout();
    let (width, height) = model.layout_size();
    let options = model.options(mode);
    let known = model.supports_effect(mode);
    let side = draft
        .advanced
        .as_ref()
        .and_then(|a| a.side_lighting.as_ref());
    let side_preview = side.map(|l| effects::side_light(l, model));

    // The board plays the drafted effect. Painted pictures are static.
    let moving =
        (effects::moves(&light) && known) || side_preview.as_ref().is_some_and(effects::moves);
    let playing = this.preview.playing() && !cx.reduce_motion();
    if moving && playing && window.is_window_active() {
        window.request_animation_frame();
    }
    let now = this.preview.now();
    let level = f32::from(light.brightness.min(4)) / 4.;
    let rgb = draft.onboard.as_ref().map(|o| o.rgb.clone());
    let glow = |slot: u8| -> Option<gpui_kit::Hsla> {
        if model.knob_cap().is_some_and(|cap| cap.slot == slot) {
            return None;
        }
        if picture {
            let i = usize::from(slot) * 3;
            let color = rgb.as_ref().and_then(|d| d.get(i..i + 3))?;
            return (level > 0. && color.iter().any(|c| *c > 0))
                .then(|| hsla([color[0], color[1], color[2]]).opacity(level));
        }
        let cap = layout.iter().find(|c| c.slot == slot)?;
        effects::led_for(&light, now, cap, &layout, width, height)
    };
    let pick: Option<OnPick> = can_paint.then(|| {
        let entity = cx.entity();
        Rc::new(move |slot: u8, _: &mut Window, cx: &mut App| {
            entity.update(cx, |this, cx| this.paint(slot, cx));
        }) as Rc<dyn Fn(u8, &mut Window, &mut App)>
    });
    let side_glow = side_preview.map(|light| {
        let strip: Vec<_> = (0..32)
            .map(|i| keys::Cap {
                slot: i,
                usage: 0,
                x: f32::from(i),
                y: 0.,
                w: 1.,
                h: 1.,
            })
            .collect();
        strip
            .iter()
            .map(|cap| effects::led_for(&light, now, cap, &strip, 32., 1.))
            .collect()
    });
    let keyboard = board_lit(
        "light",
        model,
        |slot| Look {
            legend: model.slot_legend(slot).into(),
            glow: glow(slot),
            ..Default::default()
        },
        pick,
        side_glow,
        cx,
    );

    let color_disabled =
        off || picture || light.rainbow || model.effect(mode).is_some_and(|e| !e.rgb);
    let mut rows = vec![
        ui::icon_row(
            gpui_kit::assets::IconName::Sparkles,
            "Effect",
            h_flex()
                .gap_2()
                .child(
                    Button::new("preview")
                        .ghost()
                        .small()
                        .icon(if playing {
                            gpui_kit::assets::IconName::Pause
                        } else {
                            gpui_kit::assets::IconName::Play
                        })
                        .tooltip(if playing {
                            "Pause preview"
                        } else {
                            "Play preview"
                        })
                        .disabled(!moving || cx.reduce_motion())
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.preview.toggle();
                            cx.notify();
                        })),
                )
                .child(
                    div().w(px(150.)).child(
                        Select::new(&this.effect)
                            .small()
                            .appearance(false)
                            .menu_width(px(220.))
                            .placeholder("Custom effect")
                            .menu_max_h(px(320.)),
                    ),
                ),
        ),
        ui::icon_row(
            gpui_kit::assets::IconName::Sun,
            "Brightness",
            ui::valued(
                Slider::new(&this.brightness).disabled(off),
                format!("{}", light.brightness),
                cx,
            ),
        ),
        ui::icon_row(
            gpui_kit::assets::IconName::Gauge,
            "Speed",
            ui::valued(
                Slider::new(&this.speed)
                    .disabled(off || picture || model.effect(mode).is_some_and(|e| e.speed == 0)),
                format!("{}", light.speed),
                cx,
            ),
        ),
    ];
    if options.len() > 1 {
        rows.push(ui::icon_row(
            if picture {
                gpui_kit::assets::IconName::Image
            } else {
                gpui_kit::assets::IconName::ArrowLeftRight
            },
            if picture { "Picture" } else { "Direction" },
            segmented(
                "option",
                options
                    .iter()
                    .map(|(_, label)| label.clone())
                    .collect::<Vec<_>>(),
                options
                    .iter()
                    .position(|(value, _)| *value == light.option)
                    .unwrap_or(0),
                !known,
                move |this, i, _, cx| this.edit_light(cx, |l| l.option = options[i].0),
                cx,
            ),
        ));
    }
    if !picture {
        rows.push(ui::icon_row(
            gpui_kit::assets::IconName::Rainbow,
            "Rainbow",
            Switch::new("rainbow")
                .checked(light.rainbow)
                .disabled(off || !known || model.effect(mode).is_some_and(|e| !e.dazzle))
                .on_click(cx.listener(|this, on: &bool, _, cx| {
                    let on = *on;
                    this.edit_light(cx, |l| l.rainbow = on);
                })),
        ));
        rows.push(ui::icon_row(
            gpui_kit::assets::IconName::Palette,
            "Color",
            h_flex().when(color_disabled, |d| d.opacity(0.5)).child(
                ColorPicker::new(&this.color)
                    .small()
                    .accessibility_label("Color"),
            ),
        ));
    }

    let paint_bar = picture.then(|| {
        h_flex()
            .gap_3()
            .child(
                ColorPicker::new(&this.color)
                    .small()
                    .accessibility_label("Paint color"),
            )
            .child(
                Button::new("fill")
                    .outline()
                    .small()
                    .icon(gpui_kit::assets::IconName::PaintBucket)
                    .label("Fill all")
                    .disabled(!can_paint)
                    .on_click(cx.listener(|this, _, _, cx| this.paint_all(cx))),
            )
            .into_any_element()
    });

    let mut children = Vec::new();
    children.extend(paint_bar);
    children.push(keyboard.into_any_element());
    children.push(ui::section("", rows, cx));
    if let Some(side) = side.cloned() {
        let mut side_rows = vec![ui::icon_row(
            gpui_kit::assets::IconName::Lightbulb,
            "Effect",
            segmented(
                "side-effect",
                ["Off", "Static", "Breathing", "Neon", "Wave", "Snake"],
                usize::from(side.mode).min(5),
                false,
                move |this, i, _, cx| {
                    this.edit(cx, |d| {
                        if let Some(side) =
                            d.advanced.as_mut().and_then(|a| a.side_lighting.as_mut())
                        {
                            side.mode = i as u8;
                            side.flags = if i == 3 { 8 } else { model.normal };
                        }
                    })
                },
                cx,
            ),
        )];
        for (index, label, value) in [
            (0, "Brightness", side.brightness),
            (1, "Speed", side.wire_speed),
        ] {
            side_rows.push(ui::icon_row(
                gpui_kit::assets::IconName::Sun,
                label,
                segmented(
                    SharedString::from(format!("side-value-{index}")),
                    (0..=4).map(|n| n.to_string()),
                    usize::from(value).min(4),
                    false,
                    move |this, v, _, cx| {
                        this.edit(cx, |d| {
                            if let Some(side) =
                                d.advanced.as_mut().and_then(|a| a.side_lighting.as_mut())
                            {
                                if index == 0 {
                                    side.brightness = v as u8;
                                } else {
                                    side.wire_speed = v as u8;
                                }
                            }
                        })
                    },
                    cx,
                ),
            ));
        }
        side_rows.push(ui::icon_row(
            gpui_kit::assets::IconName::Rainbow,
            "Rainbow",
            Switch::new("side-rainbow")
                .checked(side.flags & 15 == model.dazzle)
                .disabled(side.mode == 0 || side.mode == 3)
                .on_click(cx.listener(move |this, on: &bool, _, cx| {
                    this.edit(cx, |d| {
                        if let Some(side) =
                            d.advanced.as_mut().and_then(|a| a.side_lighting.as_mut())
                        {
                            side.flags =
                                (side.flags & 0xf0) | if *on { model.dazzle } else { model.normal };
                        }
                    })
                })),
        ));
        side_rows.push(ui::icon_row(
            gpui_kit::assets::IconName::Palette,
            "Color",
            ColorPicker::new(&this.side_color)
                .small()
                .accessibility_label("Side lighting color"),
        ));
        if matches!(side.mode, 4 | 5) {
            side_rows.push(ui::icon_row(
                gpui_kit::assets::IconName::ArrowLeftRight,
                "Direction",
                segmented(
                    "side-direction",
                    ["Forward", "Reverse"],
                    usize::from(side.flags >> 4).min(1),
                    false,
                    |this, i, _, cx| {
                        this.edit(cx, |d| {
                            if let Some(side) =
                                d.advanced.as_mut().and_then(|a| a.side_lighting.as_mut())
                            {
                                side.flags = (side.flags & 15) | ((i as u8) << 4);
                            }
                        })
                    },
                    cx,
                ),
            ));
        }
        children.push(ui::section("Side lighting", side_rows, cx));
    }
    ui::page("lighting", children).into_any_element()
}

// ---- Keys -----------------------------------------------------------------

fn keys_page(this: &mut Workbench, _: &mut Window, cx: &mut Context<Workbench>) -> AnyElement {
    let draft = this.draft.clone().expect("draft present");
    let baseline = this.baseline.clone();
    let fn_layer = this.fn_layer;
    let selected = this.selected;
    let model = this.model();
    let knob_slots = if model.knob_cap().is_some() {
        model.knob_slots()
    } else {
        Vec::new()
    };
    let knob = |s: u8| knob_slots.contains(&s);

    let entity = cx.entity();
    let keyboard = board(
        "keys",
        model,
        |slot| {
            let value = this.binding(&draft, slot);
            let base = baseline.as_ref().map(|b| this.binding(b, slot));
            let remapped = value != model.default_binding(fn_layer, this.layer, slot);
            let changed = base.is_some_and(|b| b != value);
            Look {
                legend: model.slot_legend(slot).into(),
                action: (remapped || changed).then(|| keys::binding_label(value).into()),
                selected: selected.is_some_and(|s| s == slot || (knob(s) && knob(slot))),
                ..Default::default()
            }
        },
        Some(Rc::new(move |slot, window, cx| {
            entity.update(cx, |this, cx| {
                let slot = if this.selected == Some(slot) {
                    None
                } else {
                    Some(slot)
                };
                this.select_key(slot, window, cx)
            })
        })),
        cx,
    );

    let toolbar = h_flex()
        .gap_3()
        .child(segmented(
            "layer",
            (1..=if fn_layer {
                model.fn_maps()
            } else {
                model.layers
            })
                .map(|n| {
                    if fn_layer {
                        let (system, index) = model.fn_address(n - 1).expect("Fn map");
                        format!(
                            "{} Fn {}",
                            if system == 0 { "Win" } else { "Mac" },
                            index + 1
                        )
                    } else {
                        format!("Layer {n}")
                    }
                }),
            usize::from(if model.magnetic && !fn_layer {
                this.layer / 4
            } else {
                this.layer
            }),
            false,
            |this, i, window, cx| {
                this.layer = if this.model().magnetic && !this.fn_layer {
                    i as u8 * 4 + this.layer % 4
                } else {
                    i as u8
                };
                this.sync_target(window, cx);
            },
            cx,
        ))
        .child(segmented(
            "map",
            ["Base", "Fn"],
            usize::from(this.fn_layer),
            false,
            |this, i, window, cx| {
                this.fn_layer = i == 1;
                this.layer = this.layer.min(
                    if this.fn_layer {
                        this.model().fn_maps()
                    } else {
                        this.model().base_maps()
                    } - 1,
                );
                this.sync_target(window, cx);
            },
            cx,
        ))
        .child(div().flex_1())
        .child(
            Button::new("reset-layer")
                .ghost()
                .small()
                .icon(gpui_kit::assets::IconName::RotateCcw)
                .label("Reset layer")
                .on_click(cx.listener(|this, _, window, cx| {
                    this.reset_layer(cx);
                    this.sync_target(window, cx);
                })),
        );

    let outputs = if model.magnetic && !fn_layer {
        segmented(
            "binding-output",
            ["Primary", "Output 2", "Output 3", "Output 4"],
            usize::from(this.layer % 4),
            false,
            |this, i, window, cx| {
                this.layer = this.layer / 4 * 4 + i as u8;
                this.sync_target(window, cx);
            },
            cx,
        )
        .into_any_element()
    } else {
        div().into_any_element()
    };
    let editor = match selected {
        None => ui::placeholder("No key selected", 120., cx).into_any_element(),
        Some(slot) => {
            let value = this.binding(&draft, slot);
            let mut rows = Vec::new();
            if knob(slot) {
                let current = knob_slots.iter().position(|s| *s == slot).unwrap_or(0);
                rows.push(ui::icon_row(
                    gpui_kit::assets::IconName::CircleDot,
                    "Input",
                    segmented(
                        "knob",
                        knob_slots
                            .iter()
                            .map(|slot| model.slot_legend(*slot))
                            .collect::<Vec<_>>(),
                        current,
                        false,
                        move |this, i, window, cx| {
                            let slot = knob_slots[i];
                            this.select_key(Some(slot), window, cx);
                        },
                        cx,
                    ),
                ));
            }
            rows.push(ui::icon_row(
                gpui_kit::assets::IconName::Zap,
                "Action",
                div().w(px(200.)).child(
                    Select::new(&this.target)
                        .disabled(!model.is_remappable(slot))
                        .small()
                        .appearance(false)
                        .menu_width(px(280.))
                        .placeholder(SharedString::from(keys::binding_label(value)))
                        .search_placeholder("Search")
                        .menu_max_h(px(360.)),
                ),
            ));
            let combo = keys::split_combo(value);
            if combo.is_some() {
                rows.push(ui::icon_row(
                    gpui_kit::assets::IconName::Command,
                    "Modifiers",
                    h_flex()
                        .gap_1()
                        .children(MODIFIERS.iter().map(|(usage, name)| {
                            let on = combo.as_ref().is_some_and(|(m, _)| m.contains(usage));
                            let usage = *usage;
                            Toggle::new(SharedString::from(*name))
                                .label(*name)
                                .small()
                                .outline()
                                .checked(on)
                                .disabled(!on && combo.as_ref().is_some_and(|(m, _)| m.len() >= 2))
                                .on_click(cx.listener(move |this, on: &bool, window, cx| {
                                    let Some(d) = &this.draft else { return };
                                    let Some((mut mods, key)) =
                                        keys::split_combo(this.binding(d, slot))
                                    else {
                                        return;
                                    };
                                    if *on {
                                        mods.push(usage);
                                    } else {
                                        mods.retain(|m| *m != usage);
                                    }
                                    mods.sort();
                                    if let Some(v) = keys::combo(&mods, key) {
                                        this.set_binding(slot, v, cx);
                                        this.sync_target(window, cx);
                                    }
                                }))
                        })),
                ));
            }
            ui::section(model.slot_legend(slot), rows, cx)
        }
    };

    ui::page(
        "keys",
        vec![
            toolbar.into_any_element(),
            outputs,
            keyboard.into_any_element(),
            editor,
        ],
    )
    .into_any_element()
}

// ---- Macros ---------------------------------------------------------------

fn macros(this: &mut Workbench, _: &mut Window, cx: &mut Context<Workbench>) -> AnyElement {
    let theme = cx.theme().clone();
    let current = this.macro_slot;
    let recording = this.recording.is_some();
    let editable = this.editable();

    let list = v_flex()
        .id("macro-list")
        .w(px(208.))
        .flex_shrink_0()
        .h_full()
        .border_r_1()
        .border_color(theme.border)
        .p_2()
        .gap_px()
        .overflow_y_scrollbar()
        .children((0..50u8).map(|slot| {
            let count = this
                .macro_image(slot)
                .and_then(|d| Macro::decode(d).ok())
                .map(|m| m.events.len());
            let active = slot == current;
            h_flex()
                .id(("macro", slot as usize))
                .h(px(32.))
                .px_2()
                .rounded(theme.radius)
                .cursor_pointer()
                .text_sm()
                .when(active, |d| {
                    d.bg(theme.accent).font_weight(gpui_kit::FontWeight::MEDIUM)
                })
                .when(!active, |d| d.hover(|s| s.bg(theme.accent.opacity(0.6))))
                .child(div().flex_1().child(format!("Macro {}", slot + 1)))
                .when_some(count, |d, n| {
                    d.child(
                        div()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .child(n.to_string()),
                    )
                })
                .on_click(cx.listener(move |this, _, _, cx| this.open_macro(slot, cx)))
        }));

    let header = h_flex()
        .gap_2()
        .child(
            div()
                .text_lg()
                .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                .flex_1()
                .child(format!("Macro {}", current + 1)),
        )
        .child(
            Button::new("clear-macro")
                .ghost()
                .small()
                .icon(gpui_kit::assets::IconName::Trash)
                .label("Clear")
                .disabled(!editable || recording || this.macro_events.is_empty())
                .on_click(cx.listener(|this, _, _, cx| {
                    this.macro_events.clear();
                    this.store_macro(cx);
                })),
        )
        .child(
            Button::new("record")
                .small()
                .when(recording, |b| b.danger())
                .when(!recording, |b| b.outline())
                .icon(if recording {
                    Icon::new(gpui_kit::assets::IconName::Square)
                } else {
                    Icon::new(gpui_kit::assets::IconName::CircleDot)
                })
                .label(if recording { "Stop" } else { "Record" })
                .disabled(!editable)
                .on_click(cx.listener(|this, _, window, cx| this.toggle_recording(window, cx))),
        );

    let repeat = this.macro_repeat;
    let stepper = h_flex()
        .gap_1()
        .child(
            Button::new("repeat-down")
                .outline()
                .small()
                .icon(IconName::Minus)
                .disabled(repeat <= 1 || recording)
                .on_click(cx.listener(|this, _, _, cx| {
                    this.macro_repeat = this.macro_repeat.saturating_sub(1).max(1);
                    this.store_macro(cx);
                })),
        )
        .child(
            div()
                .w(px(48.))
                .text_sm()
                .text_center()
                .child(repeat.to_string()),
        )
        .child(
            Button::new("repeat-up")
                .outline()
                .small()
                .icon(IconName::Plus)
                .disabled(repeat == u16::MAX || recording)
                .on_click(cx.listener(|this, _, _, cx| {
                    this.macro_repeat = this.macro_repeat.saturating_add(1);
                    this.store_macro(cx);
                })),
        );

    let events = v_flex()
        .id("events")
        .track_focus(&this.record_focus)
        .flex_1()
        .min_h(px(240.))
        .rounded(theme.radius_lg)
        .border_1()
        .border_color(if recording { theme.ring } else { theme.border })
        .when(recording, |d| d.border_2())
        .overflow_y_scrollbar()
        .on_key_down(cx.listener(|this, ev: &KeyDownEvent, _, cx| {
            if this.recording.is_none() || ev.is_held {
                return;
            }
            if let Some(u) = keys::usage_from_key_name(&ev.keystroke.key) {
                this.record(u, true, cx);
                cx.stop_propagation();
            }
        }))
        .on_key_up(cx.listener(|this, ev: &KeyUpEvent, _, cx| {
            if let Some(u) = keys::usage_from_key_name(&ev.keystroke.key) {
                this.record(u, false, cx);
            }
        }))
        .on_modifiers_changed(cx.listener(|this, ev: &ModifiersChangedEvent, _, cx| {
            let Some(rec) = &this.recording else { return };
            let m = &ev.modifiers;
            let now: Vec<u8> = [
                (m.control, 224),
                (m.shift, 225),
                (m.alt, 226),
                (m.platform, 227),
            ]
            .into_iter()
            .filter(|(on, _)| *on)
            .map(|(_, u)| u)
            .collect();
            let before = rec.modifiers.clone();
            for u in &now {
                if !before.contains(u) {
                    this.record(*u, true, cx);
                }
            }
            for u in &before {
                if !now.contains(u) {
                    this.record(*u, false, cx);
                }
            }
            if let Some(rec) = &mut this.recording {
                rec.modifiers = now;
            }
        }))
        .when(recording, |d| {
            let button = |b: MouseButton| match b {
                MouseButton::Left => Some(0xf0),
                MouseButton::Right => Some(0xf1),
                MouseButton::Middle => Some(0xf2),
                MouseButton::Navigate(gpui_kit::NavigationDirection::Back) => Some(0xf3),
                MouseButton::Navigate(gpui_kit::NavigationDirection::Forward) => Some(0xf4),
            };
            let mut d = d;
            for b in [
                MouseButton::Left,
                MouseButton::Right,
                MouseButton::Middle,
                MouseButton::Navigate(gpui_kit::NavigationDirection::Back),
                MouseButton::Navigate(gpui_kit::NavigationDirection::Forward),
            ] {
                let code = button(b).expect("mapped");
                d = d
                    .on_mouse_down(
                        b,
                        cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                            this.record(code, true, cx)
                        }),
                    )
                    .on_mouse_up(
                        b,
                        cx.listener(move |this, _: &MouseUpEvent, _, cx| {
                            this.record(code, false, cx)
                        }),
                    );
            }
            d
        })
        .when(this.macro_events.is_empty(), |d| {
            d.child(
                div()
                    .flex_1()
                    .flex()
                    .items_center()
                    .justify_center()
                    .min_h(px(240.))
                    .text_sm()
                    .text_color(theme.muted_foreground)
                    .child(if recording { "Recording" } else { "Empty" }),
            )
        })
        .children(this.macro_events.iter().enumerate().map(|(i, ev)| {
            let (down, delay) = match ev {
                MacroEvent::Keyboard { down, delay_ms, .. }
                | MacroEvent::MouseButton { down, delay_ms, .. } => (Some(*down), *delay_ms),
                MacroEvent::Move { delay_ms, .. } => (None, *delay_ms),
            };
            let adjust = move |delta: i32| {
                move |this: &mut Workbench,
                      _: &gpui_kit::ClickEvent,
                      _: &mut Window,
                      cx: &mut Context<Workbench>| {
                    if let Some(
                        MacroEvent::Keyboard { delay_ms, .. }
                        | MacroEvent::MouseButton { delay_ms, .. }
                        | MacroEvent::Move { delay_ms, .. },
                    ) = this.macro_events.get_mut(i)
                    {
                        *delay_ms =
                            (i32::from(*delay_ms) + delta).clamp(1, i32::from(u16::MAX)) as u16;
                    }
                    this.store_macro(cx);
                }
            };
            h_flex()
                .h(px(40.))
                .px_3()
                .gap_3()
                .border_b_1()
                .border_color(theme.border)
                .child(
                    div()
                        .w(px(28.))
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child((i + 1).to_string()),
                )
                .child(
                    match down {
                        Some(true) => Icon::new(IconName::ArrowDown),
                        Some(false) => Icon::new(IconName::ArrowUp),
                        None => Icon::new(gpui_kit::assets::IconName::Move),
                    }
                    .small()
                    .text_color(theme.muted_foreground),
                )
                .child(div().flex_1().text_sm().child(keys::macro_event_label(ev)))
                .child(
                    h_flex()
                        .gap_1()
                        .child(
                            Button::new(("delay-down", i))
                                .ghost()
                                .xsmall()
                                .icon(IconName::Minus)
                                .disabled(recording || delay <= 1)
                                .on_click(cx.listener(adjust(-10))),
                        )
                        .child(
                            div()
                                .w(px(64.))
                                .text_sm()
                                .text_right()
                                .text_color(theme.muted_foreground)
                                .child(format!("{delay} ms")),
                        )
                        .child(
                            Button::new(("delay-up", i))
                                .ghost()
                                .xsmall()
                                .icon(IconName::Plus)
                                .disabled(recording)
                                .on_click(cx.listener(adjust(10))),
                        ),
                )
                .child(
                    Button::new(("remove", i))
                        .ghost()
                        .xsmall()
                        .icon(IconName::Close)
                        .tooltip("Remove")
                        .disabled(recording)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.macro_events.remove(i);
                            this.store_macro(cx);
                        })),
                )
        }));

    let editor = v_flex()
        .flex_1()
        .min_w_0()
        .h_full()
        .p_6()
        .gap_4()
        .child(header)
        .child(
            h_flex()
                .gap_4()
                .child(
                    div()
                        .text_sm()
                        .font_weight(gpui_kit::FontWeight::MEDIUM)
                        .child("Repeat"),
                )
                .child(stepper),
        )
        .when_some(this.macro_error.clone(), |d, e| {
            d.child(div().text_sm().text_color(theme.danger).child(e))
        })
        .child(events);

    h_flex()
        .size_full()
        .items_start()
        .child(list)
        .child(editor)
        .into_any_element()
}

// ---- Performance ----------------------------------------------------------

fn minutes(seconds: u16) -> String {
    if seconds.is_multiple_of(60) {
        format!("{} min", seconds / 60)
    } else {
        format!("{seconds} s")
    }
}

fn performance(this: &mut Workbench, cx: &mut Context<Workbench>) -> AnyElement {
    let Some(o) = this.draft.as_ref().and_then(|d| d.onboard.clone()) else {
        return div().into_any_element();
    };
    let input = ui::section(
        "Input",
        vec![ui::icon_row(
            gpui_kit::assets::IconName::Timer,
            "Debounce",
            ui::valued(
                Slider::new(&this.debounce),
                format!("{} ms", o.debounce),
                cx,
            ),
        )],
        cx,
    );
    use gpui_kit::assets::IconName as I;
    let t = &o.sleep;
    let sleep = ui::section(
        "Wireless sleep",
        [
            (I::Bluetooth, "Bluetooth", t.time_bt),
            (I::Radio, "2.4 GHz", t.time_24),
            (I::Bluetooth, "Bluetooth deep sleep", t.deep_time_bt),
            (I::Radio, "2.4 GHz deep sleep", t.deep_time_24),
        ]
        .into_iter()
        .zip(this.sleep.iter())
        .enumerate()
        .filter(|(i, _)| this.model().sleep_ranges[*i].is_some())
        .map(|(_, ((icon, label, value), state))| {
            ui::icon_row(
                icon,
                label,
                ui::valued(Slider::new(state), minutes(value), cx),
            )
        })
        .collect(),
        cx,
    );
    let mut parts = Vec::new();
    if this.model().debounce {
        parts.push(input);
    }
    if let Some(hz) = this
        .draft
        .as_ref()
        .and_then(|d| d.advanced.as_ref())
        .and_then(|a| a.report_rate)
    {
        let rates: Vec<u16> = [125, 250, 500, 1000, 2000, 4000, 8000]
            .into_iter()
            .filter(|r| *r <= this.model().max_report_rate)
            .collect();
        parts.push(ui::section(
            "Polling",
            vec![ui::icon_row(
                I::Gauge,
                "Report rate",
                segmented(
                    "report-rate",
                    rates.iter().map(|r| format!("{r} Hz")).collect::<Vec<_>>(),
                    rates.iter().position(|r| *r == hz).unwrap_or(3),
                    false,
                    move |this, i, _, cx| {
                        this.edit(cx, |d| {
                            if let Some(a) = &mut d.advanced {
                                a.report_rate = Some(rates[i]);
                            }
                        })
                    },
                    cx,
                ),
            )],
            cx,
        ));
    }
    if let Some(a) = this.draft.as_ref().and_then(|d| d.advanced.clone()) {
        if let Some(value) = a.magnetic_mode {
            parts.push(ui::section(
                "Actuation preset",
                vec![
                    segmented(
                        "magnetic-preset",
                        ["Comfort", "Sensitive", "Gaming", "Custom"],
                        usize::from(value),
                        false,
                        |this, i, _, cx| {
                            this.edit(cx, |d| {
                                if let Some(a) = &mut d.advanced {
                                    a.magnetic_mode = Some(i as u8);
                                }
                            })
                        },
                        cx,
                    )
                    .into_any_element(),
                ],
                cx,
            ));
        }
        if let Some(value) = a.output_mode {
            parts.push(ui::section(
                "Output",
                vec![
                    segmented(
                        "output-mode",
                        ["Keyboard", "Gamepad"],
                        usize::from(value),
                        false,
                        |this, i, _, cx| {
                            this.edit(cx, |d| {
                                if let Some(a) = &mut d.advanced {
                                    a.output_mode = Some(i as u8);
                                }
                            })
                        },
                        cx,
                    )
                    .into_any_element(),
                ],
                cx,
            ));
        }
    }
    if this.model().sleep {
        parts.push(sleep);
    }
    ui::page("performance", parts).into_any_element()
}

// ---- System ---------------------------------------------------------------

fn system(this: &mut Workbench, cx: &mut Context<Workbench>) -> AnyElement {
    let Some(d) = this.draft.clone() else {
        return div().into_any_element();
    };
    let Some(o) = d.onboard.clone() else {
        return div().into_any_element();
    };
    let layer = ui::section(
        "Profile",
        vec![ui::icon_row(
            gpui_kit::assets::IconName::Layers,
            "Active layer",
            segmented(
                "active-layer",
                (1..=this.model().layers).map(|n| n.to_string()),
                usize::from(d.active_layer),
                this.busy.is_some(),
                |this, i, _, cx| this.edit(cx, |d| d.active_layer = i as u8),
                cx,
            ),
        )],
        cx,
    );
    use gpui_kit::assets::IconName as I;
    let bit = |icon: I, label: &'static str, id: &'static str, byte: usize, mask: u8| {
        let on = o.options[byte] & mask != 0;
        ui::icon_row(
            icon,
            label,
            Switch::new(id)
                .disabled(this.busy.is_some())
                .checked(on)
                .on_click(cx.listener(move |this, on: &bool, _, cx| {
                    let on = *on;
                    this.edit_onboard(cx, |o| {
                        if on {
                            o.options[byte] |= mask;
                        } else {
                            o.options[byte] &= !mask;
                        }
                    });
                })),
        )
    };
    let legacy_keyboard = ui::section(
        "Keyboard",
        vec![
            bit(I::Lock, "Windows key lock", "win-lock", 0, 1),
            bit(I::Move, "Swap WASD and arrows", "wasd", 0, 8),
            bit(I::KeyboardOff, "Keyboard lock", "kb-lock", 0, 128),
            bit(I::Leaf, "Power saving", "power-save", 2, 1),
            ui::icon_row(
                gpui_kit::assets::IconName::Monitor,
                "Automatic OS detection",
                Switch::new("auto-os")
                    .disabled(this.busy.is_some())
                    .checked(o.auto_os)
                    .on_click(cx.listener(|this, on: &bool, _, cx| {
                        let on = *on;
                        this.edit_onboard(cx, |o| o.auto_os = on);
                    })),
            ),
        ],
        cx,
    );
    let keyboard = if let Some(bytes) = d.advanced.as_ref().and_then(|a| a.options) {
        let modern_switch = |label: &'static str, id: &'static str, index: usize| {
            ui::icon_row(
                I::Keyboard,
                label,
                Switch::new(id)
                    .checked(bytes[index] != 0)
                    .on_click(cx.listener(move |this, on: &bool, _, cx| {
                        this.edit(cx, |d| {
                            if let Some(options) =
                                d.advanced.as_mut().and_then(|a| a.options.as_mut())
                            {
                                options[index] = u8::from(*on);
                            }
                        })
                    })),
            )
        };
        ui::section(
            "Keyboard",
            vec![
                modern_switch("Prevent accidental presses", "anti-mistouch", 1),
                modern_switch("Swap WASD and arrows", "modern-wasd", 3),
                ui::icon_row(
                    I::Monitor,
                    "Automatic OS detection",
                    Switch::new("modern-auto-os")
                        .checked(o.auto_os)
                        .on_click(cx.listener(|this, on: &bool, _, cx| {
                            this.edit_onboard(cx, |o| o.auto_os = *on)
                        })),
                ),
                ui::icon_row(
                    I::Gauge,
                    "Rapid trigger stability",
                    segmented(
                        "rt-stability",
                        ["Off", "25", "50", "75", "100", "125"],
                        usize::from(bytes[2]).min(5),
                        false,
                        |this, i, _, cx| {
                            this.edit(cx, |d| {
                                if let Some(options) =
                                    d.advanced.as_mut().and_then(|a| a.options.as_mut())
                                {
                                    options[2] = i as u8;
                                }
                            })
                        },
                        cx,
                    ),
                ),
                ui::icon_row(
                    I::Layers,
                    "Fn map",
                    segmented(
                        "fn-index",
                        (1..=this.model().fn_counts.get("win").copied().unwrap_or(1))
                            .map(|n| n.to_string()),
                        usize::from(bytes[0]),
                        false,
                        |this, i, _, cx| {
                            this.edit(cx, |d| {
                                if let Some(options) =
                                    d.advanced.as_mut().and_then(|a| a.options.as_mut())
                                {
                                    options[0] = i as u8;
                                }
                            })
                        },
                        cx,
                    ),
                ),
            ],
            cx,
        )
    } else {
        legacy_keyboard
    };
    let installed = this.access == monsgeek_linux::access::Status::Installed;
    let access = ui::section(
        "Access",
        vec![ui::icon_row(
            I::ShieldCheck,
            "Device access rule",
            if installed {
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child("Installed")
                    .into_any_element()
            } else {
                Button::new("access-install")
                    .outline()
                    .small()
                    .label(if this.access == monsgeek_linux::access::Status::Outdated {
                        "Update"
                    } else {
                        "Install"
                    })
                    .loading(this.access_busy)
                    .on_click(cx.listener(|this, _, window, cx| this.offer_access(window, cx)))
                    .into_any_element()
            },
        )],
        cx,
    );
    let firmware = ui::section(
        "Firmware (experimental)",
        vec![
            v_flex()
                .py_4()
                .gap_3()
                .child(div().text_sm().child(monsgeek_linux::firmware::WARNING))
                .when_some(this.firmware_image.as_ref(), |d, (path, image)| {
                    d.child(div().text_sm().child(format!(
                        "{} ({} bytes)",
                        path.file_name().unwrap_or_default().to_string_lossy(),
                        image.info().file_bytes
                    )))
                    .child(div().text_sm().child(
                        match &image.info().known_release {
                            Some(release) => format!(
                                "Verified vendor image {} for M1W ID{}",
                                release.version, release.device_id
                            ),
                            None => "Unrecognized image".into(),
                        },
                    ))
                })
                .child(
                    h_flex()
                        .gap_2()
                        .child(
                            Button::new("firmware-select")
                                .outline()
                                .small()
                                .label("Select file")
                                .disabled(this.busy.is_some())
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.choose_firmware(window, cx)
                                })),
                        )
                        .child(
                            Button::new("firmware-flash")
                                .outline()
                                .small()
                                .label("Flash firmware")
                                .disabled(!this.can_flash_firmware())
                                .loading(this.busy == Some(crate::app::Busy::Flashing))
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.confirm_firmware(window, cx)
                                })),
                        ),
                )
                .when(this.busy == Some(crate::app::Busy::Flashing), |d| {
                    d.child(
                        div().text_sm().child(
                            this.firmware_status
                                .lock()
                                .unwrap_or_else(|e| e.into_inner())
                                .clone(),
                        ),
                    )
                })
                .when(
                    this.busy.is_none()
                        && (this.transport != Some(monsgeek_linux::transport::Kind::Usb)
                            || !this.pending().is_ok_and(|changes| changes.is_empty())),
                    |d| {
                        d.child(
                            div()
                                .text_sm()
                                .text_color(cx.theme().muted_foreground)
                                .child("Reload over USB and apply or discard pending edits first."),
                        )
                    },
                )
                .into_any_element(),
        ],
        cx,
    );
    ui::page("system", vec![layer, keyboard, access, firmware]).into_any_element()
}

// ---- Backups --------------------------------------------------------------

fn ago(t: SystemTime) -> String {
    let s = SystemTime::now()
        .duration_since(t)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    match s {
        0..=59 => "Just now".into(),
        60..=3599 => format!("{} min ago", s / 60),
        3600..=86_399 => format!("{} h ago", s / 3600),
        _ => format!("{} d ago", s / 86_400),
    }
}

fn backups(this: &mut Workbench, cx: &mut Context<Workbench>) -> AnyElement {
    let theme = cx.theme().clone();
    let actions = h_flex()
        .gap_2()
        .child(
            Button::new("export")
                .outline()
                .small()
                .icon(gpui_kit::assets::IconName::Download)
                .label("Export")
                .disabled(this.previewing)
                .on_click(cx.listener(|this, _, window, cx| this.export(window, cx))),
        )
        .child(
            Button::new("open")
                .outline()
                .small()
                .icon(gpui_kit::assets::IconName::Upload)
                .label("Open")
                .disabled(!this.editable())
                .on_click(cx.listener(|this, _, window, cx| this.open_file(window, cx))),
        );

    let rows: Vec<AnyElement> = this
        .history
        .iter()
        .enumerate()
        .take(100)
        .map(|(i, r)| {
            let snapshot = r.snapshot.clone();
            h_flex()
                .min_h(px(52.))
                .gap_4()
                .child(
                    div()
                        .flex_1()
                        .text_sm()
                        .font_weight(gpui_kit::FontWeight::MEDIUM)
                        .child(ago(r.saved)),
                )
                .child(
                    div()
                        .w(px(120.))
                        .text_sm()
                        .text_color(theme.muted_foreground)
                        .child(if r.changes == 1 {
                            "1 change".to_string()
                        } else {
                            format!("{} changes", r.changes)
                        }),
                )
                .child(
                    Button::new(("restore", i))
                        .ghost()
                        .small()
                        .label("Restore")
                        .disabled(!this.editable())
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.restore(snapshot.clone(), window, cx)
                        })),
                )
                .into_any_element()
        })
        .collect();

    let history = if rows.is_empty() {
        v_flex()
            .gap_3()
            .child(ui::heading("History", cx))
            .child(ui::placeholder("No saved states", 96., cx))
            .into_any_element()
    } else {
        ui::section("History", rows, cx)
    };

    ui::page(
        "backups",
        vec![
            ui::heading_row("Snapshot", actions, cx),
            crate::features::picture_controls(this, cx),
            history,
        ],
    )
    .into_any_element()
}

// ---- Logs -----------------------------------------------------------------

fn logs(this: &mut Workbench, cx: &mut Context<Workbench>) -> AnyElement {
    use crate::log::Level;
    let theme = cx.theme().clone();
    let errors = this.logs.iter().filter(|e| e.level == Level::Error).count();
    let today = chrono::Local::now().date_naive();
    let shown: Vec<_> = this
        .logs
        .iter()
        .rev()
        .filter(|e| !this.errors_only || e.level == Level::Error)
        .take(300)
        .cloned()
        .collect();

    let toolbar = h_flex()
        .gap_2()
        .child(segmented(
            "log-filter",
            [
                "All".to_string(),
                if errors > 0 {
                    format!("Errors {errors}")
                } else {
                    "Errors".into()
                },
            ],
            usize::from(this.errors_only),
            false,
            |this, ix, _, _| this.errors_only = ix == 1,
            cx,
        ))
        .child(div().flex_1())
        .child(
            Button::new("diagnostics-export")
                .outline()
                .small()
                .icon(gpui_kit::assets::IconName::Download)
                .label("Export diagnostics")
                .on_click(cx.listener(|this, _, window, cx| this.export_diagnostics(window, cx))),
        )
        .child(
            Button::new("copy-logs")
                .outline()
                .small()
                .icon(IconName::Copy)
                .label("Copy")
                .disabled(shown.is_empty())
                .on_click(cx.listener(|this, _, window, cx| this.copy_logs(window, cx))),
        )
        .child(
            Button::new("clear-logs")
                .ghost()
                .small()
                .icon(gpui_kit::assets::IconName::Trash)
                .label("Clear")
                .disabled(this.logs.is_empty())
                .on_click(cx.listener(|this, _, window, cx| this.clear_logs(window, cx))),
        );

    let rows: Vec<AnyElement> = shown
        .iter()
        .map(|e| {
            let error = e.level == Level::Error;
            let time = e.local_time();
            let stamp = if time.date_naive() == today {
                time.format("%H:%M:%S").to_string()
            } else {
                time.format("%b %-d %H:%M").to_string()
            };
            h_flex()
                .items_start()
                .py_3()
                .gap_3()
                .child(
                    div()
                        .pt(px(2.))
                        .text_color(if error {
                            theme.danger
                        } else {
                            theme.muted_foreground
                        })
                        .child(
                            Icon::new(if error {
                                IconName::CircleX
                            } else {
                                IconName::CircleCheck
                            })
                            .small(),
                        ),
                )
                .child(
                    v_flex()
                        .flex_1()
                        .min_w_0()
                        .gap_1()
                        .child(
                            div()
                                .text_sm()
                                .font_weight(gpui_kit::FontWeight::MEDIUM)
                                .when(error, |d| d.text_color(theme.danger))
                                .child(e.message.clone()),
                        )
                        .children(e.detail.iter().map(|line| {
                            div()
                                .text_xs()
                                .font_family(theme.mono_font_family.clone())
                                .text_color(theme.muted_foreground)
                                .child(line.clone())
                        })),
                )
                .child(
                    div()
                        .flex_shrink_0()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child(stamp),
                )
                .into_any_element()
        })
        .collect();

    let list = if rows.is_empty() {
        ui::placeholder(
            if this.errors_only {
                "No errors"
            } else {
                "No entries"
            },
            96.,
            cx,
        )
    } else {
        ui::section("", rows, cx)
    };

    ui::page("logs", vec![toolbar.into_any_element(), list]).into_any_element()
}
