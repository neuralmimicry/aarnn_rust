// AARNN synaptic front-end filter demo.
// Matches the structure of src/aarnn/dynamics.rs::apply_synaptic_filter.

this_dir = get_absolute_file_path("01_synaptic_filter.sce");
exec(this_dir + "common/aarnn_fpaa_lib.sci", -1);

dt_ms = 1.0;
t_ms = (0:dt_ms:400)';
n = size(t_ms, 1);
raw = zeros(n, 1);
vmem = -65.0 * ones(n, 1);

for i = 1:n
    if t_ms(i) >= 40 & t_ms(i) < 110 then
        raw(i) = 1.2;
    elseif t_ms(i) >= 160 & t_ms(i) < 240 then
        raw(i) = 0.6;
        vmem(i) = -45.0;
    elseif t_ms(i) >= 280 & t_ms(i) < 340 then
        raw(i) = -0.9;
    end
end

[ampa, nmda, gaba, out] = aarnn_synaptic_filter(raw, vmem, dt_ms, 5.0, 100.0, 10.0, 0.25, 1.0, 0.04, 1.0);
t_s = t_ms / 1000.0;

synaptic_filter_raw = aarnn_signal(t_s, raw);
synaptic_filter_vmem = aarnn_signal(t_s, vmem);
synaptic_filter_ampa = aarnn_signal(t_s, ampa);
synaptic_filter_nmda = aarnn_signal(t_s, nmda);
synaptic_filter_gaba = aarnn_signal(t_s, gaba);
synaptic_filter_expected = aarnn_signal(t_s, out);

clf();
subplot(3, 1, 1);
plot(t_ms, [raw vmem]);
xtitle("Synaptic filter inputs", "time (ms)", "raw / vmem");
subplot(3, 1, 2);
plot(t_ms, [ampa nmda gaba]);
xtitle("AMPA / NMDA / GABA states", "time (ms)", "state");
subplot(3, 1, 3);
plot(t_ms, out);
xtitle("Filtered current", "time (ms)", "I");
