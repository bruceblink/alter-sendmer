//! Locale metadata and the generated translations shared with the original app.

include!(concat!(env!("OUT_DIR"), "/locale_data.rs"));

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Locale {
    Arabic,
    Czech,
    German,
    #[default]
    English,
    Spanish,
    Persian,
    French,
    Hindi,
    Italian,
    Japanese,
    Korean,
    Norwegian,
    Polish,
    BrazilianPortuguese,
    Russian,
    Serbian,
    Thai,
    Turkish,
    Ukrainian,
    SimplifiedChinese,
    TraditionalChinese,
}

impl Locale {
    /// Returns every locale supported by the original AlterSendme client.
    pub const fn all() -> &'static [Self] {
        &[
            Self::Arabic,
            Self::Czech,
            Self::German,
            Self::English,
            Self::Spanish,
            Self::Persian,
            Self::French,
            Self::Hindi,
            Self::Italian,
            Self::Japanese,
            Self::Korean,
            Self::Norwegian,
            Self::Polish,
            Self::BrazilianPortuguese,
            Self::Russian,
            Self::Serbian,
            Self::Thai,
            Self::Turkish,
            Self::Ukrainian,
            Self::SimplifiedChinese,
            Self::TraditionalChinese,
        ]
    }

    pub const fn code(self) -> &'static str {
        match self {
            Self::Arabic => "ar",
            Self::Czech => "cs",
            Self::German => "de",
            Self::English => "en",
            Self::Spanish => "es",
            Self::Persian => "fa",
            Self::French => "fr",
            Self::Hindi => "hi",
            Self::Italian => "it",
            Self::Japanese => "ja",
            Self::Korean => "ko",
            Self::Norwegian => "no",
            Self::Polish => "pl",
            Self::BrazilianPortuguese => "pt-BR",
            Self::Russian => "ru",
            Self::Serbian => "sr",
            Self::Thai => "th",
            Self::Turkish => "tr",
            Self::Ukrainian => "uk",
            Self::SimplifiedChinese => "zh-CN",
            Self::TraditionalChinese => "zh-TW",
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Arabic => "العربية",
            Self::Czech => "Čeština",
            Self::German => "Deutsch",
            Self::English => "English",
            Self::Spanish => "Español",
            Self::Persian => "فارسی",
            Self::French => "Français",
            Self::Hindi => "हिन्दी",
            Self::Italian => "Italiano",
            Self::Japanese => "日本語",
            Self::Korean => "한국어",
            Self::Norwegian => "Norsk",
            Self::Polish => "Polski",
            Self::BrazilianPortuguese => "Português (Brasil)",
            Self::Russian => "Русский",
            Self::Serbian => "Српски",
            Self::Thai => "ไทย",
            Self::Turkish => "Türkçe",
            Self::Ukrainian => "Українська",
            Self::SimplifiedChinese => "简体中文",
            Self::TraditionalChinese => "繁體中文",
        }
    }

    pub fn lookup(self, key: &str) -> Option<&'static str> {
        lookup(self.code(), key)
    }

    pub fn ui_copy(self, key: &str) -> Option<&'static str> {
        let source_key = match key {
            "drop" => "sender.dropFilesHere",
            "start" => "sender.startSharing",
            "stop" => "sender.stopSharing",
            "save" => "receiver.saveToFolder",
            "receive_action" => "receive",
            "copy" => "sender.copyToClipboard",
            "new" => "transfer.newTransfer",
            "path_selected_file" => "sender.fileSelected",
            "path_selected_folder" => "sender.folderSelected",
            "preparing" => "sender.preparingForTransport",
            "listening" => "sender.listeningForConnection",
            "sharing" => "sender.sharingInProgress",
            "stopping" => "sender.stoppingTransmission",
            "ticket_copied" => "sender.copyToClipboard",
            "connecting" => "receiver.connectingToSender",
            "downloading" => "receiver.downloadingInProgress",
            "download_completed" => "receiver.downloadCompleted",
            "receive_failed" => "errors.receiveFailed",
            "transfer_completed" => "sender.transferCompleted",
            "transfer_failed" => "errors.sharingFailed",
            "finalizing" => "transfer.complete",
            "open_folder" => "receiver.openFolder",
            "transfer_complete" => "transfer.complete",
            "failed" => "transfer.failed",
            "try_again" => "transfer.tryAgain",
            other => other,
        };
        self.lookup(source_key)
    }
}

#[cfg(test)]
mod tests {
    use super::Locale;

    #[test]
    fn includes_every_original_locale() {
        assert_eq!(Locale::all().len(), 21);
        assert!(Locale::all().iter().any(|locale| locale.code() == "pt-BR"));
        assert!(Locale::all().iter().any(|locale| locale.code() == "zh-TW"));
    }

    #[test]
    fn resolves_nested_common_translations() {
        assert_eq!(
            Locale::English.lookup("sender.startSharing"),
            Some("Start Sharing")
        );
        assert_eq!(
            Locale::SimplifiedChinese.lookup("sender.startSharing"),
            Some("开始共享")
        );
        assert_eq!(
            Locale::Japanese.ui_copy("drop"),
            Some("ファイルまたはフォルダをここにドロップ")
        );
        assert_eq!(Locale::English.ui_copy("open_folder"), Some("Open folder"));
        assert_eq!(
            Locale::SimplifiedChinese.ui_copy("open_folder"),
            Some("打开文件夹")
        );
    }

    #[test]
    fn chinese_shell_navigation_does_not_fall_back_to_english() {
        let required = [
            "theme.label",
            "language.label",
            "language.select",
            "diagnostics.action",
            "history.title",
            "preferences.title",
            "preferences.relay",
            "preferences.retry",
            "preferences.chunk",
            "preferences.uploadRate",
            "preferences.unlimited",
            "preferences.uploadRateInvalid",
            "state.label",
        ];
        for locale in [Locale::SimplifiedChinese, Locale::TraditionalChinese] {
            for key in required {
                let translated = locale.lookup(key).expect("Chinese shell key exists");
                assert_ne!(translated, Locale::English.lookup(key).unwrap());
            }
        }
    }

    #[test]
    fn every_non_english_locale_translates_the_navigation_shell() {
        let required = [
            "theme.label",
            "language.label",
            "language.select",
            "diagnostics.action",
            "history.title",
            "preferences.title",
            "preferences.relay",
            "preferences.retry",
            "preferences.chunk",
            "preferences.uploadRate",
            "preferences.unlimited",
            "preferences.uploadRateInvalid",
            "state.label",
        ];
        for locale in Locale::all()
            .iter()
            .copied()
            .filter(|locale| *locale != Locale::English)
        {
            for key in required {
                let translated = locale.lookup(key).expect("localized shell key exists");
                assert_ne!(
                    translated,
                    Locale::English.lookup(key).unwrap(),
                    "locale {} still falls back for {key}",
                    locale.code()
                );
            }
        }
    }

    #[test]
    fn every_locale_has_the_complete_english_fallback_catalog() {
        let required = [
            "diagnostics.action",
            "history.title",
            "history.clear",
            "preferences.relay",
            "preferences.uploadRate",
            "preferences.unlimited",
            "preferences.uploadRateInvalid",
            "sender.saveTicket",
            "receiver.invalidTicket",
            "transfer.tryAgain",
        ];
        for locale in Locale::all() {
            for key in required {
                assert!(
                    locale.lookup(key).is_some(),
                    "locale {} is missing {key}",
                    locale.code()
                );
            }
        }
    }
}
