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

//! Minimal byte-stream abstractions, ported from `Stream.hpp`.

// A source of bytes. `get` past the end must return `0`, matching `fgetc`-era
// behaviour in the reference implementation.
pub trait ByteRead {
    fn get(&mut self) -> u8;

    fn read(&mut self, out: &mut [u8]) -> usize {
        for b in out.iter_mut() {
            *b = self.get();
        }
        out.len()
    }

    fn read_bytes(&mut self, out: &mut [u8]) {
        for b in out.iter_mut() {
            *b = self.get();
        }
    }

    fn eof(&mut self) -> bool {
        false
    }

// Bytes consumed so far.
    fn position(&self) -> usize {
        0
    }
}

// A sink for bytes.
pub trait ByteWrite {
    fn put(&mut self, b: u8);

    fn write(&mut self, buf: &[u8]) {
        for &b in buf {
            self.put(b);
        }
    }

    fn write_bytes(&mut self, buf: &[u8]) {
        for &b in buf {
            self.put(b);
        }
    }
}

impl<T: ByteRead + ?Sized> ByteRead for &mut T {
    #[inline]
    fn get(&mut self) -> u8 {
        (**self).get()
    }

    #[inline]
    fn read(&mut self, out: &mut [u8]) -> usize {
        (**self).read(out)
    }

    fn eof(&mut self) -> bool {
        (**self).eof()
    }

    fn position(&self) -> usize {
        (**self).position()
    }
}

impl<T: ByteWrite + ?Sized> ByteWrite for &mut T {
    #[inline]
    fn put(&mut self, b: u8) {
        (**self).put(b)
    }

    #[inline]
    fn write(&mut self, buf: &[u8]) {
        (**self).write(buf)
    }
}

#[derive(Clone, Debug)]
pub struct SliceReader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> SliceReader<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    pub fn position(&self) -> usize {
        self.pos
    }

    pub fn remaining(&self) -> usize {
        self.data.len().saturating_sub(self.pos)
    }

    pub fn is_empty(&self) -> bool {
        self.remaining() == 0
    }
}

impl ByteRead for SliceReader<'_> {
    #[inline]
    fn get(&mut self) -> u8 {
        if self.pos < self.data.len() {
            let b = self.data[self.pos];
            self.pos += 1;
            b
        } else {
            0
        }
    }

    fn read(&mut self, out: &mut [u8]) -> usize {
        let n = self.remaining().min(out.len());
        out[..n].copy_from_slice(&self.data[self.pos..self.pos + n]);
        self.pos += n;
        n
    }

    fn eof(&mut self) -> bool {
        self.pos >= self.data.len()
    }

    fn position(&self) -> usize {
        self.pos
    }
}

#[derive(Clone, Debug, Default)]
pub struct VecWriter {
    pub data: Vec<u8>,
}

impl VecWriter {
    pub fn new() -> Self {
        Self { data: Vec::new() }
    }

    pub fn with_capacity(n: usize) -> Self {
        Self {
            data: Vec::with_capacity(n),
        }
    }

    pub fn len(&self) -> usize {
        self.data.len()
    }

    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }

    pub fn into_inner(self) -> Vec<u8> {
        self.data
    }

    pub fn as_slice(&self) -> &[u8] {
        &self.data
    }
}

// filter whose read side is never used.
impl ByteRead for VecWriter {
    fn get(&mut self) -> u8 {
        0
    }

    fn read(&mut self, _out: &mut [u8]) -> usize {
        0
    }
}

// A `Vec<u8>` is the natural append-only sink.
impl ByteWrite for Vec<u8> {
    fn put(&mut self, b: u8) {
        self.push(b);
    }

    fn write(&mut self, buf: &[u8]) {
        self.extend_from_slice(buf);
    }
}

impl ByteWrite for VecWriter {
    #[inline]
    fn put(&mut self, b: u8) {
        self.data.push(b);
    }

    fn write(&mut self, buf: &[u8]) {
        self.data.extend_from_slice(buf);
    }
}

#[derive(Clone, Debug, Default)]
pub struct CountingWriter {
    pub count: u64,
}

impl ByteWrite for CountingWriter {
    #[inline]
    fn put(&mut self, _b: u8) {
        self.count += 1;
    }

    fn write(&mut self, buf: &[u8]) {
        self.count += buf.len() as u64;
    }
}

pub fn put_u16_le(out: &mut impl ByteWrite, v: u16) {
    out.put(v as u8);
    out.put((v >> 8) as u8);
}

pub fn put_u16_be(out: &mut impl ByteWrite, v: u16) {
    out.put((v >> 8) as u8);
    out.put(v as u8);
}

pub fn put_u32_le(out: &mut impl ByteWrite, v: u32) {
    out.write_bytes(&v.to_le_bytes());
}

pub fn put_u64_le(out: &mut impl ByteWrite, v: u64) {
    out.write_bytes(&v.to_le_bytes());
}

pub fn get_u16_le(inp: &mut impl ByteRead) -> u16 {
    let lo = inp.get() as u16;
    let hi = inp.get() as u16;
    lo | (hi << 8)
}

pub fn get_u32_le(inp: &mut impl ByteRead) -> u32 {
    let mut b = [0u8; 4];
    inp.read_bytes(&mut b);
    u32::from_le_bytes(b)
}

pub fn get_u64_le(inp: &mut impl ByteRead) -> u64 {
    let mut b = [0u8; 8];
    inp.read_bytes(&mut b);
    u64::from_le_bytes(b)
}

pub fn leb128_encode(out: &mut impl ByteWrite, mut value: u64) {
    loop {
        let mut b = (value & 0x7F) as u8;
        value >>= 7;
        if value != 0 {
            b |= 0x80;
        }
        out.put(b);
        if value == 0 {
            break;
        }
    }
}

// mirrors the C++ loop (it does not error out).
pub fn leb128_decode(inp: &mut impl ByteRead) -> u64 {
    let mut result: u64 = 0;
    let mut shift = 0;
    loop {
        let b = inp.get();
        result |= ((b & 0x7F) as u64) << shift;
        shift += 7;
        if b & 0x80 == 0 || shift >= 64 {
            break;
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn leb128_roundtrip() {
        for &v in &[0u64, 1, 127, 128, 1234, u32::MAX as u64, u64::MAX] {
            let mut w = VecWriter::new();
            leb128_encode(&mut w, v);
            let mut r = SliceReader::new(&w.data);
            assert_eq!(leb128_decode(&mut r), v);
            assert!(r.is_empty());
        }
    }

    #[test]
    fn leb128_matches_known_bytes() {
        let mut w = VecWriter::new();
        leb128_encode(&mut w, 624485);
        assert_eq!(w.data, vec![0xE5, 0x8E, 0x26]);
    }

    #[test]
    fn slice_reader_saturates_with_zero() {
        let mut r = SliceReader::new(&[1, 2]);
        assert_eq!(r.get(), 1);
        assert_eq!(r.get(), 2);
        assert_eq!(r.get(), 0);
        assert_eq!(r.get(), 0);
    }
}
