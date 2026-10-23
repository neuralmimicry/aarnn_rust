# Connect each body to its AARNN brain

Read spec/aer-contract.md and the canonical Rust transport/peripheral code. Add model-driven sensory encoding and bounded output decoding that reuse the existing AARNN runtime and authorised bridge paths. Extend native clients or add shared thin Python clients for MuJoCo/Isaac without duplicating the neural kernel. Test AER1 framing, empty spikes, address windows, sign encoding, name/dimension mismatches and isolation. Keep governed AARNN-AER/1 and FPAA UDP explicit. Demonstrate one actual closed loop through a real AARNN network before multiplying profiles. Add reset/reconnect/fence/commit behaviour and bounded scheduling. Never block physics/render callbacks waiting for a brain.

Completion evidence must be recorded in the living ExecPlan before proceeding or reporting this work package complete.
