//! AlterSendmer 的窗口级渲染。

use super::*;

impl Render for AlterSendmeApp {
    /// Renders the complete send/receive workspace with a stable, bounded content column.
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = self.colors(window.appearance());
        let show_workspace_status = !self.history_open
            && !self.preferences_open
            && self.diagnostics.is_none()
            && self.status != self.text("ready");

        div()
            .id("alter-sendme-root")
            .role(A11yRole::Application)
            .aria_label(self.text("appTitle"))
            .track_focus(&self.root_focus)
            .on_action(cx.listener(|_, _: &crate::Tab, window, cx| window.focus_next(cx)))
            .on_action(cx.listener(|_, _: &crate::TabPrev, window, cx| window.focus_prev(cx)))
            .size_full()
            .relative()
            .flex()
            .flex_col()
            .bg(colors.background)
            .text_color(colors.text)
            .on_key_down(cx.listener(|app, event: &KeyDownEvent, window, cx| {
                if event.keystroke.key == "v" && event.keystroke.modifiers.platform {
                    app.paste_focused_input(window, cx);
                } else if event.keystroke.key == "escape" && app.locale_menu_open {
                    app.locale_menu_open = false;
                    cx.notify();
                } else if event.keystroke.key == "enter"
                    && app.tab == Tab::Receive
                    && app.ticket_focus.is_focused(window)
                    && !event.keystroke.modifiers.shift
                {
                    app.begin_or_retry_receiving(cx);
                }
            }))
            .child(window_header(self, colors, cx))
            .child(window_content(self, colors, cx, show_workspace_status))
            .child(window_footer(self, colors, cx))
            .when(self.locale_menu_open, |view| {
                view.child(locale_menu(self, colors, cx))
            })
    }
}
fn window_header(
    app: &mut AlterSendmeApp,
    colors: Palette,
    cx: &mut Context<AlterSendmeApp>,
) -> gpui_kit::Div {
    div()
        .h(px(64.0))
        .flex_shrink(0.0)
        .px_6()
        .flex()
        .items_center()
        .justify_between()
        .border_b_1()
        .border_color(colors.border)
        .child(
            div()
                .flex()
                .items_center()
                .gap_3()
                .child(
                    div()
                        .id("app-brand-mark")
                        .size(px(32.0))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded_md()
                        .bg(colors.accent)
                        .text_color(colors.background)
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(
                            svg()
                                .path("alter-sendme-mark.svg")
                                .size(px(22.0))
                                .text_color(colors.background),
                        ),
                )
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .child(
                            div()
                                .text_lg()
                                .font_weight(FontWeight::SEMIBOLD)
                                .child(app.text("appTitle")),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(colors.muted)
                                .child(app.text("appSubtitle")),
                        ),
                ),
        )
        .child(window_header_controls(app, colors, cx))
}

fn window_header_controls(
    app: &mut AlterSendmeApp,
    colors: Palette,
    cx: &mut Context<AlterSendmeApp>,
) -> gpui_kit::Div {
    div()
        .flex()
        .items_center()
        .gap_2()
        .child(
            div()
                .id("theme-toggle")
                .focusable()
                .tab_stop(true)
                .role(A11yRole::Button)
                .aria_label(app.text("theme.label"))
                .px_3()
                .py_2()
                .rounded_md()
                .bg(colors.panel_alt)
                .text_sm()
                .cursor_pointer()
                .on_click(cx.listener(|app, _, _, cx| {
                    app.theme = match app.theme {
                        Theme::System => Theme::Dark,
                        Theme::Dark => Theme::Light,
                        Theme::Light => Theme::System,
                    };
                    persist_theme(app.theme);
                    cx.notify();
                }))
                .child(format!(
                    "{}: {}",
                    app.text("theme.label"),
                    app.theme_label()
                )),
        )
        .child(
            div()
                .id("locale-toggle")
                .focusable()
                .tab_stop(true)
                .role(A11yRole::ComboBox)
                .aria_label(app.text("language.label"))
                .aria_expanded(app.locale_menu_open)
                .px_3()
                .py_2()
                .rounded_md()
                .border_1()
                .border_color(if app.locale_menu_open {
                    colors.accent
                } else {
                    colors.border
                })
                .bg(colors.panel_alt)
                .text_sm()
                .cursor_pointer()
                .on_click(cx.listener(|app, _, _, cx| {
                    app.locale_menu_open = !app.locale_menu_open;
                    cx.notify();
                }))
                .child(format!(
                    "{}: {}",
                    app.text("language.label"),
                    app.locale.label()
                )),
        )
}

fn window_content(
    app: &mut AlterSendmeApp,
    colors: Palette,
    cx: &mut Context<AlterSendmeApp>,
    show_workspace_status: bool,
) -> impl IntoElement {
    div()
        .flex_1()
        .flex_grow(1.0)
        .flex_shrink(1.0)
        .min_w(px(0.0))
        .min_h(px(0.0))
        .flex()
        .flex_col()
        .w_full()
        .id("content-scroll")
        .overflow_y_scroll()
        .p_6()
        .pb(px(48.0))
        .child(window_workspace(app, colors, cx, show_workspace_status))
}

fn window_workspace(
    app: &mut AlterSendmeApp,
    colors: Palette,
    cx: &mut Context<AlterSendmeApp>,
    show_workspace_status: bool,
) -> impl IntoElement {
    div()
        .w_full()
        .min_w(px(0.0))
        .min_h(px(0.0))
        .flex_shrink(0.0)
        .max_w(px(720.0))
        .mx_auto()
        .child(workspace_navigation(app, colors, cx))
        .child(workspace_transfer(app, colors, cx, show_workspace_status))
}

fn workspace_navigation(
    app: &mut AlterSendmeApp,
    colors: Palette,
    cx: &mut Context<AlterSendmeApp>,
) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .gap_4()
        .child(
            div()
                .flex()
                .gap_2()
                .p_1()
                .rounded_md()
                .bg(colors.panel_alt)
                .child(app.tab_button(Tab::Send, app.copy("send"), colors, cx))
                .child(app.tab_button(Tab::Receive, app.copy("receive"), colors, cx)),
        )
        .child(
            div()
                .flex()
                .flex_wrap()
                .gap_2()
                .child(app.utility_button(
                    "history",
                    app.text("history.title"),
                    app.history_open,
                    colors,
                    cx,
                    |app, cx| app.show_history(cx),
                ))
                .child(app.utility_button(
                    "preferences",
                    app.text("preferences.title"),
                    app.preferences_open,
                    colors,
                    cx,
                    |app, cx| app.show_preferences(cx),
                ))
                .child(app.utility_button(
                    "diagnostics",
                    app.text("diagnostics.action"),
                    app.diagnostics.is_some(),
                    colors,
                    cx,
                    |app, cx| app.run_diagnostics(cx),
                )),
        )
        .when(app.history_open, |view| {
            view.child(history_panel(app, colors, cx))
        })
        .when(app.preferences_open, |view| {
            view.child(preferences_panel(app, colors, cx))
        })
        .when(app.diagnostics.is_some(), |view| {
            view.child(
                div()
                    .p_4()
                    .rounded_md()
                    .border_1()
                    .border_color(colors.border)
                    .bg(colors.panel_alt)
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(app.text("diagnostics.action")),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(colors.muted)
                            .child(app.diagnostics.clone().unwrap_or_default()),
                    )
                    .child(app.secondary_button(
                        "diagnostics-save",
                        app.text("diagnostics.save"),
                        true,
                        colors,
                        cx,
                        |app, cx| app.save_diagnostics(cx),
                    )),
            )
        })
}

fn workspace_transfer(
    app: &mut AlterSendmeApp,
    colors: Palette,
    cx: &mut Context<AlterSendmeApp>,
    show_workspace_status: bool,
) -> impl IntoElement {
    div()
        .min_w(px(0.0))
        .min_h(px(0.0))
        .flex_shrink(0.0)
        .rounded_md()
        .border_1()
        .border_color(colors.border)
        .bg(colors.panel)
        .p_4()
        .flex()
        .flex_col()
        .gap_4()
        .when(app.tab == Tab::Send, |view| {
            view.child(app.render_send(colors, cx))
        })
        .when(app.tab == Tab::Receive, |view| {
            view.child(app.render_receive(colors, cx))
        })
        .when(app.error.is_some(), |view| {
            view.child(
                div()
                    .rounded_md()
                    .border_1()
                    .border_color(colors.danger)
                    .bg(colors.danger.alpha(0.12))
                    .p_3()
                    .text_sm()
                    .text_color(colors.danger)
                    .child(app.error.clone().unwrap_or_default()),
            )
        })
        .when(show_workspace_status, |view| {
            view.child(
                div()
                    .text_sm()
                    .text_color(colors.muted)
                    .child(app.status.clone()),
            )
        })
}
fn window_footer(
    app: &mut AlterSendmeApp,
    colors: Palette,
    cx: &mut Context<AlterSendmeApp>,
) -> gpui_kit::Div {
    div()
        .h(px(46.0))
        .flex_shrink(0.0)
        .px_6()
        .flex()
        .items_center()
        .justify_between()
        .border_t_1()
        .border_color(colors.border)
        .text_xs()
        .text_color(colors.muted)
        .child(app.text("appSubtitle"))
        .child(
            div()
                .flex_1()
                .px_3()
                .text_center()
                .truncate()
                .child(app.update_status.clone().unwrap_or_default()),
        )
        .child(
            div()
                .flex()
                .gap_3()
                .child(
                    div()
                        .id("footer-new")
                        .focusable()
                        .tab_stop(true)
                        .role(A11yRole::Button)
                        .aria_label(app.copy("new"))
                        .cursor_pointer()
                        .on_click(cx.listener(|app, _, _, cx| app.new_transfer(cx)))
                        .child(app.copy("new")),
                )
                .child(
                    div()
                        .id("footer-sponsor")
                        .focusable()
                        .tab_stop(true)
                        .role(A11yRole::Button)
                        .aria_label(app.text("donate"))
                        .cursor_pointer()
                        .on_click(cx.listener(|app, _, _, cx| app.open_sponsor_page(cx)))
                        .child(app.text("donate")),
                )
                .child(
                    div()
                        .id("footer-update")
                        .focusable()
                        .tab_stop(true)
                        .role(A11yRole::Button)
                        .aria_label(app.text("update.checkNow"))
                        .cursor_pointer()
                        .on_click(cx.listener(|app, _, _, cx| app.open_update_or_check(cx)))
                        .child(if app.update_installing {
                            app.text("update.downloading")
                        } else if app.update_checking {
                            app.text("update.checking")
                        } else if app.update_info.is_some() {
                            app.text("update.found")
                        } else {
                            app.text("update.checkNow")
                        }),
                )
                .child(div().child(format!("v{}", env!("CARGO_PKG_VERSION")))),
        )
}
