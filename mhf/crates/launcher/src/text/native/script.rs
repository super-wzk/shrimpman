use super::{MAX_TEXT_BYTES, layout, read_z};
use crate::text::utf8::display_columns;
use std::ptr;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Alignment {
    Center,
    #[default]
    Left,
    Right,
}

#[derive(Clone, Debug)]
struct Run {
    text: String,
    size: usize,
    color: Option<usize>,
}

#[derive(Clone, Debug)]
struct Line {
    runs: Vec<Run>,
    alignment: Alignment,
    advance: usize,
    size: usize,
}

/// BODY/SIZE/COLOR/BR/CENTER/LEFT/RIGHT/END/LF/C are the eleven
/// entries in 11A80704. There are no additional callers of the old D5C0,
/// D890, D920 or DB00 helpers outside the replaced DE10 entrypoint.
fn parse(source: &str) -> Vec<Line> {
    let mut lines = Vec::new();
    let mut runs = Vec::<Run>::new();
    let mut alignment = Alignment::Left;
    let mut size = 5;
    let mut color = None;
    let mut chars = source.char_indices().peekable();
    while let Some((_, mut character)) = chars.next() {
        if character == '<' {
            let mut tag = String::new();
            let mut closed = false;
            for (_, next) in chars.by_ref() {
                if next == '>' {
                    closed = true;
                    break;
                }
                if !next.is_ascii() || tag.len() >= 25 {
                    break;
                }
                tag.push(next);
            }
            if !closed {
                break;
            }
            let (name, value) = tag.split_once('=').unwrap_or((&tag, ""));
            match name {
                "SIZE" => {
                    size = usize::from_str_radix(value, 16)
                        .ok()
                        .filter(|&n| n < 16)
                        .unwrap_or(5)
                }
                "COLOR" | "C" => {
                    color = Some(
                        usize::from_str_radix(value, 16)
                            .ok()
                            .filter(|&n| n < 16)
                            .unwrap_or(7),
                    )
                }
                "CENTER" => alignment = Alignment::Center,
                "LEFT" => alignment = Alignment::Left,
                "RIGHT" => alignment = Alignment::Right,
                "BR" | "LF" => {
                    let advance = if name == "BR" {
                        1
                    } else {
                        value.parse::<usize>().unwrap_or(0).min(9)
                    };
                    lines.push(Line {
                        runs: std::mem::take(&mut runs),
                        alignment,
                        advance,
                        size,
                    });
                }
                "END" => break,
                _ => {}
            }
            if lines.len() >= 26 {
                break;
            }
            continue;
        }
        if character == '\\' {
            let Some((_, escaped)) = chars.next() else {
                break;
            };
            character = escaped;
        } else if character == '\n' {
            lines.push(Line {
                runs: std::mem::take(&mut runs),
                alignment,
                advance: 1,
                size,
            });
            if lines.len() >= 26 {
                break;
            }
            continue;
        }
        if let Some(last) = runs
            .last_mut()
            .filter(|last| last.size == size && last.color == color)
        {
            last.text.push(character);
        } else {
            runs.push(Run {
                text: character.to_string(),
                size,
                color,
            });
        }
    }
    if !runs.is_empty() && lines.len() < 26 {
        lines.push(Line {
            runs,
            alignment,
            advance: 0,
            size,
        });
    }
    lines
}

pub(super) unsafe fn render(base: usize, source: *const u8, x: f32, y: f32) -> u32 {
    let source_bytes = unsafe { read_z(source, MAX_TEXT_BYTES) }.unwrap_or_default();
    let source_text = String::from_utf8_lossy(source_bytes);
    let lines = parse(&source_text);
    let width = unsafe { ptr::read_volatile((base + 0x0EBEE5A0) as *const i32) };
    let height = unsafe { ptr::read_volatile((base + 0x0EBEE59C) as *const i32) };
    let origin = ((width - 640) / 2) as f32 + x;
    let mut y = ((height - 448) / 2) as f32 + y;
    let context = unsafe { layout::context(base) };
    if context == 0 {
        return 0;
    }
    unsafe {
        ptr::write_unaligned((context + 136) as *mut u32, 0);
        ptr::write_unaligned((context + 133072) as *mut u32, u32::MAX);
    }
    for line in lines {
        let line_width: usize = line
            .runs
            .iter()
            .map(|run| {
                let size = unsafe { font_size(base, run.size) };
                display_columns(&run.text) * size / 2
            })
            .sum();
        let mut x = match line.alignment {
            Alignment::Left => origin,
            Alignment::Center => (width as f32 - line_width as f32) / 2.0,
            Alignment::Right => width as f32 - origin - line_width as f32,
        };
        for run in line.runs {
            let size = unsafe { font_size(base, run.size) };
            if let Some(color) = run.color {
                let color = unsafe { ptr::read((base + 0x01A80730 + color) as *const u8) };
                unsafe {
                    call_color(base + 0x014DE910, u32::from(color));
                }
            }
            unsafe {
                ptr::write((context + 24) as *mut u8, size as u8);
                ptr::write((context + 25) as *mut u8, size as u8);
                call_position(base + 0x014DE870, x as i32 as u32, y as i32 as u32);
            }
            // The former script renderer invoked printf a second time. Only
            // its literal %% escape has a meaning without variadic arguments.
            let text = run.text.replace("%%", "%");
            unsafe {
                layout::queue(base, text.as_bytes());
            }
            x += (display_columns(&text) * size / 2) as f32;
        }
        y += (line.advance * unsafe { font_size(base, line.size) }) as f32;
    }
    unsafe {
        ptr::write_unaligned((context + 136) as *mut u32, 0);
        ptr::write_unaligned((context + 133072) as *mut u32, u32::MAX);
        ptr::write_unaligned((context + 24) as *mut u16, 0x1414);
        ptr::write((base + 0x01B8D7A2) as *mut u8, 1);
        ptr::write_unaligned(
            (base + 0x01C1910C) as *mut u32,
            source.add(source_bytes.len()) as u32,
        );
    }
    context as u32
}

unsafe fn font_size(base: usize, index: usize) -> usize {
    unsafe { ptr::read_unaligned((base + 0x01A80740 + index.min(15) * 2) as *const u16) as usize }
}

#[unsafe(naked)]
unsafe extern "C" fn call_color(_target: usize, _color: u32) -> u32 {
    core::arch::naked_asm!(
        "mov edx, [esp + 4]",
        "mov eax, [esp + 8]",
        "call edx",
        "ret"
    );
}

#[unsafe(naked)]
unsafe extern "C" fn call_position(_target: usize, _x: u32, _y: u32) -> u32 {
    core::arch::naked_asm!(
        "mov eax, [esp + 4]",
        "mov ecx, [esp + 8]",
        "mov edx, [esp + 12]",
        "call eax",
        "ret"
    );
}

#[cfg(test)]
mod tests {
    use super::{Alignment, parse};

    #[test]
    fn script_control_tags_and_unicode_runs_remain_separate() {
        let lines = parse("<LF=6><BODY><CENTER><COLOR=3>中😀<COLOR=7>e\u{301}<BR><END>ignored");
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].advance, 6);
        assert_eq!(lines[1].alignment, Alignment::Center);
        assert_eq!(lines[1].runs.len(), 2);
        assert_eq!(lines[1].runs[0].text, "中😀");
        assert_eq!(lines[1].runs[0].color, Some(3));
        assert_eq!(lines[1].runs[1].text, "e\u{301}");
    }

    #[test]
    fn escaped_angle_brackets_and_font_sizes_survive() {
        let lines = parse("<BODY><SIZE=2>\\<中\\><BR><SIZE=f>😀<END>");
        assert_eq!(lines[0].runs[0].text, "<中>");
        assert_eq!(lines[0].runs[0].size, 2);
        assert_eq!(lines[1].runs[0].size, 15);
    }
}
