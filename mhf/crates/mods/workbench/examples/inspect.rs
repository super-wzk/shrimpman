//! Read-only format coverage report using the workbench's actual inspector.
use mhf_workbench::{
    catalog::Catalog,
    inspect::{self, Document, Kind},
};
use std::{
    collections::HashSet,
    env, fs,
    path::PathBuf,
    sync::{Arc, atomic::AtomicBool},
};

const USAGE: &str = "usage: inspect [--tree] FILE_OR_DIRECTORY ...\n\
    --tree  Show the inspected resource graph.";

fn tree(document: &Document, index: usize, depth: usize, shown: &mut HashSet<usize>) {
    let mut pending = vec![(index, depth)];
    while let Some((index, depth)) = pending.pop() {
        let node = &document.nodes[index];
        if !shown.insert(index) {
            println!("{}{} · 共享节点 {index}", "  ".repeat(depth), node.name);
            continue;
        }
        println!(
            "{}{} · {:?} · {} bytes{}",
            "  ".repeat(depth),
            node.name,
            node.kind,
            node.range.len(),
            node.error
                .as_ref()
                .map_or(String::new(), |e| format!(" · {e}"))
        );
        if matches!(
            node.kind,
            Kind::Ecd
                | Kind::Exf
                | Kind::Jkr
                | Kind::Archive
                | Kind::Momo
                | Kind::Mha
                | Kind::Stage
                | Kind::StageObjectPackage
                | Kind::StageResourceReference
                | Kind::EffectArchive
                | Kind::GroupedMaterials
                | Kind::MotionArchive
                | Kind::Unknown
        ) {
            pending.extend(node.children.iter().rev().map(|&child| (child, depth + 1)));
        }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut show_tree = false;
    let mut paths = Vec::new();
    for argument in env::args_os().skip(1) {
        if argument == "--tree" {
            show_tree = true;
            continue;
        }
        if argument == "--help" || argument == "-h" {
            println!("{USAGE}");
            return Ok(());
        }
        let path = PathBuf::from(argument);
        if path.is_dir() {
            let catalog = Catalog::scan(&path, &AtomicBool::new(false))?;
            for (path, error) in catalog.errors {
                eprintln!("{}: {error}", path.display());
            }
            paths.extend(catalog.entries.into_iter().map(|entry| entry.path));
        } else {
            paths.push(path);
        }
    }
    if paths.is_empty() {
        return Err(USAGE.into());
    }
    for path in paths {
        let source: Arc<[u8]> = fs::read(&path)?.into();
        let document = inspect::inspect(&path.to_string_lossy(), source);
        let count = |kind| {
            document
                .nodes
                .iter()
                .filter(|node| node.kind == kind)
                .count()
        };
        let unknown = document
            .nodes
            .iter()
            .filter(|node| node.kind == Kind::Unknown && !node.range.is_empty())
            .count();
        let unknown_blocks = document
            .nodes
            .iter()
            .filter(|node| node.name.starts_with("未知"))
            .count();
        let errors = document
            .nodes
            .iter()
            .filter(|node| node.error.is_some())
            .count();
        println!(
            "{} | model={} skeleton={} textures={} motion={} unknown={unknown} unknown_blocks={unknown_blocks} materials={} effects={} event_tables={} cameras={} lighting={} hits={} references={} errors={errors}",
            path.display(),
            count(Kind::Fmod),
            count(Kind::Fskl),
            count(Kind::Txb),
            count(Kind::Motion),
            count(Kind::GroupedMaterials),
            count(Kind::EffectBank),
            count(Kind::EffectMotionEvents),
            count(Kind::EventCamera) + count(Kind::StageAreaCamera),
            count(Kind::StageLighting) + count(Kind::LegacyStageLighting),
            count(Kind::Hits),
            count(Kind::StageResourceReference)
        );
        if show_tree {
            tree(&document, document.root, 0, &mut HashSet::new());
        }
        for node in document.nodes.iter().filter(|node| node.error.is_some()) {
            println!(
                "  ERROR {:?} {}: {}",
                node.kind,
                node.name,
                node.error.as_deref().unwrap()
            );
        }
    }
    Ok(())
}
