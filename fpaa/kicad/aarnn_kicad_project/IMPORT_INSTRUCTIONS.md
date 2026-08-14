# Opening the AARNN project in KiCad

## Recommended route: KiCad 6, 7, 8 or 9

1. Extract the complete `aarnn_kicad_project` directory. Do not move the
   `.lib`, cache library or `sym-lib-table` away from the project file.
2. Start KiCad and choose **File → Open Existing Project**, then select
   `tb_aarnn_kernels.kicad_pro`. You can also double-click that file.
3. In the Project Manager, open **Schematic Editor**. The root design is
   `tb_aarnn_kernels.sch`.
4. KiCad will recognise the root as a legacy schematic. Accept the
   remapping/migration prompt, then use **File → Save As** to create
   `tb_aarnn_kernels.kicad_sch` in the same directory.
5. If KiCad asks for a symbol library, use the project-specific entry
   `tb_aarnn_kernels-cache`. The included `sym-lib-table` resolves it via
   `${KIPRJMOD}`, so the folder remains portable.
6. Keep the generated `.kicad_sch`, `.kicad_pro`, `aarnn_kernels.lib`
   and cache/symbol-library files together.

## Checking the SPICE models

1. Open the migrated root schematic.
2. For each `X...` AARNN block, open **Symbol Properties → Simulation
   Model**.
3. Confirm that **File** is `aarnn_kernels.lib` and **Model** matches the
   block name, for example `AARNN_SYNAPTIC_FILTER`.
4. Preserve the numerical pin assignment already stored on the symbol.
   `AARNN_SPICE_TO_KICAD_MAPPING.csv` provides an independent audit.
5. Open **Inspect → Simulator** and select transient analysis. The root
   sheet contains `.tran 10u 250m 0 100u`, the initial conditions and
   solver options from the original testbench.

## Expanded model pages

The nine `aarnn_*.sch` files are standalone implementation references.
Open them individually with **File → Open** in Schematic Editor and save
each as `.kicad_sch` if you want to edit them in the current native
format. They are not child sheets of the testbench because doing so would
duplicate the `.lib` models during simulation.

## If project opening does not find the legacy root

Open `tb_aarnn_kernels.sch` directly in Schematic Editor, accept the
conversion, and save it as `tb_aarnn_kernels.kicad_sch` beside
`tb_aarnn_kernels.kicad_pro`. Close and reopen the project file; KiCad
will then select the native root automatically.

Official references:

- https://docs.kicad.org/9.0/en/kicad/kicad.html
- https://docs.kicad.org/9.0/en/eeschema/eeschema.html
