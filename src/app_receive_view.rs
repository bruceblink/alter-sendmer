//! AlterSendmer 的接收页渲染。

use super::*;

impl AlterSendmeApp {
    pub(crate) fn render_receive(
        &mut self,
        colors: Palette,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let receiving = self.receive_phase.is_active();
        let completed = self.receive_phase == TransferPhase::Completed;
        let progress = self.receive_progress.clone();
        let receive_name = self
            .receive_files
            .first()
            .cloned()
            .unwrap_or_else(|| self.text("transfer.file"));

        div()
            .flex()
            .flex_col()
            .gap_3()
            .child(receive_heading(self, colors))
            .when(!receiving && !completed, |view| {
                view.child(receive_idle_panel(self, colors, cx))
            })
            .when(receiving && !completed, |view| {
                view.child(receive_active_panel(
                    self,
                    progress.clone(),
                    receive_name.clone(),
                    colors,
                    cx,
                ))
            })
            .when(completed, |view| {
                view.child(completion_card(self, colors, cx))
            })
            .into_any_element()
    }
}

fn receive_heading(app: &AlterSendmeApp, colors: Palette) -> gpui_kit::Div {
    div().flex().items_start().justify_between().gap_3().child(
        div()
            .flex_1()
            .min_w(px(0.0))
            .flex()
            .flex_col()
            .gap_1()
            .child(
                div()
                    .text_lg()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(app.text("receiver.title")),
            )
            .child(
                div()
                    .text_sm()
                    .text_color(colors.muted)
                    .child(app.text("receiver.subtitle")),
            )
            .child(
                div().text_xs().text_color(colors.muted).child(
                    app.text("state.label")
                        .replace("{{value}}", &state_label(app.receive_phase, app.locale)),
                ),
            ),
    )
}

fn receive_idle_panel(
    app: &mut AlterSendmeApp,
    colors: Palette,
    cx: &mut Context<AlterSendmeApp>,
) -> impl IntoElement {
    let failed = app.receive_phase == TransferPhase::Failed;
    let ticket_ready = !app.ticket_input.trim().is_empty();
    let receive_action = if failed {
        app.text("transfer.tryAgain")
    } else {
        app.text("receiver.startReceiving")
    };
    let save = app.save_path.display().to_string();

    div()
        .flex()
        .flex_col()
        .gap_2()
        .child(receive_step_heading(
            "1",
            app.text("receiver.ticketLabel"),
            colors,
        ))
        .child(
            div()
                .flex()
                .flex_wrap()
                .items_center()
                .gap_2()
                .child(
                    ticket_input(app, colors, cx)
                        .flex_1()
                        .min_w(px(0.0))
                        .h(px(52.0)),
                )
                .child(app.secondary_button(
                    "paste-ticket",
                    app.text("receiver.pasteFromClipboard"),
                    true,
                    colors,
                    cx,
                    |app, cx| app.paste_ticket(cx),
                )),
        )
        .child(receive_destination_step(app, save, colors, cx))
        .child(
            div()
                .border_t_1()
                .border_color(colors.border)
                .pt_1()
                .flex()
                .flex_col()
                .gap_1()
                .child(receive_step_heading("3", receive_action.clone(), colors))
                .child(
                    app.button(
                        "receive-start",
                        receive_action,
                        ticket_ready,
                        colors,
                        cx,
                        |app, cx| app.begin_or_retry_receiving(cx),
                    )
                    .w_full()
                    .h(px(34.0)),
                )
                .child(quiet_text_button(
                    "receiver-help",
                    app.text("receiver.howToReceive"),
                    colors,
                    cx,
                    |app, cx| app.toggle_receiver_help(cx),
                )),
        )
        .when(app.receiver_help_open, |view| {
            view.child(receiver_help_panel(app, colors))
        })
}

fn receive_destination_step(
    app: &mut AlterSendmeApp,
    save: String,
    colors: Palette,
    cx: &mut Context<AlterSendmeApp>,
) -> impl IntoElement {
    div()
        .border_t_1()
        .border_color(colors.border)
        .pt_1()
        .flex()
        .flex_col()
        .gap_1()
        .child(receive_step_heading(
            "2",
            app.text("receiver.saveToFolder"),
            colors,
        ))
        .child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .child(
                    div()
                        .id("receive-destination")
                        .h(px(30.0))
                        .flex_1()
                        .min_w(px(0.0))
                        .px_3()
                        .flex()
                        .items_center()
                        .rounded_md()
                        .border_1()
                        .border_color(colors.border)
                        .bg(colors.panel_alt)
                        .text_sm()
                        .truncate()
                        .child(save),
                )
                .child(app.secondary_button(
                    "choose-save",
                    app.text("browse"),
                    true,
                    colors,
                    cx,
                    |app, cx| app.choose_save_path(cx),
                )),
        )
}

fn receive_active_panel(
    app: &mut AlterSendmeApp,
    progress: Progress,
    receive_name: String,
    colors: Palette,
    cx: &mut Context<AlterSendmeApp>,
) -> impl IntoElement {
    div()
        .text_sm()
        .font_weight(FontWeight::SEMIBOLD)
        .child(app.status.clone())
        .child(
            div()
                .text_xs()
                .text_color(colors.muted)
                .truncate()
                .child(receive_name.clone()),
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
                .child(progress.current_name.clone().unwrap_or(receive_name)),
        )
        .child(
            div()
                .text_xs()
                .text_color(colors.muted)
                .child(progress.files_label(&app.text("transfer.files"))),
        )
        .child(app.button(
            "receive-stop",
            app.text("receiver.stopReceiving"),
            true,
            colors,
            cx,
            |app, cx| app.stop_receiving(cx),
        ))
}

/// Renders one numbered receive step so the idle screen reads as a single linear workflow.
fn receive_step_heading(number: &'static str, title: String, colors: Palette) -> gpui_kit::Div {
    div()
        .flex()
        .items_center()
        .gap_2()
        .child(
            div()
                .w(px(20.0))
                .h(px(20.0))
                .flex_shrink_0()
                .flex()
                .items_center()
                .justify_center()
                .rounded_md()
                .border_1()
                .border_color(colors.accent)
                .text_xs()
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(colors.accent)
                .child(number),
        )
        .child(
            div()
                .text_sm()
                .font_weight(FontWeight::SEMIBOLD)
                .child(title),
        )
}

/// Keeps help discoverable without giving it the visual weight of the receive action.
fn quiet_text_button(
    id: &'static str,
    label: String,
    colors: Palette,
    cx: &mut Context<AlterSendmeApp>,
    action: impl Fn(&mut AlterSendmeApp, &mut Context<AlterSendmeApp>) + 'static,
) -> gpui_kit::Stateful<gpui_kit::Div> {
    div()
        .id(id)
        .focusable()
        .tab_stop(true)
        .focus_visible(|style| style.text_color(colors.text))
        .role(A11yRole::Button)
        .aria_label(label.clone())
        .h(px(20.0))
        .flex()
        .items_center()
        .justify_center()
        .text_xs()
        .text_color(colors.muted)
        .cursor_pointer()
        .hover(move |style| style.text_color(colors.text))
        .on_click(cx.listener(move |app, _, _, cx| action(app, cx)))
        .child(label)
}

pub(crate) async fn pump_events(
    entity: gpui_kit::WeakEntity<AlterSendmeApp>,
    receiver: Receiver<(u64, TransferEvent)>,
    cx: &mut AsyncApp,
) {
    while let Ok((generation, event)) = receiver.recv().await {
        let _ = entity.update(cx, |app, cx| {
            if app.generation == generation {
                app.apply_event(event, cx);
            }
        });
    }
}
