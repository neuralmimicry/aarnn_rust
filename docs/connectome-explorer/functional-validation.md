# Functional validation

## Task contracts

A task suite (TOML, `examples/explorer/tasks/*.toml`) declares tasks. Each task has:

- a family: `discrimination`, `timing`, `memory`, `learning`, `closed-loop` or `quiescence`;
- a duration, a stimulus (channel segments) and a trial distribution;
- measures and criteria;
- required capabilities.

**Trial distribution.**
- An onset jitter in whole 2 ms frames is applied to segments marked `jitter = true`. A whole-frame shift keeps a pulse's frame count; an odd-millisecond shift would split it and double its spikes.
- An optional amplitude jitter and an optional background spike probability, drawn from the task-noise RNG.
- Every trial seed also seeds the network RNG.

**Measures** (per output, in a time window):
- `spikes`, `mean`, `max`;
- `latency_ms`, from a named stimulus onset to the first frame at or above `level`;
- for closed-loop tasks, `plant_final` and `plant_settle_ms`.

A missing response is stored as `NO_RESPONSE = 1e9`, which never satisfies a latency criterion.

**Criteria.** Each required task must meet its own `min_success` fraction of trials (default all), and there is no weighted average across tasks. The criterion operators:
- `>=`, `<=`, `==` against an absolute value;
- `ratio-of-reference`: candidate ≥ amount × reference mean, plus the absolute floor `value`, which always applies. With a non-positive or absent reference, only the floor decides, so a silent reference cannot let an empty candidate pass.
- `reference-plus`: candidate ≤ reference mean + amount, never a missing response, with `value` as an absolute cap. Used for additional latency.

**Quiescence.** A candidate that produces no output events in any non-quiescence task is rejected as `quiescent`, even when every criterion would pass.

**Capabilities.** A required task whose capability is unavailable fails admission and is reported. An optional one is listed as not-run and never counted as passed.

## Splits

`development` seeds drive selection: screening, beam search and threshold calibration. `locked_validation` seeds are used only by `validate`, after selection is fixed. The suite parser rejects overlapping splits, and `validate` refuses the development split.

## Authored suite `synthetic-sensorimotor-v1`

| Task | Family | Pass condition |
|---|---|---|
| go | discrimination + timing | ≥ 1 O_go spike in [30, 90] ms; latency ≤ 10 ms and ≤ intact + 1 ms |
| nogo | discrimination (negative) | no O_go spike while B is held |
| quiet | quiescence | no output at all without input |
| memory-hit | memory | O_mem responds to the probe P 200 ms after cue C |
| memory-miss | memory (negative) | no O_mem response to P without C |
| steer | closed loop | heading error ≤ 5° at the end and settled by 1400 ms, from 30-80° in either direction |

The closed-loop task feeds the controller the sensory consequences of its own actions, through a synthetic heading plant. It never replays the intact circuit's observations.

## Learning

Learning (`learning_required`) is not part of the authored contract. Preserving current output is distinct from preserving future learning. A contract that needs learning must declare a learning task (`family = "learning"`, `plasticity_policy = "source"`) with retention and new-association measures. No such task is shipped as a stub.

## Learned-state contracts

A naive, frozen network keeps its learned pathways silent: before learning, the value cells, the steering and the hunger gate that selects them do nothing visible. A contract run only on the naive network therefore licenses their removal. Under the 1 s fly contract the search removed HUNGER; under the salamander's it removed HUNGER and FALL. For a network that learns, the contract is also run on a learned state:

1. `aarnn-explorer derive --source NET --name NET-learned --preset learned-cap` sets every plastic synapse to the network's plasticity cap (`aarnn_plasticity_w_max`), a fully learned state. Plastic synapses follow the runner's `PlasticCellMask` rule: existing excitatory hidden-to-hidden synapses onto a masked plastic cell, from a cell the presynaptic mask allows.
2. The learned-state contracts (`drosophila_learned.toml`, `salamander_learned.toml`) add conditions that use the learned pathways:
   - hungry with the cue on the other side;
   - sated with a cue (the hunger gate);
   - a hungry cue that falls after 2 s (the value-fall turn).
3. Optimise the learned state. Carry the removal set back to the naive network with `derive --source NET --name NET-reduced --lower-like selected --write net.json`, then check that the naive network keeps its naive contract with an `experiment` ablating the same set.

The cap is an upper bound of what plasticity can reach, not a recorded experience. A snapshot saved after closed-loop learning would be a better learned state, but the simulator path has no final-snapshot hook.

Function slower than the trials (a 3 s turn-bout clock, a 10-20 s exploration oscillator, a 1.5 s escape adaptation) cannot be seen in 1-3 s trials. A 20 s salamander task did not catch the oscillator, which never switched sides within 20 s. When testing such function would cost more than the rest of the contract, the profile declares it out of scope with `optimisation.protect_cell_types` (prefixes) or `protect_neurons`. The optimisation record lists the result as `declared_protected`.

## Claims kept separate

Every validation record holds five verdicts:

| Verdict | Meaning here |
|---|---|
| reference_agreement | the candidate meets every criterion relative to the intact reference on the locked split |
| behavioural_preservation | the same, and the candidate is not quiescent |
| biological_validity | `not-assessed`: authored or simulated tasks only |
| resilience | the profile's claims (see resilience.md) |
| hardware_benefit | estimated cost model plus measured CPU step latency on this host |

A reduced circuit has a different internal state digest by construction. Equivalence is judged through the task oracles, never by comparing internal digests across models. Deterministic replay is checked within one model: the exported bundle's re-evaluation must reproduce its validation's per-task means exactly.
