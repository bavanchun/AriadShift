//! Pure tool tier access control (`ToolAccess`) and batch run quota checks.
//!
//! Every product surface (`ashift` CLI, local MCP, `ariad-server`, `ariad-ffi`, and `ariad-wasm`)
//! calls [`allowed`] and [`within_run_quota`] from this module. Host, server, and macOS
//! enforcement is authoritative; browser WASM enforcement is advisory.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::tools::registry::{Tier, ToolSpec};

/// Maximum number of input files allowed in a single batch or recipe run on the [`Plan::Free`] plan.
pub const FREE_MAX_RUN_FILES: usize = 25;

/// Maximum combined input size in bytes allowed in a single batch or recipe run on the [`Plan::Free`] plan (250 MB).
pub const FREE_MAX_RUN_BYTES: u64 = 250 * 1_000_000;

/// Subscription or workspace plan used to evaluate [`ToolAccess`].
#[derive(
    Clone,
    Copy,
    Debug,
    Default,
    Eq,
    PartialEq,
    Ord,
    PartialOrd,
    Hash,
    Serialize,
    Deserialize,
    JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum Plan {
    /// Free plan (default local and anonymous plan).
    #[default]
    Free,
    /// Individual Pro plan.
    Pro,
    /// Team plan (grants Pro tool access).
    Team,
    /// Enterprise plan (grants Pro tool access).
    Enterprise,
}

impl Plan {
    /// All known plans in ascending tier order.
    pub const ALL: &'static [Self] = &[Self::Free, Self::Pro, Self::Team, Self::Enterprise];

    /// Stable snake_case identifier for this plan.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Free => "free",
            Self::Pro => "pro",
            Self::Team => "team",
            Self::Enterprise => "enterprise",
        }
    }

    /// Returns `true` when this plan grants access to [`Tier::Pro`] tools and unlimited batch runs.
    #[must_use]
    pub const fn grants_pro(self) -> bool {
        match self {
            Self::Free => false,
            Self::Pro | Self::Team | Self::Enterprise => true,
        }
    }
}

impl std::fmt::Display for Plan {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::str::FromStr for Plan {
    type Err = PlanParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "free" => Ok(Self::Free),
            "pro" => Ok(Self::Pro),
            "team" => Ok(Self::Team),
            "enterprise" => Ok(Self::Enterprise),
            _ => Err(PlanParseError(s.to_owned())),
        }
    }
}

/// Error returned when parsing an unrecognized [`Plan`] identifier.
#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
#[error("unknown plan '{0}'; valid plans: free, pro, team, enterprise")]
pub struct PlanParseError(pub String);

/// Combined input size of a single tool or recipe batch run.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RunSize {
    /// Number of input files in the run.
    pub files: usize,
    /// Combined byte length of all input files in the run.
    pub bytes: u64,
}

impl RunSize {
    /// Creates a new [`RunSize`] for `files` inputs totaling `bytes` bytes.
    #[must_use]
    pub const fn new(files: usize, bytes: u64) -> Self {
        Self { files, bytes }
    }
}

/// Pure tool-access policy bound to a [`Plan`].
///
/// Named `ToolAccess` so it does not collide with `ariad-server`'s cloud quota `Entitlement` struct.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ToolAccess {
    /// The caller's active plan.
    pub plan: Plan,
}

impl ToolAccess {
    /// Default Free-plan tool access.
    pub const FREE: Self = Self::for_plan(Plan::Free);
    /// Pro-plan tool access.
    pub const PRO: Self = Self::for_plan(Plan::Pro);

    /// Creates a [`ToolAccess`] policy for `plan`.
    #[must_use]
    pub const fn new(plan: Plan) -> Self {
        Self { plan }
    }

    /// Creates a [`ToolAccess`] policy for `plan`.
    #[must_use]
    pub const fn for_plan(plan: Plan) -> Self {
        Self { plan }
    }

    /// Returns the default local [`ToolAccess`] (`Plan::Free`).
    #[must_use]
    pub const fn local_default() -> Self {
        Self::FREE
    }

    /// Returns `true` if this access policy grants [`Tier::Pro`] tools.
    #[must_use]
    pub const fn grants_pro(self) -> bool {
        self.plan.grants_pro()
    }

    /// Returns `true` if this access policy allows a tool of `tier`.
    #[must_use]
    pub const fn allows(self, tier: Tier) -> bool {
        allowed(tier, self.plan)
    }

    /// Returns `true` if this access policy allows executing `spec`.
    #[must_use]
    pub const fn allows_spec(self, spec: &ToolSpec) -> bool {
        allowed(spec.tier, self.plan)
    }

    /// Returns `true` if a batch run of `size` is within the quota for this plan.
    #[must_use]
    pub const fn within_run_quota(self, size: RunSize) -> bool {
        within_run_quota(self.plan, size)
    }
}

impl From<Plan> for ToolAccess {
    fn from(plan: Plan) -> Self {
        Self::for_plan(plan)
    }
}

/// Pure decision function returning `true` when `plan` permits running a tool of `tier`.
///
/// - [`Tier::Free`] is allowed for every [`Plan`].
/// - [`Tier::Pro`] is allowed only when `plan.grants_pro()` is `true`
///   ([`Plan::Pro`], [`Plan::Team`], [`Plan::Enterprise`]).
#[must_use]
pub const fn allowed(tier: Tier, plan: Plan) -> bool {
    match tier {
        Tier::Free => true,
        Tier::Pro => plan.grants_pro(),
    }
}

/// Pure decision function returning `true` when a run of `size` is within `plan`'s per-run quota.
///
/// On [`Plan::Free`], a run may include at most [`FREE_MAX_RUN_FILES`] (25) files and at most
/// [`FREE_MAX_RUN_BYTES`] (250 MB = 250,000,000 bytes). Pro-granting plans ([`Plan::Pro`],
/// [`Plan::Team`], [`Plan::Enterprise`]) are unlimited.
#[must_use]
pub const fn within_run_quota(plan: Plan, size: RunSize) -> bool {
    if plan.grants_pro() {
        true
    } else {
        size.files <= FREE_MAX_RUN_FILES && size.bytes <= FREE_MAX_RUN_BYTES
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn free_tier_allowed_on_all_plans_and_pro_tier_requires_pro_plan() {
        for &plan in Plan::ALL {
            assert!(
                allowed(Tier::Free, plan),
                "Free tier must be allowed on {plan}"
            );
            assert_eq!(
                allowed(Tier::Pro, plan),
                plan != Plan::Free,
                "Pro tier access mismatch on {plan}"
            );
            let access = ToolAccess::for_plan(plan);
            assert_eq!(access.allows(Tier::Free), allowed(Tier::Free, plan));
            assert_eq!(access.allows(Tier::Pro), allowed(Tier::Pro, plan));
        }
    }

    #[test]
    fn within_run_quota_enforces_free_limits_at_and_over_boundary() {
        assert_eq!(FREE_MAX_RUN_FILES, 25);
        assert_eq!(FREE_MAX_RUN_BYTES, 250_000_000);

        // At exact boundary for both files and bytes on Free plan
        assert!(within_run_quota(
            Plan::Free,
            RunSize {
                files: FREE_MAX_RUN_FILES,
                bytes: FREE_MAX_RUN_BYTES,
            }
        ));

        // Just over the files boundary (26 files)
        assert!(!within_run_quota(
            Plan::Free,
            RunSize {
                files: FREE_MAX_RUN_FILES + 1,
                bytes: 0,
            }
        ));
        assert!(!within_run_quota(
            Plan::Free,
            RunSize {
                files: FREE_MAX_RUN_FILES + 1,
                bytes: FREE_MAX_RUN_BYTES,
            }
        ));

        // Just over the bytes boundary (250_000_001 bytes)
        assert!(!within_run_quota(
            Plan::Free,
            RunSize {
                files: 1,
                bytes: FREE_MAX_RUN_BYTES + 1,
            }
        ));
        assert!(!within_run_quota(
            Plan::Free,
            RunSize {
                files: FREE_MAX_RUN_FILES,
                bytes: FREE_MAX_RUN_BYTES + 1,
            }
        ));

        // Pro-granting plans are unlimited
        for plan in [Plan::Pro, Plan::Team, Plan::Enterprise] {
            assert!(within_run_quota(
                plan,
                RunSize {
                    files: FREE_MAX_RUN_FILES + 1,
                    bytes: FREE_MAX_RUN_BYTES + 1,
                }
            ));
            assert!(within_run_quota(
                plan,
                RunSize {
                    files: usize::MAX,
                    bytes: u64::MAX,
                }
            ));
        }
    }
}
