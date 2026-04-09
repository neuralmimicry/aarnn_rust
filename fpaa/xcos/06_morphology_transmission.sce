// AARNN morphology-aware transmission demo.
// Matches src/aarnn/transmission.rs::compute_delay_and_attenuation.

this_dir = get_absolute_file_path("06_morphology_transmission.sce");
exec(this_dir + "common/aarnn_fpaa_lib.sci", -1);

dt_ms = 1.0;
t_ms = (0:dt_ms:260)';
n = size(t_ms, 1);
impulse = zeros(n, 1);
impulse(25) = 1.0;

generic_out = zeros(n, 1);
apical_out = zeros(n, 1);
myelinated_out = zeros(n, 1);
fatigued_out = zeros(n, 1);

[generic_steps, generic_atten] = aarnn_compute_delay_and_attenuation(2, dt_ms, 11, 0, 6, 3, 1, 0.2, 0.15, 1.2, 0.6, 1.0, 0, 0.5, 1.0, 1.0, 0, -1.0, 0.8, 2.2, 1.0, 1.0);
[apical_steps, apical_atten] = aarnn_compute_delay_and_attenuation(2, dt_ms, 11, 1, 6, 3, 1, 0.2, 0.15, 1.2, 0.8, 1.0, 1, 1.3, 0.85, 1.25, 1, -1.0, 0.8, 2.2, 1.0, 1.0);
[myelin_steps, myelin_atten] = aarnn_compute_delay_and_attenuation(3, dt_ms, 11, 2, 6, 3, 1, 0.2, 0.15, 1.2, 0.6, 1.0, 0, 0.5, 1.0, 1.0, 0, 0.9, 0.8, 2.2, 1.0, 1.0);
[fatigue_steps, fatigue_atten] = aarnn_compute_delay_and_attenuation(3, dt_ms, 11, 3, 6, 3, 1, 0.2, 0.15, 1.2, 0.6, 1.0, 0, 0.5, 1.0, 1.0, 0, 0.2, 0.8, 2.2, 0.35, 0.40);

if 25 + generic_steps <= n then generic_out(25 + generic_steps) = generic_atten; end
if 25 + apical_steps <= n then apical_out(25 + apical_steps) = apical_atten; end
if 25 + myelin_steps <= n then myelinated_out(25 + myelin_steps) = myelin_atten; end
if 25 + fatigue_steps <= n then fatigued_out(25 + fatigue_steps) = fatigue_atten; end

t_s = t_ms / 1000.0;
morphology_transmission_input = aarnn_signal(t_s, impulse);
morphology_transmission_generic = aarnn_signal(t_s, generic_out);
morphology_transmission_apical = aarnn_signal(t_s, apical_out);
morphology_transmission_myelinated = aarnn_signal(t_s, myelinated_out);
morphology_transmission_fatigued = aarnn_signal(t_s, fatigued_out);

morphology_transmission_summary = [generic_steps generic_atten; apical_steps apical_atten; myelin_steps myelin_atten; fatigue_steps fatigue_atten];

clf();
subplot(2, 1, 1);
plot(t_ms, [impulse generic_out apical_out myelinated_out fatigued_out]);
xtitle("Morphology-aware impulse delivery", "time (ms)", "amplitude");
subplot(2, 1, 2);
bar(morphology_transmission_summary(:, 1));
xtitle("Computed delay steps", "path index", "steps");
