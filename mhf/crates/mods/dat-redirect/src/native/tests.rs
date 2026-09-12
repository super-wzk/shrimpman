use super::*;
use std::{
    ffi::CString,
    fs::{self, File, OpenOptions},
    io::Read,
    os::windows::io::FromRawHandle,
    time::{SystemTime, UNIX_EPOCH},
};
use windows_sys::Win32::{
    Foundation::{ERROR_FILE_NOT_FOUND, GENERIC_WRITE},
    Storage::FileSystem::{CREATE_ALWAYS, FILE_ATTRIBUTE_NORMAL, FILE_SHARE_READ},
};

fn read_a(path: &Path) -> String {
    // The temporary source path is ASCII; the replacement root is Unicode.
    let path = CString::new(path.to_str().unwrap()).unwrap();
    let handle = unsafe {
        FileSystem::CreateFileA(
            path.as_ptr().cast(),
            GENERIC_READ,
            FILE_SHARE_READ,
            std::ptr::null(),
            OPEN_EXISTING,
            FILE_ATTRIBUTE_NORMAL,
            std::ptr::null_mut(),
        )
    };
    assert_ne!(handle, INVALID_HANDLE_VALUE);
    let mut file = unsafe { File::from_raw_handle(handle) };
    let mut text = String::new();
    file.read_to_string(&mut text).unwrap();
    text
}

fn open_w(open: CreateFileW, path: &Path) -> (Option<File>, u32) {
    let wide: Vec<_> = path.as_os_str().encode_wide().chain([0]).collect();
    let (handle, error) = unsafe {
        SetLastError(0x1234);
        let handle = open(
            wide.as_ptr(),
            GENERIC_READ,
            FILE_SHARE_READ,
            std::ptr::null(),
            OPEN_EXISTING,
            FILE_ATTRIBUTE_NORMAL,
            std::ptr::null_mut(),
        );
        (handle, GetLastError())
    };
    (
        (handle != INVALID_HANDLE_VALUE).then(|| unsafe { File::from_raw_handle(handle) }),
        error,
    )
}

#[test]
fn redirects_native_reads_falls_back_and_restores_after_uninstall() {
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let directory = std::env::temp_dir().join(format!("mhf-dat-redirect-{suffix}"));
    let game = directory.join("game");
    let dat = game.join("dat");
    let root = directory.join("替换资源");
    fs::create_dir_all(dat.join("em")).unwrap();
    fs::create_dir_all(root.join("em")).unwrap();
    fs::create_dir_all(game.join("database/em")).unwrap();
    let source = dat.join("em/model.bin");
    let replacement = root.join("em/model.bin");
    fs::write(&source, "original").unwrap();
    fs::write(&replacement, "replacement").unwrap();
    fs::write(dat.join("missing.bin"), "fallback").unwrap();
    fs::write(root.join("new.bin"), "new resource").unwrap();
    fs::write(game.join("database/em/model.bin"), "outside").unwrap();
    fs::write(game.join("outside.bin"), "outside").unwrap();
    fs::write(root.join("outside.bin"), "must not escape").unwrap();
    fs::write(dat.join("中文.bin"), "原文件").unwrap();
    fs::write(root.join("中文.bin"), "替换文件").unwrap();
    fs::write(dat.join("directory.bin"), "original file").unwrap();
    fs::create_dir(root.join("directory.bin")).unwrap();

    for _ in 0..2 {
        let mut hooks = install(Paths::new(&game, &root).unwrap()).unwrap();
        // Rust File exercises CreateFileW; the explicit call exercises CreateFileA.
        assert_eq!(fs::read_to_string(&source).unwrap(), "replacement");
        assert_eq!(read_a(&source), "replacement");
        {
            let invocation = STATE.enter();
            let original = invocation.state().unwrap().open_w;
            let (direct, direct_error) = open_w(original, &replacement);
            let (redirected, redirected_error) = open_w(FileSystem::CreateFileW, &source);
            assert!(direct.is_some() && redirected.is_some());
            assert_eq!(redirected_error, direct_error);

            // A verbatim trailing dot names a different file. An ordinary
            // override root must not silently open model.bin in its place.
            let mut literal = OsString::from(r"\\?\");
            literal.push(source.as_os_str());
            literal.push(".");
            let literal = PathBuf::from(literal);
            let (direct, direct_error) = open_w(original, &literal);
            let (redirected, redirected_error) = open_w(FileSystem::CreateFileW, &literal);
            assert!(direct.is_none() && redirected.is_none());
            assert_eq!(redirected_error, direct_error);
        }
        assert_eq!(
            fs::read_to_string(game.join("DAT/em/model.bin")).unwrap(),
            "replacement"
        );
        assert_eq!(
            fs::read_to_string(dat.join("em/../em/model.bin")).unwrap(),
            "replacement"
        );
        assert_eq!(
            fs::read_to_string(dat.join("中文.bin")).unwrap(),
            "替换文件"
        );
        assert_eq!(
            fs::read_to_string(dat.join("missing.bin")).unwrap(),
            "fallback"
        );
        assert_eq!(
            fs::read_to_string(dat.join("new.bin")).unwrap(),
            "new resource"
        );
        assert_eq!(
            fs::read_to_string(dat.join("directory.bin")).unwrap(),
            "original file"
        );
        assert_eq!(
            fs::read_to_string(game.join("database/em/model.bin")).unwrap(),
            "outside"
        );
        assert_eq!(
            fs::read_to_string(dat.join("../outside.bin")).unwrap(),
            "outside"
        );

        // Replacement exists but is inaccessible under the requested sharing mode.
        let wide: Vec<_> = replacement.as_os_str().encode_wide().chain([0]).collect();
        let locked = unsafe {
            FileSystem::CreateFileW(
                wide.as_ptr(),
                GENERIC_READ,
                0,
                std::ptr::null(),
                OPEN_EXISTING,
                FILE_ATTRIBUTE_NORMAL,
                std::ptr::null_mut(),
            )
        };
        assert_ne!(locked, INVALID_HANDLE_VALUE);
        let locked = unsafe { File::from_raw_handle(locked) };
        assert_eq!(read_a(&source), "original");
        assert_eq!(fs::read_to_string(&source).unwrap(), "original");
        drop(locked);

        let missing = dat.join("absent.bin");
        let wide: Vec<_> = missing.as_os_str().encode_wide().chain([0]).collect();
        let result = unsafe {
            FileSystem::CreateFileW(
                wide.as_ptr(),
                GENERIC_READ,
                FILE_SHARE_READ,
                std::ptr::null(),
                OPEN_EXISTING,
                FILE_ATTRIBUTE_NORMAL,
                std::ptr::null_mut(),
            )
        };
        let error = unsafe { GetLastError() };
        assert_eq!(result, INVALID_HANDLE_VALUE);
        assert_eq!(error, ERROR_FILE_NOT_FOUND);

        let mut writable = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&source)
            .unwrap();
        let mut text = String::new();
        writable.read_to_string(&mut text).unwrap();
        assert_eq!(text, "original");
        drop(writable);
        fs::write(&source, "written original").unwrap();
        assert_eq!(fs::read_to_string(&replacement).unwrap(), "replacement");

        hooks.uninstall().unwrap();
        assert_eq!(fs::read_to_string(&source).unwrap(), "written original");
        assert_eq!(read_a(&source), "written original");
        fs::write(&source, "original").unwrap();
    }

    // A configured root may be absent: ordinary reads still succeed.
    let hooks = install(Paths::new(&game, &directory.join("absent-root")).unwrap()).unwrap();
    assert_eq!(read_a(&source), "original");
    drop(hooks);
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn only_existing_read_only_requests_are_redirectable() {
    let request = Request {
        access: GENERIC_READ,
        share: 0,
        security: std::ptr::null(),
        disposition: OPEN_EXISTING,
        flags: 0,
        template: std::ptr::null_mut(),
    };
    assert!(request.is_read());
    assert!(
        Request {
            access: FILE_GENERIC_READ,
            ..request
        }
        .is_read()
    );
    assert!(
        !Request {
            access: GENERIC_READ | GENERIC_WRITE,
            ..request
        }
        .is_read()
    );
    assert!(
        !Request {
            disposition: CREATE_ALWAYS,
            ..request
        }
        .is_read()
    );
    assert!(
        !Request {
            flags: FILE_FLAG_DELETE_ON_CLOSE,
            ..request
        }
        .is_read()
    );
    assert!(
        !Request {
            access: 0,
            ..request
        }
        .is_read()
    );
}
