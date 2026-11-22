//! AlterSendmer 的票据、完成和历史卡片。

use super::*;

pub(crate) fn ticket_card(
    ticket: &str,
    colors: Palette,
    copy_label: &'static str,
    ticket_hint: String,
    cx: &mut Context<AlterSendmeApp>,
    action: impl Fn(&mut AlterSendmeApp, &mut Context<AlterSendmeApp>) + 'static,
) -> gpui_kit::Div {
    div()
        .flex()
        .flex_col()
        .gap_2()
        .p_3()
        .rounded_md()
        .bg(colors.panel_alt)
        .child(div().text_xs().text_color(colors.muted).child(ticket_hint))
        .child(
            div()
                .text_xs()
                .text_color(colors.text)
                .truncate()
                .child(ticket.to_owned()),
        )
        .child(
            div()
                .id("copy-ticket-secondary")
                .focusable()
                .tab_stop(true)
                .role(A11yRole::Button)
                .aria_label(copy_label)
                .h(px(32.0))
                .px_3()
                .flex()
                .items_center()
                .justify_center()
                .rounded_md()
                .border_1()
                .border_color(colors.border)
                .text_sm()
                .cursor_pointer()
                .on_click(cx.listener(move |app, _, _, cx| action(app, cx)))
                .child(copy_label),
        )
}

pub(crate) fn completion_card(
    app: &AlterSendmeApp,
    colors: Palette,
    cx: &mut Context<AlterSendmeApp>,
) -> gpui_kit::Div {
    div()
        .flex()
        .flex_col()
        .gap_3()
        .p_4()
        .rounded_md()
        .bg(colors.panel_alt)
        .child(
            div()
                .text_lg()
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(colors.accent)
                .child(if app.is_send_stopped() {
                    app.text("transfer.stopped")
                } else {
                    app.text("transfer.complete")
                }),
        )
        .child(
            div().text_sm().child(
                app.completed_name
                    .clone()
                    .unwrap_or_else(|| app.text("transfer.file")),
            ),
        )
        .child(
            div()
                .text_sm()
                .text_color(colors.muted)
                .child(if app.is_send_stopped() {
                    app.text("transfer.wasStopped")
                } else {
                    format!(
                        "{} bytes - {:.1}s",
                        app.completed_size,
                        app.completed_duration.as_secs_f32()
                    )
                }),
        )
        .child(app.button(
            "receive-reveal",
            app.text("open_folder"),
            app.completed_path.is_some(),
            colors,
            cx,
            |app, cx| app.reveal_completed_path(cx),
        ))
        .child(app.button(
            "receive-done",
            if app.receive_phase == TransferPhase::Completed {
                app.copy("new").to_owned()
            } else {
                app.text("transfer.done")
            },
            true,
            colors,
            cx,
            |app, cx| app.done_transfer(cx),
        ))
}

pub(crate) fn history_panel(
    app: &mut AlterSendmeApp,
    colors: Palette,
    cx: &mut Context<AlterSendmeApp>,
) -> gpui_kit::Div {
    let rows = app.history.iter().enumerate().fold(
        div().flex().flex_col().gap_2(),
        |panel, (index, entry)| {
            // 历史记录仅能复制当前会话仍在使用的票据。
            let ticket = entry
                .ticket
                .as_ref()
                .filter(|ticket| app.send_result.is_some() && app.ticket.as_ref() == Some(*ticket))
                .cloned();
            let path = entry.path.clone();
            let mut summary = format!(
                "{} | {} | {} bytes | {} ms | {}",
                entry.role, entry.outcome, entry.size, entry.duration_ms, entry.date
            );
            if let Some(session_id) = entry.session_id.as_deref() {
                let short_id = session_id.get(..8).unwrap_or(session_id);
                summary.push_str(&format!(" | session {short_id}"));
            }
            if let Some(error_code) = entry.error_code.as_deref() {
                let error_phase = entry.error_phase.as_deref().unwrap_or("unknown");
                summary.push_str(&format!(" | {error_code}@{error_phase}"));
            }
            panel.child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap_2()
                    .child(
                        div()
                            .flex_1()
                            .text_xs()
                            .text_color(colors.muted)
                            .truncate()
                            .child(format!("{path} - {summary}")),
                    )
                    .when_some(ticket, |view, ticket| {
                        view.child(app.button(
                            ("history-copy", index),
                            app.copy("copy"),
                            true,
                            colors,
                            cx,
                            move |app, cx| app.copy_history_ticket(ticket.clone(), cx),
                        ))
                    }),
            )
        },
    );
    div()
        .flex()
        .flex_col()
        .gap_3()
        .p_4()
        .rounded_md()
        .border_1()
        .border_color(colors.border)
        .bg(colors.panel_alt)
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .child(
                    div()
                        .text_sm()
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(app.text("history.title")),
                )
                .child(app.button(
                    "history-clear",
                    app.text("history.clear"),
                    !app.history.is_empty(),
                    colors,
                    cx,
                    |app, cx| app.clear_history(cx),
                )),
        )
        .when(app.history.is_empty(), |panel| {
            panel.child(
                div()
                    .text_sm()
                    .text_color(colors.muted)
                    .child(app.text("history.empty")),
            )
        })
        .when(!app.history.is_empty(), |panel| panel.child(rows))
}
