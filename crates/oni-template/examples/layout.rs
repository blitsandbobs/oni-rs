//! Prints a template's layout: each field's offset (after the instance's
//! pre-data), name and kind — the names a decoder reads by.
//!
//!   cargo run -p oni-template --example layout -- TURR

use oni_fourcc::FourCc;
use oni_template::{fixed_size, Field, Kind, Registry};

fn show(fields: &[Field], at: &mut u32, indent: usize) {
    for f in fields {
        let pad = " ".repeat(indent);
        match &f.kind {
            Kind::Array { len, elem } => {
                let size = fixed_size(elem);
                println!("{at:5} {pad}{}[{len}] ({size} bytes each)", f.name);
                let mut inner = *at;
                show(elem, &mut inner, indent + 2);
                *at += len * size;
            }
            Kind::VarArray { elem } => {
                println!("{at:5} {pad}{}[] ({} bytes each)", f.name, fixed_size(elem));
                let mut inner = *at;
                show(elem, &mut inner, indent + 2);
            }
            k => {
                println!("{at:5} {pad}{} {k:?}", f.name);
                *at += fixed_size(std::slice::from_ref(f));
            }
        }
    }
}

fn main() {
    let tag = std::env::args().nth(1).expect("usage: layout TAG");
    let bytes: [u8; 4] = tag.as_bytes().try_into().expect("a four-character tag");
    let registry = Registry::load();
    let layout = registry
        .get(FourCc(u32::from_be_bytes(bytes)))
        .expect("no such template");
    println!("{} ({tag})", layout.name);
    show(&layout.fields, &mut 0, 0);
}
