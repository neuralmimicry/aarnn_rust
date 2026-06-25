use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FpaaProgramSpec {
    pub fpaa_index: u8,
    pub ahf_path: PathBuf,
}
