PRAGMA foreign_keys = ON;

CREATE TABLE folders (
    id TEXT PRIMARY KEY NOT NULL,
    parent_id TEXT REFERENCES folders(id) ON DELETE RESTRICT,
    name TEXT NOT NULL CHECK (length(trim(name)) BETWEEN 1 AND 120),
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE UNIQUE INDEX folders_unique_root_name
    ON folders(lower(name)) WHERE parent_id IS NULL;
CREATE UNIQUE INDEX folders_unique_child_name
    ON folders(parent_id, lower(name)) WHERE parent_id IS NOT NULL;
CREATE INDEX folders_parent_id_idx ON folders(parent_id);

CREATE TABLE media (
    id TEXT PRIMARY KEY NOT NULL,
    folder_id TEXT REFERENCES folders(id) ON DELETE RESTRICT,
    title TEXT NOT NULL CHECK (length(trim(title)) BETWEEN 1 AND 240),
    media_type TEXT NOT NULL CHECK (media_type IN ('video', 'audio')),
    source_url TEXT NOT NULL,
    source_platform TEXT NOT NULL,
    source_id TEXT NOT NULL,
    creator TEXT,
    file_path TEXT NOT NULL UNIQUE,
    thumbnail_path TEXT,
    container TEXT NOT NULL,
    width INTEGER,
    height INTEGER,
    duration_ms INTEGER,
    file_size INTEGER NOT NULL CHECK (file_size > 0),
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX media_folder_id_idx ON media(folder_id);
CREATE INDEX media_source_idx ON media(source_platform, source_id);
CREATE INDEX media_title_idx ON media(title);

CREATE TABLE downloads (
    id TEXT PRIMARY KEY NOT NULL,
    media_id TEXT REFERENCES media(id) ON DELETE SET NULL,
    title TEXT NOT NULL,
    source_url TEXT NOT NULL,
    status TEXT NOT NULL CHECK (status IN (
        'queued', 'analyzing', 'downloading', 'processing', 'verifying',
        'finalizing', 'completed', 'failed', 'cancelled'
    )),
    progress REAL NOT NULL DEFAULT 0 CHECK (progress >= 0 AND progress <= 1),
    downloaded_bytes INTEGER,
    total_bytes INTEGER,
    bytes_per_second INTEGER,
    error_code TEXT,
    error_message TEXT,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    started_at TEXT,
    completed_at TEXT
);

CREATE INDEX downloads_status_idx ON downloads(status);

