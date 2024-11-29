use std::fs;
use std::path::PathBuf;
use std::env;
use std::io;

pub struct TempDir {
    path: PathBuf,
}

impl TempDir {

    //
    pub fn new(repo: &str, commit_id: &str) -> io::Result<Self> {
        let temp_dir = env::temp_dir();
        let temp_subdir = format!("deploy_agent.tmp.{}.{}", repo.replace('/', "_"), commit_id);
        let temp_dir_path = temp_dir.join(temp_subdir);
        fs::create_dir_all(&temp_dir_path)?;
        Ok(TempDir { path: temp_dir_path })
    }

    //
    pub fn path(&self) -> &PathBuf {
        &self.path
    }
}

// Automatically remove the temporary directory on drop.
impl Drop for TempDir {
    fn drop(&mut self) {
        if let Err(e) = fs::remove_dir_all(&self.path) {
            eprintln!("[{}] Failed to remove temp directory {:?}: {}", chrono::Local::now().format("%Y-%m-%d %H:%M:%S.%3f"), self.path, e);
        }
    }
}
