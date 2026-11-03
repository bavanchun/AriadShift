//! Explicit, hash-pinned extra Tesseract language downloads in the pack cache.
use super::{
    PackContext, PackError,
    fetch::{self, Fetcher, Location},
};
use serde::Serialize;
use std::{
    fs,
    path::{Path, PathBuf},
};
use tokio_util::sync::CancellationToken;

mod catalogue;

/// One language in the pinned catalogue and whether its verified cache entry exists.
#[derive(Clone, Debug, Serialize)]
pub struct LanguageStatus {
    pub language: &'static str,
    pub installed: bool,
    pub size: u64,
}

fn entry(language: &str) -> Result<(&'static str, &'static str, u64), PackError> {
    catalogue::LANGUAGES
        .iter()
        .copied()
        .find(|item| item.0 == language)
        .ok_or_else(|| PackError::Usage(format!("unknown OCR language '{language}'")))
}

fn directory(context: &PackContext) -> PathBuf {
    context
        .packs_dir()
        .join(".cache")
        .join("tesseract-languages")
        .join(catalogue::REVISION)
}

fn verified(
    path: &Path,
    digest: &str,
    size: u64,
    cancel: &CancellationToken,
) -> Result<bool, PackError> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(PackError::io("could not inspect", path, error)),
    };
    if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() != size {
        return Ok(false);
    }
    let (actual, actual_size) = fetch::sha256_file(path, cancel)?;
    Ok(actual == digest && actual_size == size)
}

/// List catalogue entries. A changed cache entry is reported as not installed.
pub fn list(
    context: &PackContext,
    cancel: &CancellationToken,
) -> Result<Vec<LanguageStatus>, PackError> {
    catalogue::LANGUAGES
        .iter()
        .map(|&(language, digest, size)| {
            let path = directory(context).join(format!("{language}.traineddata"));
            Ok(LanguageStatus {
                language,
                installed: verified(&path, digest, size, cancel)?,
                size,
            })
        })
        .collect()
}

/// Download a selected language and atomically publish it only after its bytes match the pin.
pub fn install(
    context: &PackContext,
    language: &str,
    cancel: &CancellationToken,
) -> Result<LanguageStatus, PackError> {
    install_from(
        context,
        language,
        cancel,
        "https://raw.githubusercontent.com/tesseract-ocr/tessdata_best",
    )
}

fn install_from(
    context: &PackContext,
    language: &str,
    cancel: &CancellationToken,
    base: &str,
) -> Result<LanguageStatus, PackError> {
    let (language, digest, size) = entry(language)?;
    let _lock = super::install::tesseract_lock(context)?;
    let dir = directory(context);
    fs::create_dir_all(&dir).map_err(|error| PackError::io("could not create", &dir, error))?;
    let destination = dir.join(format!("{language}.traineddata"));
    if verified(&destination, digest, size, cancel)? {
        return Ok(LanguageStatus {
            language,
            installed: true,
            size,
        });
    }
    let mut temporary = tempfile::NamedTempFile::new_in(&dir)
        .map_err(|error| PackError::io("could not create", &dir, error))?;
    let temporary_path = temporary.path().to_owned();
    let url = format!(
        "{}/{}/{language}.traineddata",
        base.trim_end_matches('/'),
        catalogue::REVISION
    );
    let fetcher = Fetcher::new(context.fetch.clone());
    fetcher.fetch_verified(
        &Location::parse(&url)?,
        temporary.as_file_mut(),
        &temporary_path,
        language,
        size,
        digest,
        cancel,
        &mut |_| {},
    )?;
    temporary
        .as_file_mut()
        .sync_all()
        .map_err(|error| PackError::io("could not sync", &temporary_path, error))?;
    temporary
        .persist(&destination)
        .map_err(|error| PackError::io("could not publish", &destination, error.error))?;
    Ok(LanguageStatus {
        language,
        installed: true,
        size,
    })
}

/// Remove a selected cached language without touching the signed engine pack.
pub fn remove(context: &PackContext, language: &str) -> Result<(), PackError> {
    let (language, _, _) = entry(language)?;
    let _lock = super::install::tesseract_lock(context)?;
    let path = directory(context).join(format!("{language}.traineddata"));
    match fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(PackError::io("could not remove", &path, error)),
    }
}

/// Return a cached model only after rechecking its pinned digest and size.
pub(crate) fn verified_path(
    context: &PackContext,
    language: &str,
    cancel: &CancellationToken,
) -> Result<Option<PathBuf>, PackError> {
    let (language, digest, size) = entry(language)?;
    let path = directory(context).join(format!("{language}.traineddata"));
    if verified(&path, digest, size, cancel)? {
        Ok(Some(path))
    } else {
        Ok(None)
    }
}

pub(crate) fn verify_model(path: &Path, language: &str) -> Result<bool, PackError> {
    let (_, digest, size) = entry(language)?;
    verified(path, digest, size, &CancellationToken::new())
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};

    #[test]
    fn install_rejects_tampered_download_without_publishing() {
        use std::io::{Read, Write};

        let temp = tempfile::tempdir().unwrap();
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0; 1024];
            let n = stream.read(&mut request).unwrap();
            assert!(String::from_utf8_lossy(&request[..n]).contains("fra.traineddata"));
            stream
                .write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Length: 8\r\nConnection: close\r\n\r\ntampered",
                )
                .unwrap();
        });
        let context = PackContext {
            data_dir: temp.path().join("cache"),
            keys: super::super::TrustedKeys::compiled(),
            targets: vec![],
            fetch: super::super::FetchPolicy::default(),
        };
        let error = install_from(&context, "fra", &CancellationToken::new(), &base).unwrap_err();
        server.join().unwrap();
        assert!(matches!(
            error,
            PackError::SizeMismatch { .. } | PackError::HashMismatch { .. }
        ));
        assert!(!directory(&context).join("fra.traineddata").exists());
    }

    #[test]
    fn verification_detects_changed_bytes_and_rejects_links() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("model");
        let cancel = CancellationToken::new();
        let digest = hex::encode(Sha256::digest(b"model"));
        assert!(!verified(&path, &digest, 5, &cancel).unwrap());
        fs::write(&path, b"model").unwrap();
        assert!(verified(&path, &digest, 5, &cancel).unwrap());
        fs::write(&path, b"other").unwrap();
        assert!(!verified(&path, &digest, 5, &cancel).unwrap());
        #[cfg(unix)]
        {
            let link = dir.path().join("link");
            std::os::unix::fs::symlink(&path, &link).unwrap();
            assert!(!verified(&link, &digest, 5, &cancel).unwrap());
        }
    }

    #[test]
    fn catalogue_refuses_paths_and_unknown_languages() {
        assert!(entry("../eng").is_err());
        assert!(entry("osd").is_err());
        assert!(entry("eng").is_ok());
        assert!(entry("vie").is_ok());
    }

    #[test]
    fn cached_model_lifecycle_rechecks_and_removes_only_the_selected_file() {
        let Some(root) = std::env::var_os("ARIAD_TEST_TESSERACT_DIR") else {
            assert!(
                std::env::var_os("ARIAD_REQUIRE_OCR_PACKS").is_none(),
                "ARIAD_TEST_TESSERACT_DIR is required"
            );
            eprintln!("skipping: development Tesseract model is absent");
            return;
        };
        let temp = tempfile::tempdir().unwrap();
        let context = PackContext {
            data_dir: temp.path().to_owned(),
            keys: super::super::TrustedKeys::compiled(),
            targets: vec![],
            fetch: super::super::FetchPolicy::default(),
        };
        let dir = directory(&context);
        fs::create_dir_all(&dir).unwrap();
        let model = dir.join("eng.traineddata");
        fs::copy(PathBuf::from(root).join("tessdata/eng.traineddata"), &model).unwrap();
        let unrelated = dir.join("unrelated.txt");
        fs::write(&unrelated, "keep").unwrap();
        let cancel = CancellationToken::new();
        assert!(verified_path(&context, "eng", &cancel).unwrap().is_some());
        assert!(
            list(&context, &cancel)
                .unwrap()
                .into_iter()
                .find(|s| s.language == "eng")
                .unwrap()
                .installed
        );
        fs::write(&model, b"modified").unwrap();
        assert!(verified_path(&context, "eng", &cancel).unwrap().is_none());
        remove(&context, "eng").unwrap();
        assert!(!model.exists());
        assert_eq!(fs::read_to_string(unrelated).unwrap(), "keep");
    }
}
