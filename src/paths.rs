use std::path::PathBuf;

use crate::config::AppConfig;

#[derive(Debug, Clone)]
pub struct KbPaths {
    pub vault_path: PathBuf,
    pub index_dir: PathBuf,
    pub config_path: PathBuf,
    pub db_path: PathBuf,
    pub tantivy_dir: PathBuf,
}

impl KbPaths {
    pub fn from_config(config: &AppConfig) -> Self {
        Self {
            vault_path: config.vault.path.clone(),
            index_dir: config.store_dir(),
            config_path: config.config_path(),
            db_path: config.database_path(),
            tantivy_dir: config.tantivy_index_dir(),
        }
    }
}
