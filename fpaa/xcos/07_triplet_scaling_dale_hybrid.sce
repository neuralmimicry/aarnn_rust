// AARNN triplet-metaplasticity, synaptic scaling, and Dale-constraint demo.
// Mirrors src/aarnn/plasticity.rs and is intended as a hybrid analog + host-supervised flow.

this_dir = get_absolute_file_path("07_triplet_scaling_dale_hybrid.sce");
exec(this_dir + "common/aarnn_fpaa_lib.sci", -1);

dt_ms = 1.0;
t_ms = (0:dt_ms:220)';
n = size(t_ms, 1);
pre_mean = zeros(n, 1);
post_mean = zeros(n, 1);
rate_mean = zeros(n, 1);
eta_scale = zeros(n, 1);

for i = 1:n
    pre_mean(i) = 0.15 + 0.10 * sin((2 * %pi * t_ms(i)) / 120.0);
    post_mean(i) = 0.20 + 0.08 * sin((2 * %pi * t_ms(i)) / 160.0 + 0.5);
    rate_mean(i) = 0.12 + 0.05 * sin((2 * %pi * t_ms(i)) / 200.0 + 1.0);
    eta_scale(i) = aarnn_triplet_eta_scale(pre_mean(i), post_mean(i), rate_mean(i), 0.25, 0.15);
end

weights_before = [0.30 -0.25 0.10; 0.55 0.20 -0.45];
weights_scaled = aarnn_apply_synaptic_scaling_rows(weights_before, 0.02, 1.0);
weights_dale = aarnn_apply_dale(weights_scaled, [0 1 0], 0.75, 1.0);

t_s = t_ms / 1000.0;
triplet_pre_mean = aarnn_signal(t_s, pre_mean);
triplet_post_mean = aarnn_signal(t_s, post_mean);
triplet_rate_mean = aarnn_signal(t_s, rate_mean);
triplet_eta_scale = aarnn_signal(t_s, eta_scale);
triplet_weights_before = weights_before;
triplet_weights_after_scaling = weights_scaled;
triplet_weights_after_dale = weights_dale;
weight_summary = [
    sum(abs(weights_before(1, :))) sum(abs(weights_scaled(1, :))) sum(abs(weights_dale(1, :)));
    sum(abs(weights_before(2, :))) sum(abs(weights_scaled(2, :))) sum(abs(weights_dale(2, :)))
];

clf();
subplot(2, 1, 1);
plot(t_ms, [pre_mean post_mean rate_mean eta_scale]);
xtitle("Triplet modulation and rate terms", "time (ms)", "value");
subplot(2, 1, 2);
bar(weight_summary);
xtitle("Row-wise weight magnitude before/after hybrid supervision", "row", "sum |w|");
