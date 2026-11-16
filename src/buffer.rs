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

//! Cyclic byte buffer, ported from `CyclicBuffer.hpp`.

#[derive(Clone)]
pub struct CyclicBuffer {
    storage: Vec<u8>,
    data_offset: usize,
    mask: usize,
    size: usize,
    pos: usize,
}

impl CyclicBuffer {
// `resize(new_size, padding)`; `new_size` must be a power of two.
    pub fn new(new_size: usize, padding: usize) -> Self {
        assert!(new_size.is_power_of_two(), "buffer size must be a power of 2");
        let alloc = new_size + padding * 2;
        Self {
            storage: vec![0u8; alloc],
            data_offset: padding,
            mask: new_size - 1,
            size: new_size,
            pos: 0,
        }
    }

    #[inline]
    pub fn get_mask(&self) -> usize {
        self.mask
    }

    #[inline]
    pub fn get_size(&self) -> usize {
        self.size
    }

    #[inline]
    pub fn read_u32_at(&self, offset: usize) -> u32 {
        let start = self.data_offset + (offset & self.mask);
        u32::from_le_bytes([
            self.storage[start],
            self.storage[start + 1],
            self.storage[start + 2],
            self.storage[start + 3],
        ])
    }

    pub fn get_pos(&self) -> usize {
        self.pos
    }

    #[inline]
    pub fn push(&mut self, b: u8) {
        self.storage[self.data_offset + (self.pos & self.mask)] = b;
        self.pos = self.pos.wrapping_add(1);
    }

    #[inline]
    pub fn get(&self, index: usize) -> u8 {
        self.storage[self.data_offset + (index & self.mask)]
    }

    #[inline]
    pub fn set(&mut self, index: usize, value: u8) {
        self.storage[self.data_offset + (index & self.mask)] = value;
    }

// zero-initialised allocation).
    pub fn zero(&mut self) {
        self.storage.iter_mut().for_each(|b| *b = 0);
    }
}

impl std::ops::Index<usize> for CyclicBuffer {
    type Output = u8;
    #[inline]
    fn index(&self, index: usize) -> &u8 {
        &self.storage[self.data_offset + (index & self.mask)]
    }
}

impl std::ops::IndexMut<usize> for CyclicBuffer {
    #[inline]
    fn index_mut(&mut self, index: usize) -> &mut u8 {
        &mut self.storage[self.data_offset + (index & self.mask)]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wraps_and_preserves_history_window() {
        let mut b = CyclicBuffer::new(16, 4);
        for i in 0..40u8 {
            b.push(i);
        }
        assert_eq!(b.get_pos(), 40);
        for k in 1..=16usize {
            assert_eq!(b[40 - k], (40 - k) as u8);
        }
    }

    #[test]
    fn initial_content_is_zero() {
        let b = CyclicBuffer::new(8, 4);
        assert_eq!(b[0], 0);
        assert_eq!(b[7], 0);
    }
}
