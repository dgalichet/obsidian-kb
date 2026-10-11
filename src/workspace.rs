//! Transient Obsidian workspace identity, kept separately from the search index.

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const CONTEXT_TTL: Duration = Duration::from_secs(90);
const MAX_SESSIONS: usize = 64;
const BRIDGE_TIMEOUT: Duration = Duration::from_secs(2);
const MAX_REPLY_BYTES: usize = 64 * 1024;
const MAX_HEADERS_BYTES: usize = 16 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct NoteIdentity {
    pub path: String,
    pub title: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ContextUpdate {
    pub vault_path: PathBuf,
    pub session_id: String,
    pub context_version: u64,
    #[serde(deserialize_with = "required_note")]
    pub note: Option<NoteIdentity>,
}

fn required_note<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> std::result::Result<Option<NoteIdentity>, D::Error> {
    Option::deserialize(deserializer)
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CurrentNoteRequest {
    pub vault_path: PathBuf,
    pub session_id: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ContextDelete {
    pub vault_path: PathBuf,
    pub session_id: String,
}

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum WorkspaceStatus {
    Available,
    NoActiveNote,
    Unavailable,
    Stale,
    Ambiguous,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CurrentNoteReply {
    pub status: WorkspaceStatus,
    pub vault_path: PathBuf,
    pub note: Option<NoteIdentity>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_version: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub age_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub candidates: Option<Vec<SessionCandidate>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SessionCandidate {
    pub session_id: String,
    pub context_version: u64,
    pub age_ms: u64,
}

impl CurrentNoteReply {
    fn empty(vault: &Path, status: WorkspaceStatus) -> Self {
        Self {
            status,
            vault_path: vault.to_path_buf(),
            note: None,
            session_id: None,
            context_version: None,
            age_ms: None,
            candidates: None,
            reason: None,
        }
    }
}

struct SessionContext {
    note: Option<NoteIdentity>,
    version: u64,
    refreshed_at: Instant,
}

#[derive(Default)]
pub(crate) struct WorkspaceContexts {
    sessions: BTreeMap<String, SessionContext>,
}

impl WorkspaceContexts {
    pub(crate) fn update(
        &mut self,
        vault: &Path,
        update: ContextUpdate,
    ) -> Result<CurrentNoteReply> {
        self.update_at(vault, update, Instant::now())
    }

    fn update_at(
        &mut self,
        vault: &Path,
        update: ContextUpdate,
        now: Instant,
    ) -> Result<CurrentNoteReply> {
        let vault = matching_vault(vault, &update.vault_path)?;
        validate_session_id(&update.session_id)?;
        if update.context_version == 0 {
            bail!("context_version must be a positive integer");
        }
        if let Some(note) = &update.note {
            validate_note(&vault, note)?;
        }
        if let Some(previous) = self.sessions.get(&update.session_id) {
            if update.context_version < previous.version {
                bail!("context_version is older than the accepted context");
            }
            if update.context_version == previous.version && update.note != previous.note {
                bail!("context_version already identifies a different note context");
            }
        } else if self.sessions.len() >= MAX_SESSIONS {
            let oldest_stale = self
                .sessions
                .iter()
                .filter(|(_, context)| now.duration_since(context.refreshed_at) >= CONTEXT_TTL)
                .min_by_key(|(_, context)| context.refreshed_at)
                .map(|(id, _)| id.clone());
            if let Some(id) = oldest_stale {
                self.sessions.remove(&id);
            } else {
                bail!("workspace session limit reached ({MAX_SESSIONS})");
            }
        }
        let session_id = update.session_id;
        self.sessions.insert(
            session_id.clone(),
            SessionContext {
                note: update.note,
                version: update.context_version,
                refreshed_at: now,
            },
        );
        self.current_at(
            &vault,
            CurrentNoteRequest {
                vault_path: vault.clone(),
                session_id: Some(session_id),
            },
            now,
        )
    }

    pub(crate) fn delete(&mut self, vault: &Path, request: ContextDelete) -> Result<bool> {
        matching_vault(vault, &request.vault_path)?;
        validate_session_id(&request.session_id)?;
        Ok(self.sessions.remove(&request.session_id).is_some())
    }

    pub(crate) fn current(
        &self,
        vault: &Path,
        request: CurrentNoteRequest,
    ) -> Result<CurrentNoteReply> {
        self.current_at(vault, request, Instant::now())
    }

    fn current_at(
        &self,
        vault: &Path,
        request: CurrentNoteRequest,
        now: Instant,
    ) -> Result<CurrentNoteReply> {
        let vault = matching_vault(vault, &request.vault_path)?;
        if let Some(session_id) = &request.session_id {
            validate_session_id(session_id)?;
        }
        let live = self
            .sessions
            .iter()
            .filter(|(_, context)| now.duration_since(context.refreshed_at) < CONTEXT_TTL)
            .collect::<Vec<_>>();
        let selected = if let Some(id) = &request.session_id {
            self.sessions.get_key_value(id)
        } else if live.len() > 1 {
            let mut reply = CurrentNoteReply::empty(&vault, WorkspaceStatus::Ambiguous);
            reply.candidates = Some(
                live.into_iter()
                    .map(|(id, context)| SessionCandidate {
                        session_id: id.clone(),
                        context_version: context.version,
                        age_ms: age_ms(now, context.refreshed_at),
                    })
                    .collect(),
            );
            return Ok(reply);
        } else if let Some(session) = live.first() {
            Some(*session)
        } else {
            self.sessions
                .iter()
                .max_by_key(|(_, context)| context.refreshed_at)
        };
        let Some((id, context)) = selected else {
            return Ok(CurrentNoteReply::empty(
                &vault,
                WorkspaceStatus::Unavailable,
            ));
        };
        let stale = now.duration_since(context.refreshed_at) >= CONTEXT_TTL;
        let mut reply = CurrentNoteReply::empty(
            &vault,
            if stale {
                WorkspaceStatus::Stale
            } else if context.note.is_some() {
                WorkspaceStatus::Available
            } else {
                WorkspaceStatus::NoActiveNote
            },
        );
        reply.session_id = Some(id.clone());
        reply.context_version = Some(context.version);
        reply.age_ms = Some(age_ms(now, context.refreshed_at));
        if !stale {
            if let Some(note) = &context.note {
                if validate_note(&vault, note).is_ok() {
                    reply.note = Some(note.clone());
                } else {
                    reply.status = WorkspaceStatus::Unavailable;
                    reply.reason =
                        Some("active note is no longer available inside the vault".into());
                }
            }
        }
        Ok(reply)
    }
}

fn age_ms(now: Instant, refreshed: Instant) -> u64 {
    u64::try_from(now.duration_since(refreshed).as_millis()).unwrap_or(u64::MAX)
}

fn matching_vault(expected: &Path, supplied: &Path) -> Result<PathBuf> {
    let expected = expected
        .canonicalize()
        .context("configured vault is unavailable")?;
    let supplied = supplied
        .canonicalize()
        .context("supplied vault_path is unavailable")?;
    if supplied != expected {
        bail!("vault_path does not match this service's configured vault");
    }
    Ok(expected)
}

fn validate_session_id(session_id: &str) -> Result<()> {
    if session_id.is_empty()
        || session_id.len() > 128
        || !session_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"_-:.".contains(&byte))
    {
        bail!("session_id must contain 1 to 128 ASCII letters, digits, _, -, : or .");
    }
    Ok(())
}

fn validate_note(vault: &Path, note: &NoteIdentity) -> Result<()> {
    if note.title.trim().is_empty()
        || note.title.len() > 2048
        || note.title.chars().any(char::is_control)
    {
        bail!(
            "note.title must be a nonempty title without control characters (maximum 2048 bytes)"
        );
    }
    if note.path.is_empty()
        || note.path.len() > 4096
        || note.path.contains('\\')
        || note.path.chars().any(char::is_control)
        || note
            .path
            .split('/')
            .any(|part| matches!(part, "" | "." | ".."))
        || !note.path.to_ascii_lowercase().ends_with(".md")
    {
        bail!("note.path must be a vault-relative Markdown path without traversal");
    }
    let path = Path::new(&note.path);
    if path.is_absolute() {
        bail!("note.path must be relative to the vault");
    }
    let resolved = vault
        .join(path)
        .canonicalize()
        .context("active note does not exist")?;
    if !resolved.starts_with(vault) || !resolved.is_file() {
        bail!("active note must be a file inside the configured vault");
    }
    Ok(())
}

/// Queries only the loopback HTTP service; unavailable or mismatched services fail closed.
pub(crate) fn current_note_via_http(
    vault: &Path,
    port: u16,
    session_id: Option<String>,
) -> Result<CurrentNoteReply> {
    if let Some(id) = &session_id {
        validate_session_id(id)?;
    }
    let vault = vault
        .canonicalize()
        .context("configured vault is unavailable")?;
    let request = CurrentNoteRequest {
        vault_path: vault.clone(),
        session_id,
    };
    let started = Instant::now();
    match bridge_request(port, &request).and_then(|mut reply| {
        matching_vault(&vault, &reply.vault_path)?;
        if let Some(age) = &mut reply.age_ms {
            *age = age.saturating_add(age_ms(Instant::now(), started));
        }
        if let Some(id) = &reply.session_id {
            validate_session_id(id)?;
        }
        if reply.status == WorkspaceStatus::Available {
            let note = reply
                .note
                .as_ref()
                .context("available workspace reply is missing note")?;
            validate_note(&vault, note)?;
            let age = reply
                .age_ms
                .context("available workspace reply is missing age_ms")?;
            if age >= CONTEXT_TTL.as_millis() as u64
                || reply.context_version.unwrap_or(0) == 0
                || reply.session_id.is_none()
            {
                bail!("available workspace reply is stale or missing identity metadata");
            }
        } else if reply.note.is_some() {
            bail!("unavailable workspace reply contains a usable note");
        }
        if let Some(id) = &request.session_id {
            if reply
                .session_id
                .as_ref()
                .is_some_and(|returned| returned != id)
            {
                bail!("workspace reply does not match requested session_id");
            }
        }
        Ok(reply)
    }) {
        Ok(reply) => Ok(reply),
        Err(error) => {
            let mut reply = CurrentNoteReply::empty(&vault, WorkspaceStatus::Unavailable);
            reply.reason = Some(format!("workspace service unavailable: {error}"));
            Ok(reply)
        }
    }
}

fn bridge_request(port: u16, request: &CurrentNoteRequest) -> Result<CurrentNoteReply> {
    let deadline = Instant::now() + BRIDGE_TIMEOUT;
    let address = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
    let mut stream = TcpStream::connect_timeout(&address, BRIDGE_TIMEOUT)?;
    stream.set_write_timeout(Some(remaining(deadline)?))?;
    let body = serde_json::to_vec(request)?;
    write!(
        stream,
        "POST /workspace/current-note HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    )?;
    stream.write_all(&body)?;
    let mut bytes = Vec::new();
    let mut buffer = [0; 4096];
    let (headers_end, length) = loop {
        stream.set_read_timeout(Some(remaining(deadline)?))?;
        let read = stream.read(&mut buffer)?;
        if read == 0 {
            bail!("workspace service closed before sending HTTP headers");
        }
        bytes.extend_from_slice(&buffer[..read]);
        if let Some(end) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
            if end > MAX_HEADERS_BYTES {
                bail!("workspace HTTP headers are too large");
            }
            let headers = std::str::from_utf8(&bytes[..end])?;
            let mut lines = headers.split("\r\n");
            let status = lines.next().context("missing HTTP status")?;
            let mut status_parts = status.split_whitespace();
            if !matches!(status_parts.next(), Some("HTTP/1.0" | "HTTP/1.1"))
                || status_parts.next() != Some("200")
            {
                bail!("workspace service returned {status}");
            }
            let mut length = None;
            for line in lines {
                let (name, value) = line.split_once(':').context("malformed HTTP header")?;
                if name.eq_ignore_ascii_case("transfer-encoding") {
                    bail!("workspace service must use Content-Length");
                }
                if name.eq_ignore_ascii_case("content-length") {
                    if length.is_some() {
                        bail!("duplicate HTTP Content-Length");
                    }
                    length = Some(value.trim().parse::<usize>()?);
                }
            }
            let length = length.context("missing HTTP Content-Length")?;
            if length > MAX_REPLY_BYTES {
                bail!("workspace reply is too large");
            }
            break (end + 4, length);
        }
        if bytes.len() > MAX_HEADERS_BYTES {
            bail!("workspace HTTP headers are too large");
        }
    };
    while bytes.len() < headers_end + length {
        stream.set_read_timeout(Some(remaining(deadline)?))?;
        let wanted = (headers_end + length - bytes.len()).min(buffer.len());
        let read = stream.read(&mut buffer[..wanted])?;
        if read == 0 {
            bail!("workspace service closed before sending its complete reply");
        }
        bytes.extend_from_slice(&buffer[..read]);
    }
    serde_json::from_slice(&bytes[headers_end..headers_end + length])
        .context("invalid workspace service reply")
}

fn remaining(deadline: Instant) -> Result<Duration> {
    deadline
        .checked_duration_since(Instant::now())
        .context("workspace request timed out")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader};
    use std::net::TcpListener;
    use tempfile::TempDir;

    fn setup() -> (TempDir, PathBuf, WorkspaceContexts, Instant) {
        let temp = tempfile::tempdir().unwrap();
        let vault = temp.path().join("vault");
        std::fs::create_dir_all(vault.join("Notes")).unwrap();
        std::fs::write(vault.join("Notes/Current.md"), "# Current").unwrap();
        (
            temp,
            vault.canonicalize().unwrap(),
            WorkspaceContexts::default(),
            Instant::now(),
        )
    }

    fn update(vault: &Path, session: &str, version: u64, path: Option<&str>) -> ContextUpdate {
        ContextUpdate {
            vault_path: vault.to_path_buf(),
            session_id: session.into(),
            context_version: version,
            note: path.map(|path| NoteIdentity {
                path: path.into(),
                title: "Current".into(),
            }),
        }
    }

    fn current(
        contexts: &WorkspaceContexts,
        vault: &Path,
        session: Option<&str>,
        now: Instant,
    ) -> CurrentNoteReply {
        contexts
            .current_at(
                vault,
                CurrentNoteRequest {
                    vault_path: vault.into(),
                    session_id: session.map(str::to_owned),
                },
                now,
            )
            .unwrap()
    }

    #[test]
    fn unindexed_note_and_null_context_work_without_an_index() {
        let (_temp, vault, mut contexts, now) = setup();
        assert_eq!(
            current(&contexts, &vault, None, now).status,
            WorkspaceStatus::Unavailable
        );
        contexts
            .update_at(
                &vault,
                update(&vault, "window-1", 1, Some("Notes/Current.md")),
                now,
            )
            .unwrap();
        let reply = current(&contexts, &vault, None, now);
        assert_eq!(reply.status, WorkspaceStatus::Available);
        assert_eq!(reply.note.unwrap().path, "Notes/Current.md");
        assert!(!vault.join(".obsidian-kb").exists());
        contexts
            .update_at(&vault, update(&vault, "window-1", 2, None), now)
            .unwrap();
        let reply = current(&contexts, &vault, None, now);
        assert_eq!(reply.status, WorkspaceStatus::NoActiveNote);
        assert!(reply.note.is_none());
        assert!(
            contexts
                .delete(
                    &vault,
                    ContextDelete {
                        vault_path: vault.clone(),
                        session_id: "window-1".into()
                    }
                )
                .unwrap()
        );
        assert_eq!(
            current(&contexts, &vault, None, now).status,
            WorkspaceStatus::Unavailable
        );
    }

    #[test]
    fn ordering_rejects_old_or_conflicting_updates_and_heartbeat_refreshes_ttl() {
        let (_temp, vault, mut contexts, now) = setup();
        contexts
            .update_at(
                &vault,
                update(&vault, "window-1", 2, Some("Notes/Current.md")),
                now,
            )
            .unwrap();
        assert!(
            contexts
                .update_at(&vault, update(&vault, "window-1", 1, None), now)
                .is_err()
        );
        assert!(
            contexts
                .update_at(&vault, update(&vault, "window-1", 2, None), now)
                .is_err()
        );
        let stale = current(&contexts, &vault, None, now + CONTEXT_TTL);
        assert_eq!(stale.status, WorkspaceStatus::Stale);
        assert!(stale.note.is_none());
        assert_eq!(stale.context_version, Some(2));
        contexts
            .update_at(
                &vault,
                update(&vault, "window-1", 2, Some("Notes/Current.md")),
                now + Duration::from_secs(60),
            )
            .unwrap();
        let reply = current(&contexts, &vault, None, now + CONTEXT_TTL);
        assert_eq!(reply.status, WorkspaceStatus::Available);
        assert_eq!(reply.age_ms, Some(30_000));
        assert_eq!(
            current(&contexts, &vault, Some("missing"), now).status,
            WorkspaceStatus::Unavailable
        );
    }

    #[test]
    fn multiple_live_windows_require_explicit_selection_including_empty_windows() {
        let (_temp, vault, mut contexts, now) = setup();
        contexts
            .update_at(
                &vault,
                update(&vault, "window-1", 1, Some("Notes/Current.md")),
                now,
            )
            .unwrap();
        contexts
            .update_at(
                &vault,
                update(&vault, "window-2", 1, None),
                now + Duration::from_secs(1),
            )
            .unwrap();
        let reply = current(&contexts, &vault, None, now + Duration::from_secs(2));
        assert_eq!(reply.status, WorkspaceStatus::Ambiguous);
        assert!(reply.note.is_none());
        assert_eq!(reply.candidates.unwrap().len(), 2);
        assert_eq!(
            current(
                &contexts,
                &vault,
                Some("window-1"),
                now + Duration::from_secs(2)
            )
            .status,
            WorkspaceStatus::Available
        );
        assert_eq!(
            current(&contexts, &vault, None, now + CONTEXT_TTL)
                .session_id
                .as_deref(),
            Some("window-2")
        );
        assert_eq!(
            current(&contexts, &vault, Some("window-1"), now + CONTEXT_TTL).status,
            WorkspaceStatus::Stale
        );
    }

    #[test]
    fn wrong_vault_is_rejected_for_publish_read_and_delete() {
        let (temp, vault, mut contexts, now) = setup();
        let other = temp.path().join("other");
        std::fs::create_dir_all(&other).unwrap();
        assert!(
            contexts
                .update_at(&vault, update(&other, "window-1", 1, None), now)
                .is_err()
        );
        assert!(
            contexts
                .current(
                    &vault,
                    CurrentNoteRequest {
                        vault_path: other.clone(),
                        session_id: None
                    }
                )
                .is_err()
        );
        assert!(
            contexts
                .delete(
                    &vault,
                    ContextDelete {
                        vault_path: other,
                        session_id: "window-1".into()
                    }
                )
                .is_err()
        );
        assert!(contexts.sessions.is_empty());
    }

    #[test]
    fn unsafe_invalid_or_missing_note_paths_are_rejected() {
        let (_temp, vault, mut contexts, now) = setup();
        for path in [
            "/Notes/Current.md",
            "../Current.md",
            "Notes/../Notes/Current.md",
            "Notes//Current.md",
            "Notes/./Current.md",
            "Notes\\Current.md",
            "Notes/Current.pdf",
            "Notes/Missing.md",
            "Notes/Current.md\n",
        ] {
            assert!(
                contexts
                    .update_at(&vault, update(&vault, "window-1", 1, Some(path)), now)
                    .is_err(),
                "{path}"
            );
        }
        assert!(
            contexts
                .update_at(&vault, update(&vault, "window-1", 0, None), now)
                .is_err()
        );
        assert!(
            contexts
                .update_at(&vault, update(&vault, "bad\nwindow", 1, None), now)
                .is_err()
        );
        assert!(contexts.sessions.is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn symlink_escape_and_changed_or_removed_files_fail_closed() {
        use std::os::unix::fs::symlink;
        let (temp, vault, mut contexts, now) = setup();
        let outside = temp.path().join("outside.md");
        std::fs::write(&outside, "# Outside").unwrap();
        symlink(&outside, vault.join("Escape.md")).unwrap();
        assert!(
            contexts
                .update_at(
                    &vault,
                    update(&vault, "window-1", 1, Some("Escape.md")),
                    now
                )
                .is_err()
        );
        contexts
            .update_at(
                &vault,
                update(&vault, "window-1", 1, Some("Notes/Current.md")),
                now,
            )
            .unwrap();
        std::fs::remove_file(vault.join("Notes/Current.md")).unwrap();
        symlink(&outside, vault.join("Notes/Current.md")).unwrap();
        let reply = current(&contexts, &vault, None, now);
        assert_eq!(reply.status, WorkspaceStatus::Unavailable);
        assert!(reply.note.is_none());
        std::fs::remove_file(vault.join("Notes/Current.md")).unwrap();
        assert_eq!(
            current(&contexts, &vault, None, now).status,
            WorkspaceStatus::Unavailable
        );
    }

    #[test]
    fn session_capacity_is_bounded_and_only_stale_sessions_are_evicted() {
        let (_temp, vault, mut contexts, now) = setup();
        for i in 0..MAX_SESSIONS {
            contexts
                .update_at(&vault, update(&vault, &format!("window-{i}"), 1, None), now)
                .unwrap();
        }
        assert!(
            contexts
                .update_at(&vault, update(&vault, "extra", 1, None), now)
                .is_err()
        );
        contexts
            .update_at(&vault, update(&vault, "extra", 1, None), now + CONTEXT_TTL)
            .unwrap();
        assert_eq!(contexts.sessions.len(), MAX_SESSIONS);
    }

    #[test]
    fn absent_bridge_service_returns_unavailable_without_note() {
        let (_temp, vault, _contexts, _now) = setup();
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        let reply = current_note_via_http(&vault, port, None).unwrap();
        assert_eq!(reply.status, WorkspaceStatus::Unavailable);
        assert!(reply.note.is_none());
    }

    #[test]
    fn bridge_validates_vault_session_and_reply_status() {
        let (temp, vault, _contexts, _now) = setup();
        let other = temp.path().join("other");
        std::fs::create_dir_all(&other).unwrap();
        for (reply_vault, session, status, note, reason) in [
            (other, "window-1", "available", true, "vault_path"),
            (vault.clone(), "window-2", "available", true, "session_id"),
            (vault.clone(), "window-1", "stale", true, "usable note"),
        ] {
            let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
            let port = listener.local_addr().unwrap().port();
            let body = serde_json::json!({ "status": status, "vault_path": reply_vault,
                "session_id": session, "context_version": 1, "age_ms": 0,
                "note": if note { serde_json::json!({"path":"Notes/Current.md","title":"Current"}) } else { serde_json::Value::Null } }).to_string();
            let thread = std::thread::spawn(move || {
                let (mut stream, _) = listener.accept().unwrap();
                let mut reader = BufReader::new(&mut stream);
                let mut length = 0;
                loop {
                    let mut line = String::new();
                    assert!(reader.read_line(&mut line).unwrap() > 0);
                    if let Some(value) = line.strip_prefix("Content-Length:") {
                        length = value.trim().parse().unwrap();
                    }
                    if line == "\r\n" {
                        break;
                    }
                }
                let mut request = vec![0; length];
                reader.read_exact(&mut request).unwrap();
                write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n{body}",
                    body.len()
                )
                .unwrap();
            });
            let reply = current_note_via_http(&vault, port, Some("window-1".into())).unwrap();
            assert_eq!(reply.status, WorkspaceStatus::Unavailable);
            assert!(reply.note.is_none());
            assert!(reply.reason.unwrap().contains(reason));
            thread.join().unwrap();
        }
    }
}
