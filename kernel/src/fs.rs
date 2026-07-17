// src/fs.rs
use alloc::vec::Vec;
use alloc::string::{String, ToString};
use alloc::collections::BTreeMap;
use core::str;
use conquer_once::spin::OnceCell;
use spin::Mutex;

pub static FILESYSTEM: OnceCell<Mutex<OverlayFileSystem>> = OnceCell::uninit();

pub type Fd = usize;

// mtime for TAR (system) files: no meaningful timestamp -> render as "—".
pub const MTIME_UNKNOWN: u64 = 0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FsError {
    NotFound,
    BadFd,
    NotWritable,
    InvalidSeek,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileMeta {
    pub size: u64,
    pub mtime: u64, // sortable timestamp; 0 = unknown
}

bitflags::bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct OpenFlags: u32 {
        const READ   = 1 << 0;
        const WRITE  = 1 << 1;
        const CREATE = 1 << 2;
        const APPEND = 1 << 3;
        const TRUNC  = 1 << 4;
    }
}

#[derive(Debug, Clone, Copy)]
pub enum SeekFrom {
    Start(u64),
    Current(i64),
    End(i64),
}

struct RamFile {
    data: Vec<u8>,
    mtime: u64,
}

struct OpenFile {
    path: String,
    offset: usize,
    flags: OpenFlags,
}

pub struct OverlayFileSystem<'a> {
    ram_files: BTreeMap<String, RamFile>,
    tar_fs: TarFileSystem<'a>,
    open_files: BTreeMap<Fd, OpenFile>,
    next_fd: Fd,
}

impl<'a> OverlayFileSystem<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Self {
            ram_files: BTreeMap::new(),
            tar_fs: TarFileSystem::new(data),
            open_files: BTreeMap::new(),
            next_fd: 3, // 0/1/2 reserved for stdin/stdout/stderr
        }
    }

    // Zero-copy read: borrow the bytes (use while holding the FS lock).
    pub fn read_slice(&self, name: &str) -> Option<&[u8]> {
        if let Some(f) = self.ram_files.get(name) {
            return Some(f.data.as_slice());
        }
        self.tar_fs.read_file(name)
    }

    // Owned read: kept for compatibility (allocates). Prefer read_slice / read().
    pub fn read_file(&self, name: &str) -> Option<Vec<u8>> {
        self.read_slice(name).map(|s| s.to_vec())
    }

    pub fn metadata(&self, name: &str) -> Option<FileMeta> {
        if let Some(f) = self.ram_files.get(name) {
            return Some(FileMeta { size: f.data.len() as u64, mtime: f.mtime });
        }
        if let Some(slice) = self.tar_fs.read_file(name) {
            return Some(FileMeta { size: slice.len() as u64, mtime: MTIME_UNKNOWN });
        }
        None
    }

    pub fn write_file(&mut self, name: &str, data: &[u8]) {
        let mtime = current_timestamp();
        self.ram_files.insert(name.to_string(), RamFile { data: data.to_vec(), mtime });
    }

    pub fn append_file(&mut self, name: &str, data: &[u8]) -> Result<(), &'static str> {
        self.ensure_ram(name);
        let f = self.ram_files.get_mut(name).unwrap();
        f.data.extend_from_slice(data);
        f.mtime = current_timestamp();
        Ok(())
    }

    pub fn list_files(&self) -> Vec<String> {
        let mut list = Vec::new();
        for k in self.ram_files.keys() {
            list.push(k.clone());
        }
        for k in self.tar_fs.list_files() {
            if !self.ram_files.contains_key(&k) {
                list.push(k);
            }
        }
        list
    }

    pub fn remove_files(&mut self, name: &str) -> Result<(), &'static str> {
        if self.ram_files.remove(name).is_some() {
            return Ok(());
        }
        if self.tar_fs.read_file(name).is_some() {
            return Err("Cannot delete read-only files");
        }
        Err("File not found")
    }

    fn ensure_ram(&mut self, name: &str) {
        if self.ram_files.contains_key(name) {
            return;
        }
        let bytes = self.tar_fs.read_file(name).map(|s| s.to_vec()).unwrap_or_default();
        self.ram_files.insert(name.to_string(), RamFile { data: bytes, mtime: current_timestamp() });
    }

    // ── File-descriptor API ─────────────────────────────────────────

    pub fn open(&mut self, path: &str, flags: OpenFlags) -> Result<Fd, FsError> {
        let exists = self.ram_files.contains_key(path)
            || self.tar_fs.read_file(path).is_some();

        if !exists {
            if flags.contains(OpenFlags::CREATE) {
                self.ram_files.insert(
                    path.to_string(),
                    RamFile { data: Vec::new(), mtime: current_timestamp() },
                );
            } else {
                return Err(FsError::NotFound);
            }
        }

        if flags.contains(OpenFlags::TRUNC) && flags.contains(OpenFlags::WRITE) {
            self.ensure_ram(path);
            let f = self.ram_files.get_mut(path).unwrap();
            f.data.clear();
            f.mtime = current_timestamp();
        }

        let offset = if flags.contains(OpenFlags::APPEND) {
            self.read_slice(path).map(|s| s.len()).unwrap_or(0)
        } else {
            0
        };

        let fd = self.next_fd;
        self.next_fd += 1;
        self.open_files.insert(fd, OpenFile { path: path.to_string(), offset, flags });
        Ok(fd)
    }

    pub fn read(&mut self, fd: Fd, buf: &mut [u8]) -> Result<usize, FsError> {
        let (path, offset) = {
            let of = self.open_files.get(&fd).ok_or(FsError::BadFd)?;
            (of.path.clone(), of.offset)
        };

        let n = {
            let data = self.read_slice(&path).ok_or(FsError::NotFound)?;
            if offset >= data.len() {
                0
            } else {
                let end = core::cmp::min(offset + buf.len(), data.len());
                let chunk = &data[offset..end];
                buf[..chunk.len()].copy_from_slice(chunk);
                chunk.len()
            }
        };

        if let Some(of) = self.open_files.get_mut(&fd) {
            of.offset += n;
        }
        Ok(n)
    }

    pub fn write(&mut self, fd: Fd, buf: &[u8]) -> Result<usize, FsError> {
        let (path, offset, writable) = {
            let of = self.open_files.get(&fd).ok_or(FsError::BadFd)?;
            (of.path.clone(), of.offset, of.flags.contains(OpenFlags::WRITE))
        };
        if !writable {
            return Err(FsError::NotWritable);
        }

        self.ensure_ram(&path);
        {
            let f = self.ram_files.get_mut(&path).unwrap();
            let end = offset + buf.len();
            if f.data.len() < end {
                f.data.resize(end, 0);
            }
            f.data[offset..end].copy_from_slice(buf);
            f.mtime = current_timestamp();
        }

        if let Some(of) = self.open_files.get_mut(&fd) {
            of.offset = offset + buf.len();
        }
        Ok(buf.len())
    }

    pub fn seek(&mut self, fd: Fd, pos: SeekFrom) -> Result<u64, FsError> {
        let path = {
            let of = self.open_files.get(&fd).ok_or(FsError::BadFd)?;
            of.path.clone()
        };
        let len = self.read_slice(&path).map(|s| s.len() as i64).unwrap_or(0);

        let new_offset: i64 = match pos {
            SeekFrom::Start(o) => o as i64,
            SeekFrom::Current(d) => {
                let cur = self.open_files.get(&fd).ok_or(FsError::BadFd)?.offset as i64;
                cur + d
            }
            SeekFrom::End(d) => len + d,
        };

        if new_offset < 0 {
            return Err(FsError::InvalidSeek);
        }
        let of = self.open_files.get_mut(&fd).ok_or(FsError::BadFd)?;
        of.offset = new_offset as usize;
        Ok(new_offset as u64)
    }

    pub fn close(&mut self, fd: Fd) -> Result<(), FsError> {
        self.open_files.remove(&fd).map(|_| ()).ok_or(FsError::BadFd)
    }
}

// TODO(mtime) for now : returns 0 (unknown) for now. Reading the RTC live here froze the
// system (CMOS spin under load). Revisit later  likely reuse the time that
// taskbar.tick() already reads, stamped into an atomic from the compositor.
// Size + FileMeta work fully regardless; only the "Modified" column is affected.
fn current_timestamp() -> u64 {
    0
}

pub struct TarFileSystem<'a> {
    data: &'a [u8],
}

impl<'a> TarFileSystem<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Self { data }
    }

    pub fn list_files(&self) -> Vec<String> {
        let mut files = Vec::new();
        let mut ptr = 0;
        // `<=` so a header at exactly data.len()-512 isn't skipped.
        while ptr + 512 <= self.data.len() {
            let header = &self.data[ptr..ptr + 512];
            if header[0] == 0 { break; }

            let name = parse_string(&header[0..100]);
            let size = parse_octal(&header[124..136]);

            if !name.is_empty() {
                files.push(String::from(name));
            }
            let data_blocks = (size + 511) / 512;
            ptr += 512 + (data_blocks as usize * 512);
        }
        files
    }

    pub fn read_file(&self, target_name: &str) -> Option<&'a [u8]> {
        let mut ptr = 0;
        while ptr + 512 <= self.data.len() {
            let header = &self.data[ptr..ptr + 512];
            if header[0] == 0 { break; }

            let name = parse_string(&header[0..100]);
            let size = parse_octal(&header[124..136]);

            if name.trim() == target_name.trim() {
                let start = ptr + 512;
                let end = start + size as usize;
                if end <= self.data.len() {
                    return Some(&self.data[start..end]);
                }
                return None;
            }
            let data_blocks = (size + 511) / 512;
            ptr += 512 + (data_blocks as usize * 512);
        }
        None
    }
}

fn parse_string(bytes: &[u8]) -> &str {
    str::from_utf8(bytes).unwrap_or("").trim_matches('\0')
}

fn parse_octal(bytes: &[u8]) -> u64 {
    let s = str::from_utf8(bytes).unwrap_or("0").trim_matches('\0').trim();
    u64::from_str_radix(s, 8).unwrap_or(0)
}