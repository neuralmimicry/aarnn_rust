# Xcos / Scilab Reference Simulations

These files are written for the Scilab + Xcos workflow used by the Hasler FPAA environment.

What the scripts do:

- implement the AARNN kernel equations from this repo in Scilab
- generate input and expected-output waveforms
- publish those waveforms as Xcos-friendly workspace structures with `time` and `values` fields
- plot the trajectories immediately for quick inspection

Typical workflow:

1. Start Scilab.
2. Run one script, for example:
   `exec('/absolute/path/to/fpaa/xcos/01_synaptic_filter.sce', -1);`
3. In Xcos, use `From Workspace` blocks for the generated input variables and `To Workspace` or `Scope` blocks for the outputs.
4. Rebuild the analog circuit with the Hasler palette blocks listed in the matching Okika/manifest file.

The script `00_generate_all.sce` runs every demo and leaves all workspace structures loaded.
