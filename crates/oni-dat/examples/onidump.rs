//! `onidump` — list the instances in an Oni `.dat` file.
//!
//! Usage:
//!   cargo run -p oni-dat --example onidump -- <file.dat> [--names] [--tags]
//!
//! With no flags it prints a header summary and a per-tag histogram — directly
//! comparable to an OniSplit `-export` of the same file.

use std::collections::BTreeMap;
use std::process::ExitCode;

use oni_dat::{Dat, FourCc};

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let path = match args.next() {
        Some(p) => p,
        None => {
            eprintln!("usage: onidump <file.dat> [--names] [--tags]");
            return ExitCode::from(2);
        }
    };
    let mut show_names = false;
    let mut show_tags = false;
    for a in args {
        match a.as_str() {
            "--names" => show_names = true,
            "--tags" => show_tags = true,
            other => {
                eprintln!("unknown option: {other}");
                return ExitCode::from(2);
            }
        }
    }

    let bytes = match std::fs::read(&path) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("cannot read {path}: {e}");
            return ExitCode::FAILURE;
        }
    };

    let dat = match Dat::parse(&bytes) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("parse error: {e}");
            return ExitCode::FAILURE;
        }
    };

    let h = &dat.header;
    println!("file:            {path}");
    println!("byte order:      {:?}", h.endian);
    println!("total instances: {}", dat.instances.len());
    println!("named instances: {}", dat.named_count());
    println!("templates:       {}", dat.templates.len());
    println!(
        "data block:      offset {} len {}",
        h.data_block_offset, h.data_block_length
    );
    println!(
        "name block:      offset {} len {}",
        h.name_block_offset, h.name_block_length
    );

    // Named-instance histogram is the apples-to-apples comparison with
    // OniSplit's -export (which emits one .oni per named instance).
    let named = dat.named_tag_histogram();
    let total = dat.tag_histogram();
    print_histogram("named instances by tag", &named);

    if show_tags {
        print_histogram("ALL instances by tag", &total);
        println!("\ntemplate descriptors:");
        for t in &dat.templates {
            println!(
                "  {}  used={:<6} checksum=0x{:016X}",
                t.tag, t.num_used, t.checksum
            );
        }
    }

    if show_names {
        println!("\nnamed instances:");
        for i in dat.instances.iter().filter(|i| i.is_named()) {
            println!("  {}{}", i.tag, i.name.as_deref().unwrap_or(""));
        }
    }

    ExitCode::SUCCESS
}

fn print_histogram(title: &str, hist: &BTreeMap<FourCc, usize>) {
    let mut rows: Vec<(FourCc, usize)> = hist.iter().map(|(&k, &v)| (k, v)).collect();
    rows.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    let total: usize = rows.iter().map(|(_, c)| c).sum();
    println!("\n{title} ({} tags, {total} instances):", rows.len());
    for (tag, count) in rows {
        println!("  {tag}  {count}");
    }
}
