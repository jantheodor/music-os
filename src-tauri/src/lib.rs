use music_os_core::{
    Archive, ArchiveState, ImportAudioRequest, ImportAudioResult, RepresentationRole, TrackRatings,
    TrackRecord, TrackRepresentation,
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
    track_id: Option<String>,
    title: Option<String>,
    artist: Option<String>,
    album_title: Option<String>,
    album_artist: Option<String>,
    role: RepresentationRole,
    music_rating: Option<i64>,
    file_quality_rating: Option<i64>,
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
            track_id: request.track_id,
            title: request.title,
            artist: request.artist,
            album_title: request.album_title,
            album_artist: request.album_artist,
            role: request.role,
            music_rating: request.music_rating,
            file_quality_rating: request.file_quality_rating,
        })
        .map_err(to_command_error)
}

#[tauri::command]
fn update_track_ratings(
    state: State<'_, MusicOsState>,
    track_id: String,
    music_rating: Option<i64>,
    file_quality_rating: Option<i64>,
    notes: Option<String>,
) -> Result<TrackRatings, String> {
    let archive = state
        .archive
        .lock()
        .map_err(|_| "archive lock poisoned".to_string())?;
    archive
        .update_track_ratings(
            &track_id,
            music_rating,
            file_quality_rating,
            notes.as_deref(),
        )
        .map_err(to_command_error)
}

#[tauri::command]
fn set_track_archive_state(
    state: State<'_, MusicOsState>,
    track_id: String,
    archive_state: ArchiveState,
) -> Result<TrackRecord, String> {
    let archive = state
        .archive
        .lock()
        .map_err(|_| "archive lock poisoned".to_string())?;
    archive
        .set_track_archive_state(&track_id, archive_state)
        .and_then(|_| {
            archive
                .list_tracks()?
                .into_iter()
                .find(|record| record.track.id == track_id)
                .ok_or_else(|| anyhow::anyhow!("track not found after state update"))
        })
        .map_err(to_command_error)
}

#[tauri::command]
fn create_shadow_entry(
    state: State<'_, MusicOsState>,
    track_id: String,
    label: Option<String>,
    source_path: Option<String>,
    fingerprint: Option<String>,
    notes: Option<String>,
) -> Result<TrackRepresentation, String> {
    let archive = state
        .archive
        .lock()
        .map_err(|_| "archive lock poisoned".to_string())?;
    archive
        .create_shadow_entry(
            &track_id,
            label.as_deref(),
            source_path.as_deref(),
            fingerprint.as_deref(),
            notes.as_deref(),
        )
        .map_err(to_command_error)
}

#[tauri::command]
fn set_representation_role(
    state: State<'_, MusicOsState>,
    representation_id: String,
    role: RepresentationRole,
) -> Result<TrackRepresentation, String> {
    let archive = state
        .archive
        .lock()
        .map_err(|_| "archive lock poisoned".to_string())?;
    archive
        .set_representation_role(&representation_id, role)
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
            update_track_ratings,
            set_track_archive_state,
            create_shadow_entry,
            set_representation_role,
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
