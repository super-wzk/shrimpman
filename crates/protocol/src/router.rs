use std::{borrow::Borrow, collections::HashMap, hash::Hash};

use thiserror::Error;

/// Resolves a route target from a key and its associated metadata.
pub trait RouteResolver<Key, Metadata> {
    type Target;
    type Error;

    fn resolve(&self, key: &Key, metadata: &Metadata) -> Result<Self::Target, Self::Error>;
}

/// Determines whether a route applies to a metadata value.
pub trait RouteSelector {
    type Metadata;
    type Priority: Ord;
    type Conflict;

    fn matches(&self, metadata: &Self::Metadata) -> bool;

    fn priority(&self) -> Self::Priority;

    fn is_empty(&self) -> bool;

    fn conflict(&self, other: &Self) -> Option<Self::Conflict>;
}

impl RouteSelector for () {
    type Metadata = ();
    type Priority = ();
    type Conflict = ();

    fn matches(&self, _metadata: &Self::Metadata) -> bool {
        true
    }

    fn priority(&self) -> Self::Priority {}

    fn is_empty(&self) -> bool {
        false
    }

    fn conflict(&self, _other: &Self) -> Option<Self::Conflict> {
        Some(())
    }
}

/// An invalid route table.
#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum RouteTableBuildError<Key, Conflict> {
    #[error("a route selector has no matches")]
    EmptySelector { key: Key },

    #[error("routes with the same priority overlap")]
    Conflict { key: Key, conflict: Conflict },
}

struct Route<Selector, Target> {
    selector: Selector,
    target: Target,
}

/// A validated route table that selects the highest-priority matching target.
pub struct RouteTable<Key, Selector, Target> {
    routes: HashMap<Key, Vec<Route<Selector, Target>>>,
}

impl<Key, Selector, Target> RouteTable<Key, Selector, Target>
where
    Key: Eq + Hash,
    Selector: RouteSelector,
{
    pub fn build(
        entries: impl IntoIterator<Item = (Key, Selector, Target)>,
    ) -> Result<Self, RouteTableBuildError<Key, Selector::Conflict>> {
        let mut routes: HashMap<Key, Vec<Route<Selector, Target>>> = HashMap::new();

        for (key, selector, target) in entries {
            if selector.is_empty() {
                return Err(RouteTableBuildError::EmptySelector { key });
            }

            if let Some(existing_routes) = routes.get(&key) {
                for existing in existing_routes {
                    if existing.selector.priority() != selector.priority() {
                        continue;
                    }

                    if let Some(conflict) = existing.selector.conflict(&selector) {
                        return Err(RouteTableBuildError::Conflict { key, conflict });
                    }
                }
            }

            routes
                .entry(key)
                .or_default()
                .push(Route { selector, target });
        }

        Ok(Self { routes })
    }

    pub fn resolve<Query>(&self, key: &Query, metadata: &Selector::Metadata) -> Option<&Target>
    where
        Key: Borrow<Query>,
        Query: Eq + Hash + ?Sized,
    {
        self.routes
            .get(key)?
            .iter()
            .filter(|route| route.selector.matches(metadata))
            .max_by_key(|route| route.selector.priority())
            .map(|route| &route.target)
    }
}

#[cfg(test)]
mod tests {
    use std::ops::Range;

    use super::{RouteSelector, RouteTable, RouteTableBuildError};

    enum Selector {
        Any,
        Exact(u8),
        Range(Range<u8>),
        Empty,
    }

    impl RouteSelector for Selector {
        type Metadata = u8;
        type Priority = u8;
        type Conflict = u8;

        fn matches(&self, value: &u8) -> bool {
            match self {
                Self::Any => true,
                Self::Exact(expected) => value == expected,
                Self::Range(range) => range.contains(value),
                Self::Empty => false,
            }
        }

        fn priority(&self) -> Self::Priority {
            match self {
                Self::Any => 0,
                Self::Exact(_) => 2,
                Self::Range(_) | Self::Empty => 1,
            }
        }

        fn is_empty(&self) -> bool {
            matches!(self, Self::Empty)
        }

        fn conflict(&self, other: &Self) -> Option<Self::Conflict> {
            (0..=u8::MAX).find(|value| self.matches(value) && other.matches(value))
        }
    }

    #[test]
    fn selects_the_highest_priority_matching_route() {
        let routes = RouteTable::build([
            ("SIGN:", Selector::Any, "fallback"),
            ("SIGN:", Selector::Exact(41), "exact"),
        ])
        .unwrap();

        assert_eq!(routes.resolve("SIGN:", &41), Some(&"exact"));
        assert_eq!(routes.resolve("SIGN:", &42), Some(&"fallback"));
        assert_eq!(routes.resolve("OTHER:", &41), None);
    }

    #[test]
    fn resolves_a_route_without_metadata() {
        let routes = RouteTable::build([("COMMAND", (), "handler")]).unwrap();

        assert_eq!(routes.resolve("COMMAND", &()), Some(&"handler"));
        assert_eq!(routes.resolve("OTHER", &()), None);
    }

    #[test]
    fn rejects_duplicate_routes_without_metadata() {
        let result = RouteTable::build([("COMMAND", (), 1), ("COMMAND", (), 2)]);
        let Err(error) = result else {
            panic!("duplicate unit routes were accepted")
        };

        assert_eq!(
            error,
            RouteTableBuildError::Conflict {
                key: "COMMAND",
                conflict: (),
            }
        );
    }

    #[test]
    fn rejects_empty_selectors() {
        let result = RouteTable::build([("SIGN:", Selector::Empty, ())]);
        let Err(error) = result else {
            panic!("an empty route selector was accepted")
        };

        assert_eq!(error, RouteTableBuildError::EmptySelector { key: "SIGN:" });
    }

    #[test]
    fn rejects_same_priority_conflicts() {
        let result = RouteTable::build([
            ("SIGN:", Selector::Range(20..60), ()),
            ("SIGN:", Selector::Range(41..80), ()),
        ]);
        let Err(error) = result else {
            panic!("overlapping routes were accepted")
        };

        assert_eq!(
            error,
            RouteTableBuildError::Conflict {
                key: "SIGN:",
                conflict: 41,
            }
        );
    }
}
