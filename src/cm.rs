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

//! The context mixing engine, ported from `CM.hpp` / `CM.cpp`.

use crate::buffer::CyclicBuffer;
use crate::generated::INITIAL_PROBS;
use crate::log_table::SsTable;
use crate::match_model::{MatchModel, MIN_MATCH};
use crate::mixer::Mixer;
use crate::model::FastBitModel;
use crate::range::Range7;
use crate::sse::Sse;
use crate::stream::{ByteRead, ByteWrite};
use crate::word_model::{WordModel, MAX_LEN};

pub const SHIFT: u32 = 12;
pub const MAX_VALUE: i32 = 1 << 12;
pub const MIN_ST: i32 = -2048;
pub const MAX_ST: i32 = 2048;
const NUM_STATES: usize = 256;
const MIXER_MASK: usize = 0x1FFFF;
const MB: usize = 1 << 20;

const PRED_ARRAYS: usize = 5;
const LZP_PRED: usize = PRED_ARRAYS - 1;

// Static model layout inside `hash_table`.
const O0_SIZE: usize = 0x100 * 2;
const O1_SIZE: usize = O0_SIZE * 0x100;
const O2_SIZE: usize = O0_SIZE * 0x10000;
const O0_POS: usize = 0;
const O1_POS: usize = O0_POS + O0_SIZE;
const O2_POS: usize = O1_POS + O1_SIZE;
const S2_POS: usize = O2_POS + O2_SIZE;
const S3_POS: usize = S2_POS + O1_SIZE;
const S4_POS: usize = S3_POS + O1_SIZE;
const HASH_START: usize = S4_POS + O1_SIZE;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Profile {
    Text = 0,
    Binary = 1,
    Wave16 = 2,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum BitType {
    Lzp,
    Match,
    Normal,
}

pub const MODEL_ORDER0: u32 = 1 << 0;
pub const MODEL_ORDER1: u32 = 1 << 1;
pub const MODEL_ORDER2: u32 = 1 << 2;
pub const MODEL_ORDER3: u32 = 1 << 3;
pub const MODEL_ORDER4: u32 = 1 << 4;
pub const MODEL_ORDER5: u32 = 1 << 5;
pub const MODEL_ORDER6: u32 = 1 << 6;
pub const MODEL_ORDER7: u32 = 1 << 7;
pub const MODEL_ORDER8: u32 = 1 << 8;
pub const MODEL_ORDER9: u32 = 1 << 9;
pub const MODEL_ORDER10: u32 = 1 << 10;
pub const MODEL_ORDER11: u32 = 1 << 11;
pub const MODEL_ORDER12: u32 = 1 << 12;

// `Model::kModelOrder0 + n`.
#[inline]
pub fn model_order(n: usize) -> u32 {
    1 << n
}
pub const MODEL_SPARSE2: u32 = 1 << 13;
pub const MODEL_SPARSE3: u32 = 1 << 14;
pub const MODEL_SPARSE4: u32 = 1 << 15;
pub const MODEL_SPARSE23: u32 = 1 << 16;
pub const MODEL_SPARSE34: u32 = 1 << 17;
pub const MODEL_WORD1: u32 = 1 << 18;
pub const MODEL_WORD2: u32 = 1 << 19;
pub const MODEL_WORDMASK: u32 = 1 << 20;
pub const MODEL_WORD12: u32 = 1 << 21;
pub const MODEL_MASK: u32 = 1 << 22;
pub const MODEL_MATCHHASHE: u32 = 1 << 23;

type PredModel = FastBitModel<12, 9, 30>;

pub struct Cm<const INPUTS: usize, const MIX_N: usize> {
    mem_usage: u32,
    lzp_enabled: bool,
    force_profile: bool,
    profile: Profile,
    opt_var: u32,

    table: SsTable,
    mixers: Vec<Mixer<MIX_N, 17, 11>>,
    cur_profile_mixers: usize,
    mixer_base: usize,

    sse: Sse<12, 5>,
    sse2: Sse<12, 5>,
    sse_ctx: usize,
    #[allow(dead_code)]
    mixer_sse_ctx: u32,

    hash_table: Vec<u8>,
    hash_mask: usize,
    buffer: CyclicBuffer,

    match_model: MatchModel,
    match_model_order: usize,
    min_match_lzp: usize,
    miss_fast_path: u64,
    miss_len: u64,

    word_model: WordModel,
    word_model_ctx_map: [usize; MAX_LEN + 1],
    mask_model: u32,
    current_mask_map: [usize; 256],
    binary_mask_map: [usize; 256],
    text_mask_map: [usize; 256],

    models_enabled: u32,
    max_order: usize,

    state_trans: [[u8; 2]; NUM_STATES],
// `preds[array][input][state]`, flattened.
    preds: Vec<PredModel>,
// Which entry of `preds` `cur_preds` points at.
    cur_preds: usize,

    fast_mix: Vec<PredModel>,

    owhash: u32,
    ent: Range7,
}

impl<const INPUTS: usize, const MIX_N: usize> Cm<INPUTS, MIX_N> {
    pub fn new(mem_usage: u32, lzp_enabled: bool, profile: Option<Profile>) -> Self {
        assert!(MIX_N == INPUTS + 2, "MIX_N must be INPUTS + 2");
        let force_profile = profile.is_some();
        let profile = profile.unwrap_or(Profile::Binary);
        Self {
            mem_usage,
            lzp_enabled,
            force_profile,
            profile,
            opt_var: 0,
            table: SsTable::build(),
            mixers: Vec::new(),
            cur_profile_mixers: 0,
            mixer_base: 0,
            sse: Sse::new(),
            sse2: Sse::new(),
            sse_ctx: 0,
            mixer_sse_ctx: 0,
            hash_table: Vec::new(),
            hash_mask: 0,
            buffer: CyclicBuffer::new(1 << 16, 4),
            match_model: MatchModel::new(),
            match_model_order: 0,
            min_match_lzp: 0,
            miss_fast_path: 0,
            miss_len: 0,
            word_model: WordModel::new(),
            word_model_ctx_map: [0; MAX_LEN + 1],
            mask_model: 0,
            current_mask_map: [0; 256],
            binary_mask_map: [0; 256],
            text_mask_map: [0; 256],
            models_enabled: 0,
            max_order: 0,
            state_trans: [[0; 2]; NUM_STATES],
            preds: Vec::new(),
            cur_preds: 0,
            fast_mix: Vec::new(),
            owhash: 0,
            ent: Range7::new(),
        }
    }

    pub fn set_opt(&mut self, var: u32) {
        self.opt_var = var;
    }

    pub fn get_opt(&self) -> u32 {
        self.opt_var
    }

    pub fn set_mem_usage(&mut self, mem_usage: u32) {
        self.mem_usage = mem_usage;
    }

    pub fn get_buffer(&mut self) -> &mut CyclicBuffer {
        &mut self.buffer
    }

    #[inline]
    fn model_enabled(&self, model: u32) -> bool {
        self.models_enabled & model != 0
    }

    fn set_enabled_models(&mut self, mask: u32) {
        self.models_enabled = mask;
    }

    fn enable_model(&mut self, model: u32) {
        self.models_enabled |= model;
    }

// inside the same 256-byte block.
    #[inline]
    fn hash_lookup(&self, hash: u32) -> usize {
        HASH_START + (hash as usize & self.hash_mask)
    }

// `hashFunc(a, b)` from CM.hpp.
    #[inline]
    fn hash_func(a: u32, b: u32) -> u32 {
        let mut b = b.wrapping_add(a);
        b = b.wrapping_add(b.rotate_left(11));
        b ^ (b >> 6)
    }

    pub fn init(&mut self) {
        self.mixers = Vec::new();
        let num_profiles = if self.force_profile { 1 } else { 4 };
        self.mixers.resize(num_profiles * (MIXER_MASK + 1), Mixer::new());
        for m in self.mixers.iter_mut() {
            m.init(382);
        }
        self.cur_profile_mixers = if self.force_profile {
            0
        } else {
            self.profile as usize * (MIXER_MASK + 1)
        };

        self.mixer_sse_ctx = 0;
        self.sse_ctx = 0;

        self.sse.init(257 * 256, Some(&self.table));
        self.sse2.init(257 * 256, Some(&self.table));

        self.hash_mask = ((2 * MB) << self.mem_usage) - 1;
        let hash_alloc_size = self.hash_mask + HASH_START + (1 << 16);
        self.hash_table = vec![0u8; hash_alloc_size];
        self.buffer = CyclicBuffer::new((MB / 4) << self.mem_usage, 4);

        self.match_model = MatchModel::new();
        self.match_model.resize(self.buffer.get_size() / 2);
        self.match_model.init(MIN_MATCH, 80);
        self.match_model_order = 0;

        for i in 0..256 {
            let i = i as usize;
            self.binary_mask_map[i] = (i < 1) as usize
                + (i < 32) as usize
                + (i < 64) as usize
                + (i < 128) as usize
                + (i < 255) as usize
                + (i < 142) as usize
                + (i < 138) as usize
                + (i < 140) as usize
                + (i < 137) as usize
                + (i < 97) as usize;
            self.text_mask_map[i] = (i < 41) as usize
                + (i < 92) as usize
                + (i < 124) as usize
                + (i < 58) as usize
                + (i < 11) as usize
                + (i < 46) as usize
                + (i < 36) as usize
                + (i < 47) as usize
                + (i < 64) as usize
                + (i < 4) as usize
                + (i < 61) as usize
                + (i < 97) as usize
                + (i < 125) as usize
                + (i < 45) as usize
                + (i < 48) as usize;
        }
        self.current_mask_map = self.binary_mask_map;

        self.word_model.init();
        for (i, v) in self.word_model_ctx_map.iter_mut().enumerate() {
            *v = (i >= 1) as usize
                + (i >= 2) as usize
                + (i >= 3) as usize
                + (i >= 4) as usize
                + (i >= 5) as usize
                + (i >= 6) as usize
                + (i >= 8) as usize;
        }

        {
            let sm = crate::statemap::NsStateMap::build();
            for i in 0..NUM_STATES {
                for j in 0..2 {
                    self.state_trans[i][j] = sm.get_transition(i as u8, j as u32);
                }
            }
        }

        self.preds = vec![PredModel::default(); PRED_ARRAYS * INPUTS * NUM_STATES];
        for array in 0..PRED_ARRAYS {
            for j in 0..INPUTS {
                for k in 0..NUM_STATES {
                    let m = &mut self.preds[(array * INPUTS + j) * NUM_STATES + k];
                    m.init(0);
                    m.set_p(INITIAL_PROBS[j.min(9)][k] as u32);
                }
            }
        }

        self.fast_mix = vec![PredModel::default(); 256 * 256];
        for i in 0..256 {
            let v = (self.table.st(INITIAL_PROBS[0][i] as u32)
                + self.table.st(INITIAL_PROBS[1][i] as u32))
                / 2;
            let p = self.table.sq(v);
            for j in 0..256 {
                self.fast_mix[i * 256 + j].set_p(p);
            }
        }

        self.set_data_profile(self.profile);
        self.owhash = 0;
    }

// `calcMixerBase()`.
    fn calc_mixer_base(&mut self) {
        let mut mixer_ctx = self.current_mask_map[(self.owhash & 0xFF) as usize];
        let word_enabled = self.model_enabled(MODEL_WORD1);
        if word_enabled {
            mixer_ctx <<= 3;
            mixer_ctx |= self.word_model_ctx_map[self.word_model.get_length()];
        }
        let mm_len = self.match_model.get_length();
        mixer_ctx <<= 2;
        if mm_len != 0 {
            mixer_ctx |= if word_enabled {
                1 + (mm_len >= MIN_MATCH + 2) as usize + (mm_len >= MIN_MATCH + 6) as usize
            } else {
                1 + (mm_len >= MIN_MATCH + 1) as usize + (mm_len >= MIN_MATCH + 4) as usize
            };
        }
        self.mixer_base = self.cur_profile_mixers + (mixer_ctx << 8);
    }

    fn calc_prob_base(&mut self) {
        self.cur_preds = self.profile as usize;
    }

// Index into `preds` without a bounds check.
    #[inline(always)]
    fn pred_index(&self, state: u8, ctx: usize) -> usize {
        (self.cur_preds * INPUTS + ctx) * NUM_STATES + state as usize
    }

    #[inline(always)]
    fn next_state(&mut self, state: u8, bit: u32, ctx: usize) -> u8 {
        let idx = self.pred_index(state, ctx);
// SAFETY: see `pred_index`.
        unsafe { self.preds.get_unchecked_mut(idx) }.update_rate(bit, 9);
        self.state_trans[state as usize][bit as usize]
    }

    #[inline(always)]
    fn get_p(&self, state: u8, ctx: usize) -> i32 {
        let idx = self.pred_index(state, ctx);
// SAFETY: see `pred_index`.
        let p = unsafe { self.preds.get_unchecked(idx) }.get_p();
        self.table.st(p)
    }

    fn set_match_model_order(&mut self, order: usize) {
        self.match_model_order = order - 1;
    }

    fn calculate_max_order(&mut self) {
        let mut max_order = 0;
        for order in 0..=12usize {
            if self.model_enabled(1 << order) {
                max_order = order;
            }
        }
        self.max_order = std::cmp::max(self.match_model_order, max_order);
    }

    pub fn set_data_profile(&mut self, profile: Profile) {
        self.profile = profile;
        self.cur_profile_mixers = if self.force_profile {
            0
        } else {
            profile as usize * (MIXER_MASK + 1)
        };
        self.mask_model = 0;
        self.word_model.reset();
        self.set_enabled_models(0);
        let mut idx = 0usize;
        macro_rules! enable {
            ($m:expr) => {
                if INPUTS > idx {
                    idx += 1;
                    self.enable_model($m);
                }
            };
        }
        match profile {
            Profile::Text => {
                enable!(MODEL_ORDER4);
                enable!(MODEL_WORD1);
                enable!(MODEL_ORDER6);
                enable!(MODEL_ORDER2);
                enable!(MODEL_ORDER1);
                enable!(MODEL_MASK);
                enable!(MODEL_ORDER3);
                enable!(MODEL_ORDER8);
                enable!(MODEL_ORDER0);
                enable!(MODEL_WORD12);
                self.set_match_model_order(8);
                self.current_mask_map = self.text_mask_map;
                self.min_match_lzp = if self.lzp_enabled { 9 } else { 100 };
                self.miss_fast_path = 1000000;
            }
            _ => {
                enable!(MODEL_ORDER1);
                enable!(MODEL_ORDER2);
                enable!(MODEL_SPARSE34);
                enable!(MODEL_ORDER4);
                enable!(MODEL_SPARSE23);
                enable!(MODEL_MASK);
                enable!(MODEL_SPARSE4);
                enable!(MODEL_ORDER3);
                enable!(MODEL_SPARSE2);
                enable!(MODEL_SPARSE3);
                self.set_match_model_order(6);
                self.current_mask_map = self.binary_mask_map;
                self.min_match_lzp = if self.lzp_enabled { 0 } else { 100 };
                self.miss_fast_path = 100000;
            }
        }
        let _ = idx;
        self.calculate_max_order();
    }

    pub fn update(&mut self, c: u32) {
        if self.model_enabled(MODEL_WORD1)
            || self.model_enabled(MODEL_WORD2)
            || self.model_enabled(MODEL_WORD12)
        {
            self.word_model.update(c as u8);
        }
        self.buffer.push(c as u8);
        self.owhash = (self.owhash << 8) | (c as u8) as u32;
    }

    #[allow(clippy::too_many_arguments)]
    #[inline]
    fn process_bit<const DECODE: bool, const LZP: bool, R: ByteRead + ?Sized, W: ByteWrite + ?Sized>(
        &mut self,
        input: &mut R,
        output: &mut W,
        mut bit: u32,
        base_contexts: &[usize; 10],
        ctx: usize,
        mixer_ctx: usize,
    ) -> u32 {
        let mm_l = self.match_model.get_length();
        let mut probs = [0i32; 10];
        let mut s = [0u8; 10];
        let mut sp = [0usize; 10];
        let mut sp_valid = [false; 10];

        if LZP {
            probs[0] = self.match_model.get_p(self.table.stretch_raw(), 1);
        } else if mm_l == 0 {
            sp[0] = base_contexts[0] ^ ctx;
            s[0] = self.hash_table[sp[0]];
            sp_valid[0] = true;
            probs[0] = self.get_p(s[0], 0);
        } else {
            probs[0] = self
                .match_model
                .get_p(self.table.stretch_raw(), self.match_model.get_expected_bit());
        }

        for j in 1..INPUTS {
            let idx = base_contexts[j] ^ ctx;
            sp[j] = idx;
            s[j] = self.hash_table[idx];
            sp_valid[j] = true;
            probs[j] = self.get_p(s[j], j);
        }

        let mixer_index = self.mixer_base + mixer_ctx;
        let mut stp = self.mixers[mixer_index].p_fast(9, &probs);
        if stp < MIN_ST {
            stp = MIN_ST;
        }
        if stp >= MAX_ST {
            stp = MAX_ST - 1;
        }
        let mixer_p = self.table.squnsafe(stp);

        let mut p = mixer_p;
        let use_sse = LZP || self.sse_ctx != 0;
        if use_sse {
            p = if LZP {
                self.sse2.p((stp + MAX_VALUE / 2) as u32, self.sse_ctx + mm_l)
            } else {
                self.sse
                    .p((stp + MAX_VALUE / 2) as u32, self.sse_ctx + mixer_ctx)
            };
            p += (p == 0) as u32;
        }

        #[cfg(feature = "trace")]
        if crate::trace::enabled() {
            eprintln!(
                "D {} {} {} {} {}",
                base_contexts[0],
                s[0],
                mm_l,
                (mm_l > 0) as u32,
                self.profile as u32
            );
            crate::trace::record_full(
                p,
                bit as u32,
                ctx as usize,
                stp as u32,
                mixer_p,
                probs[0],
                probs[1],
                probs[2],
                mixer_ctx as usize,
                self.sse_ctx as usize,
                s[1],
                s[2],
                self.cur_preds,
                self.preds[(self.cur_preds * INPUTS + 1) * NUM_STATES + s[1] as usize].raw() as i64,
                self.preds[(self.cur_preds * INPUTS + 2) * NUM_STATES + s[2] as usize].raw() as i64,
                mm_l,
            );
        }

        if DECODE {
            bit = self.ent.get_decoded_bit(p, SHIFT);
        }
        debug_assert!(bit < 2);

        let ret = self.mixers[mixer_index].update_fast(mixer_p as i32, bit, SHIFT, 28, 1, &probs);
        if ret {
            if LZP {
                self.match_model.update_cur_mdl_bit(1, bit);
            } else if mm_l == 0 {
                self.hash_table[sp[0]] = self.next_state(s[0], bit, 0);
            }
            for j in 1..INPUTS {
                self.hash_table[sp[j]] = self.next_state(s[j], bit, j);
            }
        }

        if use_sse {
            if LZP {
                self.sse2.update(bit);
            } else {
                self.sse.update(bit);
            }
        }
        if !LZP {
            self.match_model.update_bit(bit);
        }

        if DECODE {
            self.ent.normalize(input);
        } else {
            self.ent.encode(output, bit, p, SHIFT);
        }
        bit
    }

    fn process_nibble<const DECODE: bool, R: ByteRead + ?Sized, W: ByteWrite + ?Sized>(
        &mut self,
        input: &mut R,
        output: &mut W,
        c: u32,
        base_contexts: &[usize; 10],
        ctx_add: usize,
        ctx_add2: usize,
    ) -> u32 {
        let mut code = c << 28;
        let mut base_ctx = 1usize;
        loop {
            let ctx = ctx_add + base_ctx;
            let mut bit = 0u32;
            if !DECODE {
                bit = code >> 31;
                code <<= 1;
            }
            bit = self.process_bit::<DECODE, false, R, W>(
                input,
                output,
                bit,
                base_contexts,
                ctx_add2 + ctx,
                ctx,
            );
            base_ctx = base_ctx * 2 + bit as usize;
            if base_ctx & 16 != 0 {
                break;
            }
        }
        (base_ctx ^ 16) as u32
    }

    fn process_byte<const DECODE: bool, R: ByteRead + ?Sized, W: ByteWrite + ?Sized>(
        &mut self,
        input: &mut R,
        output: &mut W,
        mut c: u32,
    ) -> u32 {
        let mut base_contexts = [0usize; 10];
        let mut ctx_count = 0usize;
        base_contexts[0] = O0_POS;
        if self.model_enabled(MODEL_ORDER0) {
            base_contexts[ctx_count] = O0_POS;
            ctx_count += 1;
        }
        let bpos = self.buffer.get_pos();
        let p0 = (self.owhash & 0xFF) as usize;
        let p1 = ((self.owhash >> 8) & 0xFF) as usize;
        let p2 = ((self.owhash >> 16) & 0xFF) as usize;
        let p3 = ((self.owhash >> 24) & 0xFF) as usize;

        let push = |ctx: usize, ctx_count: &mut usize, base: &mut [usize; 10]| {
            if *ctx_count < 10 {
                base[*ctx_count] = ctx;
                *ctx_count += 1;
            }
        };

        if self.model_enabled(MODEL_ORDER1) {
            push(O1_POS + p0 * O0_SIZE, &mut ctx_count, &mut base_contexts);
        }
        if self.model_enabled(MODEL_SPARSE2) {
            push(S2_POS + p1 * O0_SIZE, &mut ctx_count, &mut base_contexts);
        }
        if self.model_enabled(MODEL_SPARSE3) {
            push(S3_POS + p2 * O0_SIZE, &mut ctx_count, &mut base_contexts);
        }
        if self.model_enabled(MODEL_SPARSE4) {
            push(S4_POS + p3 * O0_SIZE, &mut ctx_count, &mut base_contexts);
        }
        if self.model_enabled(MODEL_SPARSE23) {
            let h = Self::hash_func(p2 as u32, Self::hash_func(p1 as u32, 0x3722_0B98));
            push(self.hash_lookup(h), &mut ctx_count, &mut base_contexts);
        }
        if self.model_enabled(MODEL_SPARSE34) {
            let h = Self::hash_func(p3 as u32, Self::hash_func(p2 as u32, 0x651A_833E));
            push(self.hash_lookup(h), &mut ctx_count, &mut base_contexts);
        }
        if self.model_enabled(MODEL_WORDMASK) {
            let mut mask = 0xFFFF_FFFFu32;
            for bit in [10u32, 26, 24, 4, 25, 28, 5, 27, 3, 29] {
                mask ^= 1u32 << bit;
            }
            let h = (self.owhash & mask)
                .wrapping_mul(0xac2b_b7a9)
                .wrapping_add(2_119_543_231);
            push(self.hash_lookup(h), &mut ctx_count, &mut base_contexts);
        }
        if self.model_enabled(MODEL_ORDER2) {
            let off = O2_POS + ((self.owhash & 0xFFFF) as usize) * O0_SIZE;
            push(off, &mut ctx_count, &mut base_contexts);
        }

        let mut h = Self::hash_func(self.owhash & 0xFFFF, 0x4EC4_57CE);
        let mut expected_char = 0u32;
        let mut mm_len = 0usize;
        if self.match_model_order != 0 {
            #[cfg(feature = "trace")]
            if crate::trace::enabled() {
                let hb = self.match_model.get_hash() ^ self.buffer[bpos - 1] as u32;
                eprintln!(
                    "M {} {} {} {} {} {} {}",
                    bpos,
                    self.match_model.get_hash(),
                    self.hash_mask as u32,
                    INPUTS,
                    self.profile as u32,
                    self.hash_table[hb as usize & self.hash_mask],
                    self.match_model.get_length()
                );
            }
            self.match_model.update(&self.buffer);
            mm_len = self.match_model.get_length();
            if mm_len != 0 {
                self.match_model.set_ctx((h & 0xFF) as usize);
                self.match_model.update_cur_mdl();
                expected_char = self.match_model.get_expected_char(&self.buffer);
                self.match_model.update_expected_code(expected_char, 8);
            }
        }

        for order in 3..=self.max_order {
            h = Self::hash_func(self.buffer[bpos.wrapping_sub(order)] as u32, h);
            if self.model_enabled(model_order(order)) {
                let off = self.hash_lookup(h);
                push(off, &mut ctx_count, &mut base_contexts);
            }
        }

        if self.model_enabled(MODEL_WORD1) {
            let off = self.hash_lookup(self.word_model.get_hash());
            push(off, &mut ctx_count, &mut base_contexts);
        }
        if self.model_enabled(MODEL_WORD2) {
            let off = self.hash_lookup(self.word_model.get_prev_hash());
            push(off, &mut ctx_count, &mut base_contexts);
        }
        if self.model_enabled(MODEL_WORD12) {
            let off = self.hash_lookup(self.word_model.get_01_hash());
            push(off, &mut ctx_count, &mut base_contexts);
        }
        if self.model_enabled(MODEL_MASK) {
            self.mask_model = self.mask_model.wrapping_mul(16);
            self.mask_model += self.current_mask_map[p0] as u32;
            let h = Self::hash_func(0xaa0c_d8a7, self.mask_model.wrapping_mul(313));
            let off = self.hash_lookup(h);
            push(off, &mut ctx_count, &mut base_contexts);
        }
        if self.model_enabled(MODEL_MATCHHASHE) {
            let off = self.hash_lookup(self.match_model.get_hash());
            push(off, &mut ctx_count, &mut base_contexts);
        }
        self.match_model.set_hash(h);
        self.sse_ctx = 0;

        self.calc_mixer_base();
        if mm_len > 0 {
            self.miss_len = 0;
            if mm_len > self.min_match_lzp {
                self.cur_preds = LZP_PRED;
                let bit = if DECODE { 0 } else { (expected_char == c) as u32 };
                self.sse_ctx = 256 * (1 + expected_char as usize);
                let bit = self.process_bit::<DECODE, true, R, W>(
                    input,
                    output,
                    bit,
                    &base_contexts,
                    (expected_char ^ 256) as usize,
                    0,
                );
                if bit != 0 {
                    return expected_char;
                }
            }
            self.calc_prob_base();
        } else {
            self.miss_len += 1;
            if self.miss_len > self.miss_fast_path {
                let s0_base = O1_POS + p0 * O0_SIZE;
                let s1_base = O2_POS + ((self.owhash & 0xFFFF) as usize) * O0_SIZE;
                let mut ctx = 1usize;
                let mut ch = c << 24;
                while ctx < 256 {
                    let idx0 = s0_base + ctx;
                    let idx1 = s1_base + ctx;
                    let st0 = self.hash_table[idx0];
                    let st1 = self.hash_table[idx1];
                    let mut p = self.fast_mix[st0 as usize * 256 + st1 as usize].get_p();
                    p += (p == 0) as u32;
                    let bit;
                    if DECODE {
                        bit = self.ent.get_decoded_bit(p, SHIFT);
                        self.ent.normalize(input);
                    } else {
                        bit = ch >> 31;
                        self.ent.encode(output, bit, p, SHIFT);
                        ch <<= 1;
                    }
                    let mi = st0 as usize * 256 + st1 as usize;
                    self.fast_mix[mi].update_rate(bit, 9);
                    self.hash_table[idx0] = self.state_trans[st0 as usize][bit as usize];
                    self.hash_table[idx1] = self.state_trans[st1 as usize][bit as usize];
                    ctx += ctx + bit as usize;
                }
                if DECODE {
                    c = (ctx & 0xFF) as u32;
                }
                return c;
            }
        }

        let n1 = self.process_nibble::<DECODE, R, W>(
            input,
            output,
            c >> 4,
            &base_contexts,
            0,
            0,
        );
        self.match_model.fetch(n1 << 4);
        let n2 = self.process_nibble::<DECODE, R, W>(
            input,
            output,
            c & 0xF,
            &base_contexts,
            15 + (n1 as usize) * 15,
            0,
        );
        if DECODE {
            c = n2 | (n1 << 4);
        }
        c
    }

    pub fn compress<R: ByteRead + ?Sized, W: ByteWrite + ?Sized>(
        &mut self,
        input: &mut R,
        output: &mut W,
        max_count: u64,
    ) {
        assert!(self.force_profile, "detector path not implemented yet");
        self.init();
        self.ent.init();
        let mut count = max_count;
        while count > 0 {
            let c = input.get();
            self.process_byte::<false, R, W>(input, output, c as u32);
            self.update(c as u32);
            count -= 1;
        }
        self.ent.flush(output);
    }

// `CM::decompress`.
    pub fn decompress<R: ByteRead + ?Sized, W: ByteWrite + ?Sized>(
        &mut self,
        input: &mut R,
        output: &mut W,
        max_count: u64,
    ) {
        assert!(self.force_profile, "detector path not implemented yet");
        self.init();
        self.ent.init_decoder(input);
        for _ in 0..max_count {
            let c = self.process_byte::<true, R, W>(input, output, 0);
            self.update(c);
            output.put(c as u8);
        }
    }
}

pub type CmTurbo = Cm<3, 5>;
pub type CmFast = Cm<4, 6>;
pub type CmMid = Cm<6, 8>;
pub type CmHigh = Cm<8, 10>;
pub type CmMax = Cm<10, 12>;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stream::{SliceReader, VecWriter};

    fn roundtrip(mem: u32, data: &[u8]) {
        let mut cm = CmTurbo::new(mem, true, Some(Profile::Text));
        cm.set_opt(0);
        let mut out = VecWriter::new();
        let mut r = SliceReader::new(data);
        cm.compress(&mut r, &mut out, data.len() as u64);
        let packed = out.into_inner();

        let mut cm2 = CmTurbo::new(mem, true, Some(Profile::Text));
        cm2.set_opt(0);
        let mut r2 = SliceReader::new(&packed);
        let mut dec = VecWriter::new();
        cm2.decompress(&mut r2, &mut dec, data.len() as u64);
        assert_eq!(dec.as_slice(), data);
    }

    #[test]
    fn turbo_roundtrips_small_text() {
        roundtrip(1, b"the quick brown fox jumps over the lazy dog. the quick brown fox...");
    }

    #[test]
    fn turbo_roundtrips_empty() {
        roundtrip(1, b"");
    }

    #[test]
    fn turbo_roundtrips_repetitive() {
        let mut data = Vec::new();
        for i in 0..4096 {
            data.push(b"abcdefghij"[i % 10]);
        }
        roundtrip(1, &data);
    }
}
