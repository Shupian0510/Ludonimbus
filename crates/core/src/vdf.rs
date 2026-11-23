//! Valve KeyValues: binary format (`shortcuts.vdf`) and text format
//! (`loginusers.vdf`).
//!
//! Binary strings are kept as raw bytes so files round-trip byte-for-byte,
//! even when they contain invalid UTF-8.

use anyhow::{anyhow, bail, Result};

#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Map(Vec<(Vec<u8>, Value)>),
    Str(Vec<u8>),
    Int(i32),
    Float(f32),
    U64(u64),
}

const T_MAP: u8 = 0x00;
const T_STR: u8 = 0x01;
const T_INT: u8 = 0x02;
const T_FLOAT: u8 = 0x03;
const T_U64: u8 = 0x07;
const T_END: u8 = 0x08;

pub fn parse(bytes: &[u8]) -> Result<Value> {
    let mut pos = 0;
    parse_map(bytes, &mut pos)
}

fn need(b: &[u8], pos: usize, n: usize) -> Result<()> {
    if pos + n > b.len() {
        bail!(crate::tr!("VDF 文件意外结束", "The VDF file ended unexpectedly"));
    }
    Ok(())
}

fn read_cstr(b: &[u8], pos: &mut usize) -> Result<Vec<u8>> {
    let start = *pos;
    let end = b[start..]
        .iter()
        .position(|&c| c == 0)
        .map(|i| start + i)
        .ok_or_else(|| anyhow!(crate::tr!("VDF 字符串没有结束符", "A VDF string has no terminator")))?;
    *pos = end + 1;
    Ok(b[start..end].to_vec())
}

fn parse_map(b: &[u8], pos: &mut usize) -> Result<Value> {
    let mut entries = Vec::new();
    while *pos < b.len() {
        let t = b[*pos];
        *pos += 1;
        if t == T_END {
            break;
        }
        let key = read_cstr(b, pos)?;
        let val = match t {
            T_MAP => parse_map(b, pos)?,
            T_STR => Value::Str(read_cstr(b, pos)?),
            T_INT => {
                need(b, *pos, 4)?;
                let v = i32::from_le_bytes(b[*pos..*pos + 4].try_into()?);
                *pos += 4;
                Value::Int(v)
            }
            T_FLOAT => {
                need(b, *pos, 4)?;
                let v = f32::from_le_bytes(b[*pos..*pos + 4].try_into()?);
                *pos += 4;
                Value::Float(v)
            }
            T_U64 => {
                need(b, *pos, 8)?;
                let v = u64::from_le_bytes(b[*pos..*pos + 8].try_into()?);
                *pos += 8;
                Value::U64(v)
            }
            other => bail!(crate::tr!("不支持的 VDF 类型 0x{other:02x}", "Unsupported VDF type 0x{other:02x}")),
        };
        entries.push((key, val));
    }
    Ok(Value::Map(entries))
}

pub fn write(v: &Value) -> Vec<u8> {
    let mut out = Vec::new();
    if let Value::Map(entries) = v {
        write_entries(entries, &mut out);
    }
    out
}

fn write_entries(entries: &[(Vec<u8>, Value)], out: &mut Vec<u8>) {
    for (k, v) in entries {
        let t = match v {
            Value::Map(_) => T_MAP,
            Value::Str(_) => T_STR,
            Value::Int(_) => T_INT,
            Value::Float(_) => T_FLOAT,
            Value::U64(_) => T_U64,
        };
        out.push(t);
        out.extend_from_slice(k);
        out.push(0);
        match v {
            Value::Map(inner) => write_entries(inner, out),
            Value::Str(s) => {
                out.extend_from_slice(s);
                out.push(0);
            }
            Value::Int(i) => out.extend_from_slice(&i.to_le_bytes()),
            Value::Float(f) => out.extend_from_slice(&f.to_le_bytes()),
            Value::U64(u) => out.extend_from_slice(&u.to_le_bytes()),
        }
    }
    out.push(T_END);
}

impl Value {
    pub fn entries(&self) -> &[(Vec<u8>, Value)] {
        match self {
            Value::Map(e) => e,
            _ => &[],
        }
    }

    pub fn entries_mut(&mut self) -> Option<&mut Vec<(Vec<u8>, Value)>> {
        match self {
            Value::Map(e) => Some(e),
            _ => None,
        }
    }

    /// Case-insensitive key lookup.
    pub fn get(&self, key: &str) -> Option<&Value> {
        self.entries()
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(key.as_bytes()))
            .map(|(_, v)| v)
    }

    pub fn get_mut(&mut self, key: &str) -> Option<&mut Value> {
        self.entries_mut()?
            .iter_mut()
            .find(|(k, _)| k.eq_ignore_ascii_case(key.as_bytes()))
            .map(|(_, v)| v)
    }

    /// Set a string value, keeping the existing key's spelling if present.
    pub fn set_str(&mut self, key: &str, value: &str) {
        if let Some(v) = self.get_mut(key) {
            *v = Value::Str(value.as_bytes().to_vec());
        } else if let Some(e) = self.entries_mut() {
            e.push((key.as_bytes().to_vec(), Value::Str(value.as_bytes().to_vec())));
        }
    }

    pub fn str_lossy(&self) -> Option<String> {
        match self {
            Value::Str(s) => Some(String::from_utf8_lossy(s).into_owned()),
            _ => None,
        }
    }

    pub fn int(&self) -> Option<i32> {
        match self {
            Value::Int(i) => Some(*i),
            _ => None,
        }
    }

    pub fn get_str(&self, key: &str) -> String {
        self.get(key).and_then(Value::str_lossy).unwrap_or_default()
    }
}

// ---------------------------------------------------------------------------
// Text KeyValues

#[derive(Clone, Debug, PartialEq)]
pub enum Text {
    Map(Vec<(String, Text)>),
    Str(String),
}

impl Text {
    pub fn get(&self, key: &str) -> Option<&Text> {
        match self {
            Text::Map(e) => e
                .iter()
                .find(|(k, _)| k.eq_ignore_ascii_case(key))
                .map(|(_, v)| v),
            _ => None,
        }
    }
    pub fn entries(&self) -> &[(String, Text)] {
        match self {
            Text::Map(e) => e,
            _ => &[],
        }
    }
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Text::Str(s) => Some(s),
            _ => None,
        }
    }
}

#[derive(Debug, PartialEq)]
enum Tok {
    Str(String),
    Open,
    Close,
}

fn tokenize(s: &str) -> Vec<Tok> {
    let mut out = Vec::new();
    let mut it = s.chars().peekable();
    while let Some(&c) = it.peek() {
        if c.is_whitespace() {
            it.next();
        } else if c == '/' {
            // `//` comment
            it.next();
            if it.peek() == Some(&'/') {
                for c in it.by_ref() {
                    if c == '\n' {
                        break;
                    }
                }
            }
        } else if c == '{' {
            it.next();
            out.push(Tok::Open);
        } else if c == '}' {
            it.next();
            out.push(Tok::Close);
        } else if c == '"' {
            it.next();
            let mut buf = String::new();
            while let Some(c) = it.next() {
                match c {
                    '"' => break,
                    '\\' => match it.next() {
                        Some('n') => buf.push('\n'),
                        Some('t') => buf.push('\t'),
                        Some(o) => buf.push(o),
                        None => break,
                    },
                    o => buf.push(o),
                }
            }
            out.push(Tok::Str(buf));
        } else {
            let mut buf = String::new();
            while let Some(&c) = it.peek() {
                if c.is_whitespace() || c == '{' || c == '}' || c == '"' {
                    break;
                }
                buf.push(c);
                it.next();
            }
            out.push(Tok::Str(buf));
        }
    }
    out
}

pub fn parse_text(s: &str) -> Result<Text> {
    let toks = tokenize(s);
    let mut pos = 0;
    Ok(Text::Map(parse_text_map(&toks, &mut pos)))
}

fn parse_text_map(toks: &[Tok], pos: &mut usize) -> Vec<(String, Text)> {
    let mut out = Vec::new();
    while *pos < toks.len() {
        match &toks[*pos] {
            Tok::Close => {
                *pos += 1;
                break;
            }
            Tok::Open => {
                *pos += 1;
            }
            Tok::Str(k) => {
                let key = k.clone();
                *pos += 1;
                match toks.get(*pos) {
                    Some(Tok::Open) => {
                        *pos += 1;
                        out.push((key, Text::Map(parse_text_map(toks, pos))));
                    }
                    Some(Tok::Str(v)) => {
                        out.push((key, Text::Str(v.clone())));
                        *pos += 1;
                    }
                    _ => break,
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn binary_roundtrip() {
        let v = Value::Map(vec![(
            b"shortcuts".to_vec(),
            Value::Map(vec![(
                b"0".to_vec(),
                Value::Map(vec![
                    (b"appid".to_vec(), Value::Int(-123)),
                    (b"AppName".to_vec(), Value::Str(vec![0xEF, 0xBF, 0xBD, b'x'])),
                    (b"tags".to_vec(), Value::Map(vec![])),
                ]),
            )]),
        )]);
        let bytes = write(&v);
        assert_eq!(parse(&bytes).unwrap(), v);
        assert_eq!(write(&parse(&bytes).unwrap()), bytes);
    }

    #[test]
    fn text_parse() {
        let t = parse_text(
            r#""users" { "7656" { "AccountName" "me" "PersonaName" "Me \"Q\"" } }"#,
        )
        .unwrap();
        let u = t.get("users").unwrap().get("7656").unwrap();
        assert_eq!(u.get("PersonaName").unwrap().as_str(), Some("Me \"Q\""));
    }
}
