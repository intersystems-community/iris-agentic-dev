use std::path::{Path, PathBuf};

pub fn install_base(home_override: Option<&Path>) -> Option<PathBuf> {
    let home = home_override
        .map(|p| p.to_path_buf())
        .or_else(dirs::home_dir)?;

    #[cfg(target_os = "windows")]
    {
        // Claude Code on Windows: %APPDATA%\Claude\skills\
        let _ = home;
        dirs::config_dir().map(|d| d.join("Claude").join("skills"))
    }

    #[cfg(not(target_os = "windows"))]
    {
        Some(home.join(".claude").join("skills"))
    }
}
