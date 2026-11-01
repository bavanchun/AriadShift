#![allow(dead_code)]

//! Test support for engine packs: a loopback HTTP server with fault injection, throwaway
//! signing keys, archive builders that can produce malicious entries, and a pack builder.

use std::{
    collections::HashMap,
    fs,
    io::{BufRead, BufReader, Cursor, Write},
    net::{SocketAddr, TcpListener, TcpStream},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
    time::Duration,
};

use ariad_host::packs::{self, FetchPolicy, ManifestRequest, PackContext, TrustedKey, TrustedKeys};

/// How the server answers one path.
#[derive(Clone)]
pub enum Route {
    Body(Vec<u8>),
    /// Announces the full length but closes the connection after `after` bytes.
    Truncated {
        body: Vec<u8>,
        after: usize,
    },
    Status(u16),
}

/// A minimal HTTP/1.1 server on 127.0.0.1 that serves one request per connection.
pub struct Server {
    addr: SocketAddr,
    routes: Arc<Mutex<HashMap<String, Route>>>,
    hits: Arc<Mutex<Vec<String>>>,
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl Server {
    pub fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback");
        let addr = listener.local_addr().expect("local addr");
        let routes: Arc<Mutex<HashMap<String, Route>>> = Arc::default();
        let hits: Arc<Mutex<Vec<String>>> = Arc::default();
        let stop = Arc::new(AtomicBool::new(false));
        let handle = {
            let (routes, hits, stop) = (routes.clone(), hits.clone(), stop.clone());
            thread::spawn(move || {
                for stream in listener.incoming() {
                    if stop.load(Ordering::SeqCst) {
                        break;
                    }
                    if let Ok(stream) = stream {
                        let _ = serve(stream, &routes, &hits);
                    }
                }
            })
        };
        Self {
            addr,
            routes,
            hits,
            stop,
            handle: Some(handle),
        }
    }

    pub fn url(&self, path: &str) -> String {
        format!("http://{}/{}", self.addr, path.trim_start_matches('/'))
    }

    pub fn put(&self, path: &str, route: Route) {
        self.routes
            .lock()
            .unwrap()
            .insert(format!("/{}", path.trim_start_matches('/')), route);
    }

    /// Serves every file of `dir` under `prefix/`.
    pub fn put_dir(&self, prefix: &str, dir: &Path) {
        for entry in fs::read_dir(dir).expect("read pack dir") {
            let entry = entry.unwrap();
            if entry.path().is_file() {
                let name = entry.file_name().into_string().unwrap();
                self.put(
                    &format!("{prefix}/{name}"),
                    Route::Body(fs::read(entry.path()).unwrap()),
                );
            }
        }
    }

    pub fn hits(&self, path: &str) -> usize {
        let path = format!("/{}", path.trim_start_matches('/'));
        self.hits
            .lock()
            .unwrap()
            .iter()
            .filter(|hit| **hit == path)
            .count()
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        let _ = TcpStream::connect(self.addr);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

fn serve(
    stream: TcpStream,
    routes: &Mutex<HashMap<String, Route>>,
    hits: &Mutex<Vec<String>>,
) -> std::io::Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut request_line = String::new();
    reader.read_line(&mut request_line)?;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line)? == 0 || line == "\r\n" || line == "\n" {
            break;
        }
    }
    let path = request_line
        .split_whitespace()
        .nth(1)
        .unwrap_or("/")
        .to_owned();
    hits.lock().unwrap().push(path.clone());
    let route = routes.lock().unwrap().get(&path).cloned();
    let mut stream = stream;
    match route {
        Some(Route::Body(body)) => {
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            )?;
            stream.write_all(&body)?;
        }
        Some(Route::Truncated { body, after }) => {
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            )?;
            stream.write_all(&body[..after.min(body.len())])?;
        }
        Some(Route::Status(code)) => {
            write!(
                stream,
                "HTTP/1.1 {code} Status\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
            )?;
        }
        None => {
            write!(
                stream,
                "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
            )?;
        }
    }
    stream.flush()?;
    let _ = stream.shutdown(std::net::Shutdown::Both);
    Ok(())
}

/// A throwaway minisign key pair.
pub struct Key {
    pair: minisign::KeyPair,
}

impl Key {
    pub fn generate() -> Self {
        Self {
            pair: minisign::KeyPair::generate_unencrypted_keypair().expect("generate key"),
        }
    }

    pub fn public_text(&self) -> String {
        self.pair.pk.to_box().expect("public key box").into_string()
    }

    pub fn trusted(&self) -> TrustedKeys {
        TrustedKeys::new(vec![
            TrustedKey::parse("test.pub", &self.public_text()).expect("parse test key"),
        ])
    }

    pub fn sign(&self, bytes: &[u8], trusted_comment: &str) -> String {
        minisign::sign(
            Some(&self.pair.pk),
            &self.pair.sk,
            Cursor::new(bytes),
            Some(trusted_comment),
            None,
        )
        .expect("sign")
        .into_string()
    }
}

/// One archive entry for [`tar_bytes`] and [`zip_bytes`].
pub enum Entry {
    File(&'static str, Vec<u8>, u32),
    Dir(&'static str),
    Symlink(&'static str, &'static str),
    HardLink(&'static str, &'static str),
    CharDevice(&'static str),
    Fifo(&'static str),
}

/// Builds a tar archive with raw names, so malicious paths reach the extractor unchanged.
pub fn tar_bytes(entries: &[Entry]) -> Vec<u8> {
    let mut builder = tar::Builder::new(Vec::new());
    for entry in entries {
        let mut header = tar::Header::new_ustar();
        let (name, link, kind, data, mode): (&str, Option<&str>, tar::EntryType, &[u8], u32) =
            match entry {
                Entry::File(name, data, mode) => {
                    (name, None, tar::EntryType::Regular, data.as_slice(), *mode)
                }
                Entry::Dir(name) => (name, None, tar::EntryType::Directory, &[], 0o755),
                Entry::Symlink(name, target) => {
                    (name, Some(target), tar::EntryType::Symlink, &[], 0o777)
                }
                Entry::HardLink(name, target) => {
                    (name, Some(target), tar::EntryType::Link, &[], 0o644)
                }
                Entry::CharDevice(name) => (name, None, tar::EntryType::Char, &[], 0o644),
                Entry::Fifo(name) => (name, None, tar::EntryType::Fifo, &[], 0o644),
            };
        let raw = header.as_old_mut();
        raw.name = [0; 100];
        raw.name[..name.len()].copy_from_slice(name.as_bytes());
        if let Some(link) = link {
            raw.linkname = [0; 100];
            raw.linkname[..link.len()].copy_from_slice(link.as_bytes());
        }
        header.set_entry_type(kind);
        header.set_mode(mode);
        header.set_size(data.len() as u64);
        header.set_mtime(1_700_000_000);
        header.set_cksum();
        builder.append(&header, data).expect("append tar entry");
    }
    builder.into_inner().expect("finish tar")
}

pub fn zstd(bytes: &[u8]) -> Vec<u8> {
    ruzstd::encoding::compress_to_vec(bytes, ruzstd::encoding::CompressionLevel::Fastest)
}

pub fn gzip(bytes: &[u8]) -> Vec<u8> {
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
    encoder.write_all(bytes).unwrap();
    encoder.finish().unwrap()
}

pub fn zip_bytes(entries: &[Entry]) -> Vec<u8> {
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for entry in entries {
        let options = |mode: u32| {
            zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Stored)
                .unix_permissions(mode)
        };
        match entry {
            Entry::File(name, data, mode) => {
                writer.start_file(*name, options(*mode)).unwrap();
                writer.write_all(data).unwrap();
            }
            Entry::Dir(name) => writer.add_directory(*name, options(0o755)).unwrap(),
            Entry::Symlink(name, target) => {
                writer.add_symlink(*name, *target, options(0o777)).unwrap();
            }
            _ => panic!("unsupported zip entry"),
        }
    }
    writer.finish().unwrap().into_inner()
}

/// A pack under construction: component files are written to `dir`, the manifest is generated
/// by the same code `ashift __pack manifest` uses, then signed.
pub struct PackBuilder {
    pub name: String,
    pub version: String,
    pub triple: String,
    pub entrypoint: Vec<String>,
    pub requires: String,
    /// `engine_protocol` written into the manifest; defaults to what the host registers for an
    /// engine of this name, so packs for registered engines are valid.
    pub engine_protocol: Option<String>,
    pub notices: Vec<String>,
    pub env: Vec<(String, String)>,
    pub options: Vec<(String, String)>,
    pub components: Vec<(String, String, Vec<u8>, String)>,
}

impl PackBuilder {
    pub fn new(name: &str, version: &str) -> Self {
        Self {
            name: name.to_owned(),
            version: version.to_owned(),
            triple: packs::host_targets()[0].clone(),
            entrypoint: vec!["bin/engine".to_owned()],
            requires: ">=0.1.0".to_owned(),
            engine_protocol: ariad_host::engines::pack_spec(name)
                .filter(|spec| spec.protocol == ariad_host::engines::Protocol::V2)
                .map(|spec| spec.protocol.as_str().to_owned()),
            notices: vec!["LICENSE.txt".to_owned()],
            env: vec![("HF_HUB_OFFLINE".to_owned(), "1".to_owned())],
            options: vec![("artifacts_path".to_owned(), "{root}/models".to_owned())],
            components: Vec::new(),
        }
    }

    /// Overrides the `engine_protocol` written into the manifest.
    pub fn engine_protocol(mut self, value: Option<&str>) -> Self {
        self.engine_protocol = value.map(str::to_owned);
        self
    }

    /// Adds a component file named `file`, unpacked into `dest`.
    pub fn component(mut self, name: &str, file: &str, bytes: Vec<u8>, dest: &str) -> Self {
        self.components
            .push((name.to_owned(), file.to_owned(), bytes, dest.to_owned()));
        self
    }

    /// The usual valid pack: a tar.zst runtime with the engine, a notice and a model file.
    pub fn standard(name: &str, version: &str) -> Self {
        let runtime = zstd(&tar_bytes(&[
            Entry::Dir("bin/"),
            Entry::File("bin/engine", b"#!/bin/sh\nexit 0\n".to_vec(), 0o755),
            Entry::File("LICENSE.txt", b"MIT licence text\n".to_vec(), 0o644),
            Entry::File("models/weights.bin", vec![7_u8; 4096], 0o644),
        ]));
        Self::new(name, version).component("runtime", "runtime.tar.zst", runtime, ".")
    }

    /// Writes the component files and an unsigned `pack.toml` into `dir`; returns the text.
    pub fn write_unsigned(&self, dir: &Path) -> String {
        fs::create_dir_all(dir).unwrap();
        let mut files = Vec::new();
        let mut dests = Vec::new();
        for (name, file, bytes, dest) in &self.components {
            let path = dir.join(file);
            fs::write(&path, bytes).unwrap();
            #[cfg(unix)]
            if !file.contains('.') {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
            }
            files.push((Some(name.clone()), path));
            dests.push((name.clone(), dest.clone()));
        }
        // A pack that declares `engine_protocol` is generated from a template, like the release
        // pipeline does with `ashift __pack manifest --base`.
        let template = tempfile::tempdir().unwrap();
        let base = self.engine_protocol.as_ref().map(|protocol| {
            let path = template.path().join("base.toml");
            fs::write(
                &path,
                format!(
                    "schema = \"{}\"\nname = \"{}\"\nversion = \"{}\"\nlicense = \"MIT\"\n\
                     requires_ashift = \"{}\"\nengine_protocol = \"{protocol}\"\ntargets = []\n",
                    packs::MANIFEST_SCHEMA,
                    self.name,
                    self.version,
                    self.requires
                ),
            )
            .unwrap();
            path
        });
        let mut manifest = packs::generate_manifest(&ManifestRequest {
            name: self.name.clone(),
            version: self.version.clone(),
            target: self.triple.clone(),
            base,
            engine_protocol: self.engine_protocol.clone(),
            entrypoint: self.entrypoint.clone(),
            url_prefix: None,
            dests,
            components: files,
        })
        .expect("generate manifest");
        manifest.license = "MIT".to_owned();
        manifest.requires_ashift.clone_from(&self.requires);
        manifest.engine_protocol.clone_from(&self.engine_protocol);
        manifest.notices.clone_from(&self.notices);
        let target = &mut manifest.targets[0];
        for (key, value) in &self.env {
            target.env.insert(key.clone(), value.clone());
        }
        for (key, value) in &self.options {
            target
                .options
                .insert(key.clone(), serde_json::Value::String(value.clone()));
        }
        let text = manifest.to_toml().expect("manifest to TOML");
        fs::write(dir.join(packs::MANIFEST_FILE), &text).unwrap();
        text
    }

    /// Writes components and a signed manifest without the generator's dry-run scan, which
    /// would refuse unsafe archives before they reach the installer. Archives declare 1 MiB and
    /// 64 entries.
    pub fn write_unscanned(&self, dir: &Path, key: &Key) {
        use sha2::{Digest, Sha256};
        fs::create_dir_all(dir).unwrap();
        let components = self
            .components
            .iter()
            .map(|(name, file, bytes, dest)| {
                fs::write(dir.join(file), bytes).unwrap();
                let unpack = packs::Unpack::from_file_name(file);
                packs::Component {
                    name: name.clone(),
                    url: file.clone(),
                    sha256: hex::encode(Sha256::digest(bytes)),
                    size: bytes.len() as u64,
                    unpack,
                    dest: dest.clone(),
                    unpacked_size: unpack.is_archive().then_some(1 << 20),
                    entries: unpack.is_archive().then_some(64),
                    executable: false,
                }
            })
            .collect();
        let manifest = packs::Manifest {
            schema: packs::MANIFEST_SCHEMA.to_owned(),
            name: self.name.clone(),
            version: self.version.clone(),
            description: String::new(),
            license: "MIT".to_owned(),
            requires_ashift: self.requires.clone(),
            engine_protocol: self.engine_protocol.clone(),
            notices: Vec::new(),
            targets: vec![packs::Target {
                triple: self.triple.clone(),
                entrypoint: self.entrypoint.clone(),
                env: Default::default(),
                options: Default::default(),
                components,
            }],
        };
        fs::write(
            dir.join(packs::MANIFEST_FILE),
            manifest.to_toml().expect("manifest to TOML"),
        )
        .unwrap();
        sign_into(
            dir,
            key,
            &format!("pack:{} version:{}", self.name, self.version),
        );
    }

    /// Writes the pack into `dir` and signs it with the standard trusted comment.
    pub fn write(&self, dir: &Path, key: &Key) -> String {
        let text = self.write_unsigned(dir);
        let comment = format!("pack:{} version:{}", self.name, self.version);
        sign_into(dir, key, &comment);
        text
    }
}

/// Signs `dir/pack.toml` into `dir/pack.toml.minisig`.
pub fn sign_into(dir: &Path, key: &Key, trusted_comment: &str) {
    let text = fs::read(dir.join(packs::MANIFEST_FILE)).unwrap();
    fs::write(
        dir.join(packs::SIGNATURE_FILE),
        key.sign(&text, trusted_comment),
    )
    .unwrap();
}

/// A context with its own data directory, the given keys and fast retries.
pub fn context(data_dir: &Path, keys: TrustedKeys) -> PackContext {
    PackContext {
        data_dir: data_dir.to_path_buf(),
        keys,
        targets: packs::host_targets(),
        fetch: FetchPolicy {
            attempts: 2,
            backoff: Duration::from_millis(10),
            connect_timeout: Duration::from_secs(5),
            read_timeout: Duration::from_secs(5),
        },
    }
}

/// Every file below `dir`, relative, sorted; empty when `dir` is missing.
pub fn tree(dir: &Path) -> Vec<String> {
    fn walk(base: &Path, dir: &Path, out: &mut Vec<String>) {
        let Ok(entries) = fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let relative = path
                .strip_prefix(base)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            if entry.file_type().unwrap().is_dir() {
                walk(base, &path, out);
            } else {
                out.push(relative);
            }
        }
    }
    let mut out = Vec::new();
    walk(dir, dir, &mut out);
    out.sort();
    out
}

pub fn temp() -> tempfile::TempDir {
    tempfile::tempdir().expect("tempdir")
}

pub fn path_in(dir: &Path, relative: &str) -> PathBuf {
    relative
        .split('/')
        .fold(dir.to_path_buf(), |p, part| p.join(part))
}
