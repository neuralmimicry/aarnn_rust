// SPDX-FileCopyrightText: masterkiga
// SPDX-License-Identifier: MIT
//
// Licensed under the MIT License. See LICENSE in the project root.
#pragma once

// Brain output decoding shared by the legacy robot bodies. Mirrors the MuJoCo
// reference in crates/aarnn-embodiment: OutputCode negotiation (aer.rs),
// graded weighted readouts (actuation.rs) and the C. elegans muscle filter
// (celegans.rs). Standard C++ only, so the numerics are unit-testable
// outside the engine (scripts/qa/test_unreal_output_decode.cpp).

#include <algorithm>
#include <cmath>
#include <cstddef>
#include <cstdint>
#include <string>
#include <vector>

namespace NmOutputDecode
{
/** Encoding of the brain's output events, negotiated by the handshake reply. */
enum class EOutputCode : uint8_t
{
    /** Spike trains of spiking output cells (no key, or "spikes"). */
    Spikes,
    /** Graded activations as a pulse-density code ("pulse_density"): a frame
     *  carries at most one event; the fraction of 2 ms frames with an event
     *  equals the activation. */
    PulseDensity,
};

/** Contract brain frame (aer.rs announces dt_ms = 2). */
constexpr double FrameMs = 2.0;
/** Motor rate time constant (motor.rs RATE_TAU_S). */
constexpr double RateTauMs = 20.0;
/** Muscle activation time constant (celegans.rs MUSCLE_TAU_MS). */
constexpr double MuscleTauMs = 55.0;
/** Fraction of the remaining contraction recruited by one spike. */
constexpr double Twitch = 0.3;
/** A graded brain silent for longer than this disarms (50 reference frames). */
constexpr double HoldMs = 50.0 * FrameMs;

inline double RateKeep() { return std::exp(-FrameMs / RateTauMs); }
inline double MuscleDecay() { return std::exp(-FrameMs / MuscleTauMs); }

/** A frame carries at most one event: clamp a count to [0, 1]. */
inline double Event(float Count)
{
    return !(Count > 0.0f) ? 0.0 : (std::min)(1.0, static_cast<double>(Count));
}

namespace Detail
{
enum class EValueKind : uint8_t { String, Null, Other };

/** Minimal scanner for one key of a top-level JSON object. */
class FScanner
{
public:
    explicit FScanner(const std::string& InText) : S(InText) {}

    /** Finds the last top-level occurrence of Key (serde_json keeps the last
     *  duplicate). False when absent or the document is not a well-formed
     *  object prefix. */
    bool FindTopLevel(const std::string& Key, EValueKind& OutKind, std::string& OutText)
    {
        bool bFound = false;
        SkipSpace();
        if (!Eat('{')) return false;
        SkipSpace();
        if (Eat('}')) return false;
        for (;;)
        {
            SkipSpace();
            std::string Name;
            if (!ReadString(Name)) return bFound;
            SkipSpace();
            if (!Eat(':')) return bFound;
            SkipSpace();
            const std::size_t Start = P;
            if (Peek() == '"')
            {
                std::string Value;
                if (!ReadString(Value)) return bFound;
                if (Name == Key) { bFound = true; OutKind = EValueKind::String; OutText = Value; }
            }
            else
            {
                if (!SkipValue()) return bFound;
                if (Name == Key)
                {
                    bFound = true;
                    std::size_t End = P;
                    while (End > Start && IsSpace(S[End - 1])) --End;
                    OutText = S.substr(Start, End - Start);
                    OutKind = OutText == "null" ? EValueKind::Null : EValueKind::Other;
                }
            }
            SkipSpace();
            if (Eat(',')) continue;
            return bFound;
        }
    }

private:
    static bool IsSpace(char C) { return C == ' ' || C == '\t' || C == '\n' || C == '\r'; }
    char Peek() const { return P < S.size() ? S[P] : '\0'; }
    bool Eat(char C)
    {
        if (Peek() != C) return false;
        ++P;
        return true;
    }
    void SkipSpace()
    {
        while (P < S.size() && IsSpace(S[P])) ++P;
    }
    static int HexValue(char C)
    {
        if (C >= '0' && C <= '9') return C - '0';
        if (C >= 'a' && C <= 'f') return C - 'a' + 10;
        if (C >= 'A' && C <= 'F') return C - 'A' + 10;
        return -1;
    }
    static void AppendUtf8(std::string& Out, unsigned Code)
    {
        if (Code < 0x80) { Out += static_cast<char>(Code); return; }
        if (Code < 0x800)
        {
            Out += static_cast<char>(0xC0 | (Code >> 6));
            Out += static_cast<char>(0x80 | (Code & 0x3F));
            return;
        }
        Out += static_cast<char>(0xE0 | (Code >> 12));
        Out += static_cast<char>(0x80 | ((Code >> 6) & 0x3F));
        Out += static_cast<char>(0x80 | (Code & 0x3F));
    }
    bool ReadString(std::string& Out)
    {
        if (!Eat('"')) return false;
        while (P < S.size())
        {
            const char C = S[P++];
            if (C == '"') return true;
            if (C != '\\') { Out += C; continue; }
            if (P >= S.size()) return false;
            const char E = S[P++];
            switch (E)
            {
            case '"': Out += '"'; break;
            case '\\': Out += '\\'; break;
            case '/': Out += '/'; break;
            case 'b': Out += '\b'; break;
            case 'f': Out += '\f'; break;
            case 'n': Out += '\n'; break;
            case 'r': Out += '\r'; break;
            case 't': Out += '\t'; break;
            case 'u':
            {
                if (P + 4 > S.size()) return false;
                unsigned Code = 0;
                for (std::size_t i = 0; i < 4; ++i)
                {
                    const int Digit = HexValue(S[P + i]);
                    if (Digit < 0) return false;
                    Code = Code * 16u + static_cast<unsigned>(Digit);
                }
                P += 4;
                AppendUtf8(Out, Code);
                break;
            }
            default: return false;
            }
        }
        return false;
    }
    /** Skips a non-string value: object, array or literal. */
    bool SkipValue()
    {
        const char C = Peek();
        if (C == '{' || C == '[')
        {
            int Depth = 0;
            while (P < S.size())
            {
                const char D = S[P];
                if (D == '"')
                {
                    std::string Ignored;
                    if (!ReadString(Ignored)) return false;
                    continue;
                }
                ++P;
                if (D == '{' || D == '[') ++Depth;
                else if ((D == '}' || D == ']') && --Depth == 0) return true;
            }
            return false;
        }
        const std::size_t Start = P;
        while (P < S.size() && S[P] != ',' && S[P] != '}' && S[P] != ']') ++P;
        return P > Start;
    }

    const std::string& S;
    std::size_t P = 0;
};
} // namespace Detail

/**
 * OutputCode::from_handshake: an absent or null `output_code` means spikes;
 * "spikes" and "pulse_density" are accepted; any other value is refused
 * (returns false). A reply that is not a JSON object keeps the legacy
 * behaviour (spikes).
 */
inline bool ParseHandshake(const std::string& Json, EOutputCode& OutCode, std::string* OutError = nullptr)
{
    OutCode = EOutputCode::Spikes;
    Detail::EValueKind Kind = Detail::EValueKind::Other;
    std::string Text;
    Detail::FScanner Scanner(Json);
    if (!Scanner.FindTopLevel("output_code", Kind, Text) || Kind == Detail::EValueKind::Null)
        return true;
    if (Kind == Detail::EValueKind::String && Text == "spikes")
        return true;
    if (Kind == Detail::EValueKind::String && Text == "pulse_density")
    {
        OutCode = EOutputCode::PulseDensity;
        return true;
    }
    if (OutError)
        *OutError = "unsupported brain output_code " +
                    (Kind == Detail::EValueKind::String ? "\"" + Text + "\"" : Text);
    return false;
}

/** One weighted output channel of a joint readout. */
struct FReadout
{
    std::size_t Channel = 0;
    double Coefficient = 1.0;
};

/**
 * Graded (pulse-density) decode of weighted joint readouts, as Actuation does
 * for a brain that negotiated output_code = pulse_density. Call Accept with
 * each brain frame and Advance once per 2 ms frame. Optional per-joint
 * behaviour mirrors the actuator record (aer.rs decode_tau_s, wake): see
 * SetDecodeTau and SetWake. Unconfigured joints decode exactly as before.
 */
class FGradedJoints
{
public:
    FGradedJoints() = default;
    explicit FGradedJoints(std::vector<std::vector<FReadout>> InReadouts)
        : Readouts(std::move(InReadouts)), SingleSign(Readouts.size(), false),
          Rates(Readouts.size(), 0.0), Pending(Readouts.size(), 0.0),
          Keep(Readouts.size(), RateKeep()), Wakes(Readouts.size())
    {
        for (std::size_t i = 0; i < Readouts.size(); ++i)
        {
            bool bSingle = !Readouts[i].empty();
            for (const FReadout& C : Readouts[i])
                bSingle = bSingle && C.Coefficient >= 0.0;
            SingleSign[i] = bSingle;
        }
    }

    /** Joint i reads output channel i with coefficient 1. */
    static FGradedJoints Identity(std::size_t Joints)
    {
        std::vector<std::vector<FReadout>> R(Joints);
        for (std::size_t i = 0; i < Joints; ++i)
            R[i].push_back(FReadout{i, 1.0});
        return FGradedJoints(std::move(R));
    }

    std::size_t Count() const { return Rates.size(); }

    /** Graded decode time constant of Joint in seconds (actuator decode_tau_s):
     *  keep = exp(-0.002 / tau) per frame; default motor.rs RATE_TAU_S (0.02 s).
     *  A non-positive or non-finite tau is ignored. */
    void SetDecodeTau(std::size_t Joint, double TauS)
    {
        if (Joint < Keep.size() && TauS > 0.0 && std::isfinite(TauS))
            Keep[Joint] = std::exp(-FrameS / TauS);
    }

    /** Servo wake-up of a graded position readout over [Lo, Hi] (actuator
     *  wake {from, seconds}): on the first frame after arming the decoded rate
     *  starts at the rest-pose density (From - Lo)/(Hi - Lo); Target then
     *  blends linearly from From to the decoded target over Seconds of
     *  advanced frames. Non-finite values are ignored. */
    void SetWake(std::size_t Joint, double From, double Seconds, double Lo, double Hi)
    {
        if (Joint >= Wakes.size() || !std::isfinite(From) || !std::isfinite(Seconds))
            return;
        FWake& W = Wakes[Joint];
        W.bEnabled = true;
        W.From = From;
        W.Seconds = Seconds;
        W.bHasRate = Hi > Lo;
        W.Rate = Hi > Lo ? (std::max)(0.0, (std::min)(1.0, (From - Lo) / (Hi - Lo))) : 0.0;
    }

    /** Frames advanced since arming (the last Reset). */
    uint64_t ArmedFrames() const { return Armed; }

    /** Latest frame: pending = sum(coefficient * min(events, 1)). */
    void Accept(const float* Events, std::size_t NumEvents)
    {
        for (std::size_t i = 0; i < Readouts.size(); ++i)
        {
            double Drive = 0.0;
            for (const FReadout& C : Readouts[i])
                if (Events && C.Channel < NumEvents && std::isfinite(C.Coefficient))
                    Drive += C.Coefficient * Event(Events[C.Channel]);
            Pending[i] = Drive;
        }
    }

    /** One 2 ms frame: rate = clamp(rate*keep + (1-keep)*pending, -1, 1);
     *  pending = 0. The first frame after arming seeds wake joints at their
     *  rest-pose density before filtering. */
    void Advance()
    {
        if (Armed == 0)
            for (std::size_t i = 0; i < Rates.size(); ++i)
                if (Wakes[i].bHasRate)
                    Rates[i] = Wakes[i].Rate;
        for (std::size_t i = 0; i < Rates.size(); ++i)
        {
            const double K = Keep[i];
            Rates[i] = (std::max)(-1.0, (std::min)(1.0, Rates[i] * K + (1.0 - K) * Pending[i]));
            Pending[i] = 0.0;
        }
        ++Armed;
    }

    /** Disarm: forget the decoded drive and restart any wake-up. */
    void Reset()
    {
        std::fill(Rates.begin(), Rates.end(), 0.0);
        std::fill(Pending.begin(), Pending.end(), 0.0);
        Armed = 0;
    }

    double Rate(std::size_t Joint) const { return Rates[Joint]; }

    /** Position of a graded position-readout joint within its range, 0..1: a
     *  single-sign readout spans the full range, a mixed-sign one is centred. */
    double Normalized(std::size_t Joint) const
    {
        const double R = Rates[Joint];
        const double N = SingleSign[Joint] ? R : 0.5 + 0.5 * R;
        return (std::max)(0.0, (std::min)(1.0, N));
    }

    /** Graded position target in [Lo, Hi] (actuation.rs controls). A wake
     *  joint with a single-sign readout blends from its wake pose to the
     *  decoded target over the wake time (t = advanced frames x 2 ms). */
    double Target(std::size_t Joint, double Lo, double Hi) const
    {
        double Value = Lo + Normalized(Joint) * (Hi - Lo);
        const FWake& W = Wakes[Joint];
        if (W.bEnabled && SingleSign[Joint] && W.Seconds > 0.0)
        {
            const double T = static_cast<double>(Armed) * 2000.0 * 1e-6;
            Value = W.From + (std::max)(0.0, (std::min)(1.0, T / W.Seconds)) * (Value - W.From);
        }
        return (std::max)(Lo, (std::min)(Hi, Value));
    }

private:
    /** Reference frame in seconds (actuation.rs: exp(-0.002 / tau)). */
    static constexpr double FrameS = 0.002;
    struct FWake
    {
        bool bEnabled = false;
        bool bHasRate = false;
        double Rate = 0.0;
        double From = 0.0;
        double Seconds = 0.0;
    };
    std::vector<std::vector<FReadout>> Readouts;
    std::vector<bool> SingleSign;
    std::vector<double> Rates;
    std::vector<double> Pending;
    std::vector<double> Keep;
    std::vector<FWake> Wakes;
    /** Advanced frames since the last Reset (actuation.rs armed_frames). */
    uint64_t Armed = 0;
};

/**
 * Per-muscle activation filter of celegans.rs: decay = exp(-2/55) per 2 ms
 * frame; spikes sum saturating twitches, a pulse-density code is decoded
 * linearly so a steady density p settles at activation p.
 */
class FMuscleActivations
{
public:
    explicit FMuscleActivations(std::size_t Muscles = 0) : A(Muscles, 0.0) {}

    std::size_t Count() const { return A.size(); }
    double Activation(std::size_t Muscle) const { return Muscle < A.size() ? A[Muscle] : 0.0; }

    /** Advance every muscle by one 2 ms brain frame. */
    void Advance(const float* Events, std::size_t NumEvents, EOutputCode Code)
    {
        const double Decay = MuscleDecay();
        for (std::size_t i = 0; i < A.size(); ++i)
        {
            const double Kept = A[i] * Decay;
            const double E = (Events && i < NumEvents) ? Event(Events[i]) : 0.0;
            A[i] = Code == EOutputCode::PulseDensity ? Kept + (1.0 - Decay) * E
                                                     : Kept + Twitch * E * (1.0 - Kept);
        }
    }

    void Reset() { std::fill(A.begin(), A.end(), 0.0); }

private:
    std::vector<double> A;
};
} // namespace NmOutputDecode
