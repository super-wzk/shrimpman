//! Canonical native coordinates, independent of inspector grouping and labels.

use super::Document;
use mhf_resource::PathSegment::Field as Key;
use mhf_resource::{PathSegment, ResourcePath};
use std::path::Path;

#[cfg(test)]
pub(crate) mod tests;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Address {
    pub path: ResourcePath,
    /// False when this is only the closest addressable ancestor.
    pub exact: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Location {
    Resolved {
        node: usize,
        context: Vec<usize>,
        field: Option<usize>,
    },
    Expand(usize),
    Missing,
}

impl Document {
    /// Use the real source file relative to DAT; display names never identify a file.
    pub fn resource_address(
        &self,
        dat_root: &Path,
        context: &[usize],
        field: Option<usize>,
    ) -> Option<Address> {
        let relative = self.source.strip_prefix(dat_root).ok()?.to_str()?;
        let (mut segments, mut exact) = self.resource_segments(context)?;
        if let Some(field) = field {
            let node = self.nodes.get(*context.last()?)?;
            let field = node.fields.get(field)?;
            exact &= !node.kind.is_transparent();
            if exact && let Some(key) = &field.key {
                segments.push(Key(key.clone()));
            } else {
                exact = false;
            }
        }
        Some(Address {
            path: ResourcePath::from_parts(relative, segments).ok()?,
            exact,
        })
    }

    pub fn locate_resource(&self, dat_root: &Path, path: &ResourcePath) -> Location {
        if self
            .source
            .strip_prefix(dat_root)
            .ok()
            .and_then(Path::to_str)
            .and_then(|relative| ResourcePath::new(relative).ok())
            .is_none_or(|source| source.source() != path.source())
        {
            return Location::Missing;
        }
        self.locate_segments(path.segments())
    }

    pub(crate) fn resource_segments(&self, context: &[usize]) -> Option<(Vec<PathSegment>, bool)> {
        if context.first() != Some(&self.root) {
            return None;
        }
        let mut coordinates = vec![(Vec::new(), true)];
        for (at, edge) in context.windows(2).enumerate() {
            if !self.nodes.get(edge[0])?.children.contains(&edge[1]) {
                return None;
            }
            coordinates.push(self.child_coordinate(&context[..=at], &coordinates, edge[1])?);
        }
        coordinates.pop()
    }

    fn child_coordinate(
        &self,
        context: &[usize],
        coordinates: &[(Vec<PathSegment>, bool)],
        child: usize,
    ) -> Option<(Vec<PathSegment>, bool)> {
        let parent = self.nodes.get(*context.last()?)?;
        let child = self.nodes.get(child)?;
        if parent.kind.is_transparent() {
            // Reference payloads keep the caller's address rather than their
            // physical owner's directory member coordinate.
            return coordinates.last().cloned();
        }
        if let Some(address) = &child.address {
            let anchor = context.iter().rposition(|node| *node == address.anchor)?;
            let (mut segments, exact) = coordinates.get(anchor)?.clone();
            segments.extend_from_slice(&address.segments);
            Some((segments, exact))
        } else {
            Some((coordinates.last()?.0.clone(), false))
        }
    }

    pub(crate) fn locate_segments(&self, target: &[PathSegment]) -> Location {
        let mut pending = vec![(vec![self.root], vec![(Vec::new(), true)])];
        let mut found = None;
        let mut expandable = None;
        let mut associated = None;
        while let Some((context, coordinates)) = pending.pop() {
            let node = *context.last().unwrap();
            let Some(value) = self.nodes.get(node) else {
                continue;
            };
            let (segments, exact) = coordinates.last().unwrap();
            if *exact {
                if segments.as_slice() == target {
                    // Prefer a visible payload to its encoding layer.
                    if found.is_none() || !value.kind.is_transparent() {
                        found = Some(Location::Resolved {
                            node,
                            context: context.clone(),
                            field: None,
                        });
                    }
                } else if target.starts_with(segments) {
                    if value.kind == super::Kind::Emd {
                        associated =
                            super::emd::resource_expansion(self, node, &target[segments.len()..]);
                    }
                    if !value.kind.is_transparent()
                        && let [Key(key)] = &target[segments.len()..]
                        && let Some(field) = value
                            .fields
                            .iter()
                            .position(|field| field.key.as_ref() == Some(key))
                    {
                        return Location::Resolved {
                            node,
                            context,
                            field: Some(field),
                        };
                    }
                    if value.deferred
                        && expandable
                            .as_ref()
                            .is_none_or(|(depth, _)| *depth < segments.len())
                    {
                        expandable = Some((segments.len(), node));
                    }
                }
            }
            // Helpers may contain direct native coordinates outside their
            // display parent, so only explicit addresses can narrow traversal.
            for &child in value.children.iter().rev() {
                if !context.contains(&child) {
                    let Some(coordinate) = self.child_coordinate(&context, &coordinates, child)
                    else {
                        continue;
                    };
                    let mut branch = context.clone();
                    let mut branch_coordinates = coordinates.clone();
                    branch.push(child);
                    branch_coordinates.push(coordinate);
                    pending.push((branch, branch_coordinates));
                }
            }
        }
        found.unwrap_or_else(|| {
            expandable
                .map(|(_, node)| node)
                .or(associated)
                .map_or(Location::Missing, Location::Expand)
        })
    }
}
