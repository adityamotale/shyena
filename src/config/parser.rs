use std::fmt;

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
        if (self.name.starts_with('\'') && self.name.ends_with('\''))
            || (self.name.starts_with('"') && self.name.ends_with('"'))
        {
            if self.name.len() >= 0x02 {
                return &self.name[1..self.name.len() - 1];
            }
        }
        self.name
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
        };

        loop {
            self.skip_whitespace_and_comments();
            if self.pos >= self.bytes.len() {
                break;
            }

            let kw = self.read_identifier_or_keyword();

            if kw.eq_ignore_ascii_case("Register-App") {
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
}
