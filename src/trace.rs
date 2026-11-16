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


use std::sync::Mutex;

pub type Line = [i64; 16];

static TRACE: Mutex<Vec<Line>> = Mutex::new(Vec::new());
static ENABLED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub fn enable() {
    ENABLED.store(true, std::sync::atomic::Ordering::Relaxed);
}

pub fn enabled() -> bool {
    ENABLED.load(std::sync::atomic::Ordering::Relaxed)
}

pub fn record_full(
    p: u32,
    bit: u32,
    ctx: usize,
    stp: u32,
    mixer_p: u32,
    p0: i32,
    p1: i32,
    p2: i32,
    mixer_ctx: usize,
    sse_ctx: usize,
    s1: u8,
    s2: u8,
    preds_idx: usize,
    m1: i64,
    m2: i64,
    mm_l: usize,
) {
    if let Ok(mut v) = TRACE.lock() {
        v.push([
            p as i64,
            bit as i64,
            ctx as i64,
            stp as i64,
            mixer_p as i64,
            p0 as i64,
            p1 as i64,
            p2 as i64,
            mixer_ctx as i64,
            sse_ctx as i64,
            s1 as i64,
            s2 as i64,
            preds_idx as i64,
            m1,
            m2,
            mm_l as i64,
        ]);
    }
}

pub fn take() -> Vec<Line> {
    std::mem::take(&mut *TRACE.lock().unwrap())
}

pub fn len() -> usize {
    TRACE.lock().unwrap().len()
}
