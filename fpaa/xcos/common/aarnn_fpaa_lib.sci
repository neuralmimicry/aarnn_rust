// Common Scilab helpers for FPAA-oriented AARNN kernel simulation.

function y = aarnn_clamp(x, lo, hi)
    if x < lo then
        y = lo;
    elseif x > hi then
        y = hi;
    else
        y = x;
    end
endfunction

function y = aarnn_sigmoid(x)
    y = 1.0 / (1.0 + exp(-aarnn_clamp(x, -60.0, 60.0)));
endfunction

function s = aarnn_signal(time_s, values)
    s = struct("time", time_s, "values", values);
endfunction

function [ampa, nmda, gaba, out] = aarnn_synaptic_filter(raw, vmem, dt_ms, ampa_tau_ms, nmda_tau_ms, gaba_tau_ms, nmda_ratio, synaptic_gain, nmda_voltage_sensitivity, ach_gain)
    n = size(raw, 1);
    ampa = zeros(n, 1);
    nmda = zeros(n, 1);
    gaba = zeros(n, 1);
    out = zeros(n, 1);
    state_ampa = 0.0;
    state_nmda = 0.0;
    state_gaba = 0.0;
    decay_ampa = exp(-(dt_ms / max(ampa_tau_ms, 1.0e-6)));
    decay_nmda = exp(-(dt_ms / max(nmda_tau_ms, 1.0e-6)));
    decay_gaba = exp(-(dt_ms / max(gaba_tau_ms, 1.0e-6)));

    for i = 1:n
        val = raw(i);
        exc = max(val, 0.0);
        inh = max(-val, 0.0);
        gate = 1.0;
        if nmda_voltage_sensitivity > 0.0 then
            gate = aarnn_sigmoid(nmda_voltage_sensitivity * (vmem(i) + 40.0));
        end
        state_ampa = state_ampa * decay_ampa + exc * (1.0 - nmda_ratio);
        state_nmda = state_nmda * decay_nmda + exc * nmda_ratio * gate;
        state_gaba = state_gaba * decay_gaba + inh;
        ampa(i) = state_ampa;
        nmda(i) = state_nmda;
        gaba(i) = state_gaba;
        out(i) = (state_ampa + state_nmda - state_gaba) * synaptic_gain * ach_gain;
    end
endfunction

function [utilization, resources, release] = aarnn_stp(pre_spikes, dt_ms, baseline_u, tau_rec_ms, tau_facil_ms)
    n = size(pre_spikes, 1);
    utilization = zeros(n, 1);
    resources = zeros(n, 1);
    release = zeros(n, 1);
    u_state = baseline_u;
    x_state = 1.0;
    rec_decay = exp(-(dt_ms / max(tau_rec_ms, 1.0e-6)));
    facil_decay = exp(-(dt_ms / max(tau_facil_ms, 1.0e-6)));

    for i = 1:n
        u_state = u_state * facil_decay + baseline_u * (1.0 - facil_decay);
        x_state = x_state * rec_decay + (1.0 - rec_decay);
        if pre_spikes(i) <> 0 then
            rel = aarnn_clamp(u_state * x_state, 0.0, 1.0);
            x_state = max(x_state - rel, 0.0);
            u_state = aarnn_clamp(u_state + baseline_u * (1.0 - u_state), 0.0, 1.0);
        else
            rel = 0.0;
        end
        utilization(i) = u_state;
        resources(i) = x_state;
        release(i) = rel;
    end
endfunction

function [thr_offset, rate_ema] = aarnn_adaptive_threshold_homeostasis(spikes, dt_ms, thr_tau_ms, thr_inc, thr_min, thr_max, homeo_target_hz, homeo_tau_ms, homeo_gain)
    n = size(spikes, 1);
    thr_offset = zeros(n, 1);
    rate_ema = zeros(n, 1);
    thr_state = 0.0;
    rate_state = 0.0;
    thr_decay = exp(-(dt_ms / max(thr_tau_ms, 1.0e-6)));
    homeo_decay = exp(-(dt_ms / max(homeo_tau_ms, 1.0e-6)));
    base_homeo_target = homeo_target_hz * dt_ms / 1000.0;

    for i = 1:n
        thr_state = thr_state * thr_decay;
        rate_state = rate_state * homeo_decay;
        if spikes(i) <> 0 then
            thr_state = aarnn_clamp(thr_state + thr_inc, thr_min, thr_max);
            rate_state = rate_state + (1.0 - homeo_decay);
        end
        thr_state = aarnn_clamp(thr_state + homeo_gain * (rate_state - base_homeo_target), thr_min, thr_max);
        thr_offset(i) = thr_state;
        rate_ema(i) = rate_state;
    end
endfunction

function [ca_state_trace, plateau_trace, out_curr] = aarnn_active_dendrite(curr_in, local_stimulus, branching_gain, dt_ms, calcium_tau_ms, plateau_tau_ms, calcium_influx_gain, plateau_threshold, plateau_gain)
    n = size(curr_in, 1);
    ca_state_trace = zeros(n, 1);
    plateau_trace = zeros(n, 1);
    out_curr = zeros(n, 1);
    ca_state = 0.0;
    plateau_state = 0.0;
    ca_decay = exp(-(dt_ms / max(calcium_tau_ms, 1.0)));
    plateau_decay = exp(-(dt_ms / max(plateau_tau_ms, 1.0)));

    for i = 1:n
        branch_factor = aarnn_clamp(branching_gain(i), 1.0, 3.0);
        exc = max(curr_in(i), 0.0);
        drive = 0.75 * exc + 0.25 * max(local_stimulus(i), 0.0) * branch_factor;
        ca_state = aarnn_clamp(ca_state * ca_decay + max(calcium_influx_gain, 0.0) * drive, 0.0, 1.0e6);
        over = max(ca_state - max(plateau_threshold, 0.0), 0.0);
        trigger = over / (1.0 + over);
        plateau_state = aarnn_clamp(plateau_state * plateau_decay + trigger * (1.0 - plateau_decay), 0.0, 1.0);
        gain = aarnn_clamp(1.0 + max(plateau_gain, 0.0) * plateau_state * branch_factor, 1.0, 3.0);
        if curr_in(i) >= 0.0 then
            out_curr(i) = curr_in(i) * gain;
        else
            out_curr(i) = curr_in(i) * (1.0 + 0.25 * (gain - 1.0));
        end
        ca_state_trace(i) = ca_state;
        plateau_trace(i) = plateau_state;
    end
endfunction

function delta = aarnn_gap_junction_step(v, positions, strength, radius, inhibitory_only, inhibitory_mask)
    n = size(v, 1);
    delta = zeros(n, 1);
    if strength <= 0.0 | radius <= 0.0 | n < 2 then
        return;
    end
    for j = 1:n
        if inhibitory_only & (inhibitory_mask(j) == 0) then
            continue;
        end
        sum_term = 0.0;
        weight_sum = 0.0;
        for i = 1:n
            if i == j then
                continue;
            end
            if inhibitory_only & (inhibitory_mask(i) == 0) then
                continue;
            end
            dx = positions(i, 1) - positions(j, 1);
            dy = positions(i, 2) - positions(j, 2);
            dz = positions(i, 3) - positions(j, 3);
            d = sqrt(dx * dx + dy * dy + dz * dz);
            if (d <= radius) & (d > 1.0e-9) then
                w = max(1.0 - d / radius, 0.0);
                sum_term = sum_term + w * (v(i) - v(j));
                weight_sum = weight_sum + w;
            end
        end
        if weight_sum > 1.0e-9 then
            delta(j) = strength * (sum_term / weight_sum);
        end
    end
endfunction

function factors = aarnn_volume_transmission(positions, sources, radius, strength, tone)
    neuron_count = size(positions, 1);
    source_count = size(sources, 1);
    factors = ones(neuron_count, 1);
    if neuron_count == 0 | source_count == 0 | radius <= 0.0 | strength <= 0.0 then
        return;
    end
    tone_scale = aarnn_clamp(tone, 0.0, 3.0) / 3.0;
    two_sigma2 = 2.0 * radius * radius;
    for j = 1:neuron_count
        field = 0.0;
        for s = 1:source_count
            dx = positions(j, 1) - sources(s, 1);
            dy = positions(j, 2) - sources(s, 2);
            dz = positions(j, 3) - sources(s, 3);
            d2 = dx * dx + dy * dy + dz * dz;
            if d2 <= radius * radius then
                field = field + exp(-(d2 / two_sigma2));
            end
        end
        factors(j) = aarnn_clamp(1.0 + strength * tone_scale * field, 0.5, 2.5);
    end
endfunction

function jitter_steps = aarnn_deterministic_jitter_steps(base_steps, dt_ms, jitter_ms, synapse_index, time_seed)
    if jitter_ms <= 0.0 then
        jitter_steps = base_steps;
        return;
    end
    max_j = round(jitter_ms / max(dt_ms, 1.0e-6));
    if max_j == 0 then
        jitter_steps = base_steps;
        return;
    end
    frac = sin((time_seed + 1.0) * 12.9898 + (synapse_index + 1.0) * 78.233);
    frac = frac - floor(frac);
    jitter = round(((2.0 * frac) - 1.0) * max_j);
    jitter_steps = max(base_steps + jitter, 0.0);
endfunction

function [steps, attenuation] = aarnn_compute_delay_and_attenuation(depth, dt_ms, time_seed, synapse_index, axon_steps, dendrite_steps, bouton_latency_steps, jitter_ms, attenuation_per_unit, axon_length, dendrite_length, path_length_scale, compartment_code, trunk_length, forward_gain, backprop_gain, is_backward_path, myelin_level, myelin_min_gain, myelin_max_gain, axon_atp, dendrite_atp)
    steps = axon_steps + dendrite_steps + bouton_latency_steps;
    if depth >= 2 then
        steps = aarnn_deterministic_jitter_steps(steps, dt_ms, jitter_ms, synapse_index, time_seed);
    end

    attenuation = 1.0;
    atten_k = max(attenuation_per_unit, 0.0);
    if atten_k > 0.0 then
        normalized_dist = max(axon_length + dendrite_length, 0.0) / max(path_length_scale, 1.0e-3);
        attenuation = aarnn_clamp(exp(-atten_k * normalized_dist), 1.0e-2, 1.0);
    end

    if compartment_code >= 0 then
        trunk_norm = max(trunk_length, 0.0) / max(path_length_scale, 1.0e-3);
        attenuation = attenuation * aarnn_clamp(forward_gain, 0.25, 3.0);
        if is_backward_path <> 0 then
            attenuation = attenuation * aarnn_clamp(backprop_gain, 0.25, 3.0);
            steps = round(steps / max(backprop_gain, 1.0e-3));
        end
        if compartment_code == 1 then
            trunk_delay = 1.0 + 0.45 * trunk_norm;
        elseif compartment_code == 2 then
            trunk_delay = 1.0 + 0.20 * trunk_norm;
        else
            trunk_delay = 1.0 + 0.30 * trunk_norm;
        end
        steps = round(steps * max(trunk_delay, 0.1));
    end

    if myelin_level >= 0.0 then
        level = aarnn_clamp(myelin_level, 0.0, 1.0);
        min_gain = max(myelin_min_gain, 0.1);
        max_gain = max(myelin_max_gain, min_gain + 1.0e-3);
        conduction_gain = max(min_gain + (max_gain - min_gain) * level, 1.0e-3);
        steps = round(steps / conduction_gain);
        attenuation = attenuation * aarnn_clamp(0.9 + 0.1 * level, 0.5, 1.1);
    end

    if depth >= 3 then
        fatigue_level = aarnn_clamp(axon_atp * dendrite_atp, 0.01, 1.0);
        if fatigue_level < 0.5 then
            steps = round(steps * (1.0 + (0.5 - fatigue_level)));
        end
    end
endfunction

function scale = aarnn_triplet_eta_scale(pre_mean, post_mean, rate_mean, ltp_gain, ltd_gain)
    triplet_mod = max(ltp_gain, 0.0) * pre_mean * post_mean - max(ltd_gain, 0.0) * rate_mean;
    scale = aarnn_clamp(1.0 + triplet_mod, 0.05, 5.0);
endfunction

function out = aarnn_apply_synaptic_scaling_rows(mat, strength, target)
    out = mat;
    [rows, cols] = size(out);
    if strength <= 0.0 | target <= 0.0 then
        return;
    end
    for r = 1:rows
        sum_abs = 0.0;
        for c = 1:cols
            sum_abs = sum_abs + abs(out(r, c));
        end
        if sum_abs > 1.0e-9 then
            desired_ratio = aarnn_clamp(target / sum_abs, 0.25, 4.0);
            scale = 1.0 + strength * (desired_ratio - 1.0);
            for c = 1:cols
                out(r, c) = out(r, c) * scale;
            end
        end
    end
endfunction

function out = aarnn_apply_dale(mat, inhibitory_mask, strictness, max_abs_w)
    out = mat;
    [rows, cols] = size(out);
    if strictness <= 0.0 then
        return;
    end
    for r = 1:rows
        for c = 1:cols
            w = out(r, c);
            if inhibitory_mask(c) <> 0 then
                target = -abs(w);
            else
                target = abs(w);
            end
            out(r, c) = aarnn_clamp(w + strictness * (target - w), -max_abs_w, max_abs_w);
        end
    end
endfunction
