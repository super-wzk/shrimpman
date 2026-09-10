//! C declarations derived from safer-ffi layout metadata. Only naming,
//! preprocessor helpers and file composition live here; no C table is copied.
use super::*;
use safer_ffi::headers::languages::C;
pub use safer_ffi::headers::{Definer, HashSetDefiner};
use safer_ffi::layout::{CType, ReprC};
use std::{
    collections::HashSet,
    fmt::Debug,
    fs,
    io::{self, Write},
    path::Path,
};

pub fn alias<T: ReprC>(definer: &mut dyn Definer, name: &str) -> io::Result<()> {
    T::CLayout::define_self(&C, definer)?;
    if definer.insert(name) {
        writeln!(
            definer.out(),
            "typedef {};\n",
            T::CLayout::name_wrapping_var(&C, name)
        )?;
    }
    Ok(())
}
pub fn constant<T: ReprC + Debug>(
    definer: &mut dyn Definer,
    name: &str,
    value: T,
) -> io::Result<()> {
    T::CLayout::define_self(&C, definer)?;
    writeln!(
        definer.out(),
        "#define {name} (({}) {value:?})",
        T::CLayout::name(&C)
    )
}
pub fn string(definer: &mut dyn Definer, name: &str, value: &str) -> io::Result<()> {
    writeln!(definer.out(), "#define {name} {value:?}")
}
pub fn write(
    path: &Path,
    guard: &str,
    include: Option<&str>,
    known: HashSet<std::string::String>,
    define: impl FnOnce(&mut dyn Definer) -> io::Result<()>,
) -> io::Result<()> {
    let mut output = std::vec::Vec::new();
    writeln!(
        output,
        "/* Generated from Rust by safer-ffi. Do not edit. */\n#ifndef {guard}\n#define {guard}\n"
    )?;
    writeln!(
        output,
        "/* Host interface lookups return borrowed pointers. Do not free them or\n * consume/copy a generated virtual object as an owner; never call its\n * vtable.release_vptr. Calls and borrowed results must end before provider\n * unloading. Frame callbacks may further limit the borrow. Implementations\n * and callbacks must not unwind across the C ABI. */\n"
    )?;
    if let Some(include) = include {
        writeln!(output, "#include \"{include}\"\n")?;
    }
    writeln!(output, "#ifdef __cplusplus\nextern \"C\" {{\n#endif\n")?;
    define(&mut HashSetDefiner {
        defines_set: known,
        out: &mut output,
    })?;
    writeln!(
        output,
        "\n#ifdef __cplusplus\n}}\n#endif\n#endif /* {guard} */"
    )?;
    if fs::read(path).ok().as_deref() == Some(output.as_slice()) {
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, output)
}
pub fn definitions() -> io::Result<HashSet<std::string::String>> {
    let mut sink = io::sink();
    let mut definer = HashSetDefiner {
        defines_set: HashSet::new(),
        out: &mut sink,
    };
    define(&mut definer)?;
    Ok(definer.defines_set)
}
pub fn generate(path: &Path) -> io::Result<()> {
    write(path, "MHF_MOD_H", None, HashSet::new(), define)
}
pub fn define(definer: &mut dyn Definer) -> io::Result<()> {
    alias::<Status>(definer, "MhfStatus")?;
    alias::<Str>(definer, "MhfStr")?;
    // Shared safer-ffi string/slice layouts belong in the common header so
    // independently included capability headers do not redefine the same type.
    alias::<safer_ffi::prelude::str::Ref<'static>>(definer, "MhfUtf8")?;
    alias::<safer_ffi::slice::Mut<'static, u8>>(definer, "MhfMutBytes")?;
    alias::<GameInfoV2>(definer, "MhfGameInfoV2")?;
    alias::<game::LaunchParams32>(definer, "MhfLaunchParams32")?;
    alias::<game::GlobalData32>(definer, "MhfGlobalData32")?;
    alias::<LaunchTargetV1>(definer, "MhfLaunchTargetV1")?;
    alias::<LaunchApiV1>(definer, "MhfLaunchApiV1")?;
    alias::<HookGroup>(definer, "MhfHookGroup")?;
    alias::<DrainFn>(definer, "MhfDrainFn")?;
    alias::<LifecycleFn>(definer, "MhfLifecycleFn")?;
    alias::<ReadTextFn>(definer, "MhfReadTextFn")?;
    alias::<HookApiV1>(definer, "MhfHookApiV1")?;
    alias::<HostV2>(definer, "MhfHostV2")?;
    alias::<ModV2>(definer, "MhfModV2")?;
    alias::<ModQueryV2>(definer, "MhfModQueryV2")?;
    for (name, value) in [
        ("MHF_OK", OK),
        ("MHF_ERROR", ERROR),
        ("MHF_BUFFER_TOO_SMALL", BUFFER_TOO_SMALL),
        ("MHF_NOT_FOUND", NOT_FOUND),
        ("MHF_INVALID_STATE", INVALID_STATE),
        ("MHF_CONFLICT", CONFLICT),
        ("MHF_CANCELLED", CANCELLED),
    ] {
        constant(definer, name, value)?;
    }
    for (name, value) in [
        ("MHF_LOG_ERROR", LOG_ERROR),
        ("MHF_LOG_WARN", LOG_WARN),
        ("MHF_LOG_INFO", LOG_INFO),
        ("MHF_LOG_DEBUG", LOG_DEBUG),
        ("MHF_LOG_TRACE", LOG_TRACE),
        ("MHF_PHASE_PREPARE", PHASE_PREPARE),
        ("MHF_PHASE_CHECK", PHASE_CHECK),
        ("MHF_PHASE_ATTACH", PHASE_ATTACH),
        ("MHF_PHASE_RUNNING", PHASE_RUNNING),
        ("MHF_PHASE_STOP", PHASE_STOP),
        ("MHF_PHASE_DETACH", PHASE_DETACH),
        ("MHF_PHASE_DESTROY", PHASE_DESTROY),
    ] {
        constant(definer, name, value)?;
    }
    string(definer, "MHF_LAUNCH_INTERFACE_ID", LAUNCH_INTERFACE_ID)?;
    string(
        definer,
        "MHF_FALLBACK_LAUNCH_INTERFACE_ID",
        FALLBACK_LAUNCH_INTERFACE_ID,
    )?;
    writeln!(
        definer.out(),
        "\n#if defined(_WIN32)\n#define MHF_CALL __cdecl\n#define MHF_EXPORT __declspec(dllexport)\n#else\n#define MHF_CALL\n#define MHF_EXPORT __attribute__((visibility(\"default\")))\n#endif\n#define MHF_STR_LITERAL(value) {{ (const uint8_t *)(value), sizeof(value) - 1u }}"
    )?;
    Ok(())
}
