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


use crate::generated::STATE_TABLE;

pub const NUM_STATES: usize = 256;
pub const P_BITS: u32 = 12;
pub const PMAX: u32 = 1 << P_BITS;

#[derive(Clone)]
pub struct NsStateMap {
    t: [[u8; 2]; NUM_STATES],
}

impl Default for NsStateMap {
    fn default() -> Self {
        Self::build()
    }
}

impl NsStateMap {
    pub fn build() -> Self {
        let mut m = Self { t: STATE_TABLE };
        m.optimize_states();
        m
    }

    pub fn unoptimized() -> Self {
        Self { t: STATE_TABLE }
    }

    #[inline]
    pub fn p(&self, _state: u8) -> u32 {
        PMAX / 2
    }

// `getTransition(state, bit)`.
    #[inline]
    pub fn get_transition(&self, state: u8, bit: u32) -> u8 {
        self.t[state as usize][(bit & 1) as usize]
    }

    #[inline]
    pub fn next(&self, state: u8, bit: u32) -> u8 {
        self.get_transition(state, bit)
    }

    #[inline]
    pub fn transitions(&self, state: u8) -> [u8; 2] {
        self.t[state as usize]
    }

    pub fn table(&self) -> &[[u8; 2]; NUM_STATES] {
        &self.t
    }

// and unreachable ones are folded into state 0.
// differ from a "clean" renumbering:
// every reachable state is offset by one.
    pub fn optimize_states(&mut self) {
        let mut new_states: Vec<[u8; 2]> = Vec::with_capacity(NUM_STATES);
        new_states.push([1, 1]);

        let mut map: [i64; NUM_STATES] = [-1; NUM_STATES];
        self.optimize_states_rec(0, &mut map, &mut new_states);

// transitions cannot survive the copy below.
        self.t = [[0u8; 2]; NUM_STATES];
        for (i, s) in new_states.iter().enumerate() {
            if i < NUM_STATES {
                self.t[i] = *s;
            }
        }

        self.t[0][0] = self.t[1][0];
        self.t[0][1] = self.t[1][1];
    }

    fn optimize_states_rec(
        &mut self,
        state: u8,
        map: &mut [i64; NUM_STATES],
        new_states: &mut Vec<[u8; 2]>,
    ) {
        if map[state as usize] >= 0 {
            return;
        }
        let idx = new_states.len() as i64;
        map[state as usize] = idx;
        new_states.push([0, 0]);

        let mut out = [0u8; 2];
        for (bit, slot) in out.iter_mut().enumerate() {
            let child = self.t[state as usize][bit];
            self.optimize_states_rec(child, map, new_states);
            *slot = map[child as usize] as u8;
        }
        new_states[idx as usize] = out;
    }

}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn optimized_state_zero_equals_state_one() {
        let m = NsStateMap::build();
        assert_eq!(m.transitions(0), m.transitions(1));
    }

    #[test]
    fn transitions_stay_in_range() {
        let m = NsStateMap::build();
        for s in 0..NUM_STATES as u8 {
            for bit in 0..2 {
                assert!((m.get_transition(s, bit) as usize) < NUM_STATES);
            }
        }
    }

    #[test]
    fn unreachable_states_are_gone() {
        let m = NsStateMap::build();
// Everything the machine can reach must be reachable within 255 steps
// from state 0 via the transition graph.
        let mut seen = [false; NUM_STATES];
        let mut stack = vec![0u8];
        while let Some(s) = stack.pop() {
            if seen[s as usize] {
                continue;
            }
            seen[s as usize] = true;
            stack.push(m.t[s as usize][0]);
            stack.push(m.t[s as usize][1]);
        }
// leaves the tail zeroed.
        let count = seen.iter().filter(|x| **x).count();
        for s in count..NUM_STATES {
            assert_eq!(m.t[s], [0, 0], "state {s} should be unused");
        }
    }

    #[test]
    fn prediction_is_constant() {
        let m = NsStateMap::build();
        assert_eq!(m.p(0), 2048);
        assert_eq!(m.p(255), 2048);
    }

    #[test]
    fn bit_zero_and_one_use_separate_transitions() {
        let m = NsStateMap::unoptimized();
        let differing = (0..NUM_STATES)
            .filter(|&s| m.t[s][0] != m.t[s][1])
            .count();
        assert!(differing > 100, "table looks wrong: {differing}");
    }
}
