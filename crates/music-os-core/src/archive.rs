use anyhow::{anyhow, Context, Result};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

const SCHEMA_VERSION: i64 = 1;
static ID_SEQUENCE: AtomicU64 = AtomicU64::new(1);

#[derive(Debug)]
pub struct Archive {
    conn: Connection,
    vault_root: PathBuf,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ArchiveState {
    Active,
    Recall,
    Shadow,
    Historical,
    Replaceable,
    Archived,
}

impl ArchiveState {
    fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Recall => "recall",
            Self::Shadow => "shadow",
            Self::Historical => "historical",
            Self::Replaceable => "replaceable",
            Self::Archived => "archived",
        }
    }
}

impl TryFrom<&str> for ArchiveState {
    type Error = anyhow::Error;

    fn try_from(value: &str) -> Result<Self> {
        match value {
            "active" => Ok(Self::Active),
            "recall" => Ok(Self::Recall),
            "shadow" => Ok(Self::Shadow),
            "historical" => Ok(Self::Historical),
            "replaceable" => Ok(Self::Replaceable),
            "archived" => Ok(Self::Archived),
            other => Err(anyhow!("unknown archive state: {other}")),
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RepresentationRole {
    Discovery,
    Nostalgia,
    PreferredTechnical,
    HistoricalVariant,
    Shadow,
}

impl RepresentationRole {
    fn as_str(self) -> &'static str {
        match self {
            Self::Discovery => "discovery",
            Self::Nostalgia => "nostalgia",
            Self::PreferredTechnical => "preferred_technical",
            Self::HistoricalVariant => "historical_variant",
            Self::Shadow => "shadow",
        }
    }
}

impl TryFrom<&str> for RepresentationRole {
    type Error = anyhow::Error;

    fn try_from(value: &str) -> Result<Self> {
        match value {
            "discovery" => Ok(Self::Discovery),
            "nostalgia" => Ok(Self::Nostalgia),
            "preferred_technical" => Ok(Self::PreferredTechnical),
            "historical_variant" => Ok(Self::HistoricalVariant),
            "shadow" => Ok(Self::Shadow),
            other => Err(anyhow!("unknown representation role: {other}")),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportAudioRequest {
    pub source_path: PathBuf,
    pub track_id: Option<String>,
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album_title: Option<String>,
    pub album_artist: Option<String>,
    pub role: RepresentationRole,
    pub music_rating: Option<i64>,
    pub file_quality_rating: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportAudioResult {
    pub track: Track,
    pub file: VaultFile,
    pub representation: TrackRepresentation,
    pub was_already_in_vault: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Track {
    pub id: String,
    pub canonical_title: String,
    pub canonical_artist: Option<String>,
    pub archive_state: ArchiveState,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VaultFile {
    pub id: String,
    pub original_path: String,
    pub vault_path: String,
    pub sha256: String,
    pub byte_len: i64,
    pub format_extension: Option<String>,
    pub fingerprint: Option<String>,
    pub availability_status: String,
    pub imported_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrackRepresentation {
    pub id: String,
    pub track_id: String,
    pub file_id: Option<String>,
    pub role: RepresentationRole,
    pub label: Option<String>,
    pub source_path: Option<String>,
    pub is_available: bool,
    pub technical_score: Option<i64>,
    pub quality_notes: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrackRatings {
    pub track_id: String,
    pub music_rating: Option<i64>,
    pub file_quality_rating: Option<i64>,
    pub notes: Option<String>,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlbumContext {
    pub id: String,
    pub title: String,
    pub album_artist: Option<String>,
    pub preservation_state: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryEvent {
    pub id: String,
    pub track_id: String,
    pub representation_id: Option<String>,
    pub event_type: String,
    pub summary: String,
    pub payload_json: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrackRecord {
    pub track: Track,
    pub ratings: Option<TrackRatings>,
    pub albums: Vec<AlbumContext>,
    pub representations: Vec<TrackRepresentation>,
    pub history: Vec<HistoryEvent>,
}

impl Archive {
    pub fn open(db_path: impl AsRef<Path>, vault_root: impl AsRef<Path>) -> Result<Self> {
        if let Some(parent) = db_path.as_ref().parent() {
            fs::create_dir_all(parent).with_context(|| {
                format!("failed to create database directory {}", parent.display())
            })?;
        }
        fs::create_dir_all(vault_root.as_ref()).with_context(|| {
            format!(
                "failed to create vault directory {}",
                vault_root.as_ref().display()
            )
        })?;

        let conn = Connection::open(db_path.as_ref()).with_context(|| {
            format!(
                "failed to open archive database {}",
                db_path.as_ref().display()
            )
        })?;
        conn.pragma_update(None, "foreign_keys", "ON")?;

        let archive = Self {
            conn,
            vault_root: vault_root.as_ref().to_path_buf(),
        };
        archive.migrate()?;
        Ok(archive)
    }

    pub fn import_audio_file(&self, request: ImportAudioRequest) -> Result<ImportAudioResult> {
        let source_path = request.source_path;
        let metadata = fs::metadata(&source_path).with_context(|| {
            format!(
                "failed to read source file metadata {}",
                source_path.display()
            )
        })?;
        if !metadata.is_file() {
            return Err(anyhow!(
                "source path is not a file: {}",
                source_path.display()
            ));
        }

        let (sha256, byte_len) = checksum_file(&source_path)?;
        let extension = source_path
            .extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| ext.to_ascii_lowercase());
        let vault_path = self.vault_path_for_checksum(&sha256, extension.as_deref());
        let was_already_in_vault = vault_path.exists();
        if !was_already_in_vault {
            copy_into_vault(&source_path, &vault_path)?;
        }

        let now = now();
        let file = match self.find_vault_file(&sha256, byte_len)? {
            Some(file) => file,
            None => {
                let file_id = new_id();
                self.conn.execute(
                    "INSERT INTO vault_files
                        (id, original_path, vault_path, sha256, byte_len, format_extension, fingerprint, availability_status, imported_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, NULL, 'local', ?7)",
                    params![
                        file_id,
                        source_path.to_string_lossy(),
                        vault_path.to_string_lossy(),
                        sha256,
                        byte_len,
                        extension,
                        now,
                    ],
                )?;
                self.get_vault_file(&file_id)?
            }
        };

        let (inferred_artist, inferred_title) = infer_artist_title(&source_path);
        let track = match request.track_id {
            Some(track_id) => self.get_track(&track_id)?,
            None => {
                let title = request
                    .title
                    .clone()
                    .or(inferred_title)
                    .unwrap_or_else(|| "Untitled Track".to_string());
                let artist = request.artist.clone().or(inferred_artist);
                self.create_track(&title, artist.as_deref())?
            }
        };

        let representation = self.create_representation(
            &track.id,
            Some(&file.id),
            request.role,
            Some(request.role.as_str()),
            Some(source_path.to_string_lossy().as_ref()),
            true,
            None,
            None,
        )?;

        if request.music_rating.is_some() || request.file_quality_rating.is_some() {
            self.update_track_ratings(
                &track.id,
                request.music_rating,
                request.file_quality_rating,
                Some("Initial import rating"),
            )?;
        }

        if let Some(album_title) = request.album_title.as_deref() {
            let album = self.find_or_create_album(album_title, request.album_artist.as_deref())?;
            self.conn.execute(
                "INSERT OR IGNORE INTO album_tracks
                    (album_id, track_id, disc_number, track_number, title_in_album)
                 VALUES (?1, ?2, NULL, NULL, ?3)",
                params![album.id, track.id, track.canonical_title],
            )?;
            self.record_history(
                &track.id,
                Some(&representation.id),
                "album_context_preserved",
                &format!("Preserved album context: {album_title}"),
                None,
            )?;
        }

        self.record_history(
            &track.id,
            Some(&representation.id),
            "file_imported",
            "Imported source file into immutable vault storage",
            Some(serde_json::json!({
                "sha256": file.sha256,
                "original_path": file.original_path,
                "vault_path": file.vault_path,
            })),
        )?;

        Ok(ImportAudioResult {
            track,
            file,
            representation,
            was_already_in_vault,
        })
    }

    pub fn list_tracks(&self) -> Result<Vec<TrackRecord>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, canonical_title, canonical_artist, archive_state, created_at, updated_at
             FROM tracks
             ORDER BY updated_at DESC, canonical_title ASC",
        )?;
        let tracks = stmt
            .query_map([], |row| track_from_row(row))?
            .collect::<rusqlite::Result<Vec<_>>>()?;

        tracks
            .into_iter()
            .map(|track| {
                Ok(TrackRecord {
                    ratings: self.get_ratings(&track.id)?,
                    albums: self.list_album_contexts(&track.id)?,
                    representations: self.list_representations(&track.id)?,
                    history: self.list_history(&track.id, 20)?,
                    track,
                })
            })
            .collect()
    }

    pub fn update_track_ratings(
        &self,
        track_id: &str,
        music_rating: Option<i64>,
        file_quality_rating: Option<i64>,
        notes: Option<&str>,
    ) -> Result<TrackRatings> {
        validate_rating(music_rating, "music_rating")?;
        validate_rating(file_quality_rating, "file_quality_rating")?;
        self.ensure_track_exists(track_id)?;

        let now = now();
        self.conn.execute(
            "INSERT INTO track_ratings (track_id, music_rating, file_quality_rating, notes, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(track_id) DO UPDATE SET
                music_rating = excluded.music_rating,
                file_quality_rating = excluded.file_quality_rating,
                notes = excluded.notes,
                updated_at = excluded.updated_at",
            params![track_id, music_rating, file_quality_rating, notes, now],
        )?;
        self.touch_track(track_id)?;
        self.record_history(
            track_id,
            None,
            "ratings_updated",
            "Updated music appreciation and file quality ratings",
            Some(serde_json::json!({
                "music_rating": music_rating,
                "file_quality_rating": file_quality_rating,
                "notes": notes,
            })),
        )?;

        self.get_ratings(track_id)?
            .ok_or_else(|| anyhow!("ratings were not saved for track {track_id}"))
    }

    pub fn set_track_archive_state(&self, track_id: &str, state: ArchiveState) -> Result<Track> {
        self.ensure_track_exists(track_id)?;
        self.conn.execute(
            "UPDATE tracks SET archive_state = ?1, updated_at = ?2 WHERE id = ?3",
            params![state.as_str(), now(), track_id],
        )?;
        self.record_history(
            track_id,
            None,
            "archive_state_changed",
            &format!("Moved track to {} state", state.as_str()),
            Some(serde_json::json!({ "archive_state": state.as_str() })),
        )?;
        self.get_track(track_id)
    }

    pub fn create_shadow_entry(
        &self,
        track_id: &str,
        label: Option<&str>,
        source_path: Option<&str>,
        fingerprint: Option<&str>,
        notes: Option<&str>,
    ) -> Result<TrackRepresentation> {
        self.ensure_track_exists(track_id)?;
        let representation = self.create_representation(
            track_id,
            None,
            RepresentationRole::Shadow,
            label.or(Some("Shadow memory")),
            source_path,
            false,
            None,
            notes,
        )?;
        self.record_history(
            track_id,
            Some(&representation.id),
            "shadow_entry_created",
            "Created a shadow entry without requiring local audio",
            Some(serde_json::json!({
                "fingerprint": fingerprint,
                "source_path": source_path,
                "notes": notes,
            })),
        )?;
        Ok(representation)
    }

    pub fn set_representation_role(
        &self,
        representation_id: &str,
        role: RepresentationRole,
    ) -> Result<TrackRepresentation> {
        let track_id = self
            .conn
            .query_row(
                "SELECT track_id FROM track_representations WHERE id = ?1",
                params![representation_id],
                |row| row.get::<_, String>(0),
            )
            .optional()?
            .ok_or_else(|| anyhow!("representation not found: {representation_id}"))?;

        self.conn.execute(
            "UPDATE track_representations SET role = ?1 WHERE id = ?2",
            params![role.as_str(), representation_id],
        )?;
        self.touch_track(&track_id)?;
        self.record_history(
            &track_id,
            Some(representation_id),
            "representation_role_changed",
            &format!("Marked representation as {}", role.as_str()),
            Some(serde_json::json!({ "role": role.as_str() })),
        )?;
        self.get_representation(representation_id)
    }

    fn migrate(&self) -> Result<()> {
        self.conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS app_metadata (
              key TEXT PRIMARY KEY,
              value TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS vault_files (
              id TEXT PRIMARY KEY,
              original_path TEXT NOT NULL,
              vault_path TEXT NOT NULL,
              sha256 TEXT NOT NULL,
              byte_len INTEGER NOT NULL,
              format_extension TEXT,
              fingerprint TEXT,
              availability_status TEXT NOT NULL CHECK (availability_status IN ('local', 'missing', 'shadow_only')),
              imported_at TEXT NOT NULL,
              UNIQUE (sha256, byte_len)
            );

            CREATE TABLE IF NOT EXISTS tracks (
              id TEXT PRIMARY KEY,
              canonical_title TEXT NOT NULL,
              canonical_artist TEXT,
              archive_state TEXT NOT NULL CHECK (archive_state IN ('active', 'recall', 'shadow', 'historical', 'replaceable', 'archived')),
              created_at TEXT NOT NULL,
              updated_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS track_representations (
              id TEXT PRIMARY KEY,
              track_id TEXT NOT NULL REFERENCES tracks(id) ON DELETE RESTRICT,
              file_id TEXT REFERENCES vault_files(id) ON DELETE RESTRICT,
              role TEXT NOT NULL CHECK (role IN ('discovery', 'nostalgia', 'preferred_technical', 'historical_variant', 'shadow')),
              label TEXT,
              source_path TEXT,
              is_available INTEGER NOT NULL CHECK (is_available IN (0, 1)),
              technical_score INTEGER CHECK (technical_score BETWEEN 0 AND 5),
              quality_notes TEXT,
              created_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS track_ratings (
              track_id TEXT PRIMARY KEY REFERENCES tracks(id) ON DELETE RESTRICT,
              music_rating INTEGER CHECK (music_rating BETWEEN 0 AND 5),
              file_quality_rating INTEGER CHECK (file_quality_rating BETWEEN 0 AND 5),
              notes TEXT,
              updated_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS albums (
              id TEXT PRIMARY KEY,
              title TEXT NOT NULL,
              album_artist TEXT,
              release_year INTEGER,
              preservation_state TEXT NOT NULL DEFAULT 'preserved',
              created_at TEXT NOT NULL,
              UNIQUE (title, album_artist)
            );

            CREATE TABLE IF NOT EXISTS album_tracks (
              album_id TEXT NOT NULL REFERENCES albums(id) ON DELETE RESTRICT,
              track_id TEXT NOT NULL REFERENCES tracks(id) ON DELETE RESTRICT,
              disc_number INTEGER,
              track_number INTEGER,
              title_in_album TEXT,
              PRIMARY KEY (album_id, track_id)
            );

            CREATE TABLE IF NOT EXISTS track_relationships (
              id TEXT PRIMARY KEY,
              from_track_id TEXT NOT NULL REFERENCES tracks(id) ON DELETE RESTRICT,
              to_track_id TEXT NOT NULL REFERENCES tracks(id) ON DELETE RESTRICT,
              relationship_type TEXT NOT NULL,
              notes TEXT,
              created_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS history_events (
              id TEXT PRIMARY KEY,
              track_id TEXT NOT NULL REFERENCES tracks(id) ON DELETE RESTRICT,
              representation_id TEXT REFERENCES track_representations(id) ON DELETE SET NULL,
              event_type TEXT NOT NULL,
              summary TEXT NOT NULL,
              payload_json TEXT,
              created_at TEXT NOT NULL
            );

            CREATE INDEX IF NOT EXISTS idx_representations_track_id ON track_representations(track_id);
            CREATE INDEX IF NOT EXISTS idx_history_track_id_created_at ON history_events(track_id, created_at DESC);
            CREATE INDEX IF NOT EXISTS idx_album_tracks_track_id ON album_tracks(track_id);
            ",
        )?;
        self.conn.execute(
            "INSERT INTO app_metadata (key, value)
             VALUES ('schema_version', ?1)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![SCHEMA_VERSION.to_string()],
        )?;
        Ok(())
    }

    fn create_track(&self, title: &str, artist: Option<&str>) -> Result<Track> {
        let id = new_id();
        let now = now();
        self.conn.execute(
            "INSERT INTO tracks (id, canonical_title, canonical_artist, archive_state, created_at, updated_at)
             VALUES (?1, ?2, ?3, 'active', ?4, ?4)",
            params![id, title, artist, now],
        )?;
        self.record_history(&id, None, "track_created", "Created track entity", None)?;
        self.get_track(&id)
    }

    fn find_or_create_album(
        &self,
        title: &str,
        album_artist: Option<&str>,
    ) -> Result<AlbumContext> {
        if let Some(album) = self
            .conn
            .query_row(
                "SELECT id, title, album_artist, preservation_state FROM albums
                 WHERE title = ?1 AND album_artist IS ?2",
                params![title, album_artist],
                |row| {
                    Ok(AlbumContext {
                        id: row.get(0)?,
                        title: row.get(1)?,
                        album_artist: row.get(2)?,
                        preservation_state: row.get(3)?,
                    })
                },
            )
            .optional()?
        {
            return Ok(album);
        }

        let id = new_id();
        self.conn.execute(
            "INSERT INTO albums (id, title, album_artist, preservation_state, created_at)
             VALUES (?1, ?2, ?3, 'preserved', ?4)",
            params![id, title, album_artist, now()],
        )?;
        Ok(AlbumContext {
            id,
            title: title.to_string(),
            album_artist: album_artist.map(ToOwned::to_owned),
            preservation_state: "preserved".to_string(),
        })
    }

    fn create_representation(
        &self,
        track_id: &str,
        file_id: Option<&str>,
        role: RepresentationRole,
        label: Option<&str>,
        source_path: Option<&str>,
        is_available: bool,
        technical_score: Option<i64>,
        quality_notes: Option<&str>,
    ) -> Result<TrackRepresentation> {
        validate_rating(technical_score, "technical_score")?;
        let id = new_id();
        let now = now();
        self.conn.execute(
            "INSERT INTO track_representations
                (id, track_id, file_id, role, label, source_path, is_available, technical_score, quality_notes, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                id,
                track_id,
                file_id,
                role.as_str(),
                label,
                source_path,
                if is_available { 1 } else { 0 },
                technical_score,
                quality_notes,
                now,
            ],
        )?;
        self.touch_track(track_id)?;
        self.record_history(
            track_id,
            Some(&id),
            "representation_added",
            &format!("Added {} representation", role.as_str()),
            Some(serde_json::json!({ "role": role.as_str(), "file_id": file_id })),
        )?;
        self.get_representation(&id)
    }

    fn get_track(&self, track_id: &str) -> Result<Track> {
        self.conn
            .query_row(
                "SELECT id, canonical_title, canonical_artist, archive_state, created_at, updated_at
                 FROM tracks WHERE id = ?1",
                params![track_id],
                |row| track_from_row(row),
            )
            .optional()?
            .ok_or_else(|| anyhow!("track not found: {track_id}"))
    }

    fn ensure_track_exists(&self, track_id: &str) -> Result<()> {
        let exists: bool = self.conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM tracks WHERE id = ?1)",
            params![track_id],
            |row| row.get(0),
        )?;
        if exists {
            Ok(())
        } else {
            Err(anyhow!("track not found: {track_id}"))
        }
    }

    fn find_vault_file(&self, sha256: &str, byte_len: i64) -> Result<Option<VaultFile>> {
        self.conn
            .query_row(
                "SELECT id, original_path, vault_path, sha256, byte_len, format_extension, fingerprint, availability_status, imported_at
                 FROM vault_files WHERE sha256 = ?1 AND byte_len = ?2",
                params![sha256, byte_len],
                |row| vault_file_from_row(row),
            )
            .optional()
            .map_err(Into::into)
    }

    fn get_vault_file(&self, file_id: &str) -> Result<VaultFile> {
        self.conn
            .query_row(
                "SELECT id, original_path, vault_path, sha256, byte_len, format_extension, fingerprint, availability_status, imported_at
                 FROM vault_files WHERE id = ?1",
                params![file_id],
                |row| vault_file_from_row(row),
            )
            .optional()?
            .ok_or_else(|| anyhow!("vault file not found: {file_id}"))
    }

    fn get_representation(&self, representation_id: &str) -> Result<TrackRepresentation> {
        self.conn
            .query_row(
                "SELECT id, track_id, file_id, role, label, source_path, is_available, technical_score, quality_notes, created_at
                 FROM track_representations WHERE id = ?1",
                params![representation_id],
                |row| representation_from_row(row),
            )
            .optional()?
            .ok_or_else(|| anyhow!("representation not found: {representation_id}"))
    }

    fn get_ratings(&self, track_id: &str) -> Result<Option<TrackRatings>> {
        self.conn
            .query_row(
                "SELECT track_id, music_rating, file_quality_rating, notes, updated_at
                 FROM track_ratings WHERE track_id = ?1",
                params![track_id],
                |row| {
                    Ok(TrackRatings {
                        track_id: row.get(0)?,
                        music_rating: row.get(1)?,
                        file_quality_rating: row.get(2)?,
                        notes: row.get(3)?,
                        updated_at: row.get(4)?,
                    })
                },
            )
            .optional()
            .map_err(Into::into)
    }

    fn list_representations(&self, track_id: &str) -> Result<Vec<TrackRepresentation>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, track_id, file_id, role, label, source_path, is_available, technical_score, quality_notes, created_at
             FROM track_representations
             WHERE track_id = ?1
             ORDER BY created_at ASC",
        )?;
        let representations = stmt
            .query_map(params![track_id], |row| representation_from_row(row))?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(anyhow::Error::from)?;
        Ok(representations)
    }

    fn list_album_contexts(&self, track_id: &str) -> Result<Vec<AlbumContext>> {
        let mut stmt = self.conn.prepare(
            "SELECT albums.id, albums.title, albums.album_artist, albums.preservation_state
             FROM albums
             INNER JOIN album_tracks ON album_tracks.album_id = albums.id
             WHERE album_tracks.track_id = ?1
             ORDER BY albums.title ASC",
        )?;
        let albums = stmt
            .query_map(params![track_id], |row| {
                Ok(AlbumContext {
                    id: row.get(0)?,
                    title: row.get(1)?,
                    album_artist: row.get(2)?,
                    preservation_state: row.get(3)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(anyhow::Error::from)?;
        Ok(albums)
    }

    fn list_history(&self, track_id: &str, limit: i64) -> Result<Vec<HistoryEvent>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, track_id, representation_id, event_type, summary, payload_json, created_at
             FROM history_events
             WHERE track_id = ?1
             ORDER BY created_at DESC
             LIMIT ?2",
        )?;
        let history = stmt
            .query_map(params![track_id, limit], |row| {
                Ok(HistoryEvent {
                    id: row.get(0)?,
                    track_id: row.get(1)?,
                    representation_id: row.get(2)?,
                    event_type: row.get(3)?,
                    summary: row.get(4)?,
                    payload_json: row.get(5)?,
                    created_at: row.get(6)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(anyhow::Error::from)?;
        Ok(history)
    }

    fn record_history(
        &self,
        track_id: &str,
        representation_id: Option<&str>,
        event_type: &str,
        summary: &str,
        payload: Option<serde_json::Value>,
    ) -> Result<()> {
        self.conn.execute(
            "INSERT INTO history_events
                (id, track_id, representation_id, event_type, summary, payload_json, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                new_id(),
                track_id,
                representation_id,
                event_type,
                summary,
                payload.map(|value| value.to_string()),
                now(),
            ],
        )?;
        Ok(())
    }

    fn touch_track(&self, track_id: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE tracks SET updated_at = ?1 WHERE id = ?2",
            params![now(), track_id],
        )?;
        Ok(())
    }

    fn vault_path_for_checksum(&self, sha256: &str, extension: Option<&str>) -> PathBuf {
        let mut filename = sha256.to_string();
        if let Some(extension) = extension {
            filename.push('.');
            filename.push_str(extension);
        }
        self.vault_root
            .join("originals")
            .join(&sha256[0..2])
            .join(filename)
    }
}

fn checksum_file(path: &Path) -> Result<(String, i64)> {
    let file = File::open(path).with_context(|| format!("failed to open {}", path.display()))?;
    let mut reader = BufReader::new(file);
    let mut hasher = Sha256::new();
    let mut bytes_read = 0_i64;
    let mut buffer = [0_u8; 64 * 1024];

    loop {
        let count = reader.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
        bytes_read += count as i64;
    }

    Ok((format!("{:x}", hasher.finalize()), bytes_read))
}

fn copy_into_vault(source_path: &Path, vault_path: &Path) -> Result<()> {
    if let Some(parent) = vault_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create vault shard {}", parent.display()))?;
    }

    match OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(vault_path)
    {
        Ok(mut destination) => {
            let mut source = File::open(source_path).with_context(|| {
                format!("failed to reopen source file {}", source_path.display())
            })?;
            io::copy(&mut source, &mut destination)?;
            destination.flush()?;
            Ok(())
        }
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => Ok(()),
        Err(error) => Err(error)
            .with_context(|| format!("failed to create vault file {}", vault_path.display())),
    }
}

fn infer_artist_title(path: &Path) -> (Option<String>, Option<String>) {
    let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()) else {
        return (None, None);
    };

    if let Some((artist, title)) = stem.split_once(" - ") {
        (
            Some(artist.trim().to_string()),
            Some(title.trim().to_string()),
        )
    } else {
        (None, Some(stem.trim().to_string()))
    }
}

fn validate_rating(value: Option<i64>, field: &str) -> Result<()> {
    match value {
        Some(value) if !(0..=5).contains(&value) => Err(anyhow!("{field} must be between 0 and 5")),
        _ => Ok(()),
    }
}

fn new_id() -> String {
    let sequence = ID_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    format!("mos-{}-{sequence}", now_millis())
}

fn now() -> String {
    now_millis().to_string()
}

fn now_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0)
}

fn track_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Track> {
    let archive_state: String = row.get(3)?;
    Ok(Track {
        id: row.get(0)?,
        canonical_title: row.get(1)?,
        canonical_artist: row.get(2)?,
        archive_state: ArchiveState::try_from(archive_state.as_str()).map_err(to_sql_error)?,
        created_at: row.get(4)?,
        updated_at: row.get(5)?,
    })
}

fn vault_file_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<VaultFile> {
    Ok(VaultFile {
        id: row.get(0)?,
        original_path: row.get(1)?,
        vault_path: row.get(2)?,
        sha256: row.get(3)?,
        byte_len: row.get(4)?,
        format_extension: row.get(5)?,
        fingerprint: row.get(6)?,
        availability_status: row.get(7)?,
        imported_at: row.get(8)?,
    })
}

fn representation_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<TrackRepresentation> {
    let role: String = row.get(3)?;
    let is_available: i64 = row.get(6)?;
    Ok(TrackRepresentation {
        id: row.get(0)?,
        track_id: row.get(1)?,
        file_id: row.get(2)?,
        role: RepresentationRole::try_from(role.as_str()).map_err(to_sql_error)?,
        label: row.get(4)?,
        source_path: row.get(5)?,
        is_available: is_available == 1,
        technical_score: row.get(7)?,
        quality_notes: row.get(8)?,
        created_at: row.get(9)?,
    })
}

fn to_sql_error(error: anyhow::Error) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(
        0,
        rusqlite::types::Type::Text,
        Box::new(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            error.to_string(),
        )),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestDir {
        path: PathBuf,
    }

    impl TestDir {
        fn new() -> Self {
            let path = std::env::temp_dir().join(new_id());
            fs::create_dir_all(&path).expect("create temp test directory");
            Self { path }
        }

        fn path(&self) -> &Path {
            &self.path
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    fn archive() -> (TestDir, Archive) {
        let test_dir = TestDir::new();
        let db_path = test_dir.path().join("library.sqlite");
        let vault_root = test_dir.path().join("vault");
        let archive = Archive::open(db_path, vault_root).expect("archive");
        (test_dir, archive)
    }

    #[test]
    fn import_copies_file_into_checksum_vault_without_modifying_source() {
        let (temp_dir, archive) = archive();
        let source = temp_dir.path().join("Artist - Memory.flac");
        fs::write(&source, b"not really audio, but immutable bytes").expect("write source");

        let result = archive
            .import_audio_file(ImportAudioRequest {
                source_path: source.clone(),
                track_id: None,
                title: None,
                artist: None,
                album_title: Some("Old Album".to_string()),
                album_artist: Some("Artist".to_string()),
                role: RepresentationRole::Discovery,
                music_rating: Some(5),
                file_quality_rating: Some(2),
            })
            .expect("import");

        assert_eq!(
            fs::read(&source).expect("source bytes"),
            b"not really audio, but immutable bytes"
        );
        assert!(Path::new(&result.file.vault_path).exists());
        assert_eq!(result.track.canonical_title, "Memory");
        assert_eq!(result.track.canonical_artist.as_deref(), Some("Artist"));

        let records = archive.list_tracks().expect("tracks");
        assert_eq!(records.len(), 1);
        assert_eq!(
            records[0]
                .ratings
                .as_ref()
                .and_then(|rating| rating.music_rating),
            Some(5)
        );
        assert_eq!(records[0].albums[0].title, "Old Album");
    }

    #[test]
    fn same_bytes_can_be_multiple_representations_without_duplicate_vault_file() {
        let (temp_dir, archive) = archive();
        let source = temp_dir.path().join("Demo.flac");
        fs::write(&source, b"same bytes").expect("write source");

        let first = archive
            .import_audio_file(ImportAudioRequest {
                source_path: source.clone(),
                track_id: None,
                title: Some("Demo".to_string()),
                artist: None,
                album_title: None,
                album_artist: None,
                role: RepresentationRole::Discovery,
                music_rating: None,
                file_quality_rating: None,
            })
            .expect("first import");

        let second = archive
            .import_audio_file(ImportAudioRequest {
                source_path: source,
                track_id: Some(first.track.id.clone()),
                title: None,
                artist: None,
                album_title: None,
                album_artist: None,
                role: RepresentationRole::Nostalgia,
                music_rating: None,
                file_quality_rating: None,
            })
            .expect("second import");

        assert_eq!(first.file.id, second.file.id);
        let record = archive.list_tracks().expect("tracks").pop().expect("track");
        assert_eq!(record.representations.len(), 2);
    }

    #[test]
    fn shadow_entries_preserve_history_without_local_audio() {
        let (temp_dir, archive) = archive();
        let source = temp_dir.path().join("Lost Song.mp3");
        fs::write(&source, b"temporary bytes").expect("write source");

        let imported = archive
            .import_audio_file(ImportAudioRequest {
                source_path: source,
                track_id: None,
                title: Some("Lost Song".to_string()),
                artist: None,
                album_title: None,
                album_artist: None,
                role: RepresentationRole::Discovery,
                music_rating: Some(4),
                file_quality_rating: Some(1),
            })
            .expect("import");

        archive
            .set_track_archive_state(&imported.track.id, ArchiveState::Recall)
            .expect("recall state");
        let shadow = archive
            .create_shadow_entry(
                &imported.track.id,
                Some("Remembered from old laptop"),
                Some("/old/laptop/Lost Song.mp3"),
                Some("future-audio-fingerprint"),
                Some("Keep the memory even if bytes are unavailable"),
            )
            .expect("shadow");

        assert!(!shadow.is_available);
        assert_eq!(shadow.role, RepresentationRole::Shadow);
        let record = archive.list_tracks().expect("tracks").pop().expect("track");
        assert_eq!(record.track.archive_state, ArchiveState::Recall);
        assert!(record
            .history
            .iter()
            .any(|event| event.event_type == "shadow_entry_created"));
    }

    #[test]
    fn ratings_validate_music_and_file_quality_separately() {
        let (_temp_dir, archive) = archive();
        let track = archive.create_track("Song", None).expect("track");

        let error = archive
            .update_track_ratings(&track.id, Some(6), Some(3), None)
            .expect_err("rating should fail");
        assert!(error.to_string().contains("music_rating"));

        let ratings = archive
            .update_track_ratings(&track.id, Some(5), Some(0), Some("Great song, poor file"))
            .expect("ratings");
        assert_eq!(ratings.music_rating, Some(5));
        assert_eq!(ratings.file_quality_rating, Some(0));
    }
}
