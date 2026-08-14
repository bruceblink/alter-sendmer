//! Signed cross-platform updates produced by cargo-packager.

use cargo_packager_updater::{
    Config, Update, WindowsConfig, WindowsUpdateInstallMode, check_update, semver::Version,
    url::Url,
};

pub(crate) const UPDATE_MANIFEST_URL: &str =
    "https://github.com/bruceblink/alter-sendmer/releases/latest/download/latest.json";
const UPDATE_PUBLIC_KEY: &str = "dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXk6IEVDM0IyQTM0MzIzREVENjcKUldSbjdUMHlOQ283N05rYUg5S3pZRmdhNlhKSW5odVJTNnBTeDRYcnl2OUQ0ZlU5bXQ3OFd2dzkK";

/// Keeps the verified release metadata needed by the second, user-confirmed install action.
#[derive(Clone, Debug)]
pub(crate) struct AvailableUpdate {
    pub(crate) version: String,
    update: Update,
}

/// Checks the signed manifest with cargo-packager's platform and architecture selection rules.
pub(crate) fn check_for_update() -> anyhow::Result<Option<AvailableUpdate>> {
    let current_version = Version::parse(env!("CARGO_PKG_VERSION"))?;
    let config = updater_config()?;
    Ok(
        check_update(current_version, config)?.map(|update| AvailableUpdate {
            version: update.version.clone(),
            update,
        }),
    )
}

/// Downloads, verifies, and installs an update using the package format for the current platform.
pub(crate) fn install_update(update: AvailableUpdate) -> anyhow::Result<()> {
    update.update.download_and_install()?;
    Ok(())
}

fn updater_config() -> anyhow::Result<Config> {
    Ok(Config {
        endpoints: vec![Url::parse(UPDATE_MANIFEST_URL)?],
        pubkey: UPDATE_PUBLIC_KEY.to_owned(),
        windows: Some(WindowsConfig {
            installer_args: None,
            install_mode: Some(WindowsUpdateInstallMode::BasicUi),
        }),
    })
}

#[cfg(test)]
mod tests {
    use super::{UPDATE_MANIFEST_URL, UPDATE_PUBLIC_KEY, updater_config};

    #[test]
    fn updater_targets_the_active_repository_with_a_public_key() {
        let config = updater_config().expect("updater config is valid");
        assert_eq!(config.endpoints[0].as_str(), UPDATE_MANIFEST_URL);
        assert!(UPDATE_MANIFEST_URL.contains("bruceblink/alter-sendmer"));
        assert!(!UPDATE_MANIFEST_URL.contains("bruceblink/alter-sendme/"));
        assert_eq!(config.pubkey, UPDATE_PUBLIC_KEY);
        assert!(config.pubkey.len() > 100);
    }
}
