use std::fmt;

use crate::theme::Theme;

#[inline]
fn unquote_literal<'a>(lit: &'a str) -> &'a str {
    if (lit.starts_with('\'') && lit.ends_with('\''))
        || (lit.starts_with('"') && lit.ends_with('"'))
    {
        if lit.len() >= 0x02 {
            return &lit[1..lit.len() - 1];
        }
    }
    lit
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct ParseError {
    pub line: u32,
    pub col: u32,
    pub message: &'static str,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}: {}", self.line, self.col, self.message)
    }
}

impl std::error::Error for ParseError {}

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct AppDef<'a> {
    pub name: &'a str,
    pub block_span: &'a str,
}

impl<'a> AppDef<'a> {
    #[inline]
    pub fn unquoted_name(&self) -> &'a str {
        unquote_literal(self.name)
    }
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct FunctionDef<'a> {
    pub name: &'a str,
    pub block_span: &'a str,
}

#[derive(Debug, Default, PartialEq, Eq, Clone)]
pub struct ConfigAst<'a> {
    pub apps: Vec<AppDef<'a>>,
    pub functions: Vec<FunctionDef<'a>>,
    pub theme: Option<Theme>,
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

    fn skip_whitespace_and_comments(&mut self) {
        while let Some(b) = self.peek() {
            match b {
                b' ' | b'\t' | b'\r' | b'\n' => {
                    self.advance();
                }
                b'<' if self.peek_next() == Some(b'#') => {
                    // Block comment <# ... #>
                    self.advance();
                    self.advance();
                    while let Some(cb) = self.peek() {
                        if cb == b'#' && self.peek_next() == Some(b'>') {
                            self.advance();
                            self.advance();
                            break;
                        }
                        self.advance();
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
            return Err(self.make_error("Expected string literal"));
        };
        if quote != b'\'' && quote != b'"' {
            return Err(self.make_error("Expected string literal"));
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
        Err(self.make_error("Unterminated string literal"))
    }

    /// Scans a balanced `{ ... }` block, correctly ignoring braces inside strings and comments.
    fn scan_balanced_block(&mut self) -> Result<&'a str, ParseError> {
        let start = self.pos;
        if self.peek() != Some(b'{') {
            return Err(self.make_error("Expected '{'"));
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
                    self.skip_whitespace_and_comments();
                }
                b'#' => {
                    self.skip_whitespace_and_comments();
                }
                _ => {
                    self.advance();
                }
            }
        }
        Err(self.make_error("Unterminated block: missing '}'"))
    }

    pub fn parse(mut self) -> Result<ConfigAst<'a>, ParseError> {
        let mut ast = ConfigAst {
            apps: Vec::new(),
            functions: Vec::new(),
            theme: None,
        };

        loop {
            self.skip_whitespace_and_comments();
            if self.pos >= self.bytes.len() {
                break;
            }

            let kw = self.read_identifier_or_keyword();

            if kw.eq_ignore_ascii_case("Set-Theme") {
                self.skip_whitespace_and_comments();
                let raw = self.read_string_literal()?;
                let value = unquote_literal(raw);
                let Some(theme) = Theme::from_config_str(value) else {
                    return Err(self.make_error("Invalid theme: expected 'dark' or 'light'"));
                };
                ast.theme = Some(theme);
            } else if kw.eq_ignore_ascii_case("Register-App") {
                self.skip_whitespace_and_comments();
                let name = self.read_string_literal()?;

                self.skip_whitespace_and_comments();
                // Handle @{ ... } hashtable syntax
                if self.peek() == Some(b'@') && self.peek_next() == Some(b'{') {
                    self.advance(); // consume '@'
                    let block_span = self.scan_balanced_block()?;
                    ast.apps.push(AppDef { name, block_span });
                } else {
                    return Err(self.make_error("Expected @{ after Register-App name"));
                }
            } else if kw.eq_ignore_ascii_case("function") {
                self.skip_whitespace_and_comments();
                let name = self.read_identifier_or_keyword();
                if name.is_empty() {
                    return Err(self.make_error("Expected function name"));
                }

                self.skip_whitespace_and_comments();
                let block_span = self.scan_balanced_block()?;
                ast.functions.push(FunctionDef { name, block_span });
            } else {
                return Err(self.make_error("Unknown top-level directive"));
            }
        }

        Ok(ast)
    }

    fn make_error(&self, message: &'static str) -> ParseError {
        ParseError {
            line: self.line,
            col: (self.pos - self.line_start + 1) as u32,
            message,
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
        let src = r#"
# shyena:v1
Register-App 'code' @{
    Path = 'C:\VSCode\Code.exe'
    Args = @('--reuse-window')
}

function prompt {
    Write-Host 'test'
}
"#;
        let ast = Scanner::new(src).parse().expect("parse should succeed");
        assert_eq!(ast.apps.len(), 0x01);
        assert_eq!(ast.apps[0].name, "'code'");
        assert_eq!(ast.apps[0].unquoted_name(), "code");
        assert!(ast.apps[0].block_span.contains("C:\\VSCode\\Code.exe"));

        assert_eq!(ast.functions.len(), 0x01);
        assert_eq!(ast.functions[0].name, "prompt");
        assert_eq!(ast.functions[0].block_span, "{\n    Write-Host 'test'\n}");
    }

    #[test]
    fn test_braces_in_strings_and_comments() {
        let src = r#"
Register-App "complex" @{
    Script = "{ not a block }"
    Note = 'also { not a block }'
    # Comment with { brace }
    <# Block comment { with braces } #>
    Nested = {
        Inner = 42
    }
}
"#;
        let ast = Scanner::new(src).parse().expect("should parse braces in strings");
        assert_eq!(ast.apps.len(), 0x01);
        assert_eq!(ast.apps[0].unquoted_name(), "complex");
        assert!(ast.apps[0].block_span.contains("Inner = 42"));
    }

    #[test]
    fn test_single_quote_escape() {
        let src = r#"
Register-App 'app''name' @{
    Val = 'it''s escaped'
}
"#;
        let ast = Scanner::new(src).parse().expect("single quote escaped");
        assert_eq!(ast.apps.len(), 0x01);
        assert_eq!(ast.apps[0].name, "'app''name'");
    }

    #[test]
    fn test_double_quote_backtick_escape() {
        let src = "Register-App \"app`\"name\" @{ Key = \"val`\"quote\" }";
        let ast = Scanner::new(src).parse().expect("backtick escaped quote");
        assert_eq!(ast.apps.len(), 0x01);
        assert_eq!(ast.apps[0].name, "\"app`\"name\"");
    }

    #[test]
    fn test_missing_hashtable_error() {
        let src = "Register-App 'code' missing_at_brace";
        let err = Scanner::new(src).parse().expect_err("should fail");
        assert_eq!(err.message, "Expected @{ after Register-App name");
    }

    #[test]
    fn test_unterminated_block_error() {
        let src = "Register-App 'code' @{ Path = 'test' ";
        let err = Scanner::new(src).parse().expect_err("should fail");
        assert_eq!(err.message, "Unterminated block: missing '}'");
    }

    #[test]
    fn test_unknown_directive_error() {
        let src = "InvalidKeyword 'code' @{}";
        let err = Scanner::new(src).parse().expect_err("should fail");
        assert_eq!(err.message, "Unknown top-level directive");
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
        assert_eq!(ast.apps[0].name, "'code'");
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

        // Unclosed brace on line 2
        let unclosed = "Register-App 'a' @{ Path = 'a.exe' }\nfunction broken {\n    $x = 1\n";
        let err = Scanner::new(unclosed).parse().unwrap_err();
        assert_eq!(err.message, "Unterminated block: missing '}'");
        assert_eq!(err.line, 4);

        // Missing @{ hashtable
        let bad_app = "Register-App 'a' { Path = 'a.exe' }";
        let err = Scanner::new(bad_app).parse().unwrap_err();
        assert_eq!(err.message, "Expected @{ after Register-App name");
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
        assert_eq!(ast.functions.len(), 1);

        let _ = fs::remove_file(&path);
    }

    #[test]
    fn test_unterminated_string_literal() {
        let single_err = Scanner::new("Register-App 'unterminated").parse().unwrap_err();
        assert_eq!(single_err.message, "Unterminated string literal");

        let double_err = Scanner::new("Register-App 'a' @{ Path = \"unterminated }\n").parse().unwrap_err();
        assert_eq!(double_err.message, "Unterminated string literal");
    }

    #[test]
    fn test_crlf_diagnostics_and_multiline() {
        let src = "Register-App 'test' @{\r\n    Path = 'a.exe'\r\n}\r\nfunction bad {\r\n";
        let err = Scanner::new(src).parse().unwrap_err();
        assert_eq!(err.message, "Unterminated block: missing '}'");
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
        let app_double = AppDef { name: "\"app\"", block_span: "{}" };
        assert_eq!(app_double.unquoted_name(), "app");

        let app_single = AppDef { name: "'app'", block_span: "{}" };
        assert_eq!(app_single.unquoted_name(), "app");

        let app_empty_quote = AppDef { name: "''", block_span: "{}" };
        assert_eq!(app_empty_quote.unquoted_name(), "");

        let app_bare = AppDef { name: "bare_token", block_span: "{}" };
        assert_eq!(app_bare.unquoted_name(), "bare_token");
    }

    #[test]
    fn test_parse_set_theme() {
        let dark = "Set-Theme 'dark'\nRegister-App 'a' @{}";
        let ast = Scanner::new(dark).parse().expect("dark theme parses");
        assert_eq!(ast.theme, Some(Theme::Dark));

        let light = "set-theme \"LIGHT\"\nfunction f { }";
        let ast = Scanner::new(light).parse().expect("light theme parses");
        assert_eq!(ast.theme, Some(Theme::Light));
        assert_eq!(ast.functions.len(), 0x01);
    }

    #[test]
    fn test_missing_set_theme_is_none() {
        let ast = Scanner::new("Register-App 'a' @{}").parse().unwrap();
        assert_eq!(ast.theme, None);
    }

    #[test]
    fn test_invalid_set_theme_error() {
        let err = Scanner::new("Set-Theme 'mocha'").parse().unwrap_err();
        assert_eq!(err.message, "Invalid theme: expected 'dark' or 'light'");
    }

    #[test]
    fn test_set_theme_requires_string_literal() {
        let err = Scanner::new("Set-Theme dark").parse().unwrap_err();
        assert_eq!(err.message, "Expected string literal");
    }
}
