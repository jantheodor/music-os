# Music OS

Music OS is an open-source, local-first desktop archive for music collections.
It is not a traditional music player. The first priority is preserving original
audio files, memories, metadata, ratings, album context, and history without
forcing binary keep/delete decisions.

## Founding principles

- Original files are sacred: imports copy bytes into vault storage and never
  rewrite source files.
- A track is not a file: one track entity can have discovery, nostalgia,
  preferred technical, historical, and shadow representations.
- Ratings are first-class and split music appreciation from file quality.
- Recall is a workflow, not deletion: tracks can move through active, recall,
  shadow, historical, replaceable, and archived states.
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
- Create track entities independently from files.
- Store multiple file representations per track.
- Mark discovery, nostalgia, preferred technical, historical, and shadow roles.
- Assign separate music and file quality ratings.
- Move tracks through recall/archive states.
- Preserve album context.
- Create shadow entries without local audio availability.
- Show track history as a narrative event stream.

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
