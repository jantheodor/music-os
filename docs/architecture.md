# Music OS architecture

## Boundaries

Music OS separates three concerns:

1. **Archive core (`crates/music-os-core`)**
   - Owns SQLite schema, archive invariants, checksum vault paths, track
     identities, audio assets, storage state, quality pointers, ratings, and
     semantic tags.
   - Contains no UI or Tauri runtime dependency.
2. **Desktop adapter (`src-tauri`)**
   - Resolves the application data directory.
   - Opens the archive database and vault.
   - Exposes narrow commands to the frontend.
3. **Frontend (`src`)**
   - Provides an MVP workflow surface for importing, rating, tagging, storage
     state review, and asset inspection.

## Data model

### `track_identities`

Represents the abstract song identity. A TrackIdentity is not a file. It owns:

- canonical artist/title/version identity
- one global user rating, 1-5 stars
- semantic user tags through `track_identity_tags`
- pointers to preferred assets:
  - `best_lossy_asset_id`
  - `best_lossless_asset_id`
  - `best_verified_asset_id`
  - `nostalgia_asset_id`
- optional `preferred_cover_asset_id`

Ratings apply globally across every asset, placeholder, and context attached to
the track identity. Music OS may later write ratings back to materialized files
only through explicit user opt-in or export/materialization workflows, never
silently to sacred vault originals.

### `audio_assets`

Represents one concrete audio file or a former/known audio file. It owns:

- vault path if locally materialized
- original path and original filename as provenance
- checksum and optional audio fingerprint
- format, bitrate, sample rate, duration, and file size
- quality and playback facts such as integrated LUFS, true peak, dynamic range,
  ReplayGain track/album gain, clipping risk, and quality score when available
- original tags read from the file as JSON when available
- role and storage state

`storage_state` answers whether the actual audio is available:

- `local`: audio file is present in the vault
- `external`: known but not currently local
- `shadow`: only metadata/fingerprint/context remain; not currently playable as
  that exact file
- `missing`: expected file cannot be found unexpectedly

### Representation roles

Roles describe why an asset matters historically or personally. They are not
quality labels and not album/compilation flags.

Allowed roles:

- `first_found`: first known/found version of the TrackIdentity in Music OS
- `nostalgia`: consciously kept emotional/reference version
- `variant`: ordinary alternative version

The core assigns `first_found` only once by default. Later assets default to
`variant` unless explicitly marked as `nostalgia`.

### Quality/preference pointers

Best versions are pointers from TrackIdentity, not roles.

- `best_lossy_asset_id`: best verified lossy/portable version
- `best_lossless_asset_id`: best verified true lossless version
- `best_verified_asset_id`: default playback source, chosen by actual quality
- `nostalgia_asset_id`: optional direct pointer for nostalgia playback

Lossless is not automatically best. A fake FLAC sourced from a bad MP3 should
not become `best_verified_asset_id` merely because its container is lossless.

Playback selection:

- Default mode uses `best_verified_asset_id`.
- Portable/compatibility mode uses `best_lossy_asset_id`.
- Nostalgia mode uses `nostalgia_asset_id`, falling back to an asset with role
  `nostalgia` when available.

### Loudness profiles

Archive integrity is separate from playback loudness behavior. Music OS must not
create separate audio files solely for loudness normalization or different
listening contexts. One best audio source can support multiple listening
experiences through different LoudnessProfiles.

Loudness and dynamics analysis lives on AudioAsset. Playback/export can then
apply a dynamic adjustment plan without modifying vault audio:

- integrated LUFS
- true peak
- dynamic range
- ReplayGain track gain
- ReplayGain album gain
- clipping risk

Built-in MVP profiles:

- `Original`: untouched playback, no normalization, no limiter.
- `Album Respect`: album-oriented listening, prefer album gain, preserve
  dynamics strongly, default for album playback.
- `Shuffle Smooth`: moderate normalization for mixed playlists, default for
  shuffle/mixed playback.
- `Party`: stronger loudness consistency with optional soft limiting.
- `Headphones`: controlled peaks and reduced listening fatigue.

Manual override must remain possible, but advanced LUFS settings should not be
prominent in the default UI.

### `semantic_tags` and `track_identity_tags`

Tags are current search/filter labels on TrackIdentity, not historical events.
They intentionally do not track transitions like "#herzschmerz was replaced by
#wut". The current tag set is what matters for finding music later.

Examples:

- `#deep-house`
- `#herzschmerz`
- `#deutsch`
- `#2023`
- `#party`

### `cover_assets` and `cover_relationships`

Artwork is first-class media, not disposable audio-file metadata. CoverAsset
stores image identity and storage facts independently from audio:

- image checksum
- vault path when locally stored
- MIME type
- dimensions
- file size
- source/origin
- storage state: `local`, `external`, `shadow`, or `missing`

Many albums embed the same cover image into every track file. Music OS stores
identical artwork once when possible and references it many times.

Cover relationships are explicit and separate from audio roles:

- `embedded_original_cover`: artwork originally embedded in an AudioAsset
- `release_cover`: artwork for a release/album entity
- `collection_cover`: artwork for a collection or playlist
- `track_artwork`: optional track-specific artwork on TrackIdentity

TrackIdentity can also point to `preferred_cover_asset_id`. During export or
materialization, Music OS may re-embed appropriate artwork for ordinary-player
compatibility, or allow lightweight exports without embedded artwork. This is an
export/materialization behavior and does not require storing duplicate images in
the archive.

## Non-destructive import

The core import path:

1. Reads the source file and calculates SHA256.
2. Extracts trailing filename hashtags into TrackIdentity tags.
3. Uses the cleaned filename stem for artist/title inference.
4. Chooses `vault/originals/<sha-prefix>/<sha>.<ext>`.
5. Copies bytes with create-new semantics.
6. Creates or reuses a TrackIdentity.
7. Registers an AudioAsset with local storage state.

Example:

```text
Eminem - Stan #2000 #hip-hop #herzschmerz.mp3
```

becomes:

```text
TrackIdentity:
  artist = Eminem
  title = Stan
  tags = #2000, #hip-hop, #herzschmerz

AudioAsset:
  original_filename = Eminem - Stan #2000 #hip-hop #herzschmerz.mp3
```

The import path does not modify, rename, normalize, retag, or delete the source
file.

## Export naming policy

Music OS keeps original vault filenames checksum-based, but export filenames are
generated from canonical metadata. The current core policy is:

- Album-oriented export:
  - `<track-number>. <artist-name> - <track-name>.<ext>`
  - Track numbers are at least two digits: `01`, `02`, `03`.
  - Large albums or compilations expand the width from the highest known track
    number, for example `007` when the album has 120 tracks.
- Loose/single-file export:
  - `<artist-name> - <track-name>.<ext>`
  - No track number is included.

Examples:

```text
03. Eminem - Stan.mp3
007. Various Artists - Long Compilation Track.flac
Eminem - Stan.mp3
```

Export naming is intentionally separate from import. Incorrect original names
remain preserved as provenance, while exports can use clean canonical names.

## Future extension points

- Folder import and collection optimization workflows.
- Acoustic fingerprinting and deeper loudness analyzers.
- Export manifests that reconstruct ordinary folders from track/album state.
- Search/filter UI for tags, ratings, formats, and storage state.
- Optional explicit write-back/export of ratings and tags to materialized files.
- Relationship clustering for redundant variants.
