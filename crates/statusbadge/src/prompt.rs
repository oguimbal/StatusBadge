//! URL entry for a new sudo.
//!
//! GPUI has no text field at this revision, and an AppKit modal on the main thread would
//! re-enter GPUI's event loop while the application state is borrowed. A standard
//! AppleScript dialog runs in its own process instead; the text comes back on a channel.
use std::process::{Command, Stdio};

const DIALOG: &str = r#"on run argv
    activate
    set response to display dialog (item 2 of argv) with title (item 1 of argv) default answer "" buttons {"Annuler", "Continuer"} default button "Continuer" cancel button "Annuler"
    return text returned of response
end run"#;

pub fn ask_url(message: &str) -> Option<String> {
    let output = Command::new("/usr/bin/osascript")
        .args(["-e", DIALOG, "Ajouter un sudo", message])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_string())
}
