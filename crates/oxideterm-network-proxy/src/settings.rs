// Copyright (C) 2026 AnalyseDeCircuit
// SPDX-License-Identifier: GPL-3.0-only

use oxideterm_settings::{
    PersistedSettings, SettingsApplicationProxyMode, SettingsUpstreamProxyAuth,
    SettingsUpstreamProxyProtocol,
};

use crate::{
    ApplicationProxyAuth, ApplicationProxyCredentialProvider, ApplicationProxyPolicy,
    ApplicationProxyProtocol, CustomApplicationProxy, set_application_proxy_policy,
};

pub fn application_proxy_policy_from_settings(
    settings: &PersistedSettings,
    credentials: &impl ApplicationProxyCredentialProvider,
) -> ApplicationProxyPolicy {
    match settings.network.application_proxy_mode {
        SettingsApplicationProxyMode::System => return ApplicationProxyPolicy::System,
        SettingsApplicationProxyMode::Direct => return ApplicationProxyPolicy::Direct,
        SettingsApplicationProxyMode::Shared => {}
    }
    let Some(proxy) = settings.network.upstream_proxy.as_ref() else {
        return ApplicationProxyPolicy::Unavailable {
            reason: "the shared application proxy is selected but no shared proxy is configured"
                .to_string(),
        };
    };
    let auth = match &proxy.auth {
        SettingsUpstreamProxyAuth::None => ApplicationProxyAuth::None,
        SettingsUpstreamProxyAuth::Password {
            username,
            keychain_id,
        } => {
            let Some(keychain_id) = keychain_id.as_deref() else {
                return ApplicationProxyPolicy::Unavailable {
                    reason: "the application proxy password is not saved".to_string(),
                };
            };
            let password = match credentials.application_proxy_password(keychain_id) {
                Ok(password) => password,
                Err(_) => {
                    return ApplicationProxyPolicy::Unavailable {
                        reason: "the application proxy password is unavailable".to_string(),
                    };
                }
            };
            ApplicationProxyAuth::Password {
                username: username.clone(),
                password,
            }
        }
    };
    ApplicationProxyPolicy::Custom(CustomApplicationProxy {
        protocol: match proxy.protocol {
            SettingsUpstreamProxyProtocol::Socks5 => ApplicationProxyProtocol::Socks5,
            SettingsUpstreamProxyProtocol::HttpConnect => ApplicationProxyProtocol::HttpConnect,
        },
        host: proxy.host.clone(),
        port: proxy.port,
        auth,
        remote_dns: proxy.remote_dns,
        no_proxy: proxy.no_proxy.clone(),
    })
}

pub fn install_application_proxy_policy_from_settings(
    settings: &PersistedSettings,
    credentials: &impl ApplicationProxyCredentialProvider,
) {
    // The process-wide policy is replaced whenever persisted settings or its
    // keychain-backed credential changes.
    set_application_proxy_policy(application_proxy_policy_from_settings(
        settings,
        credentials,
    ));
}
