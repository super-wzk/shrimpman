// Scattered compiled text operands, validated against the unpacked client.
// Structured pointer records live in layout.rs; text lives in the Translation domain's locales/ja-JP.jsonl.
use super::Literal;

pub const LITERALS: &[Literal] = &[
    Literal {
        rva: 0x0197A5C4,
        sites: &[0x000425C1],
    },
    Literal {
        rva: 0x01986160,
        sites: &[0x000FCC6B, 0x000FCCD8, 0x000FCDB6, 0x000FCE2C],
    },
    Literal {
        rva: 0x01993F20,
        sites: &[0x001BDE8F, 0x001BE0A4],
    },
    Literal {
        rva: 0x01993F84,
        sites: &[0x001CE475],
    },
    Literal {
        rva: 0x01993FB0,
        sites: &[0x001D7DF1],
    },
    Literal {
        rva: 0x01995610,
        sites: &[0x0036BDAD],
    },
    Literal {
        rva: 0x01995618,
        sites: &[0x0036BDD2],
    },
    Literal {
        rva: 0x01995620,
        sites: &[0x0036BE30],
    },
    Literal {
        rva: 0x01995630,
        sites: &[
            0x0036C350, 0x0036C3BE, 0x0036C42C, 0x0036C49A, 0x0036C508, 0x0036C576, 0x00811874,
            0x00811905, 0x00811927, 0x0081194A, 0x00811970, 0x00811995,
        ],
    },
    Literal {
        rva: 0x019959A8,
        sites: &[0x003B02EA, 0x003B0E57],
    },
    Literal {
        rva: 0x01995BB4,
        sites: &[0x003D398C, 0x003D3FD4, 0x004A4CEA],
    },
    Literal {
        rva: 0x01995BC0,
        sites: &[0x003D39B7, 0x003D400A, 0x004A4D15],
    },
    Literal {
        rva: 0x01995E44,
        sites: &[0x003FB2F1, 0x003FBC90, 0x006BBF72, 0x006E560E, 0x014D491A],
    },
    Literal {
        rva: 0x0199607C,
        sites: &[0x00430C50],
    },
    Literal {
        rva: 0x01996204,
        sites: &[0x0045455C, 0x00454671, 0x00454785],
    },
    Literal {
        rva: 0x0199644C,
        sites: &[0x00491E5D, 0x00491F78],
    },
    Literal {
        rva: 0x0199661C,
        sites: &[0x004D5106, 0x004D5512, 0x004D58C6],
    },
    Literal {
        rva: 0x0199662C,
        sites: &[
            0x004EE14B, 0x004EE17B, 0x004EF4C3, 0x004EF4ED, 0x004EF5A9, 0x004F07E6, 0x004F080B,
            0x004F0932, 0x004F095A, 0x004F1582, 0x004F15EC, 0x004F1613, 0x004F163A, 0x004F2229,
            0x004F22EA, 0x004F2315, 0x004F2330, 0x004F235B, 0x004F2CA2, 0x004F2CBA, 0x004F2CE2,
            0x004F2CFA, 0x004F3445, 0x004F346F, 0x00575552, 0x00575642, 0x00575660, 0x005756B4,
            0x005768DF, 0x005769CF, 0x005769ED, 0x00576A41, 0x0057770C, 0x005777B2, 0x005777FA,
            0x00578761, 0x005787DD, 0x00578873, 0x0057950E, 0x0057958A, 0x00579620, 0x00579F09,
            0x00579F7F, 0x0057AB05, 0x0057AB74, 0x0057B7B0, 0x0057B81F, 0x0057C461, 0x0057C4D0,
            0x0057D2C9, 0x0057E02C, 0x0057E849, 0x0057E85E, 0x0057EEE9, 0x0057EEFE, 0x0057F59C,
            0x0057F5B1, 0x0057FBE5, 0x0057FBFA, 0x005804FC, 0x00580514, 0x0058055B, 0x0187CE28,
        ],
    },
    Literal {
        rva: 0x01996644,
        sites: &[
            0x004EE1BA, 0x004EE1E1, 0x004EF50D, 0x004EF534, 0x004EF55B, 0x004EF582, 0x004EF5D0,
            0x004F0833, 0x004F085E, 0x004F0889, 0x004F08B4, 0x004F08DF, 0x004F090A, 0x004F0985,
            0x004F15A2, 0x004F15C7, 0x004F1661, 0x004F2241, 0x004F2269, 0x004F2294, 0x004F22BF,
            0x004F2373, 0x0057556B, 0x0057559D, 0x005755D2, 0x00575607, 0x00575679, 0x005756CD,
            0x00575702, 0x005768F8, 0x0057692A, 0x0057695F, 0x00576994, 0x00576A06, 0x00576A5A,
            0x00576A8F, 0x00577729, 0x00577753, 0x00577784, 0x005777CC, 0x00577811, 0x00578779,
            0x005787AB, 0x00578894, 0x005788C9, 0x00579526, 0x00579558, 0x00579641, 0x00579676,
            0x00579F21, 0x00579F55, 0x00579F94, 0x0057AB2F, 0x0057AB4F, 0x0057ABE9, 0x0057B7DA,
            0x0057B7FA, 0x0057B894, 0x0057C48B, 0x0057C4AB, 0x0057C545, 0x0057D2F1, 0x0057D319,
            0x0057D36C, 0x0057D397, 0x0057D3ED, 0x0057D418, 0x0057D46E, 0x0057E054, 0x0057E07C,
            0x0057E0CF, 0x0057E0FA, 0x0057E150, 0x0057E17B, 0x0057E1D1, 0x0057E873, 0x0057EF13,
            0x0057F5C6, 0x0057FC0F, 0x0058052C, 0x00580570,
        ],
    },
    Literal {
        rva: 0x0199664C,
        sites: &[
            0x004EE205, 0x004EF5F4, 0x004F09AD, 0x004F1685, 0x004F239B, 0x004F2D22, 0x004F348C,
        ],
    },
    Literal {
        rva: 0x01996660,
        sites: &[
            0x004EE229, 0x004EF618, 0x004F09D5, 0x004F16A9, 0x004F23C6, 0x004F2D4D, 0x004F34B0,
            0x00575748, 0x00575767, 0x00576AD5, 0x00576AF4, 0x00577852, 0x00578812, 0x00578832,
            0x00578853, 0x00578912, 0x00578933, 0x005795BF, 0x005795DF, 0x00579600, 0x005796BF,
            0x005796E0, 0x00579FD5, 0x0057AB9B, 0x0057ABC2, 0x0057AC19, 0x0057B846, 0x0057B86D,
            0x0057B8CB, 0x0057C4F7, 0x0057C51E, 0x0057C57C, 0x0057D341, 0x0057D3C2, 0x0057D443,
            0x0057D4B2, 0x0057E0A4, 0x0057E125, 0x0057E1A6, 0x0057E20A, 0x0057E8B4, 0x0057EF54,
            0x0057F607, 0x0057FC49, 0x005805AA, 0x0187CE24,
        ],
    },
    Literal {
        rva: 0x019966CC,
        sites: &[0x0057C699, 0x00583841, 0x006881D4],
    },
    Literal {
        rva: 0x019966D4,
        sites: &[0x0057C728],
    },
    Literal {
        rva: 0x019966EC,
        sites: &[0x0057C741],
    },
    Literal {
        rva: 0x01996718,
        sites: &[0x00580029, 0x00580040, 0x005802C9, 0x005802E0],
    },
    Literal {
        rva: 0x0199673C,
        sites: &[0x005837E3],
    },
    Literal {
        rva: 0x01996748,
        sites: &[0x00583E85],
    },
    Literal {
        rva: 0x01996778,
        sites: &[0x00583E9F],
    },
    Literal {
        rva: 0x0199679C,
        sites: &[0x00583EA6],
    },
    Literal {
        rva: 0x019967B8,
        sites: &[0x00583EAD],
    },
    Literal {
        rva: 0x019967D4,
        sites: &[0x00583EB4],
    },
    Literal {
        rva: 0x019967FC,
        sites: &[0x00583EBB],
    },
    Literal {
        rva: 0x01996814,
        sites: &[0x00583EC2],
    },
    Literal {
        rva: 0x01996900,
        sites: &[0x005C4D6D],
    },
    Literal {
        rva: 0x01996AB4,
        sites: &[0x005E190B, 0x005E1967, 0x005E19C6, 0x005E1A25, 0x005E1A81],
    },
    Literal {
        rva: 0x01997600,
        sites: &[0x00648A6D],
    },
    Literal {
        rva: 0x0199778C,
        sites: &[0x0068D3CF],
    },
    Literal {
        rva: 0x01997898,
        sites: &[0x00697333],
    },
    Literal {
        rva: 0x019978A8,
        sites: &[0x00697DA3],
    },
    Literal {
        rva: 0x019978E4,
        sites: &[0x006A5D74],
    },
    Literal {
        rva: 0x01997980,
        sites: &[0x006B2A89],
    },
    Literal {
        rva: 0x0199799C,
        sites: &[0x006B2B0F],
    },
    Literal {
        rva: 0x019979DC,
        sites: &[0x006C5319],
    },
    Literal {
        rva: 0x01997A08,
        sites: &[0x006C9D00],
    },
    Literal {
        rva: 0x01997D54,
        sites: &[0x00703133],
    },
    Literal {
        rva: 0x01997F34,
        sites: &[0x0073C435],
    },
    Literal {
        rva: 0x01997F40,
        sites: &[0x0073FA04],
    },
    Literal {
        rva: 0x01997F4C,
        sites: &[0x0073FA0B],
    },
    Literal {
        rva: 0x01997F54,
        sites: &[0x0073FA12],
    },
    Literal {
        rva: 0x01997F64,
        sites: &[0x0073FA19],
    },
    Literal {
        rva: 0x01997F78,
        sites: &[0x00742E0E],
    },
    Literal {
        rva: 0x01997FB8,
        sites: &[0x007506DE],
    },
    Literal {
        rva: 0x01997FC8,
        sites: &[0x0075F92B],
    },
    Literal {
        rva: 0x01997FD8,
        sites: &[0x0075F965],
    },
    Literal {
        rva: 0x0199802C,
        sites: &[0x007644FF],
    },
    Literal {
        rva: 0x01998038,
        sites: &[0x0076450F],
    },
    Literal {
        rva: 0x0199804C,
        sites: &[0x0076F17B],
    },
    Literal {
        rva: 0x01998088,
        sites: &[0x007764C3],
    },
    Literal {
        rva: 0x0199C21C,
        sites: &[0x007B2B15, 0x007B2B51, 0x007B2B8D],
    },
    Literal {
        rva: 0x0199C3B4,
        sites: &[0x007BB73B],
    },
    Literal {
        rva: 0x0199CD48,
        sites: &[0x00812292],
    },
    Literal {
        rva: 0x019A0244,
        sites: &[0x0093BA58],
    },
    Literal {
        rva: 0x019A0290,
        sites: &[0x0094D349],
    },
    Literal {
        rva: 0x019A02AC,
        sites: &[0x00962561],
    },
    Literal {
        rva: 0x019A02C4,
        sites: &[0x00B2B2A6],
    },
    Literal {
        rva: 0x019A02C8,
        sites: &[0x00975E51],
    },
    Literal {
        rva: 0x019A03C0,
        sites: &[0x009A5832],
    },
    Literal {
        rva: 0x019A03D8,
        sites: &[0x00A3F008],
    },
    Literal {
        rva: 0x019A03EC,
        sites: &[0x00A3F047, 0x00A3F09F],
    },
    Literal {
        rva: 0x019A03FC,
        sites: &[0x00A3F06A],
    },
    Literal {
        rva: 0x019A04B0,
        sites: &[
            0x00B26DA4, 0x00B27122, 0x00B2B532, 0x00B2B572, 0x00B2B5E2, 0x00B2B703, 0x00B2B742,
            0x00B2B7D2, 0x00B2B882, 0x00B2B9E2, 0x0187CE20,
        ],
    },
    Literal {
        rva: 0x019A0518,
        sites: &[0x00B255FF],
    },
    Literal {
        rva: 0x019A0580,
        sites: &[0x00B2A29F],
    },
    Literal {
        rva: 0x019A0588,
        sites: &[0x00B2A535, 0x00B2A607, 0x00B2A771, 0x00B2A87A, 0x00B2AC67],
    },
    Literal {
        rva: 0x019A058C,
        sites: &[0x00B2A6D3, 0x00B2ABA5],
    },
    Literal {
        rva: 0x019A0590,
        sites: &[0x00B2AB4F, 0x00B2AEE7],
    },
    Literal {
        rva: 0x019A0598,
        sites: &[0x00B2AF40],
    },
    Literal {
        rva: 0x019A05A8,
        sites: &[0x00B30C24],
    },
    Literal {
        rva: 0x019A156C,
        sites: &[0x003E0CA8, 0x003E1B37],
    },
    Literal {
        rva: 0x019A157C,
        sites: &[0x00B74D53],
    },
    Literal {
        rva: 0x019A1658,
        sites: &[0x00B7962C],
    },
    Literal {
        rva: 0x019A1660,
        sites: &[0x00B7964D],
    },
    Literal {
        rva: 0x019A1674,
        sites: &[0x00B79684],
    },
    Literal {
        rva: 0x019A1680,
        sites: &[0x00B796B9],
    },
    Literal {
        rva: 0x019A168C,
        sites: &[0x00B796F1],
    },
    Literal {
        rva: 0x019A1694,
        sites: &[0x00B79710],
    },
    Literal {
        rva: 0x019A169C,
        sites: &[0x00B79736],
    },
    Literal {
        rva: 0x019A16A4,
        sites: &[0x00B7978A],
    },
    Literal {
        rva: 0x019A16AC,
        sites: &[0x00B7979B],
    },
    Literal {
        rva: 0x019A16BC,
        sites: &[0x00B797AC],
    },
    Literal {
        rva: 0x019A16C8,
        sites: &[0x00B797BD],
    },
    Literal {
        rva: 0x019A16D0,
        sites: &[0x00B7980D],
    },
    Literal {
        rva: 0x019A16E0,
        sites: &[0x00B79841],
    },
    Literal {
        rva: 0x019A16E8,
        sites: &[0x00B7987A, 0x00B798A1],
    },
    Literal {
        rva: 0x019A16F4,
        sites: &[0x00B798C7],
    },
    Literal {
        rva: 0x019A1704,
        sites: &[0x00B798F0],
    },
    Literal {
        rva: 0x019A170C,
        sites: &[0x00B79919],
    },
    Literal {
        rva: 0x019A1714,
        sites: &[0x00B79927],
    },
    Literal {
        rva: 0x019A1728,
        sites: &[0x00B79989],
    },
    Literal {
        rva: 0x019A1750,
        sites: &[0x00B94B23, 0x00B94BD2],
    },
    Literal {
        rva: 0x019A1760,
        sites: &[0x00B95878, 0x00B95968],
    },
    Literal {
        rva: 0x019A53C8,
        sites: &[0x0136E773],
    },
    Literal {
        rva: 0x019A6BD0,
        sites: &[0x014D4A44],
    },
    Literal {
        rva: 0x019B0EE4,
        sites: &[0x0150D357],
    },
    Literal {
        rva: 0x019B0F30,
        sites: &[0x0150D631],
    },
    Literal {
        rva: 0x019B1600,
        sites: &[0x0154D675],
    },
    Literal {
        rva: 0x019B1614,
        sites: &[0x0154EB14],
    },
    Literal {
        rva: 0x019B2160,
        sites: &[0x01572A8A],
    },
    Literal {
        rva: 0x019B2184,
        sites: &[0x01573C5A],
    },
];
