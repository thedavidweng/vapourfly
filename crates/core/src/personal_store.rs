//! Cross-process ownership for account-scoped personal writes and cleanup.

use std::{fs::File, path::Path};

use crate::{Result, VapourflyError};

/// Locks live beside, never inside, the account records being removed.
/// Keep the returned file alive for the whole operation; never unlink it.
pub fn lock_account(root: &Path, id: &str) -> Result<File> {
    if id.len() != 17 || !id.bytes().all(|b| b.is_ascii_digit()) {
        return Err(VapourflyError::InvalidInput(
            "Invalid personal-storage identity".into(),
        ));
    }
    std::fs::create_dir_all(root).map_err(|_| {
        VapourflyError::Internal("Cannot create personal-storage lock directory".into())
    })?;
    let file = File::options()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(root.join(format!("{id}.lock")))
        .map_err(|_| VapourflyError::Internal("Cannot open personal-storage lock".into()))?;
    file.try_lock().map_err(|_| {
        VapourflyError::InvalidInput(
            "Personal records are busy; finish or cancel the other operation and retry".into(),
        )
    })?;
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
}
