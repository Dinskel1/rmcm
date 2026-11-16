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

//! Logistic mixing ("neural network" mixer), ported from `Mixer.hpp`.

// `Mixer<int, N - 1, FP_SHIFT, WSHIFT>`.
#[derive(Clone, Debug)]
pub struct Mixer<const N: usize, const FP_SHIFT: u32 = 17, const WSHIFT: u32 = 11> {
    w: [i32; N],
    learn: i32,
}

impl<const N: usize, const FP_SHIFT: u32, const WSHIFT: u32> Default for Mixer<N, FP_SHIFT, WSHIFT> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const N: usize, const FP_SHIFT: u32, const WSHIFT: u32> Mixer<N, FP_SHIFT, WSHIFT> {
// `kUseRound = true` in the C++ class.
    pub const USE_ROUND: bool = true;
    pub const ROUND: i32 = if Self::USE_ROUND {
        1i32 << (FP_SHIFT - 1)
    } else {
        0
    };
    pub const SKEW_LEARN: u32 = 9;

    pub const fn weights() -> usize {
        N - 1
    }

    pub fn new() -> Self {
        let mut m = Self {
            w: [0; N],
            learn: 0,
        };
        m.init(366);
        m
    }

    pub fn init(&mut self, learn_rate: i32) {
        let weights = Self::weights();
        if weights != 0 {
            let wdiv = weights as i32;
            let init = (1i32 << FP_SHIFT) / wdiv;
            for slot in self.w[..weights].iter_mut() {
                *slot = init;
            }
        }
        self.w[N - 1] = 0;
        self.learn = learn_rate;
    }

    #[inline]
    pub fn get_learn(&self) -> i32 {
        self.learn
    }

    #[inline]
    pub fn weight(&self, index: usize) -> i32 {
        self.w[index]
    }

    #[inline]
    pub fn weights_slice(&self) -> &[i32] {
        &self.w
    }

    #[inline]
    pub fn p(&self, probs: &[i32]) -> i32 {
        let mut ptotal: i32 = 0;
        for i in 0..Self::weights() {
            ptotal = ptotal.wrapping_add(probs[i].wrapping_mul(self.w[i]));
        }
        ptotal = ptotal.wrapping_add(self.w[N - 1] << WSHIFT);
        ptotal >> FP_SHIFT
    }

    #[inline]
    pub fn p_fast(&self, shift: u32, probs: &[i32; 10]) -> i32 {
        let mut ptotal: i32 = 0;
        for i in 0..Self::weights().min(10) {
            ptotal = ptotal.wrapping_add(probs[i].wrapping_mul(self.w[i]));
        }
        ptotal = ptotal.wrapping_add(self.w[N - 1] << shift);
        ptotal >> FP_SHIFT
    }

    #[inline]
    pub fn update(&mut self, probs: &[i32], pr: i32, bit: u32, pshift: u32, limit: i32) {
        let err = ((bit << pshift) as i32)
            .wrapping_sub(pr)
            .wrapping_mul(self.learn);
        let round = Self::ROUND;
        for i in 0..Self::weights() {
            let delta = probs[i].wrapping_mul(err).wrapping_add(round) >> FP_SHIFT;
            self.w[i] = self.w[i].wrapping_add(delta);
        }
        let sq_round = 1i32 << (Self::SKEW_LEARN - 1);
        self.w[N - 1] = self.w[N - 1].wrapping_add((err.wrapping_add(sq_round)) >> Self::SKEW_LEARN);
        if self.learn > limit {
            self.learn -= 1;
        }
    }

// the format, not just an optimisation.
    #[inline]
    pub fn update_fast(
        &mut self,
        pr: i32,
        bit: u32,
        pshift: u32,
        limit: i32,
        _delta: i32,
        probs: &[i32; 10],
    ) -> bool {
        let err = ((bit << pshift) as i32)
            .wrapping_sub(pr)
            .wrapping_mul(self.learn);
        let delta_round = Self::ROUND >> (pshift - 3);
        let ret = err < -delta_round || err > delta_round;
        if ret {
            let round = Self::ROUND;
            for i in 0..Self::weights().min(10) {
                let delta = probs[i].wrapping_mul(err).wrapping_add(round) >> FP_SHIFT;
                self.w[i] = self.w[i].wrapping_add(delta);
            }
            let sq_round = 1i32 << (Self::SKEW_LEARN - 1);
            self.w[N - 1] =
                self.w[N - 1].wrapping_add((err.wrapping_add(sq_round)) >> Self::SKEW_LEARN);
            if self.learn > limit {
                self.learn -= 1;
            }
        }
        ret
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initial_weights_are_balanced() {
        let m = Mixer::<10, 17, 11>::new();
        assert_eq!(m.get_learn(), 366);
// (1 << 17) / 9 == 14563
        for i in 0..9 {
            assert_eq!(m.weight(i), 14563, "weight {i}");
        }
        assert_eq!(m.weight(9), 0, "skew must start at zero");
    }

    #[test]
    fn init_sets_custom_learn_rate() {
        let mut m = Mixer::<10, 17, 11>::new();
        m.init(382);
        assert_eq!(m.get_learn(), 382);
    }

    #[test]
    fn untouched_mixer_predicts_mean() {
        let m = Mixer::<10, 17, 11>::new();
        let probs = [-2048i32; 10];
// 9 * 14563 * -2048 >> 17 ~= -2048
        let p = m.p_fast(9, &probs);
        assert!((p + 2048).abs() <= 3, "p={p}");
    }

    #[test]
    fn learn_rate_decays_toward_limit() {
        let mut m = Mixer::<10, 17, 11>::new();
        let probs = [1000i32; 10];
        for _ in 0..500 {
            m.update_fast(0, 1, 12, 28, 1, &probs);
        }
        assert_eq!(m.get_learn(), 28);
    }

    #[test]
    fn small_errors_are_ignored() {
        let mut m = Mixer::<10, 17, 11>::new();
        let probs = [0i32; 10];
        m.init(28);
        let mut perfect = Mixer::<10, 17, 11>::new();
        perfect.init(28);
        let before = perfect.weight(0);
// pr == 0 and bit == 0 -> err == 0 -> skipped
        assert!(!perfect.update_fast(0, 0, 12, 28, 1, &probs));
        assert_eq!(perfect.weight(0), before);
        assert!(m.update_fast(0, 1, 12, 28, 1, &probs));
    }
}
