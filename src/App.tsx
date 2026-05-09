import { useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

type RepresentationRole = "first_found" | "nostalgia" | "variant";
type StorageState = "local" | "external" | "shadow" | "missing";

interface TrackIdentity {
  id: string;
  artist: string;
  title: string;
  version?: string | null;
  user_rating?: number | null;
  best_lossy_asset_id?: string | null;
  best_lossless_asset_id?: string | null;
  best_verified_asset_id?: string | null;
  nostalgia_asset_id?: string | null;
}

interface AudioAsset {
  id: string;
  track_identity_id: string;
  role: RepresentationRole;
  storage_state: StorageState;
  vault_path?: string | null;
  original_path?: string | null;
  original_filename?: string | null;
  checksum?: string | null;
  audio_fingerprint?: string | null;
  format?: string | null;
  bitrate_kbps?: number | null;
  sample_rate_hz?: number | null;
  duration_ms?: number | null;
  file_size?: number | null;
  quality_score?: number | null;
  true_lossless_verified: boolean;
  suspected_transcode: boolean;
}

interface SemanticTag {
  id: string;
  label: string;
  normalized_label: string;
}

interface TrackRecord {
  identity: TrackIdentity;
  assets: AudioAsset[];
  tags: SemanticTag[];
}

interface ImportForm {
  sourcePath: string;
  title: string;
  artist: string;
  version: string;
  role: "" | RepresentationRole;
  userRating: string;
  semanticTags: string;
}

const roles: RepresentationRole[] = ["first_found", "nostalgia", "variant"];
const storageStates: StorageState[] = ["local", "external", "shadow", "missing"];

const initialImportForm: ImportForm = {
  sourcePath: "",
  title: "",
  artist: "",
  version: "",
  role: "",
  userRating: "",
  semanticTags: "",
};

function App() {
  const [tracks, setTracks] = useState<TrackRecord[]>([]);
  const [selectedTrackId, setSelectedTrackId] = useState<string | null>(null);
  const [importForm, setImportForm] = useState<ImportForm>(initialImportForm);
  const [ratingDraft, setRatingDraft] = useState("");
  const [tagDraft, setTagDraft] = useState("");
  const [status, setStatus] = useState("Ready to preserve, optimize, and find music.");
  const [error, setError] = useState<string | null>(null);

  const selectedTrack = useMemo(
    () =>
      tracks.find((record) => record.identity.id === selectedTrackId) ?? tracks[0],
    [selectedTrackId, tracks],
  );

  useEffect(() => {
    void refreshTracks();
  }, []);

  useEffect(() => {
    if (!selectedTrack) {
      return;
    }
    setRatingDraft(selectedTrack.identity.user_rating?.toString() ?? "");
    setTagDraft(selectedTrack.tags.map((tag) => `#${tag.normalized_label}`).join(" "));
  }, [selectedTrack?.identity.id]);

  async function refreshTracks() {
    try {
      const records = await invoke<TrackRecord[]>("list_tracks");
      setTracks(records);
      setSelectedTrackId((current) => current ?? records[0]?.identity.id ?? null);
      setError(null);
    } catch (caught) {
      setError(formatError(caught));
    }
  }

  async function importTrack(event: React.FormEvent<HTMLFormElement>) {
    event.preventDefault();
    setStatus("Importing into checksum vault and extracting filename tags...");
    setError(null);
    try {
      await invoke("import_music_file", {
        request: {
          source_path: importForm.sourcePath,
          track_identity_id: null,
          title: optional(importForm.title),
          artist: optional(importForm.artist),
          version: optional(importForm.version),
          role: importForm.role || null,
          user_rating: optionalNumber(importForm.userRating),
          semantic_tags: splitTags(importForm.semanticTags),
          original_tags_json: null,
        },
      });
      setImportForm(initialImportForm);
      setStatus("Import complete. Source bytes were preserved and hashtags became tags.");
      await refreshTracks();
    } catch (caught) {
      setError(formatError(caught));
      setStatus("Import failed.");
    }
  }

  async function updateRating(event: React.FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (!selectedTrack) {
      return;
    }
    try {
      await invoke("update_track_rating", {
        trackIdentityId: selectedTrack.identity.id,
        userRating: optionalNumber(ratingDraft),
      });
      setStatus("Global track rating updated.");
      await refreshTracks();
    } catch (caught) {
      setError(formatError(caught));
    }
  }

  async function replaceTags(event: React.FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (!selectedTrack) {
      return;
    }
    try {
      await invoke("replace_track_tags", {
        trackIdentityId: selectedTrack.identity.id,
        semanticTags: splitTags(tagDraft),
      });
      setStatus("Current track tags updated.");
      await refreshTracks();
    } catch (caught) {
      setError(formatError(caught));
    }
  }

  async function updateStorageState(assetId: string, storageState: StorageState) {
    try {
      await invoke("update_storage_state", {
        audioAssetId: assetId,
        storageState,
      });
      setStatus(`Asset storage state changed to ${label(storageState)}.`);
      await refreshTracks();
    } catch (caught) {
      setError(formatError(caught));
    }
  }

  return (
    <main className="shell">
      <section className="hero">
        <div>
          <p className="eyebrow">Local-first collection optimizer</p>
          <h1>Music OS</h1>
          <p>
            Track identities hold ratings and semantic tags. Audio assets hold
            concrete files, storage state, quality facts, and personal roles.
          </p>
        </div>
        <div className="principles">
          <span>Original bytes stay sacred</span>
          <span>Best versions are pointers</span>
          <span>Tags are current search labels</span>
        </div>
      </section>

      <section className="notice" aria-live="polite">
        <strong>Status:</strong> {status}
        {error && <p className="error">{error}</p>}
      </section>

      <div className="grid">
        <section className="card">
          <h2>Import folder/file candidate</h2>
          <p className="muted">
            Filename suffixes like <code>#2020 #house #party</code> are extracted
            into TrackIdentity tags while the original filename remains preserved
            on the AudioAsset.
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
                placeholder="/music/Eminem - Stan #2000 #herzschmerz.mp3"
              />
            </label>
            <div className="two-column">
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
            </div>
            <div className="three-column">
              <label>
                Version
                <input
                  value={importForm.version}
                  onChange={(event) =>
                    setImportForm({ ...importForm, version: event.target.value })
                  }
                  placeholder="Radio edit, live, remaster..."
                />
              </label>
              <label>
                Role
                <select
                  value={importForm.role}
                  onChange={(event) =>
                    setImportForm({
                      ...importForm,
                      role: event.target.value as "" | RepresentationRole,
                    })
                  }
                >
                  <option value="">Auto</option>
                  {roles.map((role) => (
                    <option key={role} value={role}>
                      {label(role)}
                    </option>
                  ))}
                </select>
              </label>
              <label>
                Rating
                <input
                  type="number"
                  min="1"
                  max="5"
                  value={importForm.userRating}
                  onChange={(event) =>
                    setImportForm({ ...importForm, userRating: event.target.value })
                  }
                />
              </label>
            </div>
            <label>
              Additional tags
              <input
                value={importForm.semanticTags}
                onChange={(event) =>
                  setImportForm({ ...importForm, semanticTags: event.target.value })
                }
                placeholder="#party #deutsch #bass"
              />
            </label>
            <button type="submit">Import non-destructively</button>
          </form>
        </section>

        <section className="card">
          <h2>Track identities</h2>
          {tracks.length === 0 ? (
            <p className="empty">No tracks yet.</p>
          ) : (
            <div className="track-list">
              {tracks.map((record) => (
                <button
                  className={
                    record.identity.id === selectedTrack?.identity.id
                      ? "track-row selected"
                      : "track-row"
                  }
                  key={record.identity.id}
                  type="button"
                  onClick={() => setSelectedTrackId(record.identity.id)}
                >
                  <span>
                    <strong>{record.identity.title}</strong>
                    <small>{record.identity.artist}</small>
                  </span>
                  <span className="state">
                    {record.identity.user_rating
                      ? `${record.identity.user_rating} stars`
                      : "unrated"}
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
              <p className="eyebrow">TrackIdentity</p>
              <h2>{selectedTrack.identity.title}</h2>
              <p>
                {selectedTrack.identity.artist}
                {selectedTrack.identity.version
                  ? ` - ${selectedTrack.identity.version}`
                  : ""}
              </p>
            </div>
          </div>

          <div className="detail-grid">
            <section>
              <h3>Global rating</h3>
              <form className="stack" onSubmit={updateRating}>
                <label>
                  User rating, 1-5 stars
                  <input
                    type="number"
                    min="1"
                    max="5"
                    value={ratingDraft}
                    onChange={(event) => setRatingDraft(event.target.value)}
                  />
                </label>
                <button type="submit">Save rating</button>
              </form>
            </section>

            <section>
              <h3>Semantic tags</h3>
              <form className="stack" onSubmit={replaceTags}>
                <label>
                  Current tags
                  <input
                    value={tagDraft}
                    onChange={(event) => setTagDraft(event.target.value)}
                    placeholder="#party #80s #deutsch"
                  />
                </label>
                <button type="submit">Replace current tags</button>
              </form>
              <ul className="pill-list">
                {selectedTrack.tags.map((tag) => (
                  <li key={tag.id}>#{tag.normalized_label}</li>
                ))}
              </ul>
            </section>
          </div>

          <section>
            <h3>Audio assets</h3>
            <div className="representation-list">
              {selectedTrack.assets.map((asset) => (
                <article key={asset.id} className="representation">
                  <div>
                    <strong>{label(asset.role)}</strong>
                    <p>
                      {asset.format?.toUpperCase() ?? "Unknown format"}
                      {asset.true_lossless_verified ? " - verified lossless" : ""}
                      {asset.suspected_transcode ? " - suspected transcode" : ""}
                    </p>
                    {asset.original_filename && <small>{asset.original_filename}</small>}
                  </div>
                  <select
                    value={asset.storage_state}
                    onChange={(event) =>
                      void updateStorageState(
                        asset.id,
                        event.target.value as StorageState,
                      )
                    }
                  >
                    {storageStates.map((state) => (
                      <option key={state} value={state}>
                        {label(state)}
                      </option>
                    ))}
                  </select>
                </article>
              ))}
            </div>
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

function splitTags(value: string) {
  return value
    .split(/\s+/)
    .map((tag) => tag.trim())
    .filter(Boolean);
}

function label(value: string) {
  return value.replace(/_/g, " ");
}

function formatError(caught: unknown) {
  return caught instanceof Error ? caught.message : String(caught);
}

export default App;
