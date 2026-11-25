//! Report persistence. `output/` is created on demand and is git-ignored.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

pub fn write(dir: &Path, filename: &str, body: &str) -> io::Result<PathBuf> {
    fs::create_dir_all(dir)?;
    let path = dir.join(filename);
    fs::write(&path, body)?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_into_a_fresh_directory() {
        let base = std::env::temp_dir().join(format!("kibytm-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        let path = write(&base, "report-x.json", "{}\n").unwrap();
        assert!(path.exists());
        assert_eq!(fs::read_to_string(&path).unwrap(), "{}\n");
        let _ = fs::remove_dir_all(&base);
    }
}
