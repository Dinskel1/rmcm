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

//! Dictionary construction and the dictionary filter (`Dict.hpp`).

use std::collections::HashMap;
use std::hash::{BuildHasherDefault, Hasher};

use std::time::Instant;

use crate::format::{fmt_g, pretty_size};

// inside the word).
#[inline]
pub fn is_word_char(c: u8) -> bool {
    c.is_ascii_uppercase() || c.is_ascii_lowercase() || c >= 128
}

pub const MAX_WORD_LEN: usize = 256;
pub const INVALID_CHAR: u16 = 256;

// `(count, word)` — the reference's `WCPair`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WcPair {
    pub count: u32,
    pub word: Vec<u8>,
}

pub fn compare_wc_pair(extra_cost: usize) -> impl Fn(&WcPair, &WcPair) -> std::cmp::Ordering {
    move |a: &WcPair, b: &WcPair| {
        let sa = (a.count as i64 - 1) * (a.word.len().max(extra_cost) as i64 - extra_cost as i64);
        let sb = (b.count as i64 - 1) * (b.word.len().max(extra_cost) as i64 - extra_cost as i64);
        sa.cmp(&sb)
    }
}

// differ from each other for no benefit.
#[derive(Default, Clone)]
pub struct FnvHasher(u64);

impl Hasher for FnvHasher {
    fn finish(&self) -> u64 {
        self.0
    }

    fn write(&mut self, bytes: &[u8]) {
        let mut h = if self.0 == 0 { 0xcbf29ce484222325 } else { self.0 };
        for &b in bytes {
            h ^= b as u64;
            h = h.wrapping_mul(0x100000001b3);
        }
        self.0 = h;
    }
}

#[derive(Clone)]
pub struct WordCollectionMap {
    words: HashMap<Vec<u8>, u32, BuildHasherDefault<FnvHasher>>,
    max_words: usize,
}

impl Default for WordCollectionMap {
    fn default() -> Self {
        Self::new(10_000_000)
    }
}

impl WordCollectionMap {
    pub fn new(max_words: usize) -> Self {
        Self {
            words: HashMap::default(),
            max_words,
        }
    }

    pub fn get_words(&self, min_occurrences: u32) -> Vec<WcPair> {
        self.words
            .iter()
            .filter(|(_, &count)| count > min_occurrences)
            .map(|(word, &count)| WcPair {
                count,
                word: word.clone(),
            })
            .collect()
    }

    pub fn size(&self) -> usize {
        self.words.len()
    }

    pub fn clear(&mut self) {
        self.words.clear();
    }

    pub fn add_word(&mut self, word: &[u8]) {
        while self.words.len() > self.max_words {
            self.words.retain(|_, count| {
                *count /= 2;
                *count != 0
            });
        }
        *self.words.entry(word.to_vec()).or_insert(0) += 1;
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CodeWord {
    code: u32,
}

impl CodeWord {
    pub fn new(num_bytes: u8, c1: u8, c2: u8, c3: u8) -> Self {
        let mut code = num_bytes as u32;
        code = (code << 8) | c1 as u32;
        code = (code << 8) | c2 as u32;
        code = (code << 8) | c3 as u32;
        Self { code }
    }

    pub fn byte1(&self) -> u8 {
        (self.code >> 16) as u8
    }

    pub fn byte2(&self) -> u8 {
        (self.code >> 8) as u8
    }

    pub fn byte3(&self) -> u8 {
        self.code as u8
    }

    pub fn num_bytes(&self) -> usize {
        (self.code >> 24) as usize
    }

    pub fn raw(&self) -> u32 {
        self.code
    }
}

#[derive(Clone, Debug, Default)]
pub struct CodeWordSet {
    pub num1: usize,
    pub num2: usize,
    pub num3: usize,
    pub codewords: Vec<Vec<u8>>,
}

#[derive(Clone)]
pub struct Builder {
    word: [u8; 0x20],
    word_pos: usize,
    words: WordCollectionMap,
}

impl Default for Builder {
    fn default() -> Self {
        Self::new()
    }
}

impl Builder {
    pub const MIN_WORD_LEN: usize = 3;
    pub const MAX_WORD_LEN: usize = 0x20;
    pub const MIN_OCCURRENCES: usize = 100;

    pub fn new() -> Self {
        Self {
            word: [0; 0x20],
            word_pos: 0,
            words: WordCollectionMap::default(),
        }
    }

    pub fn add_char(&mut self, c: u8) {
        if is_word_char(c) {
            if self.word_pos < Self::MAX_WORD_LEN {
                self.word[self.word_pos] = c;
                self.word_pos += 1;
            }
        } else {
            if self.word_pos >= Self::MIN_WORD_LEN {
                let first_cap = self.word[0].is_ascii_uppercase();
                let mut cap_count = usize::from(first_cap);
                for &b in &self.word[1..self.word_pos] {
                    cap_count += usize::from(b.is_ascii_uppercase());
                }
                if cap_count == self.word_pos {
                    for b in &mut self.word[..self.word_pos] {
                        *b = b.to_ascii_lowercase();
                    }
                } else if first_cap && cap_count == 1 {
                    self.word[0] = self.word[0].to_ascii_lowercase();
                }
                let word = self.word[..self.word_pos].to_vec();
                self.words.add_word(&word);
            }
            self.word_pos = 0;
        }
    }

    pub fn words(&self) -> &WordCollectionMap {
        &self.words
    }

    pub fn words_mut(&mut self) -> &mut WordCollectionMap {
        &mut self.words
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn build(text: &[u8]) -> Builder {
        let mut b = Builder::new();
        for &c in text {
            b.add_char(c);
        }
        b.add_char(b' '); // flush the last word
        b
    }

    #[test]
    fn counts_repeated_words() {
        let b = build(b"hello world hello world hello");
        let words = b.words().get_words(0);
        let hello = words.iter().find(|w| w.word == b"hello").expect("hello");
        assert_eq!(hello.count, 3);
        let world = words.iter().find(|w| w.word == b"world").expect("world");
        assert_eq!(world.count, 2);
    }

    #[test]
    fn folds_case() {
        let b = build(b"Hello HELLO hello");
        let words = b.words().get_words(0);
        assert_eq!(words.len(), 1, "case variants must fold together");
        assert_eq!(words[0].word, b"hello");
        assert_eq!(words[0].count, 3);

        let b = build(b"McDonald McDonald");
        let words = b.words().get_words(0);
        assert_eq!(words[0].word, b"McDonald");
        assert_eq!(words[0].count, 2);
    }

    #[test]
    fn ignores_short_words() {
        let b = build(b"a ab abc");
        let words = b.words().get_words(0);
        assert_eq!(words.len(), 1);
        assert_eq!(words[0].word, b"abc");
    }

    #[test]
    fn eviction_halves_counts() {
        let mut map = WordCollectionMap::new(1);
        map.add_word(b"aaaa");
        map.add_word(b"bbbb");
        map.add_word(b"aaaa");
        map.add_word(b"cccc");
        assert!(map.size() <= 3);
    }

    #[test]
    fn code_word_packs_bytes() {
        let c = CodeWord::new(3, 0x81, 0x90, 0xA5);
        assert_eq!(c.num_bytes(), 3);
        assert_eq!(c.byte1(), 0x81);
        assert_eq!(c.byte2(), 0x90);
        assert_eq!(c.byte3(), 0xA5);
        let c = CodeWord::new(1, 0x81, 0, 0);
        assert_eq!(c.num_bytes(), 1);
        assert_eq!(c.byte1(), 0x81);
    }
}

// `CodeWordGeneratorFast::generateCodeWords`.
pub fn generate_code_words(builder: &mut Builder, out: &mut CodeWordSet) {
    out.codewords.clear();
    let mut pairs = builder.words_mut().get_words(9);
    builder.words_mut().clear();
    let start = Instant::now();
    if pairs.is_empty() {
        out.num1 = 0;
        out.num2 = 0;
        out.num3 = 0;
        println!(
            "Constructed dict words=0+0+0=0 save=0+0+0=0 extra=0 time={}s",
            fmt_g(start.elapsed().as_secs_f64(), 6)
        );
        return;
    }
// Descending by saved bytes.
    let cmp1 = compare_wc_pair(1);
    pairs.sort_unstable_by(|a, b| cmp1(b, a).then(b.count.cmp(&a.count)).then(a.word.cmp(&b.word)));

// two-byte codes still covers the remainder.
    out.num1 = 32.min(pairs.len());
    while out.num1 + 1 < pairs.len() {
        let remain = 128 - out.num1;
        let new_2 = remain.saturating_sub(1) * remain.saturating_sub(1);
        if remain == 0 || new_2 < pairs.len() - out.num1 {
            break;
        }
        out.num1 += 1;
    }
    let num1 = out.num1;

    let mut save1 = 0u64;
    for p in pairs.iter().take(num1) {
        save1 += (p.count as u64 - 1) * (p.word.len() as u64 - 1);
        out.codewords.push(p.word.clone());
    }
    out.codewords.sort_unstable();
    let mut rest: Vec<WcPair> = pairs.split_off(num1);
    let cmp2 = compare_wc_pair(2);
    rest.sort_unstable_by(|a, b| cmp2(b, a));

    let mut num3 = 0usize;
    while num3 < 32 && num3 + num1 < 128 {
        let count3 = num3 * 128 * 128;
        let count2 = (128 - num3 - num1) * 128;
        if count2 + count3 >= rest.len() {
            break;
        }
        num3 += 1;
    }
    out.num3 = num3;
    let end2 = 256 - out.num3;
    out.num2 = end2 - (128 + num1).min(end2);

    let mut count2 = 0usize;
    let mut save2 = 0u64;
    for _b1 in (128 + num1)..end2 {
        for _b2 in 128..256 {
            if count2 < rest.len() {
                let p = &rest[count2];
                save2 += (p.count as u64 - 1) * (p.word.len() as u64 - 2);
                out.codewords.push(p.word.clone());
                count2 += 1;
            }
        }
    }
    out.codewords[num1..].sort_unstable();
    let mut rest: Vec<WcPair> = rest.split_off(count2);
    let cmp3 = compare_wc_pair(3);
    rest.sort_unstable_by(|a, b| cmp3(b, a));

    let mut count3 = 0usize;
    let mut save3 = 0u64;
    for _b1 in end2..256 {
        for _b2 in 128..256 {
            for _b3 in 128..256 {
                if count3 < rest.len() {
                    let p = &rest[count3];
                    save3 += (p.count as u64 - 1) * (p.word.len() as u64 - 3);
                    out.codewords.push(p.word.clone());
                    count3 += 1;
                }
            }
        }
    }
    let extra: u64 = rest[count3..]
        .iter()
        .map(|p| (p.count as u64 - 1) * (p.word.len() as u64 - 3))
        .sum();
    println!(
        "Constructed dict words={}+{}+{}={} save={}+{}+{}={} extra={} time={}s",
        num1,
        count2,
        count3,
        num1 + count2 + count3,
        save1,
        save2,
        save3,
        save1 + save2 + save3,
        extra,
        fmt_g(start.elapsed().as_secs_f64(), 6)
    );
}

#[derive(Clone, Debug, Default)]
struct DecodeTables {
    words1b: Vec<Vec<u8>>,
    words2b: Vec<Vec<u8>>,
    words3b: Vec<Vec<u8>>,
}

pub struct DictFilter {
    escape_char: u8,
    escape_cap_first: u8,
    escape_cap_word: u8,
    last_char: u8,
    dict_buffer: Vec<u8>,
    dict_buffer_pos: usize,
    dict_buffer_size: usize,
    encode_map: HashMap<Vec<u8>, CodeWord>,
    decode: DecodeTables,
    word1bstart: usize,
    word2bstart: usize,
    word3bstart: usize,
}

impl DictFilter {
    pub fn new_encoder(set: &CodeWordSet, escape_char: u8, escape_cap_first: u8) -> Self {
        let escape_cap_word = 0x6;
        let mut f = Self {
            escape_char,
            escape_cap_first,
            escape_cap_word,
            last_char: 0,
            dict_buffer: Vec::new(),
            dict_buffer_pos: 0,
            dict_buffer_size: 0,
            encode_map: HashMap::new(),
            decode: DecodeTables::default(),
            word1bstart: 128,
            word2bstart: 0,
            word3bstart: 0,
        };
        f.add_code_words(set);
        f
    }

    pub fn new_decoder() -> Self {
        Self {
            escape_char: 0,
            escape_cap_first: 0,
            escape_cap_word: 0,
            last_char: 0,
            dict_buffer: Vec::new(),
            dict_buffer_pos: 0,
            dict_buffer_size: 4,
            encode_map: HashMap::new(),
            decode: DecodeTables::default(),
            word1bstart: 128,
            word2bstart: 0,
            word3bstart: 0,
        }
    }

    fn add_code_words(&mut self, set: &CodeWordSet) {
        self.dict_buffer.clear();
        self.dict_buffer.extend_from_slice(&[0, 0, 0, 0]);
        self.dict_buffer.push(self.escape_char);
        self.dict_buffer.push(self.escape_cap_first);
        self.dict_buffer.push(self.escape_cap_word);
        self.dict_buffer.push(set.num1 as u8);
        self.dict_buffer.push(set.num2 as u8);
        self.dict_buffer.push(set.num3 as u8);
        for w in &set.codewords {
            self.dict_buffer.extend_from_slice(w);
            self.dict_buffer.push(0);
        }
        self.dict_buffer_pos = 0;
        self.dict_buffer_size = self.dict_buffer.len();
        let size = self.dict_buffer_size as u32;
        self.dict_buffer[0..4].copy_from_slice(&size.to_be_bytes());
        println!(
            "Dictionary words={} size={}",
            set.codewords.len(),
            pretty_size(size as u64)
        );
        self.generate(set.num1, set.num2, set.num3);
    }

    fn create_from_buffer(&mut self) {
        let buf = self.dict_buffer.clone();
        if buf.len() < 10 {
            return;
        }
        self.escape_char = buf[4];
        self.escape_cap_first = buf[5];
        self.escape_cap_word = buf[6];
        let num1 = buf[7] as usize;
        let num2 = buf[8] as usize;
        let num3 = buf[9] as usize;
        self.word1bstart = 128;
        self.word2bstart = self.word1bstart + num1;
        self.word3bstart = self.word2bstart + num2;
        let mut pos = 10;
        let mut words: Vec<Vec<u8>> = Vec::new();
        while pos < buf.len() {
            let start = pos;
            while pos < buf.len() && buf[pos] != 0 {
                pos += 1;
            }
            words.push(buf[start..pos].to_vec());
            pos += 1; // skip the terminator
        }
        println!(
            "Dictionary words={} size={}",
            words.len(),
            pretty_size(buf.len() as u64)
        );
        self.generate_decode(&words, num1, num2, num3);
    }

// `Dict::Filter::generate` in encoding mode.
    fn generate(&mut self, num1: usize, num2: usize, num3: usize) {
        let start = 128usize;
        let end1 = start + num1;
        let end2 = end1 + num2;
        let end3 = end2 + num3;
        let words = self.dict_buffer_words();
        let mut idx = 0usize;
        for b1 in start..end1 {
            if idx < words.len() {
                self.encode_map
                    .insert(words[idx].clone(), CodeWord::new(1, b1 as u8, 0, 0));
                idx += 1;
            }
        }
        for b1 in end1..end2 {
            for b2 in start..256 {
                if idx < words.len() {
                    self.encode_map.insert(
                        words[idx].clone(),
                        CodeWord::new(2, b1 as u8, b2 as u8, 0),
                    );
                    idx += 1;
                }
            }
        }
        for b1 in end2..end3 {
            for b2 in start..256 {
                for b3 in start..256 {
                    if idx < words.len() {
                        self.encode_map.insert(
                            words[idx].clone(),
                            CodeWord::new(3, b1 as u8, b2 as u8, b3 as u8),
                        );
                        idx += 1;
                    }
                }
            }
        }
    }

    fn dict_buffer_words(&self) -> Vec<Vec<u8>> {
        let buf = &self.dict_buffer;
        let mut pos = 10usize;
        let mut words = Vec::new();
        while pos < buf.len() {
            let start = pos;
            while pos < buf.len() && buf[pos] != 0 {
                pos += 1;
            }
            words.push(buf[start..pos].to_vec());
            pos += 1;
        }
        words
    }

    fn generate_decode(&mut self, words: &[Vec<u8>], num1: usize, num2: usize, num3: usize) {
        let start = 128usize;
        let end1 = start + num1;
        let end2 = end1 + num2;
        let end3 = end2 + num3;
        let mut idx = 0usize;
        self.decode = DecodeTables::default();
        for _b1 in start..end1 {
            if idx < words.len() {
                self.decode.words1b.push(words[idx].clone());
                idx += 1;
            }
        }
        for _b1 in end1..end2 {
            for _b2 in start..256 {
                if idx < words.len() {
                    self.decode.words2b.push(words[idx].clone());
                    idx += 1;
                }
            }
        }
        for _b1 in end2..end3 {
            for _b2 in start..256 {
                for _b3 in start..256 {
                    if idx < words.len() {
                        self.decode.words3b.push(words[idx].clone());
                        idx += 1;
                    }
                }
            }
        }
    }
}

impl Default for DictFilter {
    fn default() -> Self {
        Self::new_decoder()
    }
}

impl crate::filter::Transform for DictFilter {
    fn forward(&mut self, out: &mut [u8], input: &[u8]) -> (usize, usize) {
        let in_limit = input.len();
        let out_limit = out.len();
        let mut in_ptr = 0usize;
        let mut out_ptr = 0usize;

        let remain_dict = self.dict_buffer_size.saturating_sub(self.dict_buffer_pos);
        if remain_dict > 0 {
            let max_write = remain_dict.min(out_limit);
            out[..max_write]
                .copy_from_slice(&self.dict_buffer[self.dict_buffer_pos..self.dict_buffer_pos + max_write]);
            out_ptr += max_write;
            self.dict_buffer_pos += max_write;
        }

        while in_ptr < in_limit && out_ptr + 4 < out_limit {
            if !self.word_char(self.last_char) {
                if self.word_char(input[in_ptr]) {
                    let mut word_len = 0usize;
                    while word_len < MAX_WORD_LEN
                        && in_ptr + word_len < in_limit
                        && self.word_char(input[in_ptr + word_len])
                    {
                        word_len += 1;
                    }
                    if in_ptr + word_len >= in_limit && word_len != in_limit {
// The word may continue in the next buffer.
                        break;
                    }
                    if (3..=MAX_WORD_LEN).contains(&word_len) {
                        let mut word = input[in_ptr..in_ptr + word_len].to_vec();
                        let mut upper_count = 0usize;
                        for &c in &word {
                            upper_count += usize::from(c.is_ascii_uppercase());
                        }
                        if upper_count == word_len {
                            for c in word.iter_mut() {
                                *c = c.to_ascii_lowercase();
                            }
                        } else if upper_count == 1 && input[in_ptr].is_ascii_uppercase() {
                            word[0] = word[0].to_ascii_lowercase();
                        }
                        if let Some(cw) = self.encode_map.get(&word).copied() {
                            if upper_count == word_len {
                                out[out_ptr] = self.escape_cap_word;
                                out_ptr += 1;
                            } else if upper_count == 1 && input[in_ptr].is_ascii_uppercase() {
                                out[out_ptr] = self.escape_cap_first;
                                out_ptr += 1;
                            }
                            out[out_ptr] = cw.byte1();
                            out_ptr += 1;
                            if cw.num_bytes() > 1 {
                                out[out_ptr] = cw.byte2();
                                out_ptr += 1;
                            }
                            if cw.num_bytes() > 2 {
                                out[out_ptr] = cw.byte3();
                                out_ptr += 1;
                            }
                            in_ptr += word_len;
                            self.last_char = b'a';
                            continue;
                        }
                    }
                }
                let c = input[in_ptr];
                if c == self.escape_char
                    || c == self.escape_cap_first
                    || c == self.escape_cap_word
                    || c >= 128
                {
                    out[out_ptr] = self.escape_char;
                    out_ptr += 1;
                }
            }
            let c = input[in_ptr];
            self.last_char = c;
            out[out_ptr] = c;
            out_ptr += 1;
            in_ptr += 1;
        }
        (out_ptr, in_ptr)
    }

    fn reverse(&mut self, out: &mut [u8], input: &[u8]) -> (usize, usize) {
        let in_limit = input.len();
        let out_limit = out.len();
        let mut in_ptr = 0usize;
        let mut out_ptr = 0usize;

// First: collect the dictionary header.
        while self.dict_buffer.len() < self.dict_buffer_size && in_ptr < in_limit {
            self.dict_buffer.push(input[in_ptr]);
            in_ptr += 1;
            if self.dict_buffer.len() == 4 {
                self.dict_buffer_size = u32::from_be_bytes([
                    self.dict_buffer[0],
                    self.dict_buffer[1],
                    self.dict_buffer[2],
                    self.dict_buffer[3],
                ]) as usize;
            } else if self.dict_buffer.len() == self.dict_buffer_size {
                self.create_from_buffer();
            }
        }

        let max = if in_ptr + 4 < in_limit { in_limit - 4 } else { in_limit };
        while in_ptr < max && out_ptr + MAX_WORD_LEN < out_limit {
            let mut c = input[in_ptr];
            in_ptr += 1;
            if !self.word_char(self.last_char) {
                let first_cap = c == self.escape_cap_first;
                let all_cap = c == self.escape_cap_word;
                if c >= 128 || first_cap || all_cap {
                    if first_cap || all_cap {
                        c = input[in_ptr];
                        in_ptr += 1;
                    }
                    let word: &[u8] = if (c as usize) < self.word2bstart {
                        &self.decode.words1b[(c as usize) - self.word1bstart]
                    } else if (c as usize) < self.word3bstart {
                        let c2 = input[in_ptr];
                        in_ptr += 1;
                        &self.decode.words2b
                            [(c as usize - self.word2bstart) * 128 + c2 as usize - 128]
                    } else {
                        let c2 = input[in_ptr];
                        let c3 = input[in_ptr + 1];
                        in_ptr += 2;
                        &self.decode.words3b[(c as usize - self.word3bstart) * 128 * 128
                            + (c2 as usize - 128) * 128
                            + c3 as usize
                            - 128]
                    };
                    let word_len = word.len();
                    out[out_ptr..out_ptr + word_len].copy_from_slice(word);
                    let capital_c = if all_cap { word_len } else { usize::from(first_cap) };
                    for i in 0..capital_c {
                        out[out_ptr + i] = out[out_ptr + i].to_ascii_uppercase();
                    }
                    out_ptr += word_len;
                    self.last_char = out[out_ptr - 1];
                    continue;
                }
                if c == self.escape_char {
                    c = input[in_ptr];
                    in_ptr += 1;
                }
            }
            self.last_char = c;
            out[out_ptr] = c;
            out_ptr += 1;
        }
        (out_ptr, in_ptr)
    }
}

impl DictFilter {
    fn word_char(&self, c: u8) -> bool {
        is_word_char(c)
    }
}

#[cfg(test)]
mod filter_tests {
    use super::*;
    use crate::filter::ByteStreamFilter;
    use crate::filter::FilterStream;
    use crate::stream::{ByteRead, ByteWrite, SliceReader, VecWriter};

    fn corpus() -> Vec<u8> {
        let words = [
            "compression", "dictionary", "entropy", "model", "context", "mixing",
            "predictor", "stream", "bit", "range", "coder", "filter",
        ];
        let mut text = Vec::new();
        let mut state: u32 = 12345;
        for i in 0..4000 {
            state = state.wrapping_mul(1103515245).wrapping_add(12345);
            let w = words[(state >> 16) as usize % words.len()];
            if i % 7 == 0 {
                text.extend_from_slice(w.to_uppercase().as_bytes());
            } else if i % 11 == 0 {
                let mut c = w.as_bytes().to_vec();
                c[0] = c[0].to_ascii_uppercase();
                text.extend_from_slice(&c);
            } else {
                text.extend_from_slice(w.as_bytes());
            }
            text.push(if i % 5 == 0 { b'.' } else { b' ' });
        }
        text
    }

    fn encode(data: &[u8], set: &CodeWordSet) -> Vec<u8> {
        let mut f = ByteStreamFilter::<_, _, 16384, 16384>::new(
            SliceReader::new(data),
            DictFilter::new_encoder(set, 0x3, 0x4),
        );
        let mut out = Vec::new();
        let mut buf = [0u8; 4096];
        loop {
            let n = f.read(&mut buf);
            if n == 0 {
                break;
            }
            out.extend_from_slice(&buf[..n]);
        }
        out
    }

    fn decode(data: &[u8]) -> Vec<u8> {
// text to the wrapped stream.
        let mut f = ByteStreamFilter::<_, _, 16384, 16384>::new(
            VecWriter::new(),
            DictFilter::new_decoder(),
        );
        f.write(data);
        f.flush();
        f.into_stream().data
    }

    #[test]
    fn dict_filter_roundtrips_and_shrinks() {
        let data = corpus();
        let mut builder = Builder::new();
        for &c in &data {
            builder.add_char(c);
        }
        let mut set = CodeWordSet::default();
        generate_code_words(&mut builder, &mut set);
        assert!(set.codewords.len() >= 12, "{}", set.codewords.len());
        let packed = encode(&data, &set);
        assert!(packed.len() < data.len() / 2, "{} vs {}", packed.len(), data.len());
        assert_eq!(decode(&packed), data);
    }
}
