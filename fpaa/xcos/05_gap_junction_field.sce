// AARNN local gap-junction and volume-transmission demo.
// Matches src/aarnn/dynamics.rs::apply_local_gap_junction_coupling and volume_transmission_factors_for_layer.

this_dir = get_absolute_file_path("05_gap_junction_field.sce");
exec(this_dir + "common/aarnn_fpaa_lib.sci", -1);

dt_ms = 1.0;
t_ms = (0:dt_ms:220)';
n_steps = size(t_ms, 1);
positions = [0.00 0.00 0.00; 0.04 0.00 0.00; 0.09 0.02 0.00; 0.21 0.00 0.00];
inhibitory_mask = [1; 1; 0; 0];
sources = [0.00 0.00 0.00];

v_history = zeros(n_steps, 4);
delta_history = zeros(n_steps, 4);
field_history = zeros(n_steps, 4);

v_state = [-62.0; -64.0; -67.0; -70.0];
for k = 1:n_steps
    if t_ms(k) >= 25 & t_ms(k) < 70 then
        v_state(1) = -48.0;
    elseif t_ms(k) >= 90 & t_ms(k) < 130 then
        v_state(2) = -50.0;
    else
        v_state(1) = -62.0;
        v_state(2) = -64.0;
    end

    delta = aarnn_gap_junction_step(v_state, positions, 0.03, 0.12, %f, inhibitory_mask);
    field = aarnn_volume_transmission(positions, sources, 0.12, 0.10, 1.5);

    v_history(k, :) = v_state';
    delta_history(k, :) = delta';
    field_history(k, :) = field';
end

t_s = t_ms / 1000.0;
gap_field_membrane = aarnn_signal(t_s, v_history);
gap_field_delta = aarnn_signal(t_s, delta_history);
gap_field_volume = aarnn_signal(t_s, field_history);

clf();
subplot(3, 1, 1);
plot(t_ms, v_history);
xtitle("Membrane nodes", "time (ms)", "Vm");
subplot(3, 1, 2);
plot(t_ms, delta_history);
xtitle("Gap-junction current contribution", "time (ms)", "delta I");
subplot(3, 1, 3);
plot(t_ms, field_history);
xtitle("Volume-transmission factors", "time (ms)", "gain");
