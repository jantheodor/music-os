# Music OS architecture

## Boundaries

Music OS separates three concerns:

1. **Archive core (`crates/music-os-core`)**
   - Owns SQLite schema, archive invariants, checksum vault paths, ratings,
     track states, representations, album context, and history.
   - Contains no UI or Tauri runtime dependency.
2. **Desktop adapter (`src-tauri`)**
   - Resolves the application data directory.
   - Opens the archive database and vault.
   - Exposes narrow commands to the frontend.
3. **Frontend (`src`)**
   - Provides an MVP workflow surface for importing, rating, state transitions,
     shadow entries, representations, and history review.

## Data model

### `vault_files`

Stores copied original audio bytes by SHA256 and size. The source path is
recorded as provenance. Rows are never used as direct track identity.

### `tracks`

Represents the conceptual musical item. A track has an archive state such as
`active`, `recall`, `shadow`, `historical`, `replaceable`, or `archived`.

### `track_representations`

Connects a track to one available vault file or to a shadow-only memory.
Representation roles include:

- `discovery`
- `nostalgia`
- `preferred_technical`
- `historical_variant`
- `shadow`

### `track_ratings`

Stores music appreciation and file quality separately, allowing cases like
"5/5 song, poor file" to inform recall and replacement workflows.

### `albums` and `album_tracks`

Preserve album and compilation context independently from current track
preference.

### `history_events`

Records meaningful archive milestones: track creation, imports, album context,
rating changes, state changes, representation role changes, and shadow entries.

## Non-destructive import

The core import path:

1. Reads the source file and calculates SHA256.
2. Chooses `vault/originals/<sha-prefix>/<sha>.<ext>`.
3. Copies bytes with create-new semantics.
4. Reuses an existing vault row for duplicate bytes.
5. Creates or reuses a track entity.
6. Adds a representation and history events.

The import path does not modify, rename, normalize, retag, or delete the source
file.

## Future extension points

- Acoustic fingerprinting and loudness metadata tables.
- Export manifests that reconstruct ordinary folders from track/album state.
- Relationship clustering for redundant variants.
- Playback profile metadata for Original, Album Respect, Party, Car, Night,
  and Smart Normalize modes.
