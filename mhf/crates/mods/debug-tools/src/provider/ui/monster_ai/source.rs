use egui::text_edit::TextEditOutput;
use egui_code_editor::{CodeEditor, ColorTheme, Syntax};
use std::sync::LazyLock;

/// The source pane owns vertical scrolling and the live breakpoint gutter.
/// CodeEditor supplies highlighting, text input and horizontal scrolling.
pub(super) fn show(
    ui: &mut egui::Ui,
    id: &str,
    text: &mut dyn egui::TextBuffer,
    numlines: bool,
    viewport_height: f32,
) -> TextEditOutput {
    let bounds = egui::Rect::from_min_size(
        ui.next_widget_position(),
        egui::vec2(ui.available_width(), viewport_height.max(0.0)),
    );
    let theme = if ui.visuals().dark_mode {
        HUNTER_DARK
    } else {
        HUNTER_LIGHT
    };
    ui.painter().rect_filled(bounds, 0.0, theme.bg());
    let rows = if numlines {
        1
    } else {
        ((viewport_height - ui.spacing().button_padding.y * 2.0 - ui.spacing().scroll.bar_width)
            / ui.text_style_height(&egui::TextStyle::Monospace))
        .floor()
        .max(1.0) as usize
    };
    ui.scope_builder(egui::UiBuilder::new().id(egui::Id::new(id)), |ui| {
        let mut output = CodeEditor::default()
            .id_source(id)
            .with_ui_fontsize(ui)
            .with_rows(rows)
            .with_numlines(numlines)
            .with_clickable_links(false)
            .with_theme(theme)
            .vscroll(false)
            .show(ui, text, &SYNTAX)
            .0;
        if ui.is_enabled() && output.response.rect.bottom() < bounds.bottom() {
            let blank = egui::Rect::from_min_max(
                egui::pos2(output.response.rect.left(), output.response.rect.bottom()),
                bounds.max,
            );
            if ui
                .interact(blank, egui::Id::new(id).with("blank"), egui::Sense::CLICK)
                .clicked()
            {
                let cursor = egui::text::CCursorRange::one(egui::text::CCursor::new(
                    text.as_str().chars().count(),
                ));
                output.state.cursor.set_char_range(Some(cursor));
                output.state.clone().store(ui.ctx(), output.response.id);
                output.response.request_focus();
            }
        }
        output
    })
    .inner
}

pub(super) fn highlight_line(ui: &egui::Ui, rect: egui::Rect) {
    let color = if ui.visuals().dark_mode {
        HUNTER_DARK.type_color(egui_code_editor::TokenType::Keyword)
    } else {
        HUNTER_LIGHT.type_color(egui_code_editor::TokenType::Keyword)
    };
    ui.painter().rect_filled(
        rect,
        0.0,
        egui::Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), 40),
    );
    ui.painter()
        .vline(rect.left(), rect.y_range(), egui::Stroke::new(2.0, color));
}

static SYNTAX: LazyLock<Syntax> = LazyLock::new(|| {
    Syntax::new("mhai")
        .with_quotes(['"'])
        .with_word_start(['_', '-'])
        .with_hyperlinks([])
        // The DSL has only // comments. NUL cannot start a source token.
        .with_comment_multiline(["\0", "\0"])
        .with_keywords([
            "mhf_ai",
            "species",
            "map",
            "base",
            "actions",
            "events",
            "states",
            "fn",
            "handler",
            "import",
            "as",
            "slot",
            "table",
            "index",
            "transition",
            "restart",
            "end",
            "random",
            "match",
            "if",
            "else",
            "return",
            "handle",
            "then",
            "pass",
        ])
        .with_types([
            "Mode",
            "EntityTarget",
            "AreaTarget",
            "Direction",
            "PointTarget",
        ])
        .with_special(["self", "context", "native"])
});

const HUNTER_DARK: ColorTheme = ColorTheme {
    name: "Hunter",
    dark: true,
    bg: "141619",
    cursor: "F2F0EA",
    selection: "435975",
    comments: "989DA6",
    functions: "E6CE9C",
    keywords: "D8B878",
    literals: "F2F0EA",
    numerics: "B9AAD8",
    punctuation: "C9C5BA",
    strs: "93CBA8",
    types: "9EC4D4",
    special: "D8B878",
};

const HUNTER_LIGHT: ColorTheme = ColorTheme {
    name: "Hunter Light",
    dark: false,
    bg: "F5F3ED",
    cursor: "282A2F",
    selection: "8EACD2",
    comments: "686B72",
    functions: "76521E",
    keywords: "76521E",
    literals: "282A2F",
    numerics: "775999",
    punctuation: "565850",
    strs: "34784D",
    types: "346B80",
    special: "76521E",
};

#[cfg(test)]
mod tests {
    use super::*;
    use egui_code_editor::{Token, TokenType};

    #[test]
    fn mhai_highlighting_preserves_source_and_recognizes_dsl_tokens() {
        let source = "// 中文说明\nimport \"common/6/combat.mhai\" as combat;\nfn main() { self.set_mode(Mode::Attack); native(0xff); return; }\n";
        let tokens = Token::default().tokens(&SYNTAX, source);
        assert_eq!(tokens.iter().map(Token::buffer).collect::<String>(), source);
        for (text, kind) in [
            ("// 中文说明", TokenType::Comment(false)),
            ("import", TokenType::Keyword),
            ("\"common/6/combat.mhai\"", TokenType::Str('"')),
            ("fn", TokenType::Keyword),
            ("self", TokenType::Special),
            ("Mode", TokenType::Type),
            ("native", TokenType::Special),
            ("return", TokenType::Keyword),
        ] {
            assert!(
                tokens
                    .iter()
                    .any(|token| token.buffer() == text && token.ty() == kind),
                "missing {kind:?}: {text}"
            );
        }
    }

    #[test]
    fn editor_preserves_rows_read_only_source_and_parent_style() {
        let context = egui::Context::default();
        egui_hunter::Theme::default().apply(&context);
        let source = "fn main() {\n    restart;\n}\n";
        let mut source_id = None;
        let output = context.run_ui(Default::default(), |ui| {
            let before = ui.visuals().clone();
            let mut text = source;
            let output = show(ui, "read-only-source", &mut text, true, 0.0);
            source_id = Some(output.response.id);
            assert_eq!(text, source);
            assert_eq!(output.galley.job.text, source);
            assert_eq!(output.galley.rows.len(), 4);
            assert_eq!(*ui.visuals(), before);
            assert!(!output.response.changed());
        });
        output.drop_without_applying_deltas();
        context.memory_mut(|memory| memory.request_focus(source_id.unwrap()));
        let output = context.run_ui(
            egui::RawInput {
                events: vec![egui::Event::Text("ignored".into())],
                ..Default::default()
            },
            |ui| {
                let mut text = source;
                let output = show(ui, "read-only-source", &mut text, true, 0.0);
                assert_eq!(output.galley.job.text, source);
                assert_eq!(text, source);
                assert!(
                    output.response.has_focus(),
                    "read-only source remains selectable"
                );
            },
        );
        output.drop_without_applying_deltas();
    }

    #[test]
    fn clicking_the_empty_bottom_of_a_short_source_focuses_and_edits_at_the_end() {
        let context = egui::Context::default();
        egui_hunter::Theme::default()
            .density(egui_hunter::Density::Compact)
            .apply(&context);
        let mut text = "restart;".to_owned();
        let mut source_id = None;
        let mut time = 0.0;
        let mut draw = |text: &mut String, events| {
            time += 0.1;
            let output = context.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(480.0, 360.0),
                    )),
                    time: Some(time),
                    events,
                    ..Default::default()
                },
                |ui| {
                    let height = ui.available_height();
                    source_id = Some(
                        show(ui, "full-height-source", text, false, height)
                            .response
                            .id,
                    );
                },
            );
            output.drop_without_applying_deltas();
        };
        draw(&mut text, vec![]);
        let pos = egui::pos2(80.0, 350.0);
        let click = |pressed| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Default::default(),
        };
        draw(&mut text, vec![egui::Event::PointerMoved(pos), click(true)]);
        draw(&mut text, vec![click(false)]);
        draw(&mut text, vec![egui::Event::Text("// edited".into())]);
        assert!(context.memory(|memory| memory.has_focus(source_id.unwrap())));
        assert_eq!(text, "restart;// edited");
    }
}
