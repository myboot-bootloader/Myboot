//! A deliberately small TOML reader for the subset MyBoot's config uses:
//! `key = value` pairs, `[section]` and `[[array-of-tables]]` headers, string /
//! integer / boolean values, `#` comments. No external crate (we are no_std and
//! want zero parser surface we don't control). TOTAL over any input: malformed
//! lines yield `Err`, never a panic.
extern crate alloc;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Value {
    Str(String),
    Int(i64),
    Bool(bool),
}

impl Value {
    pub fn as_str(&self) -> Option<&str> {
        if let Value::Str(s) = self {
            Some(s)
        } else {
            None
        }
    }
    pub fn as_int(&self) -> Option<i64> {
        if let Value::Int(i) = self {
            Some(*i)
        } else {
            None
        }
    }
    pub fn as_bool(&self) -> Option<bool> {
        if let Value::Bool(b) = self {
            Some(*b)
        } else {
            None
        }
    }
}

/// One key/value pair within a table.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pair {
    pub key: String,
    pub value: Value,
}

/// A named table (`[section]`) or an element of an array-of-tables (`[[x]]`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Table {
    pub name: String,
    pub array_element: bool,
    pub pairs: Vec<Pair>,
}

impl Table {
    pub fn get(&self, key: &str) -> Option<&Value> {
        self.pairs.iter().find(|p| p.key == key).map(|p| &p.value)
    }
}

/// The parsed document: a root table plus named sub-tables (in file order).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Document {
    pub root: Vec<Pair>,
    pub tables: Vec<Table>,
}

impl Document {
    /// All tables (array or single) with the given name, in file order.
    pub fn tables_named<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a Table> {
        self.tables.iter().filter(move |t| t.name == name)
    }
    pub fn root_get(&self, key: &str) -> Option<&Value> {
        self.root.iter().find(|p| p.key == key).map(|p| &p.value)
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct ParseError {
    pub line: usize,
    pub msg: &'static str,
}

/// Parse a TOML-subset document. Total; reports the first offending line.
pub fn parse(input: &str) -> Result<Document, ParseError> {
    let mut doc = Document::default();
    let mut current: Option<usize> = None; // index into doc.tables, or None = root

    for (i, raw) in input.lines().enumerate() {
        let line = strip_comment(raw).trim();
        if line.is_empty() {
            continue;
        }

        if let Some(rest) = line.strip_prefix("[[") {
            let name = rest.strip_suffix("]]").ok_or(ParseError {
                line: i + 1,
                msg: "unterminated [[table]]",
            })?;
            doc.tables.push(Table {
                name: name.trim().to_string(),
                array_element: true,
                pairs: Vec::new(),
            });
            current = Some(doc.tables.len() - 1);
        } else if let Some(rest) = line.strip_prefix('[') {
            let name = rest.strip_suffix(']').ok_or(ParseError {
                line: i + 1,
                msg: "unterminated [table]",
            })?;
            doc.tables.push(Table {
                name: name.trim().to_string(),
                array_element: false,
                pairs: Vec::new(),
            });
            current = Some(doc.tables.len() - 1);
        } else {
            let (k, v) = split_kv(line).ok_or(ParseError {
                line: i + 1,
                msg: "expected key = value",
            })?;
            let value = parse_value(v).ok_or(ParseError {
                line: i + 1,
                msg: "bad value",
            })?;
            let pair = Pair {
                key: k.to_string(),
                value,
            };
            match current {
                Some(idx) => doc.tables[idx].pairs.push(pair),
                None => doc.root.push(pair),
            }
        }
    }
    Ok(doc)
}

fn strip_comment(line: &str) -> &str {
    // A '#' outside of a quoted string starts a comment. We honour quotes.
    let bytes = line.as_bytes();
    let mut in_str = false;
    for (i, &b) in bytes.iter().enumerate() {
        match b {
            b'"' => in_str = !in_str,
            b'#' if !in_str => return &line[..i],
            _ => {}
        }
    }
    line
}

fn split_kv(line: &str) -> Option<(&str, &str)> {
    let eq = line.find('=')?;
    let k = line[..eq].trim();
    let v = line[eq + 1..].trim();
    if k.is_empty() || v.is_empty() {
        return None;
    }
    Some((k, v))
}

fn parse_value(v: &str) -> Option<Value> {
    if let Some(inner) = v.strip_prefix('"').and_then(|s| s.strip_suffix('"')) {
        return Some(Value::Str(unescape(inner)));
    }
    match v {
        "true" => return Some(Value::Bool(true)),
        "false" => return Some(Value::Bool(false)),
        _ => {}
    }
    v.parse::<i64>().ok().map(Value::Int)
}

fn unescape(s: &str) -> String {
    // Support the escapes our config actually needs, incl. `\\` for EFI paths.
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('\\') => out.push('\\'),
                Some('"') => out.push('"'),
                Some('n') => out.push('\n'),
                Some('t') => out.push('\t'),
                Some(other) => {
                    out.push('\\');
                    out.push(other);
                }
                None => out.push('\\'),
            }
        } else {
            out.push(c);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_sections_pairs_and_arrays() {
        let doc = parse(
            r#"
            # top-level config
            default = "nixos.current"
            timeout = 5

            [transaction]
            confirm = true
            max_tries = 2

            [[entry]]
            id = "windows"
            loader = "\\EFI\\Microsoft\\Boot\\bootmgfw.efi"

            [[entry]]
            id = "nixos"
        "#,
        )
        .unwrap();

        assert_eq!(
            doc.root_get("default").unwrap().as_str().unwrap(),
            "nixos.current"
        );
        assert_eq!(doc.root_get("timeout").unwrap().as_int().unwrap(), 5);
        let tx = doc.tables_named("transaction").next().unwrap();
        assert_eq!(tx.get("confirm").unwrap().as_bool().unwrap(), true);
        let entries: alloc::vec::Vec<_> = doc.tables_named("entry").collect();
        assert_eq!(entries.len(), 2);
        assert_eq!(
            entries[0].get("loader").unwrap().as_str().unwrap(),
            "\\EFI\\Microsoft\\Boot\\bootmgfw.efi"
        );
    }

    #[test]
    fn comments_and_quotes_coexist() {
        let doc = parse("name = \"a # b\"   # trailing comment\n").unwrap();
        assert_eq!(doc.root_get("name").unwrap().as_str().unwrap(), "a # b");
    }

    #[test]
    fn malformed_lines_error_not_panic() {
        assert!(parse("[unterminated\n").is_err());
        assert!(parse("no_equals_here\n").is_err());
        assert!(parse("k = \n").is_err());
        // arbitrary bytes must never panic
        for len in 0..80usize {
            let s: String = (0..len).map(|x| ((x * 7) % 128) as u8 as char).collect();
            let _ = parse(&s);
        }
    }
}
