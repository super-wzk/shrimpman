//! Species IDs share the native enemy roster used by Erupe/common/mhfmon.
//! Action masks come from the ZZ HD species dispatchers at 0x118C3628 and
//! CEnemyRoutine's virtual slots dispatched by 0x111A8170. Empty/pure-call slots
//! are omitted. Groups 0..3 are available before spawning; these are real action
//! IDs, not animation numbers. Runtime observations may add variant-specific IDs.
//! Chinese display names use established titles where available; small species
//! without a confirmed Chinese title use transliterations. Unnamed IDs stay explicit.
//! Name references: https://w.atwiki.jp/mhfotw/pages/22.html
//! and https://monsterhunterwiki.org/wiki/Uruki

use super::MonsterAction;

pub(super) use mhf_resource::species::NAMES;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Variant {
    pub(super) id: u8,
    pub(super) name: &'static str,
    /// Model suffix before quest-wide overrides.
    pub(super) model_suffix: &'static str,
}

impl Variant {
    const fn new(id: u8, name: &'static str, model_suffix: &'static str) -> Self {
        Self {
            id,
            name,
            model_suffix,
        }
    }
}

/// Native per-species variants, resolved through the quest resource roster by
/// 1087CB30. Model paths follow 1087C030/1087B840/1087BB50/1087BB00 and
/// 108FBA70. A numeric variant is meaningful only together with its species.
pub(super) fn variants(species: u8) -> Vec<Variant> {
    let mut variants = vec![Variant::new(0, "普通", "")];
    if matches!(
        species,
        1 | 6
            | 11
            | 14
            | 15
            | 17
            | 20
            | 21
            | 22
            | 26
            | 27
            | 28
            | 33
            | 37
            | 38
            | 39
            | 40
            | 41
            | 42
            | 43
            | 45
            | 47
            | 48
            | 49
            | 51
            | 52
            | 53
            | 54
            | 58
            | 59
            | 60
            | 65
            | 67
            | 68
            | 74
            | 75
            | 76
            | 77
            | 78
            | 79
            | 80
            | 81
            | 82
            | 83
            | 84
            | 85
            | 89
            | 90
            | 91
            | 92
            | 94
            | 95
            | 96
            | 101
            | 102
            | 104
    ) {
        variants.push(Variant::new(
            1,
            "HC",
            if matches!(species, 41 | 42) {
                "_c"
            } else {
                "_b"
            },
        ));
    }
    // 10AA6DD0 records Supremacy hunts at 9. CEnemy146's 10E600B0 uses 11;
    // 11313FA0 groups 146/155/166 with the native extreme individuals.
    // Keep each special form's name and model beside its species, including
    // unrelated event monsters that happen to share a numeric variant.
    let special: &[Variant] = match species {
        4 | 9 => &[Variant::new(6, "活动个体", "_b")],
        6 | 101 => &[Variant::new(6, "活动个体", "_c")],
        21 => &[Variant::new(12, "彼岸岛联动", "_c")],
        42 => &[Variant::new(8, "特殊强化", "_c")],
        53 => &[
            Variant::new(8, "特殊强化", "_b"),
            // 15 aliases the query for 8, but the model loader checks the raw
            // value. It needs quest-wide HC to select _b for this form.
            Variant::new(15, "特殊强化", ""),
        ],
        65 | 89 => &[Variant::new(9, "霸种", "_c")],
        95 => &[
            Variant::new(8, "特殊强化", "_b"),
            Variant::new(9, "霸种", "_c"),
            Variant::new(15, "特殊强化", "_c"),
        ],
        100 => &[Variant::new(9, "霸种", ""), Variant::new(13, "至天", "_d")],
        106 => &[
            Variant::new(9, "霸种", ""),
            Variant::new(15, "特殊强化", ""),
        ],
        107 => &[Variant::new(13, "至天", "_d")],
        113 => &[Variant::new(15, "无双袭击", "_b")],
        119 | 120 => &[Variant::new(9, "霸种", "")],
        146 => &[Variant::new(11, "极怪", "_b")],
        154 => &[Variant::new(15, "极怪", "_d")],
        155 => &[
            Variant::new(11, "极怪", "_b"),
            Variant::new(12, "SaGa联动", "_c"),
        ],
        // The bundled native extreme Nargacuga quest uses variant 10; its
        // separate species already owns the extreme model and behavior.
        163 => &[Variant::new(10, "极怪", "")],
        166 => &[Variant::new(11, "极怪", "_d")],
        _ => &[],
    };
    variants.extend_from_slice(special);
    if matches!(
        species,
        11 | 15
            | 17
            | 21
            | 48
            | 51
            | 74
            | 76
            | 80
            | 83
            | 95
            | 99
            | 103
            | 109
            | 110
            | 111
            | 112
            | 121
            | 129
            | 140
            | 141
            | 142
    ) {
        variants.push(Variant::new(16, "辿异种", "_e"));
    }
    variants
}

pub(super) fn actions(species: u8) -> Vec<MonsterAction> {
    let masks: [[u64; 4]; 4] = match species {
        1 | 11 | 37 | 41 | 42 | 49 => [
            [0x170ebfffffff, 0, 0, 0],
            [0x7fff, 0, 0, 0],
            [0x1ffffffff, 0, 0, 0],
            [0xfffffffffffffffd, 0x7fffff, 0, 0],
        ],
        2 | 36 | 71 => [
            [0x1ff, 0, 0, 0],
            [0, 0, 0, 0],
            [0x10ffff, 0, 0, 0],
            [0xffffffffc03fffff, 0xfffffffff03ff, 0, 0x8000000001ffff00],
        ],
        3 | 69 => [
            [0xffe, 0, 0, 0],
            [0x3fff, 0, 0, 0],
            [0xffe, 0, 0, 0],
            [0x1ff, 0, 0, 0],
        ],
        4 | 5 | 32 => [
            [0x3ffe, 0, 0, 0],
            [0x7f, 0, 0, 0],
            [0x3ffe, 0, 0, 0],
            [0x3f, 0, 0, 0],
        ],
        6 | 20 | 38 | 39 => [
            [0x3d3ebfffffff, 0, 0, 0],
            [0x7ffff, 0, 0, 0],
            [0x7ffffff, 0, 0, 0],
            [0, 0, 0, 0],
        ],
        7 | 50 => [
            [0xff, 0, 0, 0],
            [0x3fffff, 0, 0, 0],
            [0, 0, 0, 0],
            [0x3f, 0, 0, 0],
        ],
        8 | 34 => [
            [0xf0000ffc2ff, 0, 0, 0],
            [0xf, 0, 0, 0],
            [0x4f7fffff, 0, 0, 0],
            [0x30fdff, 0, 0, 0],
        ],
        9 | 23 => [
            [0x7c001ffff, 0, 0, 0],
            [0x1ff7, 0, 0, 0],
            [0, 0, 0, 0],
            [0xffff, 0, 0, 0],
        ],
        10 => [
            [0xfff, 0, 0, 0],
            [0xfff, 0, 0, 0],
            [0xfff, 0, 0, 0],
            [0xfff, 0, 0, 0],
        ],
        12 | 25 | 70 => [
            [0x3ffe, 0, 0, 0],
            [0x7ff, 0, 0, 0],
            [0, 0, 0, 0],
            [0x1f, 0, 0, 0],
        ],
        13 | 16 | 30 | 35 => [
            [0x13fff, 0, 0, 0],
            [0x3f, 0, 0, 0],
            [0, 0, 0, 0],
            [0xffff, 0, 0, 0],
        ],
        14 | 26 | 43 | 44 => [
            [0xfe3bf7aeff, 0, 0, 0],
            [0xfff, 0, 0, 0],
            [0xff805fff, 0, 0, 0],
            [0x7fffffc038049, 0, 0, 0],
        ],
        15 | 45 => [
            [0xf06bffe65ff, 0, 0, 0],
            [0x7e9, 0, 0, 0],
            [0x1fffb1fbff, 0, 0, 0],
            [0x7ffffffff, 0, 0, 0],
        ],
        17 | 22 | 47 => [
            [0x3febfdfffff, 0, 0, 0],
            [0x1ffff, 0, 0, 0],
            [0x3b140bf, 0, 0, 0],
            [0, 0, 0, 0],
        ],
        19 | 24 => [
            [0x1e, 0, 0, 0],
            [0, 0, 0, 0],
            [0x3ff, 0, 0, 0],
            [0x1f, 0, 0, 0],
        ],
        21 | 46 => [
            [0x600017ff7f7, 0, 0, 0],
            [0, 0, 0, 0],
            [0x3ffffffffb, 0, 0, 0],
            [0x1ffffffffffff, 0, 0, 0],
        ],
        27 | 28 | 31 => [
            [0x1ffffe, 0, 0, 0],
            [0x7ff, 0, 0, 0],
            [0, 0, 0, 0],
            [0x1ffffff, 0, 0, 0],
        ],
        33 => [[0, 0, 0, 0], [0x37fff, 0, 0, 0], [0, 0, 0, 0], [0, 0, 0, 0]],
        40 => [
            [0x33ebfffffff, 0, 0, 0],
            [0x1dff, 0, 0, 0],
            [0x3ffffff, 0, 0, 0],
            [0x7ffff, 0, 0, 0],
        ],
        48 | 66 | 67 | 73 => [
            [0x3ffffff, 0, 0, 0],
            [0x7ffffff, 0, 0, 0],
            [0x1ffff, 0, 0, 0],
            [0xffffffffffffffff, 0x1ff, 0, 0],
        ],
        51 | 52 | 53 | 61 | 62 | 123 => [
            [0x7c03fffffffffa, 0, 0, 0],
            [0x3ffff, 0, 0, 0],
            [0xff1ffff, 0, 0, 0],
            [0, 0, 0, 0],
        ],
        54 | 60 | 64 | 65 => [
            [0xfffffffff, 0, 0, 0],
            [0x1007ff07fff, 0, 0, 0],
            [0x103ffff, 0, 0, 0],
            [0xffffffffffffffff, 0x1ffffff007f, 0, 0],
        ],
        55 => [
            [0xff, 0, 0, 0],
            [0xff, 0, 0, 0],
            [0, 0, 0, 0],
            [0x7ff, 0, 0, 0],
        ],
        56 => [[0, 0, 0, 0], [0, 0, 0, 0], [0x5ff, 0, 0, 0], [0, 0, 0, 0]],
        57 => [
            [0xfff, 0, 0, 0],
            [0x1fffff, 0, 0, 0],
            [0, 0, 0, 0],
            [0xff, 0, 0, 0],
        ],
        58 | 72 | 144 => [
            [0x1f, 0, 0, 0],
            [0xc0f, 0, 0, 0],
            [0x7fffff, 0, 0, 0],
            [0x3fffffffff, 0, 0, 0],
        ],
        59 => [
            [0x71b940e3f, 0, 0, 0],
            [0x3f7, 0, 0, 0],
            [0x36febc7, 0, 0, 0],
            [0x3fffffff, 0, 0, 0],
        ],
        63 => [
            [0, 0, 0, 0],
            [0, 0, 0, 0],
            [0x7ffffff, 0, 0, 0],
            [0x3f, 0, 0, 0],
        ],
        68 => [
            [0xffffda, 0, 0, 0],
            [0x1fe7, 0, 0, 0],
            [0xf, 0, 0, 0],
            [0x1ffffc7f, 0, 0, 0],
        ],
        74 | 78 | 82 | 114 => [
            [0x400febfffffff, 0, 0, 0],
            [0x3ffdff, 0, 0, 0],
            [0x3ffffff, 0, 0, 0],
            [0xf03fffff1e240405, 0x1ffffff, 0, 0],
        ],
        75 | 79 | 119 | 120 => [
            [0xe0000fff7f7, 0, 0, 0],
            [0x3f, 0, 0, 0],
            [0x1effffffb, 0, 0, 0],
            [0x3ffc3fffffffffff, 0, 0, 0],
        ],
        76 | 77 | 106 => [
            [0x7fffffff, 0, 0, 0],
            [0x3fc0307c7f01fff, 0, 0, 0],
            [0x7cdef1f, 0, 0, 0],
            [0xf07fffffffffdfff, 0x7ff, 0, 0],
        ],
        80 | 81 | 90 | 126 => [
            [0x3f0efffffbf7, 0, 0, 0],
            [0xfecf, 0, 0, 0],
            [0xfc3ffffff, 0, 0, 0],
            [0xf1ffdf1e03ffffff, 0x1ffffff065fffeff, 0, 0],
        ],
        83 | 84 => [
            [0x1ffff, 0, 0, 0],
            [0x3fff, 0, 0, 0],
            [0x3f1dfff, 0, 0, 0],
            [0x3fffff3fefffff7f, 0, 0, 0],
        ],
        85 => [
            [0x8000038305feb, 0, 0, 0],
            [0xc27, 0, 0, 0],
            [0x100ffa9ebc7, 0, 0, 0],
            [0x3ffda379ff7fffff, 0xff00ce, 0, 0],
        ],
        89 => [
            [0x1fc3ffe3b3, 0, 0, 0],
            [0x7c033fddf, 0, 0, 0],
            [0x7c1cde317, 0, 0, 0],
            [0x7fbdff67c5b, 0, 0, 0],
        ],
        91 | 92 | 111 => [
            [0x3f1fc03fffcf, 0, 0, 0],
            [0xffff00c3ff9fff, 0, 0, 0],
            [0xc0300307, 0, 0, 0],
            [0, 0, 0, 0],
        ],
        93 => [
            [0xf, 0, 0, 0],
            [0x3fc03, 0, 0, 0],
            [0x3f007ff, 0x800000000, 0, 0],
            [0x7f1bc7f, 0, 0, 0],
        ],
        94 => [
            [0x80008c1ffffb7, 0, 0, 0],
            [0xc1f, 0, 0, 0],
            [0x3c1cdef8f, 0, 0, 0],
            [0xf09c070fffe, 0, 0, 0],
        ],
        95 => [
            [0x8000001ffe1b7, 0, 0, 0],
            [0x201c0f, 0, 0, 0],
            [0xfc1e9e387, 0, 0, 0],
            [0xf00c01fffe7f9ffe, 0x3ff, 0, 0],
        ],
        96 => [
            [0x7c1ffefff, 0, 0, 0],
            [0x9c3f, 0, 0, 0],
            [0x3c1cdef87, 0, 0, 0],
            [0x1fe37fc6e, 0, 0, 0],
        ],
        97 => [
            [0x3ffe, 0, 0, 0],
            [0xfff, 0, 0, 0],
            [0, 0, 0, 0],
            [0x1f, 0, 0, 0],
        ],
        98 => [
            [0x7ffe, 0, 0, 0],
            [0x3fff, 0, 0, 0],
            [0x7ffe, 0, 0, 0],
            [0x1ff, 0, 0, 0],
        ],
        99 | 108 => [
            [0x39e89fcf, 0, 0, 0],
            [0x1000001ff7f, 0, 0, 0],
            [0x103ffff, 0, 0, 0],
            [0x7fffffbffff, 0, 0, 0],
        ],
        100 => [
            [0xf0ebfffffff, 0, 0, 0],
            [0x2f9fff, 0, 0, 0],
            [0x7ffffff, 0, 0, 0],
            [0xfffffff7ffffeffd, 0xfffffffffc00ffff, 0, 0],
        ],
        101 | 102 => [
            [0x300feffffff, 0, 0, 0],
            [0x7ffff, 0, 0, 0],
            [0x7ffff, 0, 0, 0],
            [0x7fffff9, 0, 0, 0],
        ],
        103 => [
            [0, 0, 0, 0],
            [0x7ff, 0xffc000000, 0, 0],
            [0x1f, 0, 0, 0],
            [0xfffffffff, 0, 0, 0],
        ],
        104 | 112 => [
            [0, 0, 0, 0],
            [0x6fffdf, 0x4000000, 0, 0],
            [0xf, 0, 0, 0],
            [0xfffc3ff7fffffc3f, 0x3fc0001ff, 0, 0],
        ],
        105 => [
            [0x1fff07f7, 0, 0, 0],
            [0x1ff, 0, 0, 0],
            [0x233f, 0, 0, 0],
            [0, 0, 0, 0],
        ],
        107 => [
            [0x79e89fcf, 0, 0, 0],
            [0x10000004d3f, 0, 0, 0],
            [0x103ffff, 0, 0, 0],
            [0x7effc7ffffff, 0, 0, 0],
        ],
        109 => [
            [0xfff03ff, 0, 0, 0],
            [0x7fcf5, 0, 0, 0],
            [0x3fc1b5efff, 0, 0, 0],
            [0x1ffffffffffff, 0, 0, 0],
        ],
        110 => [
            [0xfff5eff, 0, 0, 0],
            [0xf8f03875, 0, 0, 0],
            [0x7c185eb83, 0, 0, 0],
            [0xffffffffff, 0, 0, 0],
        ],
        113 => [
            [0xffc000003ffffbf, 0, 0, 0],
            [0xffffff, 0, 0, 0],
            [0x3c1cdef8f, 0, 0, 0],
            [0xffffffffffffffff, 0x1fff, 0, 0],
        ],
        115 => [
            [0x3fffff, 0, 0, 0],
            [0x7f, 0, 0, 0],
            [0, 0, 0, 0],
            [0xfffffff, 0, 0, 0],
        ],
        116 => [
            [0x1cfff0737, 0, 0, 0],
            [0x1dff, 0, 0, 0],
            [0xfff, 0, 0, 0],
            [0xf3ffffffffffffff, 0xfff, 0, 0],
        ],
        117 => [
            [0xe7b85, 0, 0, 0],
            [0x6f, 0, 0, 0],
            [0, 0, 0, 0],
            [0, 0, 0, 0],
        ],
        121 => [
            [0x1dfff07f7, 0, 0, 0],
            [0x3005ff, 0, 0, 0],
            [0xcef0a7ff, 0, 0, 0],
            [0xf1fc7ffbc3fffff7, 0x7fff08fe, 0, 0],
        ],
        122 => [
            [0x3c0f0ebfffffff, 0, 0, 0],
            [0x6f9fff, 0, 0, 0],
            [0x7ffffff, 0, 0, 0],
            [0x3e3fffffff, 0, 0, 0],
        ],
        124 => [
            [0x3f13c7a, 0, 0, 0],
            [0xff, 0, 0, 0],
            [0, 0, 0, 0],
            [0x7f9fe, 0, 0, 0],
        ],
        125 => [
            [0x7ffe01f, 0, 0, 0],
            [0xc010040f, 0, 0, 0],
            [0xfc1e9e387, 0, 0, 0],
            [0x1c0fffff8, 0, 0, 0],
        ],
        127 => [
            [0x43ffe01f, 0, 0, 0],
            [0x30040f, 0, 0, 0],
            [0xfc1e9e387, 0, 0, 0],
            [0x18ffff, 0, 0, 0],
        ],
        128 => [
            [0x4000181e03897, 0, 0, 0],
            [0x3fc, 0, 0, 0],
            [0, 0, 0, 0],
            [0x6e038fe1fbfe, 0, 0, 0],
        ],
        129 => [
            [0x7ffb, 0, 0, 0],
            [0x1ff, 0, 0, 0],
            [0, 0, 0, 0],
            [0x7ffbff, 0, 0, 0],
        ],
        130 => [
            [0xf0e1353aeff, 0, 0, 0],
            [0x3ffd, 0, 0, 0],
            [0, 0, 0, 0],
            [0xffffe1807c038041, 0x7f, 0, 0],
        ],
        131 => [
            [0x7fffff, 0, 0, 0],
            [0x1ff, 0, 0, 0],
            [0xff, 0, 0, 0],
            [0x7fffffffff, 0, 0, 0],
        ],
        132 => [
            [0xf, 0, 0, 0],
            [0x7e, 0, 0, 0],
            [0xe, 0, 0, 0],
            [0x1fdffffffe, 0, 0, 0],
        ],
        134 => [
            [0x7f, 0, 0, 0],
            [0x7f, 0, 0, 0],
            [0x7f, 0, 0, 0],
            [0x7f, 0, 0, 0],
        ],
        139 => [
            [0x1ebb9c0407, 0, 0, 0],
            [0xff, 0, 0, 0],
            [0x80000f, 0, 0, 0],
            [0x7ffffff, 0, 0, 0],
        ],
        140 => [
            [0x7f, 0, 0, 0],
            [0xe, 0, 0, 0],
            [0x3f00fff, 0, 0, 0],
            [0x1fffdffe, 0, 0, 0],
        ],
        141 => [
            [0xffffffff, 0, 0, 0],
            [0x7fc9, 0, 0, 0],
            [0x7fefffff, 0, 0, 0],
            [0x3dfffb81fffff, 0, 0, 0],
        ],
        142 => [
            [0x80fc582e, 0, 0, 0],
            [0x7ffff, 0, 0, 0],
            [0xfff, 0, 0, 0],
            [0x3fffff9fffffff, 0, 0, 0],
        ],
        143 => [[0x1f, 0, 0, 0], [0, 0, 0, 0], [0, 0, 0, 0], [0, 0, 0, 0]],
        145 => [
            [0x1f, 0, 0, 0],
            [0x7e, 0, 0, 0],
            [0xe, 0, 0, 0],
            [0x3fffdffffffe, 0, 0, 0],
        ],
        146 => [
            [0x1fffff, 0, 0, 0],
            [0x1ff, 0, 0, 0],
            [0x1f, 0, 0, 0],
            [0xffffffffffffe, 0, 0, 0],
        ],
        147 => [
            [0x7fffff, 0, 0, 0],
            [0xffffffff, 0, 0, 0],
            [0x1ff, 0, 0, 0],
            [0xffffffff81e03fff, 0xf, 0, 0],
        ],
        148 => [
            [0xff87f, 0, 0, 0],
            [0x1fffffe, 0, 0, 0],
            [0x3fe, 0, 0, 0],
            [0xfffffffffffffffe, 0x3f, 0, 0],
        ],
        149 => [
            [0xf, 0, 0, 0],
            [0x1fffc03, 0, 0, 0],
            [0x7f007ff, 0x800000000, 0, 0],
            [0xfff1bc7f, 0, 0, 0],
        ],
        150 => [
            [0x198c9fcf, 0, 0, 0],
            [0x30046901f4f, 0, 0, 0],
            [0x100c797, 0, 0, 0],
            [0x1ffff, 0, 0, 0],
        ],
        151 => [
            [0xc1ffe21f, 0, 0, 0],
            [0x10180f, 0, 0, 0],
            [0xfc1e9e387, 0, 0, 0],
            [0xf1fc01ffc031fffe, 0x30ffff0000003ff, 0, 0],
        ],
        152 => [
            [0xff87f, 0, 0, 0],
            [0x1ffe, 0, 0, 0],
            [0x3e, 0, 0, 0],
            [0x7ffffeffffe, 0, 0, 0],
        ],
        153 => [
            [0x3fffff, 0, 0, 0],
            [0x1ff, 0, 0, 0],
            [0x1f, 0, 0, 0],
            [0xfffffffffffffffe, 0xf, 0, 0],
        ],
        154 => [
            [0xf83f, 0, 0, 0],
            [0x7e, 0, 0, 0],
            [0xf87e, 0, 0, 0],
            [0xfffdffffffffa, 0, 0, 0],
        ],
        155 => [
            [0xffffff, 0, 0, 0],
            [0x3ffffffff, 0, 0, 0],
            [0x1ff, 0, 0, 0],
            [0xffffffff81e03fff, 0x7fff8f, 0, 0],
        ],
        157 => [
            [0, 0, 0, 0],
            [0, 0, 0, 0],
            [0x2, 0, 0, 0],
            [0x1ffe, 0, 0, 0],
        ],
        158 => [
            [0x3fffff, 0, 0, 0],
            [0x7fff, 0, 0, 0],
            [0x3f, 0, 0, 0],
            [0xfffffff, 0, 0, 0],
        ],
        159 => [
            [0x1807ffbff, 0, 0, 0],
            [0xffffe, 0, 0, 0],
            [0x3ffefe, 0, 0, 0],
            [0x1ffe019fe75fe, 0, 0, 0],
        ],
        160 => [
            [0x787f, 0, 0, 0],
            [0x1e, 0, 0, 0],
            [0, 0, 0, 0],
            [0x3f8000fffe, 0, 0, 0],
        ],
        161 => [
            [0x1fffff, 0, 0, 0],
            [0x3f, 0, 0, 0],
            [0x3fffff, 0, 0, 0],
            [0x1ffffffff, 0, 0, 0],
        ],
        162 => [
            [0x1807ff8ff, 0, 0, 0],
            [0x1ffffffe, 0, 0, 0],
            [0x7dfefe, 0, 0, 0],
            [0x3fffffdffffffffe, 0, 0, 0],
        ],
        163 => [
            [0x201807ffbff, 0, 0, 0],
            [0xffffe, 0, 0, 0],
            [0x3ffefe, 0, 0, 0],
            [0xffffffe019fe75fe, 0x1ff, 0, 0],
        ],
        164 => [
            [0x1807ff8ff, 0, 0, 0],
            [0x1ffffffe, 0, 0, 0],
            [0x7dfefe, 0, 0, 0],
            [0xffffffdffffffffe, 0x1fffff, 0, 0],
        ],
        165 => [
            [0x7fff, 0, 0, 0],
            [0x7fe, 0, 0, 0],
            [0, 0, 0, 0],
            [0x3bf53bffe, 0, 0, 0],
        ],
        166 => [
            [0xffffbff, 0, 0, 0],
            [0xfe, 0, 0, 0],
            [0x7e, 0, 0, 0],
            [0xffffffffe, 0, 0, 0],
        ],
        167 => [
            [0x81f, 0, 0, 0],
            [0x7e, 0, 0, 0],
            [0xe, 0, 0, 0],
            [0x1f83fffdffffffe, 0, 0, 0],
        ],
        169 => [
            [0x7f81e, 0, 0, 0],
            [0xe1dffe, 0, 0, 0],
            [0x1bfe07ffe, 0, 0, 0],
            [0x7f87fff81e3fffe, 0, 0, 0],
        ],
        170 => [
            [0x27f87e, 0, 0, 0],
            [0x183e, 0, 0, 0],
            [0, 0, 0, 0],
            [0xffffe, 0, 0, 0],
        ],
        171 => [
            [0x1fffffffe, 0, 0, 0],
            [0, 0, 0, 0],
            [0, 0, 0, 0],
            [0, 0, 0, 0],
        ],
        172 => [
            [0x27f87e, 0, 0, 0],
            [0x183e, 0, 0, 0],
            [0, 0, 0, 0],
            [0xf800ffffe, 0, 0, 0],
        ],
        173 => [[0x3fe, 0, 0, 0], [0, 0, 0, 0], [0, 0, 0, 0], [0, 0, 0, 0]],
        174 => [
            [0, 0, 0, 0],
            [0, 0, 0, 0],
            [0, 0, 0, 0],
            [0x7f0001000000, 0, 0, 0],
        ],
        175 => [[0xfe, 0, 0, 0], [0x6, 0, 0, 0], [0, 0, 0, 0], [0, 0, 0, 0]],
        176 => [
            [0xffe, 0, 0, 0],
            [0x1fffff, 0, 0, 0],
            [0, 0, 0, 0],
            [0xff, 0, 0, 0],
        ],
        _ => [[0; 4]; 4],
    };
    let mut actions = Vec::new();
    for (group, words) in masks.into_iter().enumerate() {
        for (word, mut bits) in words.into_iter().enumerate() {
            while bits != 0 {
                let id = word * 64 + bits.trailing_zeros() as usize;
                actions.push(MonsterAction {
                    group: group as u8,
                    id: id as u8,
                });
                bits &= bits - 1;
            }
        }
    }
    actions
}

#[cfg(test)]
mod tests {
    use super::*;

    fn variant(species: u8, id: u8) -> Variant {
        variants(species)
            .into_iter()
            .find(|variant| variant.id == id)
            .unwrap()
    }

    #[test]
    fn every_species_has_complete_ordered_variant_descriptions() {
        for species in 1..NAMES.len() as u8 {
            let variants = variants(species);
            assert_eq!(variants[0].id, 0);
            assert_eq!(variants[0].model_suffix, "");
            assert!(variants.windows(2).all(|pair| pair[0].id < pair[1].id));
            for variant in variants {
                assert!(variant.id <= 16);
                assert!(!variant.name.is_empty());
                assert!(matches!(
                    variant.model_suffix,
                    "" | "_b" | "_c" | "_d" | "_e"
                ));
            }
        }
    }

    #[test]
    fn the_same_number_keeps_each_species_event_and_model() {
        let higanjima = variant(21, 12);
        let saga = variant(155, 12);
        assert_ne!(higanjima.name, saga.name);
        assert_eq!(higanjima.model_suffix, "_c");
        assert_eq!(saga.model_suffix, "_c");
        assert_eq!(variant(4, 6).model_suffix, "_b");
        assert_eq!(variant(9, 6).model_suffix, "_b");
        assert_eq!(variant(6, 6).model_suffix, "_c");
        assert_eq!(variant(101, 6).model_suffix, "_c");
        assert!(
            variants(146)
                .iter()
                .all(|variant| !matches!(variant.id, 6 | 8 | 12 | 15))
        );
    }

    #[test]
    fn special_models_do_not_inherit_numeric_query_aliases() {
        assert_eq!(variant(53, 8).model_suffix, "_b");
        assert_eq!(variant(53, 15).model_suffix, "");
        assert_eq!(variant(95, 8).model_suffix, "_b");
        assert_eq!(variant(95, 15).model_suffix, "_c");
        assert_eq!(variant(42, 8).model_suffix, "_c");
        assert_eq!(variant(106, 15).model_suffix, "");
    }
}
