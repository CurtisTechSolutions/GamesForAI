# Resuming single-seat training

`GameEnv.get_state()` version 3 binds the learner seat, game/configuration,
built-in opponent name and level, native engine state, environment RNG, and
curriculum. A restore into an environment with a different built-in opponent
fails atomically. Versions 1 and 2 remain loadable with the default random
opponent only, because those formats did not identify an opponent.

Python callbacks, including `ChatPolicy`, use the `python` identity marker.
The trainer must restore the matching model, optimizer, policy RNG and remote
model configuration itself. Environment checkpoints do not contain weights,
API keys, HTTP clients, or external model state. Gym space sampling RNGs are
also caller-owned. Use your trainer's seeded RNG for reproducible action sampling.

Checkpoints contain private engine and curriculum data. Save them in trusted
training storage; policy inputs should use only seat-visible observations.
