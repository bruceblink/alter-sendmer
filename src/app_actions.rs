//! AlterSendmer 的文案、路径、偏好和诊断动作。

use super::*;

impl AlterSendmeApp {
    pub(crate) fn colors(&self, appearance: WindowAppearance) -> Palette {
        let dark = match self.theme {
            Theme::System => matches!(
                appearance,
                WindowAppearance::Dark | WindowAppearance::VibrantDark
            ),
            Theme::Dark => true,
            Theme::Light => false,
        };
        if !dark {
            Palette {
                background: rgb(0xf2f5f9),
                panel: rgb(0xffffff),
                panel_alt: rgb(0xe7edf3),
                text: rgb(0x17201b),
                muted: rgb(0x617068),
                border: rgb(0xd0d9d4),
                accent: rgb(0x159447),
                accent_alt: rgb(0x2675ce),
                danger: rgb(0xb93c3c),
            }
        } else {
            Palette {
                background: rgb(0x191919),
                panel: rgb(0x242424),
                panel_alt: rgb(0x363636),
                text: rgb(0xf5f7f6),
                muted: rgb(0xa6b2ab),
                border: rgb(0x484f4b),
                accent: rgb(0x25d365),
                accent_alt: rgb(0x4e9bea),
                danger: rgb(0xf06b6b),
            }
        }
    }

    pub(crate) fn copy(&self, key: &str) -> &'static str {
        if let Some(value) = self.locale.ui_copy(key) {
            return value;
        }
        match (self.locale, key) {
            (_, "send") => "Send",
            (_, "receive") => "Receive",
            (_, "ready") => "Ready",
            (_, "select") => "Choose a file or folder",
            (_, "drop") => "Drop files or folders here",
            (_, "start") => "Start sharing",
            (_, "stop") => "Stop sharing",
            (_, "save") => "Save to folder",
            (_, "receive_action") => "Start receiving",
            (_, "copy") => "Copy ticket",
            (_, "new") => "New transfer",
            (_, "theme") => "Theme",
            (_, "language") => "Language",
            _ => "",
        }
    }

    pub(crate) fn theme_label(&self) -> String {
        match self.theme {
            Theme::System => self.text("theme.system"),
            Theme::Dark => self.text("theme.dark"),
            Theme::Light => self.text("theme.light"),
        }
    }

    pub(crate) fn relay_label(&self) -> String {
        if self.preferences.relay == "disabled" {
            self.text("preferences.disabled")
        } else {
            self.text("preferences.default")
        }
    }

    pub(crate) fn upload_rate_label(&self) -> String {
        self.preferences
            .max_upload_rate_mib_per_sec
            .map(|rate| format!("{rate} MiB/s"))
            .unwrap_or_else(|| self.text("preferences.unlimited"))
    }

    pub(crate) fn receive_cache_state_label(&self) -> String {
        if self.preferences.receive_cache_enabled {
            self.text("preferences.cacheOn")
        } else {
            self.text("preferences.cacheOff")
        }
    }

    pub(crate) fn receive_cache_ttl_label(&self) -> String {
        self.text("preferences.cacheDays").replace(
            "{{count}}",
            &self.preferences.receive_cache_ttl_days.to_string(),
        )
    }

    pub(crate) fn max_receivers_label(&self) -> String {
        match self.preferences.max_receivers {
            Some(count) => self
                .text("preferences.receiverCount")
                .replace("{{count}}", &count.to_string()),
            None => self.text("preferences.unlimited"),
        }
    }

    pub(crate) fn session_lifetime_label(&self) -> String {
        match self.preferences.session_lifetime_seconds {
            None => self.text("preferences.unlimited"),
            Some(seconds) if seconds % (60 * 60) == 0 => self
                .text("preferences.sessionHours")
                .replace("{{count}}", &(seconds / (60 * 60)).to_string()),
            Some(seconds) => self
                .text("preferences.sessionMinutes")
                .replace("{{count}}", &(seconds / 60).to_string()),
        }
    }

    pub(crate) fn manifest_mode_label(&self) -> String {
        if self.preferences.manifest_mode {
            self.text("preferences.manifestOn")
        } else {
            self.text("preferences.manifestOff")
        }
    }

    pub(crate) fn text(&self, key: &str) -> String {
        self.locale
            .lookup(key)
            .or_else(|| Locale::English.lookup(key))
            .unwrap_or(key)
            .to_owned()
    }

    pub(crate) fn status_text(&self, key: &str, fallback: &str) -> String {
        let value = self.text(key);
        if value == key {
            fallback.to_owned()
        } else {
            value
        }
    }

    /// Reports whether the sender completion card represents an interrupted transfer.
    pub(crate) fn is_send_stopped(&self) -> bool {
        self.completed_stopped
    }

    pub(crate) fn select_path(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        if path.exists() {
            self.selected_is_dir = path.is_dir();
            self.selected_path = Some(path);
            self.send_progress = Progress {
                total_files: self
                    .selected_path
                    .as_ref()
                    .map(path_file_count)
                    .unwrap_or_default(),
                ..Progress::default()
            };
            self.error = None;
            self.status = self.status_text(
                if self.selected_is_dir {
                    "path_selected_folder"
                } else {
                    "path_selected_file"
                },
                "Path selected",
            );
            cx.notify();
        }
    }

    pub(crate) fn choose_send_path(&mut self, cx: &mut Context<Self>) {
        let prompt = self.text("sender.browseFile");
        // Windows 的系统选择器在同时启用文件和文件夹时会强制切换为文件夹模式。
        // 读取平台能力后关闭混合模式，确保文件选择仍然可用。
        let directories = cx.can_select_mixed_files_and_dirs();
        let prompt = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories,
            multiple: false,
            prompt: Some(prompt.into()),
        });
        let app = cx.entity().downgrade();
        cx.spawn(move |_, cx: &mut AsyncApp| {
            let mut cx = cx.clone();
            async move {
                if let Ok(Ok(Some(paths))) = prompt.await
                    && let Some(path) = paths.into_iter().next()
                {
                    let _ = app.update(&mut cx, |app, cx| app.select_path(path, cx));
                }
            }
        })
        .detach();
    }

    pub(crate) fn choose_send_folder(&mut self, cx: &mut Context<Self>) {
        let prompt = self.text("transfer.folder");
        let prompt = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some(prompt.into()),
        });
        let app = cx.entity().downgrade();
        cx.spawn(move |_, cx: &mut AsyncApp| {
            let mut cx = cx.clone();
            async move {
                if let Ok(Ok(Some(paths))) = prompt.await
                    && let Some(path) = paths.into_iter().next()
                {
                    let _ = app.update(&mut cx, |app, cx| app.select_path(path, cx));
                }
            }
        })
        .detach();
    }

    pub(crate) fn choose_save_path(&mut self, cx: &mut Context<Self>) {
        let prompt = self.text("receiver.saveToFolder");
        let prompt = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some(prompt.into()),
        });
        let app = cx.entity().downgrade();
        cx.spawn(move |_, cx: &mut AsyncApp| {
            let mut cx = cx.clone();
            async move {
                if let Ok(Ok(Some(paths))) = prompt.await
                    && let Some(path) = paths.into_iter().next()
                {
                    let _ = app.update(&mut cx, |app, cx| {
                        app.save_path = path;
                        app.status = app.status_text("save", "Download folder selected");
                        cx.notify();
                    });
                }
            }
        })
        .detach();
    }

    /// Opens a completed download in the platform file manager.
    pub(crate) fn reveal_completed_path(&mut self, cx: &mut Context<Self>) {
        if let Some(path) = self.completed_path.as_ref() {
            // Windows Explorer needs a direct folder path for this action. Opening the
            // containing directory also handles received files whose parent has non-ASCII
            // characters and avoids selecting a directory from its own parent.
            let folder = if path.is_dir() {
                path.clone()
            } else {
                path.parent().unwrap_or(path).to_path_buf()
            };
            cx.open_with_system(&folder);
            self.status = self.status_text("download_completed", "Opened folder");
            cx.notify();
        }
    }

    /// Checks the signed release manifest off the UI thread; quiet startup checks only report updates.
    pub(crate) fn check_for_updates(&mut self, report_no_update: bool, cx: &mut Context<Self>) {
        if self.update_checking || self.update_installing {
            return;
        }
        self.update_checking = true;
        if report_no_update {
            self.update_status = Some(self.text("update.checking"));
        }
        cx.notify();
        let app = cx.entity().downgrade();
        let check = cx.background_spawn(async { updater::check_for_update() });
        cx.spawn(move |_, cx: &mut AsyncApp| {
            let mut cx = cx.clone();
            async move {
                let result = check.await;
                let _ = app.update(&mut cx, |app, cx| {
                    app.update_checking = false;
                    match result {
                        Ok(Some(info)) => {
                            app.update_status = Some(
                                app.text("update.found")
                                    .replace("{{version}}", &info.version),
                            );
                            app.update_info = Some(info);
                        }
                        Ok(None) => {
                            app.update_info = None;
                            app.update_status =
                                report_no_update.then(|| app.text("update.upToDate"));
                        }
                        Err(error) => {
                            app.update_info = None;
                            app.update_status = report_no_update
                                .then(|| format!("{}: {error}", app.text("update.failed")));
                        }
                    }
                    cx.notify();
                });
            }
        })
        .detach();
    }

    /// Installs a verified update or starts an explicit update check when none is cached.
    pub(crate) fn open_update_or_check(&mut self, cx: &mut Context<Self>) {
        if self.update_installing || self.update_checking {
            return;
        }
        if let Some(info) = self.update_info.clone() {
            self.update_installing = true;
            self.update_status = Some(self.text("update.downloading"));
            let app = cx.entity().downgrade();
            let install = cx.background_spawn(async move { updater::install_update(info) });
            cx.spawn(move |_, cx: &mut AsyncApp| {
                let mut cx = cx.clone();
                async move {
                    let result = install.await;
                    let _ = app.update(&mut cx, |app, cx| {
                        app.update_installing = false;
                        match result {
                            Ok(()) => {
                                app.update_info = None;
                                app.update_status = Some(app.text("update.installed"));
                            }
                            Err(error) => {
                                app.update_status =
                                    Some(format!("{}: {error}", app.text("update.failed")));
                            }
                        }
                        cx.notify();
                    });
                }
            })
            .detach();
        } else if !self.update_checking {
            self.check_for_updates(true, cx);
        }
        cx.notify();
    }

    /// Opens the original project's sponsorship page without coupling the core transfer crate to it.
    pub(crate) fn open_sponsor_page(&mut self, cx: &mut Context<Self>) {
        cx.open_url("https://buymeacoffee.com/bruceblink");
        self.status = self.status_text("donate", "Donation page opened");
        cx.notify();
    }
}
