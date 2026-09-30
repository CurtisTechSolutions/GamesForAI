CREATE TABLE gfa_matches (
    id TEXT COLLATE "C" PRIMARY KEY,
    revision BIGINT NOT NULL CHECK (revision >= 0)
);

CREATE TABLE gfa_events (
    match_id TEXT NOT NULL REFERENCES gfa_matches(id),
    sequence BIGINT NOT NULL CHECK (sequence >= 0),
    payload TEXT NOT NULL,
    PRIMARY KEY (match_id, sequence)
);

CREATE TABLE gfa_commands (
    receipt_sequence BIGINT GENERATED ALWAYS AS IDENTITY UNIQUE,
    match_id TEXT NOT NULL REFERENCES gfa_matches(id),
    command_key TEXT NOT NULL,
    payload TEXT NOT NULL,
    PRIMARY KEY (match_id, command_key)
);

CREATE TABLE gfa_assist_usage (
    match_id TEXT NOT NULL REFERENCES gfa_matches(id),
    seat BIGINT NOT NULL CHECK (seat BETWEEN 0 AND 255),
    simulation_calls BIGINT NOT NULL CHECK (simulation_calls >= 0),
    simulated_moves BIGINT NOT NULL CHECK (simulated_moves >= 0),
    PRIMARY KEY (match_id, seat)
);
