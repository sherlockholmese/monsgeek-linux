//! The physical M1W: every keycap at its true position, scaled to the width
//! available. Pages decide what each cap shows; the board owns geometry.
use std::rc::Rc;

use gpui_kit::component::{ActiveTheme, v_flex};
use gpui_kit::{
    App, ClickEvent, Hsla, InteractiveElement, IntoElement, ParentElement, SharedString,
    StatefulInteractiveElement, Styled, Window, div, prelude::FluentBuilder, px, relative,
};
use monsgeek_linux::keys::{KNOB_PRESS, LAYOUT, LAYOUT_HEIGHT, LAYOUT_WIDTH};

#[derive(Default, Clone)]
pub struct Look {
    pub legend: SharedString,
    /// Secondary line, such as a remapped action.
    pub action: Option<SharedString>,
    /// Full keycap fill, for per-key colors.
    pub fill: Option<Hsla>,
    /// Underglow strip, for effect color.
    pub glow: Option<Hsla>,
    pub selected: bool,
}

pub type OnPick = Rc<dyn Fn(u8, &mut Window, &mut App)>;

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

pub fn board(
    id: &'static str,
    look: impl Fn(u8) -> Look,
    on_pick: Option<OnPick>,
    cx: &App,
) -> gpui_kit::AnyElement {
    let theme = cx.theme();
    let gap = 0.08;
    let unit_w = |u: f32| relative(u / LAYOUT_WIDTH);
    let unit_h = |u: f32| relative(u / LAYOUT_HEIGHT);

    let cap = |slot: u8, x: f32, y: f32, w: f32, round: bool| {
        let l = look(slot);
        let surface = l.fill.unwrap_or(theme.background);
        let fg = l.fill.map(ink).unwrap_or(theme.foreground);
        let muted = l.fill.map(ink).unwrap_or(theme.muted_foreground);
        let interactive = on_pick.is_some();
        let pick = on_pick.clone();
        div()
            .id((id, slot as usize))
            .absolute()
            .left(unit_w(x + gap / 2.))
            .top(unit_h(y + gap / 2.))
            .w(unit_w(w - gap))
            .h(unit_h(1. - gap))
            .child(
                v_flex()
                    .relative()
                    .size_full()
                    .overflow_hidden()
                    .px(px(4.))
                    .py(px(3.))
                    .gap(px(1.))
                    .when(round, |d| d.rounded_full().items_center().justify_center())
                    .when(!round, |d| d.rounded(theme.radius))
                    .bg(surface)
                    .border_1()
                    .border_color(theme.border)
                    .shadow_xs()
                    .when(l.selected, |d| d.border_2().border_color(theme.ring))
                    .when(interactive, |d| {
                        d.cursor_pointer()
                            .when(l.fill.is_none(), |d| d.hover(|s| s.bg(theme.accent)))
                    })
                    .child(
                        div()
                            .text_size(px(if w <= 1. && l.legend.chars().count() > 3 {
                                9.
                            } else {
                                11.
                            }))
                            .whitespace_nowrap()
                            .font_weight(gpui_kit::FontWeight::MEDIUM)
                            .line_height(px(14.))
                            .text_color(fg)
                            .truncate()
                            .child(l.legend.clone()),
                    )
                    .when_some(l.action.clone(), |d, action| {
                        d.child(
                            div()
                                .text_size(px(10.))
                                .line_height(px(12.))
                                .text_color(if l.fill.is_some() {
                                    muted
                                } else {
                                    theme.primary
                                })
                                .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                                .truncate()
                                .child(action),
                        )
                    })
                    .when_some(l.glow, |d, glow| {
                        d.child(
                            div()
                                .absolute()
                                .left_0()
                                .right_0()
                                .bottom_0()
                                .h(px(3.))
                                .bg(glow),
                        )
                    }),
            )
            .when_some(pick, |d, pick| {
                d.on_click(move |_: &ClickEvent, window, cx| pick(slot, window, cx))
            })
    };

    div()
        .w_full()
        .rounded(theme.radius_lg)
        .bg(theme.muted)
        .p_3()
        .child(
            div()
                .relative()
                .w_full()
                .aspect_ratio(LAYOUT_WIDTH / LAYOUT_HEIGHT)
                .children(LAYOUT.iter().map(|c| cap(c.slot, c.x, c.y, c.w, false)))
                .child(cap(KNOB_PRESS, LAYOUT_WIDTH - 1., 0., 1., true)),
        )
        .into_any_element()
}
