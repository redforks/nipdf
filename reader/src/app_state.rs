//! Save and restore the application state such as last opened file path, last opened page, etc.

use anyhow::Result;
use directories_next::ProjectDirs;
use log::error;
use std::path::PathBuf;

/// Return the last opened file path. If directory not exists, create it.
fn last_file_path() -> Result<PathBuf> {
    let project_dirs = ProjectDirs::from("", "", crate::APP_NAME)
        .ok_or_else(|| anyhow::anyhow!("get project dirs failed"))?;

    let data_dir = project_dirs.data_local_dir();
    if !data_dir.exists() {
        std::fs::create_dir_all(data_dir)?;
    }

    Ok(data_dir.join("last_file_path"))
}

fn log_and_forget<T>(rv: Result<T>, msg: &str) -> Option<T> {
    match rv {
        Ok(v) => Some(v),
        Err(err) => {
            error!("{}: {}", msg, err);
            None
        }
    }
}

/// Saves the last opened file path. If error happened, error log and ignore it.
/// If data directory not exists, create it.
pub fn save_last_file(file_path: impl AsRef<str>) {
    fn _do(file_path: &str) -> anyhow::Result<()> {
        let last_file_path = last_file_path()?;
        std::fs::write(last_file_path, file_path)?;

        Ok(())
    }

    log_and_forget(_do(file_path.as_ref()), "save last file path failed");
}

/// Loads the last opened file path. If error happened, error log and ignore it.
pub fn load_last_file() -> Option<String> {
    fn _do() -> anyhow::Result<String> {
        let last_file_path = last_file_path()?;
        Ok(std::fs::read_to_string(last_file_path)?)
    }

    log_and_forget(_do(), "load last file path failed")
}
