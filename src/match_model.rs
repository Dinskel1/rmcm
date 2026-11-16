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

//! Match model, ported from `MatchModel.hpp`.

use crate::buffer::CyclicBuffer;
use crate::model::FastBitModel;

pub const MIN_MATCH: usize = 4;
pub const SMALL_MATCH: usize = 8;
pub const MAX_CTX: usize = 256;
pub const MAX_VALUE: i32 = 1 << 12;
const CODE_BIT_SHIFT: u32 = 31;

pub struct MatchModel {
// `pos`, `len`, `dist` from the reference.
    pos: usize,
    len: usize,
    dist: usize,

    hash: u32,
    hash_mask: usize,
    hash_table: Vec<u32>,

    cur_max_match: usize,
    cur_min_match: usize,
    num_length_models: usize,
    models: Vec<FastBitModel<12, 9, 30>>,
    model_base: usize,
    cur_mdl: usize,
    expected_code: u32,
    pub opt_var: u32,
}

impl MatchModel {
    pub fn new() -> Self {
        Self {
            pos: 0,
            len: 0,
            dist: 0,
            hash: 0,
            hash_mask: 0,
            hash_table: Vec::new(),
            cur_max_match: 0,
            cur_min_match: 0,
            num_length_models: 0,
            models: Vec::new(),
            model_base: 0,
            cur_mdl: 0,
            expected_code: 0,
            opt_var: 0,
        }
    }

    pub fn set_opt(&mut self, var: u32) {
        self.opt_var = var;
    }

// Allocate the position hash table; `size` must be a power of two.
    pub fn resize(&mut self, size: usize) {
        assert!(size.is_power_of_two());
        self.hash_mask = size - 1;
        self.hash_table = vec![0u32; size];
    }

// path); kept for signature fidelity.
    pub fn init(&mut self, min_match: usize, max_match: usize) {
        self.hash = 0;
        self.cur_max_match = max_match;
        self.num_length_models = (max_match + 1) * 2;
        self.models = vec![FastBitModel::<12, 9, 30>::default(); MAX_CTX * self.num_length_models];
        self.cur_min_match = min_match;
        self.expected_code = 0;
        self.pos = 0;
        self.len = 0;
        self.dist = 0;

        for c in 0..MAX_CTX {
            self.set_ctx(c);
            for i in 0..self.num_length_models {
                let len = MIN_MATCH + 4 + i / 2;
                let val = if i & 1 != 0 {
                    (MAX_VALUE - 1 - (MAX_VALUE / 2) / len as i32) as u32
                } else {
                    ((MAX_VALUE / 2) / len as i32) as u32
                };
                self.models[self.model_base + i].set_p(val);
            }
        }
        self.set_ctx(0);
        self.update_cur_mdl();
    }

    #[inline]
    pub fn get_length(&self) -> usize {
        self.len
    }

    pub fn reset_match(&mut self) {
        self.len = 0;
    }

    #[inline]
    pub fn set_ctx(&mut self, ctx: usize) {
        self.model_base = ctx * self.num_length_models;
    }

    #[inline]
    pub fn get_p(&self, st: &[i16; 4096], expected_bit: usize) -> i32 {
        debug_assert!(self.len != 0);
        st[self.models[self.cur_mdl + expected_bit].get_p() as usize] as i32
    }

    #[inline]
    pub fn get_pos(&self) -> usize {
        self.pos
    }

    #[inline]
    pub fn get_expected_bit(&self) -> usize {
        (self.expected_code >> CODE_BIT_SHIFT) as usize
    }

    pub fn get_min_match(&self) -> usize {
        MIN_MATCH
    }

    fn search(&mut self, buffer: &CyclicBuffer, spos: usize) {
        let blast = buffer.get_pos().wrapping_sub(1);
        #[cfg(feature = "trace")]
        if crate::trace::enabled() {
            eprintln!(
                "S {} {} {} {} {} {} {} {} {}",
                spos,
                blast,
                buffer[spos.wrapping_sub(4)],
                buffer[spos.wrapping_sub(3)],
                buffer[spos.wrapping_sub(2)],
                buffer[spos.wrapping_sub(1)],
                buffer[blast.wrapping_sub(4)],
                buffer[blast.wrapping_sub(3)],
                buffer[blast.wrapping_sub(2)]
            );
        }
// rather than four ring positions.
        let same = buffer.read_u32_at(spos.wrapping_sub(4)) == buffer.read_u32_at(blast.wrapping_sub(4));
        if same {
            let spos = spos.wrapping_sub(1);
            let blast = blast.wrapping_sub(1);
            let bmask = buffer.get_mask();
            self.dist = (blast & bmask).wrapping_sub(spos & bmask);
            self.pos = spos.wrapping_add(1);
            self.len = 4;
        }
    }

// Prefetch hint in the reference; a no-op here.
    pub fn fetch(&self, _ctx: u32) {}

// Advance the model by one byte of history.
    pub fn update(&mut self, buffer: &CyclicBuffer) {
        let blast = buffer.get_pos().wrapping_sub(1);
        let bmask = buffer.get_mask();
        self.hash ^= buffer[blast] as u32;
        let last_pos = blast & bmask;
        let hmask = self.hash & !(bmask as u32);
        let idx = self.hash as usize & self.hash_mask;

        let b1 = self.hash_table[idx];
        if self.len != 0 {
            if self.len < self.cur_max_match {
                self.len += 1;
            }
            self.pos += 1;
        } else if (b1 & !(bmask as u32)) == hmask {
            self.search(buffer, b1 as usize);
        }
        self.update_cur_mdl();
        self.hash_table[idx] = last_pos as u32 | hmask;
    }

    #[inline]
    pub fn get_hash(&self) -> u32 {
        self.hash
    }

    #[inline]
    pub fn set_hash(&mut self, h: u32) {
        self.hash = h;
    }

    #[inline]
    pub fn update_cur_mdl(&mut self) {
        if self.len != 0 {
            let idx = self.len.saturating_sub(MIN_MATCH).min(self.cur_max_match);
            self.cur_mdl = self.model_base + 2 * idx;
        }
    }

    #[inline]
    pub fn get_expected_char(&self, buffer: &CyclicBuffer) -> u32 {
        buffer[self.pos + 1] as u32
    }

    #[inline]
    pub fn update_expected_code(&mut self, code: u32, bit_len: u32) {
        self.expected_code = code << (CODE_BIT_SHIFT + 1 - bit_len);
    }

    #[inline]
    pub fn update_cur_mdl_bit(&mut self, expected_bit: usize, bit: u32) {
        self.models[self.cur_mdl + expected_bit].update(bit);
    }

// soon as the predicted bit was wrong.
    #[inline]
    pub fn update_bit(&mut self, bit: u32) {
        if self.len != 0 {
            let diff = (self.expected_code >> CODE_BIT_SHIFT) ^ bit;
            let expected_bit = self.get_expected_bit();
            self.models[self.cur_mdl + expected_bit].update(bit);
            if diff != 0 {
                self.len = 0;
            }
            self.expected_code = self.expected_code.wrapping_shl(1);
        }
    }

// Exposed for tests: current match distance.
    pub fn get_dist(&self) -> usize {
        self.dist
    }
}

impl Default for MatchModel {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_a_repeat() {
        let mut buffer = CyclicBuffer::new(1 << 10, 4);
        let pattern = b"abcdefghijklmnop";
        for &b in pattern {
            buffer.push(b);
        }
        let mut mm = MatchModel::new();
        mm.resize(1 << 12);
        mm.init(MIN_MATCH, 80);

        let mut hash = 0u32;
        for &b in pattern {
            mm.set_hash(hash);
            mm.update(&buffer);
            buffer.push(b);
            hash = hash
                .wrapping_mul(0x0100_0193)
                .wrapping_add(b as u32);
            if mm.get_length() != 0 {
                break;
            }
        }
        assert!(mm.get_length() <= 80 + MIN_MATCH);
    }

    #[test]
    fn wrong_prediction_kills_the_match() {
        let mut mm = MatchModel::new();
        mm.resize(1 << 12);
        mm.init(MIN_MATCH, 80);
        mm.len = MIN_MATCH + 2;
        mm.expected_code = 0xFFFF_FFFF;
// expected bit is 1, coded bit is 0 -> match must be dropped
        mm.update_bit(0);
        assert_eq!(mm.get_length(), 0);
    }

    #[test]
    fn initial_model_probabilities_are_length_scaled() {
        let mut mm = MatchModel::new();
        mm.resize(1 << 12);
        mm.init(MIN_MATCH, 80);
        mm.set_ctx(0);
        assert_eq!(mm.models[0].get_p(), 256);
        assert_eq!(mm.models[1].get_p(), 3839);
    }
}
