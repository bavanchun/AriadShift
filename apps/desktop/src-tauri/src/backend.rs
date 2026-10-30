use crate::types::*;
use ariad_host::{
    convert::{self, ConvertEvent, ConvertRequest},
    packs,
    session::{PoolConfig, SessionPool},
};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    thread::{self, JoinHandle},
    time::Duration,
};
use tokio_util::sync::CancellationToken;

pub(crate) struct Job {
    pub view: JobView,
    pub request: QueueRequest,
    pub cancel: CancellationToken,
}
pub(crate) struct Model {
    pub jobs: Vec<Job>,
    pub watches: Vec<WatchView>,
    pub settings: Settings,
    pub pack_progress: String,
    pub pack_cancel: Option<CancellationToken>,
}
pub struct Backend {
    pub(crate) model: Mutex<Model>,
    pub(crate) wake: Condvar,
    pub(crate) stopped: AtomicBool,
    pub(crate) watches: Mutex<HashMap<String, crate::watch::RunningWatch>>,
    threads: Mutex<Vec<JoinHandle<()>>>,
    pool: SessionPool,
    engine: PathBuf,
    config: PathBuf,
    next: AtomicU64,
}
#[derive(serde::Deserialize, serde::Serialize, Default)]
#[serde(default)]
struct Persisted {
    settings: Settings,
    watches: Vec<WatchView>,
}

impl Backend {
    pub fn new(data: &Path, engine: PathBuf) -> Result<Arc<Self>, String> {
        std::fs::create_dir_all(data).map_err(|e| e.to_string())?;
        let config = data.join("desktop.json");
        let saved: Persisted = match std::fs::read(&config) {
            Ok(b) => {
                serde_json::from_slice(&b).map_err(|e| format!("Invalid desktop settings: {e}"))?
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Persisted::default(),
            Err(e) => return Err(e.to_string()),
        };
        validate_setting_values(&saved.settings)?;
        let backend = Arc::new(Self {
            model: Mutex::new(Model {
                jobs: vec![],
                watches: vec![],
                settings: saved.settings,
                pack_progress: String::new(),
                pack_cancel: None,
            }),
            wake: Condvar::new(),
            stopped: AtomicBool::new(false),
            watches: Mutex::new(HashMap::new()),
            threads: Mutex::new(vec![]),
            pool: SessionPool::new(PoolConfig::default()),
            engine,
            config,
            next: AtomicU64::new(1),
        });
        for _ in 0..4 {
            let weak = Arc::downgrade(&backend);
            let handle = thread::spawn(move || {
                while let Some(b) = weak.upgrade() {
                    if b.stopped.load(Ordering::Acquire) {
                        break;
                    }
                    b.work();
                }
            });
            backend.threads.lock().unwrap().push(handle);
        }
        for watch in saved.watches {
            match backend.start_watch(watch.route.clone(), watch.paused, false) {
                Ok(_) => (),
                Err(error) => backend.model.lock().unwrap().watches.push(WatchView {
                    id: backend.id(),
                    route: watch.route,
                    paused: true,
                    error: Some(error),
                }),
            }
        }
        Ok(backend)
    }
    pub(crate) fn id(&self) -> String {
        self.next.fetch_add(1, Ordering::Relaxed).to_string()
    }
    pub(crate) fn persist(&self) -> Result<(), String> {
        let model = self.model.lock().unwrap();
        let bytes = serde_json::to_vec_pretty(&Persisted {
            settings: model.settings.clone(),
            watches: model.watches.clone(),
        })
        .map_err(|e| e.to_string())?;
        let parent = self.config.parent().ok_or("Missing settings directory")?;
        let mut file = tempfile::NamedTempFile::new_in(parent).map_err(|e| e.to_string())?;
        use std::io::Write;
        file.write_all(&bytes).map_err(|e| e.to_string())?;
        file.persist(&self.config).map_err(|e| e.to_string())?;
        Ok(())
    }
    pub fn parse_link(&self, request: LinkRequest) -> Result<Option<DeepLink>, String> {
        crate::deep_link::parse(&request.url)
    }
    /// Drains the files the OS file manager opened with the app since the last call.
    pub fn opened_files(&self, _: Empty) -> Result<Vec<String>, String> {
        Ok(crate::file_open::take())
    }
    pub fn snapshot(&self, _: Empty) -> Result<Snapshot, String> {
        let m = self.model.lock().unwrap();
        Ok(Snapshot {
            jobs: m.jobs.iter().map(|j| j.view.clone()).collect(),
            watches: m.watches.clone(),
            settings: m.settings.clone(),
            pack_progress: m.pack_progress.clone(),
            pack_busy: m.pack_cancel.is_some(),
        })
    }
    pub fn settings(&self, request: Settings) -> Result<Settings, String> {
        validate_settings(&request)?;
        let old = std::mem::replace(&mut self.model.lock().unwrap().settings, request.clone());
        if let Err(e) = self.persist() {
            self.model.lock().unwrap().settings = old;
            return Err(e);
        }
        self.wake.notify_all();
        Ok(request)
    }
    pub fn enqueue(&self, request: QueueRequest) -> Result<Vec<JobView>, String> {
        validate_route(&request.target)?;
        if request.paths.is_empty() || request.paths.len() > 1000 {
            return Err("Choose between 1 and 1000 files".into());
        }
        let folder = request
            .output_folder
            .as_deref()
            .map(directory)
            .transpose()?;
        let mut m = self.model.lock().unwrap();
        if m.jobs.len() + request.paths.len() > 1000 {
            m.jobs
                .retain(|j| matches!(j.view.status, JobStatus::Queued | JobStatus::Running));
            if m.jobs.len() + request.paths.len() > 1000 {
                return Err("Queue limit reached (1000 files)".into());
            }
        }
        let mut result: Vec<JobView> = vec![];
        for input in &request.paths {
            let path = regular_file(input)?;
            if m.jobs.iter().any(|j| {
                j.view.input == path.to_string_lossy()
                    && matches!(j.view.status, JobStatus::Queued | JobStatus::Running)
            }) {
                return Err(format!("Already queued: {}", path.display()));
            }
            let output = output_path(&path, folder.as_deref(), &request.target)?;
            if result.iter().any(|j| Path::new(&j.output) == output)
                || m.jobs.iter().any(|j| {
                    Path::new(&j.view.output) == output
                        && matches!(j.view.status, JobStatus::Queued | JobStatus::Running)
                })
            {
                return Err("Several queued files would write the same output; choose distinct output folders".into());
            }
            if convert::paths_refer_to_same_file(&path, &output) {
                return Err("Output cannot replace its input".into());
            }
            let view = JobView {
                id: self.id(),
                input: path.to_string_lossy().into_owned(),
                output: output.to_string_lossy().into_owned(),
                target: request.target.clone(),
                profile: request.profile,
                status: JobStatus::Queued,
                stage: "Queued".into(),
                error: None,
                warnings: vec![],
            };
            result.push(view);
        }
        // Validate the entire batch before changing the queue.
        for view in &result {
            m.jobs.push(Job {
                view: view.clone(),
                request: QueueRequest {
                    paths: vec![view.input.clone()],
                    target: request.target.clone(),
                    profile: request.profile,
                    output_folder: request.output_folder.clone(),
                    overwrite: request.overwrite,
                },
                cancel: CancellationToken::new(),
            });
        }
        drop(m);
        self.wake.notify_all();
        Ok(result)
    }
    pub fn cancel(&self, request: IdRequest) -> Result<(), String> {
        let mut m = self.model.lock().unwrap();
        let job = m
            .jobs
            .iter_mut()
            .find(|j| j.view.id == request.id)
            .ok_or("Unknown job")?;
        job.cancel.cancel();
        if job.view.status == JobStatus::Queued {
            job.view.status = JobStatus::Cancelled;
            job.view.stage = "Cancelled".into();
        }
        drop(m);
        self.wake.notify_all();
        Ok(())
    }
    pub fn retry(&self, request: IdRequest) -> Result<Vec<JobView>, String> {
        let request = {
            let m = self.model.lock().unwrap();
            let job = m
                .jobs
                .iter()
                .find(|j| j.view.id == request.id)
                .ok_or("Unknown job")?;
            if !matches!(job.view.status, JobStatus::Failed | JobStatus::Cancelled) {
                return Err("Only failed or cancelled jobs can be retried".into());
            }
            QueueRequest {
                paths: vec![job.view.input.clone()],
                ..job.request.clone()
            }
        };
        self.enqueue(request)
    }
    fn work(&self) {
        let mut m = self.model.lock().unwrap();
        let running = m
            .jobs
            .iter()
            .filter(|j| j.view.status == JobStatus::Running)
            .count();
        let next = if running < m.settings.concurrency {
            m.jobs
                .iter_mut()
                .find(|j| j.view.status == JobStatus::Queued)
        } else {
            None
        };
        let Some(job) = next else {
            let _guard = self
                .wake
                .wait_timeout(m, Duration::from_millis(200))
                .unwrap();
            return;
        };
        job.view.status = JobStatus::Running;
        job.view.stage = "Starting".into();
        let id = job.view.id.clone();
        let cancel = job.cancel.clone();
        let mut request = ConvertRequest::new(
            &job.view.input,
            &job.view.output,
            &job.view.target,
            &self.engine,
        );
        request.profile = job.view.profile.into();
        request.overwrite = job.request.overwrite;
        drop(m);
        let report = convert::convert_in(
            &self.pool,
            &request,
            cancel.clone(),
            |ConvertEvent::Progress { stage }| {
                let mut m = self.model.lock().unwrap();
                if let Some(j) = m.jobs.iter_mut().find(|j| j.view.id == id) {
                    j.view.stage = stage;
                }
            },
        );
        let mut m = self.model.lock().unwrap();
        let job = m.jobs.iter_mut().find(|j| j.view.id == id).unwrap();
        match report {
            Ok(report) => {
                job.view.status = JobStatus::Succeeded;
                job.view.stage = format!("Completed in {} ms", report.elapsed.as_millis());
                job.view.warnings = report.warnings.into_iter().map(|w| w.message).collect();
            }
            Err(error) => {
                job.view.status = if cancel.is_cancelled() {
                    JobStatus::Cancelled
                } else {
                    JobStatus::Failed
                };
                job.view.stage = if cancel.is_cancelled() {
                    "Cancelled"
                } else {
                    "Failed"
                }
                .into();
                job.view.error = Some(error.to_string());
            }
        }
        drop(m);
        self.wake.notify_all();
    }
    pub fn inspect(&self, request: InspectRequest) -> Result<Inspection, String> {
        let path = regular_file(&request.path)?;
        let caps = ariad_host::commands::active_capabilities().map_err(|e| e.to_string())?;
        let info = ariad_host::commands::inspect::inspect(
            &path,
            Some(&self.engine),
            &caps,
            CancellationToken::new(),
        )
        .map_err(|e| e.to_string())?;
        let plan = ariad_host::commands::plan::plan(
            &path,
            &request.target,
            request.profile.into(),
            &caps,
            Some(&self.engine),
            CancellationToken::new(),
        )
        .map_err(|e| e.to_string());
        let explanation = match plan {
            Ok(plan) => format!(
                "{} · {} · {}",
                plan.route
                    .iter()
                    .map(|s| format!("{} → {} ({})", s.from.id(), s.to.id(), s.engine))
                    .collect::<Vec<_>>()
                    .join("; "),
                if plan.measured {
                    "Benchmark measured"
                } else {
                    "Unmeasured route"
                },
                plan.missing_engine
                    .map(|e| format!("Missing engine: {e}"))
                    .unwrap_or_else(|| "Runs locally".into())
            ),
            Err(e) => e,
        };
        Ok(Inspection {
            summary: format!(
                "{} · {} bytes{}",
                info.format.id(),
                info.bytes,
                info.counts
                    .as_ref()
                    .map(|c| format!(" · {} words", c.words))
                    .unwrap_or_default()
            ),
            targets: info.reachable.iter().map(|f| f.id().to_owned()).collect(),
            explanation,
            warnings: info.warnings.into_iter().map(|w| w.message).collect(),
        })
    }
    pub fn doctor(&self, _: Empty) -> Result<Vec<Diagnostic>, String> {
        ariad_host::commands::doctor::doctor(&self.engine, CancellationToken::new())
            .map(|r| {
                r.checks
                    .into_iter()
                    .map(|c| Diagnostic {
                        id: c.id,
                        ok: c.ok,
                        required: c.required,
                        message: c.message,
                        hint: c.hint,
                    })
                    .collect()
            })
            .map_err(|e| e.to_string())
    }
    pub fn packs(&self, _: Empty) -> Result<Vec<PackRow>, String> {
        let context = packs::PackContext::from_env().map_err(|e| e.to_string())?;
        let installed = packs::list_installed(&context).map_err(|e| e.to_string())?;
        let mut rows: Vec<_> = packs::index_entries()
            .iter()
            .filter(|entry| !installed.iter().any(|p| p.name == entry.name))
            .map(|p| PackRow {
                name: p.name.clone(),
                version: p.version.clone(),
                description: p.description.clone(),
                installed: false,
                active: false,
            })
            .collect();
        rows.extend(installed.into_iter().map(|p| PackRow {
            name: p.name,
            version: p.version,
            description: "Installed engine pack".into(),
            installed: true,
            active: p.active,
        }));
        Ok(rows)
    }
    pub fn pack(&self, request: PackRequest) -> Result<String, String> {
        let cancel = CancellationToken::new();
        {
            let mut m = self.model.lock().unwrap();
            if m.pack_cancel.is_some() {
                return Err("Another pack operation is running".into());
            }
            m.pack_cancel = Some(cancel.clone());
            m.pack_progress = "Starting pack operation".into();
        }
        let result = (|| {
            let context = packs::PackContext::from_env().map_err(|e| e.to_string())?;
            match request.action {
                PackAction::Install => {
                    let source = match request.from {
                        None => packs::InstallSource::Index,
                        Some(s) if Path::new(&s).is_dir() => {
                            packs::InstallSource::Directory(PathBuf::from(s))
                        }
                        Some(s) => packs::InstallSource::Manifest(s),
                    };
                    let report = packs::install(
                        &context,
                        &packs::InstallRequest {
                            name: request.name,
                            version: request.version,
                            source,
                        },
                        &cancel,
                        &mut |event| {
                            self.model.lock().unwrap().pack_progress = match event {
                                packs::PackEvent::Stage(s) => s,
                                packs::PackEvent::Download {
                                    component,
                                    done,
                                    total,
                                } => format!("{component}: {done}/{total} bytes"),
                            };
                        },
                    )
                    .map_err(|e| e.to_string())?;
                    Ok(format!("Installed {} {}", report.name, report.version))
                }
                PackAction::Verify => {
                    let report =
                        packs::verify(&context, &request.name, request.version.as_deref(), &cancel)
                            .map_err(|e| e.to_string())?;
                    if report.ok {
                        Ok(format!("Verified {} files", report.checked))
                    } else {
                        Err(format!(
                            "Verification failed: modified {:?}, missing {:?}, problems {:?}",
                            report.modified, report.missing, report.problems
                        ))
                    }
                }
                PackAction::Remove => {
                    packs::remove(&context, &request.name, request.version.as_deref())
                        .map_err(|e| e.to_string())?;
                    Ok(format!("Removed {}", request.name))
                }
            }
        })();
        let mut m = self.model.lock().unwrap();
        m.pack_cancel = None;
        m.pack_progress = result.clone().unwrap_or_else(|e| e);
        result
    }
    pub fn pack_cancel(&self, _: Empty) -> Result<(), String> {
        if let Some(c) = &self.model.lock().unwrap().pack_cancel {
            c.cancel();
        }
        Ok(())
    }
    pub fn shutdown(&self) {
        self.stopped.store(true, Ordering::Release);
        {
            let m = self.model.lock().unwrap();
            for j in &m.jobs {
                j.cancel.cancel();
            }
            if let Some(c) = &m.pack_cancel {
                c.cancel();
            }
        }
        self.watches.lock().unwrap().clear();
        self.wake.notify_all();
        for handle in self.threads.lock().unwrap().drain(..) {
            let _ = handle.join();
        }
        self.pool.shutdown();
    }
}
pub(crate) fn directory(value: &str) -> Result<PathBuf, String> {
    let p = std::fs::canonicalize(value).map_err(|e| e.to_string())?;
    if !p.is_dir() {
        return Err("Choose an existing folder".into());
    }
    Ok(p)
}
pub(crate) fn regular_file(value: &str) -> Result<PathBuf, String> {
    let m = std::fs::symlink_metadata(value).map_err(|e| e.to_string())?;
    if !m.is_file() {
        return Err("Choose a regular file (links are not accepted)".into());
    }
    std::fs::canonicalize(value).map_err(|e| e.to_string())
}
pub(crate) fn validate_route(target: &str) -> Result<(), String> {
    if crate::deep_link::output_format(target).is_none() {
        return Err("Unsupported output format".into());
    }
    Ok(())
}
fn output_path(input: &Path, folder: Option<&Path>, target: &str) -> Result<PathBuf, String> {
    let stem = input.file_stem().ok_or("File has no name")?;
    let mut name = stem.to_os_string();
    name.push(".");
    let format = crate::deep_link::output_format(target).ok_or("Unsupported output format")?;
    name.push(format.extensions()[0]);
    Ok(folder.unwrap_or_else(|| input.parent().unwrap()).join(name))
}
fn validate_settings(s: &Settings) -> Result<(), String> {
    validate_setting_values(s)?;
    if let Some(folder) = &s.output_folder {
        directory(folder)?;
    }
    Ok(())
}
fn validate_setting_values(s: &Settings) -> Result<(), String> {
    if !(1..=4).contains(&s.concurrency) {
        return Err("Concurrency must be between 1 and 4".into());
    }
    if s.output_policy == OutputPolicy::Folder && s.output_folder.is_none() {
        return Err("Choose a default output folder".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;
    struct TestApp {
        backend: Arc<Backend>,
        _dir: tempfile::TempDir,
    }
    impl TestApp {
        fn new() -> Self {
            let dir = tempfile::tempdir().unwrap();
            let backend = Backend::new(dir.path(), std::env::current_exe().unwrap()).unwrap();
            Self { backend, _dir: dir }
        }
    }
    impl Drop for TestApp {
        fn drop(&mut self) {
            self.backend.shutdown();
        }
    }
    fn wait(b: &Backend, count: usize) -> Vec<JobView> {
        let start = Instant::now();
        loop {
            let jobs = b.snapshot(Empty {}).unwrap().jobs;
            if jobs.len() >= count
                && jobs
                    .iter()
                    .all(|j| !matches!(j.status, JobStatus::Queued | JobStatus::Running))
            {
                return jobs;
            }
            assert!(
                start.elapsed() < Duration::from_secs(20),
                "queue did not finish"
            );
            thread::sleep(Duration::from_millis(40));
        }
    }
    fn request(paths: Vec<String>) -> QueueRequest {
        QueueRequest {
            paths,
            target: "html".into(),
            profile: Profile::Private,
            output_folder: None,
            overwrite: false,
        }
    }
    #[test]
    fn batch_converts_and_preserves_existing_outputs_until_retry() {
        let app = TestApp::new();
        let a = app._dir.path().join("a.md");
        let b = app._dir.path().join("b.md");
        std::fs::write(&a, "# Alpha\n\nVietnamese: tiếng Việt.").unwrap();
        std::fs::write(&b, "# Beta").unwrap();
        std::fs::write(b.with_extension("html"), "existing").unwrap();
        let jobs = app
            .backend
            .enqueue(request(vec![
                a.to_string_lossy().into_owned(),
                b.to_string_lossy().into_owned(),
            ]))
            .unwrap();
        {
            let model = app.backend.model.lock().unwrap();
            for job in &model.jobs {
                assert_eq!(job.request.paths.len(), 1);
                assert_eq!(job.request.paths[0], job.view.input);
            }
        }
        let done = wait(&app.backend, 2);
        assert_eq!(done[0].status, JobStatus::Succeeded);
        assert_eq!(done[1].status, JobStatus::Failed);
        assert!(
            std::fs::read_to_string(a.with_extension("html"))
                .unwrap()
                .contains("tiếng Việt")
        );
        assert_eq!(
            std::fs::read_to_string(b.with_extension("html")).unwrap(),
            "existing"
        );
        std::fs::remove_file(b.with_extension("html")).unwrap();
        app.backend
            .retry(IdRequest {
                id: jobs[1].id.clone(),
            })
            .unwrap();
        assert_eq!(wait(&app.backend, 3)[2].status, JobStatus::Succeeded);
    }
    #[test]
    fn invalid_batch_is_atomic_and_settings_are_validated_and_persisted() {
        let app = TestApp::new();
        let a = app._dir.path().join("a.md");
        std::fs::write(&a, "# Safe").unwrap();
        assert!(
            app.backend
                .enqueue(request(vec![
                    a.to_string_lossy().into_owned(),
                    "/does/not/exist.md".into()
                ]))
                .is_err()
        );
        assert!(app.backend.snapshot(Empty {}).unwrap().jobs.is_empty());
        let mut settings = Settings {
            concurrency: 5,
            ..Settings::default()
        };
        assert!(app.backend.settings(settings.clone()).is_err());
        settings.concurrency = 1;
        settings.profile = Profile::Private;
        settings.telemetry = true;
        app.backend.settings(settings).unwrap();
        let saved: Persisted =
            serde_json::from_slice(&std::fs::read(&app.backend.config).unwrap()).unwrap();
        assert_eq!(saved.settings.concurrency, 1);
        assert_eq!(saved.settings.profile, Profile::Private);
        assert!(saved.settings.telemetry);
        assert!(
            app.backend
                .pack(PackRequest {
                    name: "../bad".into(),
                    version: None,
                    action: PackAction::Install,
                    from: None
                })
                .is_err()
        );
        assert!(!app.backend.snapshot(Empty {}).unwrap().pack_busy);
    }
    #[test]
    fn missing_saved_output_folder_can_be_repaired_after_restart() {
        let app = TestApp::new();
        let folder = app._dir.path().join("output");
        std::fs::create_dir(&folder).unwrap();
        app.backend
            .settings(Settings {
                output_policy: OutputPolicy::Folder,
                output_folder: Some(folder.to_string_lossy().into_owned()),
                ..Settings::default()
            })
            .unwrap();
        app.backend.shutdown();
        std::fs::remove_dir(&folder).unwrap();
        let restored = Backend::new(app._dir.path(), std::env::current_exe().unwrap()).unwrap();
        let saved = restored.snapshot(Empty {}).unwrap().settings;
        assert_eq!(saved.output_policy, OutputPolicy::Folder);
        assert!(restored.settings(saved).is_err());
        assert!(restored.settings(Settings::default()).is_ok());
        restored.shutdown();
    }
    #[test]
    fn queued_jobs_cancel_without_creating_output() {
        let app = TestApp::new();
        app.backend
            .settings(Settings {
                concurrency: 1,
                ..Settings::default()
            })
            .unwrap();
        let paths = (0..100)
            .map(|i| {
                let path = app._dir.path().join(format!("input-{i}.md"));
                std::fs::write(&path, "# A document").unwrap();
                path.to_string_lossy().into_owned()
            })
            .collect();
        let jobs = app.backend.enqueue(request(paths)).unwrap();
        let last = jobs.last().unwrap();
        app.backend
            .cancel(IdRequest {
                id: last.id.clone(),
            })
            .unwrap();
        let done = wait(&app.backend, 100);
        assert_eq!(done.last().unwrap().status, JobStatus::Cancelled);
        assert!(!Path::new(&last.output).exists());
    }
    #[test]
    fn watches_accept_reader_only_inputs_and_normalize_aliases() {
        let app = TestApp::new();
        let input = app._dir.path().join("incoming");
        let output = app._dir.path().join("out");
        std::fs::create_dir(&input).unwrap();
        std::fs::create_dir(&output).unwrap();
        let route = WatchRequest {
            interval_seconds: 2,
            recursive: false,
            input: input.to_string_lossy().into_owned(),
            output: output.to_string_lossy().into_owned(),
            from: "pdf".into(),
            target: "html".into(),
            profile: Profile::Editable,
            overwrite: false,
        };
        for (from, expected) in [
            ("pdf", "pdf"),
            ("jpg", "jpeg"),
            ("docling+json", "docling+json"),
        ] {
            let watch = app
                .backend
                .watch(WatchRequest {
                    from: from.into(),
                    ..route.clone()
                })
                .unwrap();
            assert_eq!(watch.route.from, expected);
            app.backend
                .watch_control(WatchControl {
                    id: watch.id,
                    paused: true,
                    remove: true,
                })
                .unwrap();
        }
        assert!(
            app.backend
                .watch(WatchRequest {
                    target: "png".into(),
                    ..route.clone()
                })
                .is_err()
        );
        assert!(
            app.backend
                .watch(WatchRequest {
                    from: "markdown".into(),
                    target: "md".into(),
                    ..route
                })
                .is_err()
        );
    }
    #[test]
    fn watch_debounces_changes_pauses_and_rejects_output_loops() {
        let app = TestApp::new();
        let input = app._dir.path().join("incoming");
        let output = app._dir.path().join("out");
        std::fs::create_dir(&input).unwrap();
        std::fs::create_dir(&output).unwrap();
        let route = WatchRequest {
            interval_seconds: 1,
            recursive: false,
            input: input.to_string_lossy().into_owned(),
            output: output.to_string_lossy().into_owned(),
            from: "md".into(),
            target: "html".into(),
            profile: Profile::Editable,
            overwrite: true,
        };
        assert!(
            app.backend
                .watch(WatchRequest {
                    output: input.to_string_lossy().into_owned(),
                    ..route.clone()
                })
                .is_err()
        );
        let w = app.backend.watch(route).unwrap();
        let file = input.join("watched.md");
        std::fs::write(&file, "# First").unwrap();
        thread::sleep(Duration::from_millis(100));
        std::fs::write(&file, "# Last").unwrap();
        let jobs = wait(&app.backend, 1);
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].status, JobStatus::Succeeded);
        assert!(
            std::fs::read_to_string(output.join("watched.html"))
                .unwrap()
                .contains("Last")
        );
        app.backend
            .watch_control(WatchControl {
                id: w.id.clone(),
                paused: true,
                remove: false,
            })
            .unwrap();
        std::fs::write(input.join("paused.md"), "# Paused").unwrap();
        thread::sleep(Duration::from_secs(2));
        assert!(!output.join("paused.html").exists());
        let saved: Persisted =
            serde_json::from_slice(&std::fs::read(&app.backend.config).unwrap()).unwrap();
        assert!(saved.watches[0].paused);
        app.backend
            .watch_control(WatchControl {
                id: w.id,
                paused: true,
                remove: true,
            })
            .unwrap();
        assert!(app.backend.snapshot(Empty {}).unwrap().watches.is_empty());
    }
    #[test]
    fn recursive_watch_keeps_same_named_outputs_in_their_subfolders() {
        let app = TestApp::new();
        let input = app._dir.path().join("in");
        let output = app._dir.path().join("out");
        for dir in [input.join("one"), input.join("two"), output.clone()] {
            std::fs::create_dir_all(dir).unwrap();
        }
        app.backend
            .watch(WatchRequest {
                interval_seconds: 1,
                recursive: true,
                input: input.to_string_lossy().into_owned(),
                output: output.to_string_lossy().into_owned(),
                from: "md".into(),
                target: "html".into(),
                profile: Profile::Private,
                overwrite: false,
            })
            .unwrap();
        std::fs::write(input.join("one/same.md"), "# One").unwrap();
        std::fs::write(input.join("two/same.md"), "# Two").unwrap();
        let done = wait(&app.backend, 2);
        assert!(done.iter().all(|j| j.status == JobStatus::Succeeded));
        assert!(
            std::fs::read_to_string(output.join("one/same.html"))
                .unwrap()
                .contains("One")
        );
        assert!(
            std::fs::read_to_string(output.join("two/same.html"))
                .unwrap()
                .contains("Two")
        );
    }
}
