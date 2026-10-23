//! Shared source for IPC handlers, bindings and the application ACL.
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
#[allow(unused_imports)]
// The build script consumes names; the library also consumes the macro.
pub(crate) use command_table;

macro_rules! names {
    ($(($name:ident, $arg:ty, $result:ty)),* $(,)?) => {
        pub const NAMES: &[&str] = &[$(stringify!($name)),*];
    };
}
command_table!(names);
