use std::fs;
use std::io;
use std::path::Path;

use gif_core::fit::EncodeSettings;
use gif_core::resize::OutputSize;

use crate::choice::QualityChoice;
use crate::sample::parse_fps;

#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct RecordSettings {
    pub fps: u32,
    pub quality: QualityChoice,
    pub fit_within: Option<(u32, u32)>,
}

impl Default for RecordSettings {
    fn default() -> Self {
        Self { fps: 30, quality: QualityChoice::default(), fit_within: None }
    }
}

impl RecordSettings {
    pub fn output_size(self) -> OutputSize {
        match self.fit_within {
            Some((width, height)) => OutputSize::FitWithin { width, height },
            None => OutputSize::Original,
        }
    }

    pub fn encode_settings(self) -> Result<EncodeSettings, String> {
        self.quality.settings(self.fit_within.is_some())
    }

    pub fn can_record(self) -> bool {
        parse_fps(self.fps).is_ok()
            && self.output_size().check().is_ok()
            && self.encode_settings().is_ok_and(|settings| settings.check().is_ok())
    }
}

pub fn load(file: &Path) -> RecordSettings {
    fs::read_to_string(file)
        .ok()
        .and_then(|text| serde_json::from_str::<RecordSettings>(&text).ok())
        .filter(|settings| settings.can_record())
        .unwrap_or_default()
}

pub fn store(file: &Path, settings: RecordSettings) -> io::Result<()> {
    if let Some(folder) = file.parent() {
        fs::create_dir_all(folder)?;
    }
    let unfinished = file.with_extension("json.part");
    fs::write(&unfinished, serde_json::to_string_pretty(&settings)?)?;
    fs::rename(&unfinished, file)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn empty_folder(name: &str) -> PathBuf {
        let folder = std::env::temp_dir().join(format!("midi-settings-test-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&folder);
        folder
    }

    fn file_in(folder: &Path) -> PathBuf {
        folder.join("settings.json")
    }

    fn chosen() -> RecordSettings {
        RecordSettings {
            fps: 15,
            quality: QualityChoice { level: Some(3), max_bytes: Some(2 * 1024 * 1024), dither: 0.35 },
            fit_within: None,
        }
    }

    fn loaded_from(name: &str, text: &str) -> RecordSettings {
        let folder = empty_folder(name);
        fs::create_dir_all(&folder).unwrap();
        fs::write(file_in(&folder), text).unwrap();
        load(&file_in(&folder))
    }

    #[test]
    fn the_first_run_starts_from_the_defaults() {
        let settings = load(&file_in(&empty_folder("first-run")));

        assert_eq!(settings, RecordSettings::default());
        assert_eq!(settings.fps, 30);
        assert_eq!(settings.fit_within, None);
        assert_eq!(settings.quality, QualityChoice { level: None, max_bytes: None, dither: 1.0 });
    }

    #[test]
    fn what_was_stored_comes_back() {
        let file = file_in(&empty_folder("comes-back"));

        store(&file, chosen()).unwrap();

        assert_eq!(load(&file), chosen());
    }

    #[test]
    fn a_fixed_size_comes_back() {
        let file = file_in(&empty_folder("fixed-size"));
        let fixed = RecordSettings { fit_within: Some((480, 270)), ..RecordSettings::default() };

        store(&file, fixed).unwrap();

        assert_eq!(load(&file), fixed);
    }

    #[test]
    fn storing_again_replaces_the_earlier_settings_and_leaves_nothing_else() {
        let folder = empty_folder("replaces");
        let file = file_in(&folder);

        store(&file, RecordSettings { fps: 10, ..chosen() }).unwrap();
        store(&file, chosen()).unwrap();

        assert_eq!(load(&file), chosen());
        let names: Vec<_> = fs::read_dir(&folder).unwrap().map(|entry| entry.unwrap().file_name()).collect();
        assert_eq!(names, ["settings.json"]);
    }

    #[test]
    fn a_file_that_cannot_be_read_as_settings_gives_the_defaults() {
        assert_eq!(loaded_from("not-json", "fps = 15"), RecordSettings::default());
        assert_eq!(loaded_from("wrong-type", r#"{"fps": "fast"}"#), RecordSettings::default());
    }

    #[test]
    fn a_setting_missing_from_an_older_file_takes_its_default_and_the_rest_are_kept() {
        let settings = loaded_from("older", r#"{"fps": 10, "quality": {"level": 4}}"#);

        let expected = RecordSettings {
            fps: 10,
            quality: QualityChoice { level: Some(4), max_bytes: None, dither: 1.0 },
            fit_within: None,
        };
        assert_eq!(settings, expected);
    }

    #[test]
    fn settings_that_recording_would_refuse_give_the_defaults() {
        for refused in [
            r#"{"fps": 20}"#,
            r#"{"quality": {"level": 9}}"#,
            r#"{"quality": {"dither": 3}}"#,
            r#"{"quality": {"maxBytes": 0}}"#,
            r#"{"fitWithin": [0, 270]}"#,
            r#"{"fitWithin": [480, 270], "quality": {"level": 2, "maxBytes": 5000}}"#,
        ] {
            assert_eq!(loaded_from("refused", refused), RecordSettings::default(), "{refused}");
        }
    }

    #[test]
    fn can_record_agrees_with_what_recording_accepts() {
        assert!(RecordSettings::default().can_record());
        assert!(chosen().can_record());
        assert!(!RecordSettings { fps: 20, ..chosen() }.can_record());
        assert!(!RecordSettings { fit_within: Some((480, 0)), ..chosen() }.can_record());
    }
}
