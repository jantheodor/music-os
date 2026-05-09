use anyhow::{anyhow, Context, Result};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

const SCHEMA_VERSION: i64 = 2;
static ID_SEQUENCE: AtomicU64 = AtomicU64::new(1);

#[derive(Debug)]
pub struct Archive {
    conn: Connection,
    vault_root: PathBuf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RepresentationRole {
    FirstFound,
    Nostalgia,
    Variant,
}

impl RepresentationRole {
    fn as_str(self) -> &'static str {
        match self {
            Self::FirstFound => "first_found",
            Self::Nostalgia => "nostalgia",
            Self::Variant => "variant",
        }
    }
}

impl TryFrom<&str> for RepresentationRole {
    type Error = anyhow::Error;

    fn try_from(value: &str) -> Result<Self> {
        match value {
            "first_found" => Ok(Self::FirstFound),
            "nostalgia" => Ok(Self::Nostalgia),
            "variant" => Ok(Self::Variant),
            other => Err(anyhow!("unknown representation role: {other}")),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StorageState {
    Local,
    External,
    Shadow,
    Missing,
}

impl StorageState {
    fn as_str(self) -> &'static str {
        match self {
            Self::Local => "local",
            Self::External => "external",
            Self::Shadow => "shadow",
            Self::Missing => "missing",
        }
    }
}

impl TryFrom<&str> for StorageState {
    type Error = anyhow::Error;

    fn try_from(value: &str) -> Result<Self> {
        match value {
            "local" => Ok(Self::Local),
            "external" => Ok(Self::External),
            "shadow" => Ok(Self::Shadow),
            "missing" => Ok(Self::Missing),
            other => Err(anyhow!("unknown storage state: {other}")),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlaybackMode {
    Default,
    Portable,
    Nostalgia,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrackIdentity {
    pub id: String,
    pub artist: String,
    pub title: String,
    pub version: Option<String>,
    pub user_rating: Option<i64>,
    pub best_lossy_asset_id: Option<String>,
    pub best_lossless_asset_id: Option<String>,
    pub best_verified_asset_id: Option<String>,
    pub nostalgia_asset_id: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioAsset {
    pub id: String,
    pub track_identity_id: String,
    pub role: RepresentationRole,
    pub storage_state: StorageState,
    pub vault_path: Option<String>,
    pub original_path: Option<String>,
    pub original_filename: Option<String>,
    pub checksum: Option<String>,
    pub audio_fingerprint: Option<String>,
    pub format: Option<String>,
    pub bitrate_kbps: Option<i64>,
    pub sample_rate_hz: Option<i64>,
    pub duration_ms: Option<i64>,
    pub file_size: Option<i64>,
    pub dynamic_range: Option<f64>,
    pub integrated_lufs: Option<f64>,
    pub peak_db: Option<f64>,
    pub quality_score: Option<i64>,
    pub true_lossless_verified: bool,
    pub suspected_transcode: bool,
    pub original_tags_json: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SemanticTag {
    pub id: String,
    pub label: String,
    pub normalized_label: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrackRecord {
    pub identity: TrackIdentity,
    pub assets: Vec<AudioAsset>,
    pub tags: Vec<SemanticTag>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportAudioRequest {
    pub source_path: PathBuf,
    pub track_identity_id: Option<String>,
    pub title: Option<String>,
    pub artist: Option<String>,
    pub version: Option<String>,
    pub role: Option<RepresentationRole>,
    pub user_rating: Option<i64>,
    pub semantic_tags: Vec<String>,
    pub original_tags_json: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportAudioResult {
    pub track_identity: TrackIdentity,
    pub audio_asset: AudioAsset,
    pub extracted_filename_tags: Vec<SemanticTag>,
    pub was_already_in_vault: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateTrackIdentityRequest {
    pub artist: String,
    pub title: String,
    pub version: Option<String>,
    pub user_rating: Option<i64>,
    pub semantic_tags: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegisterAudioAssetRequest {
    pub track_identity_id: String,
    pub role: Option<RepresentationRole>,
    pub storage_state: StorageState,
    pub vault_path: Option<String>,
    pub original_path: Option<String>,
    pub original_filename: Option<String>,
    pub checksum: Option<String>,
    pub audio_fingerprint: Option<String>,
    pub format: Option<String>,
    pub bitrate_kbps: Option<i64>,
    pub sample_rate_hz: Option<i64>,
    pub duration_ms: Option<i64>,
    pub file_size: Option<i64>,
    pub dynamic_range: Option<f64>,
    pub integrated_lufs: Option<f64>,
    pub peak_db: Option<f64>,
    pub quality_score: Option<i64>,
    pub true_lossless_verified: bool,
    pub suspected_transcode: bool,
    pub original_tags_json: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QualityPointerUpdate {
    pub best_lossy_asset_id: Option<String>,
    pub best_lossless_asset_id: Option<String>,
    pub best_verified_asset_id: Option<String>,
    pub nostalgia_asset_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct FilenameInterpretation {
    artist: Option<String>,
    title: Option<String>,
    clean_stem: String,
    tags: Vec<String>,
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

    pub fn create_track_identity(
        &self,
        request: CreateTrackIdentityRequest,
    ) -> Result<TrackIdentity> {
        validate_rating(request.user_rating)?;
        let artist = clean_required(&request.artist, "artist")?;
        let title = clean_required(&request.title, "title")?;
        let id = new_id();
        let now = now();
        self.conn.execute(
            "INSERT INTO track_identities
                (id, artist, title, version, user_rating, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)",
            params![id, artist, title, request.version, request.user_rating, now],
        )?;
        self.add_track_tags(&id, &request.semantic_tags)?;
        self.get_track_identity(&id)
    }

    pub fn import_audio_file(&self, request: ImportAudioRequest) -> Result<ImportAudioResult> {
        validate_rating(request.user_rating)?;
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

        let interpretation = interpret_filename(&source_path);
        let extension = source_path
            .extension()
            .and_then(|extension| extension.to_str())
            .map(|extension| extension.to_ascii_lowercase());
        let (checksum, file_size) = checksum_file(&source_path)?;
        let vault_path = self.vault_path_for_checksum(&checksum, extension.as_deref());
        let was_already_in_vault = vault_path.exists();
        if !was_already_in_vault {
            copy_into_vault(&source_path, &vault_path)?;
        }

        let track_identity = match request.track_identity_id {
            Some(track_identity_id) => {
                let track_identity = self.get_track_identity(&track_identity_id)?;
                if let Some(user_rating) = request.user_rating {
                    self.update_track_rating(&track_identity.id, Some(user_rating))?
                } else {
                    track_identity
                }
            }
            None => {
                let artist = request
                    .artist
                    .clone()
                    .or(interpretation.artist.clone())
                    .unwrap_or_else(|| "Unknown Artist".to_string());
                let title = request
                    .title
                    .clone()
                    .or(interpretation.title.clone())
                    .unwrap_or_else(|| interpretation.clean_stem.clone());
                self.create_track_identity(CreateTrackIdentityRequest {
                    artist,
                    title,
                    version: request.version.clone(),
                    user_rating: request.user_rating,
                    semantic_tags: Vec::new(),
                })?
            }
        };

        let mut tags = request.semantic_tags.clone();
        tags.extend(interpretation.tags.clone());
        let extracted_filename_tags = self.add_track_tags(&track_identity.id, &tags)?;

        let role = request
            .role
            .unwrap_or(self.default_role_for_track(&track_identity.id)?);
        let audio_asset = self.register_audio_asset(RegisterAudioAssetRequest {
            track_identity_id: track_identity.id.clone(),
            role: Some(role),
            storage_state: StorageState::Local,
            vault_path: Some(vault_path.to_string_lossy().to_string()),
            original_path: Some(source_path.to_string_lossy().to_string()),
            original_filename: source_path
                .file_name()
                .and_then(|filename| filename.to_str())
                .map(ToOwned::to_owned),
            checksum: Some(checksum),
            audio_fingerprint: None,
            format: extension,
            bitrate_kbps: None,
            sample_rate_hz: None,
            duration_ms: None,
            file_size: Some(file_size),
            dynamic_range: None,
            integrated_lufs: None,
            peak_db: None,
            quality_score: None,
            true_lossless_verified: false,
            suspected_transcode: false,
            original_tags_json: request.original_tags_json,
        })?;

        Ok(ImportAudioResult {
            track_identity: self.get_track_identity(&track_identity.id)?,
            audio_asset,
            extracted_filename_tags,
            was_already_in_vault,
        })
    }

    pub fn register_audio_asset(&self, request: RegisterAudioAssetRequest) -> Result<AudioAsset> {
        self.ensure_track_identity_exists(&request.track_identity_id)?;
        let role = request
            .role
            .unwrap_or(self.default_role_for_track(&request.track_identity_id)?);
        validate_quality_score(request.quality_score)?;
        let id = new_id();
        let now = now();
        self.conn.execute(
            "INSERT INTO audio_assets
                (id, track_identity_id, role, storage_state, vault_path, original_path,
                 original_filename, checksum, audio_fingerprint, format, bitrate_kbps,
                 sample_rate_hz, duration_ms, file_size, dynamic_range, integrated_lufs,
                 peak_db, quality_score, true_lossless_verified, suspected_transcode,
                 original_tags_json, created_at, updated_at)
             VALUES
                (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14,
                 ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?22)",
            params![
                id,
                request.track_identity_id,
                role.as_str(),
                request.storage_state.as_str(),
                request.vault_path,
                request.original_path,
                request.original_filename,
                request.checksum,
                request.audio_fingerprint,
                request.format.map(|format| normalize_format(&format)),
                request.bitrate_kbps,
                request.sample_rate_hz,
                request.duration_ms,
                request.file_size,
                request.dynamic_range,
                request.integrated_lufs,
                request.peak_db,
                request.quality_score,
                bool_to_int(request.true_lossless_verified),
                bool_to_int(request.suspected_transcode),
                request.original_tags_json,
                now,
            ],
        )?;
        self.touch_track(&request.track_identity_id)?;
        let asset = self.get_audio_asset(&id)?;
        self.initialize_quality_pointers(&asset)?;
        Ok(asset)
    }

    pub fn list_tracks(&self) -> Result<Vec<TrackRecord>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, artist, title, version, user_rating, best_lossy_asset_id,
                    best_lossless_asset_id, best_verified_asset_id, nostalgia_asset_id,
                    created_at, updated_at
             FROM track_identities
             ORDER BY updated_at DESC, artist ASC, title ASC",
        )?;
        let identities = stmt
            .query_map([], |row| track_identity_from_row(row))?
            .collect::<rusqlite::Result<Vec<_>>>()?;

        identities
            .into_iter()
            .map(|identity| {
                Ok(TrackRecord {
                    assets: self.list_audio_assets(&identity.id)?,
                    tags: self.list_track_tags(&identity.id)?,
                    identity,
                })
            })
            .collect()
    }

    pub fn update_track_rating(
        &self,
        track_identity_id: &str,
        user_rating: Option<i64>,
    ) -> Result<TrackIdentity> {
        validate_rating(user_rating)?;
        self.ensure_track_identity_exists(track_identity_id)?;
        self.conn.execute(
            "UPDATE track_identities SET user_rating = ?1, updated_at = ?2 WHERE id = ?3",
            params![user_rating, now(), track_identity_id],
        )?;
        self.get_track_identity(track_identity_id)
    }

    pub fn add_track_tags(
        &self,
        track_identity_id: &str,
        labels: &[String],
    ) -> Result<Vec<SemanticTag>> {
        self.ensure_track_identity_exists(track_identity_id)?;
        let mut normalized = BTreeSet::new();
        for label in labels {
            if let Some(tag) = normalize_tag(label) {
                normalized.insert(tag);
            }
        }

        for normalized_label in normalized {
            let tag = self.find_or_create_tag(&normalized_label)?;
            self.conn.execute(
                "INSERT OR IGNORE INTO track_identity_tags (track_identity_id, tag_id)
                 VALUES (?1, ?2)",
                params![track_identity_id, tag.id],
            )?;
        }
        self.touch_track(track_identity_id)?;
        self.list_track_tags(track_identity_id)
    }

    pub fn replace_track_tags(
        &self,
        track_identity_id: &str,
        labels: &[String],
    ) -> Result<Vec<SemanticTag>> {
        self.ensure_track_identity_exists(track_identity_id)?;
        self.conn.execute(
            "DELETE FROM track_identity_tags WHERE track_identity_id = ?1",
            params![track_identity_id],
        )?;
        self.add_track_tags(track_identity_id, labels)
    }

    pub fn update_storage_state(
        &self,
        audio_asset_id: &str,
        storage_state: StorageState,
    ) -> Result<AudioAsset> {
        let asset = self.get_audio_asset(audio_asset_id)?;
        self.conn.execute(
            "UPDATE audio_assets SET storage_state = ?1, updated_at = ?2 WHERE id = ?3",
            params![storage_state.as_str(), now(), audio_asset_id],
        )?;
        self.touch_track(&asset.track_identity_id)?;
        self.get_audio_asset(audio_asset_id)
    }

    pub fn update_quality_pointers(
        &self,
        track_identity_id: &str,
        update: QualityPointerUpdate,
    ) -> Result<TrackIdentity> {
        self.ensure_track_identity_exists(track_identity_id)?;
        self.ensure_pointer_belongs_to_track(
            track_identity_id,
            update.best_lossy_asset_id.as_deref(),
        )?;
        self.ensure_pointer_belongs_to_track(
            track_identity_id,
            update.best_lossless_asset_id.as_deref(),
        )?;
        self.ensure_pointer_belongs_to_track(
            track_identity_id,
            update.best_verified_asset_id.as_deref(),
        )?;
        self.ensure_pointer_belongs_to_track(
            track_identity_id,
            update.nostalgia_asset_id.as_deref(),
        )?;
        self.ensure_lossy_pointer(update.best_lossy_asset_id.as_deref())?;
        self.ensure_lossless_pointer(update.best_lossless_asset_id.as_deref())?;
        self.ensure_verified_pointer(update.best_verified_asset_id.as_deref())?;

        self.conn.execute(
            "UPDATE track_identities
             SET best_lossy_asset_id = ?1,
                 best_lossless_asset_id = ?2,
                 best_verified_asset_id = ?3,
                 nostalgia_asset_id = ?4,
                 updated_at = ?5
             WHERE id = ?6",
            params![
                update.best_lossy_asset_id,
                update.best_lossless_asset_id,
                update.best_verified_asset_id,
                update.nostalgia_asset_id,
                now(),
                track_identity_id,
            ],
        )?;
        self.get_track_identity(track_identity_id)
    }

    pub fn select_playback_asset(
        &self,
        track_identity_id: &str,
        mode: PlaybackMode,
    ) -> Result<Option<AudioAsset>> {
        let identity = self.get_track_identity(track_identity_id)?;
        let candidate_id = match mode {
            PlaybackMode::Default => identity.best_verified_asset_id,
            PlaybackMode::Portable => identity.best_lossy_asset_id,
            PlaybackMode::Nostalgia => identity.nostalgia_asset_id.or_else(|| {
                self.find_asset_by_role(track_identity_id, RepresentationRole::Nostalgia)
                    .ok()
                    .flatten()
                    .map(|asset| asset.id)
            }),
        };

        match candidate_id {
            Some(asset_id) => {
                let asset = self.get_audio_asset(&asset_id)?;
                if asset.storage_state == StorageState::Local {
                    Ok(Some(asset))
                } else {
                    Ok(None)
                }
            }
            None => Ok(None),
        }
    }

    pub fn get_track_record(&self, track_identity_id: &str) -> Result<TrackRecord> {
        let identity = self.get_track_identity(track_identity_id)?;
        Ok(TrackRecord {
            assets: self.list_audio_assets(track_identity_id)?,
            tags: self.list_track_tags(track_identity_id)?,
            identity,
        })
    }

    fn migrate(&self) -> Result<()> {
        self.conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS app_metadata (
              key TEXT PRIMARY KEY,
              value TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS track_identities (
              id TEXT PRIMARY KEY,
              artist TEXT NOT NULL,
              title TEXT NOT NULL,
              version TEXT,
              user_rating INTEGER CHECK (user_rating BETWEEN 1 AND 5),
              best_lossy_asset_id TEXT,
              best_lossless_asset_id TEXT,
              best_verified_asset_id TEXT,
              nostalgia_asset_id TEXT,
              created_at TEXT NOT NULL,
              updated_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS audio_assets (
              id TEXT PRIMARY KEY,
              track_identity_id TEXT NOT NULL REFERENCES track_identities(id) ON DELETE RESTRICT,
              role TEXT NOT NULL CHECK (role IN ('first_found', 'nostalgia', 'variant')),
              storage_state TEXT NOT NULL CHECK (storage_state IN ('local', 'external', 'shadow', 'missing')),
              vault_path TEXT,
              original_path TEXT,
              original_filename TEXT,
              checksum TEXT,
              audio_fingerprint TEXT,
              format TEXT,
              bitrate_kbps INTEGER,
              sample_rate_hz INTEGER,
              duration_ms INTEGER,
              file_size INTEGER,
              dynamic_range REAL,
              integrated_lufs REAL,
              peak_db REAL,
              quality_score INTEGER CHECK (quality_score BETWEEN 0 AND 100),
              true_lossless_verified INTEGER NOT NULL CHECK (true_lossless_verified IN (0, 1)),
              suspected_transcode INTEGER NOT NULL CHECK (suspected_transcode IN (0, 1)),
              original_tags_json TEXT,
              created_at TEXT NOT NULL,
              updated_at TEXT NOT NULL,
              UNIQUE (checksum, file_size)
            );

            CREATE UNIQUE INDEX IF NOT EXISTS idx_one_first_found_per_track
              ON audio_assets(track_identity_id)
              WHERE role = 'first_found';

            CREATE TABLE IF NOT EXISTS semantic_tags (
              id TEXT PRIMARY KEY,
              label TEXT NOT NULL,
              normalized_label TEXT NOT NULL UNIQUE
            );

            CREATE TABLE IF NOT EXISTS track_identity_tags (
              track_identity_id TEXT NOT NULL REFERENCES track_identities(id) ON DELETE RESTRICT,
              tag_id TEXT NOT NULL REFERENCES semantic_tags(id) ON DELETE RESTRICT,
              PRIMARY KEY (track_identity_id, tag_id)
            );

            CREATE INDEX IF NOT EXISTS idx_audio_assets_track_identity_id
              ON audio_assets(track_identity_id);
            CREATE INDEX IF NOT EXISTS idx_track_identity_tags_tag_id
              ON track_identity_tags(tag_id);
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

    fn default_role_for_track(&self, track_identity_id: &str) -> Result<RepresentationRole> {
        let has_first_found: bool = self.conn.query_row(
            "SELECT EXISTS(
               SELECT 1 FROM audio_assets
               WHERE track_identity_id = ?1 AND role = 'first_found'
             )",
            params![track_identity_id],
            |row| row.get(0),
        )?;
        Ok(if has_first_found {
            RepresentationRole::Variant
        } else {
            RepresentationRole::FirstFound
        })
    }

    fn initialize_quality_pointers(&self, asset: &AudioAsset) -> Result<()> {
        let identity = self.get_track_identity(&asset.track_identity_id)?;
        let mut best_lossy_asset_id = identity.best_lossy_asset_id.clone();
        let mut best_lossless_asset_id = identity.best_lossless_asset_id.clone();
        let mut best_verified_asset_id = identity.best_verified_asset_id.clone();
        let mut nostalgia_asset_id = identity.nostalgia_asset_id.clone();

        if best_lossy_asset_id.is_none()
            && asset.is_lossy()
            && asset.storage_state == StorageState::Local
        {
            best_lossy_asset_id = Some(asset.id.clone());
        }
        if best_lossless_asset_id.is_none()
            && asset.is_lossless_container()
            && asset.true_lossless_verified
            && !asset.suspected_transcode
            && asset.storage_state == StorageState::Local
        {
            best_lossless_asset_id = Some(asset.id.clone());
        }
        if best_verified_asset_id.is_none()
            && asset.storage_state == StorageState::Local
            && (asset.is_lossy()
                || (asset.is_lossless_container()
                    && asset.true_lossless_verified
                    && !asset.suspected_transcode))
        {
            best_verified_asset_id = Some(asset.id.clone());
        }
        if nostalgia_asset_id.is_none()
            && asset.role == RepresentationRole::Nostalgia
            && asset.storage_state == StorageState::Local
        {
            nostalgia_asset_id = Some(asset.id.clone());
        }

        self.conn.execute(
            "UPDATE track_identities
             SET best_lossy_asset_id = ?1,
                 best_lossless_asset_id = ?2,
                 best_verified_asset_id = ?3,
                 nostalgia_asset_id = ?4,
                 updated_at = ?5
             WHERE id = ?6",
            params![
                best_lossy_asset_id,
                best_lossless_asset_id,
                best_verified_asset_id,
                nostalgia_asset_id,
                now(),
                asset.track_identity_id,
            ],
        )?;
        Ok(())
    }

    fn find_or_create_tag(&self, normalized_label: &str) -> Result<SemanticTag> {
        if let Some(tag) = self
            .conn
            .query_row(
                "SELECT id, label, normalized_label FROM semantic_tags WHERE normalized_label = ?1",
                params![normalized_label],
                |row| {
                    Ok(SemanticTag {
                        id: row.get(0)?,
                        label: row.get(1)?,
                        normalized_label: row.get(2)?,
                    })
                },
            )
            .optional()?
        {
            return Ok(tag);
        }

        let id = new_id();
        self.conn.execute(
            "INSERT INTO semantic_tags (id, label, normalized_label) VALUES (?1, ?2, ?2)",
            params![id, normalized_label],
        )?;
        Ok(SemanticTag {
            id,
            label: normalized_label.to_string(),
            normalized_label: normalized_label.to_string(),
        })
    }

    fn get_track_identity(&self, track_identity_id: &str) -> Result<TrackIdentity> {
        self.conn
            .query_row(
                "SELECT id, artist, title, version, user_rating, best_lossy_asset_id,
                        best_lossless_asset_id, best_verified_asset_id, nostalgia_asset_id,
                        created_at, updated_at
                 FROM track_identities WHERE id = ?1",
                params![track_identity_id],
                |row| track_identity_from_row(row),
            )
            .optional()?
            .ok_or_else(|| anyhow!("track identity not found: {track_identity_id}"))
    }

    fn get_audio_asset(&self, audio_asset_id: &str) -> Result<AudioAsset> {
        let sql = AUDIO_ASSET_SELECT.to_string() + " WHERE id = ?1";
        self.conn
            .query_row(&sql, params![audio_asset_id], |row| {
                audio_asset_from_row(row)
            })
            .optional()?
            .ok_or_else(|| anyhow!("audio asset not found: {audio_asset_id}"))
    }

    fn find_asset_by_role(
        &self,
        track_identity_id: &str,
        role: RepresentationRole,
    ) -> Result<Option<AudioAsset>> {
        let sql = AUDIO_ASSET_SELECT.to_string()
            + " WHERE track_identity_id = ?1 AND role = ?2 ORDER BY created_at ASC LIMIT 1";
        self.conn
            .query_row(&sql, params![track_identity_id, role.as_str()], |row| {
                audio_asset_from_row(row)
            })
            .optional()
            .map_err(Into::into)
    }

    fn list_audio_assets(&self, track_identity_id: &str) -> Result<Vec<AudioAsset>> {
        let mut stmt = self.conn.prepare(
            &(AUDIO_ASSET_SELECT.to_string()
                + " WHERE track_identity_id = ?1 ORDER BY created_at ASC"),
        )?;
        let assets = stmt
            .query_map(params![track_identity_id], |row| audio_asset_from_row(row))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(assets)
    }

    fn list_track_tags(&self, track_identity_id: &str) -> Result<Vec<SemanticTag>> {
        let mut stmt = self.conn.prepare(
            "SELECT semantic_tags.id, semantic_tags.label, semantic_tags.normalized_label
             FROM semantic_tags
             INNER JOIN track_identity_tags ON track_identity_tags.tag_id = semantic_tags.id
             WHERE track_identity_tags.track_identity_id = ?1
             ORDER BY semantic_tags.normalized_label ASC",
        )?;
        let tags = stmt
            .query_map(params![track_identity_id], |row| {
                Ok(SemanticTag {
                    id: row.get(0)?,
                    label: row.get(1)?,
                    normalized_label: row.get(2)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(tags)
    }

    fn ensure_track_identity_exists(&self, track_identity_id: &str) -> Result<()> {
        let exists: bool = self.conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM track_identities WHERE id = ?1)",
            params![track_identity_id],
            |row| row.get(0),
        )?;
        if exists {
            Ok(())
        } else {
            Err(anyhow!("track identity not found: {track_identity_id}"))
        }
    }

    fn ensure_pointer_belongs_to_track(
        &self,
        track_identity_id: &str,
        audio_asset_id: Option<&str>,
    ) -> Result<()> {
        let Some(audio_asset_id) = audio_asset_id else {
            return Ok(());
        };
        let asset = self.get_audio_asset(audio_asset_id)?;
        if asset.track_identity_id == track_identity_id {
            Ok(())
        } else {
            Err(anyhow!(
                "audio asset {audio_asset_id} does not belong to track identity {track_identity_id}"
            ))
        }
    }

    fn ensure_lossy_pointer(&self, audio_asset_id: Option<&str>) -> Result<()> {
        let Some(audio_asset_id) = audio_asset_id else {
            return Ok(());
        };
        let asset = self.get_audio_asset(audio_asset_id)?;
        if asset.is_lossy() {
            Ok(())
        } else {
            Err(anyhow!("best_lossy_asset_id must point to a lossy asset"))
        }
    }

    fn ensure_lossless_pointer(&self, audio_asset_id: Option<&str>) -> Result<()> {
        let Some(audio_asset_id) = audio_asset_id else {
            return Ok(());
        };
        let asset = self.get_audio_asset(audio_asset_id)?;
        if asset.is_lossless_container()
            && asset.true_lossless_verified
            && !asset.suspected_transcode
        {
            Ok(())
        } else {
            Err(anyhow!(
                "best_lossless_asset_id must point to verified true lossless audio"
            ))
        }
    }

    fn ensure_verified_pointer(&self, audio_asset_id: Option<&str>) -> Result<()> {
        let Some(audio_asset_id) = audio_asset_id else {
            return Ok(());
        };
        let asset = self.get_audio_asset(audio_asset_id)?;
        if asset.is_lossy()
            || (asset.is_lossless_container()
                && asset.true_lossless_verified
                && !asset.suspected_transcode)
        {
            Ok(())
        } else {
            Err(anyhow!(
                "best_verified_asset_id must point to lossy audio or verified true lossless audio"
            ))
        }
    }

    fn touch_track(&self, track_identity_id: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE track_identities SET updated_at = ?1 WHERE id = ?2",
            params![now(), track_identity_id],
        )?;
        Ok(())
    }

    fn vault_path_for_checksum(&self, checksum: &str, extension: Option<&str>) -> PathBuf {
        let mut filename = checksum.to_string();
        if let Some(extension) = extension {
            filename.push('.');
            filename.push_str(extension);
        }
        self.vault_root
            .join("originals")
            .join(&checksum[0..2])
            .join(filename)
    }
}

impl AudioAsset {
    pub fn is_lossy(&self) -> bool {
        matches!(
            self.format.as_deref(),
            Some("mp3" | "aac" | "m4a" | "ogg" | "opus" | "wma")
        )
    }

    pub fn is_lossless_container(&self) -> bool {
        matches!(
            self.format.as_deref(),
            Some("flac" | "wav" | "aiff" | "aif" | "alac")
        )
    }
}

const AUDIO_ASSET_SELECT: &str = "SELECT id, track_identity_id, role, storage_state, vault_path,
    original_path, original_filename, checksum, audio_fingerprint, format, bitrate_kbps,
    sample_rate_hz, duration_ms, file_size, dynamic_range, integrated_lufs, peak_db,
    quality_score, true_lossless_verified, suspected_transcode, original_tags_json,
    created_at, updated_at FROM audio_assets";

fn track_identity_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<TrackIdentity> {
    Ok(TrackIdentity {
        id: row.get(0)?,
        artist: row.get(1)?,
        title: row.get(2)?,
        version: row.get(3)?,
        user_rating: row.get(4)?,
        best_lossy_asset_id: row.get(5)?,
        best_lossless_asset_id: row.get(6)?,
        best_verified_asset_id: row.get(7)?,
        nostalgia_asset_id: row.get(8)?,
        created_at: row.get(9)?,
        updated_at: row.get(10)?,
    })
}

fn audio_asset_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<AudioAsset> {
    let role: String = row.get(2)?;
    let storage_state: String = row.get(3)?;
    let true_lossless_verified: i64 = row.get(18)?;
    let suspected_transcode: i64 = row.get(19)?;
    Ok(AudioAsset {
        id: row.get(0)?,
        track_identity_id: row.get(1)?,
        role: RepresentationRole::try_from(role.as_str()).map_err(to_sql_error)?,
        storage_state: StorageState::try_from(storage_state.as_str()).map_err(to_sql_error)?,
        vault_path: row.get(4)?,
        original_path: row.get(5)?,
        original_filename: row.get(6)?,
        checksum: row.get(7)?,
        audio_fingerprint: row.get(8)?,
        format: row.get(9)?,
        bitrate_kbps: row.get(10)?,
        sample_rate_hz: row.get(11)?,
        duration_ms: row.get(12)?,
        file_size: row.get(13)?,
        dynamic_range: row.get(14)?,
        integrated_lufs: row.get(15)?,
        peak_db: row.get(16)?,
        quality_score: row.get(17)?,
        true_lossless_verified: true_lossless_verified == 1,
        suspected_transcode: suspected_transcode == 1,
        original_tags_json: row.get(20)?,
        created_at: row.get(21)?,
        updated_at: row.get(22)?,
    })
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

fn interpret_filename(path: &Path) -> FilenameInterpretation {
    let stem = path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("Untitled Track");
    let mut parts = stem.split_whitespace().collect::<Vec<_>>();
    let mut tags = Vec::new();

    while let Some(part) = parts.last() {
        if is_suffix_hashtag(part) {
            let tag = parts.pop().unwrap().trim_start_matches('#').to_string();
            tags.push(tag);
        } else {
            break;
        }
    }
    tags.reverse();

    let clean_stem = parts.join(" ");
    let clean_stem = if clean_stem.trim().is_empty() {
        stem.to_string()
    } else {
        clean_stem.trim().to_string()
    };

    let (artist, title) = match clean_stem.split_once(" - ") {
        Some((artist, title)) => (
            Some(artist.trim().to_string()),
            Some(title.trim().to_string()),
        ),
        None => (None, Some(clean_stem.clone())),
    };

    FilenameInterpretation {
        artist,
        title,
        clean_stem,
        tags,
    }
}

fn is_suffix_hashtag(value: &str) -> bool {
    value.starts_with('#') && value.len() > 1 && value.chars().skip(1).all(is_tag_character)
}

fn is_tag_character(character: char) -> bool {
    character.is_alphanumeric() || matches!(character, '-' | '_' | '+')
}

fn normalize_tag(label: &str) -> Option<String> {
    let trimmed = label.trim().trim_start_matches('#').trim();
    if trimmed.is_empty() {
        return None;
    }
    let normalized = trimmed
        .chars()
        .filter_map(|character| {
            if is_tag_character(character) {
                Some(character.to_lowercase().collect::<String>())
            } else if character.is_whitespace() {
                Some("-".to_string())
            } else {
                None
            }
        })
        .collect::<String>();

    if normalized.is_empty() {
        None
    } else {
        Some(normalized)
    }
}

fn clean_required(value: &str, field_name: &str) -> Result<String> {
    let cleaned = value.split_whitespace().collect::<Vec<_>>().join(" ");
    if cleaned.is_empty() {
        Err(anyhow!("{field_name} must not be empty"))
    } else {
        Ok(cleaned)
    }
}

fn validate_rating(value: Option<i64>) -> Result<()> {
    match value {
        Some(value) if !(1..=5).contains(&value) => {
            Err(anyhow!("user_rating must be between 1 and 5"))
        }
        _ => Ok(()),
    }
}

fn validate_quality_score(value: Option<i64>) -> Result<()> {
    match value {
        Some(value) if !(0..=100).contains(&value) => {
            Err(anyhow!("quality_score must be between 0 and 100"))
        }
        _ => Ok(()),
    }
}

fn normalize_format(format: &str) -> String {
    format.trim().trim_start_matches('.').to_ascii_lowercase()
}

fn bool_to_int(value: bool) -> i64 {
    if value {
        1
    } else {
        0
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

    fn create_identity(archive: &Archive) -> TrackIdentity {
        archive
            .create_track_identity(CreateTrackIdentityRequest {
                artist: "Eminem".to_string(),
                title: "Stan".to_string(),
                version: None,
                user_rating: Some(5),
                semantic_tags: vec!["storytelling".to_string()],
            })
            .expect("identity")
    }

    fn register_asset(
        archive: &Archive,
        track_identity_id: &str,
        role: Option<RepresentationRole>,
        format: &str,
        true_lossless_verified: bool,
        suspected_transcode: bool,
    ) -> AudioAsset {
        archive
            .register_audio_asset(RegisterAudioAssetRequest {
                track_identity_id: track_identity_id.to_string(),
                role,
                storage_state: StorageState::Local,
                vault_path: Some(format!("/vault/{format}/{}", new_id())),
                original_path: None,
                original_filename: None,
                checksum: Some(new_id()),
                audio_fingerprint: Some(format!("fp-{format}-{}", new_id())),
                format: Some(format.to_string()),
                bitrate_kbps: if format == "mp3" { Some(320) } else { None },
                sample_rate_hz: Some(44_100),
                duration_ms: Some(404_000),
                file_size: Some(1_000_000),
                dynamic_range: Some(9.0),
                integrated_lufs: Some(-14.0),
                peak_db: Some(-1.0),
                quality_score: Some(80),
                true_lossless_verified,
                suspected_transcode,
                original_tags_json: None,
            })
            .expect("asset")
    }

    #[test]
    fn first_found_role_is_assigned_only_once_by_default() {
        let (_test_dir, archive) = archive();
        let identity = create_identity(&archive);

        let first = register_asset(&archive, &identity.id, None, "mp3", false, false);
        let second = register_asset(&archive, &identity.id, None, "mp3", false, false);

        assert_eq!(first.role, RepresentationRole::FirstFound);
        assert_eq!(second.role, RepresentationRole::Variant);
    }

    #[test]
    fn multiple_variants_can_belong_to_same_track_identity() {
        let (_test_dir, archive) = archive();
        let identity = create_identity(&archive);

        register_asset(&archive, &identity.id, None, "mp3", false, false);
        register_asset(
            &archive,
            &identity.id,
            Some(RepresentationRole::Variant),
            "flac",
            true,
            false,
        );
        register_asset(
            &archive,
            &identity.id,
            Some(RepresentationRole::Nostalgia),
            "mp3",
            false,
            false,
        );

        let record = archive.get_track_record(&identity.id).expect("record");
        assert_eq!(record.assets.len(), 3);
        assert!(record
            .assets
            .iter()
            .any(|asset| asset.role == RepresentationRole::Nostalgia));
    }

    #[test]
    fn shadow_state_does_not_delete_identity_or_global_rating() {
        let (_test_dir, archive) = archive();
        let identity = create_identity(&archive);
        let asset = register_asset(&archive, &identity.id, None, "mp3", false, false);

        archive
            .update_storage_state(&asset.id, StorageState::Shadow)
            .expect("shadow");

        let record = archive.get_track_record(&identity.id).expect("record");
        assert_eq!(record.identity.user_rating, Some(5));
        assert_eq!(record.assets[0].storage_state, StorageState::Shadow);
    }

    #[test]
    fn best_lossy_and_best_lossless_can_both_exist() {
        let (_test_dir, archive) = archive();
        let identity = create_identity(&archive);
        let lossy = register_asset(&archive, &identity.id, None, "mp3", false, false);
        let lossless = register_asset(&archive, &identity.id, None, "flac", true, false);

        let updated = archive
            .update_quality_pointers(
                &identity.id,
                QualityPointerUpdate {
                    best_lossy_asset_id: Some(lossy.id.clone()),
                    best_lossless_asset_id: Some(lossless.id.clone()),
                    best_verified_asset_id: Some(lossless.id.clone()),
                    nostalgia_asset_id: None,
                },
            )
            .expect("quality pointers");

        assert_eq!(
            updated.best_lossy_asset_id.as_deref(),
            Some(lossy.id.as_str())
        );
        assert_eq!(
            updated.best_lossless_asset_id.as_deref(),
            Some(lossless.id.as_str())
        );
    }

    #[test]
    fn best_verified_can_point_to_lossy_when_lossless_is_fake_or_unverified() {
        let (_test_dir, archive) = archive();
        let identity = create_identity(&archive);
        let lossy = register_asset(&archive, &identity.id, None, "mp3", false, false);
        let fake_flac = register_asset(&archive, &identity.id, None, "flac", false, true);

        let updated = archive
            .update_quality_pointers(
                &identity.id,
                QualityPointerUpdate {
                    best_lossy_asset_id: Some(lossy.id.clone()),
                    best_lossless_asset_id: None,
                    best_verified_asset_id: Some(lossy.id.clone()),
                    nostalgia_asset_id: None,
                },
            )
            .expect("quality pointers");

        assert_eq!(
            updated.best_verified_asset_id.as_deref(),
            Some(lossy.id.as_str())
        );
        assert!(updated.best_lossless_asset_id.is_none());
        assert!(fake_flac.is_lossless_container());
    }

    #[test]
    fn fake_or_unverified_lossless_cannot_be_marked_best_verified() {
        let (_test_dir, archive) = archive();
        let identity = create_identity(&archive);
        let fake_flac = register_asset(&archive, &identity.id, None, "flac", false, true);

        let error = archive
            .update_quality_pointers(
                &identity.id,
                QualityPointerUpdate {
                    best_lossy_asset_id: None,
                    best_lossless_asset_id: None,
                    best_verified_asset_id: Some(fake_flac.id.clone()),
                    nostalgia_asset_id: None,
                },
            )
            .expect_err("fake flac should not become best verified");

        assert!(error.to_string().contains("best_verified_asset_id"));
    }

    #[test]
    fn global_rating_applies_to_all_references_of_track_identity() {
        let (_test_dir, archive) = archive();
        let identity = create_identity(&archive);
        register_asset(&archive, &identity.id, None, "mp3", false, false);
        register_asset(&archive, &identity.id, None, "flac", true, false);

        let updated = archive
            .update_track_rating(&identity.id, Some(4))
            .expect("rating");
        let record = archive.get_track_record(&identity.id).expect("record");

        assert_eq!(updated.user_rating, Some(4));
        assert_eq!(record.identity.user_rating, Some(4));
        assert_eq!(record.assets.len(), 2);
    }

    #[test]
    fn semantic_tags_persist_on_track_identity_without_history() {
        let (_test_dir, archive) = archive();
        let identity = archive
            .create_track_identity(CreateTrackIdentityRequest {
                artist: "Nena".to_string(),
                title: "99 Luftballons".to_string(),
                version: None,
                user_rating: Some(4),
                semantic_tags: vec![
                    "#deutsch".to_string(),
                    "#80s".to_string(),
                    "#party".to_string(),
                ],
            })
            .expect("identity");

        let record = archive.get_track_record(&identity.id).expect("record");
        let tags = record
            .tags
            .iter()
            .map(|tag| tag.normalized_label.as_str())
            .collect::<Vec<_>>();

        assert_eq!(tags, vec!["80s", "deutsch", "party"]);
    }

    #[test]
    fn nostalgia_playback_selects_nostalgia_asset_instead_of_best_verified() {
        let (_test_dir, archive) = archive();
        let identity = create_identity(&archive);
        let best = register_asset(&archive, &identity.id, None, "flac", true, false);
        let nostalgia = register_asset(
            &archive,
            &identity.id,
            Some(RepresentationRole::Nostalgia),
            "mp3",
            false,
            false,
        );

        archive
            .update_quality_pointers(
                &identity.id,
                QualityPointerUpdate {
                    best_lossy_asset_id: Some(nostalgia.id.clone()),
                    best_lossless_asset_id: Some(best.id.clone()),
                    best_verified_asset_id: Some(best.id.clone()),
                    nostalgia_asset_id: Some(nostalgia.id.clone()),
                },
            )
            .expect("quality pointers");

        let default_asset = archive
            .select_playback_asset(&identity.id, PlaybackMode::Default)
            .expect("default")
            .expect("default asset");
        let nostalgia_asset = archive
            .select_playback_asset(&identity.id, PlaybackMode::Nostalgia)
            .expect("nostalgia")
            .expect("nostalgia asset");

        assert_eq!(default_asset.id, best.id);
        assert_eq!(nostalgia_asset.id, nostalgia.id);
    }

    #[test]
    fn import_extracts_suffix_hashtags_to_track_identity_and_cleans_title() {
        let (test_dir, archive) = archive();
        let source = test_dir
            .path()
            .join("Eminem - Stan #2000 #hip-hop #herzschmerz.mp3");
        fs::write(&source, b"fake mp3 bytes").expect("source");

        let result = archive
            .import_audio_file(ImportAudioRequest {
                source_path: source.clone(),
                track_identity_id: None,
                title: None,
                artist: None,
                version: None,
                role: None,
                user_rating: Some(5),
                semantic_tags: vec!["storytelling".to_string()],
                original_tags_json: Some(r#"{"title":"stand"}"#.to_string()),
            })
            .expect("import");

        let record = archive
            .get_track_record(&result.track_identity.id)
            .expect("record");
        let tags = record
            .tags
            .iter()
            .map(|tag| tag.normalized_label.as_str())
            .collect::<Vec<_>>();

        assert_eq!(record.identity.artist, "Eminem");
        assert_eq!(record.identity.title, "Stan");
        assert_eq!(tags, vec!["2000", "herzschmerz", "hip-hop", "storytelling"]);
        assert_eq!(
            record.assets[0].original_filename.as_deref(),
            Some("Eminem - Stan #2000 #hip-hop #herzschmerz.mp3")
        );
        assert_eq!(fs::read(&source).expect("source bytes"), b"fake mp3 bytes");
    }

    #[test]
    fn hashtags_inside_title_are_not_treated_as_suffix_tags() {
        let interpretation = interpret_filename(Path::new("Artist - Song #1 Live.mp3"));

        assert_eq!(interpretation.title.as_deref(), Some("Song #1 Live"));
        assert!(interpretation.tags.is_empty());
    }
}
