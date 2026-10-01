import { useMutation, useQuery } from "@tanstack/react-query";
import { useState } from "react";
import type { FormEvent } from "react";
import type { GameSpec, JsonValue, Schemas } from "@gfa/api-client";
import { api } from "./api";
import { Failure, Loading } from "./feedback";

function options(text: string): JsonValue {
  const value: unknown = JSON.parse(text || "{}");
  if (!value || typeof value !== "object" || Array.isArray(value)) throw new Error("Game options must be a JSON object.");
  return value as JsonValue;
}

function seedValue(text: string) {
  if (!/^\d+$/.test(text)) throw new Error("Enter a nonnegative whole-number seed.");
  const value = Number(text);
  if (!Number.isSafeInteger(value)) throw new Error("The seed must be at most 9007199254740991.");
  return value;
}

export function MatchSetup({ id }: { id: string }) {
  const game = useQuery({ queryKey: ["game", id], queryFn: ({ signal }) => api.game(id, signal) });
  if (game.isPending) return <Loading label="Loading game…" />;
  if (game.isError) return <Failure error={game.error} retry={() => void game.refetch()} />;
  return <SetupForm game={game.data} />;
}

function SetupForm({ game }: { game: GameSpec }) {
  const opponents = useQuery({ queryKey: ["opponents", game.id], queryFn: ({ signal }) => api.opponents(game.id, signal) });
  const [mode, setMode] = useState("hotseat");
  const [seat, setSeat] = useState(0);
  const [opponent, setOpponent] = useState("random");
  const [level, setLevel] = useState(1);
  const [seed, setSeed] = useState("42");
  const [position, setPosition] = useState("");
  const [config, setConfig] = useState("{}");
  const [hints, setHints] = useState(false);
  const [inputError, setInputError] = useState("");
  const single = game.num_players[1] === 1;
  const selected = opponents.data?.find((entry) => entry.id === opponent);
  const validate = useMutation({ mutationFn: (body: Schemas["ValidatePosition"]) => api.validate(game.id, body, seat) });
  const create = useMutation({
    mutationFn: (body: Schemas["CreateMatch"]) => api.create(body, seat),
    onSuccess: (match) => {
      window.location.hash = "/matches/" + encodeURIComponent(match.match_id) + "?seat=" + seat + (mode === "hotseat" && !single ? "&hotseat=1" : "");
    },
  });
  const prepare = () => ({
    config: options(config), seed: seedValue(seed),
    start: position.trim() ? { position: position.trim() } : undefined,
  });
  const submit = (event: FormEvent) => {
    event.preventDefault();
    setInputError("");
    try {
      const body = prepare();
      const seats: Schemas["Seat"][] = Array.from({ length: game.num_players[0] }, (_, index) => {
        if (single || mode === "hotseat" || index === seat) return { type: "human" };
        if (mode === "external") return { type: "open" };
        if (!selected) throw new Error("Select an installed opponent.");
        return { type: "opponent", opponent: { id: selected.id, ...(selected.levels.length ? { level } : {}) } };
      });
      create.mutate({ game_id: game.id, ...body, seats, assists: { allow_analysis: hints, allow_simulation: false }, include_info: false });
    } catch (error) { setInputError(error instanceof Error ? error.message : "Check the match options."); }
  };
  return (
    <>
      <a className="back-link" href={"#/games/" + encodeURIComponent(game.id)}>← {game.name} guide</a>
      <section className="game-heading"><div><p className="eyebrow">Your next match</p><h1>Play {game.name}</h1><p>Choose who plays and where the game begins.</p></div></section>
      <form className="setup-form" onSubmit={submit}>
        {!single && <div className="form-grid">
          <label>Players<select value={mode} onChange={(event) => setMode(event.target.value)}><option value="hotseat">Hot-seat on this device</option><option value="opponent">You vs built-in opponent</option><option value="external">You vs your external agent</option></select></label>
          <label>Your seat<select value={seat} onChange={(event) => setSeat(Number(event.target.value))}>{game.seat_names.map((name, index) => <option key={name} value={index}>{name} · seat {index}</option>)}</select></label>
        </div>}
        {mode === "opponent" && !single && <div className="form-grid">
          <label>Opponent<select value={opponent} disabled={!opponents.data} onChange={(event) => { setOpponent(event.target.value); setLevel(opponents.data?.find((entry) => entry.id === event.target.value)?.levels[0]?.level ?? 1); }}>{opponents.data?.map((entry) => <option key={entry.id} value={entry.id}>{entry.name}</option>)}</select></label>
          {!!selected?.levels.length && <label>Level<select value={level} onChange={(event) => setLevel(Number(event.target.value))}>{selected.levels.map((entry) => <option key={entry.level} value={entry.level}>{entry.level}{entry.rating ? " · " + entry.rating : ""}</option>)}</select><small>{selected.calibrated ? "Measured strength" : "Search budget; strength calibration pending"}</small></label>}
          {opponents.isError && <Failure error={opponents.error} retry={() => void opponents.refetch()} />}
        </div>}
        {mode === "external" && <p>Your model can join the open seat through REST or MCP. The match page provides its identifier and the API reference.</p>}
        <label>Seed<input inputMode="numeric" value={seed} onChange={(event) => setSeed(event.target.value)} required maxLength={16} /></label>
        <label className="checkbox-label"><input type="checkbox" checked={hints} onChange={(event) => setHints(event.target.checked)} />Allow engine hints for this match</label>
        <details><summary>Custom starting position and game options</summary>
          <label>Starting position<textarea rows={4} maxLength={48000} value={position} onChange={(event) => { setPosition(event.target.value); validate.reset(); }} placeholder="Leave blank for the standard start" /></label>
          <p className="notation-help">{game.position_notation}</p>
          <label>Game options (JSON)<textarea rows={3} maxLength={8000} value={config} onChange={(event) => { setConfig(event.target.value); validate.reset(); }} /></label>
          <button className="secondary" type="button" disabled={!position.trim() || validate.isPending || create.isPending} onClick={() => {
            setInputError("");
            try { const body = prepare(); if (body.start) validate.mutate({ ...body, start: body.start }); }
            catch (error) { setInputError(error instanceof Error ? error.message : "Check the position."); }
          }}>{validate.isPending ? "Checking…" : "Validate position"}</button>
          {validate.isSuccess && <p role="status">Position is valid.{validate.data.terminated ? " This position is already finished." : ""}</p>}
          {validate.isError && <Failure error={validate.error} />}
        </details>
        {inputError && <p role="alert" className="input-error">{inputError}</p>}
        {create.isError && <Failure error={create.error} />}
        <button type="submit" disabled={create.isPending || (mode === "opponent" && !single && !selected)}>{create.isPending ? "Starting…" : "Start match"}</button>
      </form>
    </>
  );
}
