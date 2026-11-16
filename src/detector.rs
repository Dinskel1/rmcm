/*  rmcm - a Rust port of the MCM file compressor

    Copyright (C) 2015, Google Inc.
    Authors: Mathieu Chartier
    Rust port: Dinskel1 (github.com/Dinskel1)

    LICENSE

    This file is part of the rmcm file compressor.

    rmcm is free software: you can redistribute it and/or modify
    it under the terms of the GNU Affero General Public License as published by
    the Free Software Foundation, either version 3 of the License, or
    (at your option) any later version.

    rmcm is distributed in the hope that it will be useful,
    but WITHOUT ANY WARRANTY; without even the implied warranty of
    MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
    GNU Affero General Public License for more details.

    You should have received a copy of the GNU Affero General Public License
    along with rmcm.  If not, see <http://www.gnu.org/licenses/>.
*/

//! Text/binary/WAV segmentation (`Detector.hpp`).

use std::collections::VecDeque;

use crate::dict::Builder;
use crate::utf8::Utf8Decoder;
use crate::stream::ByteRead;

pub const KB: usize = 1024;

// one go (the reference uses 256 KB).
pub const DETECT_BUFFER_SIZE: usize = 256 * KB;

const MIN_TEXT_LEN: usize = 146;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
pub enum Profile {
    Text = 0,
    Binary = 1,
    Wave16 = 2,
    Eof = 3,
    Count = 4,
    Detect = 5,
}

impl Profile {
    pub fn from_u8(v: u8) -> Profile {
        match v {
            0 => Profile::Text,
            1 => Profile::Binary,
            2 => Profile::Wave16,
            3 => Profile::Eof,
            4 => Profile::Count,
            _ => Profile::Detect,
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            Profile::Text => "text",
            Profile::Binary => "binary",
            Profile::Wave16 => "wav16",
            Profile::Eof => "eof",
            Profile::Count => "count",
            Profile::Detect => "detect",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DetectedBlock {
    profile: Profile,
    length: u64,
}

impl DetectedBlock {
    pub const LENGTH_BYTES_SHIFT: u32 = 6;
    pub const DATA_PROFILE_MASK: u8 = (1 << Self::LENGTH_BYTES_SHIFT) - 1;

    pub fn new(profile: Profile, length: u64) -> Self {
        Self { profile, length }
    }

    pub fn profile(&self) -> Profile {
        self.profile
    }

    pub fn length(&self) -> u64 {
        self.length
    }

    pub fn set_length(&mut self, length: u64) {
        self.length = length;
    }

    pub fn extend(&mut self, len: u64) {
        self.length += len;
    }

    pub fn pop(&mut self, count: u64) {
        debug_assert!(self.length >= count);
        self.length -= count;
    }

// two bytes even though it is exactly 0xFF00.
    pub fn calculate_length_bytes(len: u64) -> usize {
        if len & 0xFF00_0000 != 0 {
            4
        } else if len & 0xFF_0000 != 0 {
            3
        } else if len & 0xFF00 != 0 {
            2
        } else {
            1
        }
    }

    fn get_length_bytes(header: u8) -> usize {
        ((header >> Self::LENGTH_BYTES_SHIFT) as usize) + 1
    }

    pub fn header_size(header: u8) -> usize {
        1 + Self::get_length_bytes(header)
    }

    pub fn write(&self, out: &mut [u8]) -> usize {
        let mut enc_len = self.length - 1;
        let length_bytes = Self::calculate_length_bytes(enc_len);
        out[0] = (self.profile as u8) | (((length_bytes - 1) as u8) << Self::LENGTH_BYTES_SHIFT);
        for i in 0..length_bytes {
            out[1 + i] = enc_len as u8;
            enc_len >>= 8;
        }
        1 + length_bytes
    }

    pub fn read(bytes: &[u8]) -> (Self, usize) {
        let header = bytes[0];
        let profile = Profile::from_u8(header & Self::DATA_PROFILE_MASK);
        let length_bytes = Self::get_length_bytes(header);
        let mut length: u64 = 0;
        for i in 0..length_bytes {
            length |= (bytes[1 + i] as u64) << (i * 8);
        }
        (
            Self {
                profile,
                length: length + 1,
            },
            1 + length_bytes,
        )
    }
}

fn is_forbidden(c: u8) -> bool {
    matches!(c, 0..=8 | 11 | 12 | 14..=31)
}

fn read_bytes(buffer: &[u8], pos: usize, bytes: usize, big_endian: bool) -> u32 {
    if pos + bytes > buffer.len() {
        return 0;
    }
    let mut w: u32 = 0;
    if big_endian {
        for i in 0..bytes {
            w = (w << 8) | buffer[pos + i] as u32;
        }
    } else {
        for i in 0..bytes {
            w |= (buffer[pos + i] as u32) << (i * 8);
        }
    }
    w
}

#[derive(Default)]
pub struct Analyzer {
    blocks: Vec<DetectedBlock>,
    dict_builder: Builder,
// Detector state.
    buffer: VecDeque<u8>,
    saved_blocks: VecDeque<DetectedBlock>,
    last_word: u32,
}

impl Analyzer {
    pub fn new() -> Self {
        Self {
            blocks: Vec::new(),
            dict_builder: Builder::new(),
            buffer: VecDeque::with_capacity(DETECT_BUFFER_SIZE),
            saved_blocks: VecDeque::new(),
            last_word: 0,
        }
    }

    pub fn blocks(&self) -> &[DetectedBlock] {
        &self.blocks
    }

    pub fn dict_builder(&self) -> &Builder {
        &self.dict_builder
    }

    pub fn dict_builder_mut(&mut self) -> &mut Builder {
        &mut self.dict_builder
    }

// `Analyzer::analyze`.
    pub fn analyze(&mut self, stream: &mut impl ByteRead) {
        loop {
            let mut block = self.detect_block(stream);
            if block.profile() == Profile::Eof {
                break;
            }
            for i in 0..block.length() {
                let c = self.pop_char(stream);
                let Some(c) = c else {
                    block.set_length(i);
                    break;
                };
                if block.profile() == Profile::Text {
                    self.dict_builder.add_char(c);
                }
            }
            let size = self.blocks.len();
            if size > 0 && self.blocks[size - 1].profile() == block.profile() {
                self.blocks[size - 1].extend(block.length());
            } else {
                const MIN_BINARY_LENGTH: u64 = 1;
                if block.profile() == Profile::Text && size >= 2 {
                    let b1 = self.blocks[size - 1];
                    let b2 = self.blocks[size - 2];
                    if b1.profile() == Profile::Binary
                        && b2.profile() == Profile::Text
                        && b1.length() < MIN_BINARY_LENGTH
                    {
                        self.blocks[size - 2].extend(b1.length() + block.length());
                        self.blocks.pop();
                        continue;
                    }
                }
                self.blocks.push(block);
            }
        }
    }

    fn refill_read(&mut self, stream: &mut impl ByteRead) {
        const CHUNK: usize = 4 * KB;
        let mut chunk = [0u8; CHUNK];
        loop {
            let remain = DETECT_BUFFER_SIZE.saturating_sub(self.buffer.len());
            if remain == 0 {
                break;
            }
            let want = CHUNK.min(remain);
            let n = stream.read(&mut chunk[..want]);
            self.buffer.extend(&chunk[..n]);
            if n == 0 || n < want {
                break;
            }
        }
    }

    fn pop_char(&mut self, stream: &mut impl ByteRead) -> Option<u8> {
        if self.buffer.is_empty() {
            self.refill_read(stream);
            if self.buffer.is_empty() {
                return None;
            }
        }
        self.buffer.pop_front()
    }

    fn detect_block(&mut self, stream: &mut impl ByteRead) -> DetectedBlock {
        if let Some(block) = self.saved_blocks.pop_front() {
            return block;
        }
        self.refill_read(stream);
        let buffer_size = self.buffer.len();
        if buffer_size == 0 {
            return DetectedBlock::new(Profile::Eof, 0);
        }
        let buffer: Vec<u8> = self.buffer.iter().copied().collect();

        let mut binary_len = 0usize;
        while binary_len < buffer_size {
            let mut decoder = Utf8Decoder::<true>::new();
            let mut text_len = 0usize;
            while binary_len + text_len < buffer_size {
                let pos = binary_len + text_len;
                if self.last_word == 0x5249_4646 {
                    self.refill_read(stream);
                    let new_buffer: Vec<u8> = self.buffer.iter().copied().collect();
                    if let Some(block) = self.try_wave(&new_buffer, pos) {
                        return block;
                    }
                }
                let c = buffer[pos];
                self.last_word = (self.last_word << 8) | c as u32;
                decoder.update(c as u32);
                if decoder.error() || is_forbidden(c) {
                    break;
                }
                text_len += 1;
            }
            if text_len > MIN_TEXT_LEN {
                if binary_len == 0 {
                    return DetectedBlock::new(Profile::Text, text_len as u64);
                } else {
                    break;
                }
            } else {
                binary_len += text_len;
                if binary_len >= buffer_size {
                    break;
                }
                binary_len += 1;
            }
        }
        DetectedBlock::new(Profile::Binary, binary_len as u64)
    }

    fn try_wave(&mut self, buffer: &[u8], pos: usize) -> Option<DetectedBlock> {
        let mut fpos = pos as u32;
        let chunk_size = read_bytes(buffer, fpos as usize, 4, false);
        fpos += 4;
        let format = read_bytes(buffer, fpos as usize, 4, true);
        fpos += 4;
        let subchunk_id = read_bytes(buffer, fpos as usize, 4, true);
        fpos += 4;
        if format != 0x5741_5645 || subchunk_id != 0x666d_7420 {
            return None;
        }
        let subchunk_size = read_bytes(buffer, fpos as usize, 4, false);
        fpos += 4;
        if subchunk_size != 16 && subchunk_size != 18 {
            return None;
        }
        let audio_format = read_bytes(buffer, fpos as usize, 2, false);
        fpos += 2;
        let num_channels = read_bytes(buffer, fpos as usize, 2, false);
        fpos += 2;
        if audio_format != 1 || num_channels != 2 {
            return None;
        }
        fpos = fpos.wrapping_add(subchunk_size - 6);
        let _bits_per_sample = read_bytes(buffer, fpos as usize, 2, false);
        fpos += 2;
        for _ in 0..5 {
            let subchunk2_id = read_bytes(buffer, fpos as usize, 4, true);
            fpos += 4;
            let subchunk2_size = read_bytes(buffer, fpos as usize, 4, false);
            fpos += 4;
            if subchunk2_id == 0x6461_7461 {
                if subchunk2_size >= chunk_size {
                    return None;
                }
                self.saved_blocks
                    .push_back(DetectedBlock::new(Profile::Wave16, chunk_size as u64));
                return Some(DetectedBlock::new(Profile::Binary, fpos as u64));
            } else {
                fpos = fpos.wrapping_add(subchunk2_size);
                if fpos as usize >= buffer.len() {
                    return None;
                }
            }
        }
        None
    }

// Profile histogram, for `-v` style reporting.
    pub fn dump(&self) {
        for (profile, blocks, bytes) in self.stats() {
            if bytes > 0 {
                println!(
                    "{} : {}({})",
                    profile.name(),
                    blocks,
                    crate::format::pretty_size(bytes)
                );
            }
        }
    }

    pub fn stats(&self) -> [(Profile, u64, u64); 3] {
        let mut out = [
            (Profile::Text, 0u64, 0u64),
            (Profile::Binary, 0, 0),
            (Profile::Wave16, 0, 0),
        ];
        for block in &self.blocks {
            for entry in out.iter_mut() {
                if entry.0 == block.profile() {
                    entry.1 += 1;
                    entry.2 += block.length();
                }
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stream::SliceReader;

    #[test]
    fn block_header_roundtrips() {
        for length in [1u64, 2, 255, 256, 0xFF00, 0xFF01, 0x1_0000, 0xFF_0000, 1 << 32] {
            let block = DetectedBlock::new(Profile::Text, length);
            let mut buf = [0u8; 8];
            let n = block.write(&mut buf);
            let (back, m) = DetectedBlock::read(&buf);
            assert_eq!(n, m);
            assert_eq!(back, block, "length {length}");
        }
    }

    #[test]
    fn classifies_text_and_binary() {
        let mut text = Vec::new();
        for _ in 0..40 {
            text.extend_from_slice(b"the quick brown fox jumps over the lazy dog. ");
        }
        let mut data = text.clone();
        data.extend_from_slice(&[0u8, 1, 2, 3, 0xFF, 0xFE, 0x00, 0x10]);
        for _ in 0..400 {
            data.push((data.len() % 251) as u8 ^ 0x5A);
        }

        let mut analyzer = Analyzer::new();
        let mut reader = SliceReader::new(&data);
        analyzer.analyze(&mut reader);
        let blocks = analyzer.blocks();
        assert!(!blocks.is_empty());
        let total: u64 = blocks.iter().map(|b| b.length()).sum();
        assert_eq!(total, data.len() as u64);
        assert_eq!(blocks[0].profile(), Profile::Text);
        assert!(blocks.iter().any(|b| b.profile() == Profile::Binary));
        assert!(!analyzer.dict_builder().words().get_words(0).is_empty());
    }

    #[test]
    fn empty_input_yields_no_blocks() {
        let mut analyzer = Analyzer::new();
        let mut reader = SliceReader::new(&[]);
        analyzer.analyze(&mut reader);
        assert!(analyzer.blocks().is_empty());
    }

    #[test]
    fn tiny_input_is_one_binary_block() {
        let data = b"hello".to_vec();
        let mut analyzer = Analyzer::new();
        let mut reader = SliceReader::new(&data);
        analyzer.analyze(&mut reader);
        assert_eq!(analyzer.blocks().len(), 1);
        assert_eq!(analyzer.blocks()[0].profile(), Profile::Binary);
        assert_eq!(analyzer.blocks()[0].length(), 5);
    }
}
