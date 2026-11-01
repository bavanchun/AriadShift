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

/// An engine that no worker pack is known to provide.
#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
#[error("engine '{engine}' is not provided by any server worker pack")]
pub struct UnknownEngine {
    /// The engine id that has no pack.
    pub engine: String,
}

impl Pack {
    pub const ALL: [Self; 3] = [Self::Core, Self::Office, Self::Docling];

    /// Docling's PDF runtime needs room for loaded models and its 256 MiB
    /// allocation guard. See the real-pack measurement in docs/server.md.
    #[must_use]
    pub const fn minimum_memory_mb(self) -> u32 {
        match self {
            Self::Docling => 2048,
            Self::Core | Self::Office => 0,
        }
    }

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

    /// The pack whose image provides an engine. Fails closed: an engine that no
    /// worker image is known to contain is an error, never the base install.
    pub fn for_engine(engine: &str) -> Result<Self, UnknownEngine> {
        match engine {
            "ariad-core" | "pandoc" | "pdf" | "typst" => Ok(Self::Core),
            "libreoffice" => Ok(Self::Office),
            "docling" => Ok(Self::Docling),
            other => Err(UnknownEngine {
                engine: other.to_owned(),
            }),
        }
    }

    /// The pack that can run every step of a route. Heavier images contain
    /// the base install, so the heaviest step decides.
    pub fn for_route(steps: &[PlanStep]) -> Result<Self, UnknownEngine> {
        let mut heaviest = Self::Core;
        for step in steps {
            heaviest = heaviest.max(Self::for_engine(&step.engine)?);
        }
        Ok(heaviest)
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
            Ok(Pack::Core)
        );
        assert_eq!(
            Pack::for_route(&[step("libreoffice"), step("pandoc")]),
            Ok(Pack::Office)
        );
        assert_eq!(
            Pack::for_route(&[step("libreoffice"), step("docling")]),
            Ok(Pack::Docling)
        );
        assert_eq!(Pack::for_route(&[]), Ok(Pack::Core));
    }

    #[test]
    fn every_registered_conversion_engine_has_a_pack() {
        for engine in [
            "ariad-core",
            "pandoc",
            "pdf",
            "typst",
            "libreoffice",
            "docling",
        ] {
            assert!(Pack::for_engine(engine).is_ok(), "{engine}");
        }
    }

    #[test]
    fn unknown_engines_fail_closed_instead_of_joining_the_base_install() {
        // The pdfium pack has no cloud mapping yet, so it must not run on a core worker.
        for engine in ["pdfium", "qpdf", "tesseract", "no-such-engine", ""] {
            assert_eq!(
                Pack::for_engine(engine),
                Err(UnknownEngine {
                    engine: engine.to_owned()
                }),
                "{engine:?}"
            );
        }
        let error = Pack::for_route(&[step("pandoc"), step("pdfium")]).unwrap_err();
        assert_eq!(error.engine, "pdfium");
        assert!(error.to_string().contains("'pdfium'"), "{error}");
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
