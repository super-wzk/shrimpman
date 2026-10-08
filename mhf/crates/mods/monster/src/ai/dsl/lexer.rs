//! Lexer for the monster-AI DSL: tokens, comments, and source positions.

use crate::ai::{Error, Result};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum TokenKind {
    Word(String),
    Number(u32),
    Decimal(String),
    String(String),
    FatArrow,
    LeftBracket,
    RightBracket,
    LeftParen,
    RightParen,
    LeftBrace,
    RightBrace,
    Colon,
    DoubleColon,
    Equals,
    Comma,
    Semicolon,
    Dot,
    At,
    Eof,
}

#[derive(Clone, Debug)]
pub(super) struct Token {
    pub(super) kind: TokenKind,
    pub(super) line: usize,
    pub(super) column: usize,
    pub(super) offset: usize,
    pub(super) end_offset: usize,
    pub(super) end_line: usize,
    pub(super) end_column: usize,
}

impl Token {
    pub(super) fn error(&self, message: impl Into<String>) -> Error {
        Error::at(self.line, self.column, message)
    }
}

pub(super) struct Lexer<'a> {
    source: &'a [u8],
    offset: usize,
    line: usize,
    column: usize,
}

impl<'a> Lexer<'a> {
    pub(super) fn new(source: &'a str) -> Self {
        Self {
            source: source.as_bytes(),
            offset: 0,
            line: 1,
            column: 1,
        }
    }

    pub(super) fn lex(mut self) -> Result<Vec<Token>> {
        let mut tokens = Vec::new();
        loop {
            self.skip_trivia();
            let offset = self.offset;
            let line = self.line;
            let column = self.column;
            let Some(byte) = self.peek() else {
                tokens.push(Token {
                    kind: TokenKind::Eof,
                    line,
                    column,
                    offset,
                    end_offset: offset,
                    end_line: line,
                    end_column: column,
                });
                return Ok(tokens);
            };

            let kind = match byte {
                b'"' => {
                    self.bump();
                    let start = self.offset;
                    while !matches!(self.peek(), None | Some(b'"' | b'\n' | b'\r')) {
                        self.bump();
                    }
                    if self.peek() != Some(b'"') {
                        return Err(Error::at(line, column, "unterminated import path"));
                    }
                    let path = std::str::from_utf8(&self.source[start..self.offset])
                        .map_err(|_| Error::at(line, column, "invalid UTF-8 path"))?
                        .to_owned();
                    self.bump();
                    TokenKind::String(path)
                }
                b'=' if self.peek_next() == Some(b'>') => {
                    self.bump();
                    self.bump();
                    TokenKind::FatArrow
                }
                b':' => {
                    self.bump();
                    if self.peek() == Some(b':') {
                        self.bump();
                        TokenKind::DoubleColon
                    } else {
                        TokenKind::Colon
                    }
                }
                byte if byte.is_ascii_digit() => self.lex_number(line, column)?,
                byte if byte.is_ascii_alphabetic() || byte == b'_' => {
                    let start = self.offset;
                    while self.peek().is_some_and(|byte| {
                        byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-')
                    }) && !(self.peek() == Some(b'-') && self.peek_next() == Some(b'>'))
                    {
                        self.bump();
                    }
                    let word = std::str::from_utf8(&self.source[start..self.offset])
                        .expect("word bytes are ASCII")
                        .to_owned();
                    TokenKind::Word(word)
                }
                _ => {
                    let kind = match byte {
                        b'@' => TokenKind::At,
                        b'[' => TokenKind::LeftBracket,
                        b']' => TokenKind::RightBracket,
                        b'(' => TokenKind::LeftParen,
                        b')' => TokenKind::RightParen,
                        b'{' => TokenKind::LeftBrace,
                        b'}' => TokenKind::RightBrace,
                        b'=' => TokenKind::Equals,
                        b',' => TokenKind::Comma,
                        b';' => TokenKind::Semicolon,
                        b'.' => TokenKind::Dot,
                        _ => {
                            let message = if byte.is_ascii_graphic() || byte == b' ' {
                                format!("unexpected character '{}'", char::from(byte))
                            } else {
                                format!("unexpected character byte 0x{byte:02x}")
                            };
                            return Err(Error::at(line, column, message));
                        }
                    };
                    self.bump();
                    kind
                }
            };
            tokens.push(Token {
                kind,
                line,
                column,
                offset,
                end_offset: self.offset,
                end_line: self.line,
                end_column: self.column,
            });
        }
    }

    fn lex_number(&mut self, line: usize, column: usize) -> Result<TokenKind> {
        let start = self.offset;
        let hexadecimal =
            self.peek() == Some(b'0') && matches!(self.peek_next(), Some(b'x') | Some(b'X'));
        if hexadecimal {
            self.bump();
            self.bump();
            let digits = self.offset;
            while self.peek().is_some_and(|byte| byte.is_ascii_hexdigit()) {
                self.bump();
            }
            if self.offset == digits {
                return Err(Error::at(line, column, "hexadecimal number has no digits"));
            }
        } else {
            while self.peek().is_some_and(|byte| byte.is_ascii_digit()) {
                self.bump();
            }
            if self.peek() == Some(b'.')
                && self.peek_next().is_some_and(|byte| byte.is_ascii_digit())
            {
                self.bump();
                while self.peek().is_some_and(|byte| byte.is_ascii_digit()) {
                    self.bump();
                }
                return Ok(TokenKind::Decimal(
                    std::str::from_utf8(&self.source[start..self.offset])
                        .expect("decimal ASCII")
                        .to_owned(),
                ));
            }
        }

        let text =
            std::str::from_utf8(&self.source[start..self.offset]).expect("number bytes are ASCII");
        let parsed = match text.strip_prefix("0x").or_else(|| text.strip_prefix("0X")) {
            Some(digits) => u32::from_str_radix(digits, 16),
            None => text.parse::<u32>(),
        };
        parsed.map(TokenKind::Number).map_err(|_| {
            Error::at(
                line,
                column,
                format!("'{text}' is not a number from 0 to {}", u32::MAX),
            )
        })
    }

    fn skip_trivia(&mut self) {
        loop {
            while self.peek().is_some_and(|byte| byte.is_ascii_whitespace()) {
                self.bump();
            }
            if self.peek() == Some(b'/') && self.peek_next() == Some(b'/') {
                while self.peek().is_some_and(|byte| byte != b'\n') {
                    self.bump();
                }
            } else {
                return;
            }
        }
    }

    fn peek(&self) -> Option<u8> {
        self.source.get(self.offset).copied()
    }

    fn peek_next(&self) -> Option<u8> {
        self.source.get(self.offset + 1).copied()
    }

    fn bump(&mut self) {
        let byte = self.source[self.offset];
        self.offset += 1;
        if byte == b'\n' {
            self.line += 1;
            self.column = 1;
        } else {
            self.column += 1;
        }
    }
}
