//! The same command table registers IPC handlers and generates their wrappers.
use crate::types::*;
use ts_rs::TS;

macro_rules! command_table {
    ($consumer:ident) => {
        $consumer! {
            (snapshot, Empty, Snapshot),
            (parse_link, LinkRequest, Option<DeepLink>),
            (enqueue, QueueRequest, Vec<JobView>),
            (cancel, IdRequest, ()),
            (retry, IdRequest, Vec<JobView>),
            (inspect, InspectRequest, Inspection),
            (settings, Settings, Settings),
            (watch, WatchRequest, WatchView),
            (watch_control, WatchControl, ()),
            (packs, Empty, Vec<PackRow>),
            (pack, PackRequest, String),
            (pack_cancel, Empty, ()),
            (doctor, Empty, Vec<Diagnostic>),
        }
    };
}
pub(crate) use command_table;

fn namespace_type(type_name: &str, dto_names: &[String]) -> String {
    let is_identifier =
        |character: char| character.is_alphanumeric() || matches!(character, '_' | '$');
    let mut qualified = String::with_capacity(type_name.len());
    for part in type_name.split_inclusive(|character| !is_identifier(character)) {
        let identifier = part.trim_end_matches(|character| !is_identifier(character));
        if dto_names.iter().any(|name| name == identifier) {
            qualified.push_str("T.");
        }
        qualified.push_str(part);
    }
    qualified
}

pub fn export(directory: &std::path::Path) -> std::io::Result<()> {
    std::fs::create_dir_all(directory)?;
    let header = "// Generated from Rust. Do not edit. Run just desktop-bindings.\n";
    let mut types = header.to_owned();
    let mut dto_names = Vec::new();
    let cfg = ts_rs::Config::default();
    macro_rules! declare {
        ($($ty:ty),* $(,)?) => { $(
            dto_names.push(<$ty>::name(&cfg));
            types.push_str("export ");
            types.push_str(&<$ty>::decl(&cfg));
            types.push('\n');
        )* };
    }
    declare!(
        Empty,
        Profile,
        OutputPolicy,
        Settings,
        QueueRequest,
        IdRequest,
        InspectRequest,
        Inspection,
        JobStatus,
        JobView,
        WatchRequest,
        WatchView,
        WatchControl,
        PackRow,
        PackAction,
        PackRequest,
        Diagnostic,
        Snapshot,
        DeepLink,
        LinkRequest
    );
    let mut commands = format!(
        "{header}import {{ invoke }} from '@tauri-apps/api/core';\nimport type * as T from './types';\n"
    );
    macro_rules! wrappers {
        ($(($name:ident,$arg:ty,$result:ty)),* $(,)?) => { $(
            let arg = namespace_type(&<$arg>::name(&cfg), &dto_names);
            let result = namespace_type(&<$result>::name(&cfg), &dto_names);
            commands.push_str(&format!(
                "export async function {}(request: {arg}): Promise<{result}> {{ return invoke<{result}>('{}', {{ request }}); }}\n",
                stringify!($name), stringify!($name)
            ));
        )* };
    }
    command_table!(wrappers);
    std::fs::write(directory.join("types.ts"), types)?;
    std::fs::write(directory.join("commands.ts"), commands)
}

#[cfg(test)]
mod tests {
    use super::namespace_type;

    #[test]
    fn qualifies_only_known_dto_identifiers() {
        let dto_names = vec!["DeepLink".to_owned(), "JobView".to_owned()];
        for (input, expected) in [
            ("DeepLink | null", "T.DeepLink | null"),
            ("Array<JobView>", "Array<T.JobView>"),
            (
                "Array<Array<JobView | null>>",
                "Array<Array<T.JobView | null>>",
            ),
            (
                "[DeepLink, Record<string, JobView>]",
                "[T.DeepLink, Record<string, T.JobView>]",
            ),
            (
                "Array<string | number | boolean | null>",
                "Array<string | number | boolean | null>",
            ),
            ("DeepLinkEvent | JobViews", "DeepLinkEvent | JobViews"),
        ] {
            assert_eq!(namespace_type(input, &dto_names), expected);
        }
    }
}
