// AARNN active dendritic compartment demo.
// Matches src/aarnn/dynamics.rs::apply_active_dendritic_compartment.

this_dir = get_absolute_file_path("04_active_dendrite.sce");
exec(this_dir + "common/aarnn_fpaa_lib.sci", -1);

dt_ms = 1.0;
t_ms = (0:dt_ms:450)';
n = size(t_ms, 1);
curr_in = zeros(n, 1);
local_stimulus = zeros(n, 1);
branching_gain = ones(n, 1);

for i = 1:n
    if t_ms(i) >= 40 & t_ms(i) < 120 then
        curr_in(i) = 0.8;
        local_stimulus(i) = 0.4;
    elseif t_ms(i) >= 150 & t_ms(i) < 250 then
        curr_in(i) = 1.5;
        local_stimulus(i) = 0.8;
        branching_gain(i) = 1.8;
    elseif t_ms(i) >= 300 & t_ms(i) < 360 then
        curr_in(i) = -0.7;
        local_stimulus(i) = 0.3;
        branching_gain(i) = 1.4;
    end
end

[ca_trace, plateau_trace, out_curr] = aarnn_active_dendrite(curr_in, local_stimulus, branching_gain, dt_ms, 120.0, 350.0, 0.10, 1.0, 0.40);
t_s = t_ms / 1000.0;

active_dendrite_input = aarnn_signal(t_s, curr_in);
active_dendrite_local_stimulus = aarnn_signal(t_s, local_stimulus);
active_dendrite_ca = aarnn_signal(t_s, ca_trace);
active_dendrite_plateau = aarnn_signal(t_s, plateau_trace);
active_dendrite_output = aarnn_signal(t_s, out_curr);

clf();
subplot(4, 1, 1);
plot(t_ms, [curr_in local_stimulus]);
xtitle("Active-dendrite drive", "time (ms)", "input");
subplot(4, 1, 2);
plot(t_ms, ca_trace);
xtitle("Calcium-like state", "time (ms)", "Ca");
subplot(4, 1, 3);
plot(t_ms, plateau_trace);
xtitle("Plateau state", "time (ms)", "plateau");
subplot(4, 1, 4);
plot(t_ms, out_curr);
xtitle("Soma-facing current", "time (ms)", "current");
