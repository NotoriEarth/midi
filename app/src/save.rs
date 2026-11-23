use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use gif_core::output::{Timestamp, gif_file_name};

pub fn save_gif(folder: &Path, at: Timestamp, gif: &[u8]) -> io::Result<PathBuf> {
    fs::create_dir_all(folder)?;
    let name = gif_file_name(at, |name| folder.join(name).exists());
    let path = folder.join(&name);

    let unfinished = folder.join(format!("{name}.part"));
    fs::write(&unfinished, gif)?;
    fs::rename(&unfinished, &path)?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOON: Timestamp = Timestamp { year: 2026, month: 11, day: 22, hour: 12, minute: 0, second: 0 };

    fn empty_folder(name: &str) -> PathBuf {
        let folder = std::env::temp_dir().join(format!("midi-save-test-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&folder);
        folder
    }

    fn names_in(folder: &Path) -> Vec<String> {
        let mut names: Vec<String> =
            fs::read_dir(folder).unwrap().map(|entry| entry.unwrap().file_name().into_string().unwrap()).collect();
        names.sort();
        names
    }

    #[test]
    fn the_gif_lands_in_the_folder_under_its_time() {
        let folder = empty_folder("lands");
        fs::create_dir_all(&folder).unwrap();

        let path = save_gif(&folder, NOON, b"GIF89a-one").unwrap();

        assert_eq!(path, folder.join("midi-20261122-120000.gif"));
        assert_eq!(fs::read(&path).unwrap(), b"GIF89a-one");
    }

    #[test]
    fn a_folder_that_is_not_there_yet_is_made() {
        let folder = empty_folder("made").join("deeper");

        let path = save_gif(&folder, NOON, b"GIF89a").unwrap();

        assert!(path.starts_with(&folder) && path.exists());
    }

    #[test]
    fn an_earlier_file_from_the_same_second_is_kept() {
        let folder = empty_folder("kept");

        let first = save_gif(&folder, NOON, b"first").unwrap();
        let second = save_gif(&folder, NOON, b"second").unwrap();

        assert_eq!(fs::read(first).unwrap(), b"first");
        assert_eq!(fs::read(second).unwrap(), b"second");
        assert_eq!(names_in(&folder), ["midi-20261122-120000-2.gif", "midi-20261122-120000.gif"]);
    }

    #[test]
    fn nothing_half_written_is_left_beside_the_gif() {
        let folder = empty_folder("clean");

        save_gif(&folder, NOON, b"GIF89a").unwrap();

        assert_eq!(names_in(&folder), ["midi-20261122-120000.gif"]);
    }
}
