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
            let line = self.line;
            let column = self.column;
            let Some(byte) = self.peek() else {
                tokens.push(Token {
                    kind: TokenKind::Eof,
                    line,
                    column,
                });
                return Ok(tokens);
            };

            let kind = match byte {
                b'@' => {
                    self.bump();
                    TokenKind::At
                }
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
                b'[' => {
                    self.bump();
                    TokenKind::LeftBracket
                }
                b']' => {
                    self.bump();
                    TokenKind::RightBracket
                }
                b'(' => {
                    self.bump();
                    TokenKind::LeftParen
                }
                b')' => {
                    self.bump();
                    TokenKind::RightParen
                }
                b'{' => {
                    self.bump();
                    TokenKind::LeftBrace
                }
                b'}' => {
                    self.bump();
                    TokenKind::RightBrace
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
                b'=' => {
                    self.bump();
                    TokenKind::Equals
                }
                b',' => {
                    self.bump();
                    TokenKind::Comma
                }
                b';' => {
                    self.bump();
                    TokenKind::Semicolon
                }
                b'.' => {
                    self.bump();
                    TokenKind::Dot
                }
                byte if byte.is_ascii_digit() => self.lex_number(line, column)?,
                byte if is_word_start(byte) => {
                    let start = self.offset;
                    while self.peek().is_some_and(is_word_byte)
                        && !(self.peek() == Some(b'-') && self.peek_next() == Some(b'>'))
                    {
                        self.bump();
                    }
                    let word = std::str::from_utf8(&self.source[start..self.offset])
                        .expect("word bytes are ASCII")
                        .to_owned();
                    TokenKind::Word(word)
                }
                _ => {
                    return Err(Error::at(
                        line,
                        column,
                        format!("unexpected character {}", describe_byte(byte)),
                    ));
                }
            };
            tokens.push(Token { kind, line, column });
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
                self.skip_comment();
            } else {
                return;
            }
        }
    }

    fn skip_comment(&mut self) {
        for _ in 0..2 {
            self.bump();
        }
        while self.peek().is_some_and(|byte| byte != b'\n') {
            self.bump();
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

fn is_word_start(byte: u8) -> bool {
    byte.is_ascii_alphabetic() || byte == b'_'
}

fn is_word_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-')
}

fn describe_byte(byte: u8) -> String {
    if byte.is_ascii_graphic() || byte == b' ' {
        format!("'{0}'", char::from(byte))
    } else {
        format!("byte 0x{byte:02x}")
    }
}
