//! BSL → Lua: translates a level's scripts (the original's `.bsl` text, one
//! set per level) into one Lua chunk the engine runs (`scripting/PORT.md`,
//! `scripting/BSL.md`).
//!
//! The language is small: `var` / `func` declarations, `if` / `else`,
//! `sleep`, `fork`, `return`, assignments and command calls in two styles
//! (C-style `cmd(a, b)` and shell-style `cmd a b`, a newline ending the
//! statement). What is decided at run time stays in a handful of host
//! helpers the chunk receives, so the translation itself is mechanical:
//!
//! - `V` script globals, `F` script functions, `C` engine commands, `G`
//!   engine variables;
//! - `W(name)` a bare word that is neither a script variable nor a script
//!   function (an engine variable, a value-returning command, or else the
//!   word itself as a string);
//! - `T(x)` BSL truth, `EQ(a, b)` / `NE` / `LT` / `GT` / `LE` / `GE` BSL
//!   comparisons across types, `ADD` / `SUB` arithmetic;
//! - `SLEEP(n)`, `FORK(f, ...)` the scheduler; `ERR(where, what)` an
//!   untranslatable statement (reported when run).
//!
//! The chunk's first line receives them: `local V, F, C, G, W, T, … = ...`.

use std::collections::{HashMap, HashSet};
use std::fmt::Write;

// --- lexing ---------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
enum Tok {
    Ident(String),
    Int(i64),
    Float(f64),
    Str(String),
    Punct(&'static str),
}

#[derive(Clone, Debug)]
struct Token {
    tok: Tok,
    line: u32,
    /// The last token on its line.
    eol: bool,
}

/// The value of the longest valid integer prefix (`%d`).
fn prefix_int(s: &str) -> i64 {
    let end = s
        .char_indices()
        .find(|&(k, ch)| !(ch.is_ascii_digit() || (k == 0 && ch == '-')))
        .map_or(s.len(), |(k, _)| k);
    s[..end].parse().unwrap_or(0)
}

/// The value of the longest valid float prefix (`%f`).
fn prefix_float(s: &str) -> f64 {
    (1..=s.len())
        .rev()
        .find_map(|n| s[..n].parse::<f64>().ok())
        .unwrap_or(0.0)
}

fn lex(text: &str) -> Result<Vec<Token>, String> {
    let mut out: Vec<Token> = Vec::new();
    let b = text.as_bytes();
    let mut i = 0;
    let mut line = 1u32;
    let newline = |out: &mut Vec<Token>| {
        if let Some(t) = out.last_mut() {
            t.eol = true;
        }
    };
    while i < b.len() {
        let c = b[i];
        match c {
            b'\n' => {
                newline(&mut out);
                line += 1;
                i += 1;
            }
            b' ' | b'\t' | b'\r' => i += 1,
            b'#' => {
                while i < b.len() && b[i] != b'\n' {
                    i += 1;
                }
            }
            b'"' => {
                let start = i + 1;
                i += 1;
                while i < b.len() && b[i] != b'"' && b[i] != b'\n' {
                    i += 1;
                }
                let s = String::from_utf8_lossy(&b[start..i]).into_owned();
                if i < b.len() && b[i] == b'"' {
                    i += 1;
                }
                out.push(Token {
                    tok: Tok::Str(s),
                    line,
                    eol: false,
                });
            }
            // A number: a digit, `.` or `-` then a digit, then digits / `.` /
            // `-` (only the valid prefix counts: `5-3` is 5). `f` right
            // before a digit is a prefix the original drops (`f60` = 60).
            _ if c.is_ascii_digit()
                || (c == b'.' && b.get(i + 1).is_some_and(u8::is_ascii_digit))
                || (c == b'-'
                    && b.get(i + 1)
                        .is_some_and(|d| d.is_ascii_digit() || *d == b'.'))
                || (c == b'f' && b.get(i + 1).is_some_and(u8::is_ascii_digit)) =>
            {
                if c == b'f' {
                    i += 1;
                }
                let start = i;
                let mut float = false;
                i += 1;
                while i < b.len() && (b[i].is_ascii_digit() || b[i] == b'.' || b[i] == b'-') {
                    float |= b[i] == b'.';
                    i += 1;
                }
                float |= b[start] == b'.';
                // A word that starts with digits (`3rd_guy`) is a word.
                if i < b.len() && (b[i].is_ascii_alphabetic() || b[i] == b'_') {
                    while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'_') {
                        i += 1;
                    }
                    let s = String::from_utf8_lossy(&b[start..i]).into_owned();
                    out.push(Token {
                        tok: Tok::Ident(s),
                        line,
                        eol: false,
                    });
                    continue;
                }
                let s = std::str::from_utf8(&b[start..i]).unwrap_or("0");
                let tok = if float {
                    Tok::Float(prefix_float(s))
                } else {
                    Tok::Int(prefix_int(s))
                };
                out.push(Token {
                    tok,
                    line,
                    eol: false,
                });
            }
            c if c.is_ascii_alphabetic() || c == b'_' => {
                let start = i;
                while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'_') {
                    i += 1;
                }
                let s = String::from_utf8_lossy(&b[start..i]).into_owned();
                out.push(Token {
                    tok: Tok::Ident(s),
                    line,
                    eol: false,
                });
            }
            _ => {
                let two = b.get(i..i + 2).unwrap_or(&[]);
                let p: &'static str = match two {
                    b"<=" => "<=",
                    b">=" => ">=",
                    b"==" => "==",
                    b"!=" => "!=",
                    b"&&" => "&&",
                    b"||" => "||",
                    _ => match c {
                        b'(' => "(",
                        b')' => ")",
                        b'{' => "{",
                        b'}' => "}",
                        b',' => ",",
                        b';' => ";",
                        b'=' => "=",
                        b'!' => "!",
                        b'<' => "<",
                        b'>' => ">",
                        b'+' => "+",
                        b'-' => "-",
                        b'[' => "[",
                        b']' => "]",
                        b':' => ":",
                        // (Fails only the statement it is in.)
                        _ => "?",
                    },
                };
                i += p.len();
                out.push(Token {
                    tok: Tok::Punct(p),
                    line,
                    eol: false,
                });
            }
        }
    }
    newline(&mut out);
    Ok(out)
}

// --- the level's declarations ---------------------------------------------------

/// What every file of a level declares (globals and functions are shared).
#[derive(Default)]
struct Decls {
    globals: Vec<(String, String, Option<String>)>, // name, type, initial (Lua)
    global_names: HashSet<String>,
    funcs: HashSet<String>,
}

const TYPES: [&str; 5] = ["int", "float", "bool", "string", "void"];

fn default_of(ty: &str) -> &'static str {
    match ty {
        "bool" => "false",
        "string" => "\"\"",
        _ => "0",
    }
}

// --- parsing + code generation --------------------------------------------------

struct Gen<'a> {
    toks: &'a [Token],
    pos: usize,
    file: &'a str,
    decls: &'a Decls,
    /// Locals in scope (params and `var`s inside the function).
    locals: Vec<HashSet<String>>,
    out: String,
    errors: Vec<String>,
}

fn lua_str(s: &str) -> String {
    let mut o = String::from("\"");
    for ch in s.chars() {
        match ch {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            c if (c as u32) < 0x20 => {
                let _ = write!(o, "\\{}", c as u32);
            }
            c => o.push(c),
        }
    }
    o.push('"');
    o
}

fn local(name: &str) -> String {
    format!("l_{name}")
}

impl<'a> Gen<'a> {
    fn peek(&self) -> Option<&Token> {
        self.toks.get(self.pos)
    }
    fn peek_at(&self, k: usize) -> Option<&Token> {
        self.toks.get(self.pos + k)
    }
    fn next(&mut self) -> Option<Token> {
        let t = self.toks.get(self.pos).cloned();
        self.pos += 1;
        t
    }
    fn is_punct(&self, p: &str) -> bool {
        matches!(self.peek(), Some(Token { tok: Tok::Punct(q), .. }) if *q == p)
    }
    fn is_word(&self, w: &str) -> bool {
        matches!(self.peek(), Some(Token { tok: Tok::Ident(q), .. }) if q == w)
    }
    fn eat_punct(&mut self, p: &str) -> bool {
        if self.is_punct(p) {
            self.pos += 1;
            true
        } else {
            false
        }
    }
    fn expect_punct(&mut self, p: &str) -> Result<(), String> {
        if self.eat_punct(p) {
            Ok(())
        } else {
            Err(format!(
                "expected '{p}', found {:?}",
                self.peek().map(|t| &t.tok)
            ))
        }
    }
    fn line(&self) -> u32 {
        self.peek()
            .or_else(|| self.toks.last())
            .map_or(0, |t| t.line)
    }
    fn is_local(&self, n: &str) -> bool {
        self.locals.iter().any(|s| s.contains(n))
    }

    /// Skips to the end of the current statement (after an error).
    fn recover(&mut self) {
        let mut depth = 0i32;
        while let Some(t) = self.peek().cloned() {
            match t.tok {
                Tok::Punct("{") => depth += 1,
                Tok::Punct("}") => {
                    if depth == 0 {
                        return;
                    }
                    depth -= 1;
                }
                Tok::Punct(";") if depth == 0 => {
                    self.pos += 1;
                    return;
                }
                _ => {}
            }
            self.pos += 1;
            if t.eol && depth == 0 {
                return;
            }
        }
    }

    // --- expressions --------------------------------------------------------

    /// A word as a value.
    fn word(&self, n: &str) -> String {
        match n {
            "true" => "true".into(),
            "false" => "false".into(),
            _ if self.is_local(n) => local(n),
            _ if self.decls.global_names.contains(n) => format!("V[{}]", lua_str(n)),
            _ => format!("W({})", lua_str(n)),
        }
    }

    fn call(&self, name: &str, args: &[String]) -> String {
        let table = if self.decls.funcs.contains(name) {
            "F"
        } else {
            "C"
        };
        format!("{table}[{}]({})", lua_str(name), args.join(", "))
    }

    fn expr(&mut self) -> Result<String, String> {
        self.or_expr()
    }
    fn or_expr(&mut self) -> Result<String, String> {
        let mut l = self.and_expr()?;
        while self.is_word("or") || self.is_punct("||") {
            self.pos += 1;
            let r = self.and_expr()?;
            l = format!("(T({l}) or T({r}))");
        }
        Ok(l)
    }
    fn and_expr(&mut self) -> Result<String, String> {
        let mut l = self.not_expr()?;
        while self.is_word("and") || self.is_punct("&&") {
            self.pos += 1;
            let r = self.not_expr()?;
            l = format!("(T({l}) and T({r}))");
        }
        Ok(l)
    }
    fn not_expr(&mut self) -> Result<String, String> {
        if self.eat_punct("!") {
            let e = self.not_expr()?;
            return Ok(format!("(not T({e}))"));
        }
        self.cmp_expr()
    }
    fn cmp_expr(&mut self) -> Result<String, String> {
        let l = self.add_expr()?;
        let op = match self.peek().map(|t| &t.tok) {
            Some(Tok::Ident(w)) if w == "eq" => "EQ",
            Some(Tok::Ident(w)) if w == "ne" => "NE",
            Some(Tok::Punct("==")) => "EQ",
            Some(Tok::Punct("!=")) => "NE",
            Some(Tok::Punct("<")) => "LT",
            Some(Tok::Punct(">")) => "GT",
            Some(Tok::Punct("<=")) => "LE",
            Some(Tok::Punct(">=")) => "GE",
            _ => return Ok(l),
        };
        self.pos += 1;
        let r = self.add_expr()?;
        Ok(format!("{op}({l}, {r})"))
    }
    fn add_expr(&mut self) -> Result<String, String> {
        let mut l = self.unary()?;
        loop {
            let op = if self.is_punct("+") {
                "ADD"
            } else if self.is_punct("-") {
                "SUB"
            } else {
                break;
            };
            self.pos += 1;
            let r = self.unary()?;
            l = format!("{op}({l}, {r})");
        }
        Ok(l)
    }
    fn unary(&mut self) -> Result<String, String> {
        if self.eat_punct("-") {
            let e = self.unary()?;
            return Ok(format!("SUB(0, {e})"));
        }
        self.primary()
    }
    fn primary(&mut self) -> Result<String, String> {
        let t = self.next().ok_or("unexpected end of file")?;
        Ok(match t.tok {
            Tok::Int(v) => v.to_string(),
            Tok::Float(v) => format!("{v:?}"),
            Tok::Str(s) => lua_str(&s),
            Tok::Punct("(") => {
                let e = self.expr()?;
                self.expect_punct(")")?;
                format!("({e})")
            }
            Tok::Ident(n) => {
                if self.is_punct("(") && !self.is_local(&n) && !self.decls.global_names.contains(&n)
                {
                    let args = self.paren_args()?;
                    self.call(&n, &args)
                } else if t.eol
                    && self.decls.funcs.contains(&n)
                    && !self.is_local(&n)
                    && !self.decls.global_names.contains(&n)
                {
                    // A function's name ending its line is a call.
                    format!("F[{}]()", lua_str(&n))
                } else {
                    self.word(&n)
                }
            }
            other => return Err(format!("unexpected {other:?}")),
        })
    }

    /// `( a, b, … )`
    fn paren_args(&mut self) -> Result<Vec<String>, String> {
        self.expect_punct("(")?;
        let mut args = Vec::new();
        if self.eat_punct(")") {
            return Ok(args);
        }
        loop {
            args.push(self.expr()?);
            if self.eat_punct(")") {
                return Ok(args);
            }
            self.expect_punct(",")?;
        }
    }

    /// Shell-style arguments: words / numbers / strings up to the end of the
    /// line (or `;` / `}`).
    fn shell_args(&mut self, head_eol: bool) -> Result<Vec<String>, String> {
        let mut args = Vec::new();
        if head_eol {
            return Ok(args);
        }
        while let Some(t) = self.peek().cloned() {
            if matches!(t.tok, Tok::Punct(";") | Tok::Punct("}")) {
                break;
            }
            let arg = if t.tok == Tok::Punct("-") {
                self.pos += 1;
                let n = self.next().ok_or("dangling '-'")?;
                let end = n.eol;
                let v = match n.tok {
                    Tok::Int(v) => format!("{}", -v),
                    Tok::Float(v) => format!("{:?}", -v),
                    other => return Err(format!("'-' before {other:?}")),
                };
                args.push(v);
                if end {
                    break;
                }
                continue;
            } else {
                self.pos += 1;
                match t.tok {
                    Tok::Int(v) => v.to_string(),
                    Tok::Float(v) => format!("{v:?}"),
                    Tok::Str(s) => lua_str(&s),
                    // Shell-style, a word is always its own spelling.
                    Tok::Ident(n) if n == "true" || n == "false" => n,
                    Tok::Ident(n) => lua_str(&n),
                    Tok::Punct(",") => {
                        if t.eol {
                            break;
                        }
                        continue;
                    }
                    other => return Err(format!("unexpected {other:?} in arguments")),
                }
            };
            args.push(arg);
            if t.eol {
                break;
            }
        }
        Ok(args)
    }

    // --- statements -------------------------------------------------------

    fn emit(&mut self, indent: usize, s: &str) {
        for _ in 0..indent {
            self.out.push_str("  ");
        }
        self.out.push_str(s);
        self.out.push('\n');
    }

    fn block(&mut self, indent: usize) -> Result<(), String> {
        self.expect_punct("{")?;
        self.locals.push(HashSet::new());
        while !self.is_punct("}") {
            if self.peek().is_none() {
                return Err("missing '}'".into());
            }
            self.statement(indent)?;
        }
        self.pos += 1;
        self.locals.pop();
        Ok(())
    }

    /// A statement, or on failure an `ERR` call in its place.
    fn statement(&mut self, indent: usize) -> Result<(), String> {
        let start = self.pos;
        let line = self.line();
        let mark = self.out.len();
        match self.statement_inner(indent) {
            Ok(()) => Ok(()),
            Err(e) if self.peek().is_some() => {
                self.out.truncate(mark);
                self.pos = start;
                self.recover();
                if self.pos == start {
                    self.pos += 1;
                }
                let msg = format!("{}:{line}: {e}", self.file);
                self.emit(
                    indent,
                    &format!(
                        "ERR({}, {})",
                        lua_str(&format!("{}:{line}", self.file)),
                        lua_str(&e)
                    ),
                );
                self.errors.push(msg);
                Ok(())
            }
            Err(e) => Err(e),
        }
    }

    fn statement_inner(&mut self, indent: usize) -> Result<(), String> {
        let t = self.peek().cloned().ok_or("unexpected end")?;
        match &t.tok {
            Tok::Punct(";") => {
                self.pos += 1;
                Ok(())
            }
            Tok::Punct("{") => {
                self.emit(indent, "do");
                self.block(indent + 1)?;
                self.emit(indent, "end");
                Ok(())
            }
            Tok::Ident(w) if w == "if" => {
                self.pos += 1;
                self.expect_punct("(")?;
                let c = self.expr()?;
                self.expect_punct(")")?;
                self.emit(indent, &format!("if T({c}) then"));
                self.body(indent + 1)?;
                if self.is_word("else") {
                    self.pos += 1;
                    self.emit(indent, "else");
                    self.body(indent + 1)?;
                }
                self.emit(indent, "end");
                Ok(())
            }
            Tok::Ident(w) if w == "sleep" => {
                self.pos += 1;
                let e = self.expr()?;
                self.eat_punct(";");
                self.emit(indent, &format!("SLEEP({e})"));
                Ok(())
            }
            Tok::Ident(w) if w == "return" => {
                self.pos += 1;
                let value = if t.eol || self.is_punct(";") || self.is_punct("}") {
                    None
                } else {
                    Some(self.expr()?)
                };
                self.eat_punct(";");
                // (`do … end` keeps a mid-block return legal Lua.)
                match value {
                    Some(v) => self.emit(indent, &format!("do return {v} end")),
                    None => self.emit(indent, "do return end"),
                }
                Ok(())
            }
            Tok::Ident(w) if w == "fork" => {
                self.pos += 1;
                let name = match self.next().map(|t| t.tok) {
                    Some(Tok::Ident(n)) => n,
                    other => return Err(format!("fork of {other:?}")),
                };
                let head_eol = self.toks[self.pos - 1].eol;
                let args = if self.is_punct("(") {
                    self.paren_args()?
                } else {
                    self.shell_args(head_eol)?
                };
                self.eat_punct(";");
                let f = if self.decls.funcs.contains(&name) {
                    format!("F[{}]", lua_str(&name))
                } else {
                    format!("C[{}]", lua_str(&name))
                };
                let mut all = vec![f];
                all.extend(args);
                self.emit(indent, &format!("FORK({})", all.join(", ")));
                Ok(())
            }
            Tok::Ident(w) if w == "var" || TYPES.contains(&w.as_str()) => {
                // A local declaration.
                self.pos += 1;
                let mut ty = w.clone();
                if w == "var" {
                    if let Some(Tok::Ident(t2)) = self.peek().map(|t| t.tok.clone()) {
                        if TYPES.contains(&t2.as_str()) {
                            ty = t2;
                            self.pos += 1;
                        }
                    }
                }
                let name = match self.next().map(|t| t.tok) {
                    Some(Tok::Ident(n)) => n,
                    other => return Err(format!("declaration of {other:?}")),
                };
                let init = if self.eat_punct("=") {
                    self.expr()?
                } else {
                    default_of(&ty).to_string()
                };
                self.eat_punct(";");
                self.locals
                    .last_mut()
                    .expect("in a block")
                    .insert(name.clone());
                self.emit(indent, &format!("local {} = {init}", local(&name)));
                Ok(())
            }
            Tok::Ident(name) => {
                let name = name.clone();
                self.pos += 1;
                if self.eat_punct("=") {
                    let e = self.expr()?;
                    self.eat_punct(";");
                    let target = if self.is_local(&name) {
                        local(&name)
                    } else if self.decls.global_names.contains(&name) {
                        format!("V[{}]", lua_str(&name))
                    } else {
                        format!("G[{}]", lua_str(&name))
                    };
                    self.emit(indent, &format!("{target} = {e}"));
                    return Ok(());
                }
                let args = if self.is_punct("(") && !t.eol {
                    let a = self.paren_args()?;
                    // `cmd(a) b` — trailing shell words are rare; take them.
                    if !self.toks[self.pos - 1].eol && !self.is_punct(";") && !self.is_punct("}") {
                        let mut more = self.shell_args(false)?;
                        let mut a = a;
                        a.append(&mut more);
                        a
                    } else {
                        a
                    }
                } else {
                    self.shell_args(t.eol)?
                };
                self.eat_punct(";");
                let call = self.call(&name, &args);
                self.emit(indent, &call);
                Ok(())
            }
            other => Err(format!("unexpected {other:?}")),
        }
    }

    /// The body of an `if` / `else`: a block or a single statement.
    fn body(&mut self, indent: usize) -> Result<(), String> {
        if self.is_punct("{") {
            self.block(indent)
        } else {
            self.locals.push(HashSet::new());
            let r = self.statement(indent);
            self.locals.pop();
            r
        }
    }

    // --- top level -----------------------------------------------------------

    /// `func <type> name(<params>) { … }`
    fn func(&mut self) -> Result<(), String> {
        let line = self.line();
        self.pos += 1; // func
        if let Some(Tok::Ident(t)) = self.peek().map(|t| t.tok.clone()) {
            if TYPES.contains(&t.as_str())
                && matches!(self.peek_at(1).map(|t| &t.tok), Some(Tok::Ident(_)))
            {
                self.pos += 1;
            }
        }
        let name = match self.next().map(|t| t.tok) {
            Some(Tok::Ident(n)) => n,
            other => return Err(format!("func name {other:?}")),
        };
        let mut params = Vec::new();
        if self.eat_punct("(") {
            while !self.eat_punct(")") {
                match self.next().map(|t| t.tok) {
                    Some(Tok::Ident(w)) if TYPES.contains(&w.as_str()) => {
                        if let Some(Tok::Ident(p)) = self.peek().map(|t| t.tok.clone()) {
                            if !TYPES.contains(&p.as_str()) {
                                self.pos += 1;
                                params.push(p);
                            }
                        }
                    }
                    Some(Tok::Ident(p)) => params.push(p),
                    Some(Tok::Punct(",")) => {}
                    other => return Err(format!("parameter {other:?}")),
                }
            }
        }
        self.locals = vec![params.iter().cloned().collect()];
        let list: Vec<String> = params.iter().map(|p| local(p)).collect();
        self.emit(0, &format!("-- {}:{line}", self.file));
        self.emit(
            0,
            &format!("F[{}] = function({})", lua_str(&name), list.join(", ")),
        );
        self.block(1)?;
        self.emit(0, "end");
        self.locals.clear();
        Ok(())
    }
}

/// Collects a file's global variables and functions (first pass).
fn declare(toks: &[Token], decls: &mut Decls, file: &str, errors: &mut Vec<String>) {
    let mut depth = 0i32;
    let mut i = 0;
    while i < toks.len() {
        match &toks[i].tok {
            Tok::Punct("{") => depth += 1,
            Tok::Punct("}") => depth -= 1,
            Tok::Ident(w) if depth == 0 && w == "func" => {
                // func [type] name
                let mut j = i + 1;
                if let (Some(Tok::Ident(t)), Some(Tok::Ident(_))) =
                    (toks.get(j).map(|t| &t.tok), toks.get(j + 1).map(|t| &t.tok))
                {
                    if TYPES.contains(&t.as_str()) {
                        j += 1;
                    }
                }
                if let Some(Tok::Ident(n)) = toks.get(j).map(|t| &t.tok) {
                    decls.funcs.insert(n.clone());
                }
            }
            Tok::Ident(w)
                if depth == 0 && (w == "var" || (TYPES.contains(&w.as_str()) && w != "void")) =>
            {
                let mut j = i + 1;
                let mut ty = w.clone();
                if w == "var" {
                    if let Some(Tok::Ident(t)) = toks.get(j).map(|t| &t.tok) {
                        if TYPES.contains(&t.as_str()) {
                            ty = t.clone();
                            j += 1;
                        }
                    }
                }
                match toks.get(j).map(|t| &t.tok) {
                    Some(Tok::Ident(n)) => {
                        // An initial value: a literal (possibly negative).
                        let init = match (
                            toks.get(j + 1).map(|t| &t.tok),
                            toks.get(j + 2).map(|t| &t.tok),
                        ) {
                            (Some(Tok::Punct("=")), Some(Tok::Int(v))) => Some(v.to_string()),
                            (Some(Tok::Punct("=")), Some(Tok::Float(v))) => Some(format!("{v:?}")),
                            (Some(Tok::Punct("=")), Some(Tok::Str(s))) => Some(lua_str(s)),
                            (Some(Tok::Punct("=")), Some(Tok::Ident(b)))
                                if b == "true" || b == "false" =>
                            {
                                Some(b.clone())
                            }
                            (Some(Tok::Punct("=")), Some(Tok::Punct("-"))) => {
                                match toks.get(j + 3).map(|t| &t.tok) {
                                    Some(Tok::Int(v)) => Some(format!("{}", -v)),
                                    Some(Tok::Float(v)) => Some(format!("{:?}", -v)),
                                    _ => None,
                                }
                            }
                            (Some(Tok::Punct("=")), other) => {
                                errors.push(format!(
                                    "{file}:{}: global {n} = {other:?}",
                                    toks[j].line
                                ));
                                None
                            }
                            _ => None,
                        };
                        if decls.global_names.insert(n.clone()) {
                            decls.globals.push((n.clone(), ty, init));
                        }
                    }
                    other => {
                        errors.push(format!("{file}:{}: declaration of {other:?}", toks[i].line))
                    }
                }
            }
            _ => {}
        }
        i += 1;
    }
}

/// A translated level: the Lua chunk, the lines that could not be
/// translated, and the script functions defined.
pub struct Translation {
    pub lua: String,
    pub errors: Vec<String>,
    pub functions: usize,
}

/// Translates all of a level's script files (`(file name, text)`, in load
/// order) into one Lua chunk.
pub fn translate(files: &[(String, String)]) -> Translation {
    let mut decls = Decls::default();
    let mut errors = Vec::new();
    let mut lexed = Vec::new();
    for (name, text) in files {
        match lex(text) {
            Ok(t) => {
                declare(&t, &mut decls, name, &mut errors);
                lexed.push((name.as_str(), t));
            }
            Err(e) => errors.push(format!("{name}: {e}")),
        }
    }
    let mut out = String::new();
    out.push_str("-- Translated from the level's BSL scripts by oni-convert. Do not edit.\n");
    out.push_str(
        "local V, F, C, G, W, T, EQ, NE, LT, GT, LE, GE, ADD, SUB, SLEEP, FORK, ERR = ...\n",
    );
    for (n, ty, init) in &decls.globals {
        let v = init.clone().unwrap_or_else(|| default_of(ty).to_string());
        let _ = writeln!(out, "V[{}] = {v}", lua_str(n));
    }
    let mut seen_funcs: HashMap<String, String> = HashMap::new();
    for (file, toks) in &lexed {
        let mut g = Gen {
            toks,
            pos: 0,
            file,
            decls: &decls,
            locals: Vec::new(),
            out: String::new(),
            errors: Vec::new(),
        };
        let mut depth_errors = 0;
        while g.pos < toks.len() {
            let t = toks[g.pos].clone();
            match &t.tok {
                Tok::Ident(w) if w == "func" => {
                    let mut k = g.pos + 1;
                    if let (Some(Tok::Ident(ty)), Some(Tok::Ident(_))) =
                        (toks.get(k).map(|t| &t.tok), toks.get(k + 1).map(|t| &t.tok))
                    {
                        if TYPES.contains(&ty.as_str()) {
                            k += 1;
                        }
                    }
                    if let Some(Tok::Ident(n)) = toks.get(k).map(|t| &t.tok) {
                        if let Some(prev) = seen_funcs.insert(n.clone(), file.to_string()) {
                            g.errors.push(format!(
                                "{file}:{}: function {n} also defined in {prev}",
                                t.line
                            ));
                        }
                    }
                    let mark = g.out.len();
                    if let Err(e) = g.func() {
                        g.out.truncate(mark);
                        g.errors.push(format!("{file}:{}: {e}", t.line));
                        depth_errors += 1;
                        // Skip to the next top-level `func`.
                        while g.pos < toks.len()
                            && !matches!(&toks[g.pos].tok, Tok::Ident(w) if w == "func")
                        {
                            g.pos += 1;
                        }
                    }
                }
                Tok::Ident(w) if w == "var" || (TYPES.contains(&w.as_str()) && w != "void") => {
                    // A global (declared in the first pass): skip it.
                    g.pos += 1;
                    while g.pos < toks.len() {
                        let t = &toks[g.pos];
                        g.pos += 1;
                        if t.tok == Tok::Punct(";") || t.eol {
                            break;
                        }
                    }
                }
                _ => {
                    g.errors.push(format!(
                        "{file}:{}: unexpected {:?} at top level",
                        t.line, t.tok
                    ));
                    g.pos += 1;
                }
            }
        }
        let _ = depth_errors;
        out.push_str(&g.out);
        errors.append(&mut g.errors);
    }
    Translation {
        lua: out,
        errors,
        functions: seen_funcs.len(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tr(src: &str) -> Translation {
        translate(&[("t.bsl".into(), src.into())])
    }

    #[test]
    fn functions_globals_and_calls() {
        let t = tr("var int count = 2;\nvar bool done;\nfunc void go(string who)\n{\n  cmd_one who 3 -1 thing\n  cmd_two(who, 1.5, \"str\")\n  count = count - 1\n  helper\n}\nfunc void helper(void)\n{\n  sleep 30\n  fork go me\n}\n");
        assert!(t.errors.is_empty(), "{:?}", t.errors);
        let l = &t.lua;
        assert!(l.contains("V[\"count\"] = 2"));
        assert!(l.contains("V[\"done\"] = false"));
        assert!(l.contains("F[\"go\"] = function(l_who)"));
        // Shell-style words are literal (even a parameter's name).
        assert!(
            l.contains("C[\"cmd_one\"](\"who\", 3, -1, \"thing\")"),
            "{l}"
        );
        assert!(l.contains("C[\"cmd_two\"](l_who, 1.5, \"str\")"), "{l}");
        assert!(l.contains("V[\"count\"] = SUB(V[\"count\"], 1)"), "{l}");
        assert!(l.contains("F[\"helper\"]()"), "{l}");
        assert!(l.contains("SLEEP(30)"));
        assert!(l.contains("FORK(F[\"go\"], \"me\")"), "{l}");
        assert_eq!(t.functions, 2);
    }

    #[test]
    fn conditions() {
        let t = tr("var int n;\nfunc void f(void)\n{\n  if (n eq 0) { a }\n  if(probe(3) ne 1 and !n) b\n  if (n < 5 or n > 9)\n  {\n    c\n  }\n  else\n  {\n    d\n  }\n}\n");
        assert!(t.errors.is_empty(), "{:?}", t.errors);
        let l = &t.lua;
        assert!(l.contains("if T(EQ(V[\"n\"], 0)) then"), "{l}");
        assert!(
            l.contains("if T((T(NE(C[\"probe\"](3), 1)) and T((not T(V[\"n\"]))))) then"),
            "{l}"
        );
        assert!(
            l.contains("if T((T(LT(V[\"n\"], 5)) or T(GT(V[\"n\"], 9)))) then"),
            "{l}"
        );
        assert!(l.contains("else"), "{l}");
    }

    #[test]
    fn engine_variables_and_bare_words() {
        let t = tr("func void main(void)\n{\n  fog_level=.15\n  x = save_slot;\n  sleep f60\n  set_thing(lamp_1, on, off);\n}\n");
        assert!(t.errors.is_empty(), "{:?}", t.errors);
        let l = &t.lua;
        assert!(l.contains("G[\"fog_level\"] = 0.15"), "{l}");
        assert!(l.contains("G[\"x\"] = W(\"save_slot\")"), "{l}");
        assert!(l.contains("SLEEP(60)"), "{l}");
        assert!(
            l.contains("C[\"set_thing\"](W(\"lamp_1\"), W(\"on\"), W(\"off\"))"),
            "{l}"
        );
    }

    #[test]
    fn numbers_lex_as_the_original_does() {
        let t = tr("var int n;\nfunc void get(void)\n{\n  n = 1\n}\nfunc void f(void)\n{\n  a -2 .5 5-3 f12\n  n = n - 1\n  n = get\n  b(n, get)\n}\n");
        assert!(t.errors.is_empty(), "{:?}", t.errors);
        let l = &t.lua;
        assert!(l.contains("C[\"a\"](-2, 0.5, 5, 12)"), "{l}");
        assert!(l.contains("V[\"n\"] = SUB(V[\"n\"], 1)"), "{l}");
        // A function's name ending its line is a call; elsewhere a word.
        assert!(l.contains("V[\"n\"] = F[\"get\"]()"), "{l}");
        assert!(l.contains("C[\"b\"](V[\"n\"], W(\"get\"))"), "{l}");
    }

    #[test]
    fn bad_lines_become_errors_and_the_rest_survives() {
        let t = tr("func void f(void)\n{\n  ok_one\n  weird @ thing\n  ok_two\n}\n");
        assert_eq!(t.errors.len(), 1, "{:?}", t.errors);
    }

    #[test]
    fn statements_end_at_newlines_or_semicolons() {
        let t = tr("func void f(void)\n{\n  a 1; b 2\n  c\n  d(1);\n}\n");
        assert!(t.errors.is_empty(), "{:?}", t.errors);
        for s in ["C[\"a\"](1)", "C[\"b\"](2)", "C[\"c\"]()", "C[\"d\"](1)"] {
            assert!(t.lua.contains(s), "{s} in {}", t.lua);
        }
    }
}
