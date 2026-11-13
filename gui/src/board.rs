//! The physical M1W: an aluminum case, a recessed plate, sculpted keycaps at
//! their true positions and the knob, scaled to the width available. Pages
//! decide what each cap shows; the board owns geometry and materials.
use std::rc::Rc;

use gpui_kit::component::{ActiveTheme, v_flex};
use gpui_kit::{
    App, BoxShadow, ClickEvent, Hsla, InteractiveElement, IntoElement, ParentElement, Pixels,
    SharedString, StatefulInteractiveElement, Styled, Window, div, linear_color_stop,
    linear_gradient, prelude::FluentBuilder, px, relative,
};
use monsgeek_linux::keys::{KNOB_PRESS, LAYOUT, LAYOUT_HEIGHT, LAYOUT_WIDTH};

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
    look: impl Fn(u8) -> Look,
    on_pick: Option<OnPick>,
    cx: &App,
) -> gpui_kit::AnyElement {
    let theme = cx.theme();
    let deck = Deck::of(cx);
    let interactive = on_pick.is_some();
    let unit_w = |u: f32| relative(u / LAYOUT_WIDTH);
    let unit_h = |u: f32| relative(u / LAYOUT_HEIGHT);
    let looks: Vec<Look> = LAYOUT.iter().map(|c| look(c.slot)).collect();

    // LEDs sit under the caps: their light fills the gaps and washes the plate.
    let leds = LAYOUT.iter().zip(&looks).filter_map(|(c, l)| {
        let glow = l.glow?;
        Some(
            div()
                .absolute()
                .left(unit_w(c.x))
                .top(unit_h(c.y))
                .w(unit_w(c.w))
                .h(unit_h(1.))
                .rounded(SKIRT_RADIUS)
                .bg(glow.opacity(0.6))
                .shadow(vec![
                    BoxShadow::new(px(0.), px(0.), glow.opacity(0.55))
                        .blur_radius(px(8.))
                        .spread_radius(px(1.)),
                ]),
        )
    });

    let cell = |slot: u8, x: f32, y: f32, w: f32| {
        let pick = on_pick.clone();
        div()
            .id((id, slot as usize))
            .group(SharedString::from(format!("{id}-{slot}")))
            .absolute()
            .left(unit_w(x + GAP / 2.))
            .top(unit_h(y + GAP / 2.))
            .w(unit_w(w - GAP))
            .h(unit_h(1. - GAP))
            .when(interactive, |d| d.cursor_pointer())
            .when_some(pick, |d, pick| {
                d.on_click(move |_: &ClickEvent, window, cx| pick(slot, window, cx))
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

    let cap = |slot: u8, x: f32, y: f32, w: f32, l: &Look| {
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
        cell(slot, x, y, w).child(
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
                            .group_active(group.clone(), |s| {
                                s.top(BACK + TRAVEL).bottom(FRONT - TRAVEL)
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
    };

    // The knob is a turned aluminum cylinder, lit from the upper left.
    let knob = {
        let l = look(KNOB_PRESS);
        let group = SharedString::from(format!("{id}-{KNOB_PRESS}"));
        let full = px(999.);
        cell(KNOB_PRESS, LAYOUT_WIDTH - 1., 0., 1.).child(
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
                        .id((id, 1000 + KNOB_PRESS as usize))
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
                                .group_active(group.clone(), |s| s.bg(deck.case_low))
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
    };

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
                    .aspect_ratio(LAYOUT_WIDTH / LAYOUT_HEIGHT)
                    .children(leds)
                    .children(
                        LAYOUT
                            .iter()
                            .zip(&looks)
                            .map(|(c, l)| cap(c.slot, c.x, c.y, c.w, l)),
                    )
                    .child(knob),
            ),
        )
        .into_any_element()
}
