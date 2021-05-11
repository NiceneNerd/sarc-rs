use crate::*;
use binread::{BinRead, BinReaderExt};
use core::mem::size_of;
use derivative::*;
use std::{borrow::Cow, io::Cursor};
use thiserror::Error;

#[derive(Error, Debug)]
/// An enum representing all possible errors when reading a SARC archive
pub enum SarcError {
    #[error("File index {0} out of range")]
    OutOfRange(usize),
    #[error("Invalid {0} value: \"{1}\"")]
    InvalidData(String, String),
    #[error("A string in the name table was not terminated")]
    UnterminatedStringError,
    #[error("Invalid UTF file name")]
    InvalidFileName(#[from] std::str::Utf8Error),
    #[error(transparent)]
    ParseError(#[from] binread::Error),
}

pub type Result<T> = core::result::Result<T, SarcError>;

fn find_null(data: &[u8]) -> Result<usize> {
    data.iter()
        .position(|b| b == &0u8)
        .ok_or(SarcError::UnterminatedStringError)
}

fn read<T: BinRead>(endian: Endian, reader: &mut Cursor<&[u8]>) -> Result<T> {
    Ok(match endian {
        Endian::Big => reader.read_be()?,
        Endian::Little => reader.read_le()?,
    })
}
#[derive(Derivative)]
#[derivative(Debug)]
/// A simple SARC archive reader
pub struct Sarc<'a> {
    num_files: u16,
    entries_offset: u16,
    hash_multiplier: u32,
    data_offset: u32,
    names_offset: u32,
    endian: Endian,
    #[derivative(Debug = "ignore")]
    data: Cow<'a, [u8]>,
}

impl PartialEq for Sarc<'_> {
    /// Returns true if and only if the raw archive data is identical
    fn eq(&self, other: &Self) -> bool {
        self.data == other.data
    }
}

impl<'a> Sarc<'_> {
    /// Parses a SARC archive from binary data
    pub fn new<T>(data: T) -> Result<Sarc<'a>> where T: Into<Cow<'a, [u8]>> {
        let data = data.into();

        let mut reader = Cursor::new(data.as_ref());
        reader.set_position(6);
        let endian: Endian = Endian::read(&mut reader)?;
        reader.set_position(0);

        let header: ResHeader = read(endian, &mut reader)?;
        if header.magic != SARC_MAGIC {
            return Err(SarcError::InvalidData(
                "SARC magic".to_owned(),
                header.magic.iter().collect(),
            ));
        }
        if header.version != 0x0100 {
            return Err(SarcError::InvalidData(
                "SARC version".to_owned(),
                header.version.to_string(),
            ));
        }
        if header.header_size as usize != 0x14 {
            return Err(SarcError::InvalidData(
                "SARC header size".to_owned(),
                header.header_size.to_string(),
            ));
        }

        let fat_header: ResFatHeader = read(endian, &mut reader)?;
        if fat_header.magic != SFAT_MAGIC {
            return Err(SarcError::InvalidData(
                "SFAT magic".to_owned(),
                fat_header.magic.iter().collect(),
            ));
        }
        if fat_header.header_size as usize != 0x0C {
            return Err(SarcError::InvalidData(
                "SFAT header size".to_owned(),
                fat_header.header_size.to_string(),
            ));
        }
        if (fat_header.num_files >> 0xE) != 0 {
            return Err(SarcError::InvalidData(
                "SFAT file count".to_owned(),
                fat_header.num_files.to_string(),
            ));
        }

        let num_files = fat_header.num_files;
        let entries_offset = reader.position() as u16;
        let hash_multiplier = fat_header.hash_multiplier;
        let data_offset = header.data_offset;

        let fnt_header_offset = entries_offset as usize + 0x10 * num_files as usize;
        reader.set_position(fnt_header_offset as u64);
        let fnt_header: ResFntHeader = read(endian, &mut reader)?;
        if fnt_header.magic != SFNT_MAGIC {
            return Err(SarcError::InvalidData(
                "SFNT magic".to_owned(),
                fnt_header.magic.iter().collect(),
            ));
        }
        if fnt_header.header_size as usize != 0x08 {
            return Err(SarcError::InvalidData(
                "SFNT header size".to_owned(),
                fnt_header.header_size.to_string(),
            ));
        }

        let names_offset = reader.position() as u32;
        if data_offset < names_offset {
            return Err(SarcError::InvalidData(
                "name table offset".to_owned(),
                names_offset.to_string(),
            ));
        }
        Ok(Sarc {
            data,
            data_offset,
            endian,
            entries_offset,
            num_files,
            hash_multiplier,
            names_offset,
        })
    }

    /// Get the number of files that are stored in the archive
    pub fn file_count(&self) -> usize {
        self.num_files as usize
    }

    /// Get the offset to the beginning of file data
    pub fn data_offset(&self) -> usize {
        self.data_offset as usize
    }

    /// Get the archive endianness
    pub fn endian(&self) -> Endian {
        self.endian
    }

    /// Get a file by name
    pub fn get_file(&self, file: &str) -> Result<Option<File>> {
        if self.num_files == 0 {
            return Ok(None);
        }
        let needle_hash = hash_name(self.hash_multiplier, file);
        let mut a: u32 = 0;
        let mut b: u32 = self.num_files as u32 - 1;
        let mut reader = Cursor::new(self.data.as_ref());
        while a <= b {
            let m: u32 = (a + b) as u32 / 2;
            reader.set_position(self.entries_offset as u64 + 0x10 * m as u64);
            let hash: u32 = read(self.endian, &mut reader)?;
            if needle_hash < hash {
                b = m - 1;
            } else if needle_hash > hash {
                a = m + 1
            } else {
                return Ok(Some(self.file_at(m as usize)?));
            }
        }
        Ok(None)
    }

    /// Get a file by index. Returns error if index > file count.
    pub fn file_at(&self, index: usize) -> Result<File> {
        if index >= self.num_files as usize {
            return Err(SarcError::OutOfRange(index));
        }

        let entry_offset = self.entries_offset as usize + size_of::<ResFatEntry>() * index;
        let entry: ResFatEntry = read(self.endian, &mut Cursor::new(&self.data[entry_offset..]))?;

        Ok(File {
            name: if entry.rel_name_opt_offset != 0 {
                let name_offset = self.names_offset as usize
                    + (entry.rel_name_opt_offset & 0xFFFFFF) as usize * 4;
                let term_pos = find_null(&self.data[name_offset..])?;
                Some(std::str::from_utf8(
                    &self.data[name_offset..name_offset + term_pos],
                )?)
            } else {
                None
            },
            data: &self.data[(self.data_offset + entry.data_begin) as usize
                ..(self.data_offset + entry.data_end) as usize],
        })
    }

    /// Returns an iterator over the contained files
    pub fn files(&'_ self) -> impl Iterator<Item = File<'_>> {
        let count = self.num_files;
        (0..count).flat_map(move |i| self.file_at(i as usize).ok())
    }

    /// Guess the minimum data alignment for files that are stored in the archive
    pub fn guess_min_alignment(&self) -> usize {
        const MIN_ALIGNMENT: u32 = 4;
        let mut gcd = MIN_ALIGNMENT;
        let mut reader = Cursor::new(&self.data[self.entries_offset as usize..]);
        for _ in 0..self.num_files {
            let entry: ResFatEntry = read(self.endian, &mut reader).unwrap();
            gcd = num::integer::gcd(gcd, self.data_offset + entry.data_begin);
        }

        if !is_valid_alignment(gcd as usize) {
            return MIN_ALIGNMENT as usize;
        }
        return gcd as usize;
    }

    /// Returns true is each archive contains the same files
    pub fn are_files_equal(sarc1: &Sarc, sarc2: &Sarc) -> bool {
        if sarc1.file_count() != sarc2.file_count() {
            return false;
        }

        for (file1, file2) in sarc1.files().zip(sarc2.files()) {
            if file1 != file2 {
                return false;
            }
        }
        return true;
    }
}

#[cfg(test)]
mod tests {
    use crate::{Endian, Sarc};
    use std::fs::read;
    #[test]
    fn parse_sarc() {
        let data = read("test/Dungeon119.pack").unwrap();
        let sarc = Sarc::new(&data).unwrap();
        assert_eq!(sarc.endian(), Endian::Big);
        assert_eq!(sarc.file_count(), 10);
        assert_eq!(sarc.guess_min_alignment(), 4);
        for file in &[
            "NavMesh/CDungeon/Dungeon119/Dungeon119.shknm2",
            "Map/CDungeon/Dungeon119/Dungeon119_Static.smubin",
            "Map/CDungeon/Dungeon119/Dungeon119_Dynamic.smubin",
            "Actor/Pack/DgnMrgPrt_Dungeon119.sbactorpack",
            "Physics/StaticCompound/CDungeon/Dungeon119.shksc",
            "Map/CDungeon/Dungeon119/Dungeon119_TeraTree.sblwp",
            "Map/CDungeon/Dungeon119/Dungeon119_Clustering.sblwp",
            "Map/DungeonData/CDungeon/Dungeon119.bdgnenv",
            "Model/DgnMrgPrt_Dungeon119.sbfres",
            "Model/DgnMrgPrt_Dungeon119.Tex2.sbfres",
        ] {
            sarc.get_file(file)
                .unwrap()
                .expect(&format!("Could not find file {}", file));
        }
    }
}
