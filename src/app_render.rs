//! AlterSendmer 的发送、接收、设置和历史 UI 构建函数。

use super::*;

#[derive(Clone, Copy)]
pub(crate) struct Palette {
    pub(crate) background: Rgba,
    pub(crate) panel: Rgba,
    pub(crate) panel_alt: Rgba,
    pub(crate) text: Rgba,
    pub(crate) muted: Rgba,
    pub(crate) border: Rgba,
    pub(crate) accent: Rgba,
    pub(crate) accent_alt: Rgba,
    pub(crate) danger: Rgba,
}

pub(crate) fn progress_bar(progress: &Progress, colors: Palette) -> gpui_kit::Div {
    // Keep the fill proportional to its parent so the bar remains usable at the
    // minimum window size instead of overflowing a fixed pixel width.
    let fraction = progress_fraction(progress);
    div()
        .h(px(8.0))
        .w_full()
        .rounded_md()
        .bg(colors.panel_alt)
        .child(
            div()
                .h(px(8.0))
                .w(relative(fraction))
                .rounded_md()
                .bg(colors.accent_alt),
        )
}

/// Returns whether an active transfer must keep the user on the current tab.
/// Completed and failed states remain navigable so a user can switch workflows
/// without first creating a new transfer.
pub(crate) fn tabs_locked(send_phase: TransferPhase, receive_phase: TransferPhase) -> bool {
    send_phase.is_active() || receive_phase.is_active()
}

/// Converts a transfer percentage into a stable relative fill fraction for layout.
pub(crate) fn progress_fraction(progress: &Progress) -> f32 {
    (progress.percentage() / 100.0).clamp(0.0, 1.0)
}

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

pub(crate) fn preferences_panel(
    app: &mut AlterSendmeApp,
    colors: Palette,
    cx: &mut Context<AlterSendmeApp>,
) -> gpui_kit::Div {
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
                .text_sm()
                .font_weight(FontWeight::SEMIBOLD)
                .child(app.text("preferences.title")),
        )
        .child(
            div()
                .flex()
                .flex_wrap()
                .gap_2()
                .child(app.preference_button(
                    "relay-mode",
                    app.text("preferences.relay"),
                    app.relay_label(),
                    colors,
                    cx,
                    |app, cx| app.cycle_relay(cx),
                ))
                .child(app.preference_button(
                    "retry-limit",
                    app.text("preferences.retry"),
                    app.preferences.retry_limit.to_string(),
                    colors,
                    cx,
                    |app, cx| app.cycle_retry_limit(cx),
                ))
                .child(app.preference_button(
                    "download-chunk",
                    app.text("preferences.chunk"),
                    format!("{} MB", app.preferences.download_limit_mb),
                    colors,
                    cx,
                    |app, cx| app.cycle_download_chunk(cx),
                ))
                .child(app.preference_button(
                    "max-receivers",
                    app.text("preferences.maxReceivers"),
                    app.max_receivers_label(),
                    colors,
                    cx,
                    |app, cx| app.cycle_max_receivers(cx),
                ))
                .child(app.preference_button(
                    "session-lifetime",
                    app.text("preferences.sessionLifetime"),
                    app.session_lifetime_label(),
                    colors,
                    cx,
                    |app, cx| app.cycle_session_lifetime(cx),
                ))
                .child(app.preference_button(
                    "manifest-mode",
                    app.text("preferences.manifest"),
                    app.manifest_mode_label(),
                    colors,
                    cx,
                    |app, cx| app.toggle_manifest_mode(cx),
                ))
                .child(app.preference_button(
                    "receive-cache",
                    app.text("preferences.receiveCache"),
                    app.receive_cache_state_label(),
                    colors,
                    cx,
                    |app, cx| app.toggle_receive_cache(cx),
                ))
                .child(app.preference_button(
                    "receive-cache-ttl",
                    app.text("preferences.cacheRetention"),
                    app.receive_cache_ttl_label(),
                    colors,
                    cx,
                    |app, cx| app.cycle_receive_cache_ttl(cx),
                ))
                .child(upload_rate_control(app, colors, cx)),
        )
        .child(
            app.secondary_button(
                "prune-receive-cache",
                if app.cache_maintenance_running {
                    app.text("preferences.pruningCache")
                } else {
                    app.text("preferences.pruneCache")
                },
                !app.cache_maintenance_running,
                colors,
                cx,
                |app, cx| app.prune_receive_cache(cx),
            )
            .w_full(),
        )
}

/// Renders the upload-rate mode and numeric MiB/s field as one stable preference control.
pub(crate) fn upload_rate_control(
    app: &mut AlterSendmeApp,
    colors: Palette,
    cx: &mut Context<AlterSendmeApp>,
) -> gpui_kit::Stateful<gpui_kit::Div> {
    let unlimited = app.preferences.max_upload_rate_mib_per_sec.is_none();
    let label = app.text("preferences.uploadRate");
    let unlimited_label = app.text("preferences.unlimited");
    let rate_value = if app.upload_rate_input.is_empty() {
        DEFAULT_UPLOAD_RATE_MIB_PER_SEC.to_string()
    } else {
        app.upload_rate_input.clone()
    };
    let entity = cx.entity();
    let focus = app.upload_rate_focus.clone();

    div()
        .id("upload-rate-control")
        .h(px(56.0))
        .flex_1()
        .min_w(px(300.0))
        .px_3()
        .flex()
        .items_center()
        .justify_between()
        .gap_3()
        .rounded_md()
        .border_1()
        .border_color(colors.border)
        .bg(colors.panel)
        .child(
            div()
                .text_sm()
                .text_color(colors.muted)
                .child(label.clone()),
        )
        .child(
            div()
                .h(px(34.0))
                .flex()
                .items_center()
                .gap_1()
                .child(
                    div()
                        .id("upload-rate-unlimited")
                        .focusable()
                        .tab_stop(true)
                        .role(A11yRole::Button)
                        .aria_label(format!("{label}: {unlimited_label}"))
                        .h_full()
                        .min_w(px(96.0))
                        .px_2()
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded_md()
                        .border_1()
                        .border_color(if unlimited {
                            colors.accent
                        } else {
                            colors.border
                        })
                        .bg(if unlimited {
                            colors.accent.alpha(0.14)
                        } else {
                            colors.background
                        })
                        .text_color(if unlimited {
                            colors.accent
                        } else {
                            colors.muted
                        })
                        .text_xs()
                        .font_weight(FontWeight::SEMIBOLD)
                        .cursor_pointer()
                        .on_click(cx.listener(|app, _, _, cx| app.clear_upload_rate(cx)))
                        .child(unlimited_label),
                )
                .child(
                    div()
                        .id("upload-rate-input")
                        .track_focus(&focus)
                        .tab_stop(true)
                        .focus_visible(|style| style.border_color(colors.accent))
                        .role(A11yRole::TextInput)
                        .aria_label(format!("{label} MiB/s"))
                        .relative()
                        .h_full()
                        .w(px(68.0))
                        .px_2()
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded_md()
                        .border_1()
                        .border_color(if unlimited {
                            colors.border
                        } else {
                            colors.accent
                        })
                        .bg(colors.background)
                        .text_color(if unlimited { colors.muted } else { colors.text })
                        .text_sm()
                        .font_weight(FontWeight::SEMIBOLD)
                        .cursor_pointer()
                        .on_click(
                            cx.listener(move |app, _, window, cx| {
                                app.focus_upload_rate(window, cx)
                            }),
                        )
                        .child(gpui_kit::canvas(
                            move |bounds, _, _| bounds,
                            move |bounds, _, window, cx| {
                                window.handle_input(
                                    &focus,
                                    ElementInputHandler::new(bounds, entity.clone()),
                                    cx,
                                );
                            },
                        ))
                        .child(rate_value),
                )
                .child(
                    div()
                        .w(px(42.0))
                        .text_xs()
                        .text_color(colors.muted)
                        .child("MiB/s"),
                ),
        )
}

pub(crate) fn locale_menu(
    app: &mut AlterSendmeApp,
    colors: Palette,
    cx: &mut Context<AlterSendmeApp>,
) -> gpui_kit::Stateful<gpui_kit::Div> {
    let ordered_locales = std::iter::once(app.locale)
        .chain(
            Locale::all()
                .iter()
                .copied()
                .filter(|locale| *locale != app.locale),
        )
        .collect::<Vec<_>>();
    let options = ordered_locales.into_iter().fold(
        div().flex().flex_col().flex_shrink_0().gap_1(),
        |menu, locale| {
            let selected = locale == app.locale;
            let locale_value = locale;
            menu.child(
                div()
                    .id(format!("locale-option-{}", locale.code()))
                    .focusable()
                    .tab_stop(true)
                    .role(A11yRole::ListBoxOption)
                    .aria_label(locale.label())
                    .aria_selected(selected)
                    .flex_shrink_0()
                    .h(px(38.0))
                    .px_3()
                    .flex()
                    .items_center()
                    .justify_between()
                    .rounded_md()
                    .bg(if selected {
                        colors.accent.alpha(0.16)
                    } else {
                        colors.panel
                    })
                    .text_color(if selected { colors.accent } else { colors.text })
                    .cursor_pointer()
                    .hover(move |style| style.bg(colors.panel_alt))
                    .on_click(cx.listener(move |app, _, _, cx| app.select_locale(locale_value, cx)))
                    .child(locale.label())
                    .when(selected, |option| {
                        option.child(
                            div()
                                .text_xs()
                                .font_weight(FontWeight::SEMIBOLD)
                                .child("OK"),
                        )
                    }),
            )
        },
    );

    div()
        .id("locale-menu")
        .occlude()
        .absolute()
        .top(px(58.0))
        .right(px(24.0))
        .w(px(252.0))
        .max_h(px(440.0))
        .flex()
        .flex_col()
        .rounded_md()
        .border_1()
        .border_color(colors.border)
        .bg(colors.panel)
        .shadow_lg()
        .child(
            div()
                .h(px(46.0))
                .flex_shrink_0()
                .px_3()
                .flex()
                .items_center()
                .justify_between()
                .border_b_1()
                .border_color(colors.border)
                .child(
                    div()
                        .text_sm()
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(app.text("language.select")),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(colors.muted)
                        .child(app.locale.code()),
                ),
        )
        .child(
            div()
                .id("locale-options-scroll")
                .role(A11yRole::ListBox)
                .aria_label(app.text("language.select"))
                .flex_1()
                .min_h(px(0.0))
                .overflow_y_scroll()
                .p_2()
                .child(options),
        )
}

/// Builds a compact numbered disclosure so receive guidance never looks like a primary action.
pub(crate) fn receiver_help_panel(app: &AlterSendmeApp, colors: Palette) -> gpui_kit::Div {
    let instructions = [
        app.text("receiver.instruction1"),
        app.text("receiver.instruction2"),
        app.text("receiver.instruction3"),
        app.text("receiver.instruction4"),
        app.text("receiver.instruction5"),
    ];
    let rows = instructions.into_iter().enumerate().fold(
        div().flex().flex_col().gap_2(),
        |rows, (index, instruction)| {
            rows.child(
                div()
                    .flex()
                    .items_start()
                    .gap_2()
                    .child(
                        div()
                            .w(px(18.0))
                            .flex_shrink_0()
                            .text_xs()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(colors.accent)
                            .child(format!("{}.", index + 1)),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.0))
                            .text_xs()
                            .text_color(colors.muted)
                            .child(instruction),
                    ),
            )
        },
    );

    div()
        .p_3()
        .rounded_md()
        .border_1()
        .border_color(colors.border)
        .bg(colors.panel_alt)
        .child(rows)
}

pub(crate) fn ticket_input(
    app: &mut AlterSendmeApp,
    colors: Palette,
    cx: &mut Context<AlterSendmeApp>,
) -> gpui_kit::Stateful<gpui_kit::Div> {
    let entity = cx.entity();
    let focus = app.ticket_focus.clone();
    let click_focus = focus.clone();
    div()
        .id("ticket-input")
        .track_focus(&focus)
        .tab_stop(true)
        .focus_visible(|style| style.border_color(colors.accent))
        .role(A11yRole::TextInput)
        .aria_label(app.text("receiver.ticketLabel"))
        .relative()
        .h(px(78.0))
        .w_full()
        .p_3()
        .rounded_md()
        .border_1()
        .border_color(colors.border)
        .bg(colors.background)
        .cursor_pointer()
        .on_click(cx.listener(move |_, _, window, cx| click_focus.focus(window, cx)))
        .child(gpui_kit::canvas(
            move |bounds, _, _| bounds,
            move |bounds, _, window, cx| {
                window.handle_input(&focus, ElementInputHandler::new(bounds, entity.clone()), cx);
            },
        ))
        .child(
            div()
                .text_xs()
                .truncate()
                .text_color(if app.ticket_input.is_empty() {
                    colors.muted
                } else {
                    colors.text
                })
                .child(if app.ticket_input.is_empty() {
                    app.text("receiver.ticketPlaceholder")
                } else {
                    app.ticket_input.clone()
                }),
        )
}
