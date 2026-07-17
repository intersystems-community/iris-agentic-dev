use std::path::{Path, PathBuf};

pub fn install_base(config_override: Option<&Path>) -> Option<PathBuf> {
    let config = config_override
        .map(|p| p.to_path_buf())
        .or_else(dirs::config_dir)?;
    Some(config.join("opencode").join("skills"))
}
