//! Cross-platform path mapper

use std::path::PathBuf;

pub struct PathMapper;

impl PathMapper {
    pub fn normalize(path: &str) -> PathBuf {
        let expanded = Self::expand_home(path);

        if Self::is_wsl() {
            Self::to_wsl_path(&expanded)
        } else {
            PathBuf::from(expanded)
        }
    }

    pub fn expand_home(path: &str) -> String {
        if path.starts_with('~') {
            if let Some(home) = dirs::home_dir() {
                return path.replacen('~', &home.display().to_string(), 1);
            }
        }
        path.to_string()
    }

    pub fn is_wsl() -> bool {
        #[cfg(target_os = "linux")]
        {
            Path::new("/proc/sys/fs/binfmt_misc/WSLInterop").exists()
                || std::env::var("WSL_DISTRO_NAME").is_ok()
        }
        #[cfg(not(target_os = "linux"))]
        {
            false
        }
    }

    pub fn to_wsl_path(path: &str) -> PathBuf {
        // Convert Windows paths like C:\... to /mnt/c/...
        if path.len() >= 2 {
            let chars: Vec<char> = path.chars().collect();
            if chars[1] == ':' {
                let drive = chars[0].to_ascii_lowercase();
                let rest: String = chars[2..].iter().collect();
                let unix_path = rest.replace('\\', "/");
                return PathBuf::from(format!("/mnt/{}{}", drive, unix_path));
            }
        }
        PathBuf::from(path.replace('\\', "/"))
    }

    pub fn to_windows_path(path: &str) -> PathBuf {
        // Convert WSL paths like /mnt/c/... to C:\...
        if path.starts_with("/mnt/") && path.len() >= 7 {
            let chars: Vec<char> = path.chars().collect();
            let drive = chars[5].to_ascii_uppercase();
            let rest: String = chars[6..].iter().collect();
            let win_path = rest.replace('/', "\\");
            return PathBuf::from(format!("{}:{}", drive, win_path));
        }
        PathBuf::from(path)
    }

    pub fn get_platform() -> &'static str {
        if Self::is_wsl() {
            "wsl"
        } else if cfg!(target_os = "windows") {
            "windows"
        } else if cfg!(target_os = "macos") {
            "macos"
        } else {
            "linux"
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_wsl_path_conversion() {
        let win_path = r"C:\Users\test\file.txt";
        let wsl_path = PathMapper::to_wsl_path(win_path);
        assert_eq!(wsl_path, PathBuf::from("/mnt/c/Users/test/file.txt"));
    }

    #[test]
    fn test_windows_path_conversion() {
        let wsl_path = "/mnt/c/Users/test/file.txt";
        let win_path = PathMapper::to_windows_path(wsl_path);
        assert_eq!(win_path, PathBuf::from(r"C:\Users\test\file.txt"));
    }
}
