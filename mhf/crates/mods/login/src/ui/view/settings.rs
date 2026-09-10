use super::*;
use egui_hunter::{Field, Popup, SelectField, scroll_on_focus};
use mhf_base::{FontQuality, MhfFontConfig, MhfScreenConfig, Resolution, ScreenMode};

pub(super) fn menu(ui: &mut egui::Ui, enabled: bool) -> Option<Message> {
    let anchor = ui.add_enabled(
        enabled,
        Button::new("设置")
            .id(Id::new("settings_menu"))
            .icon(Icon::Settings)
            .min_size(egui::vec2(52.0, 36.0)),
    );
    let mut message = None;
    Popup::new(&anchor)
        .initial_focus(Id::new("settings_font"))
        .show(|ui| {
            if !enabled {
                ui.close();
                return;
            }
            for (category, id, label) in [
                (SettingsCategory::Font, "settings_font", "字体设置"),
                (SettingsCategory::Screen, "settings_screen", "屏幕设置"),
                (SettingsCategory::Server, "settings_server", "登录服务器"),
            ] {
                if ui
                    .add(Button::new(label).id(Id::new(id)).full_width())
                    .clicked()
                {
                    message = Some(Message::OpenSettings(category));
                    ui.close();
                }
            }
        });
    message
}

pub(super) fn show(settings: &mut Settings, ui: &mut egui::Ui) -> Option<Message> {
    let mut message = None;
    let (title, description, can_save) = match settings {
        Settings::Font(_) => ("字体设置", "设置游戏内使用的字体。", true),
        Settings::Screen(_) => ("屏幕设置", "设置游戏的显示模式和分辨率。", true),
        Settings::Server { overridden, .. } => {
            ("登录服务器", "设置用于登录的服务器地址。", !*overridden)
        }
    };
    egui::CentralPanel::default()
        .frame(
            egui::Frame::new()
                .inner_margin(16)
                .fill(ui.visuals().panel_fill),
        )
        .show(ui, |ui| {
            egui::ScrollArea::vertical()
                .id_salt(("settings", title))
                .min_scrolled_height(0.0)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.vertical_centered(|ui| {
                        let width = ui.available_width().min(480.0);
                        ui.allocate_ui_with_layout(
                            egui::vec2(width, 0.0),
                            Layout::top_down(Align::Min),
                            |ui| {
                                ui.set_width(width);
                                ui.label(RichText::new(title).size(22.0).strong());
                                ui.add(egui::Label::new(RichText::new(description).weak()).wrap());
                                ui.add_space(12.0);
                                match settings {
                                    Settings::Font(config) => font(ui, config),
                                    Settings::Screen(config) => screen(ui, config),
                                    Settings::Server {
                                        endpoint,
                                        overridden,
                                    } => {
                                        server(ui, endpoint, *overridden);
                                    }
                                }
                                ui.add_space(16.0);
                                ui.columns(2, |columns| {
                                    if columns[0]
                                        .add(
                                            Button::new("取消")
                                                .id(Id::new("settings_cancel"))
                                                .min_size(egui::vec2(0.0, 44.0))
                                                .full_width(),
                                        )
                                        .clicked()
                                    {
                                        message = Some(Message::CancelSettings);
                                    }
                                    if columns[1]
                                        .add_enabled(
                                            can_save,
                                            Button::new("保存")
                                                .id(Id::new("settings_save"))
                                                .kind(ButtonKind::Primary)
                                                .full_width(),
                                        )
                                        .clicked()
                                    {
                                        message = Some(Message::SaveSettings);
                                    }
                                });
                                ui.add_space(4.0);
                            },
                        );
                    });
                });
        });
    message
}

fn font(ui: &mut egui::Ui, config: &mut MhfFontConfig) {
    ui.add(
        TextField::new(Id::new("settings_font_name"), &mut config.name)
            .label("字体名称")
            .hint(mhf_font::FAMILY_NAME)
            .help("输入已安装的字体名称。"),
    );
    if ui
        .add(
            Button::new("使用内置字体")
                .id(Id::new("settings_font_default"))
                .kind(ButtonKind::Quiet),
        )
        .clicked()
    {
        config.name = mhf_font::FAMILY_NAME.to_owned();
    }
    ui.add_space(4.0);
    let weight = Field::new(Id::new("settings_font_weight"))
        .label("字重")
        .help("常规为 400，粗体为 700。")
        .show(ui, |ui| {
            ui.add_sized(
                [ui.available_width(), 40.0],
                egui::DragValue::new(&mut config.weight)
                    .range(0..=1000)
                    .speed(10),
            )
        });
    scroll_on_focus(&weight);
    ui.add_space(8.0);
    SelectField::new(
        Id::new("settings_font_quality"),
        quality_name(config.quality),
    )
    .label("字体渲染")
    .show_ui(ui, |ui| {
        for quality in [
            FontQuality::Default,
            FontQuality::Draft,
            FontQuality::Proof,
            FontQuality::NonAntialiased,
            FontQuality::Antialiased,
            FontQuality::ClearType,
            FontQuality::ClearTypeNatural,
        ] {
            ui.selectable_value(&mut config.quality, quality, quality_name(quality));
        }
    });
}

fn quality_name(quality: FontQuality) -> &'static str {
    match quality {
        FontQuality::Default => "系统默认",
        FontQuality::Draft => "草稿",
        FontQuality::Proof => "精细",
        FontQuality::NonAntialiased => "无抗锯齿",
        FontQuality::Antialiased => "抗锯齿",
        FontQuality::ClearType => "ClearType",
        FontQuality::ClearTypeNatural => "自然 ClearType",
    }
}

fn screen(ui: &mut egui::Ui, config: &mut MhfScreenConfig) {
    let mode_name = match config.mode {
        ScreenMode::Windowed => "窗口",
        ScreenMode::Fullscreen => "全屏",
    };
    SelectField::new(Id::new("settings_screen_mode"), mode_name)
        .label("显示模式")
        .show_ui(ui, |ui| {
            ui.selectable_value(&mut config.mode, ScreenMode::Windowed, "窗口");
            ui.selectable_value(&mut config.mode, ScreenMode::Fullscreen, "全屏");
        });
    ui.add_space(12.0);
    resolution(ui, "window", "窗口分辨率", &mut config.window_resolution);
    ui.add_space(12.0);
    resolution(
        ui,
        "fullscreen",
        "全屏分辨率",
        &mut config.fullscreen_resolution,
    );
}

fn resolution(ui: &mut egui::Ui, id: &str, label: &str, resolution: &mut Resolution) {
    let id = Id::new(("settings_resolution", id));
    SelectField::new(id, format!("{} × {}", resolution.width, resolution.height))
        .label(label)
        .help("可选择常用尺寸，也可直接修改宽度和高度。")
        .show_ui(ui, |ui| {
            for (width, height) in [
                (1280, 720),
                (1366, 768),
                (1600, 900),
                (1920, 1080),
                (2560, 1440),
                (3840, 2160),
            ] {
                let preset = Resolution { width, height };
                ui.selectable_value(resolution, preset, format!("{width} × {height}"));
            }
        });
    if ui.available_width() < 280.0 {
        dimension(ui, id.with("width"), "宽度", &mut resolution.width);
        dimension(ui, id.with("height"), "高度", &mut resolution.height);
    } else {
        ui.columns(2, |columns| {
            dimension(
                &mut columns[0],
                id.with("width"),
                "宽度",
                &mut resolution.width,
            );
            dimension(
                &mut columns[1],
                id.with("height"),
                "高度",
                &mut resolution.height,
            );
        });
    }
}

fn dimension(ui: &mut egui::Ui, id: Id, label: &str, value: &mut u32) {
    let response = Field::new(id).label(label).show(ui, |ui| {
        ui.add_sized(
            [ui.available_width(), 40.0],
            egui::DragValue::new(value).range(1..=u32::MAX),
        )
    });
    scroll_on_focus(&response);
}

fn server(ui: &mut egui::Ui, endpoint: &mut String, overridden: bool) {
    if overridden {
        notice(
            ui,
            NoticeKind::Warning,
            "登录地址由环境变量 MHF_SIGN__ENDPOINT 指定，请移除该环境变量后再修改。",
        );
        ui.add_space(8.0);
    }
    ui.add_enabled(
        !overridden,
        TextField::new(Id::new("settings_server_endpoint"), endpoint)
            .label("服务器地址")
            .hint("tcp://127.0.0.1:53000")
            .help("填写 TCP 服务地址，格式为 tcp://主机:端口。"),
    );
}
