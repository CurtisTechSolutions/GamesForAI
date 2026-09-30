CREATE TABLE gfa_matches (
    id TEXT PRIMARY KEY NOT NULL,
    revision INTEGER NOT NULL CHECK (revision >= 0)
);

CREATE TABLE gfa_events (
    match_id TEXT NOT NULL REFERENCES gfa_matches(id),
    sequence INTEGER NOT NULL CHECK (sequence >= 0),
    payload TEXT NOT NULL,
    PRIMARY KEY (match_id, sequence)
);

CREATE TABLE gfa_commands (
    match_id TEXT NOT NULL REFERENCES gfa_matches(id),
    command_key TEXT NOT NULL,
    payload TEXT NOT NULL,
    PRIMARY KEY (match_id, command_key)
);
