//! Provider-owned model catalog snapshots.
//!
//! Freshness and entitlement are distinct. A valid snapshot records what a
//! specific profile saw, but never proves its current authorization to infer.
//! The runtime must revalidate account, model and protocol before dispatch.

use crate::CatalogPage;
use std::collections::BTreeMap;

pub const DEFAULT_DYNAMIC_TTL_SECONDS: u64 = 15 * 60;
const MAX_CACHED_IDENTITIES: usize = 512;
const MAX_DYNAMIC_TTL_SECONDS: u64 = 24 * 60 * 60;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct CacheKey {
    pub scope: String,
    pub provider_id: String,
    pub connection_id: String,
    /// None is permitted only for a connection with no authentication.
    pub profile_id: Option<String>,
}

impl CacheKey {
    fn valid(&self) -> bool {
        fn id(s: &str) -> bool {
            !s.is_empty() && s.len() <= 128 && !s.chars().any(char::is_control)
        }
        id(&self.scope)
            && id(&self.provider_id)
            && id(&self.connection_id)
            && self.profile_id.as_deref().is_none_or(id)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CachePolicy {
    /// No network expiration for a declared local, no-auth catalog.
    LocalDeclared,
    /// TTL=0 selects the accepted 15-minute default.
    Dynamic { ttl_seconds: u64 },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CacheError {
    InvalidIdentity,
    InvalidPolicy,
    TooManyIdentities,
    RevisionOverflow,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Freshness {
    Fresh,
    Stale,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogView {
    pub page: CatalogPage,
    pub checked_at: u64,
    pub revision: u64,
    /// Stale snapshots remain inspectable but must not be used for
    /// fresh selection or inference. Fresh snapshots are not auth grants.
    pub freshness: Freshness,
}

struct Snapshot {
    page: CatalogPage,
    checked_at: u64,
    expires_at: Option<u64>,
    credential_revision: u64,
    revision: u64,
}

/// A provider plugin may persist these snapshots in its authorized storage.
/// This helper itself performs no network requests, writes or token handling.
#[derive(Default)]
pub struct CatalogCache {
    entries: BTreeMap<CacheKey, Snapshot>,
    next_revision: u64,
}

impl CatalogCache {
    pub fn put(
        &mut self,
        key: CacheKey,
        checked_at: u64,
        credential_revision: u64,
        policy: CachePolicy,
        page: CatalogPage,
    ) -> Result<u64, CacheError> {
        if !key.valid() {
            return Err(CacheError::InvalidIdentity);
        }
        if self.entries.len() >= MAX_CACHED_IDENTITIES && !self.entries.contains_key(&key) {
            return Err(CacheError::TooManyIdentities);
        }
        let expires_at = match policy {
            CachePolicy::LocalDeclared => {
                if key.profile_id.is_some() {
                    return Err(CacheError::InvalidPolicy);
                }
                None
            }
            CachePolicy::Dynamic { ttl_seconds } => {
                let ttl = if ttl_seconds == 0 {
                    DEFAULT_DYNAMIC_TTL_SECONDS
                } else {
                    ttl_seconds
                };
                if ttl > MAX_DYNAMIC_TTL_SECONDS {
                    return Err(CacheError::InvalidPolicy);
                }
                Some(
                    checked_at
                        .checked_add(ttl)
                        .ok_or(CacheError::InvalidPolicy)?,
                )
            }
        };
        let revision = self
            .next_revision
            .checked_add(1)
            .ok_or(CacheError::RevisionOverflow)?;
        self.next_revision = revision;
        self.entries.insert(
            key,
            Snapshot {
                page,
                checked_at,
                expires_at,
                credential_revision,
                revision,
            },
        );
        Ok(revision)
    }

    /// The caller injects a monotonic/suitable timestamp and the current
    /// runtime-owned credential generation, never a value from a model.
    pub fn view(&self, key: &CacheKey, now: u64, credential_revision: u64) -> Option<CatalogView> {
        let entry = self.entries.get(key)?;
        let fresh = credential_revision == entry.credential_revision
            && now >= entry.checked_at
            && entry.expires_at.is_none_or(|expires_at| now < expires_at);
        Some(CatalogView {
            page: entry.page.clone(),
            checked_at: entry.checked_at,
            revision: entry.revision,
            freshness: if fresh {
                Freshness::Fresh
            } else {
                Freshness::Stale
            },
        })
    }

    /// A refresh failure leaves the last-known snapshot unchanged, for
    /// inspection only once its time or credential revision is stale.
    pub fn purge(&mut self, key: &CacheKey) -> bool {
        self.entries.remove(key).is_some()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CatalogModel, CatalogPage};

    fn key(profile: Option<&str>) -> CacheKey {
        CacheKey {
            scope: "user".into(),
            provider_id: "openai".into(),
            connection_id: "api".into(),
            profile_id: profile.map(str::to_string),
        }
    }

    fn catalog(model: &str) -> CatalogPage {
        CatalogPage {
            models: vec![CatalogModel {
                id: model.into(),
                label: model.into(),
            }],
            next_provider_cursor: None,
        }
    }

    #[test]
    fn dynamic_default_expires_at_fifteen_minutes_exactly() {
        let mut cache = CatalogCache::default();
        cache
            .put(
                key(Some("work")),
                100,
                3,
                CachePolicy::Dynamic { ttl_seconds: 0 },
                catalog("a"),
            )
            .unwrap();
        assert_eq!(
            cache.view(&key(Some("work")), 999, 3).unwrap().freshness,
            Freshness::Fresh
        );
        assert_eq!(
            cache.view(&key(Some("work")), 1000, 3).unwrap().freshness,
            Freshness::Stale
        );
    }

    #[test]
    fn account_generation_change_never_reuses_another_accounts_entitlement() {
        let mut cache = CatalogCache::default();
        cache
            .put(
                key(Some("work")),
                100,
                3,
                CachePolicy::Dynamic { ttl_seconds: 30 },
                catalog("work"),
            )
            .unwrap();
        cache
            .put(
                key(Some("personal")),
                100,
                8,
                CachePolicy::Dynamic { ttl_seconds: 30 },
                catalog("personal"),
            )
            .unwrap();
        assert_eq!(
            cache.view(&key(Some("work")), 101, 4).unwrap().freshness,
            Freshness::Stale
        );
        assert_eq!(
            cache
                .view(&key(Some("personal")), 101, 8)
                .unwrap()
                .page
                .models[0]
                .id,
            "personal"
        );
        let mut foreign = key(Some("work"));
        foreign.scope = "other-project".into();
        assert!(cache.view(&foreign, 101, 3).is_none());
        assert_eq!(cache.len(), 2);
    }

    #[test]
    fn refresh_error_preserves_inspection_and_revision_changes_after_success() {
        let mut cache = CatalogCache::default();
        let first = cache
            .put(
                key(Some("work")),
                10,
                1,
                CachePolicy::Dynamic { ttl_seconds: 1 },
                catalog("old"),
            )
            .unwrap();
        let snapshot = cache.view(&key(Some("work")), 12, 1).unwrap();
        assert_eq!(snapshot.freshness, Freshness::Stale);
        assert_eq!(snapshot.page.models[0].id, "old");
        let second = cache
            .put(
                key(Some("work")),
                12,
                1,
                CachePolicy::Dynamic { ttl_seconds: 1 },
                catalog("new"),
            )
            .unwrap();
        assert!(second > first);
        assert_eq!(
            cache.view(&key(Some("work")), 12, 1).unwrap().page.models[0].id,
            "new"
        );
    }

    #[test]
    fn offline_declared_no_auth_catalog_has_no_network_ttl() {
        let mut cache = CatalogCache::default();
        cache
            .put(
                key(None),
                1,
                0,
                CachePolicy::LocalDeclared,
                catalog("local"),
            )
            .unwrap();
        assert_eq!(
            cache.view(&key(None), u64::MAX, 0).unwrap().freshness,
            Freshness::Fresh
        );
        assert_eq!(
            cache
                .put(
                    key(Some("api")),
                    1,
                    0,
                    CachePolicy::LocalDeclared,
                    catalog("no")
                )
                .unwrap_err(),
            CacheError::InvalidPolicy
        );
        assert_eq!(cache.len(), 1);
    }

    #[test]
    fn invalid_identity_and_provider_ttl_are_rejected_without_mutating_cache() {
        let mut cache = CatalogCache::default();
        let mut invalid = key(Some("work"));
        invalid.connection_id.clear();
        assert_eq!(
            cache.put(
                invalid,
                0,
                0,
                CachePolicy::Dynamic { ttl_seconds: 0 },
                catalog("bad")
            ),
            Err(CacheError::InvalidIdentity)
        );
        assert_eq!(
            cache.put(
                key(Some("work")),
                0,
                0,
                CachePolicy::Dynamic {
                    ttl_seconds: 24 * 60 * 60 + 1,
                },
                catalog("bad")
            ),
            Err(CacheError::InvalidPolicy)
        );
        assert!(cache.is_empty());
    }
}
