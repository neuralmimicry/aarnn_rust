// AARNN adaptive threshold + slow homeostasis demo.
// Mirrors the threshold-offset and rate-EMA updates in src/sim.rs and src/runner.rs.

this_dir = get_absolute_file_path("03_adaptive_threshold_homeostasis.sce");
exec(this_dir + "common/aarnn_fpaa_lib.sci", -1);

dt_ms = 1.0;
t_ms = (0:dt_ms:600)';
n = size(t_ms, 1);
spikes = zeros(n, 1);

for i = 1:n
    if modulo(i - 1, 25) == 0 & t_ms(i) >= 50 & t_ms(i) <= 250 then
        spikes(i) = 1;
    end
    if modulo(i - 1, 10) == 0 & t_ms(i) >= 300 & t_ms(i) <= 420 then
        spikes(i) = 1;
    end
    if modulo(i - 1, 55) == 0 & t_ms(i) >= 470 then
        spikes(i) = 1;
    end
end

[thr_offset, rate_ema] = aarnn_adaptive_threshold_homeostasis(spikes, dt_ms, 200.0, 0.5, -2.0, 5.0, 3.0, 2000.0, 0.25);
t_s = t_ms / 1000.0;

adaptive_threshold_spikes = aarnn_signal(t_s, spikes);
adaptive_threshold_offset = aarnn_signal(t_s, thr_offset);
adaptive_threshold_rate_ema = aarnn_signal(t_s, rate_ema);

clf();
subplot(3, 1, 1);
plot(t_ms, spikes);
xtitle("Adaptive-threshold input spikes", "time (ms)", "spike");
subplot(3, 1, 2);
plot(t_ms, thr_offset);
xtitle("Threshold offset", "time (ms)", "offset");
subplot(3, 1, 3);
plot(t_ms, rate_ema);
xtitle("Homeostatic rate EMA", "time (ms)", "rate");
