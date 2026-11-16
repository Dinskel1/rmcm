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

//! 16-bit stereo WAV backend (`Wav16.hpp`).

use crate::model::FastBitModel;
use crate::range::Range7;
use crate::stream::{ByteRead, ByteWrite, SliceReader};

// `Wav16`, faithful to the reference.
pub struct Wav16 {
    models: Vec<FastBitModel<12, 9, 30>>,
    ent: Range7,
    noise_bits: usize,
    non_noise_bits: usize,
}

impl Default for Wav16 {
    fn default() -> Self {
        Self::new()
    }
}

impl Wav16 {
    pub fn new() -> Self {
        Self {
            models: Vec::new(),
            ent: Range7::new(),
            noise_bits: 3,
            non_noise_bits: 13,
        }
    }

    fn init(&mut self) {
        self.noise_bits = 3;
        self.non_noise_bits = 16 - self.noise_bits;
        let num_ctx = 2 << (self.non_noise_bits + 2);
        self.models = vec![FastBitModel::<12, 9, 30>::new(); num_ctx];
        for m in self.models.iter_mut() {
            m.init(1 << 11);
        }
    }

    fn process_sample<const DECODE: bool, R: ByteRead, W: ByteWrite>(
        &mut self,
        input: &mut R,
        out: &mut W,
        channel: usize,
        c: u32,
    ) -> u32 {
        let mut code = if DECODE { 0 } else { c << 16 };
        let mut ctx = 1usize;
        let context = channel << self.non_noise_bits;
        for _ in 0..self.non_noise_bits {
            let m = &mut self.models[context + ctx];
            let mut p = m.get_p();
            p += u32::from(p == 0);
            let bit;
            if DECODE {
                bit = self.ent.decode(input, p, 12);
            } else {
                bit = code >> 31;
                code <<= 1;
                self.ent.encode(out, bit, p, 12);
            }
            m.update(bit);
            ctx = ctx * 2 + bit as usize;
            if DECODE {
                self.ent.normalize(input);
            }
        }

// The noisy bits are not worth modelling.
        for _ in 0..self.noise_bits {
            if DECODE {
                let bit = self.ent.decode_bit(input);
                ctx += ctx + bit as usize;
            } else {
                let bit = code >> 31;
                code <<= 1;
                self.ent.encode_bit(out, bit);
            }
        }
        (ctx as u32) ^ (1u32 << 16)
    }

    pub fn compress<W: ByteWrite>(&mut self, input: &[u8], output: &mut W, max_count: u64) -> usize {
        self.init();
        self.ent.init();
        let mut reader = SliceReader::new(input);
        let mut last_a: u16 = 0;
        let mut last_b: u16 = 0;
        let mut last_a2: u16 = 0;
        let mut last_b2: u16 = 0;
        let mut i: u64 = 0;
        while i < max_count {
            if reader.is_empty() {
                break;
            }
// through `int get()` in the reference.
            let byte = |b: Option<u8>| -> u16 { b.map(u16::from).unwrap_or(0xFF) };
            let c1 = reader.get();
            let c2 = if reader.is_empty() { None } else { Some(reader.get()) };
            let c3 = if reader.is_empty() { None } else { Some(reader.get()) };
            let c4 = if reader.is_empty() { None } else { Some(reader.get()) };
            let a = byte(Some(c1)) | (byte(c2) << 8);
            let b = byte(c3) | (byte(c4) << 8);
            let pred_a = last_a.wrapping_mul(2).wrapping_sub(last_a2);
            let pred_b = last_b.wrapping_mul(2).wrapping_sub(last_b2);
            let delta_a = a.wrapping_sub(pred_a);
            let delta_b = b.wrapping_sub(pred_b);
            self.process_sample::<false, _, _>(&mut reader, output, 0, delta_a as u32);
            self.process_sample::<false, _, _>(&mut reader, output, 1, delta_b as u32);
            last_a2 = last_a;
            last_b2 = last_b;
            last_a = a;
            last_b = b;
            i += 4;
        }
        self.ent.flush(output);
        reader.position()
    }

    pub fn decompress<W: ByteWrite>(
        &mut self,
        input: &[u8],
        output: &mut W,
        max_count: u64,
    ) -> usize {
        self.init();
        let mut reader = SliceReader::new(input);
        self.ent.init_decoder(&mut reader);
        let mut last_a: u16 = 0;
        let mut last_b: u16 = 0;
        let mut last_a2: u16 = 0;
        let mut last_b2: u16 = 0;
        let mut remaining = max_count;
        while remaining > 0 {
            let pred_a = last_a.wrapping_mul(2).wrapping_sub(last_a2);
            let pred_b = last_b.wrapping_mul(2).wrapping_sub(last_b2);
            let da = self.process_sample::<true, _, _>(&mut reader, output, 0, 0);
            let a = pred_a.wrapping_add(da as u16);
            let db = self.process_sample::<true, _, _>(&mut reader, output, 1, 0);
            let b = pred_b.wrapping_add(db as u16);
            for byte in [a as u8, (a >> 8) as u8, b as u8, (b >> 8) as u8] {
                if remaining > 0 {
                    remaining -= 1;
                    output.put(byte);
                }
            }
            last_a2 = last_a;
            last_b2 = last_b;
            last_a = a;
            last_b = b;
        }
        reader.position()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stream::VecWriter;

    fn stereo(n: usize) -> Vec<u8> {
        let mut v = Vec::with_capacity(n * 4);
        let mut a: i32 = 0;
        let mut b: i32 = 1000;
        for i in 0..n {
            a = (a + ((i * 37) % 21) as i32 - 10).clamp(-30000, 30000);
            b = (b + ((i * 53) % 17) as i32 - 8).clamp(-30000, 30000);
            v.extend_from_slice(&(a as i16).to_le_bytes());
            v.extend_from_slice(&(b as i16).to_le_bytes());
        }
        v
    }

    #[test]
    fn wav16_roundtrips() {
        let data = stereo(2000);
        let mut c = Wav16::new();
        let mut packed = VecWriter::new();
        c.compress(&data, &mut packed, u64::MAX);
        let mut d = Wav16::new();
        let mut back = VecWriter::new();
        d.decompress(&packed.data, &mut back, data.len() as u64);
        assert_eq!(back.data, data);
        assert!(packed.data.len() < data.len() / 2);
    }
}
