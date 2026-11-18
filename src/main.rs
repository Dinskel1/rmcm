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

//! Command line front end, a transcription of `MCM.cpp`.

use rmcm::archive::{Archive, CompLevel, CompressionOptions, FilterType, LzpType};
use rmcm::format::format_number;
use std::process::ExitCode;
use std::time::Instant;

const ERRSTR: &str = "UNIMPLEMENTED";

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Unknown,
    Compress,
    Decompress,
    Add,
    Extract,
    ExtractAll,
    SingleTest,
    MemTest,
    Opt,
}

struct Options {
    mode: Mode,
    options: CompressionOptions,
    archive_file: String,
    files: Vec<String>,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            mode: Mode::Unknown,
            options: CompressionOptions::default(),
            archive_file: String::new(),
            files: Vec::new(),
        }
    }
}

fn print_header() {
    println!("======================================================================");
    println!("rmcm 0.8.3, a Rust port of mcm by Mathieu Chartier (c)2015 Google Inc.");
    println!("Experimental, may contain bugs. Rust port by Dinskel1 (github.com/Dinskel1)");
    println!("Special thanks to: Matt Mahoney, Stephan Busch, Christopher Mattern.");
    println!("======================================================================");
}

fn trim_ext(path: &str) -> String {
    match path.rfind(['\\', '/']) {
        Some(pos) => path[pos + 1..].to_string(),
        None => path.to_string(),
    }
}

fn level_name(level: CompLevel) -> &'static str {
    match level {
        CompLevel::Store => "store",
        CompLevel::Turbo => "turbo",
        CompLevel::Fast => "fast",
        CompLevel::Mid => "mid",
        CompLevel::High => "high",
        CompLevel::Max => "max",
    }
}

fn usage(name: &str) {
    print_header();
    println!("Caution: Experimental, use only for testing!");
    println!("Usage: {name} [command] [options] <infile> <outfile>");
    println!("Options: d for decompress");
    println!("-{{t|f|m|h|x}}{{1 .. 11}} compression option");
    println!(
        "t is turbo, f is fast, m is mid, h is high, x is max (default {})",
        level_name(CompLevel::Mid)
    );
    println!("0 .. 11 specifies memory with 32mb .. 5gb per thread (default 6)");
    println!("10 and 11 are only supported on 64 bits");
    println!("-test tests the file after compression is done");
    println!("Examples:");
    println!("Compress: {name} -m9 enwik8 enwik8.mcm");
    println!("Decompress: {name} d enwik8.mcm enwik8.ref");
}

fn main() -> ExitCode {
    let argv: Vec<String> = std::env::args().collect();
    let program = trim_ext(argv.first().map(String::as_str).unwrap_or("rmcm"));
    let mut opts = Options::default();
    match parse(&argv, &program, &mut opts) {
        Ok(()) => {}
        Err(code) => {
            if code != 0 {
                eprintln!("Failed to parse arguments");
            }
            return ExitCode::from(code);
        }
    }
    match opts.mode {
        Mode::Add | Mode::Extract | Mode::ExtractAll => ExitCode::SUCCESS,
        Mode::Compress | Mode::SingleTest => compress(&opts),
        Mode::Decompress => decompress(&opts),
        Mode::MemTest | Mode::Opt => {
            print_header();
            eprintln!("mode not implemented");
            ExitCode::FAILURE
        }
        Mode::Unknown => ExitCode::SUCCESS,
    }
}

fn parse(argv: &[String], program: &str, opts: &mut Options) -> Result<(), u8> {
    let mut has_comp_args = false;
    let mut block_size: u64 = 0;
    let mut i = 1usize;
    while i < argv.len() {
        let arg = argv[i].as_str();
        let mut parsed_mode = Mode::Unknown;
        match arg {
            "-test" => parsed_mode = Mode::SingleTest,
            "-memtest" => parsed_mode = Mode::MemTest,
            "-opt" => parsed_mode = Mode::Opt,
            "-stest" => parsed_mode = Mode::SingleTest,
            "c" => parsed_mode = Mode::Compress,
            "d" => parsed_mode = Mode::Decompress,
            "a" => parsed_mode = Mode::Add,
            "e" => parsed_mode = Mode::Extract,
            "x" => parsed_mode = Mode::ExtractAll,
            _ => {}
        }
        if parsed_mode != Mode::Unknown {
            if opts.mode != Mode::Unknown {
                eprintln!("Multiple commands specified");
                return Err(2);
            }
            opts.mode = parsed_mode;
            if matches!(parsed_mode, Mode::Add | Mode::Extract | Mode::ExtractAll) {
                i += 1;
                if i >= argv.len() {
                    eprintln!("Expected archive");
                    return Err(3);
                }
                opts.archive_file = argv[i].clone();
            }
        } else if arg == "-filter=none" {
            opts.options.filter_type = FilterType::None;
        } else if arg == "-filter=dict" {
            opts.options.filter_type = FilterType::Dict;
        } else if arg == "-filter=x86" {
            opts.options.filter_type = FilterType::X86;
        } else if arg == "-filter=auto" {
            opts.options.filter_type = FilterType::Auto;
        } else if arg == "-lzp=auto" {
            opts.options.lzp_type = LzpType::Auto;
        } else if arg == "-lzp=true" {
            opts.options.lzp_type = LzpType::Enable;
        } else if arg == "-lzp=false" {
            opts.options.lzp_type = LzpType::Disable;
        } else if arg == "-b" {
            if i + 1 >= argv.len() {
                usage(program);
                return Err(0);
            }
            i += 1;
            match argv[i].parse::<u64>() {
                Ok(v) => block_size = v * (1024 * 1024),
                Err(_) => {
                    usage(program);
                    return Err(0);
                }
            }
        } else if arg == "-store" {
            opts.options.comp_level = CompLevel::Store;
            has_comp_args = true;
        } else if arg.starts_with('-') {
            let bytes = arg.as_bytes();
            let level = match bytes.get(1) {
                Some(b't') => CompLevel::Turbo,
                Some(b'f') => CompLevel::Fast,
                Some(b'm') => CompLevel::Mid,
                Some(b'h') => CompLevel::High,
                Some(b'x') => CompLevel::Max,
                _ => {
                    eprintln!("Unknown option {arg}");
                    return Err(4);
                }
            };
            opts.options.comp_level = level;
            has_comp_args = true;
            let mem_string = &arg[2..];
            match mem_string {
                "" => {}
                "0" | "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "10" | "11" => {
                    opts.options.mem_usage = mem_string.parse().unwrap();
                }
                _ => {
                    eprintln!("Unknown mem level {mem_string}");
                    return Err(4);
                }
            }
        } else if !arg.is_empty() {
            if matches!(opts.mode, Mode::Add | Mode::Extract) {
                opts.files.push(arg.to_string());
            } else {
                break;
            }
        }
        i += 1;
    }
    let _ = block_size;

    if opts.mode == Mode::Unknown && has_comp_args {
        opts.mode = Mode::Compress;
    }
    let single_file_mode = matches!(
        opts.mode,
        Mode::Compress | Mode::Decompress | Mode::SingleTest | Mode::MemTest | Mode::Opt
    );
    if single_file_mode {
        let in_file = argv.get(i).cloned().unwrap_or_default();
        if i < argv.len() {
            i += 1;
        }
        let out_file = match argv.get(i) {
            Some(v) => v.clone(),
            None => {
                if opts.mode == Mode::Decompress {
                    format!("{in_file}.decomp")
                } else {
                    format!("{in_file}.mcm")
                }
            }
        };
        match opts.mode {
            Mode::MemTest => opts.files.push(in_file),
            Mode::Compress | Mode::SingleTest | Mode::Opt => {
                opts.archive_file = out_file;
                opts.files.push(in_file);
            }
            _ => {
                opts.archive_file = in_file;
                opts.files.push(out_file);
            }
        }
    }
    if opts.archive_file.is_empty() || opts.files.is_empty() {
        eprintln!("Error, input or output files missing");
        usage(program);
        return Err(5);
    }
    Ok(())
}

fn read_file(path: &str) -> Result<Vec<u8>, u8> {
    read_file_named(path, path)
}

fn read_file_named(path: &str, reported: &str) -> Result<Vec<u8>, u8> {
    std::fs::read(path).map_err(|_| {
        eprintln!("Error opening: {reported} ({ERRSTR})");
        1
    })
}

fn write_file(path: &str, data: &[u8]) -> Result<(), u8> {
    std::fs::write(path, data).map_err(|_| {
        eprintln!("Error opening: {path} ({ERRSTR})");
        2
    })
}

fn compress(opts: &Options) -> ExitCode {
    let in_file = opts.files.last().cloned().unwrap_or_default();
    let out_file = opts.archive_file.clone();
    print_header();
    let input = match read_file(&in_file) {
        Ok(d) => d,
        Err(c) => return ExitCode::from(c),
    };
    println!(
        "Compressing to {out_file} mode={} mem={}",
        level_name(opts.options.comp_level),
        opts.options.mem_usage
    );
    let start = Instant::now();
    let archive = Archive::new(opts.options);
    let packed = archive.compress(&input);
    let seconds = start.elapsed().as_secs_f64();
    if let Err(c) = write_file(&out_file, &packed) {
        return ExitCode::from(c);
    }
    let in_size = input.len() as u64;
    let out_size = packed.len() as u64;
    println!(
        "Compressed {}->{} in {:.3}s bpc={:.3}",
        format_number(in_size),
        format_number(out_size),
        seconds,
        out_size as f64 * 8.0 / in_size.max(1) as f64
    );
    println!("Avg rate: {:.3} ns/B", seconds * 1e9 / in_size.max(1) as f64);

    if opts.mode == Mode::SingleTest {
        println!("Decompresing & verifying file");
        let packed = match read_file(&out_file) {
            Ok(d) => d,
            Err(c) => return ExitCode::from(c),
        };
        let back = archive.decompress(&packed);
        verify_summary(&back, &input);
    }
    ExitCode::SUCCESS
}

fn verify_summary(got: &[u8], expected: &[u8]) {
    if got.len() != expected.len() {
        let differences = got.iter().zip(expected).filter(|(a, b)| a != b).count();
        eprintln!(
            "ERROR: Missing bytes {}/{} differences={differences}",
            got.len(),
            expected.len()
        );
        return;
    }
    match got.iter().zip(expected).position(|(a, b)| a != b) {
        Some(pos) => {
            eprintln!(
                "Difference found at byte! {pos} b1: ref: {} new: {}",
                expected[pos], got[pos]
            );
            let differences = got.iter().zip(expected).filter(|(a, b)| a != b).count();
            eprintln!("ERROR: differences={differences}");
        }
        None => println!("No differences found!"),
    }
}

fn decompress(opts: &Options) -> ExitCode {
    let in_file = opts.archive_file.clone();
    let out_file = opts.files.last().cloned().unwrap_or_default();
    let input = match read_file_named(&in_file, &out_file) {
        Ok(d) => d,
        Err(c) => return ExitCode::from(c),
    };
    print_header();
    println!("Decompresing file {in_file}");
    let archive = Archive::new(opts.options);
    if !archive.is_archive(&input) {
        eprintln!("Attempting to decompress non archive file");
        return ExitCode::from(1);
    }
    if !archive.is_same_version(&input) {
        eprintln!(
            "Attempting to decompress old version {}.{}",
            archive.major_version(&input),
            archive.minor_version(&input)
        );
        return ExitCode::from(1);
    }
    let out = archive.decompress(&input);
    if let Err(c) = write_file(&out_file, &out) {
        return ExitCode::from(c);
    }
    ExitCode::SUCCESS
}
