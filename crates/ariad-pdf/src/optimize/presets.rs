//! The compression level and target-size quality ladder.

use ariad_core::tools::params::{CompressParams, CompressPreset};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Profile {
    pub dpi: u32,
    pub quality: u8,
    pub lossless: bool,
}

/// Level profiles, ordered from highest to lowest fidelity.
pub const LEVELS: [Profile; 9] = [
    Profile {
        dpi: 1200,
        quality: 100,
        lossless: true,
    },
    Profile {
        dpi: 1200,
        quality: 100,
        lossless: true,
    },
    Profile {
        dpi: 1200,
        quality: 100,
        lossless: true,
    },
    Profile {
        dpi: 300,
        quality: 85,
        lossless: false,
    },
    Profile {
        dpi: 300,
        quality: 85,
        lossless: false,
    },
    Profile {
        dpi: 96,
        quality: 60,
        lossless: false,
    },
    Profile {
        dpi: 96,
        quality: 60,
        lossless: false,
    },
    Profile {
        dpi: 96,
        quality: 60,
        lossless: false,
    },
    Profile {
        dpi: 72,
        quality: 50,
        lossless: false,
    },
];

pub const LADDER: [Profile; 8] = [
    Profile {
        dpi: 300,
        quality: 90,
        lossless: false,
    },
    Profile {
        dpi: 300,
        quality: 85,
        lossless: false,
    },
    Profile {
        dpi: 200,
        quality: 80,
        lossless: false,
    },
    Profile {
        dpi: 150,
        quality: 75,
        lossless: false,
    },
    Profile {
        dpi: 120,
        quality: 70,
        lossless: false,
    },
    Profile {
        dpi: 96,
        quality: 60,
        lossless: false,
    },
    Profile {
        dpi: 72,
        quality: 50,
        lossless: false,
    },
    Profile {
        dpi: 36,
        quality: 30,
        lossless: false,
    },
];

pub fn selected(params: &CompressParams) -> Profile {
    let level = params.level.unwrap_or(
        match params.effective_preset().unwrap_or(CompressPreset::Ebook) {
            CompressPreset::Lossless => 1,
            CompressPreset::Print => 4,
            CompressPreset::Ebook => 6,
            CompressPreset::Screen => 8,
        },
    );
    cap(LEVELS[usize::from(level.saturating_sub(1).min(8))], params)
}

pub fn cap(mut profile: Profile, params: &CompressParams) -> Profile {
    if let Some(dpi) = params.max_dpi {
        profile.dpi = profile.dpi.min(dpi);
    }
    if let Some(quality) = params.jpeg_quality {
        profile.quality = profile.quality.min(quality);
    }
    profile
}
