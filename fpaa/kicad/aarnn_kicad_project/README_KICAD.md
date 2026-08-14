        # AARNN KiCad schematic

        This project converts `tb_aarnn_kernels.cir` and the supplied
        `aarnn_kernels.lib` into an editable KiCad schematic set. Open
        `tb_aarnn_kernels.kicad_pro` in KiCad 6, 7, 8, or 9, then launch the
        Schematic Editor. KiCad will migrate the legacy `.sch` schematic and its
        self-contained cache symbols; save it to create the native `.kicad_sch`
        form for your installed version. Full instructions are in
        `IMPORT_INSTRUCTIONS.md`.

        Revision 1.3 repairs the legacy hierarchical-label type on all expanded
        `.sch` files and corrects the resistor polyline in the cache library.
        Each sheet is now checked record by record, in addition to the component
        transform validation introduced in revision 1.2. See
        `VALIDATION_REPORT.txt` for the per-sheet results.

        ## Main testbench

        The A3 root sheet retains the simulation-safe structure of the original:

        - all 12 PULSE, SIN and DC voltage sources;
        - all nine AARNN subcircuit instances;
        - exact library pin order and all 24 exposed state/signal nodes;
        - the three instance overrides (`GAP_G`, `DISTANCE`, and morphology values);
        - ground, `.ic`, `.options` and `.tran` settings;
        - both legacy `Spice_*` and current `Sim.*` metadata fields.

        The supplied `aarnn_kernels.lib` is beside the root schematic, so the
        subcircuit models can now be resolved. Named nets are used rather than
        long crossing wires to keep the sheet readable.

        ## Expanded implementation sheets

        Nine standalone reference sheets expand all 56 internal
        behavioural sources, state resistors and the gap-junction conductance
        element. These pages preserve every expression, default parameter,
        internal node and exposed port from the library. They are deliberately
        not instantiated below the root sheet: the root continues to use the
        canonical `.lib` subcircuits and therefore does not duplicate the models
        during simulation.

        - `aarnn_synaptic_filter.sch` — AARNN_SYNAPTIC_FILTER (11 elements).
- `aarnn_stp.sch` — AARNN_STP (7 elements).
- `aarnn_adaptive_homeostasis.sch` — AARNN_ADAPTIVE_HOMEOSTASIS (7 elements).
- `aarnn_active_dendrite.sch` — AARNN_ACTIVE_DENDRITE (8 elements).
- `aarnn_gap_pair.sch` — AARNN_GAP_PAIR (1 elements).
- `aarnn_gap_observer.sch` — AARNN_GAP_OBSERVER (4 elements).
- `aarnn_volume_field.sch` — AARNN_VOLUME_FIELD (2 elements).
- `aarnn_morph_transmission.sch` — AARNN_MORPH_TRANSMISSION (8 elements).
- `aarnn_triplet.sch` — AARNN_TRIPLET (8 elements).

        ## Simulation

        In KiCad, open the root schematic, migrate/save it, then use
        **Inspect → Simulator**. The model path is relative to the project folder.
        If the migrated symbol model dialog asks for reassignment, select
        `aarnn_kernels.lib`, choose the matching subcircuit name shown on the
        block, and preserve the displayed numerical pin sequence.

        The source netlist's batch-output control block remains in
        `tb_aarnn_kernels.cir`. KiCad's internal plot/export workflow differs from
        batch ngspice, so those `.control` output commands are not placed on the
        root schematic.

        ## Other files

        - `tb_aarnn_kernels.kicad_pro` — portable KiCad project settings.
        - `sym-lib-table` — project-local mapping for the legacy symbol cache.
        - `IMPORT_INSTRUCTIONS.md` — opening, migration and simulation steps.
        - `VALIDATION_REPORT.txt` — component-record and transform checks.
        - `aarnn_kernels.lib` — supplied canonical macromodel library.
        - `tb_aarnn_kernels-cache.lib` — self-contained KiCad symbols.
        - `tb_aarnn_kernels.cir` — original testbench netlist.
        - `AARNN_SPICE_TO_KICAD_MAPPING.csv` — auditable port and instance map.
        - `schematic_preview.png` — top-level visual preview.
        - `macromodel_pages_preview.png` — contact sheet for the nine expansions.
        - `SOURCE_README.md` — original design and PCB/FPAA notes.

        ## Scope

        These remain equation-level macromodels. The expanded sheets are not a
        component-selected PCB design and do not replace the OTA, comparator,
        multiplier, biasing, rail protection and scaling work described in the
        source notes.
