use music_os_core::{
    Archive, AudioAsset, ImportAudioRequest, ImportAudioResult, PlaybackMode, QualityPointerUpdate,
    RepresentationRole, StorageState, TrackIdentity, TrackRecord,
};
use serde::Deserialize;
use std::path::PathBuf;
use std::sync::Mutex;
use tauri::{AppHandle, Manager, State};

struct MusicOsState {
    archive: Mutex<Archive>,
}

#[derive(Debug, Deserialize)]
struct ImportMusicFileCommand {
    source_path: PathBuf,
    track_identity_id: Option<String>,
    title: Option<String>,
    artist: Option<String>,
    version: Option<String>,
    role: Option<RepresentationRole>,
    user_rating: Option<i64>,
    semantic_tags: Vec<String>,
    original_tags_json: Option<String>,
}

#[tauri::command]
fn list_tracks(state: State<'_, MusicOsState>) -> Result<Vec<TrackRecord>, String> {
    let archive = state
        .archive
        .lock()
        .map_err(|_| "archive lock poisoned".to_string())?;
    archive.list_tracks().map_err(to_command_error)
}

#[tauri::command]
fn import_music_file(
    state: State<'_, MusicOsState>,
    request: ImportMusicFileCommand,
) -> Result<ImportAudioResult, String> {
    let archive = state
        .archive
        .lock()
        .map_err(|_| "archive lock poisoned".to_string())?;
    archive
        .import_audio_file(ImportAudioRequest {
            source_path: request.source_path,
            track_identity_id: request.track_identity_id,
            title: request.title,
            artist: request.artist,
            version: request.version,
            role: request.role,
            user_rating: request.user_rating,
            semantic_tags: request.semantic_tags,
            original_tags_json: request.original_tags_json,
        })
        .map_err(to_command_error)
}

#[tauri::command]
fn update_track_rating(
    state: State<'_, MusicOsState>,
    track_identity_id: String,
    user_rating: Option<i64>,
) -> Result<TrackIdentity, String> {
    let archive = state
        .archive
        .lock()
        .map_err(|_| "archive lock poisoned".to_string())?;
    archive
        .update_track_rating(&track_identity_id, user_rating)
        .map_err(to_command_error)
}

#[tauri::command]
fn replace_track_tags(
    state: State<'_, MusicOsState>,
    track_identity_id: String,
    semantic_tags: Vec<String>,
) -> Result<TrackRecord, String> {
    let archive = state
        .archive
        .lock()
        .map_err(|_| "archive lock poisoned".to_string())?;
    archive
        .replace_track_tags(&track_identity_id, &semantic_tags)
        .and_then(|_| archive.get_track_record(&track_identity_id))
        .map_err(to_command_error)
}

#[tauri::command]
fn update_storage_state(
    state: State<'_, MusicOsState>,
    audio_asset_id: String,
    storage_state: StorageState,
) -> Result<AudioAsset, String> {
    let archive = state
        .archive
        .lock()
        .map_err(|_| "archive lock poisoned".to_string())?;
    archive
        .update_storage_state(&audio_asset_id, storage_state)
        .map_err(to_command_error)
}

#[tauri::command]
fn update_quality_pointers(
    state: State<'_, MusicOsState>,
    track_identity_id: String,
    update: QualityPointerUpdate,
) -> Result<TrackIdentity, String> {
    let archive = state
        .archive
        .lock()
        .map_err(|_| "archive lock poisoned".to_string())?;
    archive
        .update_quality_pointers(&track_identity_id, update)
        .map_err(to_command_error)
}

#[tauri::command]
fn select_playback_asset(
    state: State<'_, MusicOsState>,
    track_identity_id: String,
    mode: PlaybackMode,
) -> Result<Option<AudioAsset>, String> {
    let archive = state
        .archive
        .lock()
        .map_err(|_| "archive lock poisoned".to_string())?;
    archive
        .select_playback_asset(&track_identity_id, mode)
        .map_err(to_command_error)
}

pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let (db_path, vault_root) = archive_paths(app.handle())?;
            let archive = Archive::open(db_path, vault_root).map_err(to_boxed_error)?;
            app.manage(MusicOsState {
                archive: Mutex::new(archive),
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            list_tracks,
            import_music_file,
            update_track_rating,
            replace_track_tags,
            update_storage_state,
            update_quality_pointers,
            select_playback_asset,
        ])
        .run(tauri::generate_context!())
        .expect("failed to run Music OS");
}

fn archive_paths(app_handle: &AppHandle) -> Result<(PathBuf, PathBuf), Box<dyn std::error::Error>> {
    let data_dir = app_handle.path().app_data_dir()?;
    let db_path = data_dir.join("library.sqlite");
    let vault_root = data_dir.join("vault");
    Ok((db_path, vault_root))
}

fn to_command_error(error: anyhow::Error) -> String {
    error.to_string()
}

fn to_boxed_error(error: anyhow::Error) -> Box<dyn std::error::Error> {
    error.into()
}
