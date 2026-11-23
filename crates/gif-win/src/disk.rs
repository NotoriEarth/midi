use std::os::windows::ffi::OsStrExt;
use std::path::Path;

use windows::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
use windows::core::PCWSTR;

pub fn free_disk_bytes(folder: &Path) -> Option<u64> {
    let folder: Vec<u16> = folder.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
    let mut free_for_this_user = 0;
    unsafe { GetDiskFreeSpaceExW(PCWSTR(folder.as_ptr()), Some(&mut free_for_this_user), None, None) }.ok()?;
    Some(free_for_this_user)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_temp_folder_is_on_a_disk_with_some_room() {
        let free = free_disk_bytes(&std::env::temp_dir());
        assert!(free.is_some_and(|bytes| bytes > 0), "{free:?}");
    }

    #[test]
    fn a_folder_that_does_not_exist_has_no_answer() {
        assert_eq!(free_disk_bytes(Path::new(r"Q:\no\such\folder\anywhere")), None);
    }
}
