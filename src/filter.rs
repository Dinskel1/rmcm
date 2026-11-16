/*  rmcm - a Rust port of the MCM file compressor

    Copyright (C) 2015, Google Inc.
    Authors: Mathieu Chartier
    Rust port: Dinskel1 (github.com/Dinskel1)

    LICENSE

    This file is part of the rmcm file compressor.

    MCM is free software: you can redistribute it and/or modify
    it under the terms of the GNU General Public License as published by
    the Free Software Foundation, either version 3 of the License, or
    (at your option) any later version.

    MCM is distributed in the hope that it will be useful,
    but WITHOUT ANY WARRANTY; without even the implied warranty of
    MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
    GNU General Public License for more details.

    You should have received a copy of the GNU General Public License
    along with MCM.  If not, see <http://www.gnu.org/licenses/>.
*/

//! Byte-stream filters (`Filter.hpp`).

use crate::stream::{ByteRead, ByteWrite};

pub const KB: usize = 1024;

// `(bytes_written, bytes_consumed)`.
pub trait Transform {
    fn forward(&mut self, out: &mut [u8], input: &[u8]) -> (usize, usize);
    fn reverse(&mut self, out: &mut [u8], input: &[u8]) -> (usize, usize);
}

pub trait FilterStream: ByteRead + ByteWrite {
    fn flush(&mut self);
}

// much easier to reason about.
pub struct ByteStreamFilter<S, T: Transform, const IN: usize = { 16 * KB }, const OUT: usize = { 16 * KB }> {
    pub stream: S,
    pub transform: T,
    in_buf: Vec<u8>,
    in_size: usize,
    out_buf: Vec<u8>,
    out_pos: usize,
    out_size: usize,
// Write direction.
    w_buf: Vec<u8>,
    w_len: usize,
    count: u64,
}

impl<S, T: Transform, const IN: usize, const OUT: usize> ByteStreamFilter<S, T, IN, OUT> {
    pub fn new(stream: S, transform: T) -> Self {
        Self {
            stream,
            transform,
            in_buf: vec![0; IN],
            in_size: 0,
            out_buf: vec![0; OUT],
            out_pos: 0,
            out_size: 0,
            w_buf: vec![0; IN],
            w_len: 0,
            count: 0,
        }
    }

    pub fn count(&self) -> u64 {
        self.count
    }

    pub fn stream(&self) -> &S {
        &self.stream
    }

    pub fn stream_mut(&mut self) -> &mut S {
        &mut self.stream
    }

    pub fn into_stream(self) -> S {
        self.stream
    }

}

impl<S: ByteRead, T: Transform, const IN: usize, const OUT: usize> ByteStreamFilter<S, T, IN, OUT> {
    fn refill_read(&mut self) {
        if self.in_size < IN {
            let n = self.stream.read(&mut self.in_buf[self.in_size..]);
            self.in_size += n;
        }
    }

    fn refill_read_and_process(&mut self) -> usize {
        self.refill_read();
        self.out_pos = 0;
        self.out_size = 0;
        let (out_count, in_count) = {
            let mut out = std::mem::take(&mut self.out_buf);
            let (o, i) = self.transform.forward(&mut out, &self.in_buf[..self.in_size]);
            self.out_buf = out;
            (o, i)
        };
        debug_assert!(in_count <= self.in_size);
        self.out_size = out_count;
        self.count += out_count as u64;
        self.in_buf.copy_within(in_count..self.in_size, 0);
        self.in_size -= in_count;
        self.out_size
    }

}

impl<S: ByteWrite, T: Transform, const IN: usize, const OUT: usize> ByteStreamFilter<S, T, IN, OUT> {
    pub fn flush_output(&mut self) {
        while self.w_len > 0 {
            self.refill_write_and_process();
        }
    }

    fn refill_write_and_process(&mut self) -> usize {
        let (out_count, in_count) = {
            let mut out = std::mem::take(&mut self.out_buf);
            let (o, i) = self.transform.reverse(&mut out, &self.w_buf[..self.w_len]);
            self.out_buf = out;
            (o, i)
        };
        debug_assert!(in_count <= self.w_len);
        self.stream.write(&self.out_buf[..out_count]);
        self.w_buf.copy_within(in_count..self.w_len, 0);
        self.w_len -= in_count;
        let room = IN - self.w_len;
        room
    }
}

impl<S: ByteRead, T: Transform, const IN: usize, const OUT: usize> ByteRead
    for ByteStreamFilter<S, T, IN, OUT>
{
    fn get(&mut self) -> u8 {
        if self.out_pos >= self.out_size && self.refill_read_and_process() == 0 {
// reachable on a truncated file.
            return 0;
        }
        let c = self.out_buf[self.out_pos];
        self.out_pos += 1;
        c
    }

    fn read(&mut self, buf: &mut [u8]) -> usize {
        let mut done = 0;
        while done < buf.len() {
            if self.out_pos >= self.out_size && self.refill_read_and_process() == 0 {
                break;
            }
            let n = (self.out_size - self.out_pos).min(buf.len() - done);
            buf[done..done + n].copy_from_slice(&self.out_buf[self.out_pos..self.out_pos + n]);
            self.out_pos += n;
            done += n;
        }
        done
    }
}

impl<S: ByteWrite, T: Transform, const IN: usize, const OUT: usize> ByteWrite
    for ByteStreamFilter<S, T, IN, OUT>
{
    fn put(&mut self, c: u8) {
        if self.w_len == IN {
            self.refill_write_and_process();
        }
        self.w_buf[self.w_len] = c;
        self.w_len += 1;
    }

    fn write(&mut self, buf: &[u8]) {
        let mut done = 0;
        while done < buf.len() {
            if self.w_len == IN {
                self.refill_write_and_process();
            }
            let n = (IN - self.w_len).min(buf.len() - done);
            self.w_buf[self.w_len..self.w_len + n].copy_from_slice(&buf[done..done + n]);
            self.w_len += n;
            done += n;
        }
    }
}

impl<S: ByteRead + ByteWrite, T: Transform, const IN: usize, const OUT: usize> FilterStream
    for ByteStreamFilter<S, T, IN, OUT>
{
    fn flush(&mut self) {
        while self.w_len != 0 {
            self.refill_write_and_process();
        }
    }
}

pub struct Identity;

impl Transform for Identity {
    fn forward(&mut self, out: &mut [u8], input: &[u8]) -> (usize, usize) {
        let n = out.len().min(input.len());
        out[..n].copy_from_slice(&input[..n]);
        (n, n)
    }

    fn reverse(&mut self, out: &mut [u8], input: &[u8]) -> (usize, usize) {
        let n = out.len().min(input.len());
        out[..n].copy_from_slice(&input[..n]);
        (n, n)
    }
}

pub struct ByteBufferFilter<S: ByteRead + ByteWrite, B: BlockTransform, const BLOCK: usize = { 64 * KB }> {
    pub stream: S,
    pub transform: B,
    block: Vec<u8>,
    block_pos: usize,
    block_size: usize,
}

pub trait BlockTransform {
    fn forward(&mut self, buf: &mut [u8]);
    fn reverse(&mut self, buf: &mut [u8]);
}

impl<S: ByteRead + ByteWrite, B: BlockTransform, const BLOCK: usize> ByteBufferFilter<S, B, BLOCK> {
    pub fn new(stream: S, transform: B) -> Self {
        Self {
            stream,
            transform,
            block: vec![0; BLOCK],
            block_pos: 0,
            block_size: 0,
        }
    }

    fn refill_read(&mut self) -> usize {
        self.block_pos = 0;
        self.block_size = self.stream.read(&mut self.block);
        let mut block = std::mem::take(&mut self.block);
        self.transform.forward(&mut block[..self.block_size]);
        self.block = block;
        self.block_size
    }

    fn flush_write(&mut self) -> usize {
        let mut block = std::mem::take(&mut self.block);
        self.transform.reverse(&mut block[..self.block_pos]);
        self.block = block;
        self.stream.write(&self.block[..self.block_pos]);
        self.block_pos = 0;
        BLOCK
    }
}

impl<S: ByteRead + ByteWrite, B: BlockTransform, const BLOCK: usize> ByteRead
    for ByteBufferFilter<S, B, BLOCK>
{
    fn get(&mut self) -> u8 {
        if self.block_pos >= self.block_size && self.refill_read() == 0 {
            return 0;
        }
        let c = self.block[self.block_pos];
        self.block_pos += 1;
        c
    }

    fn read(&mut self, buf: &mut [u8]) -> usize {
        let mut done = 0;
        while done < buf.len() {
            if self.block_pos >= self.block_size && self.refill_read() == 0 {
                break;
            }
            let n = (self.block_size - self.block_pos).min(buf.len() - done);
            buf[done..done + n].copy_from_slice(&self.block[self.block_pos..self.block_pos + n]);
            self.block_pos += n;
            done += n;
        }
        done
    }
}

impl<S: ByteRead + ByteWrite, B: BlockTransform, const BLOCK: usize> ByteWrite
    for ByteBufferFilter<S, B, BLOCK>
{
    fn put(&mut self, c: u8) {
        if self.block_pos >= self.block_size {
            self.flush_write();
        }
        self.block[self.block_pos] = c;
        self.block_pos += 1;
    }

    fn write(&mut self, buf: &[u8]) {
        let mut done = 0;
        while done < buf.len() {
            if self.block_pos >= self.block_size {
                self.flush_write();
            }
            let n = (self.block_size - self.block_pos).min(buf.len() - done);
            self.block[self.block_pos..self.block_pos + n].copy_from_slice(&buf[done..done + n]);
            self.block_pos += n;
            done += n;
        }
    }
}

impl<S: ByteRead + ByteWrite, B: BlockTransform, const BLOCK: usize> FilterStream
    for ByteBufferFilter<S, B, BLOCK>
{
    fn flush(&mut self) {
        if self.block_pos != 0 {
            self.flush_write();
        }
    }
}

impl BlockTransform for Identity {
    fn forward(&mut self, _buf: &mut [u8]) {}
    fn reverse(&mut self, _buf: &mut [u8]) {}
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stream::SliceReader;

    struct RW {
        input: SliceReader<'static>,
        output: Vec<u8>,
    }

    impl ByteRead for RW {
        fn get(&mut self) -> u8 {
            self.input.get()
        }
        fn read(&mut self, out: &mut [u8]) -> usize {
            self.input.read(out)
        }
    }

    impl ByteWrite for RW {
        fn put(&mut self, b: u8) {
            self.output.push(b)
        }
        fn write(&mut self, buf: &[u8]) {
            self.output.extend_from_slice(buf)
        }
    }

    #[test]
    fn identity_stream_filter_roundtrips() {
        let data: Vec<u8> = (0..40_000u32).map(|i| (i * 7) as u8).collect();
        let leaked: &'static [u8] = Box::leak(data.clone().into_boxed_slice());
        let mut f = ByteStreamFilter::<_, _, 4096, 4096>::new(
            RW {
                input: SliceReader::new(leaked),
                output: Vec::new(),
            },
            Identity,
        );
        let mut out = vec![0u8; data.len()];
        let n = f.read(&mut out);
        assert_eq!(n, data.len());
        assert_eq!(out, data);
        assert_eq!(f.count(), data.len() as u64);
    }
}
