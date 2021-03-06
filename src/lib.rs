#![feature(const_slice_index)]
#![deny(missing_docs)]
//! A simple to use library for parsing and creating Nintendo SARC files in Rust.
//! Uses zero allocation parsing and handles file alignment requirements for common
//! formats and games like `The Legend of Zelda: Breath of the Wild`.
//!
//! Sample usage:
//!
//! ```
//! use sarc_rs::{Sarc, SarcWriter};
//! let data = std::fs::read("test/Dungeon119.pack").unwrap();
//! let sarc = Sarc::new(&data).unwrap(); // Read a SARC from binary data
//! for file in sarc.files() { // Iterate files in SARC
//!     if let Some(name) = file.name {
//!        println!("File name: {}", name); // Print file name
//!     }
//!     println!("File size: {}", file.data.len()); // Print data size
//! }
//! ```
use binread::BinRead;
use binwrite::BinWrite;
mod parse;
mod writer;
pub use parse::Sarc;
pub use writer::SarcWriter;

/// A file that is stored in a SARC archive.
#[derive(Debug, PartialEq, Eq)]
pub struct File<'a> {
    /// File name. May be empty for file entries that do not use the file name
    /// table.
    pub name: Option<&'a str>,
    /// File data (as a slice).
    pub data: &'a [u8],
}

const SARC_MAGIC: [char; 4] = ['S', 'A', 'R', 'C'];
const SFAT_MAGIC: [char; 4] = ['S', 'F', 'A', 'T'];
const SFNT_MAGIC: [char; 4] = ['S', 'F', 'N', 'T'];

const fn hash_name(multiplier: u32, name: &str) -> u32 {
    let mut hash = 0u32;
    let bytes = name.as_bytes();
    let mut i = 0;
    while i < name.len() {
        hash = unsafe {
            // This is sound because obvious the index is within the string
            // length.
            hash.wrapping_mul(multiplier)
                .wrapping_add(*bytes.get_unchecked(i) as u32)
        };
        i += 1;
    }
    hash
}

#[derive(Debug, Eq, PartialEq, Copy, Clone, BinRead)]
#[br(repr = u16)]
#[repr(u16)]
/// An enum to represent SARC endianness
pub enum Endian {
    /// Big Endian (Wii U)
    Big = 0xFFFE,
    /// Little Endian (Switch)
    Little = 0xFEFF,
}

/// Size = 0x14
#[derive(Debug, Eq, PartialEq, Copy, Clone, BinRead, BinWrite)]
struct ResHeader {
    magic: [char; 4],
    header_size: u16,
    bom: Endian,
    file_size: u32,
    data_offset: u32,
    version: u16,
    reserved: u16,
}

/// Size = 0x0C
#[derive(Debug, Copy, Clone, Eq, PartialEq, BinRead, BinWrite)]
struct ResFatHeader {
    magic: [char; 4],
    header_size: u16,
    num_files: u16,
    hash_multiplier: u32,
}

/// Size = 0x10
#[derive(Debug, PartialEq, Eq, Copy, Clone, BinRead, BinWrite)]
struct ResFatEntry {
    name_hash: u32,
    rel_name_opt_offset: u32,
    data_begin: u32,
    data_end: u32,
}

/// Size = 0x8
#[derive(Debug, PartialEq, Eq, Copy, Clone, BinRead, BinWrite)]
struct ResFntHeader {
    magic: [char; 4],
    header_size: u16,
    reserved: u16,
}

#[inline(always)]
const fn is_valid_alignment(alignment: usize) -> bool {
    alignment != 0 && (alignment & (alignment - 1)) == 0
}
