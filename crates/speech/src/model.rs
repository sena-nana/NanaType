//! Only the pinned, multilingual base Q5_1 model is accepted, including imports.
use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

pub const MODEL_NAME: &str = "ggml-base-q5_1.bin";
pub const MODEL_BYTES: u64 = 59_707_625;
pub const MODEL_SHA256: &str = "422f1ae452ade6f30a004d7e5c6a43195e4433bc370bf23fac9cc591f01a8898";
const URL: &str = "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base-q5_1.bin";

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum ModelState {
    #[default]
    Missing,
    Working(u8),
    Ready,
    Error(String),
}

pub fn path(directory: &Path) -> PathBuf {
    directory.join(MODEL_NAME)
}

pub fn verify(file: &Path) -> Result<(), String> {
    if file.metadata().map_err(|e| e.to_string())?.len() != MODEL_BYTES {
        return Err("speech-model-invalid".into());
    }
    copy_checked(
        std::fs::File::open(file).map_err(|e| e.to_string())?,
        std::io::sink(),
        &AtomicBool::new(false),
        |_| {},
    )
}

pub fn install(
    directory: &Path,
    source: Option<&Path>,
    cancelled: &AtomicBool,
    progress: impl Fn(u8),
) -> Result<(), String> {
    std::fs::create_dir_all(directory).map_err(|e| e.to_string())?;
    let staging = directory.join(format!("{MODEL_NAME}.part"));
    let result = (|| {
        let input: Box<dyn Read> = if let Some(source) = source {
            Box::new(std::fs::File::open(source).map_err(|e| e.to_string())?)
        } else {
            let agent = ureq::Agent::config_builder()
                .timeout_global(Some(std::time::Duration::from_secs(600)))
                .timeout_connect(Some(std::time::Duration::from_secs(10)))
                .timeout_resolve(Some(std::time::Duration::from_secs(10)))
                .timeout_recv_response(Some(std::time::Duration::from_secs(10)))
                .build()
                .new_agent();
            let response = agent.get(URL).call().map_err(|e| e.to_string())?;
            Box::new(response.into_body().into_reader())
        };
        let mut output = std::fs::File::create(&staging).map_err(|e| e.to_string())?;
        copy_checked(input, &mut output, cancelled, progress)?;
        output.sync_all().map_err(|e| e.to_string())?;
        drop(output);
        if cancelled.load(Ordering::Acquire) {
            return Err("speech-cancelled".into());
        }
        // Remove is unnecessary on Unix, but Windows rename cannot replace a file.
        let destination = path(directory);
        if destination.exists() {
            std::fs::remove_file(&destination).map_err(|e| e.to_string())?;
        }
        std::fs::rename(&staging, destination).map_err(|e| e.to_string())
    })();
    let _ = std::fs::remove_file(staging);
    result
}

fn copy_checked(
    mut input: impl Read,
    mut output: impl Write,
    cancelled: &AtomicBool,
    progress: impl Fn(u8),
) -> Result<(), String> {
    let mut hash = Sha256::new();
    let mut copied = 0;
    let mut reported = 101;
    let mut buffer = [0; 65536];
    loop {
        if cancelled.load(Ordering::Acquire) {
            return Err("speech-cancelled".into());
        }
        let n = input.read(&mut buffer).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        copied += n as u64;
        if copied > MODEL_BYTES {
            return Err("speech-model-invalid".into());
        }
        output.write_all(&buffer[..n]).map_err(|e| e.to_string())?;
        hash.update(&buffer[..n]);
        let percent = (copied * 100 / MODEL_BYTES) as u8;
        if percent != reported {
            progress(percent);
            reported = percent;
        }
    }
    if copied != MODEL_BYTES || format!("{:x}", hash.finalize()) != MODEL_SHA256 {
        return Err("speech-model-invalid".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn corrupt_model_does_not_replace_an_existing_install() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("corrupt.bin");
        std::fs::File::create(&source)
            .unwrap()
            .set_len(MODEL_BYTES)
            .unwrap();
        let installed = path(directory.path());
        std::fs::write(&installed, b"existing model").unwrap();
        assert!(
            install(
                directory.path(),
                Some(&source),
                &AtomicBool::new(false),
                |_| {}
            )
            .is_err()
        );
        assert_eq!(std::fs::read(installed).unwrap(), b"existing model");
        assert!(!directory.path().join(format!("{MODEL_NAME}.part")).exists());
    }
    #[test]
    #[ignore = "downloads the 60 MB pinned model"]
    fn download_and_import_pinned_model() {
        let download = tempfile::tempdir().unwrap();
        let imported = tempfile::tempdir().unwrap();
        let cancel = AtomicBool::new(false);
        let start = std::time::Instant::now();
        install(download.path(), None, &cancel, |_| {}).unwrap();
        verify(&path(download.path())).unwrap();
        install(
            imported.path(),
            Some(&path(download.path())),
            &cancel,
            |_| {},
        )
        .unwrap();
        verify(&path(imported.path())).unwrap();
        println!(
            "download, import and verify: {:.2}s",
            start.elapsed().as_secs_f64()
        );
    }
    #[test]
    fn truncated_model_and_cancelled_download_are_rejected() {
        let d = tempfile::tempdir().unwrap();
        let f = d.path().join("bad");
        std::fs::write(&f, b"wrong").unwrap();
        assert!(verify(&f).is_err());
        let cancel = AtomicBool::new(true);
        assert!(install(d.path(), Some(&f), &cancel, |_| {}).is_err());
        assert!(!path(d.path()).exists());
        assert!(!d.path().join(format!("{MODEL_NAME}.part")).exists());
    }
}
