//! Run the real Pandoc adapter on the Windows executable's default stack size.

use std::{fs, io::Cursor, process::ExitCode};

use ariad_core::{
    limits::Limits,
    protocol::{Input, Output, PROTOCOL, Request},
    reader::markdown,
};
use ariad_host::{engines::pandoc, workspace::Workspace};

#[test]
fn deep_list_pandoc_engine_fits_a_one_mib_stack() {
    std::thread::Builder::new()
        .name("deep-list-engine-one-mib".into())
        .stack_size(1024 * 1024)
        .spawn(|| {
            let workspace = Workspace::new().unwrap();
            let input = workspace.input_dir().join("document.ir.json");
            let limits = Limits::local();
            let document = markdown::read(
                include_str!("../../../fixtures/md/en-deep-list.md"),
                &limits,
            )
            .unwrap()
            .document;
            fs::write(&input, serde_json::to_vec(&document).unwrap()).unwrap();
            let request = Request::Convert {
                protocol: PROTOCOL.to_owned(),
                job: "deep-list".into(),
                input: Input {
                    path: input.to_string_lossy().into_owned(),
                    format: "ariad-ir+json".into(),
                },
                output: Output {
                    dir: workspace.output_dir().to_string_lossy().into_owned(),
                    format: "docx".into(),
                },
                work_dir: workspace.work_dir().to_string_lossy().into_owned(),
                options: Default::default(),
                limits,
                session_id: None,
            };
            let mut bytes = serde_json::to_vec(&request).unwrap();
            bytes.push(b'\n');
            let mut events = Vec::new();
            assert_eq!(
                pandoc::serve(Cursor::new(bytes), &mut events),
                ExitCode::SUCCESS,
                "{}",
                String::from_utf8_lossy(&events)
            );
            assert!(workspace.output_dir().join("document.docx").is_file());
        })
        .unwrap()
        .join()
        .unwrap();
}
