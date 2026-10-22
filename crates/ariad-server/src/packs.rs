//! Engine packs, their queues, and which pack a route needs.

use std::{fmt, str::FromStr};

use ariad_core::planner::PlanStep;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// A worker image class. Each pack consumes its own queue.
#[derive(
    Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, ToSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum Pack {
    /// The base install: native readers and writers, Pandoc.
    Core,
    /// The base install plus LibreOffice.
    Office,
    /// The Office install plus the docling engine pack.
    Docling,
}

impl Pack {
    pub const ALL: [Self; 3] = [Self::Core, Self::Office, Self::Docling];

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Core => "core",
            Self::Office => "office",
            Self::Docling => "docling",
        }
    }

    /// The pgmq queue this pack's workers read.
    #[must_use]
    pub const fn queue(self) -> &'static str {
        match self {
            Self::Core => "jobs_core",
            Self::Office => "jobs_office",
            Self::Docling => "jobs_docling",
        }
    }

    /// The pack whose image provides an engine. Engines that no heavier pack
    /// claims ship with the base install.
    #[must_use]
    pub fn for_engine(engine: &str) -> Self {
        match engine {
            "docling" => Self::Docling,
            "libreoffice" => Self::Office,
            _ => Self::Core,
        }
    }

    /// The pack that can run every step of a route. Heavier images contain
    /// the base install, so the heaviest step decides.
    #[must_use]
    pub fn for_route(steps: &[PlanStep]) -> Self {
        steps
            .iter()
            .map(|step| Self::for_engine(&step.engine))
            .max()
            .unwrap_or(Self::Core)
    }
}

impl fmt::Display for Pack {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for Pack {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "core" => Ok(Self::Core),
            "office" => Ok(Self::Office),
            "docling" => Ok(Self::Docling),
            other => Err(format!(
                "unknown pack '{other}'; expected core, office or docling"
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use ariad_core::format::Format;

    use super::*;

    fn step(engine: &str) -> PlanStep {
        PlanStep {
            from: Format::Markdown,
            to: Format::AriadIrJson,
            engine: engine.to_owned(),
        }
    }

    #[test]
    fn heaviest_engine_decides_the_pack() {
        assert_eq!(
            Pack::for_route(&[step("ariad-core"), step("pandoc")]),
            Pack::Core
        );
        assert_eq!(
            Pack::for_route(&[step("libreoffice"), step("pandoc")]),
            Pack::Office
        );
        assert_eq!(
            Pack::for_route(&[step("libreoffice"), step("docling")]),
            Pack::Docling
        );
        assert_eq!(Pack::for_route(&[]), Pack::Core);
    }

    #[test]
    fn queue_names_match_the_migration() {
        let queues: Vec<_> = Pack::ALL.iter().map(|p| p.queue()).collect();
        assert_eq!(queues, ["jobs_core", "jobs_office", "jobs_docling"]);
        let migration = include_str!("../migrations/20261022000001_cloud_schema.sql");
        for queue in queues {
            assert!(migration.contains(&format!("pgmq.create('{queue}')")));
        }
    }
}
