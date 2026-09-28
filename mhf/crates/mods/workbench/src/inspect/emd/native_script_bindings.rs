//! Observed bindings for the ZZ HD client supported by the workbench.
//!
//! DLL SHA-256: 95c580195f4080d2e9582c8c9df36abeb280476e088b6366583c5f138da8f301.
//! This is an explicit client profile, not a version inferred from EMD counts.
//! All consumers call 111A88A0 and bind descriptor[1][slot]. Parameter values
//! below are observed calls, not bounds on the native byte argument.
//! See resource/docs/emd.md for selectors and native call-site evidence.

use super::script_links::NativeBinding;

pub(super) const ZZ_HD_BINDINGS: &[NativeBinding] = &[
    // 10E61520: ordinary branches use 185+p / 188+p; the special branch
    // leaves p=0 untouched and uses 272+(p-1) for observed p=1/2.
    binding(146, 185, "常规分支 A", 1, Some(0)),
    binding(146, 186, "常规分支 A", 1, Some(1)),
    binding(146, 187, "常规分支 A", 1, Some(2)),
    binding(146, 188, "常规分支 B", 1, Some(0)),
    binding(146, 189, "常规分支 B", 1, Some(1)),
    binding(146, 190, "常规分支 B", 1, Some(2)),
    binding(146, 272, "特殊分支", 1, Some(1)),
    binding(146, 273, "特殊分支", 1, Some(2)),
    // 10E7AED0: each selected triple supplies slots 2, 3 and 4.
    binding(147, 191, "分支 A", 2, None),
    binding(147, 192, "分支 A", 3, None),
    binding(147, 193, "分支 A", 4, None),
    binding(147, 194, "分支 B", 2, None),
    binding(147, 195, "分支 B", 3, None),
    binding(147, 196, "分支 B", 4, None),
    binding(147, 197, "分支 C", 2, None),
    binding(147, 198, "分支 C", 3, None),
    binding(147, 199, "分支 C", 4, None),
    // 100B2500: 307+p / 310+p; callers include 100B2160, 100B2FD0 and
    // the inherited 10E62F90, which respectively establish p=0, 1 and 2.
    binding(153, 307, "分支 A", 1, Some(0)),
    binding(153, 308, "分支 A", 1, Some(1)),
    binding(153, 309, "分支 A", 1, Some(2)),
    binding(153, 310, "分支 B", 1, Some(0)),
    binding(153, 311, "分支 B", 1, Some(1)),
    binding(153, 312, "分支 B", 1, Some(2)),
    binding(160, 362, "固定条目", 3, None), // 100FB1D0
    binding(167, 435, "固定条目", 3, None), // 1015E310
    binding(172, 479, "固定条目", 3, None), // 10188B90
    binding(175, 477, "固定条目", 0, None), // 101939C0
    binding(175, 478, "固定条目", 1, None),
];

const fn binding(
    species: u8,
    record: usize,
    group: &'static str,
    slot: u8,
    parameter: Option<u8>,
) -> NativeBinding {
    NativeBinding {
        species,
        record,
        group,
        slot,
        parameter,
    }
}
