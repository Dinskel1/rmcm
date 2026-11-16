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


use crate::filter::Transform;

pub const KB: usize = 1024;

const XOR_BYTE: u8 = 162;
const MARKER_BYTE: u8 = 99;

pub struct X86AdvancedTransform {
    offset: usize,
    last_offset: usize,
    #[allow(dead_code)]
    opt_var: usize,
    transform_count: usize,
    false_positives: usize,
}

impl Default for X86AdvancedTransform {
    fn default() -> Self {
        Self::new()
    }
}

impl X86AdvancedTransform {
    pub fn new() -> Self {
        Self {
            offset: 17,
            last_offset: 17,
            opt_var: 0,
            transform_count: 0,
            false_positives: 0,
        }
    }

    pub fn set_opt(&mut self, opt: usize) {
        self.opt_var = opt;
    }

    fn process(&mut self, out: &mut [u8], input: &[u8], encode: bool) -> (usize, usize) {
        let in_c = input.len();
        let out_c = out.len();
        let mut in_i = 0usize;
        let mut out_i = 0usize;
        if in_c <= 6 {
            debug_assert!(out_c >= in_c);
            out[..in_c].copy_from_slice(&input[..in_c]);
            in_i = in_c;
            out_i = in_c;
        } else {
            while in_i < in_c - 6 && out_i < out_c - 6 {
                out[out_i] = input[in_i];
                out_i += 1;
                let c = input[in_i];
                let is_e8e9 = (c & 0xFE) == 0xE8
                    || ((c & 0xF0) == 0x80 && in_i > 0 && input[in_i - 1] == 0x0F);
                if is_e8e9 {
                    let cur_offset = self.offset
                        + if encode {
                            in_i
                        } else {
                            out_i - 1
                        };
                    if encode {
                        let sign_byte = input[in_i + 4];
                        if sign_byte == 0xFF || sign_byte == 0x00 {
                            let delta = (input[in_i + 1] as u32)
                                | ((input[in_i + 2] as u32) << 8)
                                | ((input[in_i + 3] as u32) << 16)
                                | ((sign_byte as u32) << 24);
                            let delta_i = delta as i32;
// Don't change zero deltas.
                            if delta_i > 0
                                || (delta_i < 0 && (delta_i.wrapping_neg() as i64) < (cur_offset as i64))
                            {
                                if cur_offset.wrapping_sub(self.last_offset) > 3 * KB * 32 {
                                    self.offset = 0;
                                }
                                let addr = (delta as usize).wrapping_add(cur_offset) as u32;
                                out[out_i] = sign_byte;
                                out[out_i + 1] = (addr >> 16) as u8 ^ XOR_BYTE;
                                out[out_i + 2] = (addr >> 8) as u8 ^ XOR_BYTE;
                                out[out_i + 3] = addr as u8 ^ XOR_BYTE;
                                out[out_i + 4] = MARKER_BYTE;
                                out_i += 5;
                                in_i += 5;
                                self.transform_count += 1;
                                self.last_offset = cur_offset;
                                continue;
                            }
                        }
// Forbidden chars / no match.
                        if input[in_i + 1] == 0xFF
                            || input[in_i + 1] == 0x00
                            || input[in_i + 1] == 0xB2
                        {
                            self.false_positives += 1;
                            out[out_i] = 0xB2;
                            out[out_i + 1] = input[in_i + 1];
                            out_i += 2;
                            in_i += 2;
                            continue;
                        }
                    } else {
                        let sign_byte = input[in_i + 1];
                        if sign_byte == 0xFF || sign_byte == 0x00 {
                            if cur_offset.wrapping_sub(self.last_offset) > 3 * KB * 32 {
                                self.offset = 0;
                            }
                            let delta = ((input[in_i + 4] ^ XOR_BYTE) as u32)
                                | (((input[in_i + 3] ^ XOR_BYTE) as u32) << 8)
                                | (((input[in_i + 2] ^ XOR_BYTE) as u32) << 16);
                            let addr = delta.wrapping_sub(cur_offset as u32);
                            out[out_i] = addr as u8;
                            out[out_i + 1] = (addr >> 8) as u8;
                            out[out_i + 2] = (addr >> 16) as u8;
                            out[out_i + 3] = sign_byte;
                            out_i += 4;
                            self.last_offset = cur_offset;
                            in_i += 6;
                            continue;
                        } else if input[in_i + 1] == 0xB2 {
                            out[out_i] = input[in_i + 2];
                            out_i += 1;
                            in_i += 3;
                            continue;
                        }
                    }
                }
                in_i += 1;
            }
        }
        let in_count = in_i;
        let out_count = out_i;
        self.offset += if encode { in_count } else { out_count };
        (out_count, in_count)
    }

    pub fn counts(&self) -> (usize, usize) {
        (self.transform_count, self.false_positives)
    }
}

impl Transform for X86AdvancedTransform {
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
    use crate::filter::{ByteStreamFilter, FilterStream};
    use crate::stream::{ByteRead, ByteWrite, SliceReader, VecWriter};

    struct Mem<'a> {
        src: SliceReader<'a>,
        out: VecWriter,
    }

    impl<'a> Mem<'a> {
        fn new(src: &'a [u8]) -> Self {
            Self {
                src: SliceReader::new(src),
                out: VecWriter::new(),
            }
        }
    }

    impl ByteRead for Mem<'_> {
        fn get(&mut self) -> u8 {
            self.src.get()
        }
        fn read(&mut self, out: &mut [u8]) -> usize {
            self.src.read(out)
        }
    }

    impl ByteWrite for Mem<'_> {
        fn put(&mut self, b: u8) {
            self.out.put(b)
        }
        fn write(&mut self, buf: &[u8]) {
            self.out.write(buf)
        }
    }

    type Filter<'a> = ByteStreamFilter<Mem<'a>, X86AdvancedTransform, { 16 * KB }, { 16 * KB }>;

    fn encode(data: &[u8]) -> Vec<u8> {
        let mut f = Filter::new(Mem::new(data), X86AdvancedTransform::new());
        let mut chunk = [0u8; 4096];
        loop {
            let n = f.read(&mut chunk);
            if n == 0 {
                break;
            }
            f.stream_mut().out.write(&chunk[..n]);
        }
        f.into_stream().out.into_inner()
    }

    fn decode(data: &[u8]) -> Vec<u8> {
        let mut f = Filter::new(Mem::new(data), X86AdvancedTransform::new());
        let mut chunk = [0u8; 4096];
        loop {
            let n = f.stream_mut().src.read(&mut chunk);
            if n == 0 {
                break;
            }
            f.write(&chunk[..n]);
        }
        f.flush();
        f.into_stream().out.into_inner()
    }

    fn fake_code() -> Vec<u8> {
        let mut v = Vec::new();
        let mut state = 12345u32;
        for i in 0..4000u32 {
            state = state.wrapping_mul(1103515245).wrapping_add(12345);
            match (state >> 28) & 7 {
                0 => {
                    v.push(0xE8);
                    let rel = if i % 5 == 0 { 0 } else { (state % 4096) as i32 - 2048 };
                    v.extend_from_slice(&rel.to_le_bytes());
                }
                1 => {
                    v.push(0xE9);
                    v.extend_from_slice(&(-((state % 70000) as i32)).to_le_bytes());
                }
                2 => {
                    v.push(0x0F);
                    v.push(0x85);
                    v.extend_from_slice(&((state % 1000) as i32).to_le_bytes());
                }
                3 => {
                    v.push(0xE8);
                    v.push(0xB2);
                    v.push((state & 0xFF) as u8);
                }
                4 => {
                    v.push(0xE8);
                    v.push(0x00);
                    v.extend_from_slice(&[2, 0, 0]);
                }
                _ => {
                    v.push((state >> 8) as u8);
                    v.push((state >> 16) as u8);
                }
            }
        }
        v
    }

    #[test]
    fn x86_roundtrips() {
        let data = fake_code();
        let packed = encode(&data);
        let back = decode(&packed);
        assert!(back == data, "x86 roundtrip mismatch");
    }

    #[test]
    fn x86_handles_tiny_and_empty_inputs() {
        for data in [&b""[..], b"E8", b"E8 FF", b"hello, world"] {
            let packed = encode(data);
            let back = decode(&packed);
            assert!(back == data, "small input {data:?} did not roundtrip");
        }
    }

    #[test]
    fn x86_transforms_calls() {
// A positive relative call must be rewritten (grows by one marker byte).
        let data = [0xE8, 0x10, 0x00, 0x00, 0x00, 0x90, 0x90, 0x90, 0x90];
        let packed = encode(&data);
        assert!(packed.len() > data.len(), "call target was not transformed");
        assert_eq!(decode(&packed), data);
    }
}
