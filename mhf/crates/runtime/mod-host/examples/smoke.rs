//! Run with the packaged counter provider, Rust/C consumers, and example.hook.
//! Exercises the real Windows DLL loader, dependency graph, and host Hook API.

#[cfg(all(windows, target_arch = "x86"))]
mod smoke {
    use mhf_mod_host::{Context, ModHost, Module, Result, api};
    use mhf_mod_package::{Candidate, Manifest, discover, resolve};
    use std::{
        cell::Cell,
        collections::{BTreeMap, BTreeSet},
        ffi::c_void,
        path::Path,
        rc::Rc,
    };

    use example_counter_sdk::{CounterApi, CounterTable};

    // The diagnostic Hook table is generated from the example's ReprC type.
    #[repr(C)]
    #[derive(Default)]
    struct HookSnapshotV1 {
        entered: u32,
        completed: u32,
        active: u32,
        drains: u32,
    }

    #[repr(C)]
    struct HookProbeV1 {
        context: *mut c_void,
        invoke: unsafe extern "C" fn(u32) -> u32,
        snapshot: unsafe extern "C" fn(*mut c_void, *mut HookSnapshotV1) -> api::Status,
    }

    #[derive(Default)]
    struct Tables {
        counter: Cell<*const CounterTable>,
        hook: Cell<*const HookProbeV1>,
    }

    struct Observer(Rc<Tables>);

    impl Module for Observer {
        fn attach(&mut self, context: &Context) -> Result<()> {
            self.0.counter.set(
                context
                    .interface("example.counter", "example.counter.v1")?
                    .cast(),
            );
            self.0
                .hook
                .set(context.interface("example.hook", "example.hook.v1")?.cast());

            Ok(())
        }
    }

    fn expected(label: &str, actual: u32, expected: u32) -> Result<()> {
        if actual == expected {
            Ok(())
        } else {
            Err(format!("{label}: expected {expected}, got {actual}"))
        }
    }

    fn status(label: &str, status: api::Status) -> Result<()> {
        if status == api::OK {
            Ok(())
        } else {
            Err(format!("{label}: status {status}"))
        }
    }

    fn verify(tables: &Tables, detached: bool) -> Result<()> {
        // Observer keeps these provider interfaces borrowed through host cleanup.
        let counter = unsafe { &*tables.counter.get() };
        let hook = unsafe { &*tables.hook.get() };
        let counter_snapshot = counter.snapshot();
        expected("Rust + C counter", counter_snapshot.count, 11)?;
        expected(
            "hook target",
            unsafe { (hook.invoke)(10) },
            if detached { 11 } else { 111 },
        )?;
        let mut hooks = HookSnapshotV1::default();
        status("hook snapshot", unsafe {
            (hook.snapshot)(hook.context, &mut hooks)
        })?;
        expected("detour entries", hooks.entered, 2)?;
        expected("detour completions", hooks.completed, hooks.entered)?;
        expected("active detours", hooks.active, 0)?;
        expected("drain calls", hooks.drains, u32::from(detached))
    }

    pub(super) fn run(directory: &Path) -> Result<()> {
        let mut candidates = discover(directory).map_err(|error| error.to_string())?;
        let observer = Manifest::parse(
            r#"
schema = 1
id = "example.smoke-observer"
name = "DLL 和 Hook 验证"
version = "1.0.0"
kind = "native"

[dependencies]
"example.counter" = "^1.0"
"example.counter-consumer" = "^1.0"
"example.counter-c" = "^1.0"
"example.hook" = "^1.0"
"#,
        )
        .map_err(|error| error.to_string())?;
        candidates.push(Candidate::builtin(observer).map_err(|error| error.to_string())?);
        let resolved = resolve(
            &candidates,
            &BTreeMap::new(),
            &BTreeSet::from(["example.smoke-observer".into()]),
            &BTreeSet::new(),
        )
        .map_err(|error| error.to_string())?;
        let tables = Rc::new(Tables::default());
        let mut host = ModHost::load(resolved, &BTreeMap::new(), |_| {
            Ok(Box::new(Observer(Rc::clone(&tables))))
        })?;
        let result = (|| {
            host.prepare()?;
            host.check()?;
            host.attach()?;
            host.running();
            verify(&tables, false)
        })();
        let cleanup = host
            .stop()
            .and_then(|()| unsafe { host.detach(&[]) })
            .and_then(|()| unsafe { host.prepare_release() });
        if let Err(error) = cleanup {
            host.retain();
            return Err(format!("DLL/Hook cleanup failed: {error}"));
        }
        result?;
        verify(&tables, true)?;
        drop(host);
        Ok(())
    }
}

#[cfg(all(windows, target_arch = "x86"))]
fn main() -> Result<(), String> {
    let directory = std::env::args_os()
        .nth(1)
        .ok_or("usage: smoke <packaged-mods-directory>")?;
    for _ in 0..2 {
        smoke::run(std::path::Path::new(&directory))?;
    }
    println!(
        "DLL + C/Rust dependencies + Hook lifecycle passed twice: counter=11, target=111→11, drain=1"
    );
    Ok(())
}

#[cfg(not(all(windows, target_arch = "x86")))]
fn main() {
    eprintln!("This smoke example requires i686-pc-windows-msvc.");
    std::process::exit(2);
}
