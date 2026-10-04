CREATE TABLE gfa_llm_accounts (
    id TEXT PRIMARY KEY NOT NULL,
    payload TEXT NOT NULL
);

CREATE TABLE gfa_llm_calls (
    id TEXT PRIMARY KEY NOT NULL,
    match_id TEXT NOT NULL REFERENCES gfa_matches(id),
    seat INTEGER NOT NULL CHECK (seat BETWEEN 0 AND 255),
    turn BIGINT NOT NULL CHECK (turn >= 0),
    attempt INTEGER NOT NULL CHECK (attempt BETWEEN 0 AND 1000),
    payload TEXT NOT NULL,
    UNIQUE (match_id, seat, turn, attempt)
);
