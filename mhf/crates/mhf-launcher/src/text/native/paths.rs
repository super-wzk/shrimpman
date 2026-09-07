use std::ptr;

use windows::Win32::System::SystemInformation::GetLocalTime;

use super::{copy_z, read_z};

pub(super) unsafe fn screenshot(base: usize, destination: *mut u8) -> u32 {
    let params = unsafe { ptr::read_volatile((base + 0x0E866C5C) as *const u32) } as usize;
    if params == 0 || destination.is_null() {
        return destination as u32;
    }
    let directory = |offset| unsafe {
        read_z((params + offset) as *const u8, 0x400)
            .and_then(|bytes| std::str::from_utf8(bytes).ok())
    };
    // Both paths are supplied by the launcher's validated launch parameters.
    let Some((game_dir, launcher_dir)) = directory(20).zip(directory(1044)) else {
        unsafe { ptr::write(destination, 0) };
        return destination as u32;
    };
    let _ = std::fs::create_dir(format!("{game_dir}\\スクリーンショット"));
    let now = unsafe { GetLocalTime() };
    let filename = filename(
        launcher_dir,
        [
            now.wYear,
            now.wMonth,
            now.wDay,
            now.wHour,
            now.wMinute,
            now.wSecond,
            now.wMilliseconds,
        ],
    );
    // A partial filename is not useful. Match the native 1024-byte destination.
    let value = if filename.len() < 0x400 {
        filename.as_bytes()
    } else {
        c"".to_bytes()
    };
    unsafe { copy_z(destination, 0x400, value) };
    destination as u32
}

fn filename(directory: &str, time: [u16; 7]) -> String {
    let [year, month, day, hour, minute, second, millisecond] = time;
    format!(
        "{directory}\\スクリーンショット\\mhf_{year:04}{month:02}{day:02}_{hour:02}{minute:02}{second:02}_{millisecond:03}.jpg"
    )
}

#[cfg(test)]
mod tests {
    #[test]
    fn screenshot_filename_is_utf8_and_keeps_the_native_timestamp_format() {
        assert_eq!(
            super::filename("Z:\\游戏", [2026, 9, 7, 3, 4, 5, 6]),
            "Z:\\游戏\\スクリーンショット\\mhf_20260907_030405_006.jpg"
        );
    }
}
