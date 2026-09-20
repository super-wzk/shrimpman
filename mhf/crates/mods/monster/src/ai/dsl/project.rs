//! A bounded, species-scoped collection of editable source files.
use std::{
    collections::{HashMap, HashSet},
    fs,
    io::Read,
    path::{Component, Path, PathBuf},
};

use super::{
    compile::{Compiled, is_reserved_command},
    parser::{self, Callee, Document, Statement, StatementKind},
};
use crate::ai::{Error, Result, decompile::MAX_SOURCE_BYTES};

pub const MAX_FILES: usize = 128;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceFile {
    /// Normalized path relative to monster-ai, never an absolute host path.
    pub path: String,
    pub source: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Project {
    pub entry: String,
    pub files: Vec<SourceFile>,
}

impl Project {
    pub fn single(map: Option<u32>, species: u8, source: String) -> Self {
        let entry = match map {
            Some(map) => format!("maps/{map}/{species}/main.mhai"),
            None => format!("common/{species}/main.mhai"),
        };
        Self {
            files: vec![SourceFile {
                path: entry.clone(),
                source,
            }],
            entry,
        }
    }

    /// Only a missing map entry permits fallback. A broken specific entry is an error.
    pub fn load(root: &Path, map: u32, species: u8) -> Result<Option<Self>> {
        for entry in [
            format!("maps/{map}/{species}/main.mhai"),
            format!("common/{species}/main.mhai"),
        ] {
            match fs::symlink_metadata(root.join(&entry)) {
                Ok(_) => {
                    let directory = Path::new(&entry).parent().unwrap();
                    let common = PathBuf::from(format!("common/{species}"));
                    let source = read_file(root, &entry, directory, &common)?;
                    let mut project = Self {
                        entry: entry.clone(),
                        files: vec![SourceFile {
                            path: entry,
                            source,
                        }],
                    };
                    project.complete(root)?;
                    project.check_target(map, species)?;
                    return Ok(Some(project));
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(error) => return Err(Error::new(format!("{entry}: {error}"))),
            }
        }
        Ok(None)
    }

    pub fn check_target(&self, map: u32, species: u8) -> Result<()> {
        let entry = self.document()?;
        if entry.species != species || entry.map.is_some_and(|value| value != map) {
            return Err(Error::new(
                "AI project species/map does not match the selected instance",
            ));
        }
        Ok(())
    }

    fn validate_size(&self) -> Result<()> {
        if self.files.len() > MAX_FILES
            || self.files.iter().map(|f| f.source.len()).sum::<usize>() > MAX_SOURCE_BYTES
        {
            return Err(Error::new("AI project exceeds 128 files or 4 MiB"));
        }
        Ok(())
    }

    fn document(&self) -> Result<Document> {
        self.validate_size()?;
        let file = self
            .files
            .iter()
            .find(|file| file.path == self.entry)
            .ok_or_else(|| Error::new("AI project entry is missing"))?;
        parser::parse(&file.source).map_err(|error| Error::new(format!("{}: {error}", file.path)))
    }

    fn bounds(&self) -> Result<(PathBuf, PathBuf)> {
        let entry = self.document()?;
        let directory = Path::new(&self.entry)
            .parent()
            .ok_or_else(|| Error::new("invalid entry path"))?
            .to_path_buf();
        let common = PathBuf::from(format!("common/{}", entry.species));
        let parts: Vec<_> = self.entry.split('/').collect();
        let valid_map = parts.len() == 4
            && parts[0] == "maps"
            && parts[1]
                .parse::<u32>()
                .ok()
                .is_some_and(|map| entry.map == Some(map))
            && parts[2] == entry.species.to_string()
            && parts[3] == "main.mhai";
        if directory != common && !valid_map {
            return Err(Error::new(
                "entry must be common/<species>/main.mhai or maps/<map>/<species>/main.mhai",
            ));
        }
        if directory == common
            && (self.entry != format!("common/{}/main.mhai", entry.species) || entry.map.is_some())
        {
            return Err(Error::new("common entry must omit the map declaration"));
        }
        Ok((directory, common))
    }

    /// Load newly imported files while keeping all supplied editor drafts.
    pub fn complete(&mut self, root: &Path) -> Result<()> {
        let (directory, common) = self.bounds()?;
        let mut cursor = 0;
        while cursor < self.files.len() {
            self.validate_files(&directory, &common)?;
            let file = &self.files[cursor];
            let document = parser::parse_module(&file.source)
                .map_err(|e| Error::new(format!("{}: {e}", file.path)))?;
            let paths = document
                .imports
                .iter()
                .map(|import| resolve(&file.path, &import.path, &directory, &common))
                .collect::<Result<Vec<_>>>()?;
            for path in paths {
                if self.files.iter().any(|file| file.path == path) {
                    continue;
                }
                let source = read_file(root, &path, &directory, &common)?;
                // Keep the lexical identity; filesystem aliases are rejected by
                // read_file rather than letting one file acquire two namespaces.
                self.files.push(SourceFile { path, source });
            }
            cursor += 1;
        }
        self.validate_files(&directory, &common)
    }

    fn validate_files(&self, directory: &Path, common: &Path) -> Result<()> {
        self.validate_size()?;
        let mut paths = HashSet::new();
        for file in &self.files {
            let normalized = normalize(Path::new(&file.path))?;
            if normalized != file.path
                || !allowed(Path::new(&normalized), directory, common)
                || !paths.insert(&file.path)
            {
                return Err(Error::new(format!(
                    "duplicate or out-of-scope module: {}",
                    file.path
                )));
            }
        }
        Ok(())
    }

    pub fn compile(&self) -> Result<Compiled> {
        let (directory, common) = self.bounds()?;
        self.validate_files(&directory, &common)?;
        let mut documents = HashMap::new();
        for file in &self.files {
            let document = parser::parse_module(&file.source)
                .map_err(|e| Error::new(format!("{}: {e}", file.path)))?;
            if file.path != self.entry && !document.module {
                return Err(Error::new(format!(
                    "{}: import a module, not another entry",
                    file.path
                )));
            }
            documents.insert(file.path.clone(), document);
        }
        let mut visited = HashSet::new();
        let mut stack = Vec::new();
        visit(
            &self.entry,
            &documents,
            &directory,
            &common,
            &mut visited,
            &mut stack,
        )?;
        let mut entry = self.document()?;
        entry.actions.clear();
        entry.functions.clear();
        entry.imports.clear();
        entry.native_functions.clear();
        // Source order makes output deterministic; each canonical module is merged once.
        for file in &self.files {
            if !visited.contains(&file.path) {
                continue;
            }
            let document = &documents[&file.path];
            for (name, slot) in &document.native_functions {
                if entry.native_functions.values().any(|value| value == slot) {
                    return Err(Error::new("duplicate native slot binding across modules"));
                }
                entry
                    .native_functions
                    .insert(qualify(&file.path, name), *slot);
            }
            let mut imports = HashMap::new();
            for import in &document.imports {
                imports.insert(
                    import.alias.as_str(),
                    resolve(&file.path, &import.path, &directory, &common)?,
                );
            }
            for action in &document.actions {
                let mut action = action.clone();
                action.name = qualify(&file.path, &action.name);
                entry.actions.push(action);
            }
            for function in &document.functions {
                let mut function = function.clone();
                if function.name != "main" {
                    function.name = qualify(&file.path, &function.name);
                }
                rewrite(&mut function.body, &file.path, &imports)?;
                entry.functions.push(function);
            }
            if file.path == self.entry {
                for body in entry
                    .states
                    .iter_mut()
                    .filter_map(|d| d.body.as_mut())
                    .chain(entry.events.iter_mut().filter_map(|d| d.body.as_mut()))
                {
                    rewrite(body, &file.path, &imports)?;
                }
            }
        }
        entry.compile()
    }
}

fn visit(
    path: &str,
    documents: &HashMap<String, Document>,
    directory: &Path,
    common: &Path,
    visited: &mut HashSet<String>,
    stack: &mut Vec<String>,
) -> Result<()> {
    if stack.iter().any(|p| p == path) {
        return Err(Error::new(format!(
            "cyclic import: {} -> {path}",
            stack.join(" -> ")
        )));
    }
    if visited.contains(path) {
        return Ok(());
    }
    let document = documents
        .get(path)
        .ok_or_else(|| Error::new(format!("missing imported module: {path}")))?;
    stack.push(path.into());
    for import in &document.imports {
        let target = resolve(path, &import.path, directory, common)?;
        visit(&target, documents, directory, common, visited, stack)?;
    }
    stack.pop();
    visited.insert(path.into());
    Ok(())
}

fn qualify(path: &str, name: &str) -> String {
    format!("{path}::{name}")
}

fn rewrite(body: &mut [Statement], path: &str, imports: &HashMap<&str, String>) -> Result<()> {
    for statement in body {
        match &mut statement.kind {
            StatementKind::Call {
                callee: Callee::Name(name),
                ..
            } => {
                if name == "main" || name.ends_with(".main") {
                    return Err(Error::at(
                        statement.line,
                        statement.column,
                        "main is an entry point, not a callable helper",
                    ));
                }
                if let Some((alias, member)) = name.split_once('.') {
                    let target = imports.get(alias).ok_or_else(|| {
                        Error::at(
                            statement.line,
                            statement.column,
                            format!("{path}: unknown import '{alias}'"),
                        )
                    })?;
                    if member.contains('.') {
                        return Err(Error::new("module re-exports are not supported"));
                    }
                    *name = qualify(target, member);
                } else if !is_reserved_command(name) {
                    *name = qualify(path, name);
                }
            }
            StatementKind::EntryBody(body) => {
                rewrite(body, path, imports)?;
            }
            StatementKind::Random(branches) => {
                for (_, body) in branches {
                    rewrite(body, path, imports)?;
                }
            }
            StatementKind::TargetDistanceGroups(branches) => {
                for body in branches {
                    rewrite(body, path, imports)?;
                }
            }
            StatementKind::If {
                then_body,
                else_body,
                ..
            } => {
                rewrite(then_body, path, imports)?;
                if let Some(body) = else_body {
                    rewrite(body, path, imports)?;
                }
            }
            _ => {}
        }
    }
    Ok(())
}

fn allowed(path: &Path, directory: &Path, common: &Path) -> bool {
    path.starts_with(directory) || path.starts_with(common)
}

fn normalize(path: &Path) -> Result<String> {
    let mut result = PathBuf::new();
    for part in path.components() {
        match part {
            Component::Normal(part) => result.push(part),
            Component::CurDir => {}
            Component::ParentDir if result.pop() => {}
            _ => return Err(Error::new("import path escapes its root")),
        }
    }
    Ok(result.to_string_lossy().replace('\\', "/"))
}

fn resolve(from: &str, import: &str, directory: &Path, common: &Path) -> Result<String> {
    if import.is_empty() || import.contains(['\\', ':']) || import.starts_with(['/', '@', '~']) {
        return Err(Error::new(format!("invalid import path: {import}")));
    }
    let path = if let Some(relative) = import.strip_prefix("#common/") {
        common.join(relative)
    } else {
        if import.starts_with('#') {
            return Err(Error::new("only #common/ is a supported alias"));
        }
        Path::new(from).parent().unwrap().join(import)
    };
    let path = normalize(&path)?;
    if !allowed(Path::new(&path), directory, common) {
        return Err(Error::new(format!(
            "import leaves the current species/project: {import}"
        )));
    }
    Ok(path)
}

fn read_file(root: &Path, path: &str, directory: &Path, common: &Path) -> Result<String> {
    let root = root.canonicalize().map_err(|e| Error::new(e.to_string()))?;
    let actual = root
        .join(path)
        .canonicalize()
        .map_err(|e| Error::new(format!("{path}: {e}")))?;
    let relative = actual
        .strip_prefix(&root)
        .map_err(|_| Error::new("import symlink leaves monster-ai"))?;
    if !allowed(relative, directory, common) {
        return Err(Error::new(
            "import symlink leaves the current species/project",
        ));
    }
    let normalized = normalize(relative)?;
    // Symlink aliases would require rewriting every supplied editor key. Keep
    // project identity unambiguous by requiring the canonical relative spelling.
    if normalized != path {
        return Err(Error::new(format!(
            "use canonical module path: {normalized}"
        )));
    }
    let file = fs::File::open(&actual).map_err(|e| Error::new(format!("{path}: {e}")))?;
    let mut source = String::new();
    file.take((MAX_SOURCE_BYTES + 1) as u64)
        .read_to_string(&mut source)
        .map_err(|e| Error::new(format!("{path}: {e}")))?;
    if source.len() > MAX_SOURCE_BYTES {
        return Err(Error::new("module exceeds 4 MiB"));
    }
    Ok(source)
}
