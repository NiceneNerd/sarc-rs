use crate::*;
use binread::BinReaderExt;
use cached::proc_macro::cached;
use indexmap::IndexMap;
use num::ToPrimitive;
use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::io::{Cursor, Seek, SeekFrom, Write};
use thiserror::Error;

const FACTORY_INFO: &str = include_str!("../data/botw_resource_factory_info.tsv");
const AGLENV_INFO: &str = include_str!("../data/aglenv_file_info.json");

type Result<T> = core::result::Result<T, SarcWriteError>;

impl BinWrite for Endian {
    fn write_options<W: Write>(
        &self,
        writer: &mut W,
        _: &binwrite::WriterOption,
    ) -> std::io::Result<()> {
        match *self {
            Self::Big => [0xFEu8, 0xFFu8].write(writer),
            Self::Little => [0xFFu8, 0xFEu8].write(writer),
        }
    }
}

#[derive(Debug, Error)]
/// An enum representing all possible errors when writing a SARC archive
pub enum SarcWriteError {
    #[error("{0} is not a valid alignment")]
    InvalidAlignmentError(usize),
    #[error(transparent)]
    IOError(#[from] std::io::Error),
}

#[cached]
fn get_botw_factory_names() -> HashSet<&'static str> {
    FACTORY_INFO
        .split('\n')
        .map(|line| line.split('\t').next().unwrap())
        .collect()
}

#[derive(Deserialize)]
#[allow(dead_code)]
struct AglEnvInfo {
    id: u16,
    i0: u16,
    ext: String,
    bext: String,
    s: Option<String>,
    align: i32,
    system: String,
    desc: String,
}

#[inline(always)]
fn align(pos: usize, alignment: usize) -> usize {
    ((pos as i64 + alignment as i64 - 1) & (0 - alignment as i64)) as usize
}

#[cached]
fn get_agl_env_alignment_requirements() -> Vec<(String, usize)> {
    serde_json::from_str::<Vec<AglEnvInfo>>(AGLENV_INFO)
        .unwrap()
        .into_iter()
        .filter_map(|e| (e.align >= 0).then(|| (e.align as usize, e)))
        .flat_map(|(align, entry)| [(entry.ext, align), (entry.bext, align)].into_iter())
        .collect()
}

/// A simple SARC archive writer
#[derive(Debug, PartialEq, Eq, Clone)]
pub struct SarcWriter {
    endian: Endian,
    legacy: bool,
    hash_multiplier: u32,
    min_alignment: usize,
    alignment_map: HashMap<String, usize>,
    /// Files to be written.
    pub files: IndexMap<String, Vec<u8>>,
}

impl SarcWriter {
    /// A simple SARC archive writer
    pub fn new(endian: Endian) -> SarcWriter {
        SarcWriter {
            endian,
            legacy: false,
            hash_multiplier: 0x65,
            alignment_map: HashMap::new(),
            files: IndexMap::new(),
            min_alignment: 4,
        }
    }

    /// Creates a new SARC writer by taking attributes and files
    /// from an existing SARC reader
    pub fn from_sarc(sarc: &Sarc) -> SarcWriter {
        SarcWriter {
            endian: sarc.endian(),
            legacy: false,
            hash_multiplier: 0x65,
            alignment_map: HashMap::new(),
            files: sarc
                .files()
                .filter_map(|f| f.name.map(|name| (name.to_owned(), f.data.to_vec())))
                .collect(),
            min_alignment: sarc.guess_min_alignment(),
        }
    }

    /// Write a SARC archive to an in-memory buffer using the specified endianness.
    /// Default alignment requirements may be automatically added.
    pub fn write_to_bytes(&mut self) -> Result<Vec<u8>> {
        let est_size: usize = 0x14
            + 0x0C
            + 0x8
            + self
                .files
                .iter()
                .map(|(n, d)| 0x10 + align(n.len() + 1, 4) + d.len())
                .sum::<usize>();
        let mut buf: Vec<u8> = Vec::with_capacity((est_size as f32 * 1.5).to_usize().unwrap());
        self.write(&mut Cursor::new(&mut buf))?;
        Ok(buf)
    }

    /// Write a SARC archive to a Write + Seek writer using the specified endianness.
    /// Default alignment requirements may be automatically added.
    pub fn write<W: Write + Seek>(&mut self, writer: &mut W) -> Result<()> {
        let mut opts = binwrite::WriterOption::default();
        opts.endian = match self.endian {
            Endian::Big => binwrite::Endian::Big,
            Endian::Little => binwrite::Endian::Little,
        };
        let multiplier = self.hash_multiplier;

        self.files.sort_by(move |name, _, name2, _| {
            Ord::cmp(&hash_name(multiplier, name), &hash_name(multiplier, name2))
        });

        writer.seek(SeekFrom::Start(0x14))?;
        ResFatHeader {
            magic: SFAT_MAGIC,
            header_size: 0x0C,
            num_files: self.files.len() as u16,
            hash_multiplier: self.hash_multiplier,
        }
        .write_options(writer, &opts)?;

        self.add_default_alignments();
        let mut alignments: Vec<usize> = Vec::with_capacity(self.files.len());

        {
            let mut rel_string_offset = 0;
            let mut rel_data_offset = 0;
            for (name, data) in self.files.iter() {
                let alignment = self.get_alignment_for_file(name, data);
                alignments.push(alignment);

                let offset = align(rel_data_offset, alignment);
                ResFatEntry {
                    name_hash: hash_name(self.hash_multiplier, name),
                    rel_name_opt_offset: 1 << 24 | (rel_string_offset / 4),
                    data_begin: offset as u32,
                    data_end: (offset + data.len()) as u32,
                }
                .write_options(writer, &opts)?;

                rel_data_offset = offset + data.len();
                rel_string_offset += align(name.len() + 1, 4) as u32;
            }
        }

        ResFntHeader {
            magic: SFNT_MAGIC,
            header_size: 0x8,
            reserved: 0,
        }
        .write_options(writer, &opts)?;
        for (name, _) in self.files.iter() {
            name.write(writer)?;
            0u8.write(writer)?;
            let pos = writer.stream_position()? as usize;
            writer.seek(SeekFrom::Start(align(pos, 4) as u64))?;
        }

        let required_alignment = alignments
            .iter()
            .fold(1, |acc, alignment| num::integer::lcm(acc, *alignment));
        let pos = writer.stream_position()? as usize;
        writer.seek(SeekFrom::Start(align(pos, required_alignment) as u64))?;
        let data_offset_begin = writer.stream_position()? as u32;
        for ((_, data), alignment) in self.files.iter().zip(alignments.iter()) {
            let pos = writer.stream_position()? as usize;
            writer.seek(SeekFrom::Start(align(pos, *alignment) as u64))?;
            data.write(writer)?;
        }

        let file_size = writer.stream_position()? as u32;
        writer.seek(SeekFrom::Start(0))?;
        ResHeader {
            magic: SARC_MAGIC,
            header_size: 0x14,
            bom: self.endian,
            file_size,
            data_offset: data_offset_begin,
            version: 0x0100,
            reserved: 0,
        }
        .write_options(writer, &opts)?;
        Ok(())
    }

    /// Add or modify a data alignment requirement for a file type. Set the alignment to 1 to revert.
    ///
    /// # Arguments
    ///
    /// * `ext` - File extension without the dot (e.g. ???bgparamlist???)
    /// * `alignment` - Data alignment (must be a power of 2)
    pub fn add_alignment_requirement(&mut self, ext: String, alignment: usize) -> Result<()> {
        if !is_valid_alignment(alignment) {
            return Err(SarcWriteError::InvalidAlignmentError(alignment));
        }
        self.alignment_map.insert(ext, alignment);
        Ok(())
    }

    fn add_default_alignments(&mut self) {
        // This is perfectly sound because all of these alignments are powers
        // of 2 and thus the calls cannot fail.
        unsafe {
            for (ext, alignment) in get_agl_env_alignment_requirements() {
                self.add_alignment_requirement(ext, alignment)
                    .unwrap_unchecked();
            }
            self.add_alignment_requirement("ksky".to_owned(), 8)
                .unwrap_unchecked();
            self.add_alignment_requirement("ksky".to_owned(), 8)
                .unwrap_unchecked();
            self.add_alignment_requirement("bksky".to_owned(), 8)
                .unwrap_unchecked();
            self.add_alignment_requirement("gtx".to_owned(), 0x2000)
                .unwrap_unchecked();
            self.add_alignment_requirement("sharcb".to_owned(), 0x1000)
                .unwrap_unchecked();
            self.add_alignment_requirement("sharc".to_owned(), 0x1000)
                .unwrap_unchecked();
            self.add_alignment_requirement("baglmf".to_owned(), 0x80)
                .unwrap_unchecked();
            self.add_alignment_requirement(
                "bffnt".to_owned(),
                match self.endian {
                    Endian::Big => 0x2000,
                    Endian::Little => 0x1000,
                },
            )
            .unwrap_unchecked();
        }
    }

    /// Set the minimum data alignment
    pub fn set_min_alignment(&mut self, alignment: usize) -> Result<()> {
        if !is_valid_alignment(alignment) {
            return Err(SarcWriteError::InvalidAlignmentError(alignment));
        }
        self.min_alignment = alignment;
        Ok(())
    }

    /// Set whether to use legacy mode (for games without a BOTW-style
    /// resource system) for addtional alignment restrictions
    pub fn set_legacy_mode(&mut self, value: bool) {
        self.legacy = value
    }

    /// Set the endianness
    pub fn set_endian(&mut self, endian: Endian) {
        self.endian = endian
    }

    /// Checks if a data slice represents a SARC archive
    pub fn is_file_sarc(data: &[u8]) -> bool {
        data.len() >= 0x20
            && (&data[0..4] == b"SARC" || (&data[0..4] == b"Yaz0" && &data[0x11..0x15] == b"SARC"))
    }

    fn get_alignment_for_new_binary_file(data: &[u8]) -> usize {
        let mut reader = Cursor::new(data);
        if data.len() <= 0x20 {
            return 1;
        }
        reader.set_position(0xC);
        if let Ok(endian) = reader.read_be() {
            reader.set_position(0x1C);
            let file_size: u32 = match endian {
                Endian::Big => reader.read_be().unwrap(),
                Endian::Little => reader.read_le().unwrap(),
            };
            if file_size as usize != data.len() {
                return 1;
            } else {
                return 1 << data[0xE];
            }
        }
        1
    }

    fn get_alignment_for_cafe_bflim(data: &[u8]) -> usize {
        if data.len() <= 0x28 || &data[data.len() - 0x28..data.len() - 0x24] != b"FLIM" {
            1
        } else {
            let mut cur = Cursor::new(&data[data.len() - 0x8..]);
            let alignment: u16 = cur.read_be().unwrap();
            alignment as usize
        }
    }

    fn get_alignment_for_file(&self, name: &str, data: &[u8]) -> usize {
        let ext = match name.rfind('.') {
            Some(idx) => &name[idx + 1..],
            None => "",
        };
        let mut alignment = self.min_alignment;
        if let Some(requirement) = self.alignment_map.get(ext) {
            alignment = num::integer::lcm(alignment, *requirement);
        }
        if self.legacy && Self::is_file_sarc(data) {
            alignment = num::integer::lcm(alignment, 0x2000);
        }
        if self.legacy || !get_botw_factory_names().contains(ext) {
            alignment = num::integer::lcm(alignment, Self::get_alignment_for_new_binary_file(data));
            if let Endian::Big = self.endian {
                alignment = num::integer::lcm(alignment, Self::get_alignment_for_cafe_bflim(data));
            }
        }
        alignment
    }
}
#[cfg(test)]
mod tests {
    use crate::{Sarc, SarcWriter};

    #[test]
    fn make_sarc() {
        for file in glob::glob("test/*").unwrap().filter_map(|f| f.ok()) {
            let data = std::fs::read(&file).unwrap();
            let sarc = Sarc::new(&data).unwrap();
            let mut sarc_writer = SarcWriter::from_sarc(&sarc);
            let new_data = sarc_writer.write_to_bytes().unwrap();
            let new_sarc = Sarc::new(&new_data).unwrap();
            if !Sarc::are_files_equal(&sarc, &new_sarc) {
                for (f1, f2) in sarc.files().zip(new_sarc.files()) {
                    if f1 != f2 {
                        std::fs::write("test/f1", f1.data).unwrap();
                        std::fs::write("test/f2", f2.data).unwrap();
                        panic!("File {:?} has changed in SARC {:?}", f1.name, file);
                    }
                }
            }
            if data != new_data {
                dbg!(sarc);
                dbg!(new_sarc);
                panic!(
                    "Roundtrip not binary identical, wrong byte at offset {}",
                    data.iter()
                        .zip(new_data.iter())
                        .enumerate()
                        .find(|(_, (b1, b2))| *b1 != *b2)
                        .unwrap()
                        .0
                );
            }
        }
    }
}
