#![no_main]

use libfuzzer_sys::fuzz_target;
use nsromexefs_core::romfs::ROMFS_HEADER_LEN;
use nsromexefs_core::{RomFs, RomFsHeader};

fuzz_target!(|data: &[u8]| {
    if data.len() < ROMFS_HEADER_LEN {
        return;
    }
    let Ok(header) = RomFsHeader::parse(&data[..ROMFS_HEADER_LEN]) else {
        return;
    };

    let take = |offset: u64, size: u64| -> Option<&[u8]> {
        let start = usize::try_from(offset).ok()?;
        let end = start.checked_add(usize::try_from(size).ok()?)?;
        data.get(start..end)
    };

    let Some(dir_meta) = take(header.dir_meta_offset, header.dir_meta_size) else {
        return;
    };
    let Some(file_meta) = take(header.file_meta_offset, header.file_meta_size) else {
        return;
    };

    if let Ok(romfs) = RomFs::new(header, dir_meta.to_vec(), file_meta.to_vec()) {
        let _ = romfs.tree();
    }
});
