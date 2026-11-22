//! AlterSendmer 的发送页和通用控件渲染。

use super::*;

impl AlterSendmeApp {
    pub(crate) fn tab_button(
        &self,
        tab: Tab,
        label: &'static str,
        colors: Palette,
        cx: &mut Context<Self>,
    ) -> gpui_kit::Stateful<gpui_kit::Div> {
        let active = self.tab == tab;
        div()
            .id(label)
            .focusable()
            .tab_stop(true)
            .focus_visible(|style| style.border_color(colors.accent))
            .role(A11yRole::Tab)
            .aria_label(label)
            .aria_selected(active)
            .flex_1()
            .h(px(38.0))
            .flex()
            .items_center()
            .justify_center()
            .rounded_md()
            .text_sm()
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(if active {
                colors.background
            } else {
                colors.muted
            })
            .bg(if active {
                colors.accent
            } else {
                colors.panel_alt
            })
            .cursor_pointer()
            .on_click(cx.listener(move |app, _, _, cx| {
                if !tabs_locked(app.send_phase, app.receive_phase) && app.tab != tab {
                    app.tab = tab;
                    app.history_open = false;
                    app.preferences_open = false;
                    app.diagnostics = None;
                    app.receiver_help_open = false;
                    app.status = app.text("ready");
                    cx.notify();
                }
            }))
            .child(label)
    }

    pub(crate) fn button(
        &self,
        id: impl Into<gpui_kit::ElementId>,
        label: impl Into<String>,
        enabled: bool,
        colors: Palette,
        cx: &mut Context<Self>,
        action: impl Fn(&mut Self, &mut Context<Self>) + 'static,
    ) -> gpui_kit::Stateful<gpui_kit::Div> {
        let id = id.into();
        let label = label.into();
        let destructive = id.to_string().contains("stop");
        div()
            .id(id)
            .focusable()
            .tab_stop(enabled)
            .focus_visible(|style| style.border_color(colors.accent))
            .role(A11yRole::Button)
            .aria_label(label.clone())
            .h(px(38.0))
            .px_4()
            .flex()
            .items_center()
            .justify_center()
            .rounded_md()
            .border_1()
            .border_color(if enabled {
                if destructive {
                    colors.danger
                } else {
                    colors.accent
                }
            } else {
                colors.border
            })
            .bg(if enabled {
                if destructive {
                    colors.danger
                } else {
                    colors.accent
                }
            } else {
                colors.panel_alt
            })
            .text_color(if enabled {
                colors.background
            } else {
                colors.muted
            })
            .font_weight(FontWeight::SEMIBOLD)
            .text_sm()
            .cursor_pointer()
            .when(enabled, |button| {
                button.on_click(cx.listener(move |app, _, _, cx| action(app, cx)))
            })
            .child(label)
    }

    /// Renders a restrained secondary command without competing with the primary transfer action.
    pub(crate) fn secondary_button(
        &self,
        id: impl Into<gpui_kit::ElementId>,
        label: String,
        enabled: bool,
        colors: Palette,
        cx: &mut Context<Self>,
        action: impl Fn(&mut Self, &mut Context<Self>) + 'static,
    ) -> gpui_kit::Stateful<gpui_kit::Div> {
        div()
            .id(id)
            .focusable()
            .tab_stop(enabled)
            .focus_visible(|style| style.border_color(colors.accent))
            .role(A11yRole::Button)
            .aria_label(label.clone())
            .h(px(34.0))
            .px_3()
            .flex()
            .items_center()
            .justify_center()
            .rounded_md()
            .border_1()
            .border_color(colors.border)
            .bg(colors.panel)
            .text_color(if enabled { colors.text } else { colors.muted })
            .text_sm()
            .cursor_pointer()
            .when(enabled, |button| {
                button
                    .hover(move |style| style.border_color(colors.accent).bg(colors.panel_alt))
                    .on_click(cx.listener(move |app, _, _, cx| action(app, cx)))
            })
            .child(label)
    }

    pub(crate) fn utility_button(
        &self,
        id: impl Into<gpui_kit::ElementId>,
        label: String,
        active: bool,
        colors: Palette,
        cx: &mut Context<Self>,
        action: impl Fn(&mut Self, &mut Context<Self>) + 'static,
    ) -> gpui_kit::Stateful<gpui_kit::Div> {
        div()
            .id(id)
            .focusable()
            .tab_stop(true)
            .focus_visible(|style| style.border_color(colors.accent))
            .role(A11yRole::Button)
            .aria_label(label.clone())
            .h(px(38.0))
            .flex_1()
            .min_w(px(150.0))
            .px_3()
            .flex()
            .items_center()
            .justify_center()
            .rounded_md()
            .border_1()
            .border_color(if active { colors.accent } else { colors.border })
            .bg(if active {
                colors.accent.alpha(0.14)
            } else {
                colors.panel
            })
            .text_color(if active { colors.accent } else { colors.text })
            .text_sm()
            .font_weight(FontWeight::SEMIBOLD)
            .cursor_pointer()
            .hover(move |style| style.bg(colors.panel_alt))
            .on_click(cx.listener(move |app, _, _, cx| action(app, cx)))
            .child(label)
    }

    pub(crate) fn preference_button(
        &self,
        id: impl Into<gpui_kit::ElementId>,
        label: String,
        value: String,
        colors: Palette,
        cx: &mut Context<Self>,
        action: impl Fn(&mut Self, &mut Context<Self>) + 'static,
    ) -> gpui_kit::Stateful<gpui_kit::Div> {
        div()
            .id(id)
            .focusable()
            .tab_stop(true)
            .focus_visible(|style| style.border_color(colors.accent))
            .role(A11yRole::Button)
            .aria_label(format!("{label}: {value}"))
            .h(px(56.0))
            .flex_1()
            .min_w(px(190.0))
            .px_3()
            .flex()
            .items_center()
            .justify_between()
            .gap_3()
            .rounded_md()
            .border_1()
            .border_color(colors.border)
            .bg(colors.panel)
            .cursor_pointer()
            .hover(move |style| style.border_color(colors.accent).bg(colors.panel_alt))
            .on_click(cx.listener(move |app, _, _, cx| action(app, cx)))
            .child(div().text_sm().text_color(colors.muted).child(label))
            .child(
                div()
                    .text_sm()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(colors.accent)
                    .child(value),
            )
    }

    pub(crate) fn render_send(
        &mut self,
        colors: Palette,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let selected = self
            .selected_path
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or_default();
        let progress = self.send_progress.clone();
        let ready = self.send_phase == TransferPhase::Idle;
        let completed = self.send_phase == TransferPhase::Completed;
        let failed = self.send_phase == TransferPhase::Failed;
        let preparing = matches!(
            self.send_phase,
            TransferPhase::Preparing | TransferPhase::Stopping | TransferPhase::Finalizing
        );
        let sharing = matches!(
            self.send_phase,
            TransferPhase::Sharing
                | TransferPhase::Metadata
                | TransferPhase::Transporting
                | TransferPhase::Exporting
        );
        let selected_label = self
            .selected_path
            .as_ref()
            .and_then(|path| path.file_name())
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_else(|| selected.clone());

        div()
            .flex()
            .flex_col()
            .gap_4()
            .child(send_heading(self, colors))
            .when(self.selected_path.is_some(), |view| {
                view.child(send_selected_path(self, selected_label, colors))
            })
            .when(self.send_phase == TransferPhase::Idle, |view| {
                view.child(send_drop_zone(self, selected, colors, cx))
            })
            .when(!sharing && !completed && !failed && !preparing, |view| {
                view.child(self.button(
                    "send-start",
                    self.copy("start"),
                    ready && self.selected_path.is_some(),
                    colors,
                    cx,
                    |app, cx| app.start_sharing(cx),
                ))
            })
            .when(preparing, |view| view.child(send_preparing(self)))
            .when(sharing, |view| {
                view.child(send_sharing(self, progress.clone(), colors, cx))
            })
            .when(failed, |view| view.child(send_failed(self, colors, cx)))
            .when(completed, |view| {
                view.child(completion_card(self, colors, cx))
            })
            .when(
                sharing && self.send_result.is_some() && self.ticket.is_some(),
                |view| {
                    view.child(ticket_card(
                        self.ticket.as_deref().unwrap_or_default(),
                        colors,
                        self.copy("copy"),
                        self.text("sender.sendThisTicket"),
                        cx,
                        |app, cx| app.copy_ticket(cx),
                    ))
                },
            )
            .into_any_element()
    }
}

fn send_heading(app: &AlterSendmeApp, colors: Palette) -> gpui_kit::Div {
    div()
        .flex()
        .flex_col()
        .gap_1()
        .child(
            div()
                .text_lg()
                .font_weight(FontWeight::SEMIBOLD)
                .child(app.copy("send")),
        )
        .child(
            div()
                .text_sm()
                .text_color(colors.muted)
                .child(app.text("sender.subtitle")),
        )
        .child(
            div().text_xs().text_color(colors.muted).child(
                app.text("state.label")
                    .replace("{{value}}", &state_label(app.send_phase, app.locale)),
            ),
        )
}

fn send_selected_path(
    app: &AlterSendmeApp,
    selected_label: String,
    colors: Palette,
) -> gpui_kit::Div {
    div()
        .flex()
        .items_center()
        .justify_between()
        .gap_3()
        .child(div().flex_1().text_sm().truncate().child(format!(
            "{} {}",
            app.text("sender.fileLabel"),
            selected_label
        )))
        .child(
            div()
                .text_xs()
                .text_color(colors.muted)
                .child(if app.selected_is_dir {
                    app.text("transfer.folder")
                } else {
                    app.text("transfer.file")
                }),
        )
}

fn send_drop_zone(
    app: &mut AlterSendmeApp,
    selected: String,
    colors: Palette,
    cx: &mut Context<AlterSendmeApp>,
) -> impl IntoElement {
    div()
        .id("send-drop-zone")
        .focusable()
        .tab_stop(true)
        .role(A11yRole::Button)
        .aria_label(app.copy("drop"))
        .h(px(190.0))
        .w_full()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap_2()
        .rounded_md()
        .border_1()
        .border_color(colors.border)
        .bg(colors.panel)
        .cursor_pointer()
        .on_click(cx.listener(|app, _, _, cx| app.choose_send_path(cx)))
        .on_drop::<ExternalPaths>(cx.listener(move |app, paths, _, cx| {
            app.on_drop(paths, cx);
        }))
        .child(div().text_3xl().text_color(colors.accent).child("+"))
        .child(
            div()
                .text_sm()
                .font_weight(FontWeight::SEMIBOLD)
                .child(app.copy("drop")),
        )
        .child(
            div()
                .text_xs()
                .text_color(colors.muted)
                .child(app.text("sender.orBrowse")),
        )
        .when(!cx.can_select_mixed_files_and_dirs(), |view| {
            view.child(app.button(
                "send-browse-folder",
                app.text("transfer.folder"),
                true,
                colors,
                cx,
                |app, cx| app.choose_send_folder(cx),
            ))
        })
        .child(
            div()
                .text_xs()
                .text_color(colors.muted)
                .truncate()
                .max_w(px(660.0))
                .child(selected),
        )
}

fn send_preparing(app: &AlterSendmeApp) -> gpui_kit::Div {
    div()
        .text_sm()
        .font_weight(FontWeight::SEMIBOLD)
        .child(app.status.clone())
}

fn send_sharing(
    app: &mut AlterSendmeApp,
    progress: Progress,
    colors: Palette,
    cx: &mut Context<AlterSendmeApp>,
) -> impl IntoElement {
    let heading = if app.sender_status == SenderTransferStatus::Completed {
        app.text("sender.shareStillActive")
    } else if app.send_phase == TransferPhase::Sharing {
        app.status.clone()
    } else if progress.total > 0 {
        format!(
            "{:.1}%  {:.1} MB/s",
            progress.percentage(),
            progress.speed / 1_000_000.0
        )
    } else {
        app.status.clone()
    };

    div()
        .flex()
        .flex_col()
        .gap_3()
        .child(
            div()
                .text_sm()
                .font_weight(FontWeight::SEMIBOLD)
                .child(heading),
        )
        .child(progress_bar(&progress, colors))
        .child(
            div()
                .text_xs()
                .text_color(colors.muted)
                .child(progress.transfer_stats_label()),
        )
        .child(
            div()
                .text_xs()
                .text_color(colors.muted)
                .child(app.recovery.label()),
        )
        .child(
            div().text_xs().text_color(colors.muted).child(
                progress
                    .current_name
                    .clone()
                    .unwrap_or_else(|| app.text("transfer.file")),
            ),
        )
        .child(
            div()
                .text_xs()
                .text_color(colors.muted)
                .child(progress.files_label(&app.text("transfer.files"))),
        )
        .child(
            div()
                .flex()
                .gap_3()
                .child(app.button(
                    "send-stop",
                    app.copy("stop"),
                    true,
                    colors,
                    cx,
                    |app, cx| app.stop_sharing(cx),
                ))
                .child(app.button(
                    "send-save-ticket",
                    app.text("sender.saveTicket"),
                    app.ticket.is_some(),
                    colors,
                    cx,
                    |app, cx| app.save_ticket(cx),
                ))
                .child(app.button(
                    "send-open-folder",
                    app.text("sender.openSharedFolder"),
                    app.selected_path.is_some(),
                    colors,
                    cx,
                    |app, cx| app.open_shared_folder(cx),
                )),
        )
}

fn send_failed(
    app: &mut AlterSendmeApp,
    colors: Palette,
    cx: &mut Context<AlterSendmeApp>,
) -> impl IntoElement {
    div()
        .text_sm()
        .text_color(colors.danger)
        .child(app.text("transfer.failed"))
        .child(app.button(
            "send-retry",
            app.text("transfer.tryAgain"),
            true,
            colors,
            cx,
            |app, cx| app.retry_send(cx),
        ))
}
