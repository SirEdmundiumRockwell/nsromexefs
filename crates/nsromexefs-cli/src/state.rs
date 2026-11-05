//! Manifest final y journal de reanudación por archivo.

use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use nsromexefs_core::{parse_sha256sums, Error, ResumeFile};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const JOURNAL_NAME: &str = ".nsromexefs-journal";
const JOURNAL_BACKUP: &str = ".nsromexefs-journal.bak";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct Journal {
    pub schema: u32,
    pub source_fingerprint: String,
    pub plan_hash: String,
    pub completed: BTreeMap<String, ResumeFile>,
    pub bytes_written: u64,
}

pub(crate) struct OutputSession {
    root: PathBuf,
    journal: Journal,
    last_persisted_count: usize,
    last_persisted_bytes: u64,
}

impl OutputSession {
    /// Crea o continúa un journal. En `--resume`, los archivos existentes solo
    /// se omiten si su hash se valida en core antes de devolverlos.
    pub fn begin(
        root: &Path,
        resume: bool,
        force: bool,
        source_fingerprint: String,
        plan_hash: String,
    ) -> Result<Self, Error> {
        fs::create_dir_all(root)?;
        let path = root.join(JOURNAL_NAME);
        let backup = root.join(JOURNAL_BACKUP);
        let journal_present = path.exists() || backup.exists();
        let previous = if resume && journal_present {
            match read_journal(root) {
                Ok(journal) => Some(journal),
                Err(_error) if force => None,
                Err(error) => return Err(error),
            }
        } else {
            None
        };

        let journal = match previous {
            Some(journal)
                if journal.schema == 1
                    && journal.source_fingerprint == source_fingerprint
                    && journal.plan_hash == plan_hash =>
            {
                journal
            }
            Some(_) if !force => {
                return Err(Error::Invalid(
                    "the journal does not match the current sources or plan; use --force to start over".into(),
                ));
            }
            _ => Journal {
                schema: 1,
                source_fingerprint,
                plan_hash,
                completed: BTreeMap::new(),
                bytes_written: 0,
            },
        };

        let mut session = Self {
            root: root.to_path_buf(),
            last_persisted_count: journal.completed.len(),
            last_persisted_bytes: journal.bytes_written,
            journal,
        };

        let stale_temp = root.join(format!(".{}.part", JOURNAL_NAME));
        if stale_temp.exists() {
            fs::remove_file(stale_temp)?;
        }
        if resume {
            cleanup_partial_files(root)?;
        }

        // Si no quedó journal (por ejemplo la interrupción ocurrió justo
        // después de completar un archivo), SHA256SUMS permite sembrar hashes
        // candidatos. Core vuelve a verificar tamaño y hash antes de saltarlos.
        if resume && !path.exists() && session.journal.completed.is_empty() {
            session.seed_from_checksums();
        }
        session.persist()?;
        Ok(session)
    }

    pub fn resume_files(&self) -> BTreeMap<String, ResumeFile> {
        self.journal.completed.clone()
    }

    pub fn checkpoint(&mut self, relative: &str, size: u64, sha256: &str) -> Result<(), Error> {
        let key = relative.replace('\\', "/");
        if let Some(previous) = self.journal.completed.insert(
            key,
            ResumeFile {
                size,
                sha256: sha256.to_ascii_lowercase(),
            },
        ) {
            self.journal.bytes_written = self.journal.bytes_written.saturating_sub(previous.size);
        }
        self.journal.bytes_written = self.journal.bytes_written.saturating_add(size);
        let completed = self.journal.completed.len();
        let new_bytes = self
            .journal
            .bytes_written
            .saturating_sub(self.last_persisted_bytes);
        if self.last_persisted_count == 0
            || completed.saturating_sub(self.last_persisted_count) >= 32
            || new_bytes >= 256 * 1024 * 1024
        {
            self.persist()?;
        }
        Ok(())
    }

    pub fn finish(self) -> Result<(), Error> {
        let path = self.root.join(JOURNAL_NAME);
        if path.exists() {
            fs::remove_file(path)?;
        }
        let backup = self.root.join(JOURNAL_BACKUP);
        if backup.exists() {
            fs::remove_file(backup)?;
        }
        Ok(())
    }

    fn seed_from_checksums(&mut self) {
        let path = self.root.join("SHA256SUMS");
        let Ok(text) = fs::read_to_string(path) else {
            return;
        };
        let Ok(entries) = parse_sha256sums(&text) else {
            return;
        };
        for entry in entries {
            let file = self
                .root
                .join(entry.path.replace('/', std::path::MAIN_SEPARATOR_STR));
            let Ok(metadata) = fs::symlink_metadata(file) else {
                continue;
            };
            if metadata.file_type().is_file() {
                self.journal.completed.insert(
                    entry.path,
                    ResumeFile {
                        size: metadata.len(),
                        sha256: entry.sha256,
                    },
                );
            }
        }
        self.journal.bytes_written = self
            .journal
            .completed
            .values()
            .map(|entry| entry.size)
            .fold(0u64, u64::saturating_add);
    }

    fn persist(&mut self) -> Result<(), Error> {
        persist_json(&self.root.join(JOURNAL_NAME), &self.journal)?;
        self.last_persisted_count = self.journal.completed.len();
        self.last_persisted_bytes = self.journal.bytes_written;
        Ok(())
    }
}

pub(crate) fn source_fingerprint(paths: &[PathBuf]) -> Result<String, Error> {
    let mut sources = paths
        .iter()
        .map(|path| {
            let metadata = fs::metadata(path)?;
            let modified = metadata
                .modified()
                .ok()
                .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
                .map_or(0, |duration| duration.as_millis());
            Ok(format!(
                "{}\0{}\0{}",
                path.canonicalize()?.display(),
                metadata.len(),
                modified
            ))
        })
        .collect::<Result<Vec<_>, std::io::Error>>()?;
    sources.sort();
    Ok(hash_hex(sources.join("\n").as_bytes()))
}

pub(crate) fn plan_hash(identity: &serde_json::Value) -> String {
    hash_hex(&serde_json::to_vec(identity).unwrap_or_default())
}

pub(crate) fn timestamp_utc() -> String {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
    let days = seconds.div_euclid(86_400);
    let day_seconds = seconds.rem_euclid(86_400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let mut year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = mp + if mp < 10 { 3 } else { -9 };
    year += i64::from(month <= 2);
    let hour = day_seconds / 3_600;
    let minute = (day_seconds % 3_600) / 60;
    let second = day_seconds % 60;
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
}

pub(crate) fn atomic_json<T: Serialize>(path: &Path, value: &T) -> Result<(), Error> {
    persist_json(path, value)
}

fn persist_json<T: Serialize>(path: &Path, value: &T) -> Result<(), Error> {
    let bytes = serde_json::to_vec_pretty(value)
        .map_err(|err| Error::Invalid(format!("cannot serialize state: {err}")))?;
    let temp = path.with_file_name(format!(
        ".{}.part",
        path.file_name().unwrap_or_default().to_string_lossy()
    ));
    let backup = path.with_file_name(format!(
        "{}.bak",
        path.file_name().unwrap_or_default().to_string_lossy()
    ));
    {
        let mut file = File::create(&temp)?;
        file.write_all(&bytes)?;
        file.write_all(b"\n")?;
        file.sync_all()?;
    }
    if path.exists() {
        if backup.exists() {
            fs::remove_file(&backup)?;
        }
        fs::rename(path, &backup)?;
    }
    if let Err(err) = fs::rename(&temp, path) {
        if backup.exists() && !path.exists() {
            let _ = fs::rename(&backup, path);
        }
        return Err(err.into());
    }
    if backup.exists() {
        fs::remove_file(backup)?;
    }
    Ok(())
}

fn read_journal(root: &Path) -> Result<Journal, Error> {
    let path = root.join(JOURNAL_NAME);
    let backup = root.join(JOURNAL_BACKUP);
    for candidate in [&path, &backup] {
        if let Ok(bytes) = fs::read(candidate) {
            if let Ok(journal) = serde_json::from_slice(&bytes) {
                return Ok(journal);
            }
        }
    }
    Err(Error::Invalid("journal missing or unreadable".into()))
}

fn hash_hex(bytes: &[u8]) -> String {
    let hash: [u8; 32] = Sha256::digest(bytes).into();
    hash.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn cleanup_partial_files(root: &Path) -> Result<(), Error> {
    let mut stack = vec![root.to_path_buf()];
    while let Some(current) = stack.pop() {
        let entries = fs::read_dir(current)?.collect::<std::result::Result<Vec<_>, _>>()?;
        for entry in entries {
            let kind = entry.file_type()?;
            if kind.is_dir() {
                stack.push(entry.path());
            } else if kind.is_file() && entry.file_name().to_string_lossy().ends_with(".part") {
                fs::remove_file(entry.path())?;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fingerprint_changes_with_source_metadata() {
        let path =
            std::env::temp_dir().join(format!("nsromexefs-state-{}.nsp", std::process::id()));
        fs::write(&path, b"one").unwrap();
        let first = source_fingerprint(std::slice::from_ref(&path)).unwrap();
        fs::write(&path, b"two-two").unwrap();
        let second = source_fingerprint(std::slice::from_ref(&path)).unwrap();
        assert_ne!(first, second);
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn timestamp_is_rfc3339_utc_shape() {
        let stamp = timestamp_utc();
        assert_eq!(stamp.len(), 20);
        assert!(stamp.ends_with('Z'));
        assert_eq!(&stamp[4..5], "-");
        assert_eq!(&stamp[10..11], "T");
    }

    #[test]
    fn journal_roundtrip_and_plan_mismatch() {
        let root =
            std::env::temp_dir().join(format!("nsromexefs-journal-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let mut session =
            OutputSession::begin(&root, false, false, "source".into(), "plan".into()).unwrap();
        session
            .checkpoint("exefs/main", 4, &"0".repeat(64))
            .unwrap();
        let resumed =
            OutputSession::begin(&root, true, false, "source".into(), "plan".into()).unwrap();
        assert_eq!(resumed.resume_files().len(), 1);
        assert!(OutputSession::begin(&root, true, false, "other".into(), "plan".into()).is_err());
        resumed.finish().unwrap();
        fs::remove_dir_all(root).unwrap();
    }
}
