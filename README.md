# Music OS

Music OS is an open-source, local-first collection optimizer for large local,
NAS, and external-drive music libraries. It is not a traditional music player.
The first priority is preserving original audio files while helping users find,
rate, tag, compare, and eventually reduce redundant storage safely.

## Founding principles

- Original files are sacred: imports copy bytes into vault storage and never
  rewrite source files.
- A TrackIdentity is not a file: it holds artist/title/version identity, one
  global user rating, semantic tags, and preferred-asset pointers.
- An AudioAsset is one concrete audio file or former audio file: it holds vault
  path, checksums, technical facts, original tags, role, and storage state.
- Roles explain why an asset matters: `first_found`, `nostalgia`, or `variant`.
- Best versions are pointers, not roles: lossy, lossless, verified, and
  nostalgia assets can differ.
- Tags are current search/filter labels, not historical events.
- Albums and compilations are entities so context can survive changing taste.
- Loudness is metadata and playback policy; normalization must be dynamic and
  non-destructive.
- Data must remain portable, readable, and exportable.

## MVP stack

- Tauri desktop shell
- React + TypeScript frontend
- Rust backend
- SQLite archive database

The Rust archive core lives in `crates/music-os-core` so preservation workflows
can be tested without the desktop runtime.

## Current MVP capabilities

- Import a local music file path into checksum-addressed vault storage.
- Create TrackIdentity records independently from AudioAssets.
- Extract trailing filename hashtags such as `#2020 #house #party` into
  TrackIdentity semantic tags while preserving the original filename.
- Store multiple AudioAssets per TrackIdentity with `first_found`, `nostalgia`,
  or `variant` roles.
- Track storage state as `local`, `external`, `shadow`, or `missing`.
- Assign one global 1-5 star TrackIdentity rating.
- Maintain preferred-asset pointers for best lossy, best verified lossless,
  best verified playback, and nostalgia playback.
- Store playback-relevant loudness analysis metadata on AudioAssets and use
  curated LoudnessProfiles instead of creating normalized duplicate files.
- Store cover artwork as first-class CoverAssets with checksums, vault paths,
  storage state, and explicit relationships to audio, tracks, releases, or
  collections so identical embedded album art can be deduplicated.

## Development

```bash
npm install
npm run dev
```

Run the Rust core tests:

```bash
npm run test:rust
```

Run frontend build and core tests:

```bash
npm run check
```

Run the desktop app:

```bash
npm run tauri:dev
```

On Linux, Tauri development may require WebKitGTK and related system libraries.

## Trying the current GUI

The MVP GUI is now useful for small test collections:

1. Run `npm run tauri:dev`.
2. Use **Choose folder...** to select a small music folder.
3. Or drag audio files/folders onto the drop zone.
4. Optionally set a default rating or tags such as `#test-import`.
5. Import the folder.
6. Use the Library Retrieval filters for text, tags, minimum rating, or storage
   state.
7. Select a track to edit its global rating, replace current tags, or inspect
   AudioAssets.

Start with copied test files, not your main collection. The import path remains
non-destructive, but the product is still an MVP.
