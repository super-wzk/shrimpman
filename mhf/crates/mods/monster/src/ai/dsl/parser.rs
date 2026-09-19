//! Syntax front end for the monster-AI DSL: the document AST, the parser and
//! the structural checks.

use std::collections::HashMap;

use super::compile::is_reserved_command;
use super::lexer::{Lexer, Token, TokenKind};
use super::{EVENT_SLOT_COUNT, VERSION};
use crate::ai::{Base, Error, Result};

/// One parsed document.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Document {
    pub version: u32,
    pub species: u8,
    pub base: Base,
    pub actions: Vec<ActionDecl>,
    pub events: Vec<EventDecl>,
    pub states: Vec<StateDecl>,
}

/// `slash = [3:6];` — an alias for one native action coordinate.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ActionDecl {
    pub name: String,
    pub group: u8,
    pub id: u8,
    pub line: usize,
    pub column: usize,
}

/// `roar = 0 { ... }`, `0 { ... }`, or `roar = 0;`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EventDecl {
    /// Native event slot 0..=6 (spec §9).
    pub slot: u8,
    /// Author-facing alias; `None` when the entry wrote the slot number only.
    pub name: Option<String>,
    /// `None` clears the slot instead of installing a script.
    pub body: Option<Vec<Statement>>,
    pub line: usize,
    pub column: usize,
}

/// `idle { ... }`, `combat = 3 { ... }`, or `roam;`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StateDecl {
    /// Index in the state table, i.e. the value `0x07` writes to `+2576`.
    pub index: u8,
    pub name: String,
    /// `None` clears the index instead of installing a script.
    pub body: Option<Vec<Statement>>,
    pub line: usize,
    pub column: usize,
}

/// One statement inside a block body.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Statement {
    pub kind: StatementKind,
    pub line: usize,
    pub column: usize,
}

impl Statement {
    pub(super) fn error(&self, message: impl Into<String>) -> Error {
        Error::at(self.line, self.column, message)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StatementKind {
    /// `name(args);`, or the anonymous `action[group:id](parameter);`.
    Call { callee: Callee, args: Vec<u8> },
    /// `transition <state>;` — states blocks only.
    Transition { state: String },
    /// `restart;` — states blocks only.
    Restart,
    /// `repeat <n> { ... }`. Parsed and formatted, not emitted yet (spec §11.9).
    Repeat { count: u8, body: Vec<Statement> },
    /// `native(0xff, 0xfd);` — the only bare-value escape.
    Native { bytes: Vec<u8> },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Callee {
    Name(String),
    Action { group: u8, id: u8 },
}

/// Parse a complete monster-AI document.
///
/// `//` comments run to the end of the line. Declaration blocks may appear in
/// any order; the entries inside them are order-sensitive because they carry
/// the index cursor (spec §4).
pub fn parse(source: &str) -> Result<Document> {
    Parser::new(source)?.parse()
}

struct Parser {
    tokens: Vec<Token>,
    position: usize,
}

impl Parser {
    fn new(source: &str) -> Result<Self> {
        Ok(Self {
            tokens: Lexer::new(source).lex()?,
            position: 0,
        })
    }

    fn parse(mut self) -> Result<Document> {
        self.expect_keyword("mhf_ai")?;
        let (version, version_token) = self.take_number("format version")?;
        if version != VERSION {
            return Err(
                version_token.error(format!("unsupported monster-AI DSL version {version}"))
            );
        }
        self.expect(&TokenKind::Semicolon, "';'")?;

        self.expect_keyword("species")?;
        let (species, species_token) = self.take_number("species number")?;
        let species = byte(species, &species_token, "species")?;
        self.expect(&TokenKind::Semicolon, "';'")?;

        let base = if self.current_word() == Some("base") {
            self.advance();
            let token = self.take_word("base kind")?;
            let base = match word(&token) {
                "native" => Base::Native,
                other => {
                    return Err(token.error(format!("unknown base '{other}'; expected native")));
                }
            };
            self.expect(&TokenKind::Semicolon, "';'")?;
            base
        } else {
            Base::Empty
        };

        let mut document = Document {
            version,
            species,
            base,
            actions: Vec::new(),
            events: Vec::new(),
            states: Vec::new(),
        };
        let mut seen: Vec<String> = Vec::new();
        while !matches!(self.current().kind, TokenKind::Eof) {
            let token = self.take_word("an actions, events, or states block")?;
            let name = word(&token).to_owned();
            if seen.contains(&name) {
                return Err(token.error(format!("duplicate '{name}' block")));
            }
            seen.push(name.clone());
            match name.as_str() {
                "actions" => document.actions = self.parse_actions()?,
                "events" => document.events = self.parse_events()?,
                "states" => document.states = self.parse_states()?,
                other => {
                    return Err(token.error(format!(
                        "unknown block '{other}'; expected actions, events, or states"
                    )));
                }
            }
        }

        check_document(&document)?;
        Ok(document)
    }

    fn parse_actions(&mut self) -> Result<Vec<ActionDecl>> {
        self.expect(&TokenKind::LeftBrace, "'{'")?;
        let mut entries = Vec::new();
        while !self.consume(&TokenKind::RightBrace) {
            let token = self.take_word("action name")?;
            let name = identifier(&token)?.to_owned();
            self.expect(&TokenKind::Equals, "'='")?;
            self.expect(&TokenKind::LeftBracket, "'['")?;
            let (group, group_token) = self.take_number("action group")?;
            let group = byte(group, &group_token, "action group")?;
            self.expect(&TokenKind::Colon, "':'")?;
            let (id, id_token) = self.take_number("action id")?;
            let id = byte(id, &id_token, "action id")?;
            self.expect(&TokenKind::RightBracket, "']'")?;
            self.expect(&TokenKind::Semicolon, "';'")?;
            entries.push(ActionDecl {
                name,
                group,
                id,
                line: token.line,
                column: token.column,
            });
        }
        Ok(entries)
    }

    fn parse_events(&mut self) -> Result<Vec<EventDecl>> {
        self.expect(&TokenKind::LeftBrace, "'{'")?;
        let mut entries = Vec::new();
        while !self.consume(&TokenKind::RightBrace) {
            let token = self.current().clone();
            let (name, slot) = match &token.kind {
                TokenKind::Word(_) => {
                    self.advance();
                    let name = identifier(&token)?.to_owned();
                    self.expect(&TokenKind::Equals, "'='")?;
                    let (slot, slot_token) = self.take_number("event slot")?;
                    (Some(name), event_slot(slot, &slot_token)?)
                }
                TokenKind::Number(_) => {
                    let (slot, slot_token) = self.take_number("event slot")?;
                    (None, event_slot(slot, &slot_token)?)
                }
                _ => {
                    return Err(token.error("expected an event name or slot number"));
                }
            };
            let body = if self.consume(&TokenKind::LeftBrace) {
                Some(self.parse_body()?)
            } else {
                self.expect(&TokenKind::Semicolon, "';' or '{'")?;
                None
            };
            entries.push(EventDecl {
                slot,
                name,
                body,
                line: token.line,
                column: token.column,
            });
        }
        Ok(entries)
    }

    fn parse_states(&mut self) -> Result<Vec<StateDecl>> {
        self.expect(&TokenKind::LeftBrace, "'{'")?;
        let mut entries = Vec::new();
        let mut cursor: u32 = 0;
        while !self.consume(&TokenKind::RightBrace) {
            let token = self.take_word("state name")?;
            let name = identifier(&token)?.to_owned();
            let index = if self.consume(&TokenKind::Equals) {
                let (value, value_token) = self.take_number("state index")?;
                byte(value, &value_token, "state index")?
            } else {
                if cursor > u32::from(u8::MAX) {
                    return Err(token.error(format!(
                        "state index {cursor} is outside 0..=255: the state table is indexed by the u8 at +2576 (spec §8.1)"
                    )));
                }
                cursor as u8
            };
            cursor = u32::from(index) + 1;
            let body = if self.consume(&TokenKind::LeftBrace) {
                Some(self.parse_body()?)
            } else {
                self.expect(&TokenKind::Semicolon, "';' or '{'")?;
                None
            };
            entries.push(StateDecl {
                index,
                name,
                body,
                line: token.line,
                column: token.column,
            });
        }
        Ok(entries)
    }

    fn parse_body(&mut self) -> Result<Vec<Statement>> {
        let mut statements = Vec::new();
        while !self.consume(&TokenKind::RightBrace) {
            statements.push(self.parse_statement()?);
        }
        Ok(statements)
    }

    fn parse_statement(&mut self) -> Result<Statement> {
        let token = self.current().clone();
        let name = match &token.kind {
            TokenKind::Word(word) => word.clone(),
            _ => {
                return Err(token.error(
                    "expected a statement: a native command call, transition, restart, or repeat",
                ));
            }
        };
        self.advance();
        let position = |kind: StatementKind| Statement {
            kind,
            line: token.line,
            column: token.column,
        };

        match name.as_str() {
            "transition" => {
                if self.peek_is(&TokenKind::LeftParen) {
                    return Err(token.error(
                        "transition is a keyword, not a call: write `transition <state>;` (spec §5)",
                    ));
                }
                let target = self.take_word("state name")?;
                let state = identifier(&target)?.to_owned();
                self.expect(&TokenKind::Semicolon, "';'")?;
                Ok(position(StatementKind::Transition { state }))
            }
            "restart" => {
                if self.peek_is(&TokenKind::LeftParen) {
                    return Err(token.error(
                        "restart is a keyword, not a call: write `restart;` (spec §5)",
                    ));
                }
                self.expect(&TokenKind::Semicolon, "';'")?;
                Ok(position(StatementKind::Restart))
            }
            "repeat" => {
                if self.peek_is(&TokenKind::LeftParen) {
                    return Err(token.error(
                        "repeat takes a literal count, not a call: write `repeat <n> { ... }` (spec §5)",
                    ));
                }
                let (count, count_token) = self.take_number("repeat count")?;
                let count = byte(count, &count_token, "repeat count")?;
                self.expect(&TokenKind::LeftBrace, "'{' after the repeat count")?;
                let body = self.parse_body()?;
                Ok(position(StatementKind::Repeat { count, body }))
            }
            "self" | "if" => Err(token.error(format!(
                "'{name}' is not implemented: the language has no condition form yet, so a read-only `self.` query has nowhere to appear (spec §7)"
            ))),
            "action" if self.peek_is(&TokenKind::LeftBracket) => {
                self.expect(&TokenKind::LeftBracket, "'['")?;
                let (group, group_token) = self.take_number("action group")?;
                let group = byte(group, &group_token, "action group")?;
                self.expect(&TokenKind::Colon, "':'")?;
                let (id, id_token) = self.take_number("action id")?;
                let id = byte(id, &id_token, "action id")?;
                self.expect(&TokenKind::RightBracket, "']'")?;
                let args = self.parse_call_arguments()?;
                self.expect(&TokenKind::Semicolon, "';'")?;
                Ok(position(StatementKind::Call {
                    callee: Callee::Action { group, id },
                    args,
                }))
            }
            _ => {
                let args = self.parse_call_arguments()?;
                self.expect(&TokenKind::Semicolon, "';'")?;
                if name == "native" {
                    if args.is_empty() {
                        return Err(token.error(
                            "native() needs at least one byte: it is the escape hatch for commands that have no name yet (spec §5)",
                        ));
                    }
                    Ok(position(StatementKind::Native { bytes: args }))
                } else {
                    Ok(position(StatementKind::Call {
                        callee: Callee::Name(name),
                        args,
                    }))
                }
            }
        }
    }

    fn parse_call_arguments(&mut self) -> Result<Vec<u8>> {
        self.expect(&TokenKind::LeftParen, "'('")?;
        let mut args = Vec::new();
        if self.consume(&TokenKind::RightParen) {
            return Ok(args);
        }
        loop {
            let (value, token) = self.take_number("argument")?;
            args.push(byte(value, &token, "argument")?);
            if self.consume(&TokenKind::RightParen) {
                return Ok(args);
            }
            self.expect(&TokenKind::Comma, "',' or ')'")?;
            if self.consume(&TokenKind::RightParen) {
                return Ok(args);
            }
        }
    }

    fn expect_keyword(&mut self, expected: &str) -> Result<Token> {
        let token = self.take_word(&format!("'{expected}'"))?;
        if word(&token) != expected {
            return Err(token.error(format!("expected '{expected}', found '{}'", word(&token))));
        }
        Ok(token)
    }

    fn take_word(&mut self, description: &str) -> Result<Token> {
        let token = self.current().clone();
        if !matches!(token.kind, TokenKind::Word(_)) {
            return Err(token.error(format!(
                "expected {description}, found {}",
                describe_token(&token.kind)
            )));
        }
        self.position += 1;
        Ok(token)
    }

    fn take_number(&mut self, description: &str) -> Result<(u32, Token)> {
        let token = self.current().clone();
        let TokenKind::Number(value) = token.kind else {
            return Err(token.error(format!(
                "expected {description}, found {}",
                describe_token(&token.kind)
            )));
        };
        self.position += 1;
        Ok((value, token))
    }

    fn expect(&mut self, kind: &TokenKind, description: &str) -> Result<()> {
        let token = self.current();
        if &token.kind != kind {
            return Err(token.error(format!(
                "expected {description}, found {}",
                describe_token(&token.kind)
            )));
        }
        self.position += 1;
        Ok(())
    }

    fn consume(&mut self, kind: &TokenKind) -> bool {
        if self.peek_is(kind) {
            self.position += 1;
            true
        } else {
            false
        }
    }

    fn peek_is(&self, kind: &TokenKind) -> bool {
        &self.current().kind == kind
    }

    fn current_word(&self) -> Option<&str> {
        match &self.current().kind {
            TokenKind::Word(word) => Some(word),
            _ => None,
        }
    }

    fn current(&self) -> &Token {
        &self.tokens[self.position]
    }

    fn advance(&mut self) -> Token {
        let token = self.tokens[self.position].clone();
        self.position += 1;
        token
    }
}

fn word(token: &Token) -> &str {
    let TokenKind::Word(word) = &token.kind else {
        unreachable!("word() is only called with a word token")
    };
    word
}

fn identifier(token: &Token) -> Result<&str> {
    let TokenKind::Word(name) = &token.kind else {
        return Err(token.error("expected an identifier"));
    };
    let mut bytes = name.bytes();
    let valid_start = bytes
        .next()
        .is_some_and(|byte| byte.is_ascii_alphabetic() || byte == b'_');
    let valid_rest = bytes.all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'));
    if !valid_start || !valid_rest {
        return Err(token.error(format!("'{name}' is not a valid identifier")));
    }
    Ok(name)
}

fn byte(value: u32, token: &Token, description: &str) -> Result<u8> {
    u8::try_from(value)
        .map_err(|_| token.error(format!("{description} must be a number from 0 to 255")))
}

fn event_slot(value: u32, token: &Token) -> Result<u8> {
    if value >= EVENT_SLOT_COUNT as u32 {
        return Err(token.error(format!(
            "event slot must be a number from 0 to {} (spec §4.2)",
            EVENT_SLOT_COUNT - 1
        )));
    }
    Ok(value as u8)
}

fn describe_token(kind: &TokenKind) -> String {
    match kind {
        TokenKind::Word(word) => format!("'{word}'"),
        TokenKind::Number(number) => number.to_string(),
        TokenKind::LeftBracket => "'['".to_owned(),
        TokenKind::RightBracket => "']'".to_owned(),
        TokenKind::LeftParen => "'('".to_owned(),
        TokenKind::RightParen => "')'".to_owned(),
        TokenKind::LeftBrace => "'{'".to_owned(),
        TokenKind::RightBrace => "'}'".to_owned(),
        TokenKind::Colon => "':'".to_owned(),
        TokenKind::Equals => "'='".to_owned(),
        TokenKind::Comma => "','".to_owned(),
        TokenKind::Semicolon => "';'".to_owned(),
        TokenKind::Dot => "'.'".to_owned(),
        TokenKind::Eof => "end of input".to_owned(),
    }
}

/// Reserved keywords. None of them can become an action alias (spec §6).
///
/// `self` and `if` are reserved even though the language has no condition form
/// yet: reserving them keeps a document from giving a future keyword a second
/// meaning today (spec §7).
const KEYWORDS: &[&str] = &[
    "mhf_ai",
    "species",
    "base",
    "actions",
    "events",
    "states",
    "transition",
    "restart",
    "repeat",
    "action",
    "native",
    "self",
    "if",
];

/// Check everything that does not need to look at a block body.
///
/// [`parse`] calls this at the end of a successful read, and compilation calls
/// it again so a hand-built [`Document`] cannot skip it.
pub(super) fn check_document(document: &Document) -> Result<()> {
    if document.version != VERSION {
        return Err(Error::new(format!(
            "unsupported monster-AI DSL version {}; this build reads version {VERSION}",
            document.version
        )));
    }

    let mut actions: HashMap<&str, (u8, u8)> = HashMap::new();
    for decl in &document.actions {
        if is_reserved_command(&decl.name) || KEYWORDS.contains(&decl.name.as_str()) {
            return Err(Error::at(
                decl.line,
                decl.column,
                format!(
                    "action '{}' collides with a reserved name; a reserved name cannot be given a second meaning (spec §6)",
                    decl.name
                ),
            ));
        }
        if actions
            .insert(decl.name.as_str(), (decl.group, decl.id))
            .is_some()
        {
            return Err(Error::at(
                decl.line,
                decl.column,
                format!("action '{}' is declared twice", decl.name),
            ));
        }
    }

    let mut indices: HashMap<u8, &str> = HashMap::new();
    let mut names: HashMap<&str, u8> = HashMap::new();
    for decl in &document.states {
        if indices.insert(decl.index, decl.name.as_str()).is_some() {
            return Err(Error::at(
                decl.line,
                decl.column,
                format!("state index {} is declared twice", decl.index),
            ));
        }
        if names.insert(decl.name.as_str(), decl.index).is_some() {
            return Err(Error::at(
                decl.line,
                decl.column,
                format!("state '{}' is declared twice", decl.name),
            ));
        }
    }

    let mut slots: Vec<u8> = Vec::new();
    for decl in &document.events {
        if usize::from(decl.slot) >= EVENT_SLOT_COUNT {
            return Err(Error::at(
                decl.line,
                decl.column,
                format!(
                    "event slot {} is outside 0..={}",
                    decl.slot,
                    EVENT_SLOT_COUNT - 1
                ),
            ));
        }
        if slots.contains(&decl.slot) {
            return Err(Error::at(
                decl.line,
                decl.column,
                format!("event slot {} is declared twice", decl.slot),
            ));
        }
        slots.push(decl.slot);
    }
    Ok(())
}
