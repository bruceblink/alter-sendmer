//! AlterSendmer 的票据和上传速率文本输入处理。

use super::{
    AlterSendmeApp, app_model::*, app_persistence::*, byte_to_utf16_range, utf16_to_byte_range,
};
use gpui_kit::{Bounds, Context, EntityInputHandler, UTF16Selection, Window};
use std::ops::Range;

impl EntityInputHandler for AlterSendmeApp {
    fn text_for_range(
        &mut self,
        range: Range<usize>,
        adjusted: &mut Option<Range<usize>>,
        window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<String> {
        let text = if self.upload_rate_focus.is_focused(window) {
            &self.upload_rate_input
        } else {
            &self.ticket_input
        };
        let range = utf16_to_byte_range(text, range);
        *adjusted = Some(byte_to_utf16_range(text, range.clone()));
        Some(text.get(range)?.to_owned())
    }
    fn selected_text_range(
        &mut self,
        _ignore_disabled_input: bool,
        window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        let (text, selection) = if self.upload_rate_focus.is_focused(window) {
            (&self.upload_rate_input, self.upload_rate_selection.clone())
        } else {
            (&self.ticket_input, self.ticket_selection.clone())
        };
        Some(UTF16Selection {
            range: byte_to_utf16_range(text, selection),
            reversed: false,
        })
    }
    fn marked_text_range(
        &self,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Range<usize>> {
        None
    }
    fn unmark_text(&mut self, _window: &mut Window, _cx: &mut Context<Self>) {}
    fn replace_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        text: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.upload_rate_focus.is_focused(window) {
            self.replace_upload_rate(range, text);
        } else {
            self.replace_ticket(range, text);
        }
        cx.notify();
    }
    fn replace_and_mark_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        text: &str,
        selected: Option<Range<usize>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.upload_rate_focus.is_focused(window) {
            self.replace_upload_rate(range, text);
        } else {
            self.replace_ticket(range, text);
        }
        if let Some(selected) = selected {
            if self.upload_rate_focus.is_focused(window) {
                self.upload_rate_selection = utf16_to_byte_range(&self.upload_rate_input, selected);
            } else {
                self.ticket_selection = utf16_to_byte_range(&self.ticket_input, selected);
            }
        }
        cx.notify();
    }
    fn bounds_for_range(
        &mut self,
        _range: Range<usize>,
        bounds: Bounds<gpui_kit::Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Bounds<gpui_kit::Pixels>> {
        Some(bounds)
    }
    fn character_index_for_point(
        &mut self,
        _point: gpui_kit::Point<gpui_kit::Pixels>,
        window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<usize> {
        let text = if self.upload_rate_focus.is_focused(window) {
            &self.upload_rate_input
        } else {
            &self.ticket_input
        };
        Some(text.encode_utf16().count())
    }
    fn accepts_text_input(&self, _window: &mut Window, _cx: &mut Context<Self>) -> bool {
        true
    }
}

impl AlterSendmeApp {
    /// Replaces the selected ticket text only when the resulting UTF-8 byte length stays within
    /// the input limit; rejected edits leave the existing value intact and show a recoverable error.
    fn replace_ticket(&mut self, range: Option<Range<usize>>, text: &str) {
        let range = range
            .map(|range| utf16_to_byte_range(&self.ticket_input, range))
            .unwrap_or_else(|| self.ticket_selection.clone());
        let start = range.start.min(self.ticket_input.len());
        let end = range.end.min(self.ticket_input.len());
        if start > end {
            return;
        }
        if !ticket_replacement_fits_limit(self.ticket_input.len(), end - start, text.len()) {
            self.error = Some(self.ticket_length_error());
            return;
        }
        self.ticket_input.replace_range(start..end, text);
        self.ticket_selection = start + text.len()..start + text.len();
        self.clear_ticket_length_error();
    }

    pub(crate) fn ticket_length_error(&self) -> String {
        self.text("receiver.ticketTooLong")
            .replace("{{max}}", &MAX_TICKET_LEN.to_string())
    }

    /// Clears only the ticket-length warning, leaving unrelated transfer or settings errors visible.
    fn clear_ticket_length_error(&mut self) {
        let message = self.ticket_length_error();
        if self.error.as_deref() == Some(message.as_str()) {
            self.error = None;
        }
    }

    /// Applies one numeric upload-rate edit and persists only a valid, representable value.
    pub(crate) fn replace_upload_rate(&mut self, range: Option<Range<usize>>, text: &str) {
        let range = range
            .map(|range| utf16_to_byte_range(&self.upload_rate_input, range))
            .unwrap_or_else(|| self.upload_rate_selection.clone());
        let start = range.start.min(self.upload_rate_input.len());
        let end = range.end.min(self.upload_rate_input.len());
        if start > end {
            return;
        }
        let mut candidate = self.upload_rate_input.clone();
        candidate.replace_range(start..end, text);
        let Ok(rate) = parse_upload_rate_input(&candidate) else {
            self.error = Some(self.upload_rate_validation_error());
            return;
        };

        self.upload_rate_input = candidate;
        self.upload_rate_selection = start + text.len()..start + text.len();
        self.preferences.max_upload_rate_mib_per_sec = rate;
        persist_preferences(&self.preferences);
        self.error = None;
        self.status = self
            .text("preferences.value")
            .replace("{{name}}", &self.text("preferences.uploadRate"))
            .replace("{{value}}", &self.upload_rate_label());
    }

    fn upload_rate_validation_error(&self) -> String {
        self.text("preferences.uploadRateInvalid")
            .replace("{{max}}", &MAX_UPLOAD_RATE_MIB_PER_SEC.to_string())
    }
}
