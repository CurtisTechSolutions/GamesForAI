CREATE TABLE gfa_assist_usage (
    match_id TEXT NOT NULL REFERENCES gfa_matches(id),
    seat INTEGER NOT NULL CHECK (seat BETWEEN 0 AND 255),
    simulation_calls INTEGER NOT NULL CHECK (typeof(simulation_calls) = 'integer' AND simulation_calls >= 0),
    simulated_moves INTEGER NOT NULL CHECK (typeof(simulated_moves) = 'integer' AND simulated_moves >= 0),
    PRIMARY KEY (match_id, seat)
);
