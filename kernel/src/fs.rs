// src/fs.rs
use alloc::vec::Vec;
use alloc::string::{String,ToString};
use core::str;
use conquer_once::spin::OnceCell;
use spin::Mutex;
use alloc::collections::BTreeMap;


pub static FILESYSTEM: OnceCell<Mutex<OverlayFileSystem>> = OnceCell::uninit();
pub struct OverlayFileSystem<'a> {
    ram_files: BTreeMap<String, Vec<u8>>,
    // Bottom Layer: Read-Only TAR (Original files)
    tar_fs: TarFileSystem<'a>,
}
impl<'a> OverlayFileSystem<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Self {
            ram_files: BTreeMap::new(),
            tar_fs: TarFileSystem::new(data),
        }
    }
    pub fn write_file(&mut self, name: &str, data: &[u8]) {
        self.ram_files.insert(name.to_string(), data.to_vec());
    }
    pub fn append_file(&mut self, name: &str, data: &[u8]) -> Result<(), &'static str> {
        if let Some(file_vec) = self.ram_files.get_mut(name) {
            file_vec.extend_from_slice(data);
            return Ok(());
        }
        if let Some(tar_data) = self.tar_fs.read_file(name) {
            let mut new_vec = tar_data.to_vec();
            new_vec.extend_from_slice(data);
            self.ram_files.insert(name.to_string(), new_vec);
            return Ok(());
        }
        self.write_file(name, data);
        Ok(())
    }
    pub fn read_file(&self, name: &str) -> Option<Vec<u8>> {
        if let Some(data) = self.ram_files.get(name) {
            return Some(data.clone());
        }
        // Check TAR (Convert slice to Vec to match return type)
        self.tar_fs.read_file(name).map(|slice| slice.to_vec())
    }
    pub fn list_files(&self) -> Vec<String> {
        let mut list = Vec::new();
        // 1. Add RAM files
        for k in self.ram_files.keys() {
            list.push(k.clone());
        }
        // 2. Add TAR files (only if not already in RAM)
        for k in self.tar_fs.list_files() {
            if !self.ram_files.contains_key(&k) {
                list.push(k);
            }
        }
        list
    }
    pub fn remove_files(&mut self,name:&str) -> Result<(),&str>{
        if self.ram_files.remove(name).is_some(){
            return Ok(());
        }
        if self.tar_fs.read_file(name).is_some(){
            return Err("Cannot delete read only files"); // so  assume that i store all the important files needed for the system, that is what i meant by read only
        }
        Err("File not found")
    }
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
        
        while ptr + 512 < self.data.len() {
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
        while ptr + 512 < self.data.len() {
            let header = &self.data[ptr..ptr + 512];
            if header[0] == 0 { break; }

            let name = parse_string(&header[0..100]);
            let size = parse_octal(&header[124..136]);

            if name.trim() == target_name.trim() {
                return Some(&self.data[ptr + 512 .. ptr + 512 + size as usize]);
            }

            let data_blocks = (size + 511) / 512;
            ptr += 512 + (data_blocks as usize * 512);
        }
        None
    }
}

// Helper: Convert raw bytes to a Rust string, removing nulls
fn parse_string(bytes: &[u8]) -> &str {
    str::from_utf8(bytes)
        .unwrap_or("")
        .trim_matches('\0')
}

// Helper: Convert Octal ASCII ("000014") to number (12)
fn parse_octal(bytes: &[u8]) -> u64 {
    let s = str::from_utf8(bytes).unwrap_or("0").trim_matches('\0').trim();
    u64::from_str_radix(s, 8).unwrap_or(0)
}