use std::{
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
};

fn backup_path(path: &Path) -> PathBuf {
    path.with_extension(format!(
        "{}.bak",
        path.extension().unwrap_or_default().to_string_lossy()
    ))
}

pub fn has_backup(path: &Path) -> bool {
    backup_path(path).exists()
}

pub fn delete_backup(path: &Path) {
    fs::remove_file(backup_path(path)).unwrap();
}

/// Create only a new backup; never replace a previous run's recovery material.
pub(crate) fn create_backup(path: &Path) -> io::Result<PathBuf> {
    let original = fs::read(path)?;
    let backup = backup_path(path);
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&backup)?;
    file.write_all(&original)?;
    file.set_permissions(fs::metadata(path)?.permissions())?;
    Ok(backup)
}

pub fn backup_file(path: &Path) {
    create_backup(path).expect("cannot create source backup; check for an existing .bak file");
}

pub fn restore_file(path: &Path) {
    fs::copy(backup_path(path), path).unwrap();
}

/// Restore before a validation panic releases its compilation lock. Keep the
/// on-disk backup on failure; normal callers delete it after successful checks.
pub(crate) struct RestoreOnDrop<'a>(pub &'a Path);

impl Drop for RestoreOnDrop<'_> {
    fn drop(&mut self) {
        if let Err(error) = fs::copy(backup_path(self.0), self.0) {
            log::error!(
                "Cannot restore {}: {error}; backup retained",
                self.0.display()
            );
        }
    }
}

/// Own a temporary compiler input, restoring any pre-existing content.
pub(crate) struct TemporaryFile {
    path: PathBuf,
    original: Option<Vec<u8>>,
    finished: bool,
}

impl TemporaryFile {
    pub fn new(path: &Path) -> io::Result<Self> {
        let original = match fs::read(path) {
            Ok(bytes) => Some(bytes),
            Err(error) if error.kind() == io::ErrorKind::NotFound => None,
            Err(error) => return Err(error),
        };
        Ok(Self {
            path: path.to_owned(),
            original,
            finished: false,
        })
    }

    fn restore(&self) -> io::Result<()> {
        if let Some(bytes) = &self.original {
            fs::write(&self.path, bytes)
        } else {
            match fs::remove_file(&self.path) {
                Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
                result => result,
            }
        }
    }

    pub fn finish(mut self) -> io::Result<()> {
        self.restore()?;
        self.finished = true;
        Ok(())
    }
}

impl Drop for TemporaryFile {
    fn drop(&mut self) {
        if !self.finished {
            if let Err(error) = self.restore() {
                log::error!(
                    "Cannot restore temporary file {}: {error}",
                    self.path.display()
                );
            }
        }
    }
}
