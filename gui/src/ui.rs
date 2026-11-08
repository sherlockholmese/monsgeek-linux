//! Shared layout primitives. Pages compose these; they never pick colors.
use gpui_kit::component::{ActiveTheme, Icon, Sizable, h_flex, scroll::ScrollableElement, v_flex};
use gpui_kit::{
    AnyElement, App, Hsla, IntoElement, ParentElement, SharedString, Styled, div,
    prelude::FluentBuilder, px,
};
use gpui_kit::{ElementId, InteractiveElement};

/// Content column width shared by the title row and every page.
const COLUMN: f32 = 1120.;

/// The surface ladder: window canvas (behind the sidebar), the inset content
/// panel, and cards on the panel. Dark mode steps lighter toward the reader;
/// light mode keeps the panel and cards white on a tinted canvas.
#[derive(Clone, Copy)]
pub struct Surface {
    pub canvas: Hsla,
    pub panel: Hsla,
    pub card: Hsla,
}

impl Surface {
    pub fn of(cx: &App) -> Self {
        let theme = cx.theme();
        let base = theme.background;
        if theme.is_dark() {
            Self {
                canvas: base,
                panel: base.blend(gpui_kit::hsla(0., 0., 1., 0.045)),
                card: base.blend(gpui_kit::hsla(0., 0., 1., 0.07)),
            }
        } else {
            Self {
                canvas: base.blend(gpui_kit::hsla(0., 0., 0., 0.035)),
                panel: base,
                card: base,
            }
        }
    }
}

/// Page title on the content column, with trailing actions.
pub fn title(text: impl Into<SharedString>, actions: impl IntoElement) -> impl IntoElement {
    div().w_full().flex_shrink_0().child(
        h_flex()
            .w_full()
            .max_w(px(COLUMN))
            .mx_auto()
            .min_h(px(72.))
            .pt_2()
            .px_6()
            .gap_4()
            .child(
                div()
                    .flex_1()
                    .pl_6()
                    .text_lg()
                    .font_weight(gpui_kit::FontWeight::MEDIUM)
                    .child(text.into()),
            )
            .child(actions),
    )
}

/// Persistent connection warning, aligned with page content in both themes.
pub fn notice(text: &'static str, cx: &App) -> impl IntoElement {
    div().w_full().flex_shrink_0().child(
        div()
            .w_full()
            .max_w(px(COLUMN))
            .mx_auto()
            .px_6()
            .pb_2()
            .child(
                div()
                    .pl_6()
                    .text_sm()
                    .text_color(cx.theme().foreground)
                    .child(text),
            ),
    )
}

/// Scrollable page canvas on the shared content column.
pub fn page(id: impl Into<ElementId>, children: Vec<AnyElement>) -> impl IntoElement {
    div().id(id).size_full().overflow_y_scrollbar().child(
        v_flex()
            .w_full()
            .max_w(px(COLUMN))
            .mx_auto()
            .px_6()
            .pt_1()
            .pb_6()
            .gap_6()
            .children(children),
    )
}

/// A card of rows with inset dividers, under an optional label.
pub fn section(title: impl Into<SharedString>, rows: Vec<AnyElement>, cx: &App) -> AnyElement {
    let title: SharedString = title.into();
    let theme = cx.theme();
    let count = rows.len();
    v_flex()
        .gap_2()
        .when(!title.is_empty(), |d| d.child(heading(title, cx)))
        .child(
            v_flex()
                .px_6()
                .rounded(theme.radius_lg * 1.5)
                .border_1()
                .border_color(theme.border)
                .bg(Surface::of(cx).card)
                .children(rows.into_iter().enumerate().map(|(i, row)| {
                    div()
                        .when(i + 1 < count, |d| d.border_b_1().border_color(theme.border))
                        .child(row)
                })),
        )
        .into_any_element()
}

/// Section label, aligned with the content inside cards.
pub fn heading(title: impl Into<SharedString>, cx: &App) -> impl IntoElement {
    div()
        .pl_6()
        .text_sm()
        .font_weight(gpui_kit::FontWeight::MEDIUM)
        .text_color(cx.theme().muted_foreground)
        .child(title.into())
}

/// A setting with a leading icon.
pub fn icon_row(
    icon: impl Into<Icon>,
    label: impl Into<SharedString>,
    control: impl IntoElement,
) -> AnyElement {
    h_flex()
        .min_h(px(60.))
        .py_2()
        .gap_6()
        .justify_between()
        .child(
            h_flex()
                .gap_3()
                .child(icon.into().small())
                .child(div().text_sm().child(label.into())),
        )
        .child(h_flex().gap_3().child(control))
        .into_any_element()
}

/// A slider with its current value, right-aligned in tabular width.
pub fn valued(control: impl IntoElement, value: impl Into<SharedString>, cx: &App) -> AnyElement {
    h_flex()
        .gap_4()
        .child(div().w(px(240.)).child(control))
        .child(
            div()
                .w(px(56.))
                .text_sm()
                .text_right()
                .text_color(cx.theme().muted_foreground)
                .child(value.into()),
        )
        .into_any_element()
}

/// Dashed empty-state surface with one short label.
pub fn placeholder(label: impl Into<SharedString>, height: f32, cx: &App) -> AnyElement {
    let theme = cx.theme();
    div()
        .h(px(height))
        .flex()
        .items_center()
        .justify_center()
        .rounded(theme.radius_lg * 1.5)
        .border_1()
        .border_dashed()
        .border_color(theme.border)
        .text_sm()
        .text_color(theme.muted_foreground)
        .child(label.into())
        .into_any_element()
}
