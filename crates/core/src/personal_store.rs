//! Cross-process ownership for account-scoped personal writes and cleanup.

use std::{fs::File, path::Path};

use crate::VapourflyError;

/// Lock failures carry no account identity, path, or OS diagnostic payload.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LockError {
    InvalidIdentity,
    Directory,
    Open,
    Busy,
}

impl From<LockError> for VapourflyError {
    fn from(error: LockError) -> Self {
        match error {
            LockError::InvalidIdentity => {
                Self::InvalidInput("Invalid personal-storage identity".into())
            }
            LockError::Directory => {
                Self::Internal("Cannot create personal-storage lock directory".into())
            }
            LockError::Open => Self::Internal("Cannot open personal-storage lock".into()),
            LockError::Busy => Self::InvalidInput(
                "Personal records are busy; finish or cancel the other operation and retry".into(),
            ),
        }
    }
}

/// Locks live beside, never inside, the account records being removed.
/// Keep the returned file alive for the whole operation; never unlink it.
pub fn lock_account(root: &Path, id: &str) -> Result<File, LockError> {
    if id.len() != 17 || !id.bytes().all(|b| b.is_ascii_digit()) {
        return Err(LockError::InvalidIdentity);
    }
    std::fs::create_dir_all(root).map_err(|_| LockError::Directory)?;
    let file = File::options()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(root.join(format!("{id}.lock")))
        .map_err(|_| LockError::Open)?;
    file.try_lock().map_err(|_| LockError::Busy)?;
    Ok(file)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ownership_is_exclusive_scoped_and_released_on_drop() {
        let root = tempfile::tempdir().unwrap();
        let owner = lock_account(root.path(), "76561198000000001").unwrap();
        assert!(lock_account(root.path(), "76561198000000001").is_err());
        assert!(lock_account(root.path(), "76561198000000002").is_ok());
        drop(owner);
        assert!(lock_account(root.path(), "76561198000000001").is_ok());
        assert!(lock_account(root.path(), "../private-marker").is_err());
    }

    #[test]
    fn failures_are_typed_and_never_export_identity_or_path() {
        let root = tempfile::tempdir().unwrap();
        let id = "76561198000000001";
        let _owner = lock_account(root.path(), id).unwrap();
        assert_eq!(lock_account(root.path(), id).unwrap_err(), LockError::Busy);
        assert_eq!(
            lock_account(root.path(), "../private-marker").unwrap_err(),
            LockError::InvalidIdentity
        );
        let file = root.path().join("private-path-marker");
        std::fs::write(&file, b"not a directory").unwrap();
        assert_eq!(lock_account(&file, id).unwrap_err(), LockError::Directory);
        for error in [
            LockError::InvalidIdentity,
            LockError::Directory,
            LockError::Open,
            LockError::Busy,
        ] {
            let rendered = VapourflyError::from(error).to_string();
            assert!(!rendered.contains(id));
            assert!(!rendered.contains("private-marker"));
            assert!(!rendered.contains("private-path-marker"));
        }
    }
}
