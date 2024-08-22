use anyhow::Error;
use fs2::FileExt;
use std::fs::File;

pub struct FileLock {
    file: File,
}

impl FileLock {
    pub fn new_shared(path: &str) -> Result<Self, Error> {
        let file = File::open(path)?;
        file.lock_shared()?;
        Ok(FileLock { file })
    }

    #[allow(dead_code)] //TODO: remove when used from the CLI
    pub fn new_exclusive(file: File) -> Result<Self, Error> {
        file.lock_exclusive()?;
        Ok(FileLock { file })
    }

    pub fn get_file(&self) -> &File {
        &self.file
    }
}

impl Drop for FileLock {
    fn drop(&mut self) {
        let _ = self.file.unlock();
    }
}
