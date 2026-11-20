use std::fmt;
use std::path::PathBuf;

use crate::config::{Suggestions, fit_diagnostic};
use crate::theme::Theme;

/// Strips a matching quote pair without decoding escapes. Used for names and
/// directive values, whose valid vocabularies contain no escape sequences.
#[inline]
fn strip_quotes(lit: &str) -> &str {
    if ((lit.starts_with('\'') && lit.ends_with('\''))
        || (lit.starts_with('"') && lit.ends_with('"')))
        && lit.len() >= 2
    {
        return &lit[1..lit.len() - 1];
    }
    lit
}

/// Strips quotes and decodes PowerShell escapes: doubled single quotes, or a
/// backtick escape inside double quotes. Used for paths and argument values.
fn unescape_quoted(raw: &str) -> String {
    let bytes = raw.as_bytes();
    if bytes.len() < 2 {
        return raw.to_string();
    }
    let quote = bytes[0];
    if (quote != b'\'' && quote != b'"') || bytes[bytes.len() - 1] != quote {
        return raw.to_string();
    }
    let inner = &raw[1..raw.len() - 1];
    if quote == b'\'' {
        // PowerShell single-quoted strings escape ' as ''
        return inner.replace("''", "'");
    }
    // PowerShell double-quoted strings escape with backtick
    let mut out = String::with_capacity(inner.len());
    let mut chars = inner.chars();
    while let Some(ch) = chars.next() {
        if ch == '`' {
            if let Some(next) = chars.next() {
                out.push(next);
            } else {
                out.push('`');
            }
        } else {
            out.push(ch);
        }
    }
    out
}

const DIAGNOSTIC_PREFIX: &str = "~/.config/.shyena";

#[derive(Debug, PartialEq, Eq)]
pub struct ParseError {
    pub line: u32,
    pub col: u32,
    pub message: String,
}

impl ParseError {
    pub fn to_diagnostic(&self) -> String {
        fit_diagnostic(format!(
            "{DIAGNOSTIC_PREFIX}:{}:{}: error: {}",
            self.line, self.col, self.message
        ))
    }
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_diagnostic())
    }
}

impl std::error::Error for ParseError {}

#[derive(Debug, PartialEq, Eq)]
pub struct AppDef<'a> {
    pub name: &'a str,
    pub block_span: &'a str,
    pub path: PathBuf,
    pub args: Vec<String>,
}

impl<'a> AppDef<'a> {
    #[inline]
    pub fn unquoted_name(&self) -> &'a str {
        strip_quotes(self.name)
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct FunctionDef<'a> {
    pub name: &'a str,
    pub block_span: &'a str,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct ConfigAst<'a> {
    pub apps: Vec<AppDef<'a>>,
    pub functions: Vec<FunctionDef<'a>>,
    pub theme: Option<Theme>,
    pub suggestions: Option<Suggestions>,
}

/// A source position captured before consuming a token, so diagnostics point
/// at the start of the offending construct rather than the byte where parsing
/// happened to give up.
#[derive(Clone, Copy)]
struct Mark {
    pos: usize,
    line: u32,
    line_start: usize,
}

/// Zero-copy byte scanner for `.shyena` configuration files.
pub struct Scanner<'a> {
    src: &'a str,
    bytes: &'a [u8],
    pos: usize,
    line: u32,
    line_start: usize,
}

impl<'a> Scanner<'a> {
    #[inline]
    pub fn new(src: &'a str) -> Self {
        Self {
            src,
            bytes: src.as_bytes(),
            pos: 0,
            line: 1,
            line_start: 0,
        }
    }

    #[inline]
    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }

    #[inline]
    fn peek_next(&self) -> Option<u8> {
        self.bytes.get(self.pos + 1).copied()
    }

    fn advance(&mut self) -> Option<u8> {
        let b = self.peek()?;
        self.pos += 1;
        if b == b'\n' {
            self.line += 1;
            self.line_start = self.pos;
        }
        Some(b)
    }

    fn skip_whitespace_and_comments(&mut self) -> Result<(), ParseError> {
        while let Some(b) = self.peek() {
            match b {
                b' ' | b'\t' | b'\r' | b'\n' => {
                    self.advance();
                }
                b'<' if self.peek_next() == Some(b'#') => {
                    // Block comment <# ... #>. An unclosed comment would
                    // otherwise silently swallow the remainder of the file.
                    let start = self.mark();
                    self.advance(); // '<'
                    self.advance(); // '#'
                    let mut closed = false;
                    while let Some(cb) = self.peek() {
                        if cb == b'#' && self.peek_next() == Some(b'>') {
                            self.advance();
                            self.advance();
                            closed = true;
                            break;
                        }
                        self.advance();
                    }
                    if !closed {
                        return Err(Self::error_at(
                            start,
                            "unterminated block comment: missing '#>'",
                        ));
                    }
                }
                b'#' => {
                    // Line comment
                    while let Some(cb) = self.advance() {
                        if cb == b'\n' {
                            break;
                        }
                    }
                }
                _ => break,
            }
        }
        Ok(())
    }

    fn read_identifier_or_keyword(&mut self) -> &'a str {
        let start = self.pos;
        while let Some(b) = self.peek() {
            if b.is_ascii_alphanumeric() || b == b'-' || b == b'_' {
                self.advance();
            } else {
                break;
            }
        }
        &self.src[start..self.pos]
    }

    fn read_string_literal(&mut self) -> Result<&'a str, ParseError> {
        let Some(quote) = self.peek() else {
            return Err(self.make_error("expected string literal"));
        };
        if quote != b'\'' && quote != b'"' {
            return Err(self.make_error("expected string literal"));
        }
        let start = self.pos;
        self.advance();

        while let Some(b) = self.peek() {
            match b {
                // Double quote string uses backtick for escape
                b'"' if quote == b'"' => {
                    self.advance();
                    return Ok(&self.src[start..self.pos]);
                }
                b'`' if quote == b'"' => {
                    self.advance(); // consume backtick
                    self.advance(); // consume escaped char
                }
                // Single quote string uses doubled single quote for escape
                b'\'' if quote == b'\'' => {
                    self.advance();
                    if self.peek() == Some(b'\'') {
                        self.advance(); // Escaped single quote
                    } else {
                        return Ok(&self.src[start..self.pos]);
                    }
                }
                _ => {
                    self.advance();
                }
            }
        }
        Err(self.make_error("unterminated string literal"))
    }

    /// Scans a balanced `{ ... }` block, correctly ignoring braces inside strings and comments.
    fn scan_balanced_block(&mut self) -> Result<&'a str, ParseError> {
        let start = self.pos;
        if self.peek() != Some(b'{') {
            return Err(self.make_error("expected '{'"));
        }

        let mut depth: u32 = 0;

        while let Some(b) = self.peek() {
            match b {
                b'{' => {
                    depth += 1;
                    self.advance();
                }
                b'}' => {
                    depth -= 1;
                    self.advance();
                    if depth == 0 {
                        return Ok(&self.src[start..self.pos]);
                    }
                }
                b'"' | b'\'' => {
                    self.read_string_literal()?;
                }
                b'<' if self.peek_next() == Some(b'#') => {
                    self.skip_whitespace_and_comments()?;
                }
                b'#' => {
                    self.skip_whitespace_and_comments()?;
                }
                _ => {
                    self.advance();
                }
            }
        }
        Err(self.make_error("unterminated block: missing '}'"))
    }

    /// Parses the top-level directive stream into an AST.
    pub fn parse(mut self) -> Result<ConfigAst<'a>, ParseError> {
        let mut ast = ConfigAst::default();

        loop {
            self.skip_whitespace_and_comments()?;
            if self.pos >= self.bytes.len() {
                break;
            }

            let start = self.mark();
            let Some(b) = self.peek() else { break };
            if !(b.is_ascii_alphabetic() || b == b'_') {
                return Err(Self::unexpected_character(start, b));
            }

            let kw = self.read_identifier_or_keyword();
            if kw.eq_ignore_ascii_case("Set-Theme") {
                ast.theme = Some(self.parse_theme()?);
            } else if kw.eq_ignore_ascii_case("Set-Suggestions") {
                ast.suggestions = Some(self.parse_suggestions()?);
            } else if kw.eq_ignore_ascii_case("Register-App") {
                let app = self.parse_app()?;
                let name = app.unquoted_name();
                if ast
                    .apps
                    .iter()
                    .any(|a| a.unquoted_name().eq_ignore_ascii_case(name))
                {
                    return Err(Self::error_at(
                        start,
                        format!("duplicate app name '{name}'"),
                    ));
                }
                ast.apps.push(app);
            } else if kw.eq_ignore_ascii_case("function") {
                let func = self.parse_function()?;
                let name = func.name;
                if ast.functions.iter().any(|f| f.name.eq_ignore_ascii_case(name)) {
                    return Err(Self::error_at(
                        start,
                        format!("duplicate function name '{name}'"),
                    ));
                }
                ast.functions.push(func);
            } else {
                return Err(Self::error_at(start, format!("unexpected token '{kw}'")));
            }
        }

        Ok(ast)
    }

    fn parse_theme(&mut self) -> Result<Theme, ParseError> {
        self.skip_whitespace_and_comments()?;
        let start = self.mark();
        let raw = self.read_string_literal()?;
        Theme::from_config_str(strip_quotes(raw))
            .ok_or_else(|| Self::error_at(start, "invalid theme: expected 'dark' or 'light'"))
    }

    fn parse_suggestions(&mut self) -> Result<Suggestions, ParseError> {
        self.skip_whitespace_and_comments()?;
        let start = self.mark();
        let raw = self.read_string_literal()?;
        Suggestions::from_config_str(strip_quotes(raw)).ok_or_else(|| {
            Self::error_at(start, "invalid suggestions value: expected 'on' or 'off'")
        })
    }

    fn parse_app(&mut self) -> Result<AppDef<'a>, ParseError> {
        self.skip_whitespace_and_comments()?;
        let name = self.read_string_literal()?;

        self.skip_whitespace_and_comments()?;
        if self.peek() != Some(b'@') || self.peek_next() != Some(b'{') {
            return Err(self.make_error("expected '@{' after Register-App name"));
        }
        self.advance(); // '@'
        let block_start = self.pos; // include the opening '{' in the span
        self.advance(); // '{'
        let (path, args) = self.parse_app_hashtable_contents()?;

        Ok(AppDef {
            name,
            block_span: &self.src[block_start..self.pos],
            path,
            args,
        })
    }

    fn parse_function(&mut self) -> Result<FunctionDef<'a>, ParseError> {
        self.skip_whitespace_and_comments()?;
        let start = self.mark();
        let name = self.read_identifier_or_keyword();
        if name.is_empty() {
            return Err(Self::error_at(start, "expected function name"));
        }

        self.skip_whitespace_and_comments()?;
        Ok(FunctionDef {
            name,
            block_span: self.scan_balanced_block()?,
        })
    }

    fn unexpected_character(mark: Mark, b: u8) -> ParseError {
        Self::error_at(
            mark,
            format!("unexpected character '{}'", Self::token_display(b)),
        )
    }

    /// Parse inside an app hashtable after consuming the opening '{' (the '@' and '{' already consumed).
    /// Strictly allows only `Path` and `Args` keys with `=` assignment.
    fn parse_app_hashtable_contents(&mut self) -> Result<(PathBuf, Vec<String>), ParseError> {
        let mut path: Option<PathBuf> = None;
        let mut args: Option<Vec<String>> = None;

        loop {
            self.skip_whitespace_and_comments()?;
            let Some(b) = self.peek() else {
                return Err(self.make_error("unterminated hashtable: missing '}'"));
            };
            if b == b'}' {
                self.advance();
                return Ok((path.unwrap_or_default(), args.unwrap_or_default()));
            }

            let key_start = self.mark();
            if !(b.is_ascii_alphabetic() || b == b'_') {
                return Err(Self::unexpected_character(key_start, b));
            }
            let key = self.read_identifier_or_keyword();

            let is_path = key.eq_ignore_ascii_case("Path");
            let is_args = key.eq_ignore_ascii_case("Args");
            if !is_path && !is_args {
                return Err(Self::error_at(
                    key_start,
                    format!("unexpected token '{key}', expected 'Path' or 'Args'"),
                ));
            }

            self.skip_whitespace_and_comments()?;
            if self.peek() != Some(b'=') {
                return Err(self.make_error("expected '=' after key"));
            }
            self.advance(); // '='
            self.skip_whitespace_and_comments()?;

            if is_path {
                let value = self.read_hashtable_string_value()?;
                path = Some(PathBuf::from(value));
            } else if is_args {
                let value = self.read_args_value()?;
                args = Some(value);
            }
        }
    }

    fn read_hashtable_string_value(&mut self) -> Result<String, ParseError> {
        let Some(b) = self.peek() else {
            return Err(self.make_error("expected string value"));
        };
        if b == b'\'' || b == b'"' {
            let raw = self.read_string_literal()?;
            Ok(unescape_quoted(raw))
        } else {
            let start = self.pos;
            while let Some(c) = self.peek() {
                if c.is_ascii_whitespace()
                    || c == b'}'
                    || c == b'#'
                    || c == b'<'
                    || c == b';'
                    || c == b','
                {
                    break;
                }
                self.advance();
            }
            if self.pos == start {
                return Err(self.make_error("expected string value"));
            }
            Ok(self.src[start..self.pos].to_string())
        }
    }

    fn read_args_value(&mut self) -> Result<Vec<String>, ParseError> {
        let Some(b) = self.peek() else {
            return Err(self.make_error("expected Args value"));
        };
        if b == b'@' || b == b'(' {
            if b == b'@' {
                self.advance();
                if self.peek() != Some(b'(') {
                    return Err(self.make_error("expected '(' after '@' in Args"));
                }
            }
            self.advance(); // '('
            let mut values = Vec::new();
            loop {
                self.skip_whitespace_and_comments()?;
                let Some(c) = self.peek() else {
                    return Err(self.make_error("unterminated Args array: missing ')'"));
                };
                if c == b')' {
                    self.advance();
                    return Ok(values);
                }
                if c == b'}' {
                    return Err(self.make_error("unterminated Args array: missing ')'"));
                }
                if c == b',' {
                    self.advance();
                    continue;
                }
                values.push(self.read_array_element_string()?);
            }
        }

        // A single scalar is accepted outside an array; the same parser
        // handles both quoted and bare values.
        Ok(vec![self.read_hashtable_string_value()?])
    }

    fn read_array_element_string(&mut self) -> Result<String, ParseError> {
        let Some(b) = self.peek() else {
            return Err(self.make_error("expected string in Args array"));
        };
        if b == b'\'' || b == b'"' {
            let raw = self.read_string_literal()?;
            return Ok(unescape_quoted(raw));
        }
        // Strict: array elements must be quoted string literals
        Err(Self::unexpected_character(self.mark(), b))
    }

    #[inline]
    fn mark(&self) -> Mark {
        Mark {
            pos: self.pos,
            line: self.line,
            line_start: self.line_start,
        }
    }

    fn make_error(&self, message: impl Into<String>) -> ParseError {
        Self::error_at(self.mark(), message)
    }

    fn error_at(mark: Mark, message: impl Into<String>) -> ParseError {
        ParseError {
            line: mark.line,
            col: u32::try_from(mark.pos.saturating_sub(mark.line_start) + 1).unwrap_or(u32::MAX),
            message: message.into(),
        }
    }

    fn token_display(b: u8) -> String {
        match b {
            b'\n' => "\\n".to_string(),
            b'\r' => "\\r".to_string(),
            b'\t' => "\\t".to_string(),
            0x20..=0x7E => (b as char).to_string(),
            _ => format!("0x{b:02X}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ConfigReader;
    use std::fs;

    #[test]
    fn test_parse_register_app_and_function() {
        let src = r"
# shyena:v1
Register-App 'code' @{
    Path = 'C:\VSCode\Code.exe'
    Args = @('--reuse-window')
}

function prompt {
    Write-Host 'test'
}
";
        let ast = Scanner::new(src).parse().expect("parse should succeed");
        assert_eq!(ast.apps.len(), 1);
        assert_eq!(ast.apps[0].name, "'code'");
        assert_eq!(ast.apps[0].unquoted_name(), "code");
        assert_eq!(ast.apps[0].path, PathBuf::from(r"C:\VSCode\Code.exe"));
        assert_eq!(ast.apps[0].args, vec!["--reuse-window"]);
        assert!(ast.apps[0].block_span.contains("C:\\VSCode\\Code.exe"));

        assert_eq!(ast.functions.len(), 1);
        assert_eq!(ast.functions[0].name, "prompt");
        assert_eq!(ast.functions[0].block_span, "{\n    Write-Host 'test'\n}");
    }

    #[test]
    fn test_braces_in_strings_and_comments() {
        let src = r#"
Register-App "complex" @{
    Path = "C:\app.exe"
    Args = @('--flag={unclosed', 'note with } and # chars')
}
"#;
        let ast = Scanner::new(src)
            .parse()
            .expect("should parse braces in strings and comments");
        assert_eq!(ast.apps.len(), 1);
        assert_eq!(ast.apps[0].unquoted_name(), "complex");
        assert_eq!(ast.apps[0].args.len(), 2);
        assert!(ast.apps[0].block_span.ends_with('}'));
    }

    #[test]
    fn test_single_quote_escape() {
        let src = r"
Register-App 'app''name' @{
    Path = 'C:\it''s.exe'
    Args = @()
}
";
        let ast = Scanner::new(src).parse().expect("single quote escaped");
        assert_eq!(ast.apps.len(), 1);
        assert_eq!(ast.apps[0].name, "'app''name'");
        assert_eq!(ast.apps[0].path, PathBuf::from(r"C:\it's.exe"));
    }

    #[test]
    fn test_double_quote_backtick_escape() {
        let src = "Register-App \"app`\"name\" @{ Path = \"val`\"quote.exe\" Args = @() }";
        let ast = Scanner::new(src).parse().expect("backtick escaped quote");
        assert_eq!(ast.apps.len(), 1);
        assert_eq!(ast.apps[0].name, "\"app`\"name\"");
        assert_eq!(ast.apps[0].path, PathBuf::from("val\"quote.exe"));
    }

    #[test]
    fn test_missing_hashtable_error() {
        let src = "Register-App 'code' missing_at_brace";
        let err = Scanner::new(src).parse().expect_err("should fail");
        assert_eq!(err.message, "expected '@{' after Register-App name");
    }

    #[test]
    fn test_unterminated_block_error() {
        let src = "Register-App 'code' @{ Path = 'test' ";
        let err = Scanner::new(src).parse().expect_err("should fail");
        assert_eq!(err.message, "unterminated hashtable: missing '}'");
    }

    #[test]
    fn test_unknown_directive_error() {
        let src = "InvalidKeyword 'code' @{}";
        let err = Scanner::new(src).parse().expect_err("should fail");
        assert_eq!(err.message, "unexpected token 'InvalidKeyword'");
        assert_eq!(err.line, 1);
        assert_eq!(err.col, 1);
    }

    #[test]
    fn test_illegal_top_level_character() {
        let src = "Register-App 'a' @{ Path = 'a.exe' }\n!boom";
        let err = Scanner::new(src).parse().expect_err("should fail");
        assert_eq!(err.line, 2);
        assert_eq!(err.col, 1);
        assert!(err.message.contains("unexpected character"));
        assert!(err.message.contains('!'));
    }

    #[test]
    fn test_app_hashtable_rejects_unknown_keys() {
        let src = "Register-App 'a' @{ Path = 'a.exe'\n    Target = 'foo' }";
        let err = Scanner::new(src).parse().expect_err("should fail");
        assert_eq!(err.line, 2);
        assert_eq!(err.col, 5);
        assert_eq!(
            err.message,
            "unexpected token 'Target', expected 'Path' or 'Args'"
        );
    }

    #[test]
    fn test_app_hashtable_malformed_args_unclosed_array() {
        let src = "Register-App 'a' @{ Path = 'a.exe' Args = @('--x' }";
        let err = Scanner::new(src).parse().expect_err("should fail");
        assert_eq!(err.message, "unterminated Args array: missing ')'");
    }

    #[test]
    fn test_app_hashtable_malformed_args_missing_quotes() {
        let src = "Register-App 'a' @{ Path = 'a.exe' Args = @('ok', --bad) }";
        let err = Scanner::new(src).parse().expect_err("should fail");
        assert!(
            err.message.contains("unexpected character"),
            "unexpected message: {}",
            err.message
        );
        assert!(
            err.message.contains('-') || err.message.contains("'-'") || err.message.contains('-')
        );
    }

    #[test]
    fn test_error_diagnostic_format() {
        let err = ParseError {
            line: 4,
            col: 5,
            message: "unexpected token 'Exec', expected 'Path' or 'Args'".to_string(),
        };
        assert_eq!(
            err.to_diagnostic(),
            "~/.config/.shyena:4:5: error: unexpected token 'Exec', expected 'Path' or 'Args'"
        );
        assert!(err.to_diagnostic().chars().count() <= 120);
    }

    #[test]
    fn test_tricky_braces_in_strings_and_comments() {
        let src = r#"
# shyena:v1
Register-App 'code' @{
    Path = 'C:/Program Files/Code.exe'
    Args = @('--flag={unclosed', 'escaped''quote}')
}

<# Block comment with stray braces: {{{ #>
function complex_fn {
    # Line comment with stray brace: }
    $double = "escaped quote `" and brace } inside"
    $single = 'literal backtick ` and brace {'
    if ($true) {
        Write-Output "nested ok"
    }
}
"#;
        let ast = Scanner::new(src).parse().expect("valid config must parse");
        assert_eq!(ast.apps.len(), 1);
        assert_eq!(ast.apps[0].unquoted_name(), "code");
        assert_eq!(ast.apps[0].args.len(), 2);
        assert!(ast.apps[0].block_span.starts_with('{'));
        assert!(ast.apps[0].block_span.ends_with('}'));

        assert_eq!(ast.functions.len(), 1);
        assert_eq!(ast.functions[0].name, "complex_fn");
        assert!(ast.functions[0].block_span.ends_with('}'));
    }

    #[test]
    fn test_case_insensitivity_and_diagnostics() {
        let valid = "register-app \"wt\" @{ Path = 'wt.exe' }\nFUNCTION ping-host { ping 1.1.1.1 }";
        let ast = Scanner::new(valid).parse().unwrap();
        assert_eq!(ast.apps.len(), 1);
        assert_eq!(ast.functions.len(), 1);

        // Unclosed function brace on line 2
        let unclosed = "Register-App 'a' @{ Path = 'a.exe' }\nfunction broken {\n    $x = 1\n";
        let err = Scanner::new(unclosed).parse().unwrap_err();
        assert_eq!(err.message, "unterminated block: missing '}'");
        assert_eq!(err.line, 4);

        // Missing @{ hashtable
        let bad_app = "Register-App 'a' { Path = 'a.exe' }";
        let err = Scanner::new(bad_app).parse().unwrap_err();
        assert_eq!(err.message, "expected '@{' after Register-App name");
        assert_eq!(err.line, 1);
    }

    #[test]
    fn test_e2e_io_to_parser_pipeline() {
        let path = std::env::temp_dir().join(format!("shyena_e2e_{}.shyena", std::process::id()));
        let content = "\u{FEFF}Register-App 'term' @{ Path = 'wt.exe' }\nfunction hi { 'hello }' }";
        fs::write(&path, content).unwrap();

        let mut reader = ConfigReader::new(&path);
        let slice = reader.read().unwrap();
        let ast = Scanner::new(slice).parse().unwrap();

        assert_eq!(ast.apps.len(), 1);
        assert_eq!(ast.apps[0].path, PathBuf::from("wt.exe"));
        assert_eq!(ast.functions.len(), 1);

        let _ = fs::remove_file(&path);
    }

    #[test]
    fn test_unterminated_string_literal() {
        let single_err = Scanner::new("Register-App 'unterminated")
            .parse()
            .unwrap_err();
        assert_eq!(single_err.message, "unterminated string literal");

        let double_err = Scanner::new("Register-App 'a' @{ Path = \"unterminated }\n")
            .parse()
            .unwrap_err();
        assert_eq!(double_err.message, "unterminated string literal");
    }

    #[test]
    fn test_crlf_diagnostics_and_multiline() {
        let src = "Register-App 'test' @{\r\n    Path = 'a.exe'\r\n}\r\nfunction bad {\r\n";
        let err = Scanner::new(src).parse().unwrap_err();
        assert_eq!(err.message, "unterminated block: missing '}'");
        assert_eq!(err.line, 5);
    }

    #[test]
    fn test_empty_and_comment_only_sources() {
        let empty = "";
        let ast = Scanner::new(empty).parse().unwrap();
        assert!(ast.apps.is_empty() && ast.functions.is_empty());

        let comments_only = "   \r\n\t# single line\r\n<# block comment\r\nmultiline #>\r\n";
        let ast2 = Scanner::new(comments_only).parse().unwrap();
        assert!(ast2.apps.is_empty() && ast2.functions.is_empty());
    }

    #[test]
    fn test_function_identifier_variants() {
        let src = "function git-checkout_branch-1 { git checkout $args }";
        let ast = Scanner::new(src).parse().unwrap();
        assert_eq!(ast.functions.len(), 1);
        assert_eq!(ast.functions[0].name, "git-checkout_branch-1");
    }

    #[test]
    fn test_app_unquoted_name_edge_cases() {
        let app_double = AppDef {
            name: "\"app\"",
            block_span: "{}",
            path: PathBuf::new(),
            args: Vec::new(),
        };
        assert_eq!(app_double.unquoted_name(), "app");

        let app_single = AppDef {
            name: "'app'",
            block_span: "{}",
            path: PathBuf::new(),
            args: Vec::new(),
        };
        assert_eq!(app_single.unquoted_name(), "app");

        let app_empty_quote = AppDef {
            name: "''",
            block_span: "{}",
            path: PathBuf::new(),
            args: Vec::new(),
        };
        assert_eq!(app_empty_quote.unquoted_name(), "");

        let app_bare = AppDef {
            name: "bare_token",
            block_span: "{}",
            path: PathBuf::new(),
            args: Vec::new(),
        };
        assert_eq!(app_bare.unquoted_name(), "bare_token");
    }

    #[test]
    fn test_parse_set_theme() {
        let dark = "Set-Theme 'dark'\nRegister-App 'a' @{ Path = 'a.exe' }";
        let ast = Scanner::new(dark).parse().expect("dark theme parses");
        assert_eq!(ast.theme, Some(Theme::Dark));

        let light = "set-theme \"LIGHT\"\nfunction f { }";
        let ast = Scanner::new(light).parse().expect("light theme parses");
        assert_eq!(ast.theme, Some(Theme::Light));
        assert_eq!(ast.functions.len(), 1);
    }

    #[test]
    fn test_missing_set_theme_is_none() {
        let ast = Scanner::new("Register-App 'a' @{ Path = 'a.exe' }")
            .parse()
            .unwrap();
        assert_eq!(ast.theme, None);
    }

    #[test]
    fn test_invalid_set_theme_error() {
        let err = Scanner::new("Set-Theme 'mocha'").parse().unwrap_err();
        assert_eq!(err.message, "invalid theme: expected 'dark' or 'light'");
    }

    #[test]
    fn test_set_theme_requires_string_literal() {
        let err = Scanner::new("Set-Theme dark").parse().unwrap_err();
        assert_eq!(err.message, "expected string literal");
    }

    #[test]
    fn test_invalid_theme_values_are_rejected() {
        for source in [
            "Set-Theme 'mocha'",
            "set-theme \"BLUE\"",
            "Set-Theme ''",
            "Set-Theme 'dark '",
            "Set-Theme 'light-dark'",
        ] {
            let err = Scanner::new(source).parse().unwrap_err();
            assert_eq!(
                err.message, "invalid theme: expected 'dark' or 'light'",
                "source: {source}"
            );
            assert_eq!(err.line, 1);
        }

        let light = Scanner::new("Set-Theme \"LIGHT\"").parse().unwrap();
        assert_eq!(light.theme, Some(Theme::Light));
    }

    #[test]
    fn test_parse_set_suggestions() {
        let on = "Set-Suggestions 'on'\nRegister-App 'a' @{ Path = 'a.exe' }";
        let ast = Scanner::new(on).parse().expect("on parses");
        assert_eq!(ast.suggestions, Some(Suggestions::On));

        let off = "set-suggestions \"OFF\"\nfunction f { }";
        let ast = Scanner::new(off).parse().expect("off parses");
        assert_eq!(ast.suggestions, Some(Suggestions::Off));
        assert_eq!(ast.functions.len(), 1);
    }

    #[test]
    fn test_missing_set_suggestions_is_none() {
        let ast = Scanner::new("Register-App 'a' @{ Path = 'a.exe' }")
            .parse()
            .unwrap();
        assert_eq!(ast.suggestions, None);
    }

    #[test]
    fn test_invalid_suggestions_values_are_rejected() {
        for source in [
            "Set-Suggestions 'maybe'",
            "set-suggestions \"true\"",
            "Set-Suggestions ''",
            "Set-Suggestions 'on '",
        ] {
            let err = Scanner::new(source).parse().unwrap_err();
            assert_eq!(
                err.message, "invalid suggestions value: expected 'on' or 'off'",
                "source: {source}"
            );
            assert_eq!(err.line, 1);
        }

        let err = Scanner::new("Set-Suggestions on").parse().unwrap_err();
        assert_eq!(err.message, "expected string literal");
    }

    #[test]
    fn test_unterminated_block_comment_is_an_error() {
        let err = Scanner::new("<# never closed\nRegister-App 'a' @{}")
            .parse()
            .unwrap_err();
        assert_eq!(err.message, "unterminated block comment: missing '#>'");
        assert_eq!(err.line, 1);
        assert_eq!(err.col, 1);
    }

    #[test]
    fn test_non_directive_tokens_and_characters_are_rejected() {
        // '#' alone is a valid line comment, so it is excluded here.
        for source in ["app", "ps", "config", "@", ";", "$", "=x", "1bad"] {
            let err = Scanner::new(source).parse().unwrap_err();
            assert!(
                err.message.contains("unexpected"),
                "source: {source}, message: {}",
                err.message
            );
        }
    }

    #[test]
    fn test_trailing_junk_after_directives_is_an_error() {
        for source in [
            "Set-Theme 'dark' app",
            "Register-App 'a' @{} ps",
            "function f {} config",
            "Register-App 'a' @{ Path = 'a.exe'; Args = @() }",
        ] {
            let err = Scanner::new(source).parse().unwrap_err();
            assert!(
                err.message.contains("unexpected"),
                "source: {source}, message: {}",
                err.message
            );
        }
    }

    #[test]
    fn test_duplicate_app_name_rejected() {
        let src = "Register-App 'foo' @{ Path = 'a.exe' }\nRegister-App 'Foo' @{ Path = 'b.exe' }";
        let err = Scanner::new(src).parse().unwrap_err();
        assert_eq!(err.message, "duplicate app name 'Foo'");
        assert_eq!(err.line, 2);
    }

    #[test]
    fn test_duplicate_function_name_rejected() {
        let src = "function myfunc { 1 }\nfunction MyFunc { 2 }";
        let err = Scanner::new(src).parse().unwrap_err();
        assert_eq!(err.message, "duplicate function name 'MyFunc'");
        assert_eq!(err.line, 2);
    }
}
