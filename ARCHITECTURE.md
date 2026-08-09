# BITIG Architecture

## Boundaries

```text
React UI -> typed Tauri command DTOs -> Rust services -> repositories -> SQLite
                                      -> storage paths -> managed filesystem
                                      -> adapters -> yt-dlp / FFmpeg / ffprobe
```

React never executes a process, queries SQLite, or resolves an unrestricted local path. Commands under `src-tauri/src/commands/` only translate the request and call a service. User-visible errors are typed and sanitized; internal causes are logged with `tracing`.

## Data model

- `folders`: UUIDv7 ID, optional self-referencing `parent_id`, unique sibling name, timestamps.
- `media`: stable ID, virtual folder relationship, display title, source identity, relative managed paths, validated technical metadata.
- `downloads`: job status, progress counters, safe errors, and lifecycle timestamps.

SQL migrations are embedded from `src-tauri/migrations/`. Analysis reports existing items with the same source platform and source ID as a non-blocking duplicate warning, so different qualities remain valid. On startup, jobs left in a running state are marked failed/interrupted.

## Storage lifecycle

Each download receives a UUIDv7 job workspace under `temp/<job-id>/`. yt-dlp writes media and a WebP thumbnail only there. FFmpeg processing and ffprobe validation also happen there. A final file must exist, be non-empty, and contain the expected stream before media and thumbnail atomically leave the workspace for their managed directories. Only relative managed paths are persisted. Startup cleanup removes only UUID-shaped application job/quarantine entries and leaves unrecognized temp entries untouched.

Virtual folder moves update `media.folder_id`; title changes update `media.title`. They never move or rename the physical file. Export is a controlled copy selected through a native dialog.

## Download lifecycle and events

The allowed state sequence is `queued -> analyzing -> downloading -> processing -> verifying -> finalizing -> completed`, with `failed` and `cancelled` terminal alternatives. A Tokio semaphore limits concurrent work. Bounded line streaming parses yt-dlp's machine-formatted byte totals and speed, persists them, and emits typed Tauri events; cancellation owns and terminates the relevant child process before cleaning its job directory.

## Security boundaries

- Backend URL parsing accepts only HTTP(S).
- Child processes receive separate arguments and never user-composed shell syntax.
- Format option IDs are backend-issued opaque values, not arbitrary yt-dlp arguments.
- Frontend commands use internal media IDs; repository lookup and managed-path validation precede filesystem operations.
- Tauri capabilities expose only core window functionality and registered application commands.
- Sidecars are versioned independently, recorded with source/license/checksum provenance, verified before release, and packaged per target triple.
