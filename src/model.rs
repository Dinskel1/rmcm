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

//! Bit probability models, ported from `Model.hpp`.

// never exceeds `2^15 - 1`.
#[derive(Clone, Copy, Debug)]
pub struct SafeBitModel<const SHIFT: u32 = 12, const LEARN: u32 = 5, const BITS: u32 = 15> {
    p: u16,
}

impl<const SHIFT: u32, const LEARN: u32, const BITS: u32> Default
    for SafeBitModel<SHIFT, LEARN, BITS>
{
    fn default() -> Self {
        Self::new()
    }
}

impl<const SHIFT: u32, const LEARN: u32, const BITS: u32> SafeBitModel<SHIFT, LEARN, BITS> {
    pub const PMAX: u32 = (1 << BITS) - 1;
    pub const SHIFT_VALUE: u32 = SHIFT;

    pub fn new() -> Self {
        Self {
            p: (Self::PMAX / 2) as u16,
        }
    }

    pub fn init(&mut self) {
        self.p = (Self::PMAX / 2) as u16;
    }

    #[inline]
    pub fn update(&mut self, bit: u32) {
        let round = 1i32 << (LEARN - 1);
        let p = self.p as i32;
        let delta = (((bit as i32) << BITS) - p + round) >> LEARN;
        self.p = p.wrapping_add(delta) as u16;
    }

    #[inline]
    pub fn get_p(&self) -> u32 {
        let mut ret = (self.p as u32) >> (BITS - SHIFT);
        ret += u32::from(ret == 0);
        ret
    }

    #[inline]
    pub fn raw(&self) -> u16 {
        self.p
    }
}

#[derive(Clone, Copy, Debug)]
pub struct FastBitModel<const SHIFT: u32 = 12, const LEARN: u32 = 5, const BITS: u32 = 15> {
    p: i32,
}

impl<const SHIFT: u32, const LEARN: u32, const BITS: u32> Default
    for FastBitModel<SHIFT, LEARN, BITS>
{
    fn default() -> Self {
        Self::new()
    }
}

impl<const SHIFT: u32, const LEARN: u32, const BITS: u32> FastBitModel<SHIFT, LEARN, BITS> {
// future experiment can flip it in one place.
    pub const USE_ROUNDING: bool = false;

    pub fn new() -> Self {
        Self {
            p: (1i32 << (SHIFT - 1)) << (BITS - SHIFT),
        }
    }

    pub fn init(&mut self, new_p: u32) {
        self.p = (new_p as i32) << (BITS - SHIFT);
    }

    #[inline]
    pub fn update(&mut self, bit: u32) {
        self.update_rate(bit, LEARN);
    }

    #[inline]
    pub fn update_rate(&mut self, bit: u32, learn_rate: u32) {
        let round = if Self::USE_ROUNDING {
            1i32 << (learn_rate - 1)
        } else {
            0
        };
        let delta = (((bit as i32) << BITS) - self.p + round) >> learn_rate;
        self.p = self.p.wrapping_add(delta);
    }

    #[inline]
    pub fn set_p(&mut self, new_p: u32) {
        self.p = (new_p as i32) << (BITS - SHIFT);
    }

    #[inline]
    pub fn get_p(&self) -> u32 {
        (self.p >> (BITS - SHIFT)) as u32
    }

    #[inline]
    pub fn raw(&self) -> i32 {
        self.p
    }
}

// Layout of `p` (32 bits):
// ```text
// ```
#[derive(Clone, Copy, Debug)]
pub struct BitLearnModel<const SHIFT: u32 = 12, const LEARN: u32 = 8, const BITS: u32 = 30> {
    p: u32,
}

impl<const SHIFT: u32, const LEARN: u32, const BITS: u32> Default
    for BitLearnModel<SHIFT, LEARN, BITS>
{
    fn default() -> Self {
        Self::new()
    }
}

impl<const SHIFT: u32, const LEARN: u32, const BITS: u32> BitLearnModel<SHIFT, LEARN, BITS> {
    pub const COUNT_BITS: u32 = 8;
    pub const COUNT_MASK: u32 = (1 << Self::COUNT_BITS) - 1;
    pub const INITIAL_COUNT: u32 = 2;

    pub fn new() -> Self {
        let mut m = Self { p: 0 };
        m.init(1 << (SHIFT - 1));
        m
    }

    pub fn init(&mut self, new_p: u32) {
        self.set_p(new_p, Self::INITIAL_COUNT << 5);
    }

    #[inline]
    pub fn set_p(&mut self, new_p: u32, count: u32) {
        self.p = (new_p << (31 - SHIFT)) | count;
    }

    #[inline]
    pub fn update(&mut self, bit: u32) {
        let count = self.p & Self::COUNT_MASK;
        let learn_rate = 2 + (count >> 5);
        let m: i32 = if bit == 0 {
            Self::COUNT_MASK as i32
        } else {
            i32::MAX
        };
        let masked = (m.wrapping_sub(self.p as i32) >> learn_rate) & !(Self::COUNT_MASK as i32);
        self.p = self.p.wrapping_add(masked as u32);
        if count < Self::COUNT_MASK {
            self.p = self.p.wrapping_add(1);
        }
    }

    #[inline]
    pub fn get_p(&self) -> u32 {
        self.p >> (31 - SHIFT)
    }

    #[inline]
    pub fn get_count(&self) -> u32 {
        self.p & Self::COUNT_MASK
    }

    #[inline]
    pub fn raw(&self) -> u32 {
        self.p
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct FastCountModel {
    n: [u32; 2],
}

impl FastCountModel {
    pub fn new() -> Self {
        Self { n: [0, 0] }
    }

    pub fn init(&mut self) {
        self.n = [0, 0];
    }

    #[inline]
    pub fn update(&mut self, bit: u32) {
        let bit = (bit & 1) as usize;
        if self.n[bit] < 0xFF {
            self.n[bit] += 1;
        }
        let other = self.n[1 ^ bit];
        self.n[1 ^ bit] = other / 2 + u32::from(other != 0);
    }

    #[inline]
    pub fn get_p(&self) -> u32 {
        let a = self.n[0];
        let b = self.n[1];
        if a == 0 && b == 0 {
            return 1 << 11;
        }
        if a == 0 {
            return 0;
        }
        if b == 0 {
            return (1 << 12) - 1;
        }
        (a << 12) / (a + b)
    }

    #[inline]
    pub fn get_n(&self, i: usize) -> u32 {
        self.n[i]
    }

    #[inline]
    pub fn get_total(&self) -> u32 {
        self.n[0] + self.n[1]
    }
}

pub const fn bit_size(n: u32) -> u32 {
    let mut bits = 0;
    while (1u32 << bits) < n {
        bits += 1;
    }
    bits
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn safe_model_stays_in_range() {
        let mut m = SafeBitModel::<12, 5, 15>::new();
        for i in 0..100_000u32 {
            let bit = (i.wrapping_mul(2654435761) >> 19) & 1;
            m.update(bit);
            assert!(m.raw() < 1 << 15);
            let p = m.get_p();
            assert!((1..4096).contains(&p), "p={p}");
        }
    }

    #[test]
    fn safe_model_tracks_a_biased_source() {
        let mut m = SafeBitModel::<12, 5, 15>::new();
        for _ in 0..10_000 {
            m.update(1);
        }
        assert!(m.get_p() > 4000);
        for _ in 0..10_000 {
            m.update(0);
        }
        assert!(m.get_p() < 100);
    }

    #[test]
    fn fast_model_init_matches_shift() {
        let mut m = FastBitModel::<12, 9, 30>::new();
        assert_eq!(m.get_p(), 2048);
        m.set_p(1);
        assert_eq!(m.get_p(), 1);
        m.set_p(4095);
        assert_eq!(m.get_p(), 4095);
    }

    #[test]
    fn fast_model_is_monotone_in_bit() {
        let mut up = FastBitModel::<12, 9, 30>::new();
        let mut down = FastBitModel::<12, 9, 30>::new();
        for _ in 0..50_000 {
            up.update(1);
            down.update(0);
        }
        assert!(up.get_p() > 4090);
        assert!(down.get_p() < 6);
    }

    #[test]
    fn learn_model_count_grows_and_saturates() {
        let mut m = BitLearnModel::<12, 8, 30>::new();
        assert_eq!(m.get_count(), 64);
        for _ in 0..1000 {
            m.update(1);
        }
        assert_eq!(m.get_count(), 255);
        assert!(m.get_p() > 4000);
    }

    #[test]
    fn learn_model_initial_probability_is_half() {
        let m = BitLearnModel::<12, 8, 30>::new();
        assert_eq!(m.get_p(), 2048);
    }

    #[test]
    fn count_model_probabilities() {
        let mut m = FastCountModel::new();
        assert_eq!(m.get_p(), 1 << 11);
        m.update(1);
        assert_eq!(m.get_n(1), 1);
        assert_eq!(m.get_p(), 0);
        m.update(0);
        assert_eq!(m.get_n(0), 1);
        assert_eq!(m.get_p(), 2048);
    }

    #[test]
    fn bit_size_matches_expectations() {
        assert_eq!(bit_size(1), 0);
        assert_eq!(bit_size(2), 1);
        assert_eq!(bit_size(256), 8);
        assert_eq!(bit_size(257), 9);
    }
}
