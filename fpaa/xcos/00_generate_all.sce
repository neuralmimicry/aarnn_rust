// Run all FPAA-oriented Xcos / Scilab demos.

this_dir = get_absolute_file_path("00_generate_all.sce");
exec(this_dir + "01_synaptic_filter.sce", -1);
exec(this_dir + "02_short_term_plasticity.sce", -1);
exec(this_dir + "03_adaptive_threshold_homeostasis.sce", -1);
exec(this_dir + "04_active_dendrite.sce", -1);
exec(this_dir + "05_gap_junction_field.sce", -1);
exec(this_dir + "06_morphology_transmission.sce", -1);
exec(this_dir + "07_triplet_scaling_dale_hybrid.sce", -1);
