import { useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { open, save } from "@tauri-apps/plugin-dialog";
import { languageNames, translate, type Language } from "./i18n";

type RepresentationRole = "first_found" | "nostalgia" | "variant";
type StorageState = "local" | "external" | "shadow" | "missing";
type ThemePreference = "system" | "dark" | "light";

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
  preferred_cover_asset_id?: string | null;
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

interface FolderImportForm {
  rootPath: string;
  userRating: string;
  semanticTags: string;
}

interface ImportFolderResult {
  scanned_files: number;
  imported_files: number;
  skipped_files: number;
  errors: Array<{ path: string; error: string }>;
}

interface ExportM3uResult {
  destination_path: string;
  exported_tracks: number;
  skipped_tracks: number;
  warnings: string[];
}

interface Filters {
  text: string;
  tags: string;
  minRating: string;
  storageState: "" | StorageState;
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

const initialFolderForm: FolderImportForm = {
  rootPath: "",
  userRating: "",
  semanticTags: "",
};

function App() {
  const [language, setLanguage] = useState<Language>(() => {
    const stored = window.localStorage.getItem("music-os-language");
    return stored === "de" || stored === "en" || stored === "es" ? stored : "de";
  });
  const [themePreference, setThemePreference] = useState<ThemePreference>(() => {
    const stored = window.localStorage.getItem("music-os-theme");
    return stored === "dark" || stored === "light" || stored === "system"
      ? stored
      : "system";
  });
  const [tracks, setTracks] = useState<TrackRecord[]>([]);
  const [selectedTrackId, setSelectedTrackId] = useState<string | null>(null);
  const [importForm, setImportForm] = useState<ImportForm>(initialImportForm);
  const [folderForm, setFolderForm] = useState<FolderImportForm>(initialFolderForm);
  const [lastFolderImport, setLastFolderImport] = useState<ImportFolderResult | null>(
    null,
  );
  const [filters, setFilters] = useState<Filters>({
    text: "",
    tags: "",
    minRating: "",
    storageState: "",
  });
  const [ratingDraft, setRatingDraft] = useState("");
  const [tagDraft, setTagDraft] = useState("");
  const [exportBasketIds, setExportBasketIds] = useState<string[]>([]);
  const [isDraggingOver, setIsDraggingOver] = useState(false);
  const [status, setStatus] = useState(() => translate(language, "statusReady"));
  const [error, setError] = useState<string | null>(null);
  const t = (key: Parameters<typeof translate>[1]) => translate(language, key);

  const filteredTracks = useMemo(
    () => tracks.filter((record) => matchesFilters(record, filters)),
    [tracks, filters],
  );

  const selectedTrack = useMemo(
    () =>
      filteredTracks.find((record) => record.identity.id === selectedTrackId) ??
      filteredTracks[0] ??
      tracks.find((record) => record.identity.id === selectedTrackId) ??
      tracks[0],
    [filteredTracks, selectedTrackId, tracks],
  );

  const libraryStats = useMemo(() => {
    const assetCount = tracks.reduce((sum, record) => sum + record.assets.length, 0);
    const localAssets = tracks.reduce(
      (sum, record) =>
        sum + record.assets.filter((asset) => asset.storage_state === "local").length,
      0,
    );
    const tags = new Set(
      tracks.flatMap((record) => record.tags.map((tag) => tag.normalized_label)),
    );
    return {
      tracks: tracks.length,
      assets: assetCount,
      localAssets,
      tags: tags.size,
    };
  }, [tracks]);

  const exportBasketTracks = useMemo(
    () =>
      exportBasketIds
        .map((id) => tracks.find((record) => record.identity.id === id))
        .filter((record): record is TrackRecord => Boolean(record)),
    [exportBasketIds, tracks],
  );

  useEffect(() => {
    void refreshTracks();
  }, []);

  useEffect(() => {
    window.localStorage.setItem("music-os-language", language);
  }, [language]);

  useEffect(() => {
    window.localStorage.setItem("music-os-theme", themePreference);
    const applyTheme = () => {
      const systemTheme = window.matchMedia("(prefers-color-scheme: light)").matches
        ? "light"
        : "dark";
      document.documentElement.dataset.theme =
        themePreference === "system" ? systemTheme : themePreference;
    };
    applyTheme();
    const media = window.matchMedia("(prefers-color-scheme: light)");
    media.addEventListener("change", applyTheme);
    return () => media.removeEventListener("change", applyTheme);
  }, [themePreference]);

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    getCurrentWebview()
      .onDragDropEvent((event) => {
        if (event.payload.type === "enter" || event.payload.type === "over") {
          setIsDraggingOver(true);
        } else if (event.payload.type === "leave") {
          setIsDraggingOver(false);
        } else if (event.payload.type === "drop") {
          setIsDraggingOver(false);
          void importDroppedPaths(event.payload.paths);
        }
      })
      .then((handler) => {
        unlisten = handler;
      })
      .catch((caught) => setError(formatError(caught)));

    return () => {
      unlisten?.();
    };
  }, [folderForm.userRating, folderForm.semanticTags]);

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
    setStatus(
      language === "de"
        ? "Importiere Datei in den Checksum-Vault und lese Dateiname-Tags..."
        : language === "es"
          ? "Importando archivo al vault y extrayendo etiquetas del nombre..."
          : "Importing file into checksum vault and extracting filename tags...",
    );
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
      setStatus(
        language === "de"
          ? "Dateiimport abgeschlossen. Quelldatei wurde unverändert gelassen."
          : language === "es"
            ? "Importación completada. El archivo original no fue modificado."
            : "File import complete. Source bytes were preserved.",
      );
      await refreshTracks();
    } catch (caught) {
      setError(formatError(caught));
      setStatus(
        language === "de"
          ? "Dateiimport fehlgeschlagen."
          : language === "es"
            ? "Error al importar el archivo."
            : "File import failed.",
      );
    }
  }

  async function importFolder(event: React.FormEvent<HTMLFormElement>) {
    event.preventDefault();
    setStatus(
      language === "de"
        ? "Scanne Ordner und importiere unterstützte Audiodateien..."
        : language === "es"
          ? "Escaneando carpeta e importando archivos de audio compatibles..."
          : "Scanning folder and importing supported audio files...",
    );
    setError(null);
    setLastFolderImport(null);
    try {
      const result = await invoke<ImportFolderResult>("import_music_folder", {
        request: {
          root_path: folderForm.rootPath,
          user_rating: optionalNumber(folderForm.userRating),
          semantic_tags: splitTags(folderForm.semanticTags),
        },
      });
      setLastFolderImport(result);
      setStatus(formatFolderImportStatus(language, result));
      await refreshTracks();
    } catch (caught) {
      setError(formatError(caught));
      setStatus(
        language === "de"
          ? "Ordnerimport fehlgeschlagen."
          : language === "es"
            ? "Error al importar la carpeta."
            : "Folder import failed.",
      );
    }
  }

  async function importDroppedPaths(paths: string[]) {
    if (paths.length === 0) {
      return;
    }

    setStatus(
      language === "de"
        ? `Importiere ${paths.length} abgelegte Pfade...`
        : language === "es"
          ? `Importando ${paths.length} rutas soltadas...`
          : `Importing ${paths.length} dropped path${paths.length === 1 ? "" : "s"}...`,
    );
    setError(null);
    const aggregate: ImportFolderResult = {
      scanned_files: 0,
      imported_files: 0,
      skipped_files: 0,
      errors: [],
    };

    for (const path of paths) {
      try {
        if (isSupportedAudioPath(path)) {
          await invoke("import_music_file", {
            request: {
              source_path: path,
              track_identity_id: null,
              title: null,
              artist: null,
              version: null,
              role: null,
              user_rating: optionalNumber(folderForm.userRating),
              semantic_tags: splitTags(folderForm.semanticTags),
              original_tags_json: null,
            },
          });
          aggregate.scanned_files += 1;
          aggregate.imported_files += 1;
        } else {
          const result = await invoke<ImportFolderResult>("import_music_folder", {
            request: {
              root_path: path,
              user_rating: optionalNumber(folderForm.userRating),
              semantic_tags: splitTags(folderForm.semanticTags),
            },
          });
          aggregate.scanned_files += result.scanned_files;
          aggregate.imported_files += result.imported_files;
          aggregate.skipped_files += result.skipped_files;
          aggregate.errors.push(...result.errors);
        }
      } catch (caught) {
        aggregate.errors.push({ path, error: formatError(caught) });
      }
    }

    setLastFolderImport(aggregate);
    setStatus(formatDropImportStatus(language, aggregate));
    await refreshTracks();
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
      setStatus(
        language === "de"
          ? "Globale Track-Bewertung aktualisiert."
          : language === "es"
            ? "Valoración global actualizada."
            : "Global track rating updated.",
      );
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
      setStatus(
        language === "de"
          ? "Aktuelle Tags aktualisiert."
          : language === "es"
            ? "Etiquetas actuales actualizadas."
            : "Current track tags updated.",
      );
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
      setStatus(
        language === "de"
          ? `Speicherstatus geändert zu ${label(storageState)}.`
          : language === "es"
            ? `Estado de almacenamiento cambiado a ${label(storageState)}.`
            : `Asset storage state changed to ${label(storageState)}.`,
      );
      await refreshTracks();
    } catch (caught) {
      setError(formatError(caught));
    }
  }

  function addToExportBasket(trackIdentityId: string) {
    setExportBasketIds((current) =>
      current.includes(trackIdentityId) ? current : [...current, trackIdentityId],
    );
  }

  function addFilteredToExportBasket() {
    setExportBasketIds((current) => {
      const next = new Set(current);
      for (const record of filteredTracks) {
        next.add(record.identity.id);
      }
      return Array.from(next);
    });
  }

  function removeFromExportBasket(trackIdentityId: string) {
    setExportBasketIds((current) => current.filter((id) => id !== trackIdentityId));
  }

  async function exportBasketAsM3u() {
    if (exportBasketIds.length === 0) {
      setError(
        language === "de"
          ? "Der Exportkorb ist leer."
          : language === "es"
            ? "La cesta de exportación está vacía."
            : "Export basket is empty.",
      );
      return;
    }

    try {
      const destinationPath = await save({
        title: t("exportBasketAsM3u"),
        defaultPath: "music-os-playlist.m3u",
        filters: [{ name: "M3U playlist", extensions: ["m3u"] }],
      });
      if (!destinationPath) {
        return;
      }
      const result = await invoke<ExportM3uResult>("export_m3u_playlist", {
        request: {
          destination_path: destinationPath,
          track_identity_ids: exportBasketIds,
        },
      });
      setStatus(
        language === "de"
          ? `M3U exportiert: ${result.exported_tracks} Tracks, ${result.skipped_tracks} übersprungen.`
          : language === "es"
            ? `M3U exportado: ${result.exported_tracks} canciones, ${result.skipped_tracks} omitidas.`
            : `M3U exported: ${result.exported_tracks} tracks, ${result.skipped_tracks} skipped.`,
      );
      if (result.warnings.length > 0) {
        setError(result.warnings.slice(0, 5).join(" | "));
      } else {
        setError(null);
      }
    } catch (caught) {
      setError(formatError(caught));
    }
  }

  async function chooseFolder() {
    try {
      const selected = await open({
        directory: true,
        multiple: false,
        title: t("chooseFolder"),
      });
      if (typeof selected === "string") {
        setFolderForm({ ...folderForm, rootPath: selected });
      }
    } catch (caught) {
      setError(formatError(caught));
    }
  }

  async function chooseFile() {
    try {
      const selected = await open({
        directory: false,
        multiple: false,
        title: t("chooseAudioFile"),
        filters: [
          {
            name: "Audio",
            extensions: ["mp3", "flac", "wav", "aiff", "aif", "m4a", "aac", "ogg", "opus"],
          },
        ],
      });
      if (typeof selected === "string") {
        setImportForm({ ...importForm, sourcePath: selected });
      }
    } catch (caught) {
      setError(formatError(caught));
    }
  }

  return (
    <main className="shell">
      <section className="hero">
        <div>
          <p className="eyebrow">
            {language === "de"
              ? "Lokaler Sammlungs-Optimierer"
              : language === "es"
                ? "Optimizador local de colección"
                : "Local-first collection optimizer"}
          </p>
          <h1>Music OS</h1>
          <p>{t("heroSubtitle")}</p>
        </div>
        <div className="principles">
          <label className="language-select">
            {t("language")}
            <select
              value={language}
              onChange={(event) => setLanguage(event.target.value as Language)}
            >
              {(Object.keys(languageNames) as Language[]).map((option) => (
                <option key={option} value={option}>
                  {languageNames[option]}
                </option>
              ))}
            </select>
          </label>
          <label className="language-select">
            {t("theme")}
            <select
              value={themePreference}
              onChange={(event) =>
                setThemePreference(event.target.value as ThemePreference)
              }
            >
              <option value="system">{t("themeSystem")}</option>
              <option value="dark">{t("themeDark")}</option>
              <option value="light">{t("themeLight")}</option>
            </select>
          </label>
          <span>
            {libraryStats.tracks} {t("trackIdentities")}
          </span>
          <span>
            {libraryStats.assets} {t("audioAssets")}
          </span>
          <span>
            {libraryStats.tags} {t("tags")}
          </span>
        </div>
      </section>

      <section className="notice" aria-live="polite">
        <strong>Status:</strong> {status}
        {error && <p className="error">{error}</p>}
      </section>

      <section className={isDraggingOver ? "drop-zone dragging" : "drop-zone"}>
        <strong>{t("dragDropTitle")}</strong>
        <span>{t("dragDropHint")}</span>
      </section>

      <div className="grid">
        <section className="card">
          <h2>{t("folderImport")}</h2>
          <p className="muted">{t("folderImportHelp")}</p>
          <form className="stack" onSubmit={importFolder}>
            <label>
              {t("musicFolderPath")}
              <input
                required
                value={folderForm.rootPath}
                onChange={(event) =>
                  setFolderForm({ ...folderForm, rootPath: event.target.value })
                }
                placeholder="/home/me/Music"
              />
            </label>
            <button type="button" onClick={() => void chooseFolder()}>
              {t("chooseFolder")}
            </button>
            <div className="two-column">
              <label>
                {t("defaultRating")}
                <input
                  type="number"
                  min="1"
                  max="5"
                  value={folderForm.userRating}
                  onChange={(event) =>
                    setFolderForm({ ...folderForm, userRating: event.target.value })
                  }
                />
              </label>
              <label>
                {t("addTagsToAll")}
                <input
                  value={folderForm.semanticTags}
                  onChange={(event) =>
                    setFolderForm({ ...folderForm, semanticTags: event.target.value })
                  }
                  placeholder="#import-2026 #check"
                />
              </label>
            </div>
            <button type="submit">{t("importFolder")}</button>
          </form>

          {lastFolderImport && (
            <div className="import-summary">
              <strong>{t("lastFolderImport")}</strong>
              <div className="summary-grid">
                <span>Scanned: {lastFolderImport.scanned_files}</span>
                <span>Imported: {lastFolderImport.imported_files}</span>
                <span>Skipped: {lastFolderImport.skipped_files}</span>
                <span>Errors: {lastFolderImport.errors.length}</span>
              </div>
              {lastFolderImport.errors.length > 0 && (
                <details>
                  <summary>{t("showImportErrors")}</summary>
                  <ul>
                    {lastFolderImport.errors.slice(0, 20).map((item) => (
                      <li key={`${item.path}-${item.error}`}>
                        <code>{item.path}</code>: {item.error}
                      </li>
                    ))}
                  </ul>
                </details>
              )}
            </div>
          )}
        </section>

        <section className="card">
          <h2>{t("fileImport")}</h2>
          <p className="muted">{t("singleFileHelp")}</p>
          <form className="stack" onSubmit={importTrack}>
            <label>
              {t("sourceFilePath")}
              <input
                required
                value={importForm.sourcePath}
                onChange={(event) =>
                  setImportForm({ ...importForm, sourcePath: event.target.value })
                }
                placeholder="/music/Eminem - Stan #2000 #herzschmerz.mp3"
              />
            </label>
            <button type="button" onClick={() => void chooseFile()}>
              {t("chooseAudioFile")}
            </button>
            <div className="two-column">
              <label>
                {t("artist")}
                <input
                  value={importForm.artist}
                  onChange={(event) =>
                    setImportForm({ ...importForm, artist: event.target.value })
                  }
                  placeholder={
                    language === "de"
                      ? "Aus Dateiname ableiten"
                      : language === "es"
                        ? "Inferir del nombre"
                        : "Infer from filename"
                  }
                />
              </label>
              <label>
                {t("title")}
                <input
                  value={importForm.title}
                  onChange={(event) =>
                    setImportForm({ ...importForm, title: event.target.value })
                  }
                  placeholder={
                    language === "de"
                      ? "Aus Dateiname ableiten"
                      : language === "es"
                        ? "Inferir del nombre"
                        : "Infer from filename"
                  }
                />
              </label>
            </div>
            <div className="three-column">
              <label>
                {t("version")}
                <input
                  value={importForm.version}
                  onChange={(event) =>
                    setImportForm({ ...importForm, version: event.target.value })
                  }
                  placeholder={
                    language === "de"
                      ? "Radio Edit, live..."
                      : language === "es"
                        ? "Radio edit, en vivo..."
                        : "Radio edit, live..."
                  }
                />
              </label>
              <label>
                {t("role")}
                <select
                  value={importForm.role}
                  onChange={(event) =>
                    setImportForm({
                      ...importForm,
                      role: event.target.value as "" | RepresentationRole,
                    })
                  }
                >
                  <option value="">{t("auto")}</option>
                  {roles.map((role) => (
                    <option key={role} value={role}>
                      {label(role)}
                    </option>
                  ))}
                </select>
              </label>
              <label>
                {t("defaultRating")}
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
              {t("addTagsToAll")}
              <input
                value={importForm.semanticTags}
                onChange={(event) =>
                  setImportForm({ ...importForm, semanticTags: event.target.value })
                }
                placeholder="#party #deutsch #bass"
              />
            </label>
            <button type="submit">{t("importFile")}</button>
          </form>
        </section>
      </div>

      <section className="card library-card">
        <div className="detail-header">
          <div>
            <p className="eyebrow">{t("libraryRetrieval")}</p>
            <h2>{t("searchAndFilter")}</h2>
          </div>
          <button type="button" onClick={() => void refreshTracks()}>
            {t("refresh")}
          </button>
        </div>
        <div className="four-column">
          <label>
            {t("text")}
            <input
              value={filters.text}
              onChange={(event) => setFilters({ ...filters, text: event.target.value })}
              placeholder="artist, title, filename"
            />
          </label>
          <label>
            {t("tags")}
            <input
              value={filters.tags}
              onChange={(event) => setFilters({ ...filters, tags: event.target.value })}
              placeholder="#party #deutsch"
            />
          </label>
          <label>
            {t("minRating")}
            <input
              type="number"
              min="1"
              max="5"
              value={filters.minRating}
              onChange={(event) =>
                setFilters({ ...filters, minRating: event.target.value })
              }
            />
          </label>
          <label>
            {t("storage")}
            <select
              value={filters.storageState}
              onChange={(event) =>
                setFilters({
                  ...filters,
                  storageState: event.target.value as "" | StorageState,
                })
              }
            >
              <option value="">{t("any")}</option>
              {storageStates.map((state) => (
                <option key={state} value={state}>
                  {label(state)}
                </option>
              ))}
            </select>
          </label>
        </div>

        <p className="muted">
          {language === "de"
            ? `Zeige ${filteredTracks.length} von ${tracks.length} Tracks. ${t("filtersAndBased")}`
            : language === "es"
              ? `Mostrando ${filteredTracks.length} de ${tracks.length} canciones. ${t("filtersAndBased")}`
              : `Showing ${filteredTracks.length} of ${tracks.length} tracks. ${t("filtersAndBased")}`}
        </p>

        <div className="basket-toolbar">
          <button type="button" onClick={addFilteredToExportBasket}>
            {t("addFiltered")}
          </button>
          <button type="button" onClick={() => void exportBasketAsM3u()}>
            {t("exportBasketAsM3u")}
          </button>
          <button type="button" onClick={() => setExportBasketIds([])}>
            {t("clearBasket")}
          </button>
          <span>
            {exportBasketIds.length} {t("tracksInBasket")}
          </span>
        </div>

        {tracks.length === 0 ? (
          <p className="empty">{t("noTracks")}</p>
        ) : (
          <div className="library-layout">
            <div className="track-list">
              {filteredTracks.map((record) => (
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
                    <small>
                      {record.identity.artist}
                      {record.identity.version ? ` - ${record.identity.version}` : ""}
                    </small>
                    <small>{record.tags.map((tag) => `#${tag.normalized_label}`).join(" ")}</small>
                  </span>
                  <span className="state">
                    {record.identity.user_rating
                      ? `${record.identity.user_rating} stars`
                      : "unrated"}
                  </span>
                  <span
                    className={
                      exportBasketIds.includes(record.identity.id)
                        ? "basket-marker active"
                        : "basket-marker"
                    }
                  >
                    {exportBasketIds.includes(record.identity.id)
                      ? t("exportBasket")
                      : t("notQueued")}
                  </span>
                </button>
              ))}
            </div>

            {selectedTrack && (
              <section className="detail-panel">
                <div className="detail-header">
                  <div>
                    <p className="eyebrow">{t("detailTrackIdentity")}</p>
                    <h2>{selectedTrack.identity.title}</h2>
                    <p>{selectedTrack.identity.artist}</p>
                  </div>
                  <button
                    type="button"
                    onClick={() => addToExportBasket(selectedTrack.identity.id)}
                  >
                    {t("addToBasket")}
                  </button>
                </div>

                <div className="detail-grid">
                  <section>
                    <h3>{t("globalRating")}</h3>
                    <form className="stack" onSubmit={updateRating}>
                      <label>
                        {t("ratingOneToFive")}
                        <input
                          type="number"
                          min="1"
                          max="5"
                          value={ratingDraft}
                          onChange={(event) => setRatingDraft(event.target.value)}
                        />
                      </label>
                      <button type="submit">{t("saveRating")}</button>
                    </form>
                  </section>

                  <section>
                    <h3>{t("tags")}</h3>
                    <form className="stack" onSubmit={replaceTags}>
                      <label>
                        {t("currentTags")}
                        <input
                          value={tagDraft}
                          onChange={(event) => setTagDraft(event.target.value)}
                          placeholder="#party #80s #deutsch"
                        />
                      </label>
                      <button type="submit">{t("replaceCurrentTags")}</button>
                    </form>
                    <ul className="pill-list">
                      {selectedTrack.tags.map((tag) => (
                        <li key={tag.id}>#{tag.normalized_label}</li>
                      ))}
                    </ul>
                  </section>
                </div>

                <section>
                  <h3>{t("audioAssets")}</h3>
                  <div className="representation-list">
                    {selectedTrack.assets.map((asset) => (
                      <article key={asset.id} className="representation">
                        <div>
                          <strong>{label(asset.role)}</strong>
                          <p>
                            {asset.format?.toUpperCase() ?? t("unknownFormat")}
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

                {exportBasketTracks.length > 0 && (
                  <section>
                    <h3>{t("exportBasket")}</h3>
                    <div className="basket-list">
                      {exportBasketTracks.map((record) => (
                        <article key={record.identity.id} className="basket-item">
                          <span>
                            <strong>{record.identity.artist}</strong> - {record.identity.title}
                          </span>
                          <button
                            type="button"
                            onClick={() => removeFromExportBasket(record.identity.id)}
                          >
                            {t("remove")}
                          </button>
                        </article>
                      ))}
                    </div>
                  </section>
                )}
              </section>
            )}
          </div>
        )}
      </section>
    </main>
  );
}

function matchesFilters(record: TrackRecord, filters: Filters) {
  const text = filters.text.trim().toLowerCase();
  if (text.length > 0) {
    const haystack = [
      record.identity.artist,
      record.identity.title,
      record.identity.version ?? "",
      ...record.assets.map((asset) => asset.original_filename ?? ""),
    ]
      .join(" ")
      .toLowerCase();
    if (!haystack.includes(text)) {
      return false;
    }
  }

  const wantedTags = splitTags(filters.tags).map(normalizeTag);
  const actualTags = new Set(record.tags.map((tag) => tag.normalized_label));
  if (wantedTags.some((tag) => !actualTags.has(tag))) {
    return false;
  }

  const minRating = optionalNumber(filters.minRating);
  if (minRating !== null && (record.identity.user_rating ?? 0) < minRating) {
    return false;
  }

  if (
    filters.storageState &&
    !record.assets.some((asset) => asset.storage_state === filters.storageState)
  ) {
    return false;
  }

  return true;
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

function normalizeTag(value: string) {
  return value.trim().replace(/^#/, "").toLowerCase();
}

function formatFolderImportStatus(language: Language, result: ImportFolderResult) {
  if (language === "de") {
    return `Ordnerimport abgeschlossen: ${result.imported_files} importiert, ${result.skipped_files} übersprungen, ${result.errors.length} Fehler.`;
  }
  if (language === "es") {
    return `Importación de carpeta completada: ${result.imported_files} importados, ${result.skipped_files} omitidos, ${result.errors.length} errores.`;
  }
  return `Folder import complete: ${result.imported_files} imported, ${result.skipped_files} skipped, ${result.errors.length} errors.`;
}

function formatDropImportStatus(language: Language, result: ImportFolderResult) {
  if (language === "de") {
    return `Drop-Import abgeschlossen: ${result.imported_files} importiert, ${result.skipped_files} übersprungen, ${result.errors.length} Fehler.`;
  }
  if (language === "es") {
    return `Importación por arrastre completada: ${result.imported_files} importados, ${result.skipped_files} omitidos, ${result.errors.length} errores.`;
  }
  return `Drop import complete: ${result.imported_files} imported, ${result.skipped_files} skipped, ${result.errors.length} errors.`;
}

function isSupportedAudioPath(path: string) {
  return /\.(mp3|flac|wav|aiff|aif|m4a|aac|ogg|opus)$/i.test(path);
}

function label(value: string) {
  return value.replace(/_/g, " ");
}

function formatError(caught: unknown) {
  return caught instanceof Error ? caught.message : String(caught);
}

export default App;
