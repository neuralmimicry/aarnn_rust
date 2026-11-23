// SPDX-FileCopyrightText: masterkiga
// SPDX-License-Identifier: MIT
//
// Licensed under the MIT License. See LICENSE in the project root.
// Offline vectors for the Unity legacy-body output decoder (NmOutputDecode.cs).
// Expected values are the MuJoCo reference (crates/aarnn-embodiment
// Actuation / Muscles) on the same inputs; tolerances repeat the Rust tests'.
// C# 9 (Unity 2022.3). Run via scripts/qa/test_legacy_output_decode.py.
using System;
using NeuralMimicry;

static class Program
{
    static int failures;
    static void Require(bool ok, string what)
    {
        if (!ok) { failures++; Console.WriteLine("FAIL " + what); }
    }
    static void Near(double got, double want, double tol, string what) =>
        Require(Math.Abs(got - want) <= tol, $"{what}: {got:R} vs {want:R}");

    // graded_position_readout_spans_the_full_joint_range (actuation.rs)
    static void GradedPositionReadout()
    {
        const double lo = -1.4, hi = -0.2; // hexapod.json femur control_range
        const int joint = 1, channel = 1;  // hex_o_001_lf_femur, readout channel 1, coefficient 1
        var reference = new System.Collections.Generic.Dictionary<double, (int step, double value)[]>
        {
            [0.0] = new[] { (1, -1.39999999999999991), (400, -1.39999999999999991) },
            [0.5] = new[] { (1, -1.39999999999999991), (2, -1.28580490164315142), (7, -1.14281136413280393), (50, -0.774269713354299283), (399, -0.829975024974728415), (400, -0.770024975025272451) },
            [1.0] = new[] { (1, -1.28580490164315142), (2, -1.18247690369357805), (7, -0.795902364549691099), (50, -0.208085536398902482), (400, -0.200000000000000622) },
            [0.3] = new[] { (7, -1.20120709206785703), (50, -0.989156262868102099), (399, -1.06907241572953127), (400, -0.986369240734982999) },
        };
        foreach (var density in new[] { 0.0, 0.5, 1.0, 0.3 })
        {
            var joints = NmGradedJoints.Identity(18);
            double phase = 0;
            for (int step = 1; step <= 400; step++)
            {
                phase += density;
                var spikes = new float[18];
                if (phase >= 1.0 - 1e-12) { phase -= 1.0; spikes[channel] = 1; }
                joints.Accept(spikes);
                joints.Advance();
                foreach (var (s, v) in reference[density])
                    if (s == step) Near(joints.Target(joint, lo, hi), v, 1e-12, $"density {density} step {step} matches Rust");
            }
            double control = joints.Target(joint, lo, hi), expected = lo + density * (hi - lo);
            if (density != 0.3) Near(control, expected, 0.08 * (hi - lo), $"density {density} spans the joint range");
            // Graded joints stay stiff: silence commands lo, never a limp neutral/mid.
            if (density == 0.0) Near(control, lo, 1e-12, "density 0 holds the lower limit");
            Require(joints.Target(0, -0.7, 0.7) == -0.7, "unexercised joint rests at lo");
        }
    }

    // pulse_density_code_decodes_linearly (celegans.rs)
    static void PulseDensityDecodesLinearly()
    {
        const int mdr05 = 28; // celegans_o_028_MDR05
        var reference = new System.Collections.Generic.Dictionary<double, (double pd, double spk, double lastPd, double lastSpk)>
        {
            [0.2] = (0.200000000000000094, 0.670853663238015319, 0.214803295322949700, 0.720507887714990636),
            [0.5] = (0.499999999999999889, 0.844004043797125125, 0.509089907470285929, 0.859347881122468404),
        };
        foreach (var (p, floor) in new[] { (0.2, 0.6), (0.5, 0.8) })
        {
            var graded = new NmMuscleActivations(96);
            var spiking = new NmMuscleActivations(96);
            double phase = 0, sumG = 0, sumS = 0, n = 0;
            for (int frame = 0; frame < 5000; frame++)
            {
                phase += p;
                var spikes = new float[96];
                if (phase >= 1.0) { phase -= 1.0; spikes[mdr05] = 1; }
                graded.Advance(spikes, NmOutputCode.PulseDensity);
                spiking.Advance(spikes, NmOutputCode.Spikes);
                if (frame >= 2500) { sumG += graded[mdr05]; sumS += spiking[mdr05]; n++; }
            }
            Near(sumG / n, p, 0.01, $"p {p} linear decode");
            Require(sumS / n > floor, $"p {p} twitch compresses: {sumS / n}");
            var r = reference[p];
            Near(sumG / n, r.pd, 1e-12, $"p {p} pulse-density mean matches Rust");
            Near(sumS / n, r.spk, 1e-12, $"p {p} twitch mean matches Rust");
            Near(graded[mdr05], r.lastPd, 1e-12, $"p {p} final pulse-density state matches Rust");
            Near(spiking[mdr05], r.lastSpk, 1e-12, $"p {p} final twitch state matches Rust");
            Require(graded[0] == 0 && spiking[95] == 0, "silent muscles stay at rest");
        }
    }

    static void Handshake()
    {
        void Expect(string json, bool ok, NmOutputCode want)
        {
            bool got = NmOutputDecode.TryParseHandshake(json, out var code, out var error);
            Require(got == ok && (!ok || code == want), $"handshake {json} -> {got}/{code}/{error}");
            Require(ok || !string.IsNullOrEmpty(error), $"refusal explains itself: {json}");
        }
        Expect("{\"expected_o\": 96}", true, NmOutputCode.Spikes);
        Expect("{\"output_code\": \"pulse_density\"}", true, NmOutputCode.PulseDensity);
        Expect("{\"output_code\": \"analog\"}", false, default);
        Expect("{\"output_code\": null}", true, NmOutputCode.Spikes);
        Expect("{\"output_code\":\"spikes\"}", true, NmOutputCode.Spikes);
        Expect("{\"output_code\": 1}", false, default);
        Expect("{\"output_code\": [\"pulse_density\"]}", false, default);
        Expect("{\"expected_s\":34,\"expected_o\":18,\"output_code\":\"pulse_density\"}", true, NmOutputCode.PulseDensity);
        Expect("{\"brain_id\":\"output_code\",\"meta\":{\"output_code\":\"analog\"},\"list\":[\"output_code\",{\"x\":\"}\"}],\"output_code\":\"pulse_\\u0064ensity\"}", true, NmOutputCode.PulseDensity);
        Expect("{\"meta\":{\"output_code\":\"pulse_density\"}}", true, NmOutputCode.Spikes);
        Expect("", true, NmOutputCode.Spikes);
        Expect("not json", true, NmOutputCode.Spikes);
        Expect("{}", true, NmOutputCode.Spikes);
    }

    static void MixedSignAndReset()
    {
        // zebrafish-style antagonist pair: +1 * ch0, -1 * ch1 -> centred readout.
        var j = new NmGradedJoints(new[] { new[] { new NmReadout(0, 1.0), new NmReadout(1, -1.0) } });
        Near(j.Target(0, -1.2, 1.2), 0.0, 1e-15, "mixed-sign rest is the midpoint");
        for (int i = 0; i < 400; i++) { j.Accept(new float[] { 0, 1 }); j.Advance(); }
        Near(j.Rate(0), -1.0, 1e-12, "antagonist saturates");
        Near(j.Target(0, -1.2, 1.2), -1.2, 1e-12, "mixed-sign spans to lo");
        j.Accept(new float[] { 5, 0 }); // counts clamp to one event per frame
        j.Advance();
        Near(j.Rate(0), -1.0 * NmOutputDecode.RateKeep + (1 - NmOutputDecode.RateKeep), 1e-12, "event count clamps to 1");
        j.Reset();
        Require(j.Rate(0) == 0, "reset disarms");
        Near(NmOutputDecode.HoldMs, 100.0, 0, "hold matches 50 reference frames");
    }

    // NAO decode_tau_s + wake (actuation.rs wake_rate / armed_frames), printed by
    // scripts/qa/output_decode_reference on generated/nao.json: A = LKneePitch
    // as generated (wake from 0 over 1 s, tau 0.2 s; silent for 20 replies, then
    // density 0.37); B = LHipPitch with wake {-0.45, 0.25 s}, tau 0.05 s, density
    // 0.8; C = HeadYaw with a seed-only wake {0.5, 0 s}, default tau, silent.
    // Replies 1..1500, a disarm (Reset), then replies 1501..1600 re-arm.
    static void WakeAndDecodeTau()
    {
        int[] channels = { 9, 6, 1 };
        double[] lo = { -0.092346, -1.535889, -2.0857 }, hi = { 2.112528, 0.48409, 2.0857 };
        var j = new NmGradedJoints(new[]
        {
            new[] { new NmReadout(channels[0], 1.0) },
            new[] { new NmReadout(channels[1], 1.0) },
            new[] { new NmReadout(channels[2], 1.0) },
        });
        j.SetDecodeTau(0, 0.2); j.SetWake(0, 0.0, 1.0, lo[0], hi[0]);
        j.SetDecodeTau(1, 0.05); j.SetWake(1, -0.45, 0.25, lo[1], hi[1]);
        j.SetWake(2, 0.5, 0.0, lo[2], hi[2]);
        // reply -> (control, rate) per joint A, B, C
        var reference = new System.Collections.Generic.Dictionary<int, (double c, double r)[]>
        {
            [1] = new[] { (-1.8377161051986446e-6, 0.041465925920211616), (-0.4503406265336666, 0.5164958067839701), (0.253938111815581, 0.5608759917091578) },
            [2] = new[] { (-7.314293259257477e-6, 0.04105333306356083), (-0.4500685216679685, 0.5354542773721742), (0.03129210823373807, 0.5075015841764726) },
            [10] = new[] { (-0.00017575767588102505, 0.037897002824627955), (-0.43518415950492084, 0.6292575349488729), (-1.134474128963002, 0.22803516110586328) },
            [21] = new[] { (-0.0007346550633087554, 0.03394942875573069), (-0.40722178487498195, 0.6636312718533274), (-1.769064413466265, 0.07590631119857483) },
            [125] = new[] { (0.11317488826487161, 0.2472003175961467), (0.10796583816054633, 0.8137979841179271), (-2.085690363992893, 2.3100175257812116e-6) },
            [126] = new[] { (0.11824221075770995, 0.2546907995896385), (0.04350936800410432, 0.7818885087439544), (-2.085691280980209, 2.090190293645687e-6) },
            [500] = new[] { (0.7149185313802094, 0.36612728499687935), (0.1117508846982797, 0.8156717890127965), (-2.0857, 1.1955622769046709e-22) },
            [501] = new[] { (0.7068861150845764, 0.3624842576421947), (0.047146000744258565, 0.7836888406979768), (-2.0857, 1.0817894837356152e-22) },
            [1500] = new[] { (0.7210995014506245, 0.36893060621633006), (0.11175088585613419, 0.8156717895859977), (-2.0857, 4.4475825041495715e-66) },
            [1501] = new[] { (-1.8377161051986446e-6, 0.041465925920211616), (-0.4503406265336666, 0.5164958067839701), (0.253938111815581, 0.5608759917091578) },
            [1502] = new[] { (8.044115818928937e-5, 0.05100349931439272), (-0.4500685216679685, 0.5354542773721742), (0.03129210823373807, 0.5075015841764726) },
            [1540] = new[] { (0.019366678750649135, 0.15167736767865833), (-0.3065326918331624, 0.7595248950713683), (-2.0383412525254, 0.011353202156254462) },
            [1600] = new[] { (0.09116439944378199, 0.248616472968029), (-0.008830352411706655, 0.8105782582320741), (-2.085582609401613, 2.8141774556948843e-5) },
        };
        var phase = new double[3];
        double expected = lo[0] + 0.37 * (hi[0] - lo[0]), sum = 0, worst = 0;
        var tail = new System.Collections.Generic.List<double>();
        for (int reply = 1; reply <= 1600; reply++)
        {
            if (reply == 1501) j.Reset();
            var events = new float[40];
            for (int k = 0; k < 3; k++)
            {
                phase[k] += k == 0 ? (reply > 20 ? 0.37 : 0.0) : k == 1 ? 0.8 : 0.0;
                if (phase[k] >= 1.0 - 1e-12) { phase[k] -= 1.0; events[channels[k]] = 1; }
            }
            j.Accept(events);
            j.Advance();
            if (reference.TryGetValue(reply, out var want))
                for (int k = 0; k < 3; k++)
                {
                    Near(j.Target(k, lo[k], hi[k]), want[k].c, 1e-12, $"wake joint {k} reply {reply} control matches Rust");
                    Near(j.Rate(k), want[k].r, 1e-12, $"wake joint {k} reply {reply} rate matches Rust");
                }
            if (reply <= 20 || (reply > 1500 && reply <= 1520))
                Near(j.Target(0, lo[0], hi[0]), 0.0, 0.05, $"reply {reply} holds the rest pose");
            if (reply > 1000 && reply <= 1500) tail.Add(j.Target(0, lo[0], hi[0]));
        }
        foreach (var c in tail) sum += c;
        double mean = sum / tail.Count;
        foreach (var c in tail) worst = Math.Max(worst, Math.Abs(c - mean));
        Near(mean, expected, 0.03 * (hi[0] - lo[0]), "slow decode reaches the density target");
        Require(worst < 0.02 * (hi[0] - lo[0]), $"slow decode ripple {worst}");
        Near(j.NormalizedTarget(0, lo[0], hi[0]), (j.Target(0, lo[0], hi[0]) - lo[0]) / (hi[0] - lo[0]), 1e-15, "normalized target");
        Require(j.ArmedFrames == 100, "re-armed frame count");
    }

    static int Main()
    {
        GradedPositionReadout();
        PulseDensityDecodesLinearly();
        Handshake();
        MixedSignAndReset();
        WakeAndDecodeTau();
        Console.WriteLine(failures == 0 ? "PASS NmOutputDecode (C#)" : $"{failures} failure(s)");
        return failures == 0 ? 0 : 1;
    }
}
