//! A small zip writer for sending folders and several files as one archive (Telegram Desktop
//! 7.2.6). Deflate from `flate2` and CRC from `crc32fast`, both already in the build, so no zip
//! crate. Plain zip without Zip64: Telegram takes at most 4 GB per file anyway.

use std::fs::File;
use std::io::{self, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use flate2::Compression;
use flate2::write::DeflateEncoder;

use crate::i18n::{l, lf};

/// The plain zip format counts sizes and offsets in 32 bits and entries in 16.
const MAX_BYTES: u64 = u32::MAX as u64 - (64 << 20);
const MAX_ENTRIES: usize = u16::MAX as usize;

/// Formats that are compressed already: deflating them again costs time and saves nothing.
const STORED: [&str; 28] = [
    "jpg", "jpeg", "png", "gif", "webp", "heic", "avif", "mp4", "mov", "m4v", "mkv", "webm", "avi", "mp3", "ogg", "opus",
    "m4a", "aac", "flac", "zip", "rar", "7z", "gz", "xz", "bz2", "zst", "tgs", "apk",
];

/// One file or folder inside the archive.
struct Entry {
    src: PathBuf,
    /// Path inside the archive with `/` separators; folders end with `/`.
    name: String,
    dir: bool,
}

/// What a folder or a set of files holds: shown in the send box before packing.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Summary {
    pub files: usize,
    pub bytes: u64,
}

/// Counts the files and bytes that `write` would pack.
pub fn summary(items: &[PathBuf]) -> Summary {
    let entries = collect(items).unwrap_or_default();
    entries.iter().filter(|e| !e.dir).fold(Summary::default(), |s, e| Summary {
        files: s.files + 1,
        bytes: s.bytes + std::fs::metadata(&e.src).map(|m| m.len()).unwrap_or(0),
    })
}

/// Packs files and folders (with everything inside) into `out`. Folders keep their own name as
/// the top level. Stops with `Interrupted` as soon as `cancel` is set; the caller removes `out`.
pub fn write(items: &[PathBuf], out: &Path, cancel: &AtomicBool) -> io::Result<()> {
    let entries = collect(items)?;
    let n = entries.len();
    if n > MAX_ENTRIES {
        return Err(io::Error::other(lf!("Забагато файлів для архіву: {n}", "Too many files for an archive: {n}")));
    }
    if summary(items).bytes > MAX_BYTES {
        return Err(io::Error::other(l!("Архів був би більшим за 4 ГБ", "The archive would be larger than 4 GB")));
    }
    let mut w = BufWriter::new(File::create(out)?);
    let mut central = Vec::new();
    for e in &entries {
        if cancel.load(Ordering::Relaxed) {
            return Err(io::ErrorKind::Interrupted.into());
        }
        let offset = w.stream_position()?;
        let (time, date) = std::fs::metadata(&e.src).and_then(|m| m.modified()).map(dos_time).unwrap_or((0, 0x21));
        let ext = Path::new(&e.name).extension().map(|x| x.to_string_lossy().to_lowercase()).unwrap_or_default();
        let method: u16 = if e.dir || STORED.contains(&ext.as_str()) { 0 } else { 8 };
        // The header goes first with zero CRC and sizes; they are filled in once the data is written.
        local_header(&mut w, &e.name, method, time, date, 0, 0, 0)?;
        let (crc, size) = if e.dir { (0, 0) } else { copy(&e.src, &mut w, method, cancel)? };
        let end = w.stream_position()?;
        let packed = end - offset - 30 - e.name.len() as u64;
        if size > MAX_BYTES || end > MAX_BYTES {
            return Err(io::Error::other(l!("Архів був би більшим за 4 ГБ", "The archive would be larger than 4 GB")));
        }
        w.seek(SeekFrom::Start(offset + 14))?;
        w.write_all(&crc.to_le_bytes())?;
        w.write_all(&(packed as u32).to_le_bytes())?;
        w.write_all(&(size as u32).to_le_bytes())?;
        w.seek(SeekFrom::Start(end))?;
        central_header(&mut central, e, method, time, date, crc, packed as u32, size as u32, offset as u32);
    }
    let cd_offset = w.stream_position()?;
    w.write_all(&central)?;
    // End of central directory.
    let n = entries.len() as u16;
    w.write_all(&0x0605_4b50u32.to_le_bytes())?;
    w.write_all(&[0; 4])?;
    w.write_all(&n.to_le_bytes())?;
    w.write_all(&n.to_le_bytes())?;
    w.write_all(&(central.len() as u32).to_le_bytes())?;
    w.write_all(&(cd_offset as u32).to_le_bytes())?;
    w.write_all(&[0; 2])?;
    w.flush()
}

/// Every entry under `items`, folders first and in name order. Symlinks are skipped: they could
/// point outside the folder or loop.
fn collect(items: &[PathBuf]) -> io::Result<Vec<Entry>> {
    let mut entries = Vec::new();
    let mut tops: Vec<String> = Vec::new();
    for item in items {
        let base = item.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "file".into());
        // Two picked items with one name (from different folders) must not overwrite each other.
        let mut name = base.clone();
        let mut k = 2;
        while tops.iter().any(|t| t.eq_ignore_ascii_case(&name)) {
            name = match base.rsplit_once('.') {
                Some((stem, ext)) if !stem.is_empty() => format!("{stem} ({k}).{ext}"),
                _ => format!("{base} ({k})"),
            };
            k += 1;
        }
        tops.push(name.clone());
        walk(item, name, &mut entries)?;
    }
    Ok(entries)
}

fn walk(path: &Path, name: String, out: &mut Vec<Entry>) -> io::Result<()> {
    let meta = std::fs::symlink_metadata(path)?;
    if meta.file_type().is_symlink() {
        return Ok(());
    }
    if !meta.is_dir() {
        out.push(Entry { src: path.to_owned(), name, dir: false });
        return Ok(());
    }
    out.push(Entry { src: path.to_owned(), name: format!("{name}/"), dir: true });
    if out.len() > MAX_ENTRIES {
        return Ok(());
    }
    let mut children: Vec<_> = std::fs::read_dir(path)?.filter_map(Result::ok).collect();
    children.sort_by_key(|c| c.file_name());
    for c in children {
        walk(&c.path(), format!("{name}/{}", c.file_name().to_string_lossy()), out)?;
    }
    Ok(())
}

/// Writes one file's data, stored or deflated; returns its CRC and original size.
fn copy<W: Write>(src: &Path, w: &mut W, method: u16, cancel: &AtomicBool) -> io::Result<(u32, u64)> {
    let file = File::open(src)?;
    if method != 8 {
        return pump(file, w, cancel);
    }
    let mut d = DeflateEncoder::new(w, Compression::fast());
    let done = pump(file, &mut d, cancel)?;
    d.finish()?;
    Ok(done)
}

fn pump<W: Write>(mut file: File, w: &mut W, cancel: &AtomicBool) -> io::Result<(u32, u64)> {
    let mut crc = crc32fast::Hasher::new();
    let mut size = 0u64;
    let mut buf = vec![0; 64 * 1024];
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err(io::ErrorKind::Interrupted.into());
        }
        let n = file.read(&mut buf)?;
        if n == 0 {
            return Ok((crc.finalize(), size));
        }
        crc.update(&buf[..n]);
        size += n as u64;
        w.write_all(&buf[..n])?;
    }
}

#[allow(clippy::too_many_arguments)]
fn local_header<W: Write>(w: &mut W, name: &str, method: u16, time: u16, date: u16, crc: u32, packed: u32, size: u32) -> io::Result<()> {
    w.write_all(&0x0403_4b50u32.to_le_bytes())?;
    w.write_all(&20u16.to_le_bytes())?;
    // Bit 11: the name is UTF-8, so Cyrillic names survive on every OS.
    w.write_all(&0x0800u16.to_le_bytes())?;
    for v in [method, time, date] {
        w.write_all(&v.to_le_bytes())?;
    }
    for v in [crc, packed, size] {
        w.write_all(&v.to_le_bytes())?;
    }
    w.write_all(&(name.len() as u16).to_le_bytes())?;
    w.write_all(&0u16.to_le_bytes())?;
    w.write_all(name.as_bytes())
}

#[allow(clippy::too_many_arguments)]
fn central_header(out: &mut Vec<u8>, e: &Entry, method: u16, time: u16, date: u16, crc: u32, packed: u32, size: u32, offset: u32) {
    out.extend(0x0201_4b50u32.to_le_bytes());
    for v in [20u16, 20, 0x0800, method, time, date] {
        out.extend(v.to_le_bytes());
    }
    for v in [crc, packed, size] {
        out.extend(v.to_le_bytes());
    }
    // Name length, extra, comment, disk, internal attributes.
    for v in [e.name.len() as u16, 0, 0, 0, 0] {
        out.extend(v.to_le_bytes());
    }
    // External attributes: the MS-DOS folder bit.
    out.extend(if e.dir { 0x10u32 } else { 0 }.to_le_bytes());
    out.extend(offset.to_le_bytes());
    out.extend(e.name.as_bytes());
}

/// Modification time as MS-DOS time and date, in local time like other zip tools.
fn dos_time(t: std::time::SystemTime) -> (u16, u16) {
    let Ok(ts) = jiff::Timestamp::try_from(t) else { return (0, 0x21) };
    let z = ts.to_zoned(jiff::tz::TimeZone::system());
    if z.year() < 1980 {
        return (0, 0x21);
    }
    let time = (z.hour() as u16) << 11 | (z.minute() as u16) << 5 | (z.second() as u16) / 2;
    let date = ((z.year() - 1980) as u16) << 9 | (z.month() as u16) << 5 | z.day() as u16;
    (time, date)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn u16_at(b: &[u8], i: usize) -> usize {
        u16::from_le_bytes([b[i], b[i + 1]]) as usize
    }
    fn u32_at(b: &[u8], i: usize) -> u32 {
        u32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]])
    }

    /// Reads an archive back through its central directory, checking every CRC.
    fn unpack(zip: &[u8]) -> HashMap<String, Vec<u8>> {
        let eocd = zip.len() - 22;
        assert_eq!(u32_at(zip, eocd), 0x0605_4b50);
        let (n, mut p) = (u16_at(zip, eocd + 10), u32_at(zip, eocd + 16) as usize);
        let mut out = HashMap::new();
        for _ in 0..n {
            assert_eq!(u32_at(zip, p), 0x0201_4b50);
            let (method, crc, packed) = (u16_at(zip, p + 10), u32_at(zip, p + 16), u32_at(zip, p + 20) as usize);
            let name_len = u16_at(zip, p + 28);
            let name = String::from_utf8(zip[p + 46..p + 46 + name_len].to_vec()).expect("utf-8 name");
            let local = u32_at(zip, p + 42) as usize;
            assert_eq!(u32_at(zip, local), 0x0403_4b50);
            assert_eq!(u32_at(zip, local + 14), crc, "local CRC of {name}");
            let start = local + 30 + u16_at(zip, local + 26);
            let raw = &zip[start..start + packed];
            let data = if method == 8 {
                let mut v = Vec::new();
                flate2::read::DeflateDecoder::new(raw).read_to_end(&mut v).expect("inflate");
                v
            } else {
                raw.to_vec()
            };
            assert_eq!(crc32fast::hash(&data), crc, "CRC of {name}");
            out.insert(name, data);
            p += 46 + name_len;
        }
        out
    }

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("glassgram-zip-test-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("scratch dir");
        dir
    }

    #[test]
    fn packs_a_folder_with_subfolders() {
        let dir = scratch("folder");
        let root = dir.join("Звіт");
        std::fs::create_dir_all(root.join("фото")).unwrap();
        std::fs::create_dir_all(root.join("порожня")).unwrap();
        let text = "Привіт, архів! ".repeat(500);
        std::fs::write(root.join("нотатки.txt"), &text).unwrap();
        std::fs::write(root.join("фото/a.jpg"), [0xFF, 0xD8, 1, 2, 3]).unwrap();
        let out = dir.join("out.zip");
        write(std::slice::from_ref(&root), &out, &AtomicBool::new(false)).unwrap();
        let zip = std::fs::read(&out).unwrap();
        let files = unpack(&zip);
        assert_eq!(files["Звіт/нотатки.txt"], text.as_bytes());
        assert_eq!(files["Звіт/фото/a.jpg"], [0xFF, 0xD8, 1, 2, 3]);
        assert!(files.contains_key("Звіт/порожня/"));
        // Text is deflated: the archive is much smaller than the text.
        assert!(zip.len() < text.len() / 4);
        assert_eq!(summary(&[root]), Summary { files: 2, bytes: text.len() as u64 + 5 });
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn same_names_from_different_folders_are_kept_apart() {
        let dir = scratch("names");
        for sub in ["a", "b"] {
            std::fs::create_dir_all(dir.join(sub)).unwrap();
            std::fs::write(dir.join(sub).join("photo.png"), sub).unwrap();
        }
        let out = dir.join("out.zip");
        write(&[dir.join("a/photo.png"), dir.join("b/photo.png")], &out, &AtomicBool::new(false)).unwrap();
        let files = unpack(&std::fs::read(&out).unwrap());
        assert_eq!(files["photo.png"], b"a");
        assert_eq!(files["photo (2).png"], b"b");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn cancel_stops_packing() {
        let dir = scratch("cancel");
        std::fs::write(dir.join("f.txt"), "x").unwrap();
        let err = write(&[dir.join("f.txt")], &dir.join("out.zip"), &AtomicBool::new(true)).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::Interrupted);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn symlinks_are_skipped() {
        let dir = scratch("links");
        let root = dir.join("root");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("real.txt"), "ok").unwrap();
        std::os::unix::fs::symlink("/etc/hostname", root.join("link")).unwrap();
        std::os::unix::fs::symlink(&root, root.join("loop")).unwrap();
        let out = dir.join("out.zip");
        write(&[root], &out, &AtomicBool::new(false)).unwrap();
        let files = unpack(&std::fs::read(&out).unwrap());
        assert_eq!(files.len(), 2, "{:?}", files.keys());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
