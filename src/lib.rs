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

//! `rmcm` -- a Rust reimplementation of the MCM v0.83 compressor.

pub mod archive;
pub mod buffer;
pub mod cm;
pub mod delta;
pub mod detector;
pub mod dict;
pub mod filter;
pub mod format;
pub mod generated;
pub mod log_table;
pub mod match_model;
pub mod mixer;
pub mod model;
pub mod range;
pub mod sse;
pub mod statemap;
pub mod utf8;
pub mod stream;
#[cfg(feature = "trace")]
pub mod trace;
pub mod wav16;
pub mod word_model;
pub mod x86;

// Version string reported by the CLI.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
