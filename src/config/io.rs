//! Low-level, read-only access to the configuration file.
//!
//! The reader keeps a reusable buffer across reloads so the steady-state path
//! is a single `ReadFile` syscall with no allocation.

use std::io;
use std::os::windows::ffi::OsStrExt;
use std::path::Path;
use windows_sys::Win32::Foundation::{CloseHandle, GENERIC_READ, HANDLE, INVALID_HANDLE_VALUE};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, FILE_FLAG_SEQUENTIAL_SCAN, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
    GetFileSizeEx, OPEN_EXISTING, ReadFile,
};

/// Fast-path buffer size. A read that fills it triggers an exact-size second
/// pass, so it is a ceiling on the common case, not a file-size limit.
const DEFAULT_CAPACITY: usize = 64 * 1024;
const UTF8_BOM: &[u8; 3] = &[0xEF, 0xBB, 0xBF];
/// Allow editors to keep the file open while we read it.
const SHARE_ALL: u32 = FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE;

/// Closes the Win32 handle on every return path, including read errors.
struct FileHandle(HANDLE);

impl Drop for FileHandle {
    #[inline]
    fn drop(&mut self) {
        if self.0 != INVALID_HANDLE_VALUE && !self.0.is_null() {
            // SAFETY: `self.0` is an owned handle returned by `CreateFileW`;
            // the guards above exclude both sentinel values.
            unsafe {
                CloseHandle(self.0);
            }
        }
    }
}

/// Low-latency, read-only configuration file reader.
///
/// The path is encoded to UTF-16 once at construction and the heap buffer is
/// reused on every reload; the store owns the display path for diagnostics.
pub struct ConfigReader {
    wide_path: Vec<u16>,
    buffer: Vec<u8>,
}

impl ConfigReader {
    /// Encodes `path` to null-terminated UTF-16 once and pre-allocates the
    /// buffer used by the fast-path read.
    pub fn new(path: impl AsRef<Path>) -> Self {
        let wide_path: Vec<u16> = path
            .as_ref()
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();

        Self {
            wide_path,
            buffer: vec![0u8; DEFAULT_CAPACITY],
        }
    }

    /// Reads the whole file into the reusable buffer and returns a BOM-stripped,
    /// UTF-8-validated slice that borrows from it (no second copy).
    pub fn read(&mut self) -> io::Result<&str> {
        // SAFETY: `wide_path` is null-terminated by `new`; the pointers passed
        // for the security attributes and overlapped structures are null,
        // which `CreateFileW` explicitly permits.
        let raw_handle = unsafe {
            CreateFileW(
                self.wide_path.as_ptr(),
                GENERIC_READ,
                SHARE_ALL,
                std::ptr::null(),
                OPEN_EXISTING,
                FILE_FLAG_SEQUENTIAL_SCAN,
                std::ptr::null_mut(),
            )
        };

        if raw_handle == INVALID_HANDLE_VALUE {
            return Err(io::Error::last_os_error());
        }

        let handle = FileHandle(raw_handle);
        let mut total_read: usize = 0;

        // Speculative single-syscall read. Most configs fit in the buffer, so
        // the common path avoids a size query entirely.
        let mut bytes_read: u32 = 0;
        // SAFETY: the buffer is allocated for `DEFAULT_CAPACITY` bytes and the
        // request length is clamped to it and to `u32::MAX`.
        let ok = unsafe {
            ReadFile(
                handle.0,
                self.buffer.as_mut_ptr(),
                u32::try_from(self.buffer.len()).unwrap_or(u32::MAX),
                &raw mut bytes_read,
                std::ptr::null_mut(),
            )
        };

        if ok == 0 {
            return Err(io::Error::last_os_error());
        }
        total_read += bytes_read as usize;

        // A full buffer is ambiguous (exact fit or more data), so query the
        // real size and read the remainder.
        if total_read == self.buffer.len() {
            let mut file_size: i64 = 0;
            // SAFETY: `handle.0` is open and `file_size` is a valid out-pointer.
            let size_ok = unsafe { GetFileSizeEx(handle.0, &raw mut file_size) };
            if size_ok == 0 {
                return Err(io::Error::last_os_error());
            }

            let exact_size = usize::try_from(file_size).map_err(|_| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    "config exceeds addressable memory",
                )
            })?;

            if exact_size > total_read {
                // Allocate +1 byte so subsequent reads don't spuriously hit exact equality
                self.buffer.resize(exact_size + 1, 0);

                while total_read < exact_size {
                    let remaining = u32::try_from(exact_size - total_read).unwrap_or(u32::MAX);
                    let mut chunk_read: u32 = 0;
                    // SAFETY: the slice starting at `total_read` is at least
                    // `remaining` bytes long after the resize above, and the
                    // handle is still open.
                    let read_ok = unsafe {
                        ReadFile(
                            handle.0,
                            self.buffer[total_read..].as_mut_ptr(),
                            remaining,
                            &raw mut chunk_read,
                            std::ptr::null_mut(),
                        )
                    };
                    if read_ok == 0 {
                        return Err(io::Error::last_os_error());
                    }
                    if chunk_read == 0 {
                        break; // Unexpected EOF
                    }
                    total_read += chunk_read as usize;
                }
            }
        }

        // Explicitly release OS file handle prior to UTF-8 validation
        drop(handle);

        let raw_slice = &self.buffer[..total_read];
        let content_slice = raw_slice.strip_prefix(UTF8_BOM).unwrap_or(raw_slice);

        std::str::from_utf8(content_slice)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::{self, OpenOptions};
    use std::os::windows::fs::OpenOptionsExt;
    use std::path::PathBuf;

    fn temp_test_path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "shyena_test_{}_{}.shyena",
            std::process::id(),
            name
        ))
    }

    #[test]
    fn test_fast_path_bom_and_shrink_reuse() {
        let path = temp_test_path("shrink_reuse");
        let large = format!(
            "\u{FEFF}Register-App 'code' @{{ Path = '{}' }}\n",
            "x".repeat(4096)
        );
        fs::write(&path, &large).unwrap();

        let mut reader = ConfigReader::new(&path);
        let first = reader.read().unwrap();
        assert!(!first.starts_with('\u{FEFF}'));
        assert_eq!(first.len(), large.len() - 3);

        // Overwrite with smaller payload to verify stale bytes are not leaked
        let small = "Register-App 'wt' @{ Path = 'wt.exe' }";
        fs::write(&path, small).unwrap();
        let second = reader.read().unwrap();
        assert_eq!(second, small);

        let _ = fs::remove_file(&path);
    }

    #[test]
    fn test_exact_64kb_and_slow_path_resize() {
        let path = temp_test_path("boundary_64kb");

        // Exact 64 KB boundary
        let exact = "a".repeat(DEFAULT_CAPACITY);
        fs::write(&path, &exact).unwrap();
        let mut reader = ConfigReader::new(&path);
        assert_eq!(reader.read().unwrap().len(), DEFAULT_CAPACITY);

        // Above 64 KB (triggers Pass 2 GetFileSizeEx + resize)
        let over = "b".repeat(DEFAULT_CAPACITY + 12_345);
        fs::write(&path, &over).unwrap();
        assert_eq!(reader.read().unwrap(), over);

        let _ = fs::remove_file(&path);
    }

    #[test]
    fn test_concurrent_editor_lock_compatibility() {
        let path = temp_test_path("share_lock");
        fs::write(&path, "Register-App 'app' @{ Path = 'app.exe' }").unwrap();

        // Simulate an editor holding the file open with write + delete sharing
        let _editor_handle = OpenOptions::new()
            .write(true)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
            .open(&path)
            .unwrap();

        let mut reader = ConfigReader::new(&path);
        assert!(reader.read().is_ok());

        let _ = fs::remove_file(&path);
    }

    #[test]
    fn test_io_error_variants() {
        let missing = temp_test_path("does_not_exist");
        let mut reader = ConfigReader::new(&missing);
        assert_eq!(reader.read().unwrap_err().kind(), io::ErrorKind::NotFound);

        let bad_utf8 = temp_test_path("bad_utf8");
        fs::write(&bad_utf8, [0xFF, 0xFE, 0xFD]).unwrap();
        let mut reader = ConfigReader::new(&bad_utf8);
        assert_eq!(
            reader.read().unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );

        let _ = fs::remove_file(&bad_utf8);
    }

    #[test]
    fn test_empty_file_returns_empty_slice() {
        let path = temp_test_path("empty_file");
        fs::write(&path, b"").unwrap();

        let mut reader = ConfigReader::new(&path);
        let content = reader.read().unwrap();
        assert_eq!(content, "");

        let _ = fs::remove_file(&path);
    }

    #[test]
    fn test_slow_path_with_bom() {
        let path = temp_test_path("slow_path_bom");
        let payload = format!("\u{FEFF}{}", "c".repeat(DEFAULT_CAPACITY + 0x1000));
        fs::write(&path, &payload).unwrap();

        let mut reader = ConfigReader::new(&path);
        let content = reader.read().unwrap();
        assert!(!content.starts_with('\u{FEFF}'));
        assert_eq!(content.len(), DEFAULT_CAPACITY + 0x1000);

        let _ = fs::remove_file(&path);
    }
}
