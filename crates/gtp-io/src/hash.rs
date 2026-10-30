//! Integrity of originals: SHA-256 of a file and the read-only flag (docs/specification.md section 9).

use std::fs::{self, File};
use std::io::Read;
use std::path::Path;

use gtp_core::AppError;
use sha2::{Digest, Sha256};

/// SHA-256 of the file as lower-case hex, read in chunks so large scans do not fill memory.
pub fn sha256_file(path: &Path) -> Result<String, AppError> {
    let mut file = File::open(path).map_err(|err| AppError::from_io(&err))?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 64 * 1024];
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|err| AppError::from_io(&err))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

/// Sets the read-only attribute, so editors and stray writes cannot change an original.
/// This does not stop the file from being deleted or replaced by someone with directory access.
pub fn mark_read_only(path: &Path) -> Result<(), AppError> {
    let mut permissions = fs::metadata(path)
        .map_err(|err| AppError::from_io(&err))?
        .permissions();
    permissions.set_readonly(true);
    fs::set_permissions(path, permissions).map_err(|err| AppError::from_io(&err))
}

#[cfg(test)]
mod tests {
    use super::*;
    use gtp_core::ErrorCode;

    fn temp_file(label: &str, bytes: &[u8]) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!("gtp-io-hash-{label}-{}", std::process::id()));
        fs::write(&path, bytes).unwrap();
        path
    }

    #[test]
    fn known_digests() {
        let abc = temp_file("abc", b"abc");
        assert_eq!(
            sha256_file(&abc).unwrap(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        let empty = temp_file("empty", b"");
        assert_eq!(
            sha256_file(&empty).unwrap(),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        fs::remove_file(abc).unwrap();
        fs::remove_file(empty).unwrap();
    }

    #[test]
    fn a_file_larger_than_the_read_buffer_hashes_correctly() {
        let data = vec![7u8; 200_000];
        let path = temp_file("big", &data);
        let expected: String = Sha256::digest(&data)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        assert_eq!(sha256_file(&path).unwrap(), expected);
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn a_missing_file_is_not_found() {
        let err = sha256_file(Path::new("definitely/not/here.tif")).unwrap_err();
        assert_eq!(err.code, ErrorCode::NotFound);
    }

    // Clearing the flag at the end only lets the test delete its own temporary file.
    #[allow(clippy::permissions_set_readonly_false)]
    #[test]
    fn a_read_only_original_cannot_be_overwritten() {
        let path = temp_file("ro", b"original");
        mark_read_only(&path).unwrap();
        assert!(fs::write(&path, b"changed").is_err());
        assert_eq!(fs::read(&path).unwrap(), b"original");

        let mut permissions = fs::metadata(&path).unwrap().permissions();
        permissions.set_readonly(false);
        fs::set_permissions(&path, permissions).unwrap();
        fs::remove_file(path).unwrap();
    }
}
