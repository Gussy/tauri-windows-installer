use serde::{Deserialize, Serialize};

/// Metadata entry for a file in the package
#[derive(Serialize, Deserialize, Debug)]
pub struct MetadataEntry {
    pub name: String,
    pub offset: u64,
    pub size: usize,
}

/// Metadata offset size must be large enough to store an i64 as a string
pub const METADATA_OFFSET_SIZE: usize = std::mem::size_of::<i64>() * 2;
