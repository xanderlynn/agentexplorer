//! Updates and rollbacks. The release list comes from GitHub; installing a
//! version uses that release's signed `latest.json` (published by
//! release.yml), so any listed release can be installed, newer or older.
//! Only the public release list and the chosen release's files are fetched;
//! nothing about the user is sent.

use std::time::Duration;

use anyhow::{Context, Result, anyhow};
use semver::Version;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Url};
use tauri_plugin_updater::UpdaterExt;

const REPO: &str = "xanderlynn/tilly";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReleaseInfo {
    pub version: String,
    pub published_at: Option<String>,
    /// The release notes, in Markdown.
    pub notes: String,
    pub url: String,
    /// "newer", "current" or "previous".
    pub relation: &'static str,
    /// Signed update files exist, so it can be installed from the app.
    pub installable: bool,
}

#[derive(Deserialize)]
struct GhRelease {
    tag_name: String,
    draft: bool,
    prerelease: bool,
    published_at: Option<String>,
    body: Option<String>,
    html_url: String,
    assets: Vec<GhAsset>,
}

#[derive(Deserialize)]
struct GhAsset {
    name: String,
}

fn current_version(app: &AppHandle) -> Version {
    app.package_info().version.clone()
}

/// The releases you can move to: every one newer than this build, this
/// build, and the one before it (to roll back). Newest first. Pre-releases
/// are listed only while running one.
pub fn list(app: &AppHandle) -> Result<Vec<ReleaseInfo>> {
    let current = current_version(app);
    let raw: Vec<GhRelease> = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(20))
        .user_agent(format!("Tilly/{current}"))
        .build()?
        .get(format!(
            "https://api.github.com/repos/{REPO}/releases?per_page=100"
        ))
        .header("Accept", "application/vnd.github+json")
        .send()?
        .error_for_status()
        .context("GitHub releases")?
        .json()?;
    Ok(pick(raw, &current))
}

fn pick(raw: Vec<GhRelease>, current: &Version) -> Vec<ReleaseInfo> {
    let mut all: Vec<(Version, GhRelease)> = raw
        .into_iter()
        .filter(|r| !r.draft && (!r.prerelease || !current.pre.is_empty()))
        .filter_map(|r| Some((Version::parse(r.tag_name.trim_start_matches('v')).ok()?, r)))
        .collect();
    all.sort_by(|a, b| b.0.cmp(&a.0));
    let previous = all.iter().position(|(v, _)| v < current);
    all.into_iter()
        .enumerate()
        .filter_map(|(i, (v, r))| {
            let relation = if v > *current {
                "newer"
            } else if v == *current {
                "current"
            } else if Some(i) == previous {
                "previous"
            } else {
                return None;
            };
            Some(ReleaseInfo {
                version: v.to_string(),
                published_at: r.published_at,
                notes: r.body.unwrap_or_default(),
                url: r.html_url,
                relation,
                installable: r.assets.iter().any(|a| a.name == "latest.json"),
            })
        })
        .collect()
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct Progress {
    downloaded: u64,
    total: Option<u64>,
}

/// Downloads, verifies and installs `version` (newer or older than this
/// build), reporting `update-progress` events, then restarts the app.
pub async fn install(app: AppHandle, version: String) -> Result<()> {
    let url: Url =
        format!("https://github.com/{REPO}/releases/download/v{version}/latest.json").parse()?;
    let update = app
        .updater_builder()
        .endpoints(vec![url])?
        // Any version but this one, so older releases install too.
        .version_comparator(|current, remote| remote.version != current)
        .build()?
        .check()
        .await?
        .ok_or_else(|| anyhow!("Version {version} is already installed"))?;
    let mut downloaded = 0u64;
    update
        .download_and_install(
            |chunk, total| {
                downloaded += chunk as u64;
                let _ = app.emit("update-progress", Progress { downloaded, total });
            },
            || {},
        )
        .await?;
    tracing::info!("installed version {version}; restarting");
    app.restart()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rel(tag: &str, pre: bool, latest: bool) -> GhRelease {
        GhRelease {
            tag_name: tag.into(),
            draft: false,
            prerelease: pre,
            published_at: None,
            body: None,
            html_url: String::new(),
            assets: if latest {
                vec![GhAsset {
                    name: "latest.json".into(),
                }]
            } else {
                vec![]
            },
        }
    }

    #[test]
    fn lists_newer_current_and_one_previous() {
        let raw = vec![
            rel("v0.1.0", false, false),
            rel("v0.3.0", false, true),
            rel("v0.4.0-beta.1", true, true),
            rel("v0.2.0", false, true),
            rel("v0.1.5", false, true),
            rel("v0.2.1", false, true),
        ];
        let got: Vec<_> = pick(raw, &Version::parse("0.2.0").unwrap())
            .into_iter()
            .map(|r| (r.version, r.relation))
            .collect();
        assert_eq!(
            got,
            [
                ("0.3.0".to_string(), "newer"),
                ("0.2.1".to_string(), "newer"),
                ("0.2.0".to_string(), "current"),
                ("0.1.5".to_string(), "previous"),
            ]
        );
    }

    #[test]
    fn pre_releases_only_for_pre_release_builds() {
        let raw = || vec![rel("v0.3.0-beta.1", true, true), rel("v0.2.0", false, true)];
        assert_eq!(pick(raw(), &Version::parse("0.2.0").unwrap()).len(), 1);
        assert_eq!(
            pick(raw(), &Version::parse("0.3.0-beta.0").unwrap()).len(),
            2
        );
    }
}
