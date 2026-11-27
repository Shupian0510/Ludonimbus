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

/// Deepest nesting read. Steam's files go about a dozen levels deep; one
/// nested far deeper is damaged, and reading it all would run out of stack
/// (which ends the program instead of failing).
const MAX_DEPTH: usize = 64;

fn too_deep() -> anyhow::Error {
    anyhow!(crate::tr!("VDF 文件嵌套太深", "The VDF file is nested too deeply"))
}

pub fn parse(bytes: &[u8]) -> Result<Value> {
    let mut pos = 0;
    parse_map(bytes, &mut pos, 0)
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

fn parse_map(b: &[u8], pos: &mut usize, depth: usize) -> Result<Value> {
    if depth > MAX_DEPTH {
        return Err(too_deep());
    }
    let mut entries = Vec::new();
    while *pos < b.len() {
        let t = b[*pos];
        *pos += 1;
        if t == T_END {
            break;
        }
        let key = read_cstr(b, pos)?;
        let val = match t {
            T_MAP => parse_map(b, pos, depth + 1)?,
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
        self.entries().iter().find(|(k, _)| k.eq_ignore_ascii_case(key.as_bytes())).map(|(_, v)| v)
    }

    pub fn get_mut(&mut self, key: &str) -> Option<&mut Value> {
        self.entries_mut()?.iter_mut().find(|(k, _)| k.eq_ignore_ascii_case(key.as_bytes())).map(|(_, v)| v)
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
            Text::Map(e) => e.iter().find(|(k, _)| k.eq_ignore_ascii_case(key)).map(|(_, v)| v),
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
        } else if c == '/' && it.clone().nth(1) == Some('/') {
            // `//` comment (a single `/` starts an unquoted string)
            for c in it.by_ref() {
                if c == '\n' {
                    break;
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
    Ok(Text::Map(parse_text_map(&toks, &mut pos, 0)?))
}

fn parse_text_map(toks: &[Tok], pos: &mut usize, depth: usize) -> Result<Vec<(String, Text)>> {
    if depth > MAX_DEPTH {
        return Err(too_deep());
    }
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
                        out.push((key, Text::Map(parse_text_map(toks, pos, depth + 1)?)));
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
    Ok(out)
}

// ---------------------------------------------------------------------------
// Changing one value of a text KeyValues file in place
//
// Steam keeps settings of its own next to the ones Ludonimbus changes
// (`localconfig.vdf`), in a format that differs between versions in small
// ways (key case, order, line endings). So a value is changed by replacing
// its bytes, and nothing else of the file is written anew.

/// A token of text KeyValues and where it is (byte offsets), read by the
/// same rules as [`parse_text`].
#[derive(Debug)]
enum Spanned {
    /// A string, unescaped, and the bytes it was read from (quotes included).
    Str(String, usize, usize),
    Open(usize),
    Close(usize),
}

fn tokenize_spans(s: &str) -> Vec<Spanned> {
    let mut out = Vec::new();
    let mut it = s.char_indices().peekable();
    while let Some(&(at, c)) = it.peek() {
        if c.is_whitespace() {
            it.next();
        } else if c == '/' && it.clone().nth(1).map(|(_, c)| c) == Some('/') {
            for (_, c) in it.by_ref() {
                if c == '\n' {
                    break;
                }
            }
        } else if c == '{' {
            it.next();
            out.push(Spanned::Open(at));
        } else if c == '}' {
            it.next();
            out.push(Spanned::Close(at));
        } else if c == '"' {
            it.next();
            let mut buf = String::new();
            let mut end = s.len();
            while let Some((i, c)) = it.next() {
                match c {
                    '"' => {
                        end = i + 1;
                        break;
                    }
                    '\\' => match it.next() {
                        Some((_, 'n')) => buf.push('\n'),
                        Some((_, 't')) => buf.push('\t'),
                        Some((_, o)) => buf.push(o),
                        None => break,
                    },
                    o => buf.push(o),
                }
            }
            out.push(Spanned::Str(buf, at, end));
        } else {
            let mut buf = String::new();
            let mut end = s.len();
            while let Some(&(i, c)) = it.peek() {
                if c.is_whitespace() || c == '{' || c == '}' || c == '"' {
                    end = i;
                    break;
                }
                buf.push(c);
                it.next();
            }
            out.push(Spanned::Str(buf, at, end));
        }
    }
    out
}

/// A key and what it holds, with where both are in the file.
#[derive(Debug)]
struct Node {
    key: String,
    /// Where the key's token starts.
    start: usize,
    body: Body,
}

#[derive(Debug)]
enum Body {
    /// The value's token (quotes included).
    Value(usize, usize),
    /// Where its `{` and `}` are.
    Block { open: usize, close: usize, children: Vec<Node> },
}

/// Read the entries of one block (the whole file at the top, `depth` 0).
/// Strict: a file Ludonimbus doesn't understand completely is not changed.
fn parse_nodes(toks: &[Spanned], pos: &mut usize, depth: usize) -> Result<(Vec<Node>, usize)> {
    if depth > MAX_DEPTH {
        return Err(too_deep());
    }
    let top = depth == 0;
    let mut out = Vec::new();
    loop {
        match toks.get(*pos) {
            None if top => return Ok((out, usize::MAX)),
            None => bail!("a block is not closed"),
            Some(Spanned::Close(at)) if !top => {
                *pos += 1;
                return Ok((out, *at));
            }
            Some(Spanned::Close(_)) => bail!("a block is closed that was never opened"),
            Some(Spanned::Open(_)) => bail!("a block has no name"),
            Some(Spanned::Str(key, start, _)) => {
                *pos += 1;
                let body = match toks.get(*pos) {
                    Some(Spanned::Open(open)) => {
                        *pos += 1;
                        let (children, close) = parse_nodes(toks, pos, depth + 1)?;
                        Body::Block { open: *open, close, children }
                    }
                    Some(Spanned::Str(_, s, e)) => {
                        *pos += 1;
                        Body::Value(*s, *e)
                    }
                    _ => bail!("\"{key}\" has no value"),
                };
                out.push(Node { key: key.clone(), start: *start, body });
            }
        }
    }
}

/// Whether `s` is text KeyValues that every key has a value or a block in,
/// with every block closed.
pub fn text_well_formed(s: &str) -> bool {
    parse_nodes(&tokenize_spans(s), &mut 0, 0).is_ok()
}

/// A string as Steam writes it in text KeyValues: `\` and `"` escaped.
pub fn quote_text(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

/// The value at `path` (keys matched without regard to case, the first of
/// equal keys taken, as [`Text::get`] does).
pub fn text_value<'a>(doc: &'a Text, path: &[&str]) -> Option<&'a str> {
    path.iter().try_fold(doc, |t, k| t.get(k))?.as_str()
}

/// `text` with the value at `path` set (`None`: removed), every other byte
/// as it was. A value that is there is replaced in place, a new one goes in
/// as the first entry of its block, and blocks of the path that are missing
/// are added, in Steam's own layout (tabs, the file's line endings). Keys
/// are matched without regard to case, the first of equal keys taken.
/// Fails for a file that isn't well formed, or whose path leads through a
/// value; nothing is changed then.
pub fn set_text_value(text: &str, path: &[&str], value: Option<&str>) -> Result<String> {
    let Some((leaf, blocks)) = path.split_last() else { bail!("no key to change") };
    let toks = tokenize_spans(text);
    let (top, _) = parse_nodes(&toks, &mut 0, 0).map_err(|e| anyhow!(crate::tr!("文件格式无法识别（{e}）", "The file's format isn't understood ({e})")))?;
    // The block that holds the next key, and how deep the path exists.
    let mut level: &[Node] = &top;
    let mut holder: Option<&Node> = None;
    let mut depth = 0;
    while depth < blocks.len() {
        let Some(node) = level.iter().find(|n| n.key.eq_ignore_ascii_case(blocks[depth])) else { break };
        let Body::Block { children, .. } = &node.body else {
            bail!(crate::tr!("「{}」不是一个块", "\"{}\" isn't a block", blocks[depth]));
        };
        holder = Some(node);
        level = children;
        depth += 1;
    }
    if depth == blocks.len() {
        if let Some(node) = level.iter().find(|n| n.key.eq_ignore_ascii_case(leaf)) {
            let Body::Value(s, e) = node.body else {
                bail!(crate::tr!("「{leaf}」是一个块，不是值", "\"{leaf}\" is a block, not a value"));
            };
            return Ok(match value {
                Some(v) => format!("{}{}{}", &text[..s], quote_text(v), &text[e..]),
                None => remove_entry(text, node.start, e),
            });
        }
    }
    // Nothing there to replace or remove.
    let Some(value) = value else { return Ok(text.to_string()) };
    let eol = if text.contains("\r\n") { "\r\n" } else { "\n" };
    let Some(holder) = holder else {
        // Not even the outermost block: it goes at the end of the file.
        let sep = if text.is_empty() || text.ends_with('\n') { "" } else { eol };
        return Ok(format!("{text}{sep}{}", entry_text(blocks, leaf, value, "", eol)));
    };
    let Body::Block { open, close, children } = &holder.body else { unreachable!("checked above") };
    let indent = children
        .first()
        .map(|c| &text[line_start(text, c.start)..c.start])
        .filter(|i| i.chars().all(|c| c == ' ' || c == '\t'))
        .map(str::to_string)
        .unwrap_or_else(|| format!("{}\t", leading_space(text, *open)));
    let new = entry_text(&blocks[depth..], leaf, value, &indent, eol);
    // As the block's first entry: on the line after its `{`, or right after
    // the `{` when the block ends on that line.
    let at = match text[*open..].find('\n').map(|i| open + i + 1) {
        Some(at) if at <= *close => at,
        _ => open + 1,
    };
    let lead = if text[..at].ends_with('\n') { "" } else { eol };
    Ok(format!("{}{lead}{new}{}", &text[..at], &text[at..]))
}

/// The entry `leaf` = `value` inside the blocks `blocks`, as Steam lays it
/// out: one key per line, `{` and `}` on lines of their own, two tabs
/// between a key and its value.
fn entry_text(blocks: &[&str], leaf: &str, value: &str, indent: &str, eol: &str) -> String {
    match blocks.split_first() {
        None => format!("{indent}{}\t\t{}{eol}", quote_text(leaf), quote_text(value)),
        Some((name, rest)) => {
            format!("{indent}{}{eol}{indent}{{{eol}{}{indent}}}{eol}", quote_text(name), entry_text(rest, leaf, value, &format!("{indent}\t"), eol))
        }
    }
}

/// Where the line holding byte `at` starts.
fn line_start(text: &str, at: usize) -> usize {
    text[..at].rfind('\n').map_or(0, |i| i + 1)
}

/// The spaces and tabs the line holding byte `at` starts with.
fn leading_space(text: &str, at: usize) -> &str {
    let start = line_start(text, at);
    let line = &text[start..];
    &line[..line.len() - line.trim_start_matches([' ', '\t']).len()]
}

/// `text` without the entry from `start` to `end`: its whole line when it
/// has one of its own.
fn remove_entry(text: &str, start: usize, end: usize) -> String {
    let from = line_start(text, start);
    let to = text[end..].find('\n').map_or(text.len(), |i| end + i + 1);
    let alone = text[from..start].trim().is_empty() && text[end..to].trim().is_empty();
    if alone {
        format!("{}{}", &text[..from], &text[to..])
    } else {
        format!("{}{}", &text[..start], &text[end..])
    }
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

    const PATH: [&str; 7] = ["UserLocalConfigStore", "Software", "Valve", "Steam", "apps", "1000", "LaunchOptions"];

    /// A `localconfig.vdf` the way Steam writes it, with `eol` line endings.
    fn localconfig(app: &str, eol: &str) -> String {
        [
            "\"UserLocalConfigStore\"",
            "{",
            "\t\"friends\"",
            "\t{",
            "\t\t\"1000\"",
            "\t\t{",
            "\t\t\t\"LaunchOptions\"\t\t\"not an app\"",
            "\t\t}",
            "\t}",
            "\t\"Software\"",
            "\t{",
            "\t\t\"Valve\"",
            "\t\t{",
            "\t\t\t\"Steam\"",
            "\t\t\t{",
            "\t\t\t\t\"apps\"",
            "\t\t\t\t{",
            "\t\t\t\t\t\"220\"",
            "\t\t\t\t\t{",
            "\t\t\t\t\t\t\"LastPlayed\"\t\t\"1700000000\"",
            "\t\t\t\t\t}",
            app,
            "\t\t\t\t}",
            "\t\t\t}",
            "\t\t}",
            "\t}",
            "}",
            "",
        ]
        .join(eol)
    }

    fn app_block(lines: &[&str]) -> String {
        let mut out = vec!["\t\t\t\t\t\"1000\"".to_string(), "\t\t\t\t\t{".to_string()];
        out.extend(lines.iter().map(|l| format!("\t\t\t\t\t\t{l}")));
        out.push("\t\t\t\t\t}".to_string());
        out.join("\n")
    }

    fn value(text: &str) -> Option<String> {
        text_value(&parse_text(text).unwrap(), &PATH).map(str::to_string)
    }

    #[test]
    fn a_value_is_replaced_in_place() {
        for eol in ["\n", "\r\n"] {
            let app =
                app_block(&["\"LastPlayed\"\t\t\"1\"", "\"LaunchOptions\"\t\t\"-novid\"", "\"cloud\"", "{", "\t\"last_sync_state\"\t\t\"synchronized\"", "}"])
                    .replace('\n', eol);
            let before = localconfig(&app, eol);
            // A Windows path in quotes: written escaped, read back as it was.
            let wanted = r#""C:\Tools\ludonimbus-run.exe" --game fe -- %command% -config "C:\My Games\a.cfg""#;
            let after = set_text_value(&before, &PATH, Some(wanted)).unwrap();
            assert_eq!(value(&after).as_deref(), Some(wanted));
            assert_eq!(after, before.replace("\"-novid\"", &quote_text(wanted)));
            assert!(after.contains(r#""\"C:\\Tools\\ludonimbus-run.exe\" --game"#));
            // The block elsewhere that is named like the app is not touched.
            assert!(after.contains("\"not an app\""));
            // And back, byte for byte.
            assert_eq!(set_text_value(&after, &PATH, Some("-novid")).unwrap(), before);
        }
    }

    #[test]
    fn a_new_value_goes_in_and_out_byte_for_byte() {
        for eol in ["\n", "\r\n"] {
            let before = localconfig(&app_block(&["\"LastPlayed\"\t\t\"1\""]).replace('\n', eol), eol);
            assert_eq!(value(&before), None);
            let after = set_text_value(&before, &PATH, Some("-x \"y\"")).unwrap();
            let expected = before.replace(
                &format!("\t\t\t\t\t{{{eol}\t\t\t\t\t\t\"LastPlayed\"\t\t\"1\""),
                &format!("\t\t\t\t\t{{{eol}\t\t\t\t\t\t\"LaunchOptions\"\t\t\"-x \\\"y\\\"\"{eol}\t\t\t\t\t\t\"LastPlayed\"\t\t\"1\""),
            );
            assert_eq!(after, expected);
            assert_eq!(value(&after).as_deref(), Some("-x \"y\""));
            assert!(text_well_formed(&after));
            // Removed again: the file as it was.
            assert_eq!(set_text_value(&after, &PATH, None).unwrap(), before);
            // Removing what isn't there changes nothing.
            assert_eq!(set_text_value(&before, &PATH, None).unwrap(), before);
        }
    }

    #[test]
    fn missing_blocks_are_added_in_steams_layout() {
        // No block for the app.
        let before = localconfig("", "\n").replace("\n\n", "\n");
        let after = set_text_value(&before, &PATH, Some("-novid")).unwrap();
        let block = "\t\t\t\t\t\"1000\"\n\t\t\t\t\t{\n\t\t\t\t\t\t\"LaunchOptions\"\t\t\"-novid\"\n\t\t\t\t\t}\n";
        assert_eq!(after, before.replace("\t\t\t\t{\n\t\t\t\t\t\"220\"", &format!("\t\t\t\t{{\n{block}\t\t\t\t\t\"220\"")));
        assert_eq!(value(&after).as_deref(), Some("-novid"));
        // No `apps` block either, in a file with Windows line endings and
        // other key case.
        let before = "\"UserLocalConfigStore\"\r\n{\r\n\t\"software\"\r\n\t{\r\n\t\t\"valve\"\r\n\t\t{\r\n\t\t\t\"Steam\"\r\n\t\t\t{\r\n\t\t\t}\r\n\t\t}\r\n\t}\r\n}\r\n";
        let after = set_text_value(before, &PATH, Some("-novid")).unwrap();
        assert_eq!(
            after,
            before.replace(
                "\t\t\t\"Steam\"\r\n\t\t\t{\r\n",
                "\t\t\t\"Steam\"\r\n\t\t\t{\r\n\t\t\t\t\"apps\"\r\n\t\t\t\t{\r\n\t\t\t\t\t\"1000\"\r\n\t\t\t\t\t{\r\n\t\t\t\t\t\t\"LaunchOptions\"\t\t\"-novid\"\r\n\t\t\t\t\t}\r\n\t\t\t\t}\r\n"
            )
        );
        assert_eq!(value(&after).as_deref(), Some("-novid"));
        // A block that ends on its own line.
        let after = set_text_value("\"UserLocalConfigStore\" { \"Software\" { } }", &PATH, Some("a")).unwrap();
        assert_eq!(value(&after).as_deref(), Some("a"));
        assert!(text_well_formed(&after));
    }

    #[test]
    fn keys_in_other_case_are_the_same_keys() {
        let before = localconfig(&app_block(&["\"launchoptions\"\t\t\"-a\""]), "\n").replace("\"apps\"", "\"Apps\"");
        let after = set_text_value(&before, &PATH, Some("-b")).unwrap();
        assert_eq!(after, before.replace("\"-a\"", "\"-b\""));
    }

    #[test]
    fn files_that_arent_understood_are_left_alone() {
        assert!(set_text_value("\"UserLocalConfigStore\" { \"Software\" {", &PATH, Some("a")).is_err());
        assert!(set_text_value("\"UserLocalConfigStore\" { } }", &PATH, Some("a")).is_err());
        assert!(set_text_value("\"UserLocalConfigStore\" { \"Software\" }", &PATH, Some("a")).is_err());
        // The path goes through a value.
        assert!(set_text_value("\"UserLocalConfigStore\" { \"Software\" \"x\" }", &PATH, Some("a")).is_err());
        assert!(!text_well_formed("\"a\" {"));
        assert!(text_well_formed("// comment\n\"a\" { \"b\" \"c\" }"));
    }

    #[test]
    fn text_parse() {
        let t = parse_text(r#""users" { "7656" { "AccountName" "me" "PersonaName" "Me \"Q\"" } }"#).unwrap();
        let u = t.get("users").unwrap().get("7656").unwrap();
        assert_eq!(u.get("PersonaName").unwrap().as_str(), Some("Me \"Q\""));
    }

    #[test]
    fn deep_nesting_fails_instead_of_overflowing_the_stack() {
        // Binary: a map in a map in a map…, two bytes a level.
        let deep = [T_MAP, 0].repeat(200_000);
        assert!(parse(&deep).is_err());
        let ok = [[T_MAP, 0].repeat(MAX_DEPTH), vec![T_END; MAX_DEPTH + 1]].concat();
        assert!(parse(&ok).is_ok());
        // Text.
        let deep = "\"a\" {".repeat(200_000);
        assert!(parse_text(&deep).is_err());
        assert!(!text_well_formed(&format!("{deep}{}", "}".repeat(200_000))));
        assert!(set_text_value(&deep, &["a", "b"], Some("c")).is_err());
        let ok = format!("{}\"k\" \"v\"{}", "\"a\" {".repeat(MAX_DEPTH), "}".repeat(MAX_DEPTH));
        assert!(parse_text(&ok).is_ok() && text_well_formed(&ok));
    }

    #[test]
    fn a_lone_slash_is_part_of_a_string() {
        let t = parse_text("\"Path\" /usr/lib/steam // a comment\n\"Next\" \"x\"").unwrap();
        assert_eq!(t.get("Path").unwrap().as_str(), Some("/usr/lib/steam"));
        assert_eq!(t.get("Next").unwrap().as_str(), Some("x"));
        // Changed in place as a whole, not leaving its first `/` behind.
        let after = set_text_value("\"a\"\n{\n\t\"Path\"\t\t/old\n}\n", &["a", "Path"], Some("new")).unwrap();
        assert_eq!(after, "\"a\"\n{\n\t\"Path\"\t\t\"new\"\n}\n");
    }
}
