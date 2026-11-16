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

//! The `.mcm` container (`Archive.hpp` / `Archive.cpp`).

use crate::cm::{CmFast, CmHigh, CmMax, CmMid, CmTurbo};
use crate::detector::{Analyzer, Profile};
use crate::dict::{generate_code_words, CodeWordSet, DictFilter};
use crate::filter::{ByteStreamFilter, Transform};
use crate::format::{fmt_g, format_number};
use crate::stream::{leb128_decode, ByteRead, ByteWrite, SliceReader};
use crate::wav16::Wav16;
use crate::x86::X86AdvancedTransform;
use std::io::Write as _;

pub fn push_leb128(out: &mut Vec<u8>, mut value: u64) {
    loop {
        let mut b = (value & 0x7F) as u8;
        value >>= 7;
        if value != 0 {
            b |= 0x80;
        }
        out.push(b);
        if value == 0 {
            break;
        }
    }
}

pub const K_SIZE_PAD: usize = 10;
pub const MAGIC: &[u8; 10] = b"MCMARCHIVE";
pub const MAJOR_VERSION: u16 = 0;
pub const MINOR_VERSION: u16 = 83;

// `Compressor::Type`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CompressorType {
    Store = 0,
    Wav16 = 1,
    CmTurbo = 2,
    CmFast = 3,
    CmMid = 4,
    CmHigh = 5,
    CmMax = 6,
}

impl CompressorType {
    fn from_u8(v: u8) -> Self {
        match v {
            1 => CompressorType::Wav16,
            2 => CompressorType::CmTurbo,
            3 => CompressorType::CmFast,
            4 => CompressorType::CmMid,
            5 => CompressorType::CmHigh,
            6 => CompressorType::CmMax,
            _ => CompressorType::Store,
        }
    }
}

// `FilterType`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FilterType {
    None = 0,
    Dict = 1,
    X86 = 2,
    Auto = 3,
}

// `CompLevel`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CompLevel {
    Store,
    Turbo,
    Fast,
    Mid,
    High,
    Max,
}

// `LZPType`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LzpType {
    Auto,
    Enable,
    Disable,
}

// `CompressionOptions`.
#[derive(Clone, Copy, Debug)]
pub struct CompressionOptions {
    pub mem_usage: u32,
    pub comp_level: CompLevel,
    pub filter_type: FilterType,
    pub lzp_type: LzpType,
}

impl Default for CompressionOptions {
    fn default() -> Self {
        Self {
            mem_usage: 6,
            comp_level: CompLevel::Mid,
            filter_type: FilterType::Auto,
            lzp_type: LzpType::Auto,
        }
    }
}

// `Archive::Algorithm`, five bytes on disk.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Algorithm {
    pub mem_usage: u8,
    pub algorithm: CompressorType,
    pub lzp_enabled: bool,
    pub filter: FilterType,
    pub profile: Profile,
}

impl Algorithm {
    pub fn new(options: &CompressionOptions, profile: Profile) -> Self {
        let algorithm;
        if profile != Profile::Wave16 {
            algorithm = match options.comp_level {
                CompLevel::Store => CompressorType::Store,
                CompLevel::Turbo => CompressorType::CmTurbo,
                CompLevel::Fast => CompressorType::CmFast,
                CompLevel::Mid => CompressorType::CmMid,
                CompLevel::High => CompressorType::CmHigh,
                CompLevel::Max => CompressorType::CmMax,
            };
        } else {
            algorithm = CompressorType::Wav16;
        }
        let mut lzp_enabled = true;
        let mut filter = FilterType::None;
        match profile {
            Profile::Binary => {
                lzp_enabled = true;
                filter = FilterType::X86;
            }
            Profile::Text => {
                lzp_enabled = true;
                filter = FilterType::Dict;
            }
            _ => {}
        }
        if options.lzp_type == LzpType::Enable {
            lzp_enabled = true;
        } else if options.lzp_type == LzpType::Disable {
            lzp_enabled = false;
        }
        if options.filter_type != FilterType::Auto {
            filter = options.filter_type;
        }
        Self {
            mem_usage: options.mem_usage as u8,
            algorithm,
            lzp_enabled,
            filter,
            profile,
        }
    }

    pub fn write(&self, out: &mut Vec<u8>) {
        out.push(self.mem_usage);
        out.push(self.algorithm as u8);
        out.push(u8::from(self.lzp_enabled));
        out.push(self.filter as u8);
        out.push(self.profile as u8);
    }

    pub fn read(input: &[u8], pos: &mut usize) -> Self {
        let get = |p: &mut usize| {
            let v = input.get(*p).copied().unwrap_or(0);
            *p += 1;
            v
        };
        let mem_usage = get(pos);
        let algorithm = CompressorType::from_u8(get(pos));
        let lzp_enabled = get(pos) != 0;
        let filter = match get(pos) {
            1 => FilterType::Dict,
            2 => FilterType::X86,
            3 => FilterType::Auto,
            _ => FilterType::None,
        };
        let profile = Profile::from_u8(get(pos));
        Self {
            mem_usage,
            algorithm,
            lzp_enabled,
            filter,
            profile,
        }
    }
}

// `FileSegmentStream::SegmentRange`.
#[derive(Clone, Copy, Debug)]
pub struct SegmentRange {
    pub offset: u64,
    pub length: u64,
}

// `FileSegmentStream::FileSegments`.
#[derive(Clone, Debug, Default)]
pub struct FileSegments {
    pub base_offset: u64,
    pub total_size: u64,
    pub ranges: Vec<SegmentRange>,
}

impl FileSegments {
    pub fn calculate_total_size(&mut self) {
        self.total_size = self.ranges.iter().map(|r| r.length).sum();
    }

    pub fn write(&self, out: &mut Vec<u8>) {
        push_leb128(out, self.base_offset);
        push_leb128(out, self.ranges.len() as u64);
        let mut prev = 0u64;
        for r in &self.ranges {
            push_leb128(out, r.length);
        }
        for r in &self.ranges {
            push_leb128(out, r.offset.wrapping_sub(prev));
            prev = r.offset.wrapping_add(r.length);
        }
    }

    pub fn read(input: &[u8], pos: &mut usize) -> Self {
        let base_offset = leb128_vec(input, pos);
        let num_ranges = leb128_vec(input, pos) as usize;
        let mut ranges = vec![
            SegmentRange {
                offset: 0,
                length: 0
            };
            num_ranges.min(10_000_000)
        ];
        for r in ranges.iter_mut() {
            r.length = leb128_vec(input, pos);
        }
        let mut prev = 0u64;
        for r in ranges.iter_mut() {
            let delta = leb128_vec(input, pos);
            r.offset = prev.wrapping_add(delta);
            prev = r.offset.wrapping_add(r.length);
        }
        let mut seg = Self {
            base_offset,
            total_size: 0,
            ranges,
        };
        seg.calculate_total_size();
        seg
    }
}

fn leb128_vec(input: &[u8], pos: &mut usize) -> u64 {
    let mut r = SliceReader::new(&input[*pos..]);
    let v = leb128_decode(&mut r);
    *pos += r.position();
    v
}

// `Archive::SolidBlock`.
#[derive(Clone, Debug)]
pub struct SolidBlock {
    pub algorithm: Algorithm,
    pub segments: Vec<FileSegments>,
    pub total_size: u64,
}

pub struct SegmentReader<'a> {
    segments: &'a [FileSegments],
    input: &'a [u8],
    file_idx: usize,
    range_idx: usize,
    num_ranges: usize,
    cur_pos: u64,
    cur_end: u64,
    count: u64,
    total: u64,
    started: bool,
}

impl<'a> SegmentReader<'a> {
    pub fn new(segments: &'a [FileSegments], input: &'a [u8]) -> Self {
        Self {
            segments,
            input,
            file_idx: 0,
            range_idx: 0,
            num_ranges: 0,
            cur_pos: 0,
            cur_end: 0,
            count: 0,
            total: segments.iter().map(|s| s.total_size).sum(),
            started: false,
        }
    }

    fn next_range(&mut self) {
        while self.cur_pos >= self.cur_end {
            if self.range_idx < self.num_ranges {
                let seg = &self.segments[self.file_idx];
                let range = seg.ranges[self.range_idx];
                self.cur_pos = seg.base_offset + range.offset;
                self.cur_end = self.cur_pos + range.length;
                self.range_idx += 1;
            } else {
                if self.started {
                    self.file_idx += 1;
                }
                self.started = true;
                if self.file_idx >= self.segments.len() {
                    return;
                }
                let seg = &self.segments[self.file_idx];
                self.num_ranges = seg.ranges.len();
                self.range_idx = 0;
            }
        }
    }
}

impl ByteRead for SegmentReader<'_> {
    #[inline]
    fn get(&mut self) -> u8 {
        if self.cur_pos >= self.cur_end {
            self.next_range();
            if self.cur_pos >= self.cur_end {
                return 0;
            }
        }
        let pos = self.cur_pos as usize;
        if pos >= self.input.len() {
            return 0;
        }
        self.cur_pos += 1;
        self.count += 1;
        self.input[pos]
    }

    fn read(&mut self, out: &mut [u8]) -> usize {
        let mut written = 0usize;
        while written < out.len() {
            if self.cur_pos >= self.cur_end {
                self.next_range();
                if self.cur_pos >= self.cur_end {
                    break;
                }
            }
            let max_c = ((self.cur_end - self.cur_pos) as usize).min(out.len() - written);
            let start = self.cur_pos as usize;
            let avail = self.input.len().saturating_sub(start).min(max_c);
            out[written..written + avail].copy_from_slice(&self.input[start..start + avail]);
            self.cur_pos += avail as u64;
            written += avail;
            if avail < max_c {
                break;
            }
        }
        self.count += written as u64;
        written
    }

    fn eof(&mut self) -> bool {
        self.count >= self.total
    }
}

pub struct SegmentWriter<'a> {
    segments: &'a [FileSegments],
    output: &'a mut Vec<u8>,
    file_idx: usize,
    range_idx: usize,
    num_ranges: usize,
    cur_pos: u64,
    cur_end: u64,
    count: u64,
    started: bool,
    max_end: u64,
}

impl<'a> SegmentWriter<'a> {
    pub fn new(segments: &'a [FileSegments], output: &'a mut Vec<u8>) -> Self {
        Self {
            segments,
            output,
            file_idx: 0,
            range_idx: 0,
            num_ranges: 0,
            cur_pos: 0,
            cur_end: 0,
            count: 0,
            started: false,
            max_end: 0,
        }
    }

    fn next_range(&mut self) {
        while self.cur_pos >= self.cur_end {
            if self.range_idx < self.num_ranges {
                let seg = &self.segments[self.file_idx];
                let range = seg.ranges[self.range_idx];
                self.cur_pos = seg.base_offset + range.offset;
                self.cur_end = self.cur_pos + range.length;
                self.range_idx += 1;
            } else {
                if self.started {
                    self.file_idx += 1;
                }
                self.started = true;
                if self.file_idx >= self.segments.len() {
                    return;
                }
                let seg = &self.segments[self.file_idx];
                self.num_ranges = seg.ranges.len();
                self.range_idx = 0;
            }
        }
    }

    pub fn finish(self) -> u64 {
        self.max_end
    }
}

impl ByteWrite for SegmentWriter<'_> {
    #[inline]
    fn put(&mut self, b: u8) {
        if self.cur_pos >= self.cur_end {
            self.next_range();
            if self.cur_pos >= self.cur_end {
                return;
            }
        }
        let pos = self.cur_pos as usize;
        if self.output.len() <= pos {
            self.output.resize(pos + 1, 0);
        }
        self.output[pos] = b;
        self.cur_pos += 1;
        self.count += 1;
        self.max_end = self.max_end.max(self.cur_pos);
    }

    fn write(&mut self, buf: &[u8]) {
        let mut consumed = 0usize;
        while consumed < buf.len() {
            if self.cur_pos >= self.cur_end {
                self.next_range();
                if self.cur_pos >= self.cur_end {
                    break;
                }
            }
            let max_c = ((self.cur_end - self.cur_pos) as usize).min(buf.len() - consumed);
            let start = self.cur_pos as usize;
            if self.output.len() < start + max_c {
                self.output.resize(start + max_c, 0);
            }
            self.output[start..start + max_c].copy_from_slice(&buf[consumed..consumed + max_c]);
            self.cur_pos += max_c as u64;
            self.max_end = self.max_end.max(self.cur_pos);
            consumed += max_c;
        }
        self.count += consumed as u64;
    }

}

enum FilterKind {
    None,
    Dict(DictFilter),
    X86(X86AdvancedTransform),
}

impl FilterKind {
    fn from(algorithm: &Algorithm, analyzer: Option<&Analyzer>) -> Self {
        match algorithm.filter {
            FilterType::Dict => {
                if let Some(a) = analyzer {
                    let mut builder = a.dict_builder().clone();
                    let mut set = CodeWordSet::default();
                    generate_code_words(&mut builder, &mut set);
                    FilterKind::Dict(DictFilter::new_encoder(&set, 0x3, 0x4))
                } else {
                    FilterKind::Dict(DictFilter::new_decoder())
                }
            }
            FilterType::X86 => FilterKind::X86(X86AdvancedTransform::new()),
            _ => FilterKind::None,
        }
    }
}

impl Transform for FilterKind {
    fn forward(&mut self, out: &mut [u8], input: &[u8]) -> (usize, usize) {
        match self {
            FilterKind::None => {
                let n = out.len().min(input.len());
                out[..n].copy_from_slice(&input[..n]);
                (n, n)
            }
            FilterKind::Dict(f) => f.forward(out, input),
            FilterKind::X86(f) => f.forward(out, input),
        }
    }

    fn reverse(&mut self, out: &mut [u8], input: &[u8]) -> (usize, usize) {
        match self {
            FilterKind::None => {
                let n = out.len().min(input.len());
                out[..n].copy_from_slice(&input[..n]);
                (n, n)
            }
            FilterKind::Dict(f) => f.reverse(out, input),
            FilterKind::X86(f) => f.reverse(out, input),
        }
    }
}

#[derive(Default)]
pub struct ProgressCounters {
    in_bytes: std::sync::atomic::AtomicU64,
    out_bytes: std::sync::atomic::AtomicU64,
    printed: std::sync::atomic::AtomicBool,
}

impl ProgressCounters {
    fn add_in(&self, n: u64) {
        self.in_bytes
            .fetch_add(n, std::sync::atomic::Ordering::Relaxed);
    }

    fn add_out(&self, n: u64) {
        self.out_bytes
            .fetch_add(n, std::sync::atomic::Ordering::Relaxed);
    }
}

// `ProgressMeter::printRatio`.
fn print_ratio(in_size: u64, comp_size: u64, encode: bool, start: std::time::Instant) {
    const KB: u64 = 1024;
    if in_size == 0 && comp_size == 0 {
        return;
    }
    let seconds = start.elapsed().as_secs_f64();
    let seconds = if seconds > 0.0 { seconds } else { 1.0 };
    let rate = ((in_size / KB) as f64 / seconds) as u32;
    let mut line = format!("{}KB ", in_size / KB);
    if comp_size > KB {
        line.push_str(&format!(
            "{} {}KB ",
            if encode { "->" } else { "<-" },
            comp_size / KB
        ));
    } else {
        line.push_str(", ");
    }
    line.push_str(&format!("{}KB/s", rate));
    if comp_size > KB {
        line.push_str(&format!(
            " ratio: {:.5}",
            comp_size as f64 / in_size.max(1) as f64
        ));
    }
    let mut stdout = std::io::stdout();
    let _ = write!(stdout, "{}\t\r", line);
    let _ = stdout.flush();
}

pub struct Progress {
    counters: std::sync::Arc<ProgressCounters>,
    stop: Option<std::sync::mpsc::Sender<()>>,
    handle: Option<std::thread::JoinHandle<()>>,
}

impl Progress {
    pub fn start(encode: bool) -> Self {
        use std::sync::mpsc::{channel, RecvTimeoutError};
        let counters = std::sync::Arc::new(ProgressCounters::default());
        let thread_counters = counters.clone();
        let (stop, wait) = channel::<()>();
        let handle = std::thread::spawn(move || {
            let start = std::time::Instant::now();
            loop {
                match wait.recv_timeout(std::time::Duration::from_millis(250)) {
                    Ok(()) | Err(RecvTimeoutError::Disconnected) => return,
                    Err(RecvTimeoutError::Timeout) => {
                        print_ratio(
                            thread_counters
                                .in_bytes
                                .load(std::sync::atomic::Ordering::Relaxed),
                            thread_counters
                                .out_bytes
                                .load(std::sync::atomic::Ordering::Relaxed),
                            encode,
                            start,
                        );
                        thread_counters
                            .printed
                            .store(true, std::sync::atomic::Ordering::Relaxed);
                    }
                }
            }
        });
        Self {
            counters,
            stop: Some(stop),
            handle: Some(handle),
        }
    }

    pub fn counters(&self) -> std::sync::Arc<ProgressCounters> {
        self.counters.clone()
    }

// which case the caller must emit the newline that terminates it.
    pub fn finish(mut self) -> bool {
        self.stop.take();
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
        self.counters
            .printed
            .load(std::sync::atomic::Ordering::Relaxed)
    }
}

pub struct MeterReader<'a, R: ByteRead + ?Sized> {
    inner: &'a mut R,
    counters: std::sync::Arc<ProgressCounters>,
    count_out: bool,
    pending: u64,
}

impl<'a, R: ByteRead + ?Sized> MeterReader<'a, R> {
    pub fn new(inner: &'a mut R, counters: std::sync::Arc<ProgressCounters>, count_out: bool) -> Self {
        Self {
            inner,
            counters,
            count_out,
            pending: 0,
        }
    }

    fn flush(&mut self) {
        if self.pending != 0 {
            if self.count_out {
                self.counters.add_out(self.pending);
            } else {
                self.counters.add_in(self.pending);
            }
            self.pending = 0;
        }
    }
}

impl<R: ByteRead + ?Sized> ByteRead for MeterReader<'_, R> {
    fn get(&mut self) -> u8 {
        let b = self.inner.get();
        self.pending += 1;
        if self.pending >= 4096 {
            self.flush();
        }
        b
    }

    fn read(&mut self, out: &mut [u8]) -> usize {
        let n = self.inner.read(out);
        self.pending += n as u64;
        self.flush();
        n
    }

    fn eof(&mut self) -> bool {
        self.inner.eof()
    }

    fn position(&self) -> usize {
        self.inner.position()
    }
}

pub struct MeterWriter<'a, W: ByteWrite + ?Sized> {
    inner: &'a mut W,
    counters: std::sync::Arc<ProgressCounters>,
    count_in: bool,
    pending: u64,
}

impl<'a, W: ByteWrite + ?Sized> MeterWriter<'a, W> {
    pub fn new(inner: &'a mut W, counters: std::sync::Arc<ProgressCounters>, count_in: bool) -> Self {
        Self {
            inner,
            counters,
            count_in,
            pending: 0,
        }
    }

    fn flush(&mut self) {
        if self.pending != 0 {
            if self.count_in {
                self.counters.add_in(self.pending);
            } else {
                self.counters.add_out(self.pending);
            }
            self.pending = 0;
        }
    }
}

impl<W: ByteWrite + ?Sized> ByteWrite for MeterWriter<'_, W> {
    fn put(&mut self, b: u8) {
        self.inner.put(b);
        self.pending += 1;
        if self.pending >= 4096 {
            self.flush();
        }
    }

    fn write(&mut self, buf: &[u8]) {
        self.inner.write(buf);
        self.pending += buf.len() as u64;
        self.flush();
    }
}

fn read_all<R: ByteRead + ?Sized>(input: &mut R, max_count: u64) -> Vec<u8> {
    let mut out = Vec::new();
    let mut buf = [0u8; 8192];
    while (out.len() as u64) < max_count {
        let want = (buf.len() as u64).min(max_count - out.len() as u64) as usize;
        let n = input.read(&mut buf[..want]);
        if n == 0 {
            break;
        }
        out.extend_from_slice(&buf[..n]);
    }
    out
}

// Unifies the backends the container can name.
pub enum Compressor {
    Store,
    Wav(Wav16),
    Turbo(Box<CmTurbo>),
    Fast(Box<CmFast>),
    Mid(Box<CmMid>),
    High(Box<CmHigh>),
    Max(Box<CmMax>),
}

impl Compressor {
    pub fn create(algorithm: &Algorithm) -> Self {
        let m = algorithm.mem_usage as u32;
        let lzp = algorithm.lzp_enabled;
        let profile = Some(match algorithm.profile {
            Profile::Text => crate::cm::Profile::Text,
            Profile::Binary => crate::cm::Profile::Binary,
            _ => crate::cm::Profile::Wave16,
        });
        match algorithm.algorithm {
            CompressorType::Wav16 => Compressor::Wav(Wav16::new()),
            CompressorType::Store => Compressor::Store,
            CompressorType::CmTurbo => Compressor::Turbo(Box::new(CmTurbo::new(m, lzp, profile))),
            CompressorType::CmFast => Compressor::Fast(Box::new(CmFast::new(m, lzp, profile))),
            CompressorType::CmMid => Compressor::Mid(Box::new(CmMid::new(m, lzp, profile))),
            CompressorType::CmHigh => Compressor::High(Box::new(CmHigh::new(m, lzp, profile))),
            CompressorType::CmMax => Compressor::Max(Box::new(CmMax::new(m, lzp, profile))),
        }
    }

    pub fn compress<R: ByteRead + ?Sized, W: ByteWrite>(
        &mut self,
        input: &mut R,
        out: &mut W,
        max_count: u64,
    ) -> usize {
        match self {
            Compressor::Store => {
                let mut buf = [0u8; 8192];
                let mut total = 0u64;
                while total < max_count {
                    let want = (buf.len() as u64).min(max_count - total) as usize;
                    let n = input.read(&mut buf[..want]);
                    if n == 0 {
                        break;
                    }
                    out.write(&buf[..n]);
                    total += n as u64;
                }
                total as usize
            }
            Compressor::Wav(w) => {
                let data = read_all(input, max_count);
                w.compress(&data, out, max_count)
            }
            Compressor::Turbo(c) => {
                c.compress(input, out, max_count);
                input.position()
            }
            Compressor::Fast(c) => {
                c.compress(input, out, max_count);
                input.position()
            }
            Compressor::Mid(c) => {
                c.compress(input, out, max_count);
                input.position()
            }
            Compressor::High(c) => {
                c.compress(input, out, max_count);
                input.position()
            }
            Compressor::Max(c) => {
                c.compress(input, out, max_count);
                input.position()
            }
        }
    }

    pub fn decompress<R: ByteRead + ?Sized, W: ByteWrite>(
        &mut self,
        input: &mut R,
        out: &mut W,
        max_count: u64,
    ) -> usize {
        match self {
            Compressor::Store => {
                let mut buf = [0u8; 8192];
                let mut total = 0u64;
                while total < max_count {
                    let want = (buf.len() as u64).min(max_count - total) as usize;
                    let n = input.read(&mut buf[..want]);
                    if n == 0 {
                        break;
                    }
                    out.write(&buf[..n]);
                    total += n as u64;
                }
                total as usize
            }
            Compressor::Wav(w) => {
                let data = read_all(input, max_count);
                w.decompress(&data, out, max_count)
            }
            Compressor::Turbo(c) => {
                c.decompress(input, out, max_count);
                input.position()
            }
            Compressor::Fast(c) => {
                c.decompress(input, out, max_count);
                input.position()
            }
            Compressor::Mid(c) => {
                c.decompress(input, out, max_count);
                input.position()
            }
            Compressor::High(c) => {
                c.decompress(input, out, max_count);
                input.position()
            }
            Compressor::Max(c) => {
                c.decompress(input, out, max_count);
                input.position()
            }
        }
    }
}

pub struct Store;

pub struct Archive {
    pub options: CompressionOptions,
}

impl Archive {
    pub fn new(options: CompressionOptions) -> Self {
        Self { options }
    }

    fn construct_blocks(&self, analyzer: &mut Analyzer) -> Vec<SolidBlock> {
        let mut blocks = Vec::new();
        for p in 0..4u8 {
            let profile = Profile::from_u8(p);
            if profile == Profile::Eof {
                continue;
            }
            let mut pos = 0u64;
            let mut ranges = Vec::new();
            for b in analyzer.blocks() {
                let len = b.length();
                if b.profile() == profile {
                    ranges.push(SegmentRange { offset: pos, length: len });
                }
                pos += len;
            }
            let mut seg = FileSegments {
                base_offset: 0,
                total_size: 0,
                ranges,
            };
            seg.calculate_total_size();
            if seg.total_size > 0 {
                let total_size = seg.total_size;
                blocks.push(SolidBlock {
                    algorithm: Algorithm::new(&self.options, profile),
                    segments: vec![seg],
                    total_size,
                });
            }
        }
// Descending by size.
        blocks.sort_by(|a, b| b.total_size.cmp(&a.total_size));
        blocks
    }

// Compress a whole input buffer.
    pub fn compress(&self, input: &[u8]) -> Vec<u8> {
        let mut analyzer = Analyzer::new();
        let mut reader = SliceReader::new(input);
        let analyze_start = std::time::Instant::now();
        println!("Analyzing");
        let analyze_progress = Progress::start(true);
        {
            let mut metered = MeterReader::new(&mut reader, analyze_progress.counters(), false);
            analyzer.analyze(&mut metered);
        }
        if analyze_progress.finish() {
            println!();
        }
        analyzer.dump();
        println!(
            "Analyzing took {}s",
            fmt_g(analyze_start.elapsed().as_secs_f64(), 6)
        );
        println!();
        let blocks = self.construct_blocks(&mut analyzer);

        let mut out = Vec::new();
        out.extend_from_slice(MAGIC);
        out.extend_from_slice(&MAJOR_VERSION.to_be_bytes());
        out.extend_from_slice(&MINOR_VERSION.to_be_bytes());

// Metadata.
        let mut meta = Vec::new();
        push_leb128(&mut meta, blocks.len() as u64);
        for b in &blocks {
            b.algorithm.write(&mut meta);
            push_leb128(&mut meta, b.segments.len() as u64);
            for seg in &b.segments {
                seg.write(&mut meta);
            }
        }
        let mut meta_comp = Compressor::Fast(Box::new(CmFast::new(6, true, Some(crate::cm::Profile::Text))));
        let mut meta_compressed = Vec::new();
        let mut meta_src = SliceReader::new(&meta);
        meta_comp.compress(&mut meta_src, &mut meta_compressed, meta.len() as u64);
        let meta_start = out.len();
        push_leb128(&mut out, meta.len() as u64);
        out.extend_from_slice(&meta_compressed);
        push_leb128(&mut out, 1234);
        println!(
            "Compressed metadata {} -> {}",
            meta.len(),
            out.len() - meta_start
        );
        println!();

// Payload.
        for block in &blocks {
            let start = std::time::Instant::now();
            let out_start = out.len();
            out.extend_from_slice(&[0u8; K_SIZE_PAD]);
            println!(
                "Compressing {} stream size={}\t",
                block.algorithm.profile.name(),
                format_number(block.total_size)
            );
            let filter = FilterKind::from(&block.algorithm, Some(&analyzer));
            let reader = SegmentReader::new(&block.segments, input);
            let progress = Progress::start(true);
            let counters = progress.counters();
            let mut filtered = Vec::with_capacity(block.total_size as usize);
            match filter {
                FilterKind::None => {
                    let mut r = reader;
                    let mut buf = [0u8; 4096];
                    loop {
                        let n = r.read(&mut buf);
                        if n == 0 {
                            break;
                        }
                        filtered.extend_from_slice(&buf[..n]);
                    }
                }
                f => {
                    let mut stream = ByteStreamFilter::<_, _, 16384, 16384>::new(reader, f);
                    let mut buf = [0u8; 4096];
                    loop {
                        let n = stream.read(&mut buf);
                        if n == 0 {
                            break;
                        }
                        filtered.extend_from_slice(&buf[..n]);
                    }
                }
            }
            let filter_size = filtered.len() as u64;
            let mut comp = Compressor::create(&block.algorithm);
            {
                let mut src = SliceReader::new(&filtered);
                let mut metered_out = MeterWriter::new(&mut out, counters, false);
                let mut metered_in = MeterReader::new(&mut src, progress.counters(), false);
                comp.compress(&mut metered_in, &mut metered_out, filter_size);
            }
            if progress.finish() {
                println!();
            }
            let after = out.len();
            let mut lenbuf = Vec::new();
            push_leb128(&mut lenbuf, filter_size);
            out[out_start..out_start + lenbuf.len()].copy_from_slice(&lenbuf);
            debug_assert!(after > out_start + K_SIZE_PAD);
            println!(
                "Compressed {} -> {} in {}s",
                format_number(block.total_size),
                format_number((after - out_start) as u64),
                fmt_g(start.elapsed().as_secs_f64(), 6)
            );
            println!();
        }
        out
    }

// `Header::isArchive`.
    pub fn is_archive(&self, input: &[u8]) -> bool {
        input.len() >= 10 && &input[0..10] == MAGIC
    }

// `Header::majorVersion`.
    pub fn major_version(&self, input: &[u8]) -> u16 {
        u16::from_be_bytes([input[10], input[11]])
    }

// `Header::minorVersion`.
    pub fn minor_version(&self, input: &[u8]) -> u16 {
        u16::from_be_bytes([input[12], input[13]])
    }

// `Header::isSameVersion`.
    pub fn is_same_version(&self, input: &[u8]) -> bool {
        input.len() >= 14
            && self.major_version(input) == MAJOR_VERSION
            && self.minor_version(input) == MINOR_VERSION
    }

    pub fn decompress(&self, input: &[u8]) -> Vec<u8> {
        assert_eq!(&input[0..10], MAGIC, "not an MCM archive");
        let major = u16::from_be_bytes([input[10], input[11]]);
        let minor = u16::from_be_bytes([input[12], input[13]]);
        assert_eq!((major, minor), (MAJOR_VERSION, MINOR_VERSION), "version mismatch");
        let mut pos = 14usize;

        let metadata_size = leb128_vec(input, &mut pos);
        println!("Metadata size={metadata_size}");
        let mut meta_comp = Compressor::Fast(Box::new(CmFast::new(6, true, Some(crate::cm::Profile::Text))));
        let mut meta = Vec::new();
        let mut meta_src = SliceReader::new(&input[pos..]);
        let consumed = meta_comp.decompress(&mut meta_src, &mut meta, metadata_size);
        pos += consumed;
        assert_eq!(leb128_vec(input, &mut pos), 1234, "metadata sentinel mismatch");

        let mut mpos = 0usize;
        let num_blocks = leb128_vec(&meta, &mut mpos) as usize;
        let mut blocks = Vec::with_capacity(num_blocks);
        for _ in 0..num_blocks {
            let algorithm = Algorithm::read(&meta, &mut mpos);
            let num_segments = leb128_vec(&meta, &mut mpos) as usize;
            let mut segments = Vec::with_capacity(num_segments);
            for _ in 0..num_segments {
                let seg = FileSegments::read(&meta, &mut mpos);
                println!("{} size {}", algorithm.profile.name(), seg.total_size);
                segments.push(seg);
            }
            let total_size = segments.iter().map(|s| s.total_size).sum();
            blocks.push(SolidBlock {
                algorithm,
                segments,
                total_size,
            });
        }

        let mut out = Vec::new();
        for block in &blocks {
            let start = std::time::Instant::now();
            let out_start = pos;
            let block_size = leb128_vec(input, &mut pos);
            println!(
                "Decompressing {} stream size={}\t",
                block.algorithm.profile.name(),
                format_number(block.total_size)
            );
            while pos < out_start + K_SIZE_PAD {
                pos += 1;
            }
            let mut comp = Compressor::create(&block.algorithm);
            let progress = Progress::start(false);
            let counters = progress.counters();
            let consumed = match FilterKind::from(&block.algorithm, None) {
                FilterKind::None => {
                    let mut w = SegmentWriter::new(&block.segments, &mut out);
                    let mut w = MeterWriter::new(&mut w, counters.clone(), true);
                    let mut src = SliceReader::new(&input[pos..]);
                    let mut src = MeterReader::new(&mut src, counters.clone(), true);
                    comp.decompress(&mut src, &mut w, block_size)
                }
                f => {
                    let mut w = SegmentWriter::new(&block.segments, &mut out);
                    let w = MeterWriter::new(&mut w, counters.clone(), true);
                    let mut stream = ByteStreamFilter::<_, _, 16384, 16384>::new(w, f);
                    let mut src = SliceReader::new(&input[pos..]);
                    let mut src = MeterReader::new(&mut src, counters.clone(), true);
                    let consumed = comp.decompress(&mut src, &mut stream, block_size);
                    stream.flush_output();
                    consumed
                }
            };
            pos += consumed;
            if progress.finish() {
                println!();
            }
            let written: u64 = block.segments.iter().map(|s| s.total_size).sum();
            println!(
                "Decompressed {} <- {} in {}s",
                format_number(written),
                format_number((pos - out_start) as u64),
                fmt_g(start.elapsed().as_secs_f64(), 6)
            );
            println!();
        }
        out
    }
}
