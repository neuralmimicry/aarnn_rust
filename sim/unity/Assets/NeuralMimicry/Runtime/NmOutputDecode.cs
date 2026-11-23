// SPDX-FileCopyrightText: masterkiga
// SPDX-License-Identifier: MIT
//
// Licensed under the MIT License. See LICENSE in the project root.
// NmOutputDecode.cs — brain output decoding shared by the legacy robot bodies.
// Mirrors the MuJoCo reference in crates/aarnn-embodiment: OutputCode
// negotiation (aer.rs), graded weighted readouts (actuation.rs) and the
// C. elegans muscle filter (celegans.rs). Pure C# (no UnityEngine) so the
// numerics are unit-testable outside the editor.

using System;
using System.Text;

namespace NeuralMimicry
{
    /// <summary>
    /// Encoding of the brain's output events, advertised once by the brain in
    /// its handshake reply (<c>output_code</c>). Both arrive as per-frame
    /// event counts.
    /// </summary>
    public enum NmOutputCode
    {
        /// <summary>Spike trains of spiking output cells (no key, or "spikes").</summary>
        Spikes,
        /// <summary>
        /// Pulse-density code of graded activations ("pulse_density"): a frame
        /// carries at most one event and the fraction of 2 ms frames with an
        /// event equals the activation.
        /// </summary>
        PulseDensity,
    }

    /// <summary>Contract constants and handshake negotiation.</summary>
    public static class NmOutputDecode
    {
        /// <summary>Contract brain frame (aer.rs announces dt_ms = 2).</summary>
        public const double FrameMs = 2.0;
        /// <summary>Motor rate time constant (motor.rs RATE_TAU_S).</summary>
        public const double RateTauMs = 20.0;
        /// <summary>Muscle activation time constant (celegans.rs MUSCLE_TAU_MS).</summary>
        public const double MuscleTauMs = 55.0;
        /// <summary>Fraction of the remaining contraction recruited by one spike (celegans.rs TWITCH).</summary>
        public const double Twitch = 0.3;
        /// <summary>
        /// A graded brain silent for longer than this disarms (actuation.rs:
        /// 50 frames without a reply).
        /// </summary>
        public const double HoldMs = 50 * FrameMs;
        /// <summary>Per-frame retention of a graded motor rate.</summary>
        public static readonly double RateKeep = Math.Exp(-FrameMs / RateTauMs);
        /// <summary>Per-frame retention of a muscle activation.</summary>
        public static readonly double MuscleDecay = Math.Exp(-FrameMs / MuscleTauMs);

        /// <summary>
        /// <c>OutputCode::from_handshake</c>: an absent or null
        /// <c>output_code</c> means spikes; "spikes" and "pulse_density" are
        /// accepted; any other value is refused (returns false). A reply that
        /// is not a JSON object keeps the legacy behaviour (spikes).
        /// </summary>
        public static bool TryParseHandshake(string json, out NmOutputCode code, out string error)
        {
            code = NmOutputCode.Spikes;
            error = null;
            var scanner = new Scanner(json ?? "");
            if (!scanner.FindTopLevel("output_code", out var kind, out var text))
                return true;
            if (kind == ValueKind.Null)
                return true;
            if (kind == ValueKind.String && text == "spikes")
                return true;
            if (kind == ValueKind.String && text == "pulse_density")
            {
                code = NmOutputCode.PulseDensity;
                return true;
            }
            error = "unsupported brain output_code " + (kind == ValueKind.String ? "\"" + text + "\"" : text);
            return false;
        }

        /// <summary>A frame carries at most one event: clamp a count to [0, 1].</summary>
        public static double Event(float count) =>
            float.IsNaN(count) || count <= 0f ? 0.0 : Math.Min(1.0, count);

        private enum ValueKind { String, Null, Other }

        /// <summary>Minimal scanner for one key of a top-level JSON object.</summary>
        private struct Scanner
        {
            private readonly string _s;
            private int _p;

            public Scanner(string s)
            {
                _s = s;
                _p = 0;
            }

            /// <summary>
            /// Finds the last top-level occurrence of <paramref name="key"/>
            /// (serde_json keeps the last duplicate). Returns false when the key
            /// is absent or the document is not a well-formed object prefix.
            /// </summary>
            public bool FindTopLevel(string key, out ValueKind kind, out string text)
            {
                kind = ValueKind.Other;
                text = null;
                bool found = false;
                Skip();
                if (!Eat('{')) return false;
                Skip();
                if (Eat('}')) return false;
                while (true)
                {
                    Skip();
                    if (!ReadString(out var name)) return found;
                    Skip();
                    if (!Eat(':')) return found;
                    Skip();
                    int start = _p;
                    if (Peek() == '"')
                    {
                        if (!ReadString(out var value)) return found;
                        if (name == key) { found = true; kind = ValueKind.String; text = value; }
                    }
                    else
                    {
                        if (!SkipValue()) return found;
                        if (name == key)
                        {
                            found = true;
                            text = _s.Substring(start, _p - start).Trim();
                            kind = text == "null" ? ValueKind.Null : ValueKind.Other;
                        }
                    }
                    Skip();
                    if (Eat(',')) continue;
                    return found;
                }
            }

            private char Peek() => _p < _s.Length ? _s[_p] : '\0';

            private bool Eat(char c)
            {
                if (Peek() != c) return false;
                _p++;
                return true;
            }

            private void Skip()
            {
                while (_p < _s.Length && (_s[_p] == ' ' || _s[_p] == '\t' || _s[_p] == '\n' || _s[_p] == '\r'))
                    _p++;
            }

            private bool ReadString(out string value)
            {
                value = null;
                if (!Eat('"')) return false;
                var sb = new StringBuilder();
                while (_p < _s.Length)
                {
                    char c = _s[_p++];
                    if (c == '"') { value = sb.ToString(); return true; }
                    if (c != '\\') { sb.Append(c); continue; }
                    if (_p >= _s.Length) return false;
                    char e = _s[_p++];
                    switch (e)
                    {
                        case '"': sb.Append('"'); break;
                        case '\\': sb.Append('\\'); break;
                        case '/': sb.Append('/'); break;
                        case 'b': sb.Append('\b'); break;
                        case 'f': sb.Append('\f'); break;
                        case 'n': sb.Append('\n'); break;
                        case 'r': sb.Append('\r'); break;
                        case 't': sb.Append('\t'); break;
                        case 'u':
                            if (_p + 4 > _s.Length) return false;
                            int code = 0;
                            for (int i = 0; i < 4; i++)
                            {
                                int d = HexValue(_s[_p + i]);
                                if (d < 0) return false;
                                code = code * 16 + d;
                            }
                            _p += 4;
                            sb.Append((char)code);
                            break;
                        default: return false;
                    }
                }
                return false;
            }

            private static int HexValue(char c) =>
                c >= '0' && c <= '9' ? c - '0'
                : c >= 'a' && c <= 'f' ? c - 'a' + 10
                : c >= 'A' && c <= 'F' ? c - 'A' + 10
                : -1;

            /// <summary>Skips a non-string value: object, array or literal.</summary>
            private bool SkipValue()
            {
                char c = Peek();
                if (c == '{' || c == '[')
                {
                    int depth = 0;
                    while (_p < _s.Length)
                    {
                        char d = _s[_p];
                        if (d == '"') { if (!ReadString(out _)) return false; continue; }
                        _p++;
                        if (d == '{' || d == '[') depth++;
                        else if ((d == '}' || d == ']') && --depth == 0) return true;
                    }
                    return false;
                }
                int start = _p;
                while (_p < _s.Length && _s[_p] != ',' && _s[_p] != '}' && _s[_p] != ']')
                    _p++;
                return _p > start;
            }
        }
    }

    /// <summary>One weighted output channel of a joint readout.</summary>
    public readonly struct NmReadout
    {
        public readonly int Channel;
        public readonly double Coefficient;

        public NmReadout(int channel, double coefficient)
        {
            Channel = channel;
            Coefficient = coefficient;
        }
    }

    /// <summary>
    /// Graded (pulse-density) decode of weighted joint readouts, as
    /// <c>Actuation</c> does for a brain that negotiated
    /// <c>output_code = pulse_density</c>. Call <see cref="Accept"/> with each
    /// brain frame and <see cref="Advance"/> once per 2 ms frame.
    /// Optional per-joint behaviour mirrors the actuator record
    /// (aer.rs <c>decode_tau_s</c>, <c>wake</c>): see <see cref="SetDecodeTau"/>
    /// and <see cref="SetWake"/>. Unconfigured joints decode exactly as before.
    /// </summary>
    public sealed class NmGradedJoints
    {
        /// <summary>Reference frame in seconds (actuation.rs: exp(-0.002 / tau)).</summary>
        private const double FrameS = 0.002;
        private readonly NmReadout[][] _readouts;
        private readonly bool[] _singleSign;
        private readonly double[] _rates;
        private readonly double[] _pending;
        private readonly double[] _keep;
        private readonly bool[] _hasWake;
        private readonly bool[] _hasWakeRate;
        private readonly double[] _wakeRate;
        private readonly double[] _wakeFrom;
        private readonly double[] _wakeSeconds;
        /// <summary>Advanced frames since the last <see cref="Reset"/> (actuation.rs armed_frames).</summary>
        private long _armedFrames;

        public NmGradedJoints(NmReadout[][] readouts)
        {
            if (readouts == null) throw new ArgumentNullException(nameof(readouts));
            _readouts = readouts;
            _singleSign = new bool[readouts.Length];
            _rates = new double[readouts.Length];
            _pending = new double[readouts.Length];
            _keep = new double[readouts.Length];
            _hasWake = new bool[readouts.Length];
            _hasWakeRate = new bool[readouts.Length];
            _wakeRate = new double[readouts.Length];
            _wakeFrom = new double[readouts.Length];
            _wakeSeconds = new double[readouts.Length];
            for (int i = 0; i < readouts.Length; i++)
                _keep[i] = NmOutputDecode.RateKeep;
            for (int i = 0; i < readouts.Length; i++)
            {
                var readout = readouts[i] ?? throw new ArgumentException("joint readout required");
                bool single = readout.Length > 0;
                foreach (var c in readout)
                {
                    if (c.Channel < 0 || double.IsNaN(c.Coefficient) || double.IsInfinity(c.Coefficient))
                        throw new ArgumentException("invalid readout channel");
                    single &= c.Coefficient >= 0.0;
                }
                _singleSign[i] = single;
            }
        }

        /// <summary>Joint <c>i</c> reads output channel <c>i</c> with coefficient 1.</summary>
        public static NmGradedJoints Identity(int joints)
        {
            var readouts = new NmReadout[joints][];
            for (int i = 0; i < joints; i++)
                readouts[i] = new[] { new NmReadout(i, 1.0) };
            return new NmGradedJoints(readouts);
        }

        public int Count => _rates.Length;

        /// <summary>
        /// Graded decode time constant of <paramref name="joint"/> in seconds
        /// (actuator <c>decode_tau_s</c>): keep = exp(-0.002 / tau) per frame.
        /// The default is motor.rs RATE_TAU_S (0.02 s).
        /// </summary>
        public void SetDecodeTau(int joint, double tauS)
        {
            if (!(tauS > 0.0) || double.IsInfinity(tauS))
                throw new ArgumentOutOfRangeException(nameof(tauS), "decode tau must be positive and finite");
            _keep[joint] = Math.Exp(-FrameS / tauS);
        }

        /// <summary>
        /// Servo wake-up of a graded position readout over [lo, hi] (actuator
        /// <c>wake {from, seconds}</c>): on the first frame after arming, the
        /// decoded rate starts at the rest-pose density (from - lo)/(hi - lo);
        /// <see cref="Target"/> then blends linearly from <paramref name="from"/>
        /// to the decoded target over <paramref name="seconds"/> of advanced frames.
        /// </summary>
        public void SetWake(int joint, double from, double seconds, double lo, double hi)
        {
            if (double.IsNaN(from) || double.IsInfinity(from) || double.IsNaN(seconds) || double.IsInfinity(seconds))
                throw new ArgumentOutOfRangeException(nameof(from), "wake must be finite");
            _hasWake[joint] = true;
            _wakeFrom[joint] = from;
            _wakeSeconds[joint] = seconds;
            _hasWakeRate[joint] = hi > lo;
            _wakeRate[joint] = hi > lo ? Math.Max(0.0, Math.Min(1.0, (from - lo) / (hi - lo))) : 0.0;
        }

        /// <summary>Frames advanced since arming (the last <see cref="Reset"/>).</summary>
        public long ArmedFrames => _armedFrames;

        /// <summary>Latest frame: pending = sum(coefficient * min(events, 1)).</summary>
        public void Accept(float[] events)
        {
            if (events == null) throw new ArgumentNullException(nameof(events));
            for (int i = 0; i < _readouts.Length; i++)
            {
                double drive = 0.0;
                foreach (var c in _readouts[i])
                    if (c.Channel < events.Length)
                        drive += c.Coefficient * NmOutputDecode.Event(events[c.Channel]);
                _pending[i] = drive;
            }
        }

        /// <summary>
        /// One 2 ms frame: rate = clamp(rate*keep + (1-keep)*pending, -1, 1);
        /// pending = 0. The first frame after arming seeds wake joints at
        /// their rest-pose density before filtering.
        /// </summary>
        public void Advance()
        {
            if (_armedFrames == 0)
                for (int i = 0; i < _rates.Length; i++)
                    if (_hasWakeRate[i])
                        _rates[i] = _wakeRate[i];
            for (int i = 0; i < _rates.Length; i++)
            {
                double keep = _keep[i];
                _rates[i] = Math.Max(-1.0, Math.Min(1.0, _rates[i] * keep + (1.0 - keep) * _pending[i]));
                _pending[i] = 0.0;
            }
            _armedFrames++;
        }

        /// <summary>Disarm: forget the decoded drive and restart any wake-up.</summary>
        public void Reset()
        {
            Array.Clear(_rates, 0, _rates.Length);
            Array.Clear(_pending, 0, _pending.Length);
            _armedFrames = 0;
        }

        public double Rate(int joint) => _rates[joint];

        /// <summary>
        /// Position of a graded position-readout joint within its range, 0..1:
        /// a single-sign readout spans the full range (clamp(rate, 0, 1)); a
        /// mixed-sign readout is centred (0.5 + 0.5 * rate).
        /// </summary>
        public double Normalized(int joint)
        {
            double rate = _rates[joint];
            double n = _singleSign[joint] ? rate : 0.5 + 0.5 * rate;
            return Math.Max(0.0, Math.Min(1.0, n));
        }

        /// <summary>
        /// Graded position target in [lo, hi] (actuation.rs controls). A wake
        /// joint with a single-sign readout blends from its wake pose to the
        /// decoded target over the wake time (t = advanced frames x 2 ms).
        /// </summary>
        public double Target(int joint, double lo, double hi)
        {
            double target = lo + Normalized(joint) * (hi - lo);
            if (_hasWake[joint] && _singleSign[joint] && _wakeSeconds[joint] > 0.0)
            {
                double t = _armedFrames * 2000.0 * 1e-6;
                double from = _wakeFrom[joint];
                target = from + Math.Max(0.0, Math.Min(1.0, t / _wakeSeconds[joint])) * (target - from);
            }
            return Math.Max(lo, Math.Min(hi, target));
        }

        /// <summary><see cref="Target"/> as a fraction of [lo, hi] (0..1).</summary>
        public double NormalizedTarget(int joint, double lo, double hi) =>
            hi > lo ? (Target(joint, lo, hi) - lo) / (hi - lo) : 0.0;
    }

    /// <summary>
    /// Per-muscle activation filter of celegans.rs: decay = exp(-2/55) per
    /// 2 ms frame; spikes sum saturating twitches, a pulse-density code is
    /// decoded linearly so a steady density p settles at activation p.
    /// </summary>
    public sealed class NmMuscleActivations
    {
        private readonly double[] _a;

        public NmMuscleActivations(int muscles)
        {
            _a = new double[muscles];
        }

        public int Count => _a.Length;

        public double this[int muscle] => _a[muscle];

        /// <summary>Advance every muscle by one 2 ms brain frame.</summary>
        public void Advance(float[] events, NmOutputCode code)
        {
            if (events == null) throw new ArgumentNullException(nameof(events));
            double decay = NmOutputDecode.MuscleDecay;
            for (int i = 0; i < _a.Length; i++)
            {
                double a = _a[i] * decay;
                double e = i < events.Length ? NmOutputDecode.Event(events[i]) : 0.0;
                _a[i] = code == NmOutputCode.PulseDensity
                    ? a + (1.0 - decay) * e
                    : a + NmOutputDecode.Twitch * e * (1.0 - a);
            }
        }

        public void Reset() => Array.Clear(_a, 0, _a.Length);
    }
}
