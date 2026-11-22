//! One-time codegen tool: parse Oni's C `tm_template`/`tm_struct`/`tm_enum`
//! declarations and emit a standalone Rust file of `oni_template::Layout`s.
//!
//! The emitted file (`crates/oni-template/src/generated.rs`) is committed and
//! is the preservation artifact — after running this once, the original C
//! source is no longer needed to read any Oni file type.
//!
//! Usage:
//!   `cargo run -p oni-template-gen -- <OniSource dir> <out generated.rs>`
//!
//! This is a pragmatic C-subset parser, not a full compiler front end. It
//! handles comments, `#if UUmPlatform` blocks (keeping Win32), scalar
//! typedefs, `tm_enum`/`tm_struct`/plain `struct`, and `tm_template`, with
//! natural (VC6) field alignment. Templates it cannot fully resolve are skipped
//! and reported, so the output is always valid.

use std::collections::HashMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

fn main() {
    let mut args = std::env::args().skip(1);
    let root = args.next().unwrap_or_else(|| "../original/source".into());
    let out = args
        .next()
        .unwrap_or_else(|| "crates/oni-template/src/generated.rs".into());

    let mut files = Vec::new();
    collect_headers(Path::new(&root), &mut files);
    files.sort(); // deterministic codegen regardless of filesystem order
    eprintln!("scanning {} headers under {root}", files.len());

    let mut types = TypeTable::new();
    let mut templates: Vec<Template> = Vec::new();
    for f in &files {
        let src = std::fs::read_to_string(f).unwrap_or_default();
        let src = preprocess(&src);
        parse_file(&src, &mut types, &mut templates);
    }
    eprintln!(
        "parsed {} templates, {} named types",
        templates.len(),
        types.len()
    );

    // Resolve each template to a flat, aligned layout.
    let mut ok = Vec::new();
    let mut failed: Vec<(String, String)> = Vec::new();
    for t in &templates {
        match resolve_layout(t, &types) {
            Ok(fields) => ok.push((t.clone(), fields)),
            Err(e) => failed.push((t.name.clone(), e)),
        }
    }
    // De-duplicate by tag (keep first), templates can be declared once.
    ok.sort_by_key(|(t, _)| t.tag);
    ok.dedup_by_key(|(t, _)| t.tag);

    let code = emit(&ok);
    std::fs::write(&out, code).expect("write generated.rs");
    eprintln!("wrote {} layouts to {out}", ok.len());
    if !failed.is_empty() {
        eprintln!("skipped {} templates:", failed.len());
        for (n, e) in &failed {
            eprintln!("  {n}: {e}");
        }
    }
}

fn collect_headers(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            collect_headers(&p, out);
        } else if p.extension().and_then(|x| x.to_str()) == Some("h") {
            out.push(p);
        }
    }
}

// ---------------------------------------------------------------------------
// Preprocessing: strip comments, keep Win32 side of `#if UUmPlatform` blocks.
// ---------------------------------------------------------------------------

fn preprocess(src: &str) -> String {
    let no_comments = strip_comments(src);
    let mut out = String::with_capacity(no_comments.len());
    // Stack of "are we currently emitting" for each open #if we track.
    let mut emit_stack: Vec<bool> = Vec::new();
    for line in no_comments.lines() {
        let t = line.trim_start();
        if t.starts_with('#') {
            let d = t.trim_start_matches('#').trim_start();
            if d.starts_with("if") {
                // Decide based on platform mention.
                let keep = platform_keep(d);
                emit_stack.push(keep.unwrap_or(true));
                continue;
            } else if d.starts_with("elif") {
                if let Some(top) = emit_stack.last_mut() {
                    *top = platform_keep(d).unwrap_or(true);
                }
                continue;
            } else if d.starts_with("else") {
                if let Some(top) = emit_stack.last_mut() {
                    *top = !*top;
                }
                continue;
            } else if d.starts_with("endif") {
                emit_stack.pop();
                continue;
            }
            // other directives (#include/#define/#pragma): drop
            continue;
        }
        if emit_stack.iter().all(|&b| b) {
            out.push_str(line);
            out.push('\n');
        }
    }
    out
}

/// For an `#if`/`#elif` condition mentioning the platform, returns Some(true) to
/// keep the Win32 branch, Some(false) to drop a Mac-only branch, or None when
/// the condition is not platform-related (keep by default).
fn platform_keep(cond: &str) -> Option<bool> {
    let win = cond.contains("UUmPlatform_Win32");
    let mac = cond.contains("UUmPlatform_Mac");
    match (win, mac) {
        (true, false) => Some(true),
        (false, true) => Some(false),
        _ => None,
    }
}

fn strip_comments(src: &str) -> String {
    let b = src.as_bytes();
    let mut out = String::with_capacity(src.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'/' && i + 1 < b.len() && b[i + 1] == b'/' {
            while i < b.len() && b[i] != b'\n' {
                i += 1;
            }
        } else if b[i] == b'/' && i + 1 < b.len() && b[i + 1] == b'*' {
            i += 2;
            while i + 1 < b.len() && !(b[i] == b'*' && b[i + 1] == b'/') {
                i += 1;
            }
            i += 2;
        } else {
            out.push(b[i] as char);
            i += 1;
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Type model
// ---------------------------------------------------------------------------

#[derive(Clone)]
enum TypeDef {
    Scalar {
        size: u32,
        signed: bool,
        float: bool,
    },
    Enum,
    Struct(Vec<RawField>),
}

#[derive(Clone)]
struct RawField {
    qual: Qual,
    type_name: String,
    name: String,
    is_ptr: bool,
    dims: Vec<u32>,
}

#[derive(Clone, PartialEq)]
enum Qual {
    None,
    Pad,
    Raw,
    Separate,
    TemplateRef,
    VarIndex,
    VarArray,
}

#[derive(Clone)]
struct Template {
    tag: [u8; 4],
    name: String,
    fields: Vec<RawField>,
}

struct TypeTable {
    map: HashMap<String, TypeDef>,
}

impl TypeTable {
    fn new() -> Self {
        let mut map = HashMap::new();
        let mut s = |n: &str, size: u32, signed: bool, float: bool| {
            map.insert(
                n.to_string(),
                TypeDef::Scalar {
                    size,
                    signed,
                    float,
                },
            );
        };
        for n in ["char", "UUtInt8", "signed char"] {
            s(n, 1, true, false);
        }
        for n in ["UUtUns8", "unsigned char", "UUtBool8", "UUtBool"] {
            // UUtBool is a single byte in Oni (confirmed by manual `tm_pad[3]`
            // alignment after bool fields, e.g. ONtCharacterImpactArray).
            s(n, 1, false, false);
        }
        for n in ["UUtInt16", "short", "UUtInt16"] {
            s(n, 2, true, false);
        }
        for n in ["UUtUns16", "unsigned short"] {
            s(n, 2, false, false);
        }
        for n in ["UUtInt32", "int", "long", "UUtError"] {
            s(n, 4, true, false);
        }
        for n in ["UUtUns32", "unsigned long", "unsigned int"] {
            s(n, 4, false, false);
        }
        for n in ["float", "UUtFloat"] {
            s(n, 4, true, true);
        }
        for n in ["UUtInt64", "UUtUns64", "double"] {
            s(n, 8, false, n == "double");
        }
        TypeTable { map }
    }
    fn len(&self) -> usize {
        self.map.len()
    }
    fn get(&self, n: &str) -> Option<&TypeDef> {
        self.map.get(n)
    }
    fn insert(&mut self, n: String, d: TypeDef) {
        self.map.entry(n).or_insert(d);
    }
}

// ---------------------------------------------------------------------------
// Parsing
// ---------------------------------------------------------------------------

fn parse_file(src: &str, types: &mut TypeTable, templates: &mut Vec<Template>) {
    let b: Vec<char> = src.chars().collect();
    let mut i = 0;
    while i < b.len() {
        // Look for keywords at word boundaries.
        if let Some(rest) = starts_word(&b, i, "tm_template") {
            if let Some((tmpl, next)) = parse_template(&b, rest) {
                templates.push(tmpl);
                i = next;
                continue;
            }
        }
        if let Some(rest) = starts_word(&b, i, "tm_struct") {
            if let Some((name, fields, next)) = parse_named_block(&b, rest) {
                if let Some(n) = name {
                    types.insert(n, TypeDef::Struct(fields));
                }
                i = next;
                continue;
            }
        }
        if let Some(rest) = starts_word(&b, i, "tm_enum") {
            if let Some((name, _f, next)) = parse_named_block(&b, rest) {
                if let Some(n) = name {
                    types.insert(n, TypeDef::Enum);
                }
                i = next;
                continue;
            }
        }
        if let Some(rest) = starts_word(&b, i, "typedef") {
            if let Some(next) = parse_typedef(&b, rest, types, templates) {
                i = next;
                continue;
            }
        }
        if let Some(rest) = starts_word(&b, i, "struct") {
            if let Some((name, fields, next)) = parse_named_block(&b, rest) {
                if let Some(n) = name {
                    types.insert(n, TypeDef::Struct(fields));
                }
                i = next;
                continue;
            }
        }
        if let Some(rest) = starts_word(&b, i, "enum") {
            if let Some((name, _f, next)) = parse_named_block(&b, rest) {
                if let Some(n) = name {
                    types.insert(n, TypeDef::Enum);
                }
                i = next;
                continue;
            }
        }
        i += 1;
    }
}

/// If `text[i..]` begins with `word` at a word boundary, return the index just
/// after it.
fn starts_word(b: &[char], i: usize, word: &str) -> Option<usize> {
    let w: Vec<char> = word.chars().collect();
    if i + w.len() > b.len() {
        return None;
    }
    if b[i..i + w.len()] != w[..] {
        return None;
    }
    // boundary before
    if i > 0 && is_ident(b[i - 1]) {
        return None;
    }
    let after = i + w.len();
    if after < b.len() && is_ident(b[after]) {
        return None;
    }
    Some(after)
}

fn is_ident(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// Parse `('a','b','c','d', "name") NAME { ... }` after `tm_template`.
fn parse_template(b: &[char], mut i: usize) -> Option<(Template, usize)> {
    i = skip_ws(b, i);
    if b.get(i)? != &'(' {
        return None;
    }
    // collect the 4 char literals — match the closing ')' while skipping any
    // ')' inside the description string literal (e.g. "Screen (aiming) Coll.").
    let close = find_matching_paren(b, i)?;
    let args: String = b[i + 1..close].iter().collect();
    let tag = parse_tag(&args)?;
    i = close + 1;
    // optional name token before '{'
    let (name, body_start) = read_name_before_brace(b, i)?;
    let (fields, next) = parse_brace_fields(b, body_start)?;
    // skip trailing `NAME;`
    let next = skip_to_semicolon(b, next);
    Some((
        Template {
            tag,
            name: name.unwrap_or_else(|| tag_str(&tag)),
            fields,
        },
        next,
    ))
}

fn parse_tag(args: &str) -> Option<[u8; 4]> {
    // args like: 'T', 'X', 'M', 'P', "Texture Map"
    let mut chars = Vec::new();
    let bytes: Vec<char> = args.chars().collect();
    let mut i = 0;
    while i < bytes.len() && chars.len() < 4 {
        if bytes[i] == '\'' {
            // a char literal 'c' — take the char and skip past the closing quote
            let c = bytes.get(i + 1)?;
            chars.push(*c as u8);
            i += 3; // past opening quote, char, and closing quote
        } else {
            i += 1;
        }
    }
    if chars.len() == 4 {
        Some([chars[0], chars[1], chars[2], chars[3]])
    } else {
        None
    }
}

fn tag_str(t: &[u8; 4]) -> String {
    t.iter().map(|&c| c as char).collect()
}

/// After a type keyword, read an optional NAME then expect `{...}`. Returns
/// (name, fields, index-after-closing-brace-and-trailing-name-semicolon).
fn parse_named_block(b: &[char], i: usize) -> Option<(Option<String>, Vec<RawField>, usize)> {
    let (name, body_start) = read_name_before_brace(b, i)?;
    let (fields, after) = parse_brace_fields(b, body_start)?;
    // read optional trailing typedef name: `} NAME;`
    let (tail_name, after2) = read_optional_ident(b, after);
    let after2 = skip_to_semicolon(b, after2);
    Some((name.or(tail_name), fields, after2))
}

/// Reads an optional identifier starting at `i`, returning it and the next index.
fn read_name_before_brace(b: &[char], i: usize) -> Option<(Option<String>, usize)> {
    let j = skip_ws(b, i);
    match b.get(j)? {
        '{' => Some((None, j)),
        c if is_ident(*c) => {
            let (id, k) = read_ident(b, j);
            let k = skip_ws(b, k);
            if b.get(k)? == &'{' {
                Some((Some(id), k))
            } else {
                None
            }
        }
        _ => None,
    }
}

fn read_optional_ident(b: &[char], i: usize) -> (Option<String>, usize) {
    let j = skip_ws(b, i);
    if j < b.len() && is_ident(b[j]) {
        let (id, k) = read_ident(b, j);
        (Some(id), k)
    } else {
        (None, j)
    }
}

/// Parse `{ field; field; ... }` into raw fields, given index of `{`.
fn parse_brace_fields(b: &[char], brace: usize) -> Option<(Vec<RawField>, usize)> {
    if b.get(brace)? != &'{' {
        return None;
    }
    let end = match_brace(b, brace)?;
    let body: String = b[brace + 1..end].iter().collect();
    let mut fields = Vec::new();
    for stmt in body.split(';') {
        let s = stmt.trim();
        if s.is_empty() || s.contains('{') {
            // nested anonymous block or empty: unsupported here
            continue;
        }
        if let Some(f) = parse_field(s) {
            fields.push(f);
        }
    }
    Some((fields, end + 1))
}

fn parse_field(s: &str) -> Option<RawField> {
    // tokenize on whitespace, but keep tm_raw(TYPE) together-ish
    let s = s.replace('\t', " ");
    let mut toks: Vec<String> = Vec::new();
    for w in s.split_whitespace() {
        toks.push(w.to_string());
    }
    if toks.is_empty() {
        return None;
    }

    let mut qual = Qual::None;
    let mut idx = 0;
    let mut raw_inner: Option<String> = None;
    match toks[0].as_str() {
        "tm_pad" => qual = Qual::Pad,
        "tm_separate" => qual = Qual::Separate,
        "tm_templateref" => qual = Qual::TemplateRef,
        "tm_varindex" => qual = Qual::VarIndex,
        "tm_vararray" => qual = Qual::VarArray,
        t if t.starts_with("tm_raw") => {
            qual = Qual::Raw;
            // tm_raw(TYPE) possibly split; inner type irrelevant (4-byte ref)
            raw_inner = Some("void*".into());
        }
        _ => {}
    }
    if qual != Qual::None {
        idx = 1;
        // tm_raw may be "tm_raw(type" "name" or "tm_raw(type)name"
        if qual == Qual::Raw {
            // skip any tokens until we reach the declarator name (last token)
        }
    }
    let _ = raw_inner;

    // Remaining tokens: TYPE ... NAME[dims]
    let rest = &toks[idx..];
    if rest.is_empty() {
        return None;
    }
    // The declarator is the last token (may include [..]); everything before is type.
    let last = rest.last().unwrap().clone();
    let is_ptr_last = last.contains('*');
    let (name, dims) = split_name_dims(last.trim_start_matches('*'));

    // Determine type name: the token(s) before last, joined; strip '*'.
    let type_tokens = &rest[..rest.len().saturating_sub(1)];
    let mut type_name = type_tokens.join(" ");
    let mut is_ptr = is_ptr_last;
    if type_name.contains('*') {
        is_ptr = true;
    }
    type_name = type_name.replace('*', "").trim().to_string();
    if type_name.is_empty() {
        // e.g. "tm_pad name[dims]" where pad's base type is char
        type_name = match qual {
            Qual::Pad => "char".into(),
            Qual::Separate | Qual::Raw | Qual::TemplateRef => "UUtUns32".into(),
            _ => return None,
        };
    }

    Some(RawField {
        qual,
        type_name,
        name,
        is_ptr,
        dims,
    })
}

/// Splits a declarator like `name[4][2]` into (name, `[4, 2]`).
fn split_name_dims(decl: &str) -> (String, Vec<u32>) {
    let decl = decl.trim_end_matches(';').trim();
    let name: String = decl.chars().take_while(|c| is_ident(*c)).collect();
    let mut dims = Vec::new();
    let mut rest = &decl[name.len()..];
    while let Some(open) = rest.find('[') {
        if let Some(close) = rest[open..].find(']') {
            let inside = &rest[open + 1..open + close];
            if let Ok(n) = inside.trim().parse::<u32>() {
                dims.push(n);
            } else {
                dims.push(0); // non-literal dim (e.g. a macro) — treat as 0/unknown
            }
            rest = &rest[open + close + 1..];
        } else {
            break;
        }
    }
    (name, dims)
}

/// Handle `typedef ...`. Returns next index if it consumed something useful.
fn parse_typedef(
    b: &[char],
    i: usize,
    types: &mut TypeTable,
    templates: &mut Vec<Template>,
) -> Option<usize> {
    let j = skip_ws(b, i);
    // typedef tm_template(...)
    if let Some(r) = starts_word(b, j, "tm_template") {
        if let Some((tmpl, next)) = parse_template(b, r) {
            templates.push(tmpl);
            return Some(next);
        }
    }
    for (kw, is_struct) in [
        ("tm_struct", true),
        ("struct", true),
        ("tm_enum", false),
        ("enum", false),
    ] {
        if let Some(r) = starts_word(b, j, kw) {
            if let Some((name, fields, next)) = parse_named_block(b, r) {
                if let Some(n) = name {
                    types.insert(
                        n,
                        if is_struct {
                            TypeDef::Struct(fields)
                        } else {
                            TypeDef::Enum
                        },
                    );
                }
                return Some(next);
            }
        }
    }
    // simple alias: typedef BASE NAME;  (no braces before ';')
    let semi = skip_to_semicolon(b, j);
    let decl: String = b[j..semi.saturating_sub(1).max(j)].iter().collect();
    if !decl.contains('{') && !decl.contains('(') {
        let toks: Vec<&str> = decl.split_whitespace().collect();
        if toks.len() >= 2 {
            let name = toks.last().unwrap().trim_matches('*').to_string();
            let base = toks[..toks.len() - 1].join(" ").replace('*', "");
            let base = base.trim().to_string();
            if toks.iter().any(|t| t.contains('*')) {
                types.insert(
                    name,
                    TypeDef::Scalar {
                        size: 4,
                        signed: false,
                        float: false,
                    },
                );
            } else if let Some(TypeDef::Scalar {
                size,
                signed,
                float,
            }) = types.get(&base).cloned().as_ref()
            {
                types.insert(
                    name,
                    TypeDef::Scalar {
                        size: *size,
                        signed: *signed,
                        float: *float,
                    },
                );
            }
        }
        return Some(semi);
    }
    None
}

// ---------------------------------------------------------------------------
// Low-level scanners
// ---------------------------------------------------------------------------

fn skip_ws(b: &[char], mut i: usize) -> usize {
    while i < b.len() && b[i].is_whitespace() {
        i += 1;
    }
    i
}
fn read_ident(b: &[char], mut i: usize) -> (String, usize) {
    let start = i;
    while i < b.len() && is_ident(b[i]) {
        i += 1;
    }
    (b[start..i].iter().collect(), i)
}
/// Finds the `)` matching the `(` at `open`, tracking nesting and skipping over
/// `"..."` and `'.'` literals (so parens inside a string don't fool us).
fn find_matching_paren(b: &[char], open: usize) -> Option<usize> {
    let mut depth = 0i32;
    let mut i = open;
    while i < b.len() {
        match b[i] {
            '"' => {
                i += 1;
                while i < b.len() && b[i] != '"' {
                    if b[i] == '\\' {
                        i += 1;
                    }
                    i += 1;
                }
            }
            '\'' => {
                // char literal 'c' or '\n'
                i += if b.get(i + 1) == Some(&'\\') { 3 } else { 2 };
                continue;
            }
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}

fn match_brace(b: &[char], open: usize) -> Option<usize> {
    let mut depth = 0;
    let mut i = open;
    while i < b.len() {
        match b[i] {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}
fn skip_to_semicolon(b: &[char], mut i: usize) -> usize {
    while i < b.len() && b[i] != ';' {
        i += 1;
    }
    (i + 1).min(b.len())
}

// ---------------------------------------------------------------------------
// Layout resolution (natural / VC6 alignment)
// ---------------------------------------------------------------------------

/// A resolved layout field we will emit.
enum OutField {
    Scalar {
        name: String,
        kind: &'static str,
    },
    Pad {
        bytes: u32,
    },
    Array {
        name: String,
        len: u32,
        elem: Vec<OutField>,
    },
    VarArray {
        name: String,
        elem: Vec<OutField>,
    },
}

fn resolve_layout(t: &Template, types: &TypeTable) -> Result<Vec<OutField>, String> {
    let mut out = Vec::new();
    let mut offset = 0u32;
    lay_fields(&t.fields, types, &mut out, &mut offset, 0)?;
    Ok(out)
}

/// Lay out raw fields with natural alignment, pushing OutFields and advancing offset.
fn lay_fields(
    fields: &[RawField],
    types: &TypeTable,
    out: &mut Vec<OutField>,
    offset: &mut u32,
    depth: u32,
) -> Result<(), String> {
    if depth > 16 {
        return Err("recursion too deep".into());
    }
    for f in fields {
        match f.qual {
            Qual::Pad => {
                let n: u32 = f
                    .dims
                    .iter()
                    .copied()
                    .product::<u32>()
                    .max(f.dims.is_empty() as u32);
                let n = if f.dims.is_empty() { 1 } else { n };
                out.push(OutField::Pad { bytes: n });
                *offset += n;
            }
            Qual::Raw | Qual::Separate | Qual::TemplateRef => {
                align_to(4, out, offset);
                let kind = match f.qual {
                    Qual::Raw => "RawPtr",
                    Qual::Separate => "SeparateIndex",
                    _ => "TemplateRef",
                };
                out.push(OutField::Scalar {
                    name: field_name(f),
                    kind,
                });
                *offset += 4;
            }
            Qual::VarArray => {
                let elem = resolve_elem(&f.type_name, f.is_ptr, types, depth)?;
                // align like its element
                let a = elem_align(&elem);
                align_to(a, out, offset);
                out.push(OutField::VarArray {
                    name: field_name(f),
                    elem,
                });
            }
            Qual::VarIndex | Qual::None => {
                lay_scalar_or_aggregate(f, types, out, offset, depth)?;
            }
        }
    }
    Ok(())
}

fn lay_scalar_or_aggregate(
    f: &RawField,
    types: &TypeTable,
    out: &mut Vec<OutField>,
    offset: &mut u32,
    depth: u32,
) -> Result<(), String> {
    if f.is_ptr {
        align_to(4, out, offset);
        out.push(OutField::Scalar {
            name: field_name(f),
            kind: "TemplateRef",
        });
        *offset += 4;
        return Ok(());
    }
    let count: u32 = if f.dims.is_empty() {
        1
    } else {
        f.dims.iter().copied().product()
    };
    let single = resolve_elem(&f.type_name, false, types, depth)?;
    let a = elem_align(&single);
    if f.dims.is_empty() {
        align_to(a, out, offset);
        // inline the element's fields directly (flatten)
        inline(single, field_name(f), out, offset);
    } else {
        align_to(a, out, offset);
        let elem_sz = elem_fixed_size(&single);
        out.push(OutField::Array {
            name: field_name(f),
            len: count,
            elem: single,
        });
        *offset += count * elem_sz;
    }
    Ok(())
}

/// Resolve a type name into a list of OutFields representing one value.
fn resolve_elem(
    type_name: &str,
    is_ptr: bool,
    types: &TypeTable,
    depth: u32,
) -> Result<Vec<OutField>, String> {
    if is_ptr {
        return Ok(vec![OutField::Scalar {
            name: String::new(),
            kind: "TemplateRef",
        }]);
    }
    match types.get(type_name) {
        Some(TypeDef::Scalar {
            size,
            signed,
            float,
        }) => {
            let kind = scalar_kind(*size, *signed, *float)?;
            Ok(vec![OutField::Scalar {
                name: String::new(),
                kind,
            }])
        }
        Some(TypeDef::Enum) => Ok(vec![OutField::Scalar {
            name: String::new(),
            kind: "Enum",
        }]),
        Some(TypeDef::Struct(fields)) => {
            let mut out = Vec::new();
            let mut off = 0u32;
            lay_fields(fields, types, &mut out, &mut off, depth + 1)?;
            // tail-pad struct to its own alignment
            let a = elem_align(&out);
            let pad = (a - (off % a)) % a;
            if pad > 0 {
                out.push(OutField::Pad { bytes: pad });
            }
            Ok(out)
        }
        None => Err(format!("unknown type '{type_name}'")),
    }
}

fn scalar_kind(size: u32, signed: bool, float: bool) -> Result<&'static str, String> {
    Ok(match (size, signed, float) {
        (1, false, _) => "U8",
        (1, true, _) => "I8",
        (2, false, _) => "U16",
        (2, true, _) => "I16",
        (4, _, true) => "F32",
        (4, false, _) => "U32",
        (4, true, _) => "I32",
        (8, _, _) => "U64",
        _ => return Err(format!("odd scalar {size}")),
    })
}

/// Inline a resolved element's fields into `out`, naming the first scalar when
/// the element is a single scalar, else prefixing nested names.
fn inline(elem: Vec<OutField>, name: String, out: &mut Vec<OutField>, offset: &mut u32) {
    if elem.len() == 1 {
        if let OutField::Scalar { kind, .. } = &elem[0] {
            let k = *kind;
            out.push(OutField::Scalar { name, kind: k });
            *offset += scalar_width(k);
            return;
        }
    }
    for sub in elem {
        match &sub {
            OutField::Scalar { kind, .. } => *offset += scalar_width(kind),
            OutField::Pad { bytes } => *offset += bytes,
            OutField::Array { len, elem, .. } => *offset += len * elem_fixed_size(elem),
            OutField::VarArray { .. } => {}
        }
        // prefix nested field names (arrays' too: two nested structs'
        // arrays of one name would otherwise collide)
        let renamed = match sub {
            OutField::Scalar { name: n, kind } => OutField::Scalar {
                name: format!("{name}.{n}"),
                kind,
            },
            OutField::Array { name: n, len, elem } => OutField::Array {
                name: format!("{name}.{n}"),
                len,
                elem,
            },
            OutField::VarArray { name: n, elem } => OutField::VarArray {
                name: format!("{name}.{n}"),
                elem,
            },
            other => other,
        };
        out.push(renamed);
    }
}

fn field_name(f: &RawField) -> String {
    if f.name.is_empty() {
        f.type_name.clone()
    } else {
        f.name.clone()
    }
}

fn align_to(a: u32, out: &mut Vec<OutField>, offset: &mut u32) {
    if a == 0 {
        return;
    }
    let pad = (a - (*offset % a)) % a;
    if pad > 0 {
        out.push(OutField::Pad { bytes: pad });
        *offset += pad;
    }
}

fn scalar_width(kind: &str) -> u32 {
    match kind {
        "U8" | "I8" => 1,
        "U16" | "I16" => 2,
        "U64" => 8,
        _ => 4,
    }
}
fn elem_fixed_size(elem: &[OutField]) -> u32 {
    elem.iter()
        .map(|f| match f {
            OutField::Scalar { kind, .. } => scalar_width(kind),
            OutField::Pad { bytes } => *bytes,
            OutField::Array { len, elem, .. } => len * elem_fixed_size(elem),
            OutField::VarArray { .. } => 0,
        })
        .sum()
}
fn elem_align(elem: &[OutField]) -> u32 {
    elem.iter()
        .map(|f| match f {
            OutField::Scalar { kind, .. } => scalar_width(kind).min(4),
            OutField::Array { elem, .. } => elem_align(elem),
            _ => 1,
        })
        .max()
        .unwrap_or(1)
        .max(1)
}

// ---------------------------------------------------------------------------
// Emit Rust
// ---------------------------------------------------------------------------

fn emit(layouts: &[(Template, Vec<OutField>)]) -> String {
    let mut s = String::new();
    s.push_str("// @generated by oni-template-gen from the Oni C headers. Do not edit by hand.\n");
    s.push_str("// This file is the standalone preservation artifact: it reconstructs every\n");
    s.push_str(
        "// template's field layout without the original C source.\n#![allow(clippy::all)]\n\n",
    );
    s.push_str("use crate::{Field, Kind, Layout};\nuse oni_fourcc::FourCc;\n\n");
    s.push_str("/// All template layouts parsed from the Oni headers.\n");
    s.push_str("pub fn layouts() -> Vec<Layout> {\n    vec![\n");
    for (t, fields) in layouts {
        let tag = u32::from_be_bytes(t.tag);
        writeln!(
            s,
            "        Layout {{ tag: FourCc(0x{tag:08X}), name: {:?}.into(), fields: vec![",
            t.name
        )
        .unwrap();
        for f in fields {
            emit_field(&mut s, f, 3);
        }
        s.push_str("        ] },\n");
    }
    s.push_str("    ]\n}\n");
    s
}

fn emit_field(s: &mut String, f: &OutField, indent: usize) {
    let pad = "    ".repeat(indent);
    match f {
        OutField::Scalar { name, kind } => {
            writeln!(s, "{pad}Field::new({name:?}, Kind::{kind}),").unwrap();
        }
        OutField::Pad { bytes } => {
            writeln!(s, "{pad}Field::new(\"_pad\", Kind::Pad({bytes})),").unwrap();
        }
        OutField::Array { name, len, elem } => {
            writeln!(
                s,
                "{pad}Field::new({name:?}, Kind::Array {{ len: {len}, elem: vec!["
            )
            .unwrap();
            for e in elem {
                emit_field(s, e, indent + 1);
            }
            writeln!(s, "{pad}] }}),").unwrap();
        }
        OutField::VarArray { name, elem } => {
            writeln!(s, "{pad}Field::new({name:?}, Kind::VarArray {{ elem: vec![").unwrap();
            for e in elem {
                emit_field(s, e, indent + 1);
            }
            writeln!(s, "{pad}] }}),").unwrap();
        }
    }
}
