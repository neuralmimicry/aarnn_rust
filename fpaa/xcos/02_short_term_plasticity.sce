// AARNN short-term plasticity demo.
// Matches src/aarnn/plasticity.rs::stp_step.

this_dir = get_absolute_file_path("02_short_term_plasticity.sce");
exec(this_dir + "common/aarnn_fpaa_lib.sci", -1);

dt_ms = 1.0;
t_ms = (0:dt_ms:500)';
n = size(t_ms, 1);
pre_spikes = zeros(n, 1);

for i = 1:n
    if modulo(i - 1, 40) == 0 & t_ms(i) >= 40 & t_ms(i) <= 320 then
        pre_spikes(i) = 1;
    end
    if modulo(i - 1, 15) == 0 & t_ms(i) >= 340 & t_ms(i) <= 460 then
        pre_spikes(i) = 1;
    end
end

[utilization, resources, release] = aarnn_stp(pre_spikes, dt_ms, 0.2, 800.0, 200.0);
t_s = t_ms / 1000.0;

stp_pre_spikes = aarnn_signal(t_s, pre_spikes);
stp_utilization = aarnn_signal(t_s, utilization);
stp_resources = aarnn_signal(t_s, resources);
stp_release = aarnn_signal(t_s, release);

clf();
subplot(4, 1, 1);
plot(t_ms, pre_spikes);
xtitle("STP input spikes", "time (ms)", "spike");
subplot(4, 1, 2);
plot(t_ms, utilization);
xtitle("Utilization u", "time (ms)", "u");
subplot(4, 1, 3);
plot(t_ms, resources);
xtitle("Available resources x", "time (ms)", "x");
subplot(4, 1, 4);
plot(t_ms, release);
xtitle("Effective release u*x", "time (ms)", "release");
