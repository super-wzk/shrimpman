//! Syntax front end for the monster-AI DSL: the document AST, the parser and
//! the structural checks.

use std::collections::HashMap;

use super::compile::is_reserved_command;
use super::condition::{Condition, Degrees, Mode};
use super::lexer::{Lexer, Token, TokenKind};
use super::target::{Direction, EntityTarget};
use super::{EVENT_SLOT_COUNT, VERSION};
use crate::ai::control::EVENT_SLOTS;
use crate::ai::{Base, Error, Result};

/// One parsed document.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Document {
    pub(crate) native_functions: HashMap<String, super::slot::NativeSlot>,
    pub version: u32,
    pub species: u8,
    pub base: Base,
    pub actions: Vec<ActionDecl>,
    pub events: Vec<EventDecl>,
    pub states: Vec<StateDecl>,
    pub map: Option<u32>,
    pub functions: Vec<Function>,
    pub imports: Vec<Import>,
    pub(super) module: bool,
    pub(super) auto_finish: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Function {
    pub name: String,
    pub body: Vec<Statement>,
    /// `handler fn`: a native request handler with its own return contract.
    pub handler: bool,
    pub line: usize,
    pub column: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Import {
    pub path: String,
    pub alias: String,
    pub line: usize,
    pub column: usize,
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

/// A named event binding, inline body, or explicit clear declaration.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EventDecl {
    /// Native event slot 0..=6 (spec §9).
    pub slot: u8,
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
    EntryBody(Vec<Statement>),
    /// Guarded dispatch to a handler, then the takeover continuation.
    Handle {
        handler: String,
        then_body: Vec<Statement>,
    },
    Random(Vec<(u32, Vec<Statement>)>),
    /// Ordered distance groups followed by the required fallback body.
    TargetDistanceGroups(Vec<Vec<Statement>>),
    /// Evaluate the native context callback once, then select a byte-valued case.
    ContextQuery {
        argument: u8,
        branches: Vec<(u8, Vec<Statement>)>,
        fallback: Vec<Statement>,
    },
    /// Select the species/map-specific area-route configuration.
    AreaRouteProfile {
        branches: Vec<(u8, Vec<Statement>)>,
        fallback: Vec<Statement>,
    },
    /// Exact matching of the current species ID in ascending order.
    Species {
        branches: Vec<(u16, Vec<Statement>)>,
        fallback: Option<Vec<Statement>>,
    },
    /// Ordered matching of the shared native debug selector.
    DebugMode {
        branches: Vec<(u16, Vec<Statement>)>,
        fallback: Option<Vec<Statement>>,
    },
    /// Match map-adapted area IDs in source order.
    Area {
        branches: Vec<(u16, Vec<Statement>)>,
        fallback: Option<Vec<Statement>>,
    },
    /// Match the current species against normalized species groups in source order.
    SpeciesGroup {
        branches: Vec<(u16, Vec<Statement>)>,
        fallback: Option<Vec<Statement>>,
    },
    SelectTargetEntity(EntityTarget),
    SelectPlayerSlot(u8),
    SelectWaypoint(u8),
    SelectDefaultPoint,
    SelectTargetArea(u16),
    SelectTargetPlayerArea,
    SelectRelativePoint(Direction),
    BindAwarenessTarget,
    BindCurrentTarget,
    SetMode(Mode),
    ResolveTarget,
    TryChangeArea,
    IncrementRandomValue,
    If {
        condition: Condition,
        then_body: Vec<Statement>,
        else_body: Option<Vec<Statement>>,
    },
    Return,
    /// `pass;` — give the turn back to the normal AI, handlers only.
    Pass,
    /// `name(args);`, or the anonymous `self.action(group:id, parameter);`.
    Call {
        callee: Callee,
        args: Vec<u8>,
    },
    /// `transition <state>;` — states blocks only.
    Transition {
        state: String,
    },
    /// `restart;` — states blocks only.
    Restart,
    /// `end;` — reset the main entry and active event lanes.
    Reset,
    /// `end forget_target;` — additionally clear the current target's tracking data.
    ResetForgetTarget,
    /// `native(0xff, 0xfd);` — the only bare-value escape.
    Native {
        bytes: Vec<u8>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Callee {
    Name(String),
    Action { group: u8, id: u8 },
}

/// Parse a complete monster-AI document.
///
/// `//` comments run to the end of the line. Declaration blocks may appear in
/// any order. State entries carry an index cursor; named events are independent
/// of declaration order (spec §4).
pub fn parse(source: &str) -> Result<Document> {
    Parser::new(source)?.parse(false)
}

pub(super) fn parse_module(source: &str) -> Result<Document> {
    Parser::new(source)?.parse(true)
}

struct Parser {
    tokens: Vec<Token>,
    position: usize,
    body_depth: usize,
}

impl Parser {
    fn new(source: &str) -> Result<Self> {
        Ok(Self {
            tokens: Lexer::new(source).lex()?,
            position: 0,
            body_depth: 0,
        })
    }

    fn parse(mut self, allow_module: bool) -> Result<Document> {
        let module = allow_module && self.current_word() != Some("mhf_ai");
        let (version, species, map, base) = if module {
            (VERSION, 0, None, Base::Native)
        } else {
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

            let map = if self.current_word() == Some("map") {
                self.advance();
                let map = Some(self.take_number("map ID")?.0);
                self.expect(&TokenKind::Semicolon, "';'")?;
                map
            } else {
                None
            };
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
            (version, species, map, base)
        };

        let mut document = Document {
            native_functions: HashMap::new(),
            version,
            species,
            base,
            actions: Vec::new(),
            events: Vec::new(),
            states: Vec::new(),
            map,
            functions: Vec::new(),
            imports: Vec::new(),
            module,
            auto_finish: false,
        };
        let mut seen: Vec<String> = Vec::new();
        while !matches!(self.current().kind, TokenKind::Eof) {
            let binding = if self.consume(&TokenKind::At) {
                self.expect_keyword("slot")?;
                self.expect(&TokenKind::LeftParen, "'(' after @slot")?;
                self.expect_keyword("table")?;
                self.expect(&TokenKind::Equals, "'=' after table")?;
                let (table, token) = self.take_number("table index")?;
                if !matches!(table, 1 | 9 | 15..=270) {
                    return Err(token.error("subscript table must be 1, 9 or 15..=270"));
                }
                self.expect(&TokenKind::Comma, "','")?;
                self.expect_keyword("index")?;
                self.expect(&TokenKind::Equals, "'=' after index")?;
                let (index, token) = self.take_number("slot index")?;
                let index = byte(index, &token, "slot index")?;
                self.expect(&TokenKind::RightParen, "')'")?;
                Some(super::slot::NativeSlot {
                    table: table as usize,
                    index,
                })
            } else {
                None
            };
            let token = self.take_word("an actions, events, or states block")?;
            let keyword = word(&token);
            // `handler` is contextual: only `handler fn` declares a handler, so
            // an ordinary function may still be called handler().
            let handler = keyword == "handler" && self.current_word() == Some("fn");
            let name = if handler {
                self.advance();
                "fn"
            } else {
                keyword
            }
            .to_owned();
            if binding.is_some() && name != "fn" {
                return Err(token.error("@slot must annotate a function"));
            }
            if name == "import" {
                let path = self.advance();
                let TokenKind::String(value) = &path.kind else {
                    return Err(path.error("expected a quoted import path"));
                };
                self.expect_keyword("as")?;
                let alias = self.take_word("module alias")?;
                self.expect(&TokenKind::Semicolon, "';'")?;
                document.imports.push(Import {
                    path: value.clone(),
                    alias: identifier(&alias)?.into(),
                    line: token.line,
                    column: token.column,
                });
                continue;
            }
            if name == "fn" {
                let name = self.take_word("function name")?;
                if let Some(slot) = binding {
                    let name_text = identifier(&name)?;
                    if name_text == "main" {
                        return Err(name.error("main cannot have a subscript @slot"));
                    }
                    if document
                        .native_functions
                        .values()
                        .any(|value| *value == slot)
                    {
                        return Err(name.error("duplicate native slot binding"));
                    }
                    document.native_functions.insert(name_text.into(), slot);
                }
                self.expect(&TokenKind::LeftParen, "'('")?;
                self.expect(&TokenKind::RightParen, "')' (functions have no parameters)")?;
                self.expect(&TokenKind::LeftBrace, "'{'")?;
                document.functions.push(Function {
                    name: identifier(&name)?.into(),
                    body: self.parse_body()?,
                    handler,
                    line: name.line,
                    column: name.column,
                });
                document.auto_finish = true;
                continue;
            }
            if module && matches!(name.as_str(), "states" | "events") {
                return Err(token.error("imported modules cannot declare states or events"));
            }
            if seen.contains(&name) {
                return Err(token.error(format!("duplicate '{name}' block")));
            }
            seen.push(name.clone());
            match name.as_str() {
                "actions" => document.actions = self.parse_actions()?,
                "events" => document.events = self.parse_events(&mut document.auto_finish)?,
                "states" => document.states = self.parse_states(&mut document.auto_finish)?,
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

    fn parse_events(&mut self, auto_finish: &mut bool) -> Result<Vec<EventDecl>> {
        self.expect(&TokenKind::LeftBrace, "'{'")?;
        let mut entries = Vec::new();
        while !self.consume(&TokenKind::RightBrace) {
            let token = self.take_word("built-in event name")?;
            let name = identifier(&token)?;
            let slot = EVENT_SLOTS
                .iter()
                .position(|event| event.name == name)
                .ok_or_else(|| {
                    token.error(format!(
                        "unknown event '{name}'; expected {}",
                        EVENT_SLOTS
                            .iter()
                            .map(|event| event.name)
                            .collect::<Vec<_>>()
                            .join(", ")
                    ))
                })? as u8;
            if self.current().kind == TokenKind::Equals {
                return Err(self
                    .current()
                    .error("events use fixed names; aliases and numeric slots are not supported"));
            }
            let body = if self.consume(&TokenKind::FatArrow) {
                *auto_finish = true;
                Some(vec![self.entry_reference()?])
            } else if self.consume(&TokenKind::LeftBrace) {
                Some(self.parse_body()?)
            } else {
                self.expect(&TokenKind::Semicolon, "';' or '{'")?;
                None
            };
            entries.push(EventDecl {
                slot,
                body,
                line: token.line,
                column: token.column,
            });
        }
        Ok(entries)
    }

    fn parse_states(&mut self, auto_finish: &mut bool) -> Result<Vec<StateDecl>> {
        self.expect(&TokenKind::LeftBrace, "'{'")?;
        let mut entries = Vec::new();
        let mut cursor: u32 = 1;
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
            let body = if self.consume(&TokenKind::FatArrow) {
                *auto_finish = true;
                Some(vec![self.entry_reference()?])
            } else if self.consume(&TokenKind::LeftBrace) {
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
        if self.body_depth >= 64 {
            return Err(self.current().error("block nesting exceeds 64 levels"));
        }
        self.body_depth += 1;
        let mut statements = Vec::new();
        while !self.consume(&TokenKind::RightBrace) {
            statements.push(self.parse_statement()?);
        }
        self.body_depth -= 1;
        Ok(statements)
    }

    fn parse_branch_body(&mut self) -> Result<Vec<Statement>> {
        if self.consume(&TokenKind::LeftBrace) {
            self.parse_body()
        } else {
            Ok(vec![self.parse_statement()?])
        }
    }

    fn qualified_name(&mut self, mut name: String) -> Result<String> {
        while self.consume(&TokenKind::Dot) {
            name.push('.');
            name.push_str(identifier(&self.take_word("qualified name")?)?);
        }
        Ok(name)
    }

    fn function_reference(&mut self) -> Result<Statement> {
        let token = self.take_word("function reference")?;
        let name = self.qualified_name(identifier(&token)?.into())?;
        self.expect(
            &TokenKind::Semicolon,
            "';' after function reference (without parentheses)",
        )?;
        Ok(Statement {
            kind: StatementKind::Call {
                callee: Callee::Name(name),
                args: Vec::new(),
            },
            line: token.line,
            column: token.column,
        })
    }

    fn entry_reference(&mut self) -> Result<Statement> {
        let token = self.current().clone();
        if self.consume(&TokenKind::LeftBrace) {
            Ok(Statement {
                kind: StatementKind::EntryBody(self.parse_body()?),
                line: token.line,
                column: token.column,
            })
        } else {
            self.function_reference()
        }
    }

    fn parse_statement(&mut self) -> Result<Statement> {
        let token = self.current().clone();
        let name = match &token.kind {
            TokenKind::Word(word) => word.clone(),
            _ => {
                return Err(token.error(
                    "expected a statement: a command call, transition, restart, or control-flow block",
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
            "context" => Err(token.error("context.query(id) is only valid as a match selector")),
            "handle" => {
                let target = self.take_word("handler name")?;
                let handler = self.qualified_name(identifier(&target)?.into())?;
                self.expect(&TokenKind::LeftParen, "'(' after the handler name")?;
                self.expect(&TokenKind::RightParen, "')' (handlers take no arguments)")?;
                self.expect_keyword("then")?;
                self.expect(&TokenKind::LeftBrace, "'{' after then")?;
                let then_body = self.parse_body()?;
                Ok(position(StatementKind::Handle { handler, then_body }))
            }
            "pass" => {
                self.expect(&TokenKind::Semicolon, "';'")?;
                Ok(position(StatementKind::Pass))
            }
            "if" => {
                let namespace = self.take_word("self or context")?;
                if !matches!(word(&namespace), "self" | "context") {
                    return Err(namespace.error("expected self or context"));
                }
                self.expect(&TokenKind::Dot, "'.' after condition namespace")?;
                let property = self.take_word("condition property")?;
                let property_name = self.qualified_name(word(&property).into())?;
                let parsed = if word(&namespace) == "context" {
                    match property_name.as_str() {
                        "any_player_carrying" => Some(Condition::AnyPlayerCarrying),
                        "is_daytime" => Some(Condition::IsDaytime),
                        _ => None,
                    }
                } else {
                    Condition::parse(&property_name)
                };
                let mut condition = parsed.ok_or_else(|| {
                    property.error(format!(
                        "unknown condition {}.{property_name}; expected a supported self condition or context.any_player_carrying / context.is_daytime", word(&namespace)
                    ))
                })?;
                if condition.is_method() {
                    self.expect(&TokenKind::LeftParen, "'(' after condition method")?;
                    if matches!(condition, Condition::TargetAngleIn { .. }) {
                        let min = self.take_degrees()?;
                        self.expect(&TokenKind::Comma, "',' between angle bounds")?;
                        let max = self.take_degrees()?;
                        if min.value() > max.value() {
                            return Err(property.error("target_angle_in requires min <= max; intervals crossing zero are not supported"));
                        }
                        condition = Condition::TargetAngleIn { min, max };
                    } else if matches!(condition, Condition::InAction(_, _)) {
                        let (group, token) = self.take_number("action group")?;
                        let group = byte(group, &token, "action group")?;
                        self.expect(&TokenKind::Colon, "':' between action group and ID")?;
                        let (id, token) = self.take_number("action ID")?;
                        condition = Condition::InAction(group, byte(id, &token, "action ID")?);
                    } else if matches!(condition, Condition::NearTarget(_)) {
                        let (value, token) = self.take_number("distance threshold")?;
                        condition =
                            Condition::NearTarget(byte(value, &token, "distance threshold")?);
                    } else if matches!(condition, Condition::InArea(_)) {
                        let (area, token) = self.take_number("area ID")?;
                        condition = Condition::InArea(
                            u16::try_from(area)
                                .map_err(|_| token.error("area ID must be 0..65535"))?,
                        );
                    } else if matches!(condition, Condition::ModeIs(_)) {
                        self.expect_keyword("Mode")?;
                        self.expect(&TokenKind::DoubleColon, "'::' after Mode")?;
                        let member = self.take_word("Mode member")?;
                        let mode = Mode::parse(word(&member)).ok_or_else(|| {
                            member.error(format!(
                                "unknown Mode member '{}'; expected Normal or Attack",
                                word(&member)
                            ))
                        })?;
                        condition = Condition::ModeIs(mode);
                    }
                    self.expect(
                        &TokenKind::RightParen,
                        "')' after condition method arguments",
                    )?;
                }
                self.expect(&TokenKind::LeftBrace, "'{' after condition")?;
                let then_body = self.parse_body()?;
                let else_body = if self.current_word() == Some("else") {
                    self.advance();
                    self.expect(&TokenKind::LeftBrace, "'{' after else")?;
                    Some(self.parse_body()?)
                } else {
                    None
                };
                Ok(position(StatementKind::If {
                    condition,
                    then_body,
                    else_body,
                }))
            }
            "else" => Err(token.error("else must immediately follow an if block")),
            "return" => {
                self.expect(&TokenKind::Semicolon, "';'")?;
                Ok(position(StatementKind::Return))
            }
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
            "restart" | "end" => {
                if self.peek_is(&TokenKind::LeftParen) {
                    return Err(
                        token.error(format!("{name} is a keyword, not a call: write `{name};`"))
                    );
                }
                let forget_target = name == "end" && self.current_word() == Some("forget_target");
                if forget_target {
                    self.advance();
                }
                self.expect(&TokenKind::Semicolon, "';'")?;
                Ok(position(if forget_target {
                    StatementKind::ResetForgetTarget
                } else if name == "end" {
                    StatementKind::Reset
                } else {
                    StatementKind::Restart
                }))
            }
            "match" => {
                /// The byte-match selectors share one layout; they differ only in
                /// the diagnostic label, the case width and the ordering rule.
                #[derive(Clone, Copy)]
                enum MatchSelector {
                    Species,
                    SpeciesGroup,
                    Area,
                    DebugMode,
                }

                impl MatchSelector {
                    fn label(self) -> &'static str {
                        match self {
                            Self::Species => "species",
                            Self::SpeciesGroup => "species group",
                            Self::Area => "area",
                            Self::DebugMode => "debug mode",
                        }
                    }

                    fn kind(
                        self,
                        branches: Vec<(u16, Vec<Statement>)>,
                        fallback: Option<Vec<Statement>>,
                    ) -> StatementKind {
                        match self {
                            Self::Species => StatementKind::Species { branches, fallback },
                            Self::SpeciesGroup => {
                                StatementKind::SpeciesGroup { branches, fallback }
                            }
                            Self::Area => StatementKind::Area { branches, fallback },
                            Self::DebugMode => StatementKind::DebugMode { branches, fallback },
                        }
                    }
                }

                if self.body_depth >= 64 {
                    return Err(token.error("block nesting exceeds 64 levels"));
                }
                self.body_depth += 1;
                let context_match = self.current_word() == Some("context");
                if context_match {
                    self.advance();
                    self.expect(&TokenKind::Dot, "'.' after context")?;
                } else {
                    self.expect_keyword("self")?;
                    self.expect(&TokenKind::Dot, "'.' after self")?;
                }
                let area_route =
                    !context_match && self.current_word() == Some("area_route_profile");
                let selector = match self.current_word() {
                    Some("debug_mode") if context_match => Some(MatchSelector::DebugMode),
                    Some("species") if !context_match => Some(MatchSelector::Species),
                    Some("species_group") if !context_match => Some(MatchSelector::SpeciesGroup),
                    Some("area") if !context_match => Some(MatchSelector::Area),
                    _ => None,
                };
                if let Some(selector) = selector {
                    let label = selector.label();
                    // Area cases are 16-bit; every other selector carries one byte.
                    let area_case = matches!(selector, MatchSelector::Area);
                    let ordered =
                        matches!(selector, MatchSelector::Species | MatchSelector::DebugMode);
                    self.advance();
                    self.expect(&TokenKind::LeftBrace, "'{' after match selector")?;
                    let mut branches = Vec::new();
                    let fallback = loop {
                        if self.consume(&TokenKind::RightBrace) {
                            if branches.is_empty() {
                                return Err(
                                    token.error(format!("{label} match requires 1..255 cases"))
                                );
                            }
                            break None;
                        }
                        if self.current_word() == Some("else") {
                            self.advance();
                            if branches.is_empty() {
                                return Err(token.error(format!(
                                    "{label} match requires 1..255 cases before else"
                                )));
                            }
                            self.expect(&TokenKind::FatArrow, "'=>' after else")?;
                            let body = self.parse_branch_body()?;
                            self.expect(&TokenKind::RightBrace, "'}' after final else branch")?;
                            break Some(body);
                        }
                        if branches.len() == 255 {
                            return Err(token.error(format!("{label} match requires 1..255 cases")));
                        }
                        let (value, at) = self.take_number(&format!("{label} case or else"))?;
                        let value = if area_case {
                            u16::try_from(value)
                                .map_err(|_| at.error("area case must be 0..65535"))?
                        } else {
                            u16::from(byte(value, &at, &format!("{label} case"))?)
                        };
                        if ordered
                            && branches
                                .last()
                                .is_some_and(|(previous, _)| *previous >= value)
                        {
                            return Err(
                                at.error(format!("{label} cases must be strictly increasing"))
                            );
                        }
                        self.expect(&TokenKind::FatArrow, "'=>' after case")?;
                        branches.push((value, self.parse_branch_body()?));
                    };
                    self.body_depth -= 1;
                    return Ok(position(selector.kind(branches, fallback)));
                }
                if area_route || context_match {
                    let argument = if area_route {
                        self.advance();
                        None
                    } else {
                        self.expect_keyword("query")?;
                        self.expect(&TokenKind::LeftParen, "'('")?;
                        let (argument, at) = self.take_number("context query ID")?;
                        let argument = byte(argument, &at, "context query ID")?;
                        self.expect(&TokenKind::RightParen, "')'")?;
                        Some(argument)
                    };
                    self.expect(&TokenKind::LeftBrace, "'{' after match selector")?;
                    let mut branches = Vec::new();
                    let fallback = loop {
                        if self.current_word() == Some("else") {
                            self.advance();
                            if branches.is_empty() {
                                return Err(
                                    token.error("byte match requires 1..255 cases before else")
                                );
                            }
                            self.expect(&TokenKind::FatArrow, "'=>' after else")?;
                            let body = self.parse_branch_body()?;
                            self.expect(&TokenKind::RightBrace, "'}' after final else branch")?;
                            break body;
                        }
                        let (value, at) = self.take_number("byte match case or else")?;
                        let value = byte(value, &at, "byte match case")?;
                        if branches.len() == 255
                            || branches
                                .last()
                                .is_some_and(|(previous, _)| *previous >= value)
                        {
                            return Err(
                                at.error("byte match requires 1..255 strictly increasing cases")
                            );
                        }
                        self.expect(&TokenKind::FatArrow, "'=>' after case")?;
                        branches.push((value, self.parse_branch_body()?));
                    };
                    self.body_depth -= 1;
                    return Ok(position(match argument {
                        Some(argument) => StatementKind::ContextQuery {
                            argument,
                            branches,
                            fallback,
                        },
                        None => StatementKind::AreaRouteProfile { branches, fallback },
                    }));
                }
                self.expect_keyword("target_distance_group")?;
                self.expect(&TokenKind::LeftParen, "'('")?;
                self.expect(&TokenKind::RightParen, "')'")?;
                self.expect(&TokenKind::LeftBrace, "'{' after match selector")?;
                let mut branches = Vec::new();
                loop {
                    let fallback = self.current_word() == Some("else");
                    if fallback {
                        self.advance();
                        if branches.is_empty() {
                            return Err(token.error(
                                "distance match requires 1..4 numbered groups before else",
                            ));
                        }
                    } else {
                        let (group, at) = self.take_number("distance group or else")?;
                        if branches.len() >= 4 || group as usize != branches.len() + 1 {
                            return Err(at.error(
                                "distance groups must be consecutive from 1, with at most 4 groups",
                            ));
                        }
                    }
                    self.expect(&TokenKind::FatArrow, "'=>' after distance group")?;
                    let body = self.parse_branch_body()?;
                    branches.push(body);
                    if fallback {
                        self.expect(&TokenKind::RightBrace, "'}' after final else branch")?;
                        break;
                    }
                }
                self.body_depth -= 1;
                Ok(position(StatementKind::TargetDistanceGroups(branches)))
            }
            "random" => {
                if self.body_depth >= 64 {
                    return Err(token.error("block nesting exceeds 64 levels"));
                }
                self.body_depth += 1;
                self.expect(&TokenKind::LeftBrace, "'{' after random")?;
                let mut branches = Vec::new();
                while !self.consume(&TokenKind::RightBrace) {
                    let (weight, token) = self.take_number("nonnegative random weight")?;
                    if branches.len() == 31 {
                        return Err(token.error("random requires 1..31 branches"));
                    }
                    self.expect(&TokenKind::FatArrow, "'=>' after random weight")?;
                    let body = self.parse_branch_body()?;
                    branches.push((weight, body));
                }
                if branches.is_empty() {
                    return Err(token.error("random requires at least one branch"));
                }
                self.body_depth -= 1;
                Ok(position(StatementKind::Random(branches)))
            }
            "self" => {
                self.expect(&TokenKind::Dot, "'.' after self")?;
                let method = self.take_word("self method")?;
                let name = word(&method);
                self.expect(&TokenKind::LeftParen, &format!("'(' after {name}"))?;
                let kind = match name {
                    "select_target_point" => {
                        if self.current_word() == Some("PointTarget") {
                            self.advance();
                            self.expect(&TokenKind::DoubleColon, "'::' after PointTarget")?;
                            self.expect_keyword("Default")?;
                            StatementKind::SelectDefaultPoint
                        } else if self.current_word() == Some("Direction") {
                            self.advance();
                            self.expect(&TokenKind::DoubleColon, "'::' after Direction")?;
                            let member = self.take_word("Direction member")?;
                            let direction = Direction::parse(word(&member)).ok_or_else(|| {
                                member.error("unknown Direction member; expected Forward500, Left500, Right500, Backward500, Forward1000, Left1000, Right1000 or Backward1000")
                            })?;
                            StatementKind::SelectRelativePoint(direction)
                        } else {
                            let (index, token) =
                                self.take_number("waypoint index, Direction member or PointTarget::Default")?;
                            StatementKind::SelectWaypoint(byte(index, &token, "waypoint index")?)
                        }
                    }
                    "select_target_entity" => {
                        if self.current_word() == Some("EntityTarget") {
                            self.advance();
                            self.expect(&TokenKind::DoubleColon, "'::' after EntityTarget")?;
                            let member = self.take_word("EntityTarget member")?;
                            let strategy = EntityTarget::parse(word(&member)).ok_or_else(|| {
                                member.error("unknown EntityTarget member; expected SameArea, SameAreaGroundGroup, SameOrAllowedArea, TrackedPlayer, LeaderTarget, PlayerOrMonster, CurrentOrLargeMonster, LargeMonster, OtherMonster or OtherLargeMonster")
                            })?;
                            StatementKind::SelectTargetEntity(strategy)
                        } else {
                            let (slot, token) =
                                self.take_number("player slot or EntityTarget member")?;
                            StatementKind::SelectPlayerSlot(byte(slot, &token, "player slot")?)
                        }
                    }
                    "select_target_area" => {
                        if self.current_word() == Some("AreaTarget") {
                            self.advance();
                            self.expect(&TokenKind::DoubleColon, "'::' after AreaTarget")?;
                            self.expect_keyword("TargetPlayer")?;
                            StatementKind::SelectTargetPlayerArea
                        } else {
                            let (area, token) = self.take_number("area ID or AreaTarget::TargetPlayer")?;
                            StatementKind::SelectTargetArea(u16::try_from(area).map_err(|_| token.error("area ID must be 0..65535"))?)
                        }
                    }
                    "set_mode" => {
                        self.expect_keyword("Mode")?;
                        self.expect(&TokenKind::DoubleColon, "'::' after Mode")?;
                        let member = self.take_word("Mode member")?;
                        let mode = Mode::parse(word(&member)).ok_or_else(|| {
                            member.error("unknown Mode member; expected Normal or Attack")
                        })?;
                        StatementKind::SetMode(mode)
                    }
                    "bind_awareness_target" => StatementKind::BindAwarenessTarget,
                    "bind_current_target" => StatementKind::BindCurrentTarget,
                    "resolve_target" => StatementKind::ResolveTarget,
                    "try_change_area" => StatementKind::TryChangeArea,
                    "increment_random_value" => StatementKind::IncrementRandomValue,
                    "action" => {
                        let (group, token) = self.take_number("action group")?;
                        let group = byte(group, &token, "action group")?;
                        self.expect(&TokenKind::Colon, "':' between action group and ID")?;
                        let (id, token) = self.take_number("action ID")?;
                        let id = byte(id, &token, "action ID")?;
                        self.expect(&TokenKind::Comma, "',' between action ID and parameter")?;
                        let (parameter, token) = self.take_number("action parameter")?;
                        let parameter = byte(parameter, &token, "action parameter")?;
                        self.expect(&TokenKind::RightParen, "')' after action arguments")?;
                        self.expect(&TokenKind::Semicolon, "';'")?;
                        return Ok(position(StatementKind::Call {
                            callee: Callee::Action { group, id },
                            args: vec![parameter],
                        }));
                    }
                    _ => return Err(method.error(format!(
                        "unknown self method '{name}'; expected select_target_entity, select_target_point, select_target_area, bind_awareness_target, bind_current_target, set_mode, resolve_target, try_change_area or increment_random_value"
                    ))),
                };
                self.expect(
                    &TokenKind::RightParen,
                    &format!("')' after {name} arguments"),
                )?;
                self.expect(&TokenKind::Semicolon, "';'")?;
                Ok(position(kind))
            }
            _ => {
                let name = self.qualified_name(name)?;
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

    fn take_degrees(&mut self) -> Result<Degrees> {
        let token = self.advance();
        let value = match &token.kind {
            TokenKind::Number(value) => f64::from(*value),
            TokenKind::Decimal(value) => value
                .parse::<f64>()
                .map_err(|_| token.error("invalid angle"))?,
            _ => return Err(token.error("expected angle in degrees from 0 to 360")),
        };
        Degrees::new(value).ok_or_else(|| token.error("angle must be from 0 to 360 degrees"))
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

fn describe_token(kind: &TokenKind) -> String {
    match kind {
        TokenKind::Word(word) => format!("'{word}'"),
        TokenKind::Number(number) => number.to_string(),
        TokenKind::String(value) => format!("\"{value}\""),
        TokenKind::FatArrow => "'=>'".into(),
        TokenKind::Decimal(value) => value.clone(),
        TokenKind::LeftBracket => "'['".to_owned(),
        TokenKind::RightBracket => "']'".to_owned(),
        TokenKind::LeftParen => "'('".to_owned(),
        TokenKind::RightParen => "')'".to_owned(),
        TokenKind::LeftBrace => "'{'".to_owned(),
        TokenKind::RightBrace => "'}'".to_owned(),
        TokenKind::Colon => "':'".to_owned(),
        TokenKind::DoubleColon => "'::'".to_owned(),
        TokenKind::Equals => "'='".to_owned(),
        TokenKind::Comma => "','".to_owned(),
        TokenKind::Semicolon => "';'".to_owned(),
        TokenKind::Dot => "'.'".to_owned(),
        TokenKind::At => "'@'".to_owned(),
        TokenKind::Eof => "end of input".to_owned(),
    }
}

/// Reserved keywords. None of them can become an action alias (spec §6).
const KEYWORDS: &[&str] = &[
    "Mode",
    "EntityTarget",
    "AreaTarget",
    "Direction",
    "PointTarget",
    "mhf_ai",
    "species",
    "base",
    "actions",
    "events",
    "states",
    "transition",
    "restart",
    "end",
    "random",
    "match",
    "action",
    "native",
    "self",
    "context",
    "if",
    "else",
    "fn",
    "import",
    "as",
    "map",
    "return",
    "handle",
    "pass",
];

/// Check everything that does not need to look at a block body.
///
/// [`parse`] calls this at the end of a successful read, and compilation calls
/// it again so a hand-built [`Document`] cannot skip it.
pub(super) fn check_document(document: &Document) -> Result<()> {
    if document.module && document.functions.iter().any(|f| f.name == "main") {
        return Err(Error::new("main can only be declared in the project entry"));
    }
    if document.actions.iter().any(|a| a.name == "main")
        || document.imports.iter().any(|i| i.alias == "main")
    {
        return Err(Error::new(
            "main is reserved for the state 0 entry function",
        ));
    }
    let mut symbols = HashMap::new();
    for (name, line, column) in document
        .functions
        .iter()
        .map(|f| (&f.name, f.line, f.column))
        .chain(
            document
                .imports
                .iter()
                .map(|i| (&i.alias, i.line, i.column)),
        )
        .chain(document.actions.iter().map(|a| (&a.name, a.line, a.column)))
    {
        if KEYWORDS.contains(&name.as_str()) || is_reserved_command(name) {
            return Err(Error::at(
                line,
                column,
                format!("'{name}' collides with a reserved name"),
            ));
        }
        if symbols.insert(name, ()).is_some() {
            return Err(Error::at(
                line,
                column,
                format!(
                    "symbol '{name}' is declared twice (action '{name}' is declared twice if used as an action)"
                ),
            ));
        }
    }
    for function in &document.functions {
        if function.handler && function.name == "main" {
            return Err(Error::at(
                function.line,
                function.column,
                "main is the state 0 entry, not a request handler",
            ));
        }
        if !function.handler && contains_pass(&function.body) {
            return Err(Error::at(
                function.line,
                function.column,
                format!(
                    "pass; is only valid inside a request handler; declare {} with `handler fn`",
                    function.name
                ),
            ));
        }
    }
    if document.version != VERSION {
        return Err(Error::new(format!(
            "unsupported monster-AI DSL version {}; this build reads version {VERSION}",
            document.version
        )));
    }

    let mut indices: HashMap<u8, &str> = HashMap::new();
    let mut names: HashMap<&str, u8> = HashMap::new();
    for decl in &document.states {
        if decl.name == "main" || (document.auto_finish && decl.index == 0) {
            return Err(Error::at(
                decl.line,
                decl.column,
                "state 0 is reserved for fn main(); do not bind it in states",
            ));
        }
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
                format!(
                    "event '{}' is declared twice",
                    EVENT_SLOTS[usize::from(decl.slot)].name
                ),
            ));
        }
        slots.push(decl.slot);
    }
    Ok(())
}

/// Whether `body` reaches a `pass;` anywhere, including nested branches.
fn contains_pass(body: &[Statement]) -> bool {
    body.iter().any(|statement| match &statement.kind {
        StatementKind::Pass => true,
        StatementKind::EntryBody(body) => contains_pass(body),
        StatementKind::Handle { then_body, .. } => contains_pass(then_body),
        StatementKind::If {
            then_body,
            else_body,
            ..
        } => contains_pass(then_body) || else_body.as_deref().is_some_and(contains_pass),
        StatementKind::Random(branches) => branches.iter().any(|(_, body)| contains_pass(body)),
        StatementKind::ContextQuery {
            branches, fallback, ..
        }
        | StatementKind::AreaRouteProfile { branches, fallback } => {
            branches.iter().any(|(_, body)| contains_pass(body)) || contains_pass(fallback)
        }
        StatementKind::Area { branches, fallback }
        | StatementKind::SpeciesGroup { branches, fallback }
        | StatementKind::DebugMode { branches, fallback }
        | StatementKind::Species { branches, fallback } => {
            branches.iter().any(|(_, body)| contains_pass(body))
                || fallback.as_deref().is_some_and(contains_pass)
        }
        StatementKind::TargetDistanceGroups(branches) => {
            branches.iter().any(|body| contains_pass(body))
        }
        _ => false,
    })
}
