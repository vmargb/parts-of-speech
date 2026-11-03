// Persistent user preferences (theme + AppSettings), stored as JSON:
//   Linux:   ~/.config/parts-of-speech/settings.json
//   macOS:   ~/Library/Application Support/parts-of-speech/settings.json
//   Windows: %APPDATA%\parts-of-speech\settings.json
//
// JSON so the file survives new settings being added later, fields
// missing from an older file simply take their default value

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use crate::themes::ThemeKind;
use crate::AppSettings;

#[derive(Serialize, Deserialize, Clone, PartialEq, Default)]
#[serde(default)]
pub struct SavedConfig {
    pub theme: ThemeKind,
    pub settings: AppSettings,
}

fn config_path() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("parts-of-speech").join("settings.json"))
}

impl SavedConfig {
    // never fails, a missing, unreadable or corrupt file just yields the defaults
    pub fn load() -> Self {
        config_path()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    // writes to a temp file and rename it into place so a crash
    // mid-write can't leave a half-written config
    pub fn save(&self) {
        let Some(path) = config_path() else { return };
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        match serde_json::to_string_pretty(self) {
            Ok(json) => {
                let tmp = path.with_extension("json.tmp");
                if std::fs::write(&tmp, json).is_ok() {
                    let _ = std::fs::rename(&tmp, &path);
                }
            }
            Err(e) => eprintln!("Failed to serialise settings: {e}"),
        }
    }
}
