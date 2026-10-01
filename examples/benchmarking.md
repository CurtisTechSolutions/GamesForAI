# Vector environment throughput

## Measuring batch throughput

Run `python examples/vector_throughput.py` for seeded full-game Connect Four batches. It times `VectorEnv.step`, including native transitions, projections, and owned NumPy transfer, with random policy selection and explicit resets outside the timed interval. One sample warms up the process; the JSON report records repeated rates, batch size, workers, platform, and logical CPUs. Wheel CI uploads one report per OS. Hosted-runner measurements are diagnostics; the PRD target still requires verification on the specified eight-core reference machine.

Numeric-only projections are an optional game hook with a full-observation fallback. Connect Four computes its three tensor planes directly from bitboards. Vector stepping uses this hook to avoid text/JSON construction; native text policies retain full observations. CI compares numeric planes to public boards across complete seeded games and checks viewer validation and default behavior for every registered game.
