//! Update check against the GitHub release of this repository, download of its DMG,
//! and restart once the installed bundle has been replaced.
use anyhow::{anyhow, bail, Context, Result};
use semver::Version;
use sha2::{Digest, Sha256};
use statusbadge_core::updates::{latest_release_url, plist_version, Release};
use std::{
    io::{Read, Write},
    path::{Path, PathBuf},
    time::Duration,
};

const USER_AGENT: &str = concat!("StatusBadge/", env!("CARGO_PKG_VERSION"));
/// GitHub allows 60 unauthenticated API requests per hour and per address.
pub const CHECK_INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);
pub const RETRY_INTERVAL: Duration = Duration::from_secs(30 * 60);

fn installed() -> Result<Version> {
    Ok(Version::parse(env!("CARGO_PKG_VERSION"))?)
}

pub fn check() -> Result<Option<Release>> {
    let response = match ureq::AgentBuilder::new()
        .redirects(0)
        .timeout(Duration::from_secs(15))
        .build()
        .get(&latest_release_url())
        .set("Accept", "application/vnd.github+json")
        .set("User-Agent", USER_AGENT)
        .call()
    {
        Ok(response) if response.status() == 200 => response,
        Err(ureq::Error::Status(404, _)) => return Ok(None), // Before the first publication.
        _ => bail!("Vérification des mises à jour indisponible"),
    };
    let mut body = Vec::new();
    response
        .into_reader()
        .take(1024 * 1024)
        .read_to_end(&mut body)?;
    Release::from_github(&body, &installed()?).map_err(|_| anyhow!("Release GitHub invalide"))
}

pub fn download(release: &Release) -> Result<PathBuf> {
    if !release.validate(&installed()?) {
        bail!("Mise à jour invalide");
    }
    let user = directories::UserDirs::new().context("Dossier utilisateur indisponible")?;
    let downloads = user
        .download_dir()
        .context("Dossier Téléchargements indisponible")?;
    let mut temporary = tempfile::NamedTempFile::new_in(downloads)?;
    // GitHub redirects a release file to its storage host. The request carries no
    // credential, and the size, digest and Gatekeeper checks below apply to what arrives.
    let response = ureq::AgentBuilder::new()
        .redirects(5)
        .timeout(Duration::from_secs(300))
        .build()
        .get(&release.url)
        .set("User-Agent", USER_AGENT)
        .call()?;
    if response.status() != 200 {
        bail!("Téléchargement refusé");
    }
    let mut reader = response.into_reader().take(release.size + 1);
    let mut hash = Sha256::new();
    let mut bytes = 0u64;
    let mut buffer = [0; 64 * 1024];
    loop {
        let n = reader.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        bytes += n as u64;
        hash.update(&buffer[..n]);
        temporary.write_all(&buffer[..n])?;
    }
    if bytes != release.size
        || !format!("{:x}", hash.finalize()).eq_ignore_ascii_case(&release.sha256)
    {
        bail!("Le fichier téléchargé ne correspond pas à la release");
    }
    temporary.as_file().sync_all()?;
    // Unique filename: never replace a user's existing file in Downloads.
    let suffix = rand::random::<u32>();
    let destination = downloads.join(format!("StatusBadge-{}-{suffix:08x}.dmg", release.version));
    temporary.persist_noclobber(&destination)?;
    // Gatekeeper checks Developer ID and notarization before the DMG opens.
    let result = std::process::Command::new("/usr/sbin/spctl")
        .args([
            "--assess",
            "--type",
            "open",
            "--context",
            "context:primary-signature",
        ])
        .arg(&destination)
        .output()?;
    if !result.status.success() {
        bail!("La vérification Gatekeeper du DMG a échoué");
    }
    let result = std::process::Command::new("/usr/bin/open")
        .arg(&destination)
        .status()?;
    if !result.success() {
        bail!("Impossible d’ouvrir le DMG");
    }
    Ok(destination)
}

/// The `.app` this process was started from; `None` under `cargo run`.
pub fn running_bundle() -> Option<PathBuf> {
    let executable = std::env::current_exe().ok()?;
    let bundle = executable.parent()?.parent()?.parent()?;
    (bundle.extension()? == "app").then(|| bundle.to_path_buf())
}

/// Whether the bundle on disk is now newer than this process: the user dragged the
/// update over the installed application.
pub fn bundle_was_updated(bundle: &Path) -> bool {
    let Ok(current) = installed() else {
        return false;
    };
    std::fs::read_to_string(bundle.join("Contents/Info.plist"))
        .ok()
        .and_then(|plist| plist_version(&plist))
        .is_some_and(|on_disk| on_disk > current)
}

/// Opens the bundle again once this process has exited, so that the new instance
/// finds the single-instance lock released.
pub fn relaunch_after_exit(bundle: &Path) {
    let result = std::process::Command::new("/bin/sh")
        .args([
            "-c",
            r#"while kill -0 "$1" 2>/dev/null; do sleep 0.2; done; exec /usr/bin/open "$0""#,
        ])
        .arg(bundle)
        .arg(std::process::id().to_string())
        .spawn();
    if result.is_err() {
        tracing::warn!("cannot schedule the restart after the update");
    }
}
