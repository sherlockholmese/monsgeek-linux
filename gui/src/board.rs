//! Model-specific keyboards: an aluminum case, a recessed plate, sculpted keycaps at
//! their true positions and the knob, scaled to the width available. Pages
//! decide what each cap shows; the board owns geometry and materials.
use std::rc::Rc;

use gpui_kit::component::{ActiveTheme, v_flex};
use gpui_kit::{
    Animation, AnimationExt, App, BoxShadow, ClickEvent, FocusHandle, Hsla, InteractiveElement,
    IntoElement, MouseButton, ParentElement, Pixels, RenderOnce, SharedString,
    StatefulInteractiveElement, Styled, Subscription, Window, div, linear_color_stop,
    linear_gradient, prelude::FluentBuilder, px, relative,
};
use monsgeek_linux::models::Model;

#[derive(Default, Clone)]
pub struct Look {
    pub legend: SharedString,
    /// Secondary line, such as a remapped action.
    pub action: Option<SharedString>,
    /// Solid keycap color, for states such as a tested key.
    pub fill: Option<Hsla>,
    /// LED under the key. Alpha carries intensity.
    pub glow: Option<Hsla>,
    pub selected: bool,
    /// Drawn held down.
    pub pressed: bool,
}

pub type OnPick = Rc<dyn Fn(u8, &mut Window, &mut App)>;

struct ContactState {
    focus: FocusHandle,
    held: bool,
    release: u64,
    _subscriptions: Vec<Subscription>,
}

/// One contact surface for pointer and keyboard. Native click synthesis owns
/// activation; this state only supplies immediate, geometry-stable feedback.
#[derive(IntoElement)]
struct Contact {
    id: SharedString,
    label: String,
    slot: u8,
    radius: Pixels,
    pick: OnPick,
}

impl RenderOnce for Contact {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let state = window.use_keyed_state(self.id.clone(), cx, |window, cx| {
            let focus = cx.focus_handle();
            let blur = cx.on_blur(&focus, window, |state: &mut ContactState, _, cx| {
                state.held = false;
                state.release = 0;
                cx.notify();
            });
            let deactivate =
                cx.observe_window_activation(window, |state: &mut ContactState, window, cx| {
                    if !window.is_window_active() {
                        state.held = false;
                        state.release = 0;
                        cx.notify();
                    }
                });
            ContactState {
                focus,
                held: false,
                release: 0,
                _subscriptions: vec![blur, deactivate],
            }
        });
        let focus = state.read(cx).focus.clone();
        let held = state.read(cx).held && focus.is_focused(window) && window.is_window_active();
        let release = state.clone();
        let pointer_down = state.clone();
        let pointer_up = state.clone();
        let contact = cx.theme().foreground.opacity(0.12);
        let ring = cx.theme().ring;
        let material = div().absolute().inset_0().rounded(self.radius).bg(contact);
        let overlay = if held {
            Some(material.into_any_element())
        } else if state.read(cx).release > 0 {
            Some(
                material
                    .with_animation(
                        (self.id.clone(), state.read(cx).release as usize),
                        Animation::new(crate::ui::PRESS_RELEASE)
                            .with_easing(gpui_kit::ease_out_quint()),
                        |d, progress| d.opacity(1. - progress),
                    )
                    .into_any_element(),
            )
        } else {
            None
        };
        let element = div()
            .id(self.id)
            .absolute()
            .inset_0()
            .rounded(self.radius)
            .role(gpui_kit::Role::Button)
            .aria_label(self.label)
            .track_focus(&focus)
            .cursor_pointer()
            .focus_visible(move |s| {
                s.shadow(vec![
                    BoxShadow::new(px(0.), px(0.), ring).spread_radius(px(2.)),
                ])
            })
            .children(overlay)
            .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                pointer_down.update(cx, |s, cx| {
                    s.held = true;
                    cx.notify();
                });
            })
            .capture_any_mouse_up(move |_, _, cx| {
                pointer_up.update(cx, |s, cx| {
                    if s.held {
                        s.held = false;
                        s.release += 1;
                        cx.notify();
                    }
                });
            })
            .on_key_down(move |event, _, cx| {
                state.update(cx, |s, cx| {
                    s.held = matches!(event.keystroke.key.as_str(), "enter" | "space")
                        && !event.keystroke.modifiers.modified();
                    cx.notify();
                });
            })
            .on_key_up(move |_, _, cx| {
                release.update(cx, |s, cx| {
                    if s.held {
                        s.release += 1;
                    }
                    s.held = false;
                    cx.notify();
                })
            })
            .on_click(move |_: &ClickEvent, window, cx| (self.pick)(self.slot, window, cx));
        #[cfg(test)]
        let element = element.debug_selector(|| {
            if held {
                "key-contact-held"
            } else {
                "key-contact"
            }
            .into()
        });
        element
    }
}

/// Gap between neighboring caps, in key units.
const GAP: f32 = 0.08;
/// How far the cap's top face sits inside its skirt: sides, back, front.
const SIDE: Pixels = px(3.);
const BACK: Pixels = px(2.);
const FRONT: Pixels = px(6.);
/// Travel of a held key.
const TRAVEL: Pixels = px(3.);
const FACE_RADIUS: Pixels = px(4.);
const SKIRT_RADIUS: Pixels = px(6.);

/// Board materials, resolved per theme: dark anodized aluminum with dark
/// caps, or silver with white caps.
struct Deck {
    case_high: Hsla,
    case_low: Hsla,
    case_edge: Hsla,
    plate: Hsla,
    skirt: Hsla,
    face: Hsla,
    face_hover: Hsla,
    shade: Hsla,
    legend: Hsla,
}

impl Deck {
    fn of(cx: &App) -> Self {
        let theme = cx.theme();
        let base = theme.background;
        let white = |a| gpui_kit::hsla(0., 0., 1., a);
        let black = |a| gpui_kit::hsla(0., 0., 0., a);
        if theme.is_dark() {
            Self {
                case_high: base.blend(white(0.16)),
                case_low: base.blend(white(0.09)),
                case_edge: base.blend(white(0.24)),
                plate: base.blend(black(0.5)),
                skirt: base.blend(white(0.07)),
                face: base.blend(white(0.15)),
                face_hover: base.blend(white(0.22)),
                shade: black(0.45),
                legend: theme.foreground,
            }
        } else {
            Self {
                case_high: base.blend(black(0.07)),
                case_low: base.blend(black(0.15)),
                case_edge: base.blend(black(0.22)),
                plate: base.blend(black(0.34)),
                skirt: base.blend(black(0.16)),
                face: base.blend(black(0.01)),
                face_hover: base.blend(black(0.07)),
                shade: black(0.22),
                legend: theme.foreground,
            }
        }
    }
}

/// Contrast-safe legend color on an arbitrary fill.
fn ink(fill: Hsla) -> Hsla {
    let c = fill.to_rgb();
    let y = 0.2126 * c.r + 0.7152 * c.g + 0.0722 * c.b;
    if y > 0.55 {
        gpui_kit::black()
    } else {
        gpui_kit::white()
    }
}

/// A surface lit from above: `color` at the back edge, slightly darker in front.
fn lit(color: Hsla) -> gpui_kit::Background {
    linear_gradient(
        180.,
        linear_color_stop(color, 0.),
        linear_color_stop(color.blend(gpui_kit::hsla(0., 0., 0., 0.08)), 1.),
    )
}

pub fn board(
    id: &'static str,
    model: &Model,
    look: impl Fn(u8) -> Look,
    on_pick: Option<OnPick>,
    cx: &App,
) -> gpui_kit::AnyElement {
    board_lit(id, model, look, on_pick, None, cx)
}

pub fn board_lit(
    id: &'static str,
    model: &Model,
    look: impl Fn(u8) -> Look,
    on_pick: Option<OnPick>,
    side_light: Option<Vec<Option<Hsla>>>,
    cx: &App,
) -> gpui_kit::AnyElement {
    let theme = cx.theme();
    let deck = Deck::of(cx);
    let interactive = on_pick.is_some();
    let (width, height) = model.layout_size();
    let layout = model.layout();
    let unit_w = |u: f32| relative(u / width);
    let unit_h = |u: f32| relative(u / height);
    let looks: Vec<Look> = layout.iter().map(|c| look(c.slot)).collect();

    // LEDs sit under the caps: their light fills the gaps and washes the plate.
    let leds = layout.iter().zip(&looks).filter_map(|(c, l)| {
        let glow = l.glow?;
        Some(
            div()
                .absolute()
                .left(unit_w(c.x))
                .top(unit_h(c.y))
                .w(unit_w(c.w))
                .h(unit_h(c.h))
                .rounded(SKIRT_RADIUS)
                .bg(glow.opacity(0.6))
                .shadow(vec![
                    BoxShadow::new(px(0.), px(0.), glow.opacity(0.55))
                        .blur_radius(px(8.))
                        .spread_radius(px(1.)),
                ]),
        )
    });

    let cell = |slot: u8, x: f32, y: f32, w: f32, h: f32| {
        div()
            .id((id, slot as usize))
            .group(SharedString::from(format!("{id}-{slot}")))
            .absolute()
            .left(unit_w(x + GAP / 2.))
            .top(unit_h(y + GAP / 2.))
            .w(unit_w(w - GAP))
            .h(unit_h(h - GAP))
            .when(interactive, |d| d.cursor_pointer())
    };
    let contact = |slot: u8, radius: Pixels| {
        on_pick.clone().map(|pick| Contact {
            id: format!("{id}-{slot}-contact").into(),
            label: model.slot_name(slot),
            slot,
            radius,
            pick,
        })
    };
    let ring = |radius: Pixels| {
        div()
            .absolute()
            .inset_0()
            .rounded(radius)
            .border_2()
            .border_color(theme.primary)
    };
    let action = |text: SharedString, color: Hsla| {
        div()
            .text_size(px(10.))
            .line_height(px(12.))
            .text_color(color)
            .font_weight(gpui_kit::FontWeight::SEMIBOLD)
            .truncate()
            .child(text)
    };

    let cap = |slot: u8, x: f32, y: f32, w: f32, h: f32, l: &Look| {
        let group = SharedString::from(format!("{id}-{slot}"));
        let tint = |base: Hsla, strength: f32| {
            l.glow
                .map_or(base, |glow| base.blend(glow.opacity(strength)))
        };
        let (face, skirt, legend, accent) = match l.fill {
            Some(fill) => (
                fill,
                fill.blend(gpui_kit::hsla(0., 0., 0., 0.3)),
                ink(fill),
                ink(fill),
            ),
            None => (
                tint(deck.face, 0.1),
                tint(deck.skirt, 0.55),
                deck.legend,
                theme.primary,
            ),
        };
        let hover = tint(deck.face_hover, 0.1);
        let small = w <= 1. && l.legend.chars().count() > 3;
        cell(slot, x, y, w, h)
            .child(
                div()
                    .relative()
                    .size_full()
                    .rounded(SKIRT_RADIUS)
                    .bg(skirt)
                    .shadow(vec![
                        BoxShadow::new(px(0.), px(1.), deck.shade).blur_radius(px(2.)),
                    ])
                    .child(
                        v_flex()
                            .id((id, 1000 + slot as usize))
                            .absolute()
                            .left(SIDE)
                            .right(SIDE)
                            .top(BACK)
                            .bottom(FRONT)
                            .when(l.pressed, |d| d.top(BACK + TRAVEL).bottom(FRONT - TRAVEL))
                            .overflow_hidden()
                            .px(px(3.))
                            .py(px(2.))
                            .rounded(FACE_RADIUS)
                            .bg(lit(face))
                            .when(interactive, |d| {
                                d.when(l.fill.is_none(), |d| {
                                    d.group_hover(group.clone(), |s| s.bg(lit(hover)))
                                })
                            })
                            .child(
                                div()
                                    .text_size(px(if small { 9. } else { 11. }))
                                    .whitespace_nowrap()
                                    .font_weight(gpui_kit::FontWeight::MEDIUM)
                                    .line_height(px(14.))
                                    .text_color(legend)
                                    .child(l.legend.clone()),
                            )
                            .when_some(l.action.clone(), |d, text| d.child(action(text, accent))),
                    )
                    .when(l.selected, |d| d.child(ring(SKIRT_RADIUS))),
            )
            .children(contact(slot, SKIRT_RADIUS))
    };

    // The knob is a turned aluminum cylinder, lit from the upper left.
    let knob = model.knob_cap().map(|knob| {
        let slot = knob.slot;
        let l = look(slot);
        let group = SharedString::from(format!("{id}-{slot}"));
        let full = px(999.);
        cell(slot, knob.x, knob.y, knob.w, knob.h)
            .child(
                div()
                    .relative()
                    .size_full()
                    .rounded(full)
                    .bg(linear_gradient(
                        135.,
                        linear_color_stop(deck.case_edge, 0.),
                        linear_color_stop(deck.case_low, 1.),
                    ))
                    .shadow(vec![
                        BoxShadow::new(px(0.), px(2.), deck.shade).blur_radius(px(4.)),
                    ])
                    .child(
                        div()
                            .id((id, 1000 + slot as usize))
                            .absolute()
                            .inset(relative(0.14))
                            .flex()
                            .items_center()
                            .justify_center()
                            .overflow_hidden()
                            .rounded(full)
                            .bg(linear_gradient(
                                135.,
                                linear_color_stop(deck.case_high, 0.),
                                linear_color_stop(deck.case_edge, 1.),
                            ))
                            .when(interactive, |d| {
                                d.group_hover(group.clone(), |s| s.bg(deck.case_edge))
                            })
                            .when(l.pressed, |d| d.bg(deck.case_low))
                            .child(
                                div()
                                    .absolute()
                                    .top(relative(0.1))
                                    .left(relative(0.47))
                                    .w(relative(0.06))
                                    .h(relative(0.2))
                                    .rounded(full)
                                    .bg(deck.legend.opacity(0.55)),
                            )
                            .when_some(l.action.clone(), |d, text| {
                                d.child(action(text, theme.primary))
                            }),
                    )
                    .when(l.selected, |d| d.child(ring(full))),
            )
            .children(contact(slot, full))
    });

    div()
        .w_full()
        .rounded(px(16.))
        .bg(linear_gradient(
            180.,
            linear_color_stop(deck.case_high, 0.),
            linear_color_stop(deck.case_low, 1.),
        ))
        .border_1()
        .border_color(deck.case_edge)
        .px_4()
        .pt_4()
        .pb_5()
        .child(
            div().rounded(px(8.)).bg(deck.plate).p_1().child(
                div()
                    .relative()
                    .w_full()
                    .aspect_ratio(width / height)
                    .children(leds)
                    .children(
                        layout
                            .iter()
                            .zip(&looks)
                            .map(|(c, l)| cap(c.slot, c.x, c.y, c.w, c.h, l)),
                    )
                    .children(knob),
            ),
        )
        .when(model.side_light, |d| {
            d.child(
                div()
                    .mt_2()
                    .h(px(3.))
                    .rounded_full()
                    .overflow_hidden()
                    .flex()
                    .children(
                        side_light
                            .unwrap_or_else(|| vec![None])
                            .into_iter()
                            .map(|color| {
                                div()
                                    .flex_1()
                                    .h_full()
                                    .bg(color.map_or(deck.plate, |c| deck.plate.blend(c)))
                            }),
                    ),
            )
        })
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_kit::{
        Context, KeyDownEvent, KeyUpEvent, Keystroke, Modifiers, MouseButton, Render,
        TestAppContext, point,
    };
    use std::cell::Cell;

    struct Harness(Rc<Cell<usize>>);
    impl Render for Harness {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let clicks = self.0.clone();
            div().relative().size(px(100.)).child(Contact {
                id: "contact-test".into(),
                label: "A".into(),
                slot: 0,
                radius: px(4.),
                pick: Rc::new(move |_, _, _| clicks.set(clicks.get() + 1)),
            })
        }
    }

    #[gpui::test]
    fn keyboard_and_pointer_contact_keep_bounds_and_activate_once(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let clicks = Rc::new(Cell::new(0));
        let (_, cx) = cx.add_window_view({
            let clicks = clicks.clone();
            move |_, _| Harness(clicks)
        });
        cx.update(|window, cx| {
            window.activate_window();
            window.draw(cx).clear(cx);
        });
        let resting = cx.debug_bounds("key-contact").unwrap();
        let point = point(px(20.), px(20.));
        cx.simulate_mouse_move(point, None, Modifiers::default());
        cx.simulate_mouse_down(point, MouseButton::Left, Modifiers::default());
        cx.update(|window, cx| window.draw(cx).clear(cx));
        assert_eq!(clicks.get(), 0);
        assert_eq!(cx.debug_bounds("key-contact-held"), Some(resting));
        cx.simulate_mouse_up(point, MouseButton::Left, Modifiers::default());
        assert_eq!(clicks.get(), 1);
        cx.update(|window, cx| {
            window.focus_next(cx);
            window.draw(cx).clear(cx);
        });
        for key in ["enter", "space"] {
            let start = clicks.get();
            let keystroke = Keystroke::parse(key).unwrap();
            for is_held in [false, true] {
                cx.simulate_event(KeyDownEvent {
                    keystroke: keystroke.clone(),
                    is_held,
                    prefer_character_input: false,
                });
                cx.update(|window, cx| window.draw(cx).clear(cx));
                assert_eq!(clicks.get(), start);
                assert_eq!(cx.debug_bounds("key-contact-held"), Some(resting));
            }
            cx.simulate_event(KeyUpEvent { keystroke });
            cx.update(|window, cx| window.draw(cx).clear(cx));
            assert_eq!(clicks.get(), start + 1);
            assert!(cx.debug_bounds("key-contact-held").is_none());
        }
        let stroke = Keystroke::parse("space").unwrap();
        cx.simulate_event(KeyDownEvent {
            keystroke: stroke.clone(),
            is_held: false,
            prefer_character_input: false,
        });
        cx.update(|window, cx| {
            window.blur(cx);
            window.draw(cx).clear(cx);
        });
        cx.simulate_event(KeyUpEvent { keystroke: stroke });
        assert_eq!(clicks.get(), 3);
        assert!(cx.debug_bounds("key-contact-held").is_none());
    }
}
