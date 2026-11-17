use std::path::Path;

use windows::core::PCWSTR;
use windows::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;

/// Free bytes available to the current user on the volume that contains
/// `path`. Read-only; never creates or modifies files.
pub fn free_space(path: &Path) -> Result<u64, String> {
    let wide: Vec<u16> = path
        .as_os_str()
        .to_string_lossy()
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    let mut available = 0u64;
    unsafe { GetDiskFreeSpaceExW(PCWSTR(wide.as_ptr()), Some(&mut available), None, None) }
        .map_err(|error| {
            format!(
                "Failed to read free disk space for {}: {error}",
                path.display()
            )
        })?;
    Ok(available)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_free_space_for_existing_temp_directory() {
        let temp = tempfile::tempdir().unwrap();
        let free = free_space(temp.path()).expect("free space must be readable");
        assert!(free > 0);
    }
}
