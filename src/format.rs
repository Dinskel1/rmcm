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

//! what the reference prints.

pub fn format_number(n: u64) -> String {
    let mut ret = String::new();
    let mut n = n;
    while n >= 1000 {
        ret = format!(",{:03}{}", n % 1000, ret);
        n /= 1000;
    }
    format!("{n}{ret}")
}

pub fn pretty_size(size: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;
    let (units, name) = if size >= GB {
        (GB, "GB")
    } else if size >= MB {
        (MB, "MB")
    } else if size >= KB {
        (KB, "KB")
    } else {
        (1, "B")
    };
    let value = (size as f64 / units as f64) as f32;
    format!("{}{}", fmt_g(value as f64, 6), name)
}

pub fn fmt_g(x: f64, precision: usize) -> String {
    if x == 0.0 {
        return "0".to_string();
    }
    if !x.is_finite() {
        return if x.is_nan() {
            "nan".to_string()
        } else if x > 0.0 {
            "inf".to_string()
        } else {
            "-inf".to_string()
        };
    }
    let precision = precision.max(1);
    let sci = format!("{:.*e}", precision - 1, x);
    let (mantissa, exp) = sci.split_once('e').unwrap_or((&sci, "0"));
    let exp: i32 = exp.parse().unwrap_or(0);
    if exp < -4 || exp >= precision as i32 {
        let mantissa = trim_zeros(mantissa);
        let sign = if exp < 0 { '-' } else { '+' };
        return format!("{mantissa}e{sign}{:02}", exp.abs());
    }
    let decimals = (precision as i32 - 1 - exp).max(0) as usize;
    let fixed = format!("{:.*}", decimals, x);
    trim_zeros(&fixed).to_string()
}

fn trim_zeros(s: &str) -> &str {
    if s.contains('.') {
        let s = s.trim_end_matches('0');
        s.trim_end_matches('.')
    } else {
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers() {
        assert_eq!(format_number(0), "0");
        assert_eq!(format_number(999), "999");
        assert_eq!(format_number(1000), "1,000");
        assert_eq!(format_number(6593996), "6,593,996");
    }

    #[test]
    fn sizes() {
        assert_eq!(pretty_size(975), "975B");
        assert_eq!(pretty_size(22710), "22.1777KB");
        assert_eq!(pretty_size(1024), "1KB");
        assert_eq!(pretty_size(3 * 1024 * 1024), "3MB");
    }

    #[test]
    fn g_format() {
        assert_eq!(fmt_g(0.0, 3), "0");
        assert_eq!(fmt_g(1.352, 3), "1.35");
        assert_eq!(fmt_g(0.0538, 3), "0.0538");
        assert_eq!(fmt_g(8.8e-5, 6), "8.8e-05");
        assert_eq!(fmt_g(1.0632, 3), "1.06");
        assert_eq!(fmt_g(1000000.0, 6), "1e+06");
    }
}
