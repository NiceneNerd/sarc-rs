#![feature(seek_convenience)]
use binread::BinRead;
use binwrite::BinWrite;
mod parse;
mod writer;
pub use parse::Sarc;
pub use writer::SarcWriter;

const SARC_MAGIC: [char; 4] = ['S', 'A', 'R', 'C'];
const SFAT_MAGIC: [char; 4] = ['S', 'F', 'A', 'T'];
const SFNT_MAGIC: [char; 4] = ['S', 'F', 'N', 'T'];

fn hash_name(multiplier: u32, name: &str) -> u32 {
    name.chars()
        .fold(0, |hash, c| hash.wrapping_mul(multiplier) + (c as u32))
}

#[derive(Debug, Eq, PartialEq, Copy, Clone, BinRead)]
#[br(repr = u16)]
#[repr(u16)]
pub enum Endian {
    Big = 0xFFFE,
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

fn is_valid_alignment(alignment: usize) -> bool {
    alignment != 0 && (alignment & (alignment - 1)) == 0
}
