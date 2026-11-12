//! Reading of the GitHub release that carries the update. No network access here.
use semver::Version;
use serde::Deserialize;

/// Repository whose releases are published by `.github/workflows/release.yml`.
/// Exact spelling: GitHub repeats it in download addresses, which are compared as text.
pub const REPOSITORY: &str = "oguimbal/StatusBadge";
/// No version in the file name: `releases/latest/download/StatusBadge.dmg` stays valid.
pub const DMG_NAME: &str = "StatusBadge.dmg";
const MAX_DMG_SIZE: u64 = 512 * 1024 * 1024;

pub fn latest_release_url() -> String {
    format!("https://api.github.com/repos/{REPOSITORY}/releases/latest")
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Release {
    pub version: Version,
    pub url: String,
    pub sha256: String,
    pub size: u64,
}

#[derive(Debug, PartialEq, Eq)]
pub struct InvalidRelease;

#[derive(Deserialize)]
struct GithubRelease {
    tag_name: String,
    draft: bool,
    prerelease: bool,
    assets: Vec<GithubAsset>,
}
#[derive(Deserialize)]
struct GithubAsset {
    name: String,
    browser_download_url: String,
    size: u64,
    /// `sha256:<hex>`, computed by GitHub when the file is uploaded.
    digest: Option<String>,
}

impl Release {
    /// The update described by the body of `releases/latest`.
    ///
    /// `Ok(None)`: nothing newer, or the release does not have its DMG yet (the workflow
    /// creates the release, then uploads the file). `Err`: a newer release whose DMG is
    /// not the expected file of this repository, or has no usable digest.
    pub fn from_github(body: &[u8], installed: &Version) -> Result<Option<Self>, InvalidRelease> {
        let release: GithubRelease = serde_json::from_slice(body).map_err(|_| InvalidRelease)?;
        if release.draft || release.prerelease {
            return Ok(None);
        }
        let tag = release.tag_name.as_str();
        let version =
            Version::parse(tag.strip_prefix('v').unwrap_or(tag)).map_err(|_| InvalidRelease)?;
        if version <= *installed || !version.pre.is_empty() || !version.build.is_empty() {
            return Ok(None);
        }
        let Some(asset) = release.assets.into_iter().find(|a| a.name == DMG_NAME) else {
            return Ok(None);
        };
        let update = Self {
            version,
            url: asset.browser_download_url,
            sha256: asset
                .digest
                .as_deref()
                .and_then(|digest| digest.strip_prefix("sha256:"))
                .ok_or(InvalidRelease)?
                .to_string(),
            size: asset.size,
        };
        update
            .validate(installed)
            .then_some(Some(update))
            .ok_or(InvalidRelease)
    }

    pub fn validate(&self, installed: &Version) -> bool {
        self.version > *installed
            && self.version.pre.is_empty()
            && self.version.build.is_empty()
            && self.url
                == format!(
                    "https://github.com/{REPOSITORY}/releases/download/v{}/{DMG_NAME}",
                    self.version
                )
            && self.sha256.len() == 64
            && self.sha256.bytes().all(|b| b.is_ascii_hexdigit())
            && self.size > 0
            && self.size <= MAX_DMG_SIZE
    }
}

/// `CFBundleShortVersionString` of an XML `Info.plist`.
pub fn plist_version(plist: &str) -> Option<Version> {
    let after_key = plist.split_once("<key>CFBundleShortVersionString</key>")?.1;
    let value = after_key
        .split_once("<string>")?
        .1
        .split_once("</string>")?
        .0;
    Version::parse(value.trim()).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};

    fn installed() -> Version {
        Version::new(0, 1, 9)
    }
    fn github(tag: &str) -> Value {
        json!({
            "tag_name": tag,
            "draft": false,
            "prerelease": false,
            "body": "notes",
            "assets": [{
                "name": DMG_NAME,
                "browser_download_url":
                    format!("https://github.com/{REPOSITORY}/releases/download/{tag}/{DMG_NAME}"),
                "size": 10,
                "digest": format!("sha256:{}", "a".repeat(64)),
            }],
        })
    }
    fn read(release: &Value) -> Result<Option<Release>, InvalidRelease> {
        Release::from_github(release.to_string().as_bytes(), &installed())
    }

    #[test]
    fn announces_a_newer_release_with_its_dmg() {
        let update = read(&github("v0.1.10")).unwrap().unwrap();
        assert_eq!(update.version, Version::new(0, 1, 10));
        assert_eq!(
            update.url,
            "https://github.com/oguimbal/StatusBadge/releases/download/v0.1.10/StatusBadge.dmg"
        );
        assert_eq!(update.sha256, "a".repeat(64));
        assert_eq!(update.size, 10);
    }

    #[test]
    fn ignores_current_older_unfinished_and_unpublished_releases() {
        assert_eq!(read(&github("v0.1.9")), Ok(None));
        assert_eq!(read(&github("v0.1.2")), Ok(None));
        assert_eq!(read(&github("v0.2.0-beta.1")), Ok(None));
        let mut uploading = github("v0.2.0");
        uploading["assets"] = json!([]);
        assert_eq!(read(&uploading), Ok(None));
        for flag in ["draft", "prerelease"] {
            let mut release = github("v0.2.0");
            release[flag] = json!(true);
            assert_eq!(read(&release), Ok(None));
        }
    }

    #[test]
    fn refuses_a_download_that_is_not_this_repository_dmg() {
        for url in [
            "https://example.com/StatusBadge.dmg".to_string(),
            format!("https://github.com/{REPOSITORY}/releases/download/v0.1.0/{DMG_NAME}"),
            format!("https://github.com/{REPOSITORY}/releases/download/v0.2.0/{DMG_NAME}?x=1"),
            format!("https://github.com/oguimbal/statusbadge/releases/download/v0.2.0/{DMG_NAME}"),
        ] {
            let mut release = github("v0.2.0");
            release["assets"][0]["browser_download_url"] = json!(url);
            assert_eq!(read(&release), Err(InvalidRelease));
        }
    }

    #[test]
    fn refuses_a_dmg_without_a_usable_digest_or_size() {
        for (field, value) in [
            ("digest", Value::Null),
            ("digest", json!("sha512:abcd")),
            ("digest", json!(format!("sha256:{}", "z".repeat(64)))),
            ("size", json!(0)),
            ("size", json!(MAX_DMG_SIZE + 1)),
        ] {
            let mut release = github("v0.2.0");
            release["assets"][0][field] = value;
            assert_eq!(read(&release), Err(InvalidRelease));
        }
        assert_eq!(
            Release::from_github(br#"{"message":"Not Found"}"#, &installed()),
            Err(InvalidRelease)
        );
        assert_eq!(read(&github("nightly")), Err(InvalidRelease));
    }

    #[test]
    fn reads_the_bundle_version() {
        let plist = include_str!("../../../bundle/Info.plist");
        assert_eq!(
            plist_version(plist),
            Version::parse(env!("CARGO_PKG_VERSION")).ok()
        );
        assert_eq!(plist_version("<dict></dict>"), None);
    }
}
