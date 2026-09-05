//! Run with `cargo run -p egui-hunter --example gallery --target <host-triple>`.
use std::path::PathBuf;

use egui::{Color32, FontId, Id, Rect, RichText, Sense, Stroke, Vec2, pos2, vec2};
use egui_hunter::{
    ButtonKind, Direction, FocusGroup, Icon, MenuStack, NoticeKind, Notifications, OverlayState,
    Property, Surface, Tab, TabsState, Theme, Validation,
};

#[derive(Default)]
struct Options {
    font: Option<PathBuf>,
    screenshot: Option<PathBuf>,
    compact: bool,
    dialog: bool,
    containers: bool,
    popup: bool,
    window: bool,
    notices: bool,
    details: bool,
    tooltip: bool,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut options = Options::default();
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--font" => options.font = Some(args.next().ok_or("--font needs a font path")?.into()),
            "--screenshot" => {
                options.screenshot =
                    Some(args.next().ok_or("--screenshot needs a PNG path")?.into())
            }
            "--compact" => options.compact = true,
            "--dialog" => options.dialog = true,
            "--containers" => options.containers = true,
            "--notices" => options.notices = true,
            "--details" => options.details = true,
            "--tooltip" => {
                options.details = true;
                options.tooltip = true;
            }
            "--window" => {
                options.containers = true;
                options.window = true;
            }
            "--popup" => {
                options.containers = true;
                options.popup = true;
            }
            _ => return Err(format!("unknown argument: {arg}").into()),
        }
    }
    let size = if options.compact {
        [760.0, 1100.0]
    } else {
        [1440.0, 1050.0]
    };
    eframe::run_native(
        "猎人工坊 · egui-hunter",
        eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default()
                .with_inner_size(size)
                .with_min_inner_size([620.0, 520.0]),
            centered: true,
            ..Default::default()
        },
        Box::new(move |cc| {
            install_fonts(&cc.egui_ctx, options.font.as_deref())?;
            let mut gallery = Gallery::default();
            gallery.theme.apply(&cc.egui_ctx);
            gallery.screenshot = options.screenshot;
            if options.dialog {
                gallery.dialog.open(&cc.egui_ctx);
            }
            if options.containers {
                gallery.page.select(Id::new("containers"));
            }
            if options.details {
                gallery.page.select(Id::new("details"));
            }
            gallery.preview_tooltip = options.tooltip;
            if options.popup {
                gallery.popup.open(&cc.egui_ctx);
            }
            gallery.window_open = options.window;
            if options.notices {
                gallery.play_notices(&cc.egui_ctx);
            }
            Ok(Box::new(gallery))
        }),
    )?;
    Ok(())
}

fn install_fonts(ctx: &egui::Context, path: Option<&std::path::Path>) -> std::io::Result<()> {
    // The library never performs font I/O. This native example uses an explicitly
    // supplied font or a system CJK font, without redistributing font files.
    let bytes = if let Some(path) = path {
        Some(std::fs::read(path)?)
    } else {
        [
            "/System/Library/Fonts/STHeiti Light.ttc",
            "C:/Windows/Fonts/msyh.ttc",
            "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
            "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc",
        ]
        .into_iter()
        .find_map(|path| std::fs::read(path).ok())
    };
    if let Some(bytes) = bytes {
        let mut fonts = egui::FontDefinitions::default();
        fonts.font_data.insert(
            "hunter-cjk".into(),
            egui::FontData::from_owned(bytes).into(),
        );
        fonts
            .families
            .entry(egui::FontFamily::Proportional)
            .or_default()
            .insert(0, "hunter-cjk".into());
        ctx.set_fonts(fonts);
    } else {
        eprintln!("No system CJK font found; pass --font /path/to/font.ttf for Chinese labels.");
    }
    Ok(())
}

struct Quest {
    name: &'static str,
    location: &'static str,
    goal: &'static str,
    reward: &'static str,
    rank: &'static str,
    icon: Icon,
}

const QUESTS: [Quest; 3] = [
    Quest {
        name: "狩猎雄火龙",
        location: "古代森林",
        goal: "狩猎 1 头雄火龙",
        reward: "7200 z",
        rank: "★★★★★",
        icon: Icon::Sword,
    },
    Quest {
        name: "采集药草",
        location: "密林营地",
        goal: "交付 10 株药草",
        reward: "1200 z",
        rank: "★★",
        icon: Icon::Herb,
    },
    Quest {
        name: "讨伐速龙",
        location: "荒野高地",
        goal: "讨伐 8 头速龙",
        reward: "2400 z",
        rank: "★★★",
        icon: Icon::Quest,
    },
];

struct Item {
    name: &'static str,
    detail: &'static str,
    icon: Icon,
    count: u32,
    recovery: bool,
}

#[derive(Debug, PartialEq, Eq, Hash)]
enum MenuPage {
    Camp,
    Equipment,
    Weapon,
}

struct Gallery {
    theme: Theme,
    quest: usize,
    item: usize,
    category: usize,
    items: [Item; 10],
    query: String,
    tips: bool,
    auto_sort: bool,
    input_style: usize,
    volume: f32,
    health: f32,
    dialog: OverlayState,
    popup: OverlayState,
    notices: Notifications,
    page: TabsState,
    menu: MenuStack<MenuPage>,
    window_open: bool,
    hunter_name: String,
    room_name: String,
    room_password: String,
    guild_id: String,
    locked_title: String,
    camp_note: String,
    loadout: usize,
    loadout_cursor: usize,
    pointer_repeat: Option<(Direction, f64)>,
    preview_tooltip: bool,
    screenshot: Option<PathBuf>,
    frames: usize,
}

impl Default for Gallery {
    fn default() -> Self {
        Self {
            theme: Theme::default(),
            quest: 0,
            item: 0,
            category: 0,
            items: [
                Item {
                    name: "回复药",
                    detail: "恢复少量体力",
                    icon: Icon::Potion,
                    count: 10,
                    recovery: true,
                },
                Item {
                    name: "回复药·大",
                    detail: "恢复大量体力",
                    icon: Icon::Potion,
                    count: 5,
                    recovery: true,
                },
                Item {
                    name: "药草",
                    detail: "用于调合回复药",
                    icon: Icon::Herb,
                    count: 8,
                    recovery: false,
                },
                Item {
                    name: "龙骨",
                    detail: "用于锻造装备",
                    icon: Icon::Bone,
                    count: 6,
                    recovery: false,
                },
                Item {
                    name: "铁矿石",
                    detail: "用于强化武器",
                    icon: Icon::Ore,
                    count: 7,
                    recovery: false,
                },
                Item {
                    name: "麻痹陷阱",
                    detail: "短时间限制怪物行动",
                    icon: Icon::Trap,
                    count: 1,
                    recovery: false,
                },
                Item {
                    name: "古龙骨",
                    detail: "珍贵的锻造素材",
                    icon: Icon::Bone,
                    count: 2,
                    recovery: false,
                },
                Item {
                    name: "燕雀石",
                    detail: "散发冷光的矿石",
                    icon: Icon::Ore,
                    count: 4,
                    recovery: false,
                },
                Item {
                    name: "解毒药",
                    detail: "解除中毒状态",
                    icon: Icon::Potion,
                    count: 3,
                    recovery: true,
                },
                Item {
                    name: "落穴陷阱",
                    detail: "让怪物陷入地面",
                    icon: Icon::Trap,
                    count: 1,
                    recovery: false,
                },
            ],
            query: String::new(),
            tips: true,
            auto_sort: true,
            input_style: 0,
            volume: 70.0,
            health: 0.68,
            dialog: OverlayState::default(),
            popup: OverlayState::default(),
            notices: Notifications::new(Id::new("gallery-notices")),
            page: TabsState::default(),
            menu: MenuStack::new(Id::new("camp-menu"), MenuPage::Camp),
            window_open: false,
            hunter_name: String::new(),
            room_name: "密林集会所".into(),
            room_password: "hunter42".into(),
            guild_id: "HR-0042".into(),
            locked_title: "苍蓝之星".into(),
            camp_note: String::new(),
            loadout: 0,
            loadout_cursor: 0,
            pointer_repeat: None,
            preview_tooltip: false,
            screenshot: None,
            frames: 0,
        }
    }
}

impl Gallery {
    fn show(&mut self, ui: &mut egui::Ui) {
        let t = self.theme;
        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(t.palette.background)
                    .inner_margin(24),
            )
            .show(ui, |ui| {
                egui::ScrollArea::vertical()
                    .id_salt("gallery-scroll")
                    .show(ui, |ui| {
                        self.header(ui);
                        ui.add_space(16.0);
                        // Keep tab state outside the content closure, so page actions
                        // may freely mutate the rest of the demo state.
                        let mut page = std::mem::take(&mut self.page);
                        t.tabs(Id::new("gallery-pages")).show(
                            ui,
                            &mut page,
                            &[
                                Tab::new(Id::new("widgets"), "组件展厅"),
                                Tab::new(Id::new("containers"), "容器与导航"),
                                Tab::new(Id::new("details"), "信息与交互"),
                                Tab::new(Id::new("locked"), "封存档案").enabled(false),
                            ],
                            |ui, page| {
                                if page == Id::new("containers") {
                                    self.containers(ui);
                                } else if page == Id::new("details") {
                                    self.details(ui);
                                } else {
                                    t.columns(Id::new("gallery-columns"))
                                        .min_column_width(540.0)
                                        .show(ui, 4, |ui, index| match index {
                                            0 => self.quests(ui),
                                            1 => self.inventory(ui),
                                            2 => self.controls(ui),
                                            _ => self.hud(ui),
                                        });
                                }
                            },
                        );
                        self.page = page;
                        ui.add_space(12.0);
                        ui.separator();
                        ui.horizontal_wrapped(|ui| {
                            for label in ["选择任务", "确认目标", "整理道具", "出发狩猎"]
                            {
                                ui.label(RichText::new("◆").color(t.palette.brass).size(10.0));
                                ui.label(RichText::new(label).color(t.palette.muted).size(14.0));
                                ui.add_space(24.0);
                            }
                        });
                        t.scroll_focus(ui, Id::new("gallery-scroll"));
                    });
            });
        self.modal(ui.ctx());
        t.window("随行手记")
            .id(Id::new("field-window"))
            .open(&mut self.window_open)
            .default_size([340.0, 220.0])
            .default_pos([920.0, 500.0])
            .show(ui.ctx(), |ui| {
                ui.label("拖动空白处移动，拖动边缘调整大小。");
                ui.add_space(8.0);
                t.panel("今日准备")
                    .surface(Surface::Parchment)
                    .show(ui, |ui| {
                        ui.label("◆ 检查装备锋利度");
                        ui.label("◆ 补充回复药与陷阱");
                        ui.label("◆ 在营地集合后出发");
                    });
            });
        self.notices.show(ui.ctx(), &t);
    }

    fn header(&self, ui: &mut egui::Ui) {
        let t = self.theme;
        let wide = ui.available_width() >= 1100.0;
        ui.horizontal(|ui| {
            let (rect, _) = ui.allocate_exact_size(Vec2::splat(56.0), Sense::hover());
            ui.painter()
                .circle_stroke(rect.center(), 26.0, Stroke::new(1.0, t.palette.border));
            Icon::Sword.paint(ui.painter(), rect.shrink(8.0), t.palette.brass);
            ui.add_space(8.0);
            ui.vertical(|ui| {
                ui.label(RichText::new("猎人工坊").size(34.0).color(t.palette.text));
                ui.label(
                    RichText::new("狩猎界面 · 统一设计语言")
                        .size(14.0)
                        .color(t.palette.muted),
                );
            });
            if wide {
                ui.add_space(80.0);
                self.palette(ui);
            }
        });
        if !wide {
            ui.add_space(12.0);
            self.palette(ui);
        }
    }

    fn palette(&self, ui: &mut egui::Ui) {
        let t = self.theme;
        ui.horizontal_wrapped(|ui| {
            for (name, color) in [
                ("骨白", t.palette.text),
                ("炭黑", t.palette.panel),
                ("苔绿 · 选中", t.palette.moss),
                ("黄铜 · 聚焦", t.palette.brass),
                ("朱红 · 危险", t.palette.danger),
            ] {
                let (rect, _) = ui.allocate_exact_size(Vec2::splat(16.0), Sense::hover());
                ui.painter().rect_filled(rect, 2, color);
                ui.label(RichText::new(name).size(13.0).color(t.palette.muted));
                ui.add_space(14.0);
            }
        });
    }

    fn quests(&mut self, ui: &mut egui::Ui) {
        let t = self.theme;
        t.panel("01  任务界面").show(ui, |ui| {
            ui.set_min_height(352.0);
            ui.columns(2, |cols| {
                cols[0].label(RichText::new("集会所委托").small().color(t.palette.muted));
                cols[0].add_space(6.0);
                for (index, quest) in QUESTS.iter().enumerate() {
                    let response = cols[0]
                        .push_id(index, |ui| {
                            ui.add(
                                t.button(quest.name)
                                    .icon(quest.icon)
                                    .selected(self.quest == index)
                                    .full_width()
                                    .min_size(vec2(0.0, 56.0)),
                            )
                        })
                        .inner;
                    if response.clicked() {
                        self.quest = index;
                    }
                }
                cols[0].add_space(12.0);
                cols[0].label(
                    RichText::new("委托记录在此，准备好便出发。")
                        .small()
                        .color(t.palette.muted),
                );
                let quest = &QUESTS[self.quest];
                t.panel("")
                    .surface(Surface::Parchment)
                    .show(&mut cols[1], |ui| {
                        ui.label(RichText::new(quest.name).size(22.0).strong());
                        ui.label(RichText::new(quest.rank).size(16.0).color(t.palette.ink));
                        let (rect, _) = ui
                            .allocate_exact_size(vec2(ui.available_width(), 98.0), Sense::hover());
                        ui.painter().circle_stroke(
                            rect.center(),
                            43.0,
                            Stroke::new(1.0, t.palette.ink.gamma_multiply(0.3)),
                        );
                        quest.icon.paint(
                            ui.painter(),
                            Rect::from_center_size(rect.center(), Vec2::splat(72.0)),
                            t.palette.ink,
                        );
                        ui.separator();
                        ui.label(quest.goal);
                        t.properties(
                            ui,
                            &[
                                Property::new("目的地", quest.location),
                                Property::new("限制时间", "50 分钟"),
                                Property::new("报酬", quest.reward),
                            ],
                        );
                    });
            });
            ui.add_space(12.0);
            ui.separator();
            ui.horizontal_wrapped(|ui| {
                let accept = ui.add(
                    t.button("接受任务")
                        .kind(ButtonKind::Primary)
                        .icon(Icon::Quest),
                );
                if accept.clicked() {
                    self.dialog.open_from(&accept);
                }
                let (confirm, back) = if self.input_style == 0 {
                    ("Enter", "Esc")
                } else {
                    ("A", "B")
                };
                t.key_hint(ui, confirm, "确认");
                t.key_hint(ui, back, "返回");
            });
        });
    }

    fn details(&mut self, ui: &mut egui::Ui) {
        let t = self.theme;
        t.columns(Id::new("detail-columns"))
            .min_column_width(540.0)
            .show(ui, 4, |ui, index| match index {
                0 => self.registration(ui),
                1 => self.equipment_details(ui),
                2 => {
                    self.loadouts(ui);
                }
                _ => self.guild_records(ui),
            });
    }

    fn registration(&mut self, ui: &mut egui::Ui) {
        let t = self.theme;
        t.panel("01  猎人登记").show(ui, |ui| {
            let valid_name = !self.hunter_name.trim().is_empty();
            ui.add(
                t.text_field(Id::new("hunter-name"), &mut self.hunter_name)
                    .label("猎人姓名")
                    .hint("为旅途留下一个名字")
                    .validation(if valid_name {
                        Validation::Success("姓名可以使用")
                    } else {
                        Validation::Error("请填写猎人姓名")
                    }),
            );
            ui.add(
                t.text_field(Id::new("room-name"), &mut self.room_name)
                    .label("集会所名称")
                    .help("队友可通过名称找到你的集会所"),
            );
            ui.add(
                t.text_field(Id::new("room-password"), &mut self.room_password)
                    .label("集会所口令")
                    .password(true)
                    .help("只向同行的猎人分享口令"),
            );
            if ui
                .add_enabled(
                    valid_name,
                    t.button("保存登记")
                        .kind(ButtonKind::Primary)
                        .icon(Icon::Check),
                )
                .clicked()
            {
                self.notices
                    .push(ui.ctx(), NoticeKind::Success, "猎人登记已保存。");
            }
        });
    }

    fn equipment_details(&mut self, ui: &mut egui::Ui) {
        let t = self.theme;
        t.panel("02  装备详情").show(ui, |ui| {
            ui.horizontal(|ui| {
                let response = ui.add(
                    t.item_slot("铁刀·神乐")
                        .icon(Icon::Sword)
                        .selected(true)
                        .size(72.0)
                        .hover_text(false),
                );
                ui.vertical(|ui| {
                    ui.label(RichText::new("铁刀·神乐").size(22.0).strong());
                    ui.label(RichText::new("太刀 / 稀有度 3").color(t.palette.brass));
                    ui.label(RichText::new("悬停或聚焦图标，查看锻造资料").small().weak());
                });
                if self.preview_tooltip {
                    response.request_focus();
                    self.preview_tooltip = false;
                }
                t.tooltip(&response, "铁刀·神乐 · 锻造资料").show(|ui| {
                    ui.label("工坊以精炼矿石打造的太刀，挥舞轻快，适合连续斩击。");
                    ui.separator();
                    t.properties(
                        ui,
                        &[
                            Property::new("铁矿石", "7 / 5 · 足够").color(t.palette.moss),
                            Property::new("燕雀石", "1 / 3 · 缺少 2").color(t.palette.danger),
                            Property::new("锻造费用", "2400 z"),
                        ],
                    );
                });
            });
            ui.add_space(8.0);
            ui.separator();
            t.properties(
                ui,
                &[
                    Property::new("攻击力", "528  (+48)").color(t.palette.moss),
                    Property::new("会心率", "0%"),
                    Property::new("属性", "无"),
                    Property::new("防御加成", "+10").color(t.palette.moss),
                    Property::new("强化条件", "完成工坊的矿石委托"),
                ],
            );
            ui.add_space(8.0);
            t.panel("工匠手记")
                .surface(Surface::Parchment)
                .show(ui, |ui| {
                    ui.label("熟悉武器的节奏，比锋刃本身更重要。准备好素材后，再来工坊看看。");
                });
        });
    }

    fn guild_records(&mut self, ui: &mut egui::Ui) {
        let t = self.theme;
        t.panel("04  公会记录").show(ui, |ui| {
            ui.add(
                t.text_field(Id::new("guild-id"), &mut self.guild_id)
                    .label("猎人编号")
                    .read_only(true)
                    .validation(Validation::Success("已登记 · 编号可选择复制")),
            );
            ui.add_enabled(
                false,
                t.text_field(Id::new("locked-title"), &mut self.locked_title)
                    .label("专属称号")
                    .help("完成指定委托后解锁"),
            );
            ui.add(
                t.text_field(Id::new("camp-alias"), &mut self.camp_note)
                    .label("营地备注")
                    .hint("记录这次旅途的准备事项")
                    .validation(Validation::Warning("备注仅供本次行程使用")),
            );
        });
    }

    fn loadouts(&mut self, ui: &mut egui::Ui) -> [egui::Response; 3] {
        let t = self.theme;
        t.panel("03  出发装备")
            .show(ui, |ui| {
                let labels = ["森林探索", "火龙狩猎", "高阶讨伐 · 尚未解锁", "采集与调合"];
                let controls: Vec<_> = labels
                    .iter()
                    .enumerate()
                    .map(|(index, label)| {
                        let response = ui.add_enabled(
                            index != 2,
                            t.button(label)
                                .id(Id::new(("loadout", index)))
                                .selected(self.loadout == index)
                                .full_width(),
                        );
                        if response.clicked() {
                            self.loadout = index;
                            self.loadout_cursor = index;
                        }
                        response
                    })
                    .collect();
                let group = FocusGroup::vertical().wrap(true);
                group.navigate(ui, &controls);
                if let Some(index) = controls.iter().position(egui::Response::has_focus) {
                    self.loadout_cursor = index;
                }
                ui.add_space(8.0);
                ui.label(
                    RichText::new("方向键移动，确认后切换装备；按住方向可连续移动。")
                        .small()
                        .weak(),
                );
                let buttons = ui
                    .horizontal_wrapped(|ui| {
                        let sense = Sense::click().difference(Sense::focusable_noninteractive());
                        let buttons = ["向上", "向下", "确认"]
                            .map(|label| ui.add(egui::Button::new(label).sense(sense)));
                        t.key_hint(ui, "Enter", "确认装备");
                        buttons
                    })
                    .inner;
                let directions = [Direction::Up, Direction::Down];
                let held = (0..2)
                    .find(|&i| buttons[i].is_pointer_button_down_on())
                    .map(|i| directions[i]);
                let clicked = (0..2)
                    .find(|&i| buttons[i].clicked())
                    .map(|i| directions[i]);
                let now = ui.input(|i| i.time);
                let previous = self.pointer_repeat;
                let next = held
                    .filter(|direction| {
                        previous.is_none_or(|(old, deadline)| old != *direction || now >= deadline)
                    })
                    .or(clicked.filter(|_| previous.is_none()));
                if let Some(direction) = next {
                    if let Some(id) =
                        group.move_focus(ui, &controls, controls[self.loadout_cursor].id, direction)
                    {
                        self.loadout_cursor = controls
                            .iter()
                            .position(|response| response.id == id)
                            .unwrap();
                    }
                    let delay = if previous.is_none_or(|(old, _)| old != direction) {
                        0.35
                    } else {
                        0.09
                    };
                    self.pointer_repeat = held.map(|direction| (direction, now + delay));
                } else if held.is_none() {
                    self.pointer_repeat = None;
                }
                if let Some((_, deadline)) = self.pointer_repeat {
                    ui.ctx()
                        .request_repaint_after(std::time::Duration::from_secs_f64(
                            (deadline - now).max(0.0),
                        ));
                }
                let confirmed = buttons[2].clicked();
                if confirmed {
                    self.loadout = self.loadout_cursor;
                }
                // egui may surrender focus on a mouse release outside the list.
                // Restore the navigation cursor, which is independent of selection.
                if held.is_some() || clicked.is_some() || confirmed {
                    controls[self.loadout_cursor].request_focus();
                }
                if clicked.is_some() || confirmed {
                    ui.ctx().request_repaint();
                }
                buttons
            })
            .inner
    }

    fn inventory(&mut self, ui: &mut egui::Ui) {
        let t = self.theme;
        t.panel("02  道具袋").show(ui, |ui| {
            ui.set_min_height(352.0);
            ui.horizontal_wrapped(|ui| {
                for (index, label) in ["全部", "回复", "素材"].iter().enumerate() {
                    if ui
                        .add(t.button(label).selected(self.category == index))
                        .clicked()
                    {
                        self.category = index;
                    }
                }
            });
            t.text_edit(ui, Id::new("item-search"), &mut self.query, "搜索道具");
            let mut visible: Vec<usize> = self
                .items
                .iter()
                .enumerate()
                .filter(|(_, item)| {
                    item.name.contains(&self.query)
                        && match self.category {
                            1 => item.recovery,
                            2 => !item.recovery,
                            _ => true,
                        }
                })
                .map(|(index, _)| index)
                .collect();
            if self.auto_sort {
                visible.sort_by_key(|&index| (!self.items[index].recovery, self.items[index].name));
            }
            let columns = ((ui.available_width() / 72.0) as usize).clamp(3, 5);
            let size =
                ((ui.available_width() - (columns - 1) as f32 * 8.0) / columns as f32).min(80.0);
            egui::ScrollArea::vertical()
                .id_salt("inventory-slots")
                .max_height(size * 2.0 + 8.0)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    let mut slots = Vec::with_capacity(visible.len());
                    egui::Grid::new("inventory-grid")
                        .spacing([8.0, 8.0])
                        .show(ui, |ui| {
                            for (cell, &index) in visible.iter().enumerate() {
                                let item = &self.items[index];
                                let response = ui
                                    .push_id(index, |ui| {
                                        ui.add(
                                            t.item_slot(item.name)
                                                .icon(item.icon)
                                                .quantity(item.count)
                                                .selected(self.item == index)
                                                .size(size)
                                                .hover_text(false)
                                                .tint(if item.recovery {
                                                    t.palette.moss
                                                } else {
                                                    t.palette.text
                                                }),
                                        )
                                    })
                                    .inner;
                                if response.clicked() {
                                    self.item = index;
                                }
                                t.tooltip(&response, item.name).show(|ui| {
                                    ui.label(item.detail);
                                    t.properties(
                                        ui,
                                        &[
                                            Property::new("持有数量", &item.count.to_string()),
                                            Property::new(
                                                "用途",
                                                if item.recovery {
                                                    "体力回复"
                                                } else {
                                                    "调合与锻造"
                                                },
                                            ),
                                        ],
                                    );
                                });
                                slots.push(response);
                                if cell % columns == columns - 1 {
                                    ui.end_row();
                                }
                            }
                        });
                    FocusGroup::grid(columns).navigate(ui, &slots);
                    if visible.is_empty() {
                        ui.label(RichText::new("没有找到匹配的道具").color(t.palette.muted));
                    }
                    t.scroll_focus(ui, Id::new("inventory-slots"));
                });
            ui.separator();
            let item = &self.items[self.item];
            ui.horizontal(|ui| {
                ui.label(RichText::new(item.name).size(18.0).strong());
                ui.label(
                    RichText::new(format!("持有 {}", item.count))
                        .small()
                        .color(t.palette.muted),
                );
            });
            ui.label(RichText::new(item.detail).size(14.0).color(t.palette.muted));
            let usable = item.recovery && item.count > 0;
            if ui
                .add_enabled(usable, t.button("使用道具").kind(ButtonKind::Primary))
                .clicked()
            {
                self.items[self.item].count -= 1;
                self.health = (self.health + 0.15).min(1.0);
                self.notices.push(
                    ui.ctx(),
                    NoticeKind::Success,
                    format!("已使用{}", self.items[self.item].name),
                );
            }
        });
    }

    fn controls(&mut self, ui: &mut egui::Ui) {
        let t = self.theme;
        t.panel("03  基础组件与状态").show(ui, |ui| {
            ui.set_min_height(228.0);
            ui.horizontal_wrapped(|ui| {
                ui.add(t.button("普通操作"));
                ui.add(t.button("主要操作").kind(ButtonKind::Primary));
                ui.add_enabled(false, t.button("暂不可用"));
                if ui
                    .add(t.button("清空筛选").kind(ButtonKind::Danger))
                    .clicked()
                {
                    self.query.clear();
                    self.category = 0;
                }
            });
            ui.add_space(8.0);
            ui.horizontal_wrapped(|ui| {
                ui.add(t.checkbox(&mut self.tips, "显示提示"));
                ui.add(t.toggle(&mut self.auto_sort, "自动整理"));
                egui::ComboBox::from_id_salt("input-hints")
                    .selected_text(["键鼠提示", "手柄提示"][self.input_style])
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut self.input_style, 0, "键鼠提示");
                        ui.selectable_value(&mut self.input_style, 1, "手柄提示");
                    });
            });
            if self.auto_sort {
                // Keep stable item indices while grouping the grid's display order.
                ui.label(
                    RichText::new("整理偏好：优先显示回复道具")
                        .small()
                        .color(t.palette.muted),
                );
            }
            ui.add(t.slider(&mut self.volume, 0.0..=100.0).text("音量"));
            ui.horizontal_wrapped(|ui| {
                let help = ui.add(t.button("查看道具说明").icon(Icon::Potion));
                if self.tips {
                    help.on_hover_text("回复药：恢复少量体力。\n道具用尽后，使用按钮会禁用。");
                }
                t.key_hint(ui, "Tab", "切换焦点");
            });
            t.notice(ui, NoticeKind::Success, "选中用苔绿与菱形，焦点用黄铜角标");
        });
    }

    fn hud(&mut self, ui: &mut egui::Ui) {
        let t = self.theme;
        t.panel("04  战斗 HUD").show(ui, |ui| {
            ui.set_min_height(228.0);
            let size = vec2(ui.available_width(), 192.0);
            let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
            let painter = ui.painter_at(rect);
            painter.rect_filled(rect, 2, t.palette.background);
            // Quiet geometric terrain: the showcase uses no game assets.
            for row in 0..4 {
                let y = rect.top() + 76.0 + row as f32 * 28.0;
                let points = (0..=12)
                    .map(|i| {
                        pos2(
                            rect.left() + i as f32 * rect.width() / 12.0,
                            y + ((i * 7 + row * 5) % 9) as f32 * 3.0,
                        )
                    })
                    .collect();
                painter.add(egui::Shape::line(
                    points,
                    Stroke::new(1.0, t.palette.border.gamma_multiply(0.35)),
                ));
            }
            ui.scope_builder(
                egui::UiBuilder::new().max_rect(Rect::from_min_size(
                    rect.min + vec2(12.0, 10.0),
                    vec2(rect.width() * 0.53, 68.0),
                )),
                |ui| {
                    ui.add(t.meter(self.health).label("体力"));
                    ui.add(t.meter(0.56).label("耐力").color(t.palette.brass));
                },
            );
            painter.text(
                rect.right_top() + vec2(-12.0, 14.0),
                egui::Align2::RIGHT_TOP,
                QUESTS[self.quest].name,
                FontId::proportional(13.0),
                t.palette.text,
            );
            let map = pos2(rect.left() + 48.0, rect.bottom() - 43.0);
            painter.circle_filled(map, 31.0, t.palette.parchment.gamma_multiply(0.6));
            painter.circle_stroke(map, 31.0, Stroke::new(1.0, t.palette.brass));
            painter.line_segment(
                [map - vec2(20.0, 8.0), map + vec2(16.0, 12.0)],
                Stroke::new(1.0, t.palette.ink),
            );
            painter.text(
                map,
                egui::Align2::CENTER_CENTER,
                "▲",
                FontId::proportional(16.0),
                t.palette.ink,
            );
            painter.text(
                map - vec2(0.0, 35.0),
                egui::Align2::CENTER_BOTTOM,
                "N",
                FontId::proportional(10.0),
                t.palette.text,
            );
            let item_rect = Rect::from_min_size(
                pos2(rect.right() - 82.0, rect.bottom() - 86.0),
                vec2(72.0, 76.0),
            );
            ui.scope_builder(egui::UiBuilder::new().max_rect(item_rect), |ui| {
                ui.add(
                    t.item_slot("回复药")
                        .icon(Icon::Potion)
                        .quantity(self.items[0].count)
                        .selected(true)
                        .size(60.0)
                        .tint(t.palette.moss),
                );
                ui.label(RichText::new("回复药").size(12.0));
            });
            ui.label(
                RichText::new("信息沿边缘分布 · 中央保留战斗视野")
                    .small()
                    .color(t.palette.muted),
            );
        });
    }

    fn containers(&mut self, ui: &mut egui::Ui) {
        let t = self.theme;
        t.columns(Id::new("container-columns"))
            .min_column_width(480.0)
            .show(ui, 4, |ui, index| match index {
                0 => self.camp_menu(ui),
                1 => self.quest_archive(ui),
                2 => self.overlays(ui),
                _ => self.feedback(ui),
            });
    }

    fn camp_menu(&mut self, ui: &mut egui::Ui) {
        let t = self.theme;
        t.panel("01  营地导航").show(ui, |ui| {
            ui.set_min_height(282.0);
            ui.label(
                RichText::new("营地 / 装备 / 武器")
                    .small()
                    .color(t.palette.muted),
            );
            let can_back = self.menu.can_go_back();
            let action = self
                .menu
                .show(ui, |ui, page| {
                    ui.add_space(8.0);
                    let (title, description, next) = match page {
                        MenuPage::Camp => (
                            "营地",
                            "整理行装，选择下一步行动。",
                            Some(("管理装备", MenuPage::Equipment)),
                        ),
                        MenuPage::Equipment => (
                            "装备箱",
                            "检视当前装备并调整出发配置。",
                            Some(("检视武器", MenuPage::Weapon)),
                        ),
                        MenuPage::Weapon => ("猎人之刃", "攻击 320    锋利度 绿    稀有度 4", None),
                    };
                    ui.heading(title);
                    ui.label(description);
                    ui.add_space(12.0);
                    if let Some((label, next)) = next {
                        let response = ui.add(t.button(label).icon(Icon::Sword).full_width());
                        response.clicked().then_some((response, next))
                    } else {
                        ui.add(t.meter(0.72).label("锋利度"));
                        None
                    }
                })
                .inner;
            if let Some((opener, next)) = action {
                self.menu.push_from(&opener, next);
            }
            ui.add_space(16.0);
            if ui.add_enabled(can_back, t.button("返回上一级")).clicked() {
                self.menu.back(ui.ctx());
            }
            t.key_hint(ui, "Esc", "返回上一级");
        });
    }

    fn quest_archive(&mut self, ui: &mut egui::Ui) {
        let t = self.theme;
        t.scroll_panel(Id::new("archive-scroll"), "02  委托档案")
            .max_height(258.0)
            .show_list(
                ui,
                36.0,
                10_000,
                |_| true,
                |ui, row| {
                    let name = format!(
                        "第 {:05} 号委托    ·    {}",
                        row + 1,
                        ["采集药草", "讨伐速龙", "运送矿石"][row % 3]
                    );
                    let response = ui.add_sized(
                        [ui.available_width(), 36.0],
                        t.button(&name).icon(Icon::Quest),
                    );
                    if response.clicked() {
                        self.notices.push(
                            ui.ctx(),
                            NoticeKind::Success,
                            format!("已查阅第 {} 号委托", row + 1),
                        );
                    }
                    response
                },
            );
    }

    fn overlays(&mut self, ui: &mut egui::Ui) {
        let t = self.theme;
        t.panel("03  窗口与浮层").show(ui, |ui| {
            ui.set_min_height(210.0);
            ui.horizontal_wrapped(|ui| {
                if ui.add(t.button("打开随行手记").icon(Icon::Quest)).clicked() {
                    self.window_open = true;
                }
                let confirm = ui.add(t.button("确认委托").kind(ButtonKind::Primary));
                if confirm.clicked() {
                    self.dialog.open_from(&confirm);
                }
            });
            ui.add_space(8.0);
            let anchor = ui.add(t.button("营地行动").icon(Icon::Quest));
            t.popup(&anchor)
                .title("营地行动")
                .show(&mut self.popup, |ui| {
                    for (label, icon) in [
                        ("补充道具", Icon::Potion),
                        ("整理装备", Icon::Sword),
                        ("查阅委托", Icon::Quest),
                    ] {
                        if ui.add(t.button(label).icon(icon).full_width()).clicked() {
                            ui.close();
                        }
                    }
                });
            ui.add_space(8.0);
            ui.label(
                RichText::new("选择行动后收起菜单；点击空白处或按 Esc 关闭。")
                    .small()
                    .color(t.palette.muted),
            );
        });
    }

    fn feedback(&mut self, ui: &mut egui::Ui) {
        let t = self.theme;
        t.panel("04  消息与反馈").show(ui, |ui| {
            ui.set_min_height(210.0);
            t.notice(ui, NoticeKind::Success, "装备检查完成，可以出发。");
            ui.add_space(8.0);
            ui.horizontal_wrapped(|ui| {
                if ui
                    .add(t.button("播放营地消息").kind(ButtonKind::Primary))
                    .clicked()
                {
                    self.play_notices(ui.ctx());
                }
                if ui
                    .add_enabled(!self.notices.is_empty(), t.button("清空消息"))
                    .clicked()
                {
                    self.notices.clear(ui.ctx());
                }
            });
            ui.label(
                RichText::new(format!("待展示消息：{}", self.notices.len()))
                    .small()
                    .color(t.palette.muted),
            );
        });
    }

    fn modal(&mut self, ctx: &egui::Context) {
        let t = self.theme;
        let confirm_id = Id::new("confirm-quest");
        let response = t
            .dialog(Id::new("accept-quest"), "确认委托")
            .initial_focus(confirm_id)
            .show(ctx, &mut self.dialog, |ui| {
                ui.label(RichText::new(QUESTS[self.quest].name).size(24.0));
                ui.label("接受委托后，可以继续整理道具。");
                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    let accepted = ui
                        .add(
                            t.button("接受委托")
                                .id(confirm_id)
                                .kind(ButtonKind::Primary),
                        )
                        .clicked();
                    if accepted {
                        ui.close();
                    }
                    if ui.add(t.button("取消")).clicked() {
                        ui.close();
                    }
                    accepted
                })
                .inner
            });
        if response.is_some_and(|response| response.inner) {
            self.notices.push(
                ctx,
                NoticeKind::Success,
                format!("已接受委托：{}", QUESTS[self.quest].name),
            );
        }
    }

    fn play_notices(&mut self, ctx: &egui::Context) {
        self.notices
            .push(ctx, NoticeKind::Success, "道具已补充至最大携带量。");
        self.notices
            .push(ctx, NoticeKind::Warning, "陷阱存量不足，请及时补充。");
        self.notices
            .push(ctx, NoticeKind::Success, "队友已在营地集合。");
    }
}

impl eframe::App for Gallery {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.show(ui);
        self.frames += 1;
        if let Some(path) = &self.screenshot {
            let screenshot = ui.ctx().input(|input| {
                input.events.iter().find_map(|event| {
                    if let egui::Event::Screenshot { image, .. } = event {
                        Some(image.clone())
                    } else {
                        None
                    }
                })
            });
            if let Some(image) = screenshot {
                let pixels: Vec<u8> = image.pixels.iter().flat_map(Color32::to_array).collect();
                if let Err(error) = image::save_buffer(
                    path,
                    &pixels,
                    image.width() as u32,
                    image.height() as u32,
                    image::ColorType::Rgba8,
                ) {
                    eprintln!("Could not save {}: {error}", path.display());
                    std::process::exit(1);
                }
                println!("Saved {}", path.display());
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                self.screenshot = None;
            } else if self.frames == 8 {
                ui.ctx()
                    .send_viewport_cmd(egui::ViewportCommand::Screenshot(Default::default()));
            } else {
                ui.ctx().request_repaint();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(
        ctx: &egui::Context,
        gallery: &mut Gallery,
        time: f64,
        events: Vec<egui::Event>,
    ) -> [egui::Response; 3] {
        let raw = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(640.0, 480.0))),
            time: Some(time),
            events,
            ..Default::default()
        };
        let mut buttons = None;
        let mut output = ctx.run_ui(raw, |ui| {
            egui::CentralPanel::default().show(ui, |ui| buttons = Some(gallery.loadouts(ui)));
        });
        output.textures_delta.clear();
        buttons.unwrap()
    }

    fn pointer(pos: egui::Pos2, pressed: bool) -> Vec<egui::Event> {
        vec![
            egui::Event::PointerMoved(pos),
            egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            },
        ]
    }

    #[test]
    fn mouse_navigation_keeps_cursor_on_release_and_repeats_without_changing_selection() {
        let ctx = egui::Context::default();
        let mut gallery = Gallery::default();
        let buttons = frame(&ctx, &mut gallery, 0.0, vec![]);
        let down = buttons[1].rect.center();
        let confirm = buttons[2].rect.center();
        for (time, expected) in [(0.1, 1_usize), (0.4, 3)] {
            frame(&ctx, &mut gallery, time, pointer(down, true));
            frame(&ctx, &mut gallery, time + 0.1, pointer(down, false));
            frame(&ctx, &mut gallery, time + 0.2, vec![]);
            assert_eq!(gallery.loadout_cursor, expected);
            assert_eq!(
                ctx.memory(|m| m.focused()),
                Some(Id::new(("loadout", expected)))
            );
            assert_eq!(gallery.loadout, 0);
        }
        frame(&ctx, &mut gallery, 0.8, pointer(confirm, true));
        frame(&ctx, &mut gallery, 0.9, pointer(confirm, false));
        assert_eq!(gallery.loadout, 3);
        frame(&ctx, &mut gallery, 1.0, pointer(down, true));
        assert_eq!(gallery.loadout_cursor, 0);
        frame(&ctx, &mut gallery, 1.2, vec![]);
        assert_eq!(gallery.loadout_cursor, 0);
        frame(&ctx, &mut gallery, 1.36, vec![]);
        assert_eq!(gallery.loadout_cursor, 1);
        frame(&ctx, &mut gallery, 1.46, vec![]);
        assert_eq!(gallery.loadout_cursor, 3);
        frame(&ctx, &mut gallery, 1.5, pointer(down, false));
        frame(&ctx, &mut gallery, 1.6, vec![]);
        assert_eq!(gallery.loadout_cursor, 3);
        assert_eq!(
            ctx.memory(|m| m.focused()),
            Some(Id::new(("loadout", 3_usize)))
        );
    }
}
