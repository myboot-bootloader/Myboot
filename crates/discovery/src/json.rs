//! A tiny, panic-free JSON reader for the subset the NixOS bootspec uses
//! (objects, arrays, strings, numbers, booleans, null). No external crate: we
//! stay no_std with zero parser surface we do not control, matching the
//! discipline of the TOML reader in `config`. TOTAL over any input.
extern crate alloc;
use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

#[derive(Clone, Debug, PartialEq)]
pub enum Json {
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    Arr(Vec<Json>),
    Obj(BTreeMap<String, Json>),
}

impl Json {
    pub fn as_str(&self) -> Option<&str> {
        if let Json::Str(s) = self {
            Some(s)
        } else {
            None
        }
    }
    pub fn as_array(&self) -> Option<&[Json]> {
        if let Json::Arr(a) = self {
            Some(a)
        } else {
            None
        }
    }
    pub fn get(&self, key: &str) -> Option<&Json> {
        if let Json::Obj(m) = self {
            m.get(key)
        } else {
            None
        }
    }
    /// Navigate nested objects by a path of keys.
    pub fn path(&self, keys: &[&str]) -> Option<&Json> {
        let mut cur = self;
        for k in keys {
            cur = cur.get(k)?;
        }
        Some(cur)
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct JsonError;

/// Parse a complete JSON document. Total; malformed input yields `Err`.
pub fn parse(input: &str) -> Result<Json, JsonError> {
    let bytes = input.as_bytes();
    let mut p = Parser { b: bytes, i: 0 };
    p.skip_ws();
    let v = p.value()?;
    p.skip_ws();
    if p.i != p.b.len() {
        return Err(JsonError);
    } // trailing garbage
    Ok(v)
}

struct Parser<'a> {
    b: &'a [u8],
    i: usize,
}

impl<'a> Parser<'a> {
    fn peek(&self) -> Option<u8> {
        self.b.get(self.i).copied()
    }
    fn bump(&mut self) -> Option<u8> {
        let c = self.peek();
        if c.is_some() {
            self.i += 1;
        }
        c
    }
    fn skip_ws(&mut self) {
        while let Some(c) = self.peek() {
            if c == b' ' || c == b'\t' || c == b'\n' || c == b'\r' {
                self.i += 1;
            } else {
                break;
            }
        }
    }

    fn value(&mut self) -> Result<Json, JsonError> {
        self.skip_ws();
        match self.peek() {
            Some(b'{') => self.object(),
            Some(b'[') => self.array(),
            Some(b'"') => Ok(Json::Str(self.string()?)),
            Some(b't') | Some(b'f') => self.boolean(),
            Some(b'n') => self.null(),
            Some(c) if c == b'-' || c.is_ascii_digit() => self.number(),
            _ => Err(JsonError),
        }
    }

    fn object(&mut self) -> Result<Json, JsonError> {
        self.expect(b'{')?;
        let mut map = BTreeMap::new();
        self.skip_ws();
        if self.peek() == Some(b'}') {
            self.i += 1;
            return Ok(Json::Obj(map));
        }
        loop {
            self.skip_ws();
            let key = self.string()?;
            self.skip_ws();
            self.expect(b':')?;
            let val = self.value()?;
            map.insert(key, val);
            self.skip_ws();
            match self.bump() {
                Some(b',') => continue,
                Some(b'}') => break,
                _ => return Err(JsonError),
            }
        }
        Ok(Json::Obj(map))
    }

    fn array(&mut self) -> Result<Json, JsonError> {
        self.expect(b'[')?;
        let mut out = Vec::new();
        self.skip_ws();
        if self.peek() == Some(b']') {
            self.i += 1;
            return Ok(Json::Arr(out));
        }
        loop {
            let v = self.value()?;
            out.push(v);
            self.skip_ws();
            match self.bump() {
                Some(b',') => continue,
                Some(b']') => break,
                _ => return Err(JsonError),
            }
        }
        Ok(Json::Arr(out))
    }

    fn string(&mut self) -> Result<String, JsonError> {
        self.expect(b'"')?;
        let mut s = String::new();
        loop {
            match self.bump() {
                Some(b'"') => break,
                Some(b'\\') => match self.bump() {
                    Some(b'"') => s.push('"'),
                    Some(b'\\') => s.push('\\'),
                    Some(b'/') => s.push('/'),
                    Some(b'n') => s.push('\n'),
                    Some(b't') => s.push('\t'),
                    Some(b'r') => s.push('\r'),
                    Some(b'u') => {
                        // read 4 hex digits → BMP code point (enough for our data)
                        let mut cp: u32 = 0;
                        for _ in 0..4 {
                            let d = self.bump().ok_or(JsonError)?;
                            cp = cp * 16 + hex(d)? as u32;
                        }
                        s.push(char::from_u32(cp).unwrap_or('\u{FFFD}'));
                    }
                    _ => return Err(JsonError),
                },
                Some(c) => {
                    // push raw UTF-8 byte(s); the input is &str so bytes are valid
                    s.push(c as char);
                }
                None => return Err(JsonError),
            }
        }
        Ok(s)
    }

    fn number(&mut self) -> Result<Json, JsonError> {
        let start = self.i;
        if self.peek() == Some(b'-') {
            self.i += 1;
        }
        while let Some(c) = self.peek() {
            if c.is_ascii_digit() || c == b'.' || c == b'e' || c == b'E' || c == b'+' || c == b'-' {
                self.i += 1;
            } else {
                break;
            }
        }
        let slice = core::str::from_utf8(&self.b[start..self.i]).map_err(|_| JsonError)?;
        slice.parse::<f64>().map(Json::Num).map_err(|_| JsonError)
    }

    fn boolean(&mut self) -> Result<Json, JsonError> {
        if self.b[self.i..].starts_with(b"true") {
            self.i += 4;
            Ok(Json::Bool(true))
        } else if self.b[self.i..].starts_with(b"false") {
            self.i += 5;
            Ok(Json::Bool(false))
        } else {
            Err(JsonError)
        }
    }

    fn null(&mut self) -> Result<Json, JsonError> {
        if self.b[self.i..].starts_with(b"null") {
            self.i += 4;
            Ok(Json::Null)
        } else {
            Err(JsonError)
        }
    }

    fn expect(&mut self, c: u8) -> Result<(), JsonError> {
        if self.bump() == Some(c) {
            Ok(())
        } else {
            Err(JsonError)
        }
    }
}

fn hex(d: u8) -> Result<u8, JsonError> {
    match d {
        b'0'..=b'9' => Ok(d - b'0'),
        b'a'..=b'f' => Ok(d - b'a' + 10),
        b'A'..=b'F' => Ok(d - b'A' + 10),
        _ => Err(JsonError),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_nested_bootspec_shape() {
        let j = parse(r#"
        { "org.nixos.bootspec.v1": {
            "label": "NixOS 24.05 (Linux 6.12)",
            "kernel": "/nix/store/aaa/bzImage",
            "kernelParams": ["init=/nix/store/xxx/init", "loglevel=4"],
            "initrd": "/nix/store/bbb/initrd",
            "toplevel": "/nix/store/ccc"
          },
          "org.nixos.specialisation.v1": {
            "gaming": { "org.nixos.bootspec.v1": { "label": "gaming", "kernel": "/k", "initrd": "/i", "kernelParams": [] } }
          }
        }"#).unwrap();
        let bs = j.path(&["org.nixos.bootspec.v1"]).unwrap();
        assert_eq!(
            bs.get("kernel").unwrap().as_str().unwrap(),
            "/nix/store/aaa/bzImage"
        );
        let params = bs.get("kernelParams").unwrap().as_array().unwrap();
        assert_eq!(params.len(), 2);
        assert_eq!(params[0].as_str().unwrap(), "init=/nix/store/xxx/init");
        assert!(j.path(&["org.nixos.specialisation.v1", "gaming"]).is_some());
    }

    #[test]
    fn total_on_garbage() {
        for s in [
            "",
            "{",
            "[1,2",
            "{\"a\":}",
            "nul",
            "\"unterminated",
            "trailing 1",
        ] {
            let _ = parse(s); // must never panic
        }
        assert!(parse("{}").is_ok());
        assert!(parse("[]").is_ok());
        assert!(parse("truex").is_err()); // trailing garbage rejected
    }
}
