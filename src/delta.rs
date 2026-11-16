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

//! Delta filter (`DeltaFilter.hpp`).

use crate::filter::Transform;

#[inline]
fn read_le(ptr: &[u8], bytes: usize) -> u32 {
    let mut w = 0u32;
    for (i, &b) in ptr[..bytes].iter().enumerate() {
        w |= (b as u32) << (i * 8);
    }
    w
}

#[inline]
fn write_le(ptr: &mut [u8], bytes: usize, value: u32) {
    for (i, slot) in ptr[..bytes].iter_mut().enumerate() {
        *slot = (value >> (i * 8)) as u8;
    }
}

pub struct DeltaTransform {
    bytes: usize,
    offset: usize,
    pos: usize,
    prev: Vec<u32>,
}

impl DeltaTransform {
// `offset` is the number of samples in one period and must be a multiple of
// `bytes`.
    pub fn new(bytes: usize, offset: usize) -> Self {
        assert!(bytes >= 1 && bytes <= 4);
        assert!(offset > 0);
        Self {
            bytes,
            offset,
            pos: 0,
            prev: vec![0; offset],
        }
    }

    fn process(&mut self, out: &mut [u8], input: &[u8], encode: bool) -> (usize, usize) {
        let max_c = out.len().min(input.len());
        if max_c < self.bytes {
// not touch the prediction state.
            out[..max_c].copy_from_slice(&input[..max_c]);
            return (max_c, max_c);
        }
        let mut i = 0;
        while i <= max_c - self.bytes {
            let mut cur = read_le(&input[i..], self.bytes);
            let prev = self.prev[self.pos];
            if encode {
                self.prev[self.pos] = cur;
                cur = cur.wrapping_sub(prev);
            } else {
                cur = cur.wrapping_add(prev);
                self.prev[self.pos] = cur;
            }
            write_le(&mut out[i..], self.bytes, cur);
            self.pos += 1;
            if self.pos == self.offset {
                self.pos = 0;
            }
            i += self.bytes;
        }
        (i, i)
    }
}

impl Transform for DeltaTransform {
    fn forward(&mut self, out: &mut [u8], input: &[u8]) -> (usize, usize) {
        self.process(out, input, true)
    }

    fn reverse(&mut self, out: &mut [u8], input: &[u8]) -> (usize, usize) {
        self.process(out, input, false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::filter::Identity;

    #[test]
    fn delta_roundtrips() {
        let data: Vec<u8> = (0..10_000u32).map(|i| (i.wrapping_mul(2654435761) >> 13) as u8).collect();
        let mut enc = DeltaTransform::new(2, 2);
        let mut buf = vec![0u8; data.len()];
        let (o, i) = enc.forward(&mut buf, &data);
        assert_eq!((o, i), (data.len(), data.len()));
        let mut dec = DeltaTransform::new(2, 2);
        let mut back = vec![0u8; data.len()];
        let (o, i) = dec.reverse(&mut back, &buf);
        assert_eq!((o, i), (data.len(), data.len()));
        assert_eq!(back, data);
    }

    #[test]
    fn delta_short_input_is_copied() {
        let mut enc = DeltaTransform::new(4, 4);
        let mut buf = [0u8; 8];
        let (o, i) = enc.forward(&mut buf, &[1, 2]);
        assert_eq!((o, i), (2, 2));
        assert_eq!(&buf[..2], &[1, 2]);
        let _ = Identity;
    }
}
