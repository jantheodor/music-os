import { useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

type ArchiveState =
  | "active"
  | "recall"
  | "shadow"
  | "historical"
  | "replaceable"
  | "archived";

type RepresentationRole =
  | "discovery"
  | "nostalgia"
  | "preferred_technical"
  | "historical_variant"
  | "shadow";

interface Track {
  id: string;
  canonical_title: string;
  canonical_artist?: string | null;
  archive_state: ArchiveState;
  created_at: string;
  updated_at: string;
}

interface TrackRatings {
  track_id: string;
  music_rating?: number | null;
  file_quality_rating?: number | null;
  notes?: string | null;
  updated_at: string;
}

interface AlbumContext {
  id: string;
  title: string;
  album_artist?: string | null;
  preservation_state: string;
}

interface TrackRepresentation {
  id: string;
  track_id: string;
  file_id?: string | null;
  role: RepresentationRole;
  label?: string | null;
  source_path?: string | null;
  is_available: boolean;
  technical_score?: number | null;
  quality_notes?: string | null;
  created_at: string;
}

interface HistoryEvent {
  id: string;
  track_id: string;
  representation_id?: string | null;
  event_type: string;
  summary: string;
  payload_json?: string | null;
  created_at: string;
}

interface TrackRecord {
  track: Track;
  ratings?: TrackRatings | null;
  albums: AlbumContext[];
  representations: TrackRepresentation[];
  history: HistoryEvent[];
}

interface ImportForm {
  sourcePath: string;
  title: string;
  artist: string;
  albumTitle: string;
  albumArtist: string;
  role: RepresentationRole;
  musicRating: string;
  fileQualityRating: string;
}

const archiveStates: ArchiveState[] = [
  "active",
  "recall",
  "shadow",
  "historical",
  "replaceable",
  "archived",
];

const representationRoles: RepresentationRole[] = [
  "discovery",
  "nostalgia",
  "preferred_technical",
  "historical_variant",
  "shadow",
];

const initialImportForm: ImportForm = {
  sourcePath: "",
  title: "",
  artist: "",
  albumTitle: "",
  albumArtist: "",
  role: "discovery",
  musicRating: "",
  fileQualityRating: "",
};

function App() {
  const [tracks, setTracks] = useState<TrackRecord[]>([]);
  const [selectedTrackId, setSelectedTrackId] = useState<string | null>(null);
  const [importForm, setImportForm] = useState<ImportForm>(initialImportForm);
  const [ratingDraft, setRatingDraft] = useState({
    musicRating: "",
    fileQualityRating: "",
    notes: "",
  });
  const [shadowDraft, setShadowDraft] = useState({
    label: "",
    sourcePath: "",
    fingerprint: "",
    notes: "",
  });
  const [status, setStatus] = useState("Ready to preserve, not overwrite.");
  const [error, setError] = useState<string | null>(null);

  const selectedTrack = useMemo(
    () => tracks.find((record) => record.track.id === selectedTrackId) ?? tracks[0],
    [selectedTrackId, tracks],
  );

  useEffect(() => {
    void refreshTracks();
  }, []);

  useEffect(() => {
    if (!selectedTrack) {
      return;
    }
    setRatingDraft({
      musicRating: selectedTrack.ratings?.music_rating?.toString() ?? "",
      fileQualityRating: selectedTrack.ratings?.file_quality_rating?.toString() ?? "",
      notes: selectedTrack.ratings?.notes ?? "",
    });
  }, [selectedTrack?.track.id]);

  async function refreshTracks() {
    try {
      const records = await invoke<TrackRecord[]>("list_tracks");
      setTracks(records);
      setSelectedTrackId((current) => current ?? records[0]?.track.id ?? null);
      setError(null);
    } catch (caught) {
      setError(formatError(caught));
    }
  }

  async function importTrack(event: React.FormEvent<HTMLFormElement>) {
    event.preventDefault();
    setStatus("Importing into checksum vault...");
    setError(null);
    try {
      await invoke("import_music_file", {
        request: {
          source_path: importForm.sourcePath,
          title: optional(importForm.title),
          artist: optional(importForm.artist),
          album_title: optional(importForm.albumTitle),
          album_artist: optional(importForm.albumArtist),
          role: importForm.role,
          music_rating: optionalNumber(importForm.musicRating),
          file_quality_rating: optionalNumber(importForm.fileQualityRating),
        },
      });
      setImportForm(initialImportForm);
      setStatus("Import complete. Source file was left untouched.");
      await refreshTracks();
    } catch (caught) {
      setError(formatError(caught));
      setStatus("Import failed.");
    }
  }

  async function updateRatings(event: React.FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (!selectedTrack) {
      return;
    }
    try {
      await invoke("update_track_ratings", {
        trackId: selectedTrack.track.id,
        musicRating: optionalNumber(ratingDraft.musicRating),
        fileQualityRating: optionalNumber(ratingDraft.fileQualityRating),
        notes: optional(ratingDraft.notes),
      });
      setStatus("Ratings updated as separate music and file-quality signals.");
      await refreshTracks();
    } catch (caught) {
      setError(formatError(caught));
    }
  }

  async function updateArchiveState(archiveState: ArchiveState) {
    if (!selectedTrack) {
      return;
    }
    try {
      await invoke("set_track_archive_state", {
        trackId: selectedTrack.track.id,
        archiveState,
      });
      setStatus(`Track moved to ${label(archiveState)}.`);
      await refreshTracks();
    } catch (caught) {
      setError(formatError(caught));
    }
  }

  async function createShadowEntry(event: React.FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (!selectedTrack) {
      return;
    }
    try {
      await invoke("create_shadow_entry", {
        trackId: selectedTrack.track.id,
        label: optional(shadowDraft.label),
        sourcePath: optional(shadowDraft.sourcePath),
        fingerprint: optional(shadowDraft.fingerprint),
        notes: optional(shadowDraft.notes),
      });
      setShadowDraft({ label: "", sourcePath: "", fingerprint: "", notes: "" });
      setStatus("Shadow entry created without requiring local audio.");
      await refreshTracks();
    } catch (caught) {
      setError(formatError(caught));
    }
  }

  async function updateRepresentationRole(
    representationId: string,
    role: RepresentationRole,
  ) {
    try {
      await invoke("set_representation_role", {
        representationId,
        role,
      });
      setStatus(`Representation marked as ${label(role)}.`);
      await refreshTracks();
    } catch (caught) {
      setError(formatError(caught));
    }
  }

  return (
    <main className="shell">
      <section className="hero">
        <div>
          <p className="eyebrow">Open, local-first music archive</p>
          <h1>Music OS</h1>
          <p>
            A non-destructive library manager for music as artifacts, memories,
            variants, album contexts, and histories.
          </p>
        </div>
        <div className="principles">
          <span>Original files are sacred</span>
          <span>Tracks are entities, not files</span>
          <span>History is narrative, not clutter</span>
        </div>
      </section>

      <section className="notice" aria-live="polite">
        <strong>Status:</strong> {status}
        {error && <p className="error">{error}</p>}
      </section>

      <div className="grid">
        <section className="card">
          <h2>Import into Vault</h2>
          <p className="muted">
            Import copies bytes into immutable checksum storage and records the
            original path. No source file edits or destructive normalization occur.
          </p>
          <form className="stack" onSubmit={importTrack}>
            <label>
              Source file path
              <input
                required
                value={importForm.sourcePath}
                onChange={(event) =>
                  setImportForm({ ...importForm, sourcePath: event.target.value })
                }
                placeholder="/home/me/Music/Artist - Song.flac"
              />
            </label>
            <div className="two-column">
              <label>
                Title
                <input
                  value={importForm.title}
                  onChange={(event) =>
                    setImportForm({ ...importForm, title: event.target.value })
                  }
                  placeholder="Infer from filename"
                />
              </label>
              <label>
                Artist
                <input
                  value={importForm.artist}
                  onChange={(event) =>
                    setImportForm({ ...importForm, artist: event.target.value })
                  }
                  placeholder="Infer from filename"
                />
              </label>
            </div>
            <div className="two-column">
              <label>
                Album
                <input
                  value={importForm.albumTitle}
                  onChange={(event) =>
                    setImportForm({ ...importForm, albumTitle: event.target.value })
                  }
                  placeholder="Optional album context"
                />
              </label>
              <label>
                Album artist
                <input
                  value={importForm.albumArtist}
                  onChange={(event) =>
                    setImportForm({ ...importForm, albumArtist: event.target.value })
                  }
                />
              </label>
            </div>
            <div className="three-column">
              <label>
                Representation
                <select
                  value={importForm.role}
                  onChange={(event) =>
                    setImportForm({
                      ...importForm,
                      role: event.target.value as RepresentationRole,
                    })
                  }
                >
                  {representationRoles.map((role) => (
                    <option key={role} value={role}>
                      {label(role)}
                    </option>
                  ))}
                </select>
              </label>
              <label>
                Music rating
                <input
                  type="number"
                  min="0"
                  max="5"
                  value={importForm.musicRating}
                  onChange={(event) =>
                    setImportForm({ ...importForm, musicRating: event.target.value })
                  }
                />
              </label>
              <label>
                File quality
                <input
                  type="number"
                  min="0"
                  max="5"
                  value={importForm.fileQualityRating}
                  onChange={(event) =>
                    setImportForm({
                      ...importForm,
                      fileQualityRating: event.target.value,
                    })
                  }
                />
              </label>
            </div>
            <button type="submit">Import without destructive changes</button>
          </form>
        </section>

        <section className="card">
          <h2>Library</h2>
          {tracks.length === 0 ? (
            <p className="empty">
              No tracks yet. Import a local audio file path to create the first
              entity and vault representation.
            </p>
          ) : (
            <div className="track-list">
              {tracks.map((record) => (
                <button
                  className={
                    record.track.id === selectedTrack?.track.id
                      ? "track-row selected"
                      : "track-row"
                  }
                  key={record.track.id}
                  type="button"
                  onClick={() => setSelectedTrackId(record.track.id)}
                >
                  <span>
                    <strong>{record.track.canonical_title}</strong>
                    <small>{record.track.canonical_artist ?? "Unknown artist"}</small>
                  </span>
                  <span className={`state ${record.track.archive_state}`}>
                    {label(record.track.archive_state)}
                  </span>
                </button>
              ))}
            </div>
          )}
        </section>
      </div>

      {selectedTrack && (
        <section className="detail card">
          <div className="detail-header">
            <div>
              <p className="eyebrow">Track entity</p>
              <h2>{selectedTrack.track.canonical_title}</h2>
              <p>{selectedTrack.track.canonical_artist ?? "Unknown artist"}</p>
            </div>
            <label>
              Recall state
              <select
                value={selectedTrack.track.archive_state}
                onChange={(event) =>
                  void updateArchiveState(event.target.value as ArchiveState)
                }
              >
                {archiveStates.map((state) => (
                  <option key={state} value={state}>
                    {label(state)}
                  </option>
                ))}
              </select>
            </label>
          </div>

          <div className="detail-grid">
            <section>
              <h3>Ratings</h3>
              <form className="stack" onSubmit={updateRatings}>
                <div className="two-column">
                  <label>
                    Music appreciation
                    <input
                      type="number"
                      min="0"
                      max="5"
                      value={ratingDraft.musicRating}
                      onChange={(event) =>
                        setRatingDraft({
                          ...ratingDraft,
                          musicRating: event.target.value,
                        })
                      }
                    />
                  </label>
                  <label>
                    File quality
                    <input
                      type="number"
                      min="0"
                      max="5"
                      value={ratingDraft.fileQualityRating}
                      onChange={(event) =>
                        setRatingDraft({
                          ...ratingDraft,
                          fileQualityRating: event.target.value,
                        })
                      }
                    />
                  </label>
                </div>
                <label>
                  Notes
                  <textarea
                    value={ratingDraft.notes}
                    onChange={(event) =>
                      setRatingDraft({ ...ratingDraft, notes: event.target.value })
                    }
                    placeholder="Great song, poor rip; keep memory, seek better copy."
                  />
                </label>
                <button type="submit">Save ratings</button>
              </form>
            </section>

            <section>
              <h3>Album context</h3>
              {selectedTrack.albums.length === 0 ? (
                <p className="empty">No album context recorded yet.</p>
              ) : (
                <ul className="pill-list">
                  {selectedTrack.albums.map((album) => (
                    <li key={album.id}>
                      {album.title}
                      {album.album_artist ? ` by ${album.album_artist}` : ""}
                    </li>
                  ))}
                </ul>
              )}
            </section>
          </div>

          <div className="detail-grid">
            <section>
              <h3>Representations</h3>
              <div className="representation-list">
                {selectedTrack.representations.map((representation) => (
                  <article key={representation.id} className="representation">
                    <div>
                      <strong>{label(representation.role)}</strong>
                      <p>
                        {representation.is_available
                          ? "Local vault-backed audio"
                          : "Shadow-only memory"}
                      </p>
                      {representation.source_path && (
                        <small>{representation.source_path}</small>
                      )}
                    </div>
                    <select
                      value={representation.role}
                      onChange={(event) =>
                        void updateRepresentationRole(
                          representation.id,
                          event.target.value as RepresentationRole,
                        )
                      }
                    >
                      {representationRoles.map((role) => (
                        <option key={role} value={role}>
                          {label(role)}
                        </option>
                      ))}
                    </select>
                  </article>
                ))}
              </div>
            </section>

            <section>
              <h3>Create shadow entry</h3>
              <form className="stack" onSubmit={createShadowEntry}>
                <label>
                  Label
                  <input
                    value={shadowDraft.label}
                    onChange={(event) =>
                      setShadowDraft({ ...shadowDraft, label: event.target.value })
                    }
                    placeholder="Old iPod copy"
                  />
                </label>
                <label>
                  Remembered path or source
                  <input
                    value={shadowDraft.sourcePath}
                    onChange={(event) =>
                      setShadowDraft({
                        ...shadowDraft,
                        sourcePath: event.target.value,
                      })
                    }
                    placeholder="/Volumes/old-drive/song.mp3"
                  />
                </label>
                <label>
                  Fingerprint
                  <input
                    value={shadowDraft.fingerprint}
                    onChange={(event) =>
                      setShadowDraft({
                        ...shadowDraft,
                        fingerprint: event.target.value,
                      })
                    }
                    placeholder="Future acoustic fingerprint"
                  />
                </label>
                <label>
                  Notes
                  <textarea
                    value={shadowDraft.notes}
                    onChange={(event) =>
                      setShadowDraft({ ...shadowDraft, notes: event.target.value })
                    }
                  />
                </label>
                <button type="submit">Preserve shadow memory</button>
              </form>
            </section>
          </div>

          <section>
            <h3>History</h3>
            <ol className="timeline">
              {selectedTrack.history.map((event) => (
                <li key={event.id}>
                  <span>{formatTimestamp(event.created_at)}</span>
                  <strong>{label(event.event_type)}</strong>
                  <p>{event.summary}</p>
                </li>
              ))}
            </ol>
          </section>
        </section>
      )}
    </main>
  );
}

function optional(value: string) {
  const trimmed = value.trim();
  return trimmed.length > 0 ? trimmed : null;
}

function optionalNumber(value: string) {
  const trimmed = value.trim();
  return trimmed.length > 0 ? Number(trimmed) : null;
}

function label(value: string) {
  return value.replace(/_/g, " ");
}

function formatTimestamp(value: string) {
  const millis = Number(value);
  return Number.isFinite(millis) ? new Date(millis).toLocaleString() : value;
}

function formatError(caught: unknown) {
  return caught instanceof Error ? caught.message : String(caught);
}

export default App;
