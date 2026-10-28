//! A deliberately small parser for the TOML subset ARES needs.
//!
//! Supported:
//! - `# comments`, blank lines
//! - `[section]` headers (bare names only, no dotted or nested tables)
//! - `key = "string"` with `\\ \" \n \t` escapes
//! - `key = 123` (non-negative or negative integers)
//! - `key = true` or `key = false` (lowercase only, as in TOML)
//! - `key = ["a", "b"]` arrays of strings, single or multi line, trailing
//!   comma allowed
//!
//! Anything else is an error with a line number. It is not a general TOML
//! parser, and that is intentional: fewer features, fewer surprises, no
//! dependencies.

use std::collections::HashSet;
use std::error::Error;
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value {
    Str(String),
    Int(i64),
    Bool(bool),
    StrArray(Vec<String>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// Section name, or an empty string before any `[section]` header.
    pub section: String,
    pub key: String,
    pub value: Value,
    /// One-based line number where the entry starts.
    pub line: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    pub line: usize,
    pub message: String,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "line {}: {}", self.line, self.message)
    }
}

impl Error for ParseError {}

fn perr(line: usize, message: impl Into<String>) -> ParseError {
    ParseError {
        line,
        message: message.into(),
    }
}

/// Remove a trailing `# comment`, ignoring `#` inside quoted strings.
fn strip_comment(line: &str) -> &str {
    let mut in_str = false;
    let mut escaped = false;
    for (i, c) in line.char_indices() {
        if in_str {
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == '"' {
                in_str = false;
            }
        } else if c == '"' {
            in_str = true;
        } else if c == '#' {
            return &line[..i];
        }
    }
    line
}

fn is_bare_key(s: &str) -> bool {
    !s.is_empty()
        && s
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

/// Parse a quoted string at the start of `s`. Returns the value and the rest.
fn parse_string(s: &str) -> Result<(String, &str), String> {
    let mut chars = s.char_indices();
    match chars.next() {
        Some((_, '"')) => {}
        _ => return Err("expected a quoted string".to_string()),
    }
    let mut out = String::new();
    let mut escaped = false;
    for (i, c) in chars {
        if escaped {
            match c {
                '\\' => out.push('\\'),
                '"' => out.push('"'),
                'n' => out.push('\n'),
                't' => out.push('\t'),
                other => return Err(format!("unsupported escape \\{other}")),
            }
            escaped = false;
        } else if c == '\\' {
            escaped = true;
        } else if c == '"' {
            return Ok((out, &s[i + 1..]));
        } else {
            out.push(c);
        }
    }
    Err("unterminated string".to_string())
}

fn parse_array(s: &str) -> Result<Vec<String>, String> {
    let mut rest = s.trim_start();
    rest = rest.strip_prefix('[').ok_or("expected '['")?;
    let mut items = Vec::new();
    loop {
        rest = rest.trim_start();
        if let Some(after) = rest.strip_prefix(']') {
            if !after.trim().is_empty() {
                return Err("unexpected text after array".to_string());
            }
            return Ok(items);
        }
        let (item, after) = parse_string(rest)?;
        items.push(item);
        rest = after.trim_start();
        if let Some(after) = rest.strip_prefix(',') {
            rest = after;
        } else if !rest.starts_with(']') {
            return Err("expected ',' or ']' in array".to_string());
        }
    }
}

fn parse_value(s: &str) -> Result<Value, String> {
    let s = s.trim();
    if s.starts_with('[') {
        parse_array(s).map(Value::StrArray)
    } else if s.starts_with('"') {
        let (v, rest) = parse_string(s)?;
        if !rest.trim().is_empty() {
            return Err("unexpected text after string".to_string());
        }
        Ok(Value::Str(v))
    } else if s == "true" {
        Ok(Value::Bool(true))
    } else if s == "false" {
        Ok(Value::Bool(false))
    } else {
        s.parse::<i64>().map(Value::Int).map_err(|_| {
            format!(
                "unsupported value {s:?} (expected string, integer, true, false, or array of strings)"
            )
        })
    }
}

/// True when every `[` opened outside a string has been closed.
fn array_complete(s: &str) -> bool {
    let mut depth = 0i32;
    let mut in_str = false;
    let mut escaped = false;
    for c in s.chars() {
        if in_str {
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == '"' {
                in_str = false;
            }
        } else if c == '"' {
            in_str = true;
        } else if c == '[' {
            depth += 1;
        } else if c == ']' {
            depth -= 1;
        }
    }
    depth <= 0
}

pub fn parse(input: &str) -> Result<Vec<Entry>, ParseError> {
    let mut entries = Vec::new();
    let mut seen: HashSet<(String, String)> = HashSet::new();
    let mut section = String::new();
    let mut lines = input.lines().enumerate();

    while let Some((idx, raw)) = lines.next() {
        let lineno = idx + 1;
        let line = strip_comment(raw).trim();
        if line.is_empty() {
            continue;
        }

        if let Some(rest) = line.strip_prefix('[') {
            let name = rest
                .strip_suffix(']')
                .ok_or_else(|| perr(lineno, "section header is missing ']'"))?
                .trim();
            if !is_bare_key(name) {
                return Err(perr(lineno, format!("invalid section name {name:?}")));
            }
            section = name.to_string();
            continue;
        }

        let (key, value_text) = line
            .split_once('=')
            .ok_or_else(|| perr(lineno, "expected 'key = value'"))?;
        let key = key.trim();
        if !is_bare_key(key) {
            return Err(perr(lineno, format!("invalid key {key:?}")));
        }

        let mut value_text = value_text.trim().to_string();
        if value_text.starts_with('[') {
            while !array_complete(&value_text) {
                match lines.next() {
                    Some((_, more)) => {
                        value_text.push(' ');
                        value_text.push_str(strip_comment(more).trim());
                    }
                    None => return Err(perr(lineno, "unterminated array")),
                }
            }
        }

        let value = parse_value(&value_text).map_err(|m| perr(lineno, m))?;
        if !seen.insert((section.clone(), key.to_string())) {
            return Err(perr(lineno, format!("duplicate key {key:?}")));
        }
        entries.push(Entry {
            section: section.clone(),
            key: key.to_string(),
            value,
            line: lineno,
        });
    }
    Ok(entries)
}
