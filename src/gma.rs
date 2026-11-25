//! Garry's Mod addon archive (`.gma`) parsing.
//!
//! A GMA is a little-endian-on-disk container: the magic `GMA7` is stored
//! big-endian, so a reader that reverses each 32-bit word sees `1145130311`.
//!
//! Layout of an uncompressed (`RAW`) archive:
//!
//! ```text
//! "GMA7"                      4 bytes
//! version                     u8
//! steamid                     i64 BE
//! timestamp                   i64 BE
//! [version >= 2] required     NUL-terminated strings, empty string ends
//! title                       NUL-terminated string
//! description                 NUL-terminated string
//! author                      NUL-terminated string
//! addon version               u32 BE
//! repeat:
//!   file index                u32 BE   (0 ends the table)
//!   file name                 NUL-terminated string
//!   file size                 i64 BE
//!   file crc                  u32 BE
//! addon crc                   u32 BE   (at the very end)
//! ```
//!
//! LZMA-compressed variants exist; they are detected and reported rather than
//! decoded here, so callers can decide how to handle them.

use std::io;

/// Magic value as seen after byte-reversing the first 32-bit word.
pub const MAGIC: u32 = 1_145_130_311;

/// Magic used by Source's LZMA container.
pub const SOURCE_LZMA_MAGIC: u32 = 1_095_588_428;

/// Hard cap on the file table, matching the original launcher.
pub const MAX_FILES: usize = 200_000;

/// Hard cap on a single string field.
pub const MAX_STRING: usize = 4096;

/// Hard cap on the declared required-content list.
pub const MAX_REQUIRED: usize = 1024;

/// Hard cap on total uncompressed size (100 GiB).
pub const MAX_TOTAL: u64 = 107_374_182_400;

/// Container encoding of the archive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    /// Plain archive, data follows the index directly.
    Raw,
    /// Source's own LZMA container.
    SourceLzma,
    /// Standard `.lzma` alone stream.
    LzmaAlone,
    /// Not a GMA.
    Unknown,
}

impl Format {
    /// Detects the container from the first bytes, mirroring the original
    /// `GmaArchive.format()` checks.
    pub fn detect(bytes: &[u8]) -> Format {
        if bytes.len() < 4 {
            return Format::Unknown;
        }
        let word = u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
        if word == MAGIC {
            return Format::Raw;
        }
        if word == SOURCE_LZMA_MAGIC {
            return Format::SourceLzma;
        }
        // `.lzma` alone: properties byte 0x5D followed by a 4-byte dictionary size.
        if bytes[0] == 0x5D && bytes.len() >= 17 {
            return Format::LzmaAlone;
        }
        Format::Unknown
    }
}

/// One file inside the archive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// Index as stored in the archive (1-based in practice).
    pub index: u32,
    /// Normalised relative path (`\` replaced by `/`).
    pub path: String,
    /// Uncompressed size in bytes.
    pub size: u64,
    /// CRC recorded in the index.
    pub crc: u32,
    /// Offset of this file's data relative to the data section start.
    pub offset: u64,
}

/// Parsed archive header plus file index.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Archive {
    pub version: u8,
    pub steam_id: i64,
    pub timestamp: i64,
    pub required: Vec<String>,
    pub title: String,
    pub description: String,
    pub author: String,
    pub addon_version: u32,
    pub entries: Vec<Entry>,
    /// Offset in the file where file data begins.
    pub data_start: usize,
    /// Total uncompressed data length.
    pub data_length: u64,
}

/// Errors produced while parsing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GmaError {
    /// The magic or container is not a plain GMA.
    NotRaw(Format),
    /// The file ended unexpectedly.
    Truncated,
    /// A string exceeded its limit or was not NUL-terminated.
    BadString,
    /// The file table exceeded [`MAX_FILES`].
    TooManyFiles,
    /// Declared sizes are inconsistent with the file length.
    BadBoundary,
    /// The required-content list exceeded [`MAX_REQUIRED`].
    TooManyRequired,
}

impl std::fmt::Display for GmaError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GmaError::NotRaw(format) => write!(f, "not a plain GMA archive (format: {format:?})"),
            GmaError::Truncated => write!(f, "archive is truncated"),
            GmaError::BadString => write!(f, "invalid string field"),
            GmaError::TooManyFiles => write!(f, "file table is too large"),
            GmaError::BadBoundary => write!(f, "invalid data boundary"),
            GmaError::TooManyRequired => write!(f, "invalid required-content list"),
        }
    }
}

impl std::error::Error for GmaError {}

impl From<GmaError> for io::Error {
    fn from(e: GmaError) -> Self {
        io::Error::new(io::ErrorKind::InvalidData, e.to_string())
    }
}

/// Normalises a path from the archive: backslashes to slashes, no leading slash.
pub fn normalize_path(raw: &str) -> String {
    raw.replace('\\', "/").trim_start_matches('/').to_owned()
}

/// Parses a plain (`RAW`) GMA archive from memory.
///
/// Compressed containers are rejected with [`GmaError::NotRaw`]; callers should
/// check [`Format::detect`] first if they want to decompress.
pub fn parse(bytes: &[u8]) -> Result<Archive, GmaError> {
    let format = Format::detect(bytes);
    if format != Format::Raw {
        return Err(GmaError::NotRaw(format));
    }

    let mut r = Reader::new(bytes);
    r.skip(4)?; // magic, already validated

    let version = r.u8()?;
    let steam_id = r.i64()?;
    let timestamp = r.i64()?;

    let mut required = Vec::new();
    if version >= 2 {
        loop {
            let item = r.cstring(MAX_STRING)?;
            if item.is_empty() {
                break;
            }
            required.push(item);
            if required.len() > MAX_REQUIRED {
                return Err(GmaError::TooManyRequired);
            }
        }
    }

    let title = r.cstring(MAX_STRING)?;
    let description = r.cstring(MAX_STRING)?;
    let author = r.cstring(MAX_STRING)?;
    let addon_version = r.u32()?;

    let mut entries = Vec::new();
    let mut offset: u64 = 0;
    let data_start;
    let data_length;

    loop {
        let index = r.u32()?;
        if index == 0 {
            data_start = r.position();
            data_length = offset;
            break;
        }
        if entries.len() >= MAX_FILES {
            return Err(GmaError::TooManyFiles);
        }
        let name = r.cstring(MAX_STRING)?;
        let size = r.i64()?;
        let crc = r.u32()?;

        if size < 0 || (size as u64) > MAX_TOTAL.saturating_sub(offset) {
            return Err(GmaError::BadBoundary);
        }
        let size = size as u64;

        entries.push(Entry {
            index,
            path: normalize_path(&name),
            size,
            crc,
            offset,
        });
        offset += size;
    }

    // Trailing addon CRC: the data section may be followed by exactly 4 bytes.
    let remaining = bytes.len().saturating_sub(data_start) as u64;
    if remaining < data_length {
        return Err(GmaError::Truncated);
    }
    let slack = remaining - data_length;
    if slack != 0 && slack != 4 {
        return Err(GmaError::BadBoundary);
    }

    Ok(Archive {
        version,
        steam_id,
        timestamp,
        required,
        title,
        description,
        author,
        addon_version,
        entries,
        data_start,
        data_length,
    })
}

/// Returns true when `path` is an allowed addon content path.
///
/// Mirrors the original whitelist: only known game-content extensions are
/// accepted, which prevents an addon from dropping arbitrary files (binaries,
/// engine libraries) into the game directory.
pub fn is_allowed_path(path: &str) -> bool {
    let p = normalize_path(path).to_ascii_lowercase();
    if p.is_empty() || p.contains("..") {
        return false;
    }

    // Rejected model variants (console-specific, never used on mobile).
    for suffix in [".sw.vtx", ".360.vtx", ".xbox.vtx"] {
        if p.ends_with(suffix) {
            return false;
        }
    }

    const TOP_LEVEL: &[(&str, &[&str])] = &[
        ("lua/", &[".lua"]),
        ("scenes/", &[".vcd"]),
        ("particles/", &[".pcf"]),
        ("resource/fonts/", &[".ttf"]),
        ("scripts/vehicles/", &[".txt"]),
        ("maps/", &[".bsp", ".lmp", ".nav", ".ain", ".png", ".txt"]),
        ("sound/", &[".wav", ".mp3", ".ogg"]),
        (
            "materials/",
            &[".vmt", ".vtf", ".png", ".jpg", ".jpeg", ".raw"],
        ),
        ("models/", &[".mdl", ".phy", ".ani", ".vvd", ".vtx"]),
        ("data_static/", &[".txt", ".dat", ".json", ".xml", ".csv"]),
        ("shaders/fxc/", &[".vcs"]),
        ("gamemodes/", &[]), // handled below
        ("resource/localization/", &[".properties"]),
    ];

    if p.starts_with("gamemodes/") {
        // Only specific sub-paths inside a gamemode are permitted.
        let allowed_gamemode: &[(&str, &[&str])] = &[
            (
                "/content/models/",
                &[".mdl", ".phy", ".ani", ".vvd", ".vtx"],
            ),
            (
                "/content/materials/",
                &[".vmt", ".vtf", ".png", ".jpg", ".jpeg", ".raw"],
            ),
            ("/content/scenes/", &[".vcd"]),
            ("/content/particles/", &[".pcf"]),
            ("/content/resource/fonts/", &[".ttf"]),
            ("/content/scripts/vehicles/", &[".txt"]),
            ("/content/resource/localization/", &[".properties"]),
            ("/content/maps/", &[".bsp", ".nav", ".ain", ".png"]),
            ("/content/sound/", &[".wav", ".mp3", ".ogg"]),
            ("/gamemode/", &[".lua"]),
            ("/entities/effects/", &[".lua"]),
            ("/entities/weapons/", &[".lua"]),
            ("/entities/entities/", &[".lua"]),
            ("/backgrounds/", &[".png", ".jpg", ".jpeg"]),
        ];
        // Must have a gamemode name segment, and no deeper nesting than the
        // patterns above allow.
        let parts: Vec<&str> = p.split('/').collect();
        if parts.len() < 3 {
            return false;
        }
        // Files directly at the gamemode root: logo/icon plus the gamemode's
        // own descriptor files. Handled here because the marker search below
        // matches the first slash, which is the one after "gamemodes".
        if parts.len() == 3 {
            return matches!(parts[2], "logo.png" | "icon24.png")
                || parts[2].ends_with(".txt")
                || parts[2].ends_with(".fgd");
        }
        for (marker, exts) in allowed_gamemode {
            if let Some(pos) = p.find(marker) {
                let after = &p[pos + marker.len()..];
                return exts.iter().any(|e| after.ends_with(e));
            }
        }
        return false;
    }

    for (prefix, exts) in TOP_LEVEL {
        if p.starts_with(prefix) {
            if exts.is_empty() {
                return false;
            }
            return exts.iter().any(|e| p.ends_with(e));
        }
    }
    false
}

/// Splits a parsed archive into allowed and rejected paths.
pub fn partition_paths(archive: &Archive) -> (Vec<&Entry>, Vec<&Entry>) {
    archive
        .entries
        .iter()
        .partition(|e| is_allowed_path(&e.path))
}

struct Reader<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, pos: 0 }
    }

    fn position(&self) -> usize {
        self.pos
    }

    fn skip(&mut self, n: usize) -> Result<(), GmaError> {
        if self.pos + n > self.bytes.len() {
            return Err(GmaError::Truncated);
        }
        self.pos += n;
        Ok(())
    }

    fn take(&mut self, n: usize) -> Result<&'a [u8], GmaError> {
        if self.pos + n > self.bytes.len() {
            return Err(GmaError::Truncated);
        }
        let slice = &self.bytes[self.pos..self.pos + n];
        self.pos += n;
        Ok(slice)
    }

    fn u8(&mut self) -> Result<u8, GmaError> {
        Ok(self.take(1)?[0])
    }

    fn u32(&mut self) -> Result<u32, GmaError> {
        let b = self.take(4)?;
        Ok(u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
    }

    fn i64(&mut self) -> Result<i64, GmaError> {
        let b = self.take(8)?;
        Ok(i64::from_be_bytes([
            b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7],
        ]))
    }

    fn cstring(&mut self, limit: usize) -> Result<String, GmaError> {
        let start = self.pos;
        for i in 0..=limit {
            let idx = start + i;
            if idx >= self.bytes.len() {
                return Err(GmaError::Truncated);
            }
            if self.bytes[idx] == 0 {
                self.pos = idx + 1;
                return String::from_utf8(self.bytes[start..idx].to_vec())
                    .map_err(|_| GmaError::BadString);
            }
        }
        Err(GmaError::BadString)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds a minimal RAW GMA in memory.
    fn build(files: &[(&str, &[u8])], version: u8) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&MAGIC.to_be_bytes());
        out.push(version);
        out.extend_from_slice(&76561197960265728i64.to_be_bytes()); // steamid
        out.extend_from_slice(&1_700_000_000i64.to_be_bytes()); // timestamp
        if version >= 2 {
            // required list: one entry then terminator
            out.extend_from_slice(b"garrysmod\0\0");
        }
        out.extend_from_slice(b"Test Addon\0");
        out.extend_from_slice(b"A description\0");
        out.extend_from_slice(b"Tester\0");
        out.extend_from_slice(&7u32.to_be_bytes()); // addon version

        for (i, (name, _)) in files.iter().enumerate() {
            out.extend_from_slice(&(i as u32 + 1).to_be_bytes());
            out.extend_from_slice(name.as_bytes());
            out.push(0);
            out.extend_from_slice(&(files[i].1.len() as i64).to_be_bytes());
            out.extend_from_slice(&0xDEAD_BEEFu32.to_be_bytes());
        }
        out.extend_from_slice(&0u32.to_be_bytes()); // end of table
        for (_, data) in files {
            out.extend_from_slice(data);
        }
        out.extend_from_slice(&0xCAFE_F00Du32.to_be_bytes()); // addon crc
        out
    }

    #[test]
    fn detects_formats() {
        let gma = build(&[("lua/autorun/a.lua", b"x")], 1);
        assert_eq!(Format::detect(&gma), Format::Raw);
        assert_eq!(
            Format::detect(&SOURCE_LZMA_MAGIC.to_be_bytes()),
            Format::SourceLzma
        );
        assert_eq!(Format::detect(b"nope"), Format::Unknown);
        let mut lzma = vec![0x5D];
        lzma.extend_from_slice(&[0; 16]);
        assert_eq!(Format::detect(&lzma), Format::LzmaAlone);
    }

    #[test]
    fn parses_header_and_index() {
        let gma = build(
            &[
                ("lua/autorun/init.lua", b"print(1)"),
                ("materials/x.vmt", b"\"VertexLitGeneric\"{}"),
            ],
            2,
        );
        let a = parse(&gma).expect("parses");
        assert_eq!(a.version, 2);
        assert_eq!(a.title, "Test Addon");
        assert_eq!(a.description, "A description");
        assert_eq!(a.author, "Tester");
        assert_eq!(a.addon_version, 7);
        assert_eq!(a.required, vec!["garrysmod".to_string()]);
        assert_eq!(a.entries.len(), 2);

        assert_eq!(a.entries[0].path, "lua/autorun/init.lua");
        assert_eq!(a.entries[0].size, 8);
        assert_eq!(a.entries[0].offset, 0);
        assert_eq!(a.entries[1].path, "materials/x.vmt");
        assert_eq!(a.entries[1].offset, 8);
        assert_eq!(a.data_length, 8 + 20);

        // Data section must start right after the terminating zero index.
        assert_eq!(&gma[a.data_start..a.data_start + 8], b"print(1)");
    }

    #[test]
    fn rejects_non_raw() {
        let lzma = [0x5D, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        assert_eq!(parse(&lzma), Err(GmaError::NotRaw(Format::LzmaAlone)));
    }

    #[test]
    fn rejects_truncated() {
        let gma = build(&[("lua/a.lua", b"hello")], 1);
        let cut = &gma[..gma.len() - 8];
        assert!(matches!(parse(cut), Err(GmaError::Truncated)));
    }

    #[test]
    fn rejects_bad_boundary() {
        let mut gma = build(&[("lua/a.lua", b"hello")], 1);
        gma.push(0); // extra slack byte beyond the allowed 4-byte CRC
        assert_eq!(parse(&gma), Err(GmaError::BadBoundary));
    }

    #[test]
    fn version_one_has_no_required_list() {
        let gma = build(&[("lua/a.lua", b"x")], 1);
        let a = parse(&gma).unwrap();
        assert!(a.required.is_empty());
        assert_eq!(a.title, "Test Addon");
    }

    #[test]
    fn path_normalisation() {
        assert_eq!(normalize_path("lua\\autorun\\a.lua"), "lua/autorun/a.lua");
        assert_eq!(normalize_path("/maps/a.bsp"), "maps/a.bsp");
    }

    #[test]
    fn whitelist_accepts_game_content() {
        for p in [
            "lua/autorun/client/x.lua",
            "maps/gm_construct.bsp",
            "maps/thumb/x.png",
            "materials/models/x.vmt",
            "materials/models/x.vtf",
            "models/x.mdl",
            "models/x.vvd",
            "sound/x.wav",
            "sound/x.ogg",
            "particles/x.pcf",
            "scenes/x.vcd",
            "resource/fonts/x.ttf",
            "data_static/x.json",
            "gamemodes/sandbox/gamemode/init.lua",
            "gamemodes/sandbox/entities/weapons/x.lua",
            "gamemodes/sandbox/logo.png",
            "gamemodes/sandbox/sandbox.txt",
        ] {
            assert!(is_allowed_path(p), "should allow {p}");
        }
    }

    #[test]
    fn whitelist_rejects_dangerous_paths() {
        for p in [
            "../../etc/passwd",
            "bin/linux64/libengine.so",
            "lua/autorun/x.so",
            "models/x.sw.vtx",
            "models/x.360.vtx",
            "models/x.xbox.vtx",
            "maps/../../x.bsp",
            "",
            "shaders/fxc/x.txt",
        ] {
            assert!(!is_allowed_path(p), "should reject {p}");
        }
    }

    #[test]
    fn partition_splits_entries() {
        let gma = build(&[("lua/autorun/ok.lua", b"a"), ("bin/evil.so", b"b")], 1);
        let a = parse(&gma).unwrap();
        let (ok, bad) = partition_paths(&a);
        assert_eq!(ok.len(), 1);
        assert_eq!(ok[0].path, "lua/autorun/ok.lua");
        assert_eq!(bad.len(), 1);
        assert_eq!(bad[0].path, "bin/evil.so");
    }
}
