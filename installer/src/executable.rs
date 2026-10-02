//! Minimal bounded PE validation before replacing a working installation.
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

pub fn validate(path: &Path) -> Result<(), String> {
    crate::transaction::reject_reparse_point(path)?;
    let mut file =
        File::open(path).map_err(|error| format!("Cannot open the main executable: {error}"))?;
    let size = file.metadata().map_err(|error| error.to_string())?.len();
    let mut dos = [0u8; 64];
    file.read_exact(&mut dos)
        .map_err(|_| "The main executable has a truncated DOS header.")?;
    if &dos[..2] != b"MZ" {
        return Err("The main executable is not a Windows PE image.".into());
    }
    let offset =
        u32::from_le_bytes(dos[60..64].try_into().map_err(|_| "Invalid DOS header.")?) as u64;
    if !(64..=1024 * 1024).contains(&offset) || offset + 24 > size {
        return Err("The main executable has an invalid PE header offset.".into());
    }
    file.seek(SeekFrom::Start(offset))
        .map_err(|error| error.to_string())?;
    let mut coff = [0u8; 24];
    file.read_exact(&mut coff)
        .map_err(|error| error.to_string())?;
    if &coff[..4] != b"PE\0\0" {
        return Err("The main executable has no PE signature.".into());
    }
    let machine = u16::from_le_bytes([coff[4], coff[5]]);
    if !matches!(machine, 0x8664 | 0xaa64) {
        return Err("The main executable must be a 64-bit Windows image.".into());
    }
    let count = u16::from_le_bytes([coff[6], coff[7]]) as u64;
    let optional_size = u16::from_le_bytes([coff[20], coff[21]]) as u64;
    let flags = u16::from_le_bytes([coff[22], coff[23]]);
    if flags & 2 == 0
        || flags & 0x2000 != 0
        || count == 0
        || count > 96
        || !(112..=4096).contains(&optional_size)
        || offset + 24 + optional_size + count * 40 > size
    {
        return Err("The main executable has invalid executable headers or is a DLL.".into());
    }
    let mut optional = vec![0u8; optional_size as usize];
    file.read_exact(&mut optional)
        .map_err(|error| error.to_string())?;
    if u16::from_le_bytes([optional[0], optional[1]]) != 0x20b {
        return Err("The main executable is not a PE32+ image.".into());
    }
    let entry = u32::from_le_bytes(
        optional[16..20]
            .try_into()
            .map_err(|_| "Invalid entry point.")?,
    ) as u64;
    let image_size = u32::from_le_bytes(
        optional[56..60]
            .try_into()
            .map_err(|_| "Invalid image size.")?,
    ) as u64;
    if entry == 0 || entry >= image_size {
        return Err("The main executable has an invalid entry point.".into());
    }
    let mut executable_entry = false;
    for _ in 0..count {
        let mut section = [0u8; 40];
        file.read_exact(&mut section)
            .map_err(|error| error.to_string())?;
        let virtual_size =
            u32::from_le_bytes(section[8..12].try_into().map_err(|_| "Invalid section.")?) as u64;
        let address =
            u32::from_le_bytes(section[12..16].try_into().map_err(|_| "Invalid section.")?) as u64;
        let raw_size =
            u32::from_le_bytes(section[16..20].try_into().map_err(|_| "Invalid section.")?) as u64;
        let raw_offset =
            u32::from_le_bytes(section[20..24].try_into().map_err(|_| "Invalid section.")?) as u64;
        let flags = u32::from_le_bytes(section[36..40].try_into().map_err(|_| "Invalid section.")?);
        if raw_offset + raw_size > size || address + virtual_size.max(raw_size) > image_size {
            return Err("The main executable contains an out-of-bounds section.".into());
        }
        executable_entry |= flags & 0x2000_0000 != 0
            && entry >= address
            && entry < address + virtual_size.max(raw_size);
    }
    if !executable_entry {
        return Err("The main executable entry point is outside an executable section.".into());
    }
    Ok(())
}

#[cfg(test)]
pub(crate) fn fixture_image() -> Vec<u8> {
    let mut image = vec![0u8; 512];
    image[..2].copy_from_slice(b"MZ");
    image[60..64].copy_from_slice(&64u32.to_le_bytes());
    image[64..68].copy_from_slice(b"PE\0\0");
    image[68..70].copy_from_slice(&0x8664u16.to_le_bytes());
    image[70..72].copy_from_slice(&1u16.to_le_bytes());
    image[84..86].copy_from_slice(&112u16.to_le_bytes());
    image[86..88].copy_from_slice(&2u16.to_le_bytes());
    image[88..90].copy_from_slice(&0x20bu16.to_le_bytes());
    image[104..108].copy_from_slice(&0x1000u32.to_le_bytes());
    image[144..148].copy_from_slice(&0x2000u32.to_le_bytes());
    image[208..212].copy_from_slice(&16u32.to_le_bytes());
    image[212..216].copy_from_slice(&0x1000u32.to_le_bytes());
    image[216..220].copy_from_slice(&16u32.to_le_bytes());
    image[220..224].copy_from_slice(&256u32.to_le_bytes());
    image[236..240].copy_from_slice(&0x20000000u32.to_le_bytes());
    image
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_text_truncation_dll_and_out_of_bounds_sections() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("app.exe");
        fs_write(&path, b"not an executable");
        assert!(validate(&path).is_err());
        let mut image = fixture_image();
        fs_write(&path, &image);
        validate(&path).unwrap();
        image[86..88].copy_from_slice(&0x2002u16.to_le_bytes());
        fs_write(&path, &image);
        assert!(validate(&path).is_err());
        image[86..88].copy_from_slice(&2u16.to_le_bytes());
        image[220..224].copy_from_slice(&9999u32.to_le_bytes());
        fs_write(&path, &image);
        assert!(validate(&path).is_err());
    }
    fn fs_write(path: &Path, data: &[u8]) {
        std::fs::write(path, data).unwrap();
    }
}
