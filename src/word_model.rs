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

//! Word model, ported from `WordModel.hpp`.

use crate::generated::WORD_TRANSFORM;
use crate::utf8::Utf8Decoder;

pub const INVALID_CHAR: u16 = 256;
pub const MAX_LEN: usize = 31;

#[derive(Clone)]
pub struct WordModel {
    transform: [u16; 256],
    h1: u32,
    h2: u32,
// Hash of the previous word, rotated.
    prev: u32,
    len: usize,
    decoder: Utf8Decoder<false>,
    pub opt_var: u32,
}

impl Default for WordModel {
    fn default() -> Self {
        Self::new()
    }
}

impl WordModel {
    pub fn new() -> Self {
        Self {
            transform: WORD_TRANSFORM,
            h1: 0,
            h2: 0,
            prev: 0,
            len: 0,
            decoder: Utf8Decoder::new(),
            opt_var: 0,
        }
    }

    pub fn set_opt(&mut self, var: u32) {
        self.opt_var = var;
    }

    pub fn init(&mut self) {
        self.len = 0;
        self.prev = 0;
        self.reset();
        self.decoder.init();
    }

    #[inline]
    pub fn transform_value(&self, c: u8) -> u16 {
        self.transform[c as usize]
    }

    #[inline]
    pub fn reset(&mut self) {
        self.h1 = 0x1F20_239A;
        self.h2 = 0xBE5F_D47A;
        self.len = 0;
    }

    #[inline]
    pub fn get_hash(&self) -> u32 {
        self.h1.wrapping_mul(11).wrapping_add(self.h2.wrapping_mul(7))
    }

    #[inline]
    pub fn get_prev_hash(&self) -> u32 {
        self.prev
    }

    #[inline]
    pub fn get_01_hash(&self) -> u32 {
        self.get_hash() ^ self.get_prev_hash()
    }

    #[inline]
    pub fn get_length(&self) -> usize {
        self.len
    }

    #[inline]
    pub fn hash_func(c: u32, h: u32) -> u32 {
        let mut h = h.wrapping_mul(61);
        h = h.wrapping_add(c);
        h = h.wrapping_add(h.rotate_left(10));
        h ^ (h >> 8)
    }

// Feed one byte of input.
    pub fn update(&mut self, c: u8) {
        let cur = self.transform[c as usize];
        if cur != INVALID_CHAR {
            self.h1 = Self::hash_func(cur as u32, self.h1);
            self.h2 = self.h1.wrapping_mul(4);
            if self.len < MAX_LEN {
                self.len += 1;
            }
        } else if self.len != 0 {
            self.prev = self.get_hash().rotate_left(21);
            self.reset();
        }
    }

// accumulator while a code point is incomplete.
    pub fn update_utf(&mut self, c: u8) {
        self.decoder.update(c as u32);
        let mut cur = self.decoder.acc();
        if self.decoder.done() {
            if cur < 256 {
                cur = self.transform[cur as usize] as u32;
            }
            if cur != INVALID_CHAR as u32 {
                self.h1 = Self::hash_func(cur, self.h1);
                self.h2 = self.h1.wrapping_mul(4);
                if self.len < MAX_LEN {
                    self.len += 1;
                }
            } else if self.len != 0 {
                self.prev = self.get_hash().rotate_left(21);
                self.reset();
            }
        } else {
            self.h2 = Self::hash_func(cur, self.h2);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn letters_share_case() {
        let wm = WordModel::new();
        assert_eq!(wm.transform_value(b'a'), wm.transform_value(b'A'));
    }

    #[test]
    fn digits_are_not_word_characters() {
        let wm = WordModel::new();
        for c in b'0'..=b'9' {
            assert_eq!(wm.transform_value(c), INVALID_CHAR, "byte {c}");
        }
    }

    #[test]
    fn space_terminates_a_word() {
        let mut wm = WordModel::new();
        wm.init();
        for c in b"hello" {
            wm.update(*c);
        }
        assert_eq!(wm.get_length(), 5);
        let word_hash = wm.get_hash();
        wm.update(b' ');
        assert_eq!(wm.get_length(), 0);
        assert_eq!(wm.get_prev_hash(), word_hash.rotate_left(21));
    }

    #[test]
    fn hash_is_stable_for_a_known_word() {
        let mut wm = WordModel::new();
        wm.init();
        for c in b"the" {
            wm.update(*c);
        }
        assert_eq!(wm.get_hash(), 0xADD6_7BBD);
    }
}
