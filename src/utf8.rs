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

//! UTF-8 decoder, ported from `UTF8.hpp`.

// `UTF8Decoder<error_check>`
#[derive(Clone, Copy, Debug, Default)]
pub struct Utf8Decoder<const ERROR_CHECK: bool = false> {
// Continuation bytes still expected.
    extra: u32,
// Code point accumulated so far.
    acc: u32,
    error: bool,
}

impl<const ERROR_CHECK: bool> Utf8Decoder<ERROR_CHECK> {
    pub fn new() -> Self {
        Self {
            extra: 0,
            acc: 0,
            error: false,
        }
    }

    pub fn init(&mut self) {
        self.extra = 0;
        self.acc = 0;
        self.error = false;
    }

    #[inline]
    pub fn done(&self) -> bool {
        self.extra == 0
    }

    #[inline]
    pub fn acc(&self) -> u32 {
        self.acc
    }

    #[inline]
    pub fn error(&self) -> bool {
        self.error
    }

    pub fn clear_err(&mut self) {
        self.error = false;
    }

// once [`Self::done`] becomes true.
    pub fn update(&mut self, c: u32) {
        if self.extra != 0 {
            self.acc = (self.acc << 6) | (c & 0x3F);
            self.extra -= 1;
            if ERROR_CHECK && (c >> 6) != 2 {
                self.error = true;
            }
        } else if c & 0x80 == 0 {
            self.acc = c & 0x7F;
        } else if c & 0xFE == 0xFC {
            self.acc = c & 0x1;
            self.extra = 5;
        } else if c & 0xFC == 0xF8 {
            self.acc = c & 0x3;
            self.extra = 4;
        } else if c & 0xF8 == 0xF0 {
            self.acc = c & 0x7;
            self.extra = 3;
        } else if c & 0xF0 == 0xE0 {
            self.acc = c & 0xF;
            self.extra = 2;
        } else if c & 0xE0 == 0xC0 {
            self.acc = c & 0x1F;
            self.extra = 1;
        } else {
            self.acc = c;
            if ERROR_CHECK {
                self.error = true;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ascii_is_immediate() {
        let mut d = Utf8Decoder::<false>::new();
        d.update(b'a' as u32);
        assert!(d.done());
        assert_eq!(d.acc(), 'a' as u32);
    }

    #[test]
    fn two_byte_sequence() {
        let mut d = Utf8Decoder::<false>::new();
        d.update(0xC3);
        assert!(!d.done());
        d.update(0xA9);
        assert!(d.done());
        assert_eq!(d.acc(), 0xE9);
    }

    #[test]
    fn four_byte_sequence() {
        let mut d = Utf8Decoder::<false>::new();
        for b in [0xF0u32, 0x9F, 0x98, 0x80] {
            d.update(b);
        }
        assert!(d.done());
        assert_eq!(d.acc(), 0x1F600);
    }

    #[test]
    fn error_check_rejects_bad_continuation() {
        let mut ok = Utf8Decoder::<true>::new();
        ok.update(0xC3);
        ok.update(0xA9);
        assert!(!ok.error());

        let mut bad = Utf8Decoder::<true>::new();
        bad.update(0xC3);
        bad.update(0x41); // not a continuation byte
        assert!(bad.error());
    }
}
