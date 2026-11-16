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

//! Secondary Symbol Estimation (SSE / APM), ported from `SSE.hpp`.

use crate::log_table::SsTable;
use crate::model::{BitLearnModel, FastBitModel};

const fn max(a: u32, b: u32) -> u32 {
    if a > b {
        a
    } else {
        b
    }
}

#[derive(Clone)]
pub struct Sse<const PROB_BITS: u32 = 12, const STEM_BITS: u32 = 5> {
    models: Vec<BitLearnModel<PROB_BITS, 8, 30>>,
    pw: usize,
    opt: u64,
    count: u64,
}

impl<const PROB_BITS: u32, const STEM_BITS: u32> Default for Sse<PROB_BITS, STEM_BITS> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const PROB_BITS: u32, const STEM_BITS: u32> Sse<PROB_BITS, STEM_BITS> {
    pub const STEMS: u32 = (1 << STEM_BITS) + 1;
    pub const MAX_P: u32 = 1 << PROB_BITS;
    pub const PROB_MASK: u32 = (1 << (PROB_BITS - STEM_BITS)) - 1;
    pub const STEM_SHIFT: u32 = PROB_BITS - STEM_BITS;

    pub fn new() -> Self {
        Self {
            models: Vec::new(),
            pw: 0,
            opt: 0,
            count: 0,
        }
    }

    pub fn set_opt(&mut self, v: u64) {
        self.opt = v;
    }

// (`257 * 256` for both SSE stages in CM), and it must be a multiple of the
    pub fn init(&mut self, num_ctx: usize, table: Option<&SsTable>) {
        assert!(num_ctx > 0);
        self.pw = 0;
        self.count = 0;

        let stems = Self::STEMS as usize;
        self.models = vec![BitLearnModel::<PROB_BITS, 8, 30>::default(); num_ctx * stems];

        for i in 0..Self::STEMS {
            let p = max((i << Self::STEM_SHIFT).min(Self::MAX_P), 0);
            let init = match table {
                Some(t) => t.sq(p as i32 - 2048),
                None => p,
            };
            self.models[i as usize].init(init);
        }

        let mut ctx = 1usize;
        while ctx * 2 <= num_ctx {
            let n = stems * ctx;
            self.models.copy_within(0..n, n);
            ctx *= 2;
        }
        let tail = stems * (num_ctx - ctx);
        if tail > 0 {
            self.models.copy_within(0..tail, ctx * stems);
        }
    }

    pub fn num_models(&self) -> usize {
        self.models.len()
    }

    pub fn models(&self) -> &[BitLearnModel<PROB_BITS, 8, 30>] {
        &self.models
    }

// model to update.
    #[inline]
    pub fn p(&mut self, p: u32, ctx: usize) -> u32 {
        debug_assert!(p < Self::MAX_P);
        let idx = (p >> Self::STEM_SHIFT) as usize;
        debug_assert!(idx < Self::STEMS as usize);
        let s1 = ctx * Self::STEMS as usize + idx;
        let mask = p & Self::PROB_MASK;
        self.pw = s1 + (mask >> (Self::STEM_SHIFT - 1)) as usize;
        let hi = self.models[s1].get_p();
        let lo = self.models[s1 + 1].get_p();
        (hi.wrapping_mul(1 + Self::PROB_MASK - mask).wrapping_add(lo.wrapping_mul(mask)))
            >> Self::STEM_SHIFT
    }

    #[inline]
    pub fn update(&mut self, bit: u32) {
        self.models[self.pw].update(bit);
    }
}

#[derive(Clone)]
pub struct FastSse<const PROB_BITS: u32 = 12, const STEM_BITS: u32 = 5> {
    models: Vec<FastBitModel<PROB_BITS, 8, 30>>,
    pw: usize,
    opt: u64,
}

impl<const PROB_BITS: u32, const STEM_BITS: u32> Default for FastSse<PROB_BITS, STEM_BITS> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const PROB_BITS: u32, const STEM_BITS: u32> FastSse<PROB_BITS, STEM_BITS> {
    pub const STEMS: u32 = 1 << STEM_BITS;
    pub const MAX_P: u32 = 1 << PROB_BITS;

    pub fn new() -> Self {
        Self {
            models: Vec::new(),
            pw: 0,
            opt: 0,
        }
    }

    pub fn set_opt(&mut self, v: u64) {
        self.opt = v;
    }

    pub fn init(&mut self, num_ctx: usize, table: Option<&SsTable>) {
        assert!(num_ctx > 0);
        let stems = Self::STEMS as usize;
        self.models = vec![FastBitModel::<PROB_BITS, 8, 30>::default(); num_ctx * stems];
        for i in 0..Self::STEMS {
            let p = (i << (PROB_BITS - STEM_BITS)) + (1 << (PROB_BITS - STEM_BITS - 1));
            let init = match table {
                Some(t) => t.sq(p as i32 - 2048),
                None => p,
            };
            self.models[i as usize].init(init);
        }
        let mut ctx = 1usize;
        while ctx * 2 <= num_ctx {
            let n = stems * ctx;
            self.models.copy_within(0..n, n);
            ctx *= 2;
        }
        let tail = stems * (num_ctx - ctx);
        if tail > 0 {
            self.models.copy_within(0..tail, ctx * stems);
        }
    }

    #[inline]
    pub fn p(&mut self, p: u32, ctx: usize) -> u32 {
        debug_assert!(p < Self::MAX_P);
        let idx = (p >> (PROB_BITS - STEM_BITS)) as usize;
        debug_assert!(idx < Self::STEMS as usize);
        self.pw = ctx * Self::STEMS as usize + idx;
        self.models[self.pw].get_p()
    }

    #[inline]
    pub fn update(&mut self, bit: u32) {
        self.models[self.pw].update(bit);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn doubling_fill_matches_cpp_layout() {
// 257 * 256, the size CM uses.
        let num_ctx = 257 * 256;
        let t = SsTable::build();
        let mut sse = Sse::<12, 5>::new();
        sse.init(num_ctx, Some(&t));
        assert_eq!(sse.num_models(), num_ctx * 33);

// group equals the seed group.
        let seeds: Vec<u32> = (0..33).map(|i| sse.models()[i].raw()).collect();
        for group in 0..(num_ctx) {
            for i in 0..33 {
                assert_eq!(
                    sse.models()[group * 33 + i].raw(),
                    seeds[i],
                    "group {group} stem {i}"
                );
            }
        }
    }

    #[test]
    fn interpolation_hits_endpoints() {
        let t = SsTable::build();
        let mut sse = Sse::<12, 5>::new();
        sse.init(257 * 256, Some(&t));
// p with mask == 0 uses models[s1] only.
        let expected = sse.models()[0].get_p();
        assert_eq!(sse.p(0, 0), expected);
    }

    #[test]
    fn update_only_touches_selected_model() {
        let t = SsTable::build();
        let mut sse = Sse::<12, 5>::new();
        sse.init(257 * 256, Some(&t));
        let before: Vec<u32> = sse.models().iter().map(|m| m.raw()).collect();
        sse.p(0, 7);
        sse.update(1);
        let after: Vec<u32> = sse.models().iter().map(|m| m.raw()).collect();
        let changed: Vec<usize> = (0..before.len()).filter(|&i| before[i] != after[i]).collect();
        assert_eq!(changed.len(), 1);
    }
}
