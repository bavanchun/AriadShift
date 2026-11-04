//! Locating and querying the digital signature engine.
//!
//! The engine is a Python program speaking `ariad-engine/2`. It comes from the
//! `ASHIFT_SIGN_ENGINE` development override, which holds a command line such as
//! `engines/sign/.venv/bin/python -I -m ariad_sign`, or from the `sign` engine pack.

use std::{
    collections::BTreeMap,
    ffi::OsString,
    path::{Path, PathBuf},
};

use serde_json::Value;

use crate::{packs, runner};

/// The engine identifier used in capabilities and tool specifications.
pub const ENGINE_ID: &str = "sign";

/// Development override: the command line that starts the engine.
pub const OVERRIDE_ENV: &str = "ASHIFT_SIGN_ENGINE";

/// Environment variable naming the pack entrypoint program.
pub const PROGRAM_ENV: &str = "ASHIFT_SIGN_PROGRAM";

/// How to start the sign engine and any request options it needs.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EngineCommand {
    pub program: PathBuf,
    pub args: Vec<OsString>,
    pub env: Vec<(OsString, OsString)>,
    pub options: BTreeMap<String, Value>,
    pub install_root: Option<PathBuf>,
}

impl EngineCommand {
    /// Converts to a runner engine command.
    #[must_use]
    pub fn runner_command(&self, launcher: Option<&Path>) -> runner::EngineCommand {
        runner::EngineCommand {
            engine: ENGINE_ID.to_owned(),
            program: self.program.clone(),
            args: self.args.clone(),
            env: self.env.clone(),
            install_root: self.install_root.clone(),
            launcher: launcher.map(Path::to_path_buf),
            sandbox: None,
        }
    }
}

/// Finds the engine: the development override first, then an installed pack.
#[must_use]
pub fn resolve() -> Option<EngineCommand> {
    if let Some(value) = std::env::var_os(OVERRIDE_ENV) {
        return from_override(&value);
    }
    installed_pack()
}

fn installed_pack() -> Option<EngineCommand> {
    let pack = packs::resolve(ENGINE_ID)?;
    Some(EngineCommand {
        install_root: Some(pack.root),
        program: pack.program,
        args: pack.args,
        env: pack.env,
        options: pack.options.into_iter().collect(),
    })
}

fn from_override(value: &std::ffi::OsStr) -> Option<EngineCommand> {
    let raw = value.to_str()?;
    let words = split_command_line(raw)?;
    let (program, args) = words.split_first()?;
    Some(EngineCommand {
        install_root: None,
        program: PathBuf::from(program),
        args: args.iter().map(OsString::from).collect(),
        env: Vec::new(),
        options: BTreeMap::new(),
    })
}

/// Splits a command line on whitespace; double quotes group words that contain spaces.
#[must_use]
pub fn split_command_line(line: &str) -> Option<Vec<String>> {
    let mut words = Vec::new();
    let mut current = String::new();
    let mut in_word = false;
    let mut quoted = false;
    for character in line.chars() {
        match character {
            '"' => {
                quoted = !quoted;
                in_word = true;
            }
            c if c.is_whitespace() && !quoted => {
                if in_word {
                    words.push(std::mem::take(&mut current));
                    in_word = false;
                }
            }
            c => {
                current.push(c);
                in_word = true;
            }
        }
    }
    if quoted {
        return None;
    }
    if in_word {
        words.push(current);
    }
    Some(words)
}
