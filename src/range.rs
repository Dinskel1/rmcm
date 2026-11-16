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

//! Byte-oriented binary arithmetic coder.

use crate::stream::{ByteRead, ByteWrite};

const TOP_BITS: u32 = 24;
const TOP_VALUE: u32 = 1 << TOP_BITS;
const TOP: u32 = 0xFFFF_FFFF;

#[derive(Clone, Debug)]
pub struct Range7 {
    range: u32,
    code: u32,
    cache_size: u32,
    low: u64,
    cache: u8,
}

impl Default for Range7 {
    fn default() -> Self {
        Self::new()
    }
}

impl Range7 {
    pub fn new() -> Self {
        let mut r = Self {
            range: 0,
            code: 0,
            cache_size: 0,
            low: 0,
            cache: 0,
        };
        r.init();
        r
    }

    pub fn init(&mut self) {
        self.code = 0;
        self.low = 0;
        self.range = TOP;
        self.cache = 0;
        self.cache_size = 1;
    }

    #[inline]
    fn shift_low<W: ByteWrite + ?Sized>(&mut self, out: &mut W) {
        if (self.low as u32) < (0xFFu32 << TOP_BITS) || (self.low >> 32) != 0 {
            let mut temp = self.cache;
            loop {
                out.put(temp.wrapping_add((self.low >> 32) as u8));
                temp = 0xFF;
                self.cache_size -= 1;
                if self.cache_size == 0 {
                    break;
                }
            }
            self.cache = (self.low >> 24) as u8;
        }
        self.cache_size += 1;
        self.low = ((self.low << 8) as u32) as u64;
    }

    #[inline]
    pub fn increase_range<W: ByteWrite + ?Sized>(&mut self, out: &mut W) {
        while self.range < TOP_VALUE {
            self.range <<= 8;
            self.shift_low(out);
        }
    }

    #[inline]
    pub fn get_decoded_bit(&mut self, p: u32, shift: u32) -> u32 {
        let mid = (self.range >> shift).wrapping_mul(p);
        let bit = u32::from(self.code < mid);
        if bit != 0 {
            self.range = mid;
        } else {
            self.code = self.code.wrapping_sub(mid);
            self.range -= mid;
        }
        bit
    }

    #[inline]
    pub fn encode<W: ByteWrite + ?Sized>(&mut self, out: &mut W, bit: u32, p: u32, shift: u32) {
        debug_assert!(p < (1u32 << shift), "p out of range: {p}");
        debug_assert!(p != 0);
        let mid = (self.range >> shift).wrapping_mul(p);
        if bit != 0 {
            self.range = mid;
        } else {
            self.low += mid as u64;
            self.range -= mid;
        }
        self.increase_range(out);
    }

    #[inline]
    pub fn decode<R: ByteRead + ?Sized>(&mut self, inp: &mut R, p: u32, shift: u32) -> u32 {
        debug_assert!(p < (1u32 << shift), "p out of range: {p}");
        debug_assert!(p != 0);
        let bit = self.get_decoded_bit(p, shift);
        self.normalize(inp);
        bit
    }

    #[inline]
    pub fn encode_bit<W: ByteWrite + ?Sized>(&mut self, out: &mut W, bit: u32) {
        self.range >>= 1;
        if bit != 0 {
            self.low += self.range as u64;
        }
        while self.range < TOP_VALUE {
            self.range <<= 8;
            self.shift_low(out);
        }
    }

    #[inline]
    pub fn decode_bit<R: ByteRead + ?Sized>(&mut self, inp: &mut R) -> u32 {
        self.range >>= 1;
        if self.code >= self.range {
            self.code -= self.range;
            self.normalize(inp);
            1
        } else {
            self.normalize(inp);
            0
        }
    }

    pub fn encode_bits<W: ByteWrite + ?Sized>(
        &mut self,
        out: &mut W,
        value: u32,
        num_bits: u32,
    ) {
        for i in (0..num_bits).rev() {
            self.range >>= 1;
            let bit = (value >> i) & 1;
            self.low += (self.range & 0u32.wrapping_sub(bit)) as u64;
            while self.range < TOP_VALUE {
                self.range <<= 8;
                self.shift_low(out);
            }
        }
    }

// state lives in locals as in the original.
    pub fn decode_direct_bits<R: ByteRead + ?Sized>(&mut self, inp: &mut R, num_bits: u32) -> u32 {
        let mut range = self.range;
        let mut code = self.code;
        let mut result = 0u32;
        for _ in 0..num_bits {
            range >>= 1;
            let t = code.wrapping_sub(range) >> 31;
            code = code.wrapping_sub(range & t.wrapping_sub(1));
            result = (result << 1) | (1u32.wrapping_sub(t));
            if range < TOP_VALUE {
                code = (code << 8) | (inp.get() as u32);
                range <<= 8;
            }
        }
        self.range = range;
        self.code = code;
        result
    }

    pub fn decode_direct_bit<R: ByteRead + ?Sized>(&mut self, inp: &mut R) -> u32 {
        self.range >>= 1;
        let t = self.code.wrapping_sub(self.range) >> 31;
        self.code = self.code.wrapping_sub(self.range & t.wrapping_sub(1));
        self.normalize(inp);
        t ^ 1
    }

    pub fn encode_symbol<W: ByteWrite + ?Sized>(
        &mut self,
        out: &mut W,
        start: u32,
        size: u32,
        total: u32,
    ) {
        debug_assert!(size > 0 && start < total && start + size <= total);
        self.range /= total;
        self.low += (start * self.range) as u64;
        self.range *= size;
        while self.range < TOP_VALUE {
            self.range <<= 8;
            self.shift_low(out);
        }
    }

    pub fn decode_symbol<R: ByteRead + ?Sized>(&mut self, inp: &mut R, start: u32, size: u32) {
        self.code -= start * self.range;
        self.range *= size;
        self.normalize(inp);
    }

    pub fn encode_direct<W: ByteWrite + ?Sized>(&mut self, out: &mut W, start: u32, total: u32) {
        debug_assert!(start < total);
        self.range /= total;
        self.low += (start * self.range) as u64;
        while self.range < TOP_VALUE {
            self.range <<= 8;
            self.shift_low(out);
        }
    }

    pub fn decode_byte<R: ByteRead + ?Sized>(&mut self, inp: &mut R) -> u32 {
        self.range >>= 8;
        let start = self.code / self.range;
        self.code -= start * self.range;
        self.normalize(inp);
        start
    }

    pub fn get_threshold(&mut self, total: u32) -> u32 {
        self.range /= total;
        self.code / self.range
    }

    pub fn decode_direct<R: ByteRead + ?Sized>(&mut self, inp: &mut R, total: u32) -> u32 {
        let start = self.get_threshold(total);
        self.code -= start * self.range;
        self.normalize(inp);
        start
    }

    pub fn flush<W: ByteWrite + ?Sized>(&mut self, out: &mut W) {
        for _ in 0..5 {
            self.shift_low(out);
        }
    }

// Prime the decoder from the first 5 bytes.
    pub fn init_decoder<R: ByteRead + ?Sized>(&mut self, inp: &mut R) {
        self.init();
        self.code = 0;
        self.range = TOP;
        for _ in 0..5 {
            self.code = (self.code << 8) | (inp.get() as u32);
        }
    }

    #[inline]
    pub fn normalize<R: ByteRead + ?Sized>(&mut self, inp: &mut R) {
        while self.range < TOP_VALUE {
            self.code = (self.code << 8) | (inp.get() as u32);
            self.range <<= 8;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stream::{SliceReader, VecWriter};

    fn bits(n: usize) -> impl Iterator<Item = (u32, u32)> {
        (0..n).map(|i| {
            let p = 1 + ((i as u64).wrapping_mul(2654435761) % 4095) as u32;
            let b = ((i as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15) >> 63) as u32;
            (b, p)
        })
    }

    #[test]
    fn modelled_bits_roundtrip() {
        let seq: Vec<(u32, u32)> = bits(20_000).collect();
        let mut enc = Range7::new();
        let mut w = VecWriter::new();
        for &(b, p) in &seq {
            enc.encode(&mut w, b, p, 12);
        }
        enc.flush(&mut w);

        let mut dec = Range7::new();
        let mut r = SliceReader::new(w.as_slice());
        dec.init_decoder(&mut r);
        for &(b, p) in &seq {
            assert_eq!(dec.decode(&mut r, p, 12), b);
        }
    }

    #[test]
    fn raw_bits_roundtrip() {
        let seq: Vec<u32> = (0..5_000)
            .map(|i| ((i as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15) >> 63) as u32)
            .collect();
        let mut enc = Range7::new();
        let mut w = VecWriter::new();
        for &b in &seq {
            enc.encode_bit(&mut w, b);
        }
        enc.flush(&mut w);

        let mut dec = Range7::new();
        let mut r = SliceReader::new(w.as_slice());
        dec.init_decoder(&mut r);
        for &b in &seq {
            assert_eq!(dec.decode_bit(&mut r), b);
        }
    }

    #[test]
    fn direct_bits_roundtrip() {
        let mut enc = Range7::new();
        let mut w = VecWriter::new();
        enc.encode_bits(&mut w, 0x1A2B3C, 24);
        enc.encode_bits(&mut w, 0, 1);
        enc.encode_bits(&mut w, 1, 1);
        enc.flush(&mut w);

        let mut dec = Range7::new();
        let mut r = SliceReader::new(w.as_slice());
        dec.init_decoder(&mut r);
        assert_eq!(dec.decode_direct_bits(&mut r, 24), 0x1A2B3C);
        assert_eq!(dec.decode_direct_bits(&mut r, 1), 0);
        assert_eq!(dec.decode_direct_bits(&mut r, 1), 1);
    }

    #[test]
    fn symbol_coder_roundtrip() {
        let syms: Vec<u32> = (0..2_000u32).map(|i| (i * 37 + 11) % 256).collect();
        let mut enc = Range7::new();
        let mut w = VecWriter::new();
        for &s in &syms {
            enc.encode_symbol(&mut w, s, 1, 256);
        }
        enc.flush(&mut w);

        let mut dec = Range7::new();
        let mut r = SliceReader::new(w.as_slice());
        dec.init_decoder(&mut r);
        for &s in &syms {
            assert_eq!(dec.decode_byte(&mut r), s);
        }
    }
}
