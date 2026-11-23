// SPDX-FileCopyrightText: masterkiga
// SPDX-License-Identifier: MIT
//
// Licensed under the MIT License. See LICENSE in the project root.
// Offline vectors for the Unreal legacy-body output decoder. Expected values
// are the MuJoCo reference (crates/aarnn-embodiment Actuation / Muscles) on
// the same inputs; tolerances repeat the Rust tests'.
#include <NmOutputDecode.h>

#include <algorithm>
#include <cmath>
#include <cstdio>
#include <cstdlib>
#include <string>
#include <vector>

namespace
{
int Failures = 0;

void Require(bool bOk, const std::string& What)
{
    if (!bOk)
    {
        ++Failures;
        std::fprintf(stderr, "FAIL %s\n", What.c_str());
    }
}

void Near(double Got, double Want, double Tol, const std::string& What)
{
    char Buffer[96];
    std::snprintf(Buffer, sizeof(Buffer), ": %.17g vs %.17g", Got, Want);
    Require(std::fabs(Got - Want) <= Tol, What + Buffer);
}

struct FSample
{
    int Step;
    double Value;
};

// graded_position_readout_spans_the_full_joint_range (actuation.rs)
void GradedPositionReadout()
{
    const double Lo = -1.4, Hi = -0.2; // hexapod.json femur control_range
    const std::size_t Joint = 1, Channel = 1;
    struct FCase
    {
        double Density;
        std::vector<FSample> Reference;
    };
    const FCase Cases[] = {
        {0.0, {{1, -1.39999999999999991}, {400, -1.39999999999999991}}},
        {0.5, {{1, -1.39999999999999991}, {2, -1.28580490164315142}, {7, -1.14281136413280393},
               {50, -0.774269713354299283}, {399, -0.829975024974728415}, {400, -0.770024975025272451}}},
        {1.0, {{1, -1.28580490164315142}, {2, -1.18247690369357805}, {7, -0.795902364549691099},
               {50, -0.208085536398902482}, {400, -0.200000000000000622}}},
        {0.3, {{7, -1.20120709206785703}, {50, -0.989156262868102099}, {399, -1.06907241572953127},
               {400, -0.986369240734982999}}},
    };
    for (const FCase& Case : Cases)
    {
        NmOutputDecode::FGradedJoints Joints = NmOutputDecode::FGradedJoints::Identity(18);
        double Phase = 0.0;
        for (int Step = 1; Step <= 400; ++Step)
        {
            Phase += Case.Density;
            std::vector<float> Spikes(18, 0.0f);
            if (Phase >= 1.0 - 1e-12)
            {
                Phase -= 1.0;
                Spikes[Channel] = 1.0f;
            }
            Joints.Accept(Spikes.data(), Spikes.size());
            Joints.Advance();
            for (const FSample& S : Case.Reference)
                if (S.Step == Step)
                    Near(Joints.Target(Joint, Lo, Hi), S.Value, 1e-12,
                         "density " + std::to_string(Case.Density) + " step " + std::to_string(Step) + " matches Rust");
        }
        const double Control = Joints.Target(Joint, Lo, Hi);
        if (Case.Density != 0.3)
            Near(Control, Lo + Case.Density * (Hi - Lo), 0.08 * (Hi - Lo),
                 "density " + std::to_string(Case.Density) + " spans the joint range");
        if (Case.Density == 0.0)
            Near(Control, Lo, 1e-12, "graded joints stay stiff: silence holds the lower limit");
        Require(Joints.Target(0, -0.7, 0.7) == -0.7, "unexercised joint rests at lo");
    }
}

// pulse_density_code_decodes_linearly (celegans.rs)
void PulseDensityDecodesLinearly()
{
    const std::size_t Mdr05 = 28; // celegans_o_028_MDR05
    struct FCase
    {
        double P, Floor, MeanPd, MeanSpk, LastPd, LastSpk;
    };
    const FCase Cases[] = {
        {0.2, 0.6, 0.200000000000000094, 0.670853663238015319, 0.214803295322949700, 0.720507887714990636},
        {0.5, 0.8, 0.499999999999999889, 0.844004043797125125, 0.509089907470285929, 0.859347881122468404},
    };
    for (const FCase& C : Cases)
    {
        NmOutputDecode::FMuscleActivations Graded(96), Spiking(96);
        double Phase = 0.0, SumG = 0.0, SumS = 0.0, N = 0.0;
        for (int Frame = 0; Frame < 5000; ++Frame)
        {
            Phase += C.P;
            std::vector<float> Spikes(96, 0.0f);
            if (Phase >= 1.0)
            {
                Phase -= 1.0;
                Spikes[Mdr05] = 1.0f;
            }
            Graded.Advance(Spikes.data(), Spikes.size(), NmOutputDecode::EOutputCode::PulseDensity);
            Spiking.Advance(Spikes.data(), Spikes.size(), NmOutputDecode::EOutputCode::Spikes);
            if (Frame >= 2500)
            {
                SumG += Graded.Activation(Mdr05);
                SumS += Spiking.Activation(Mdr05);
                N += 1.0;
            }
        }
        const std::string P = "p " + std::to_string(C.P);
        Near(SumG / N, C.P, 0.01, P + " linear decode");
        Require(SumS / N > C.Floor, P + " twitch compresses");
        Near(SumG / N, C.MeanPd, 1e-12, P + " pulse-density mean matches Rust");
        Near(SumS / N, C.MeanSpk, 1e-12, P + " twitch mean matches Rust");
        Near(Graded.Activation(Mdr05), C.LastPd, 1e-12, P + " final pulse-density state matches Rust");
        Near(Spiking.Activation(Mdr05), C.LastSpk, 1e-12, P + " final twitch state matches Rust");
        Require(Graded.Activation(0) == 0.0 && Spiking.Activation(95) == 0.0, "silent muscles stay at rest");
    }
}

void Handshake()
{
    using NmOutputDecode::EOutputCode;
    auto Expect = [](const std::string& Json, bool bOk, EOutputCode Want)
    {
        EOutputCode Code = EOutputCode::PulseDensity;
        std::string Error;
        const bool bGot = NmOutputDecode::ParseHandshake(Json, Code, &Error);
        Require(bGot == bOk && (!bOk || Code == Want), "handshake " + Json + " -> " + Error);
        Require(bOk || !Error.empty(), "refusal explains itself: " + Json);
    };
    Expect(R"({"expected_o": 96})", true, EOutputCode::Spikes);
    Expect(R"({"output_code": "pulse_density"})", true, EOutputCode::PulseDensity);
    Expect(R"({"output_code": "analog"})", false, EOutputCode::Spikes);
    Expect(R"({"output_code": null})", true, EOutputCode::Spikes);
    Expect(R"({"output_code":"spikes"})", true, EOutputCode::Spikes);
    Expect(R"({"output_code": 1})", false, EOutputCode::Spikes);
    Expect(R"({"output_code": ["pulse_density"]})", false, EOutputCode::Spikes);
    Expect(R"({"expected_s":34,"expected_o":18,"output_code":"pulse_density"})", true, EOutputCode::PulseDensity);
    Expect(R"({"brain_id":"output_code","meta":{"output_code":"analog"},"list":["output_code",{"x":"}"}],"output_code":"pulse_density"})",
           true, EOutputCode::PulseDensity);
    Expect(R"({"meta":{"output_code":"pulse_density"}})", true, EOutputCode::Spikes);
    Expect("", true, EOutputCode::Spikes);
    Expect("not json", true, EOutputCode::Spikes);
    Expect("{}", true, EOutputCode::Spikes);
}

void MixedSignAndReset()
{
    NmOutputDecode::FGradedJoints J({{NmOutputDecode::FReadout{0, 1.0}, NmOutputDecode::FReadout{1, -1.0}}});
    Near(J.Target(0, -1.2, 1.2), 0.0, 1e-15, "mixed-sign rest is the midpoint");
    const float Right[2] = {0.0f, 1.0f};
    for (int i = 0; i < 400; ++i)
    {
        J.Accept(Right, 2);
        J.Advance();
    }
    Near(J.Rate(0), -1.0, 1e-12, "antagonist saturates");
    Near(J.Target(0, -1.2, 1.2), -1.2, 1e-12, "mixed-sign spans to lo");
    const float Burst[2] = {5.0f, 0.0f};
    J.Accept(Burst, 2);
    J.Advance();
    const double Keep = NmOutputDecode::RateKeep();
    Near(J.Rate(0), -Keep + (1.0 - Keep), 1e-12, "event count clamps to 1");
    J.Reset();
    Require(J.Rate(0) == 0.0, "reset disarms");
    Near(NmOutputDecode::HoldMs, 100.0, 0.0, "hold matches 50 reference frames");
}
// NAO decode_tau_s + wake (actuation.rs wake_rate / armed_frames), printed by
// scripts/qa/output_decode_reference on generated/nao.json: A = LKneePitch as
// generated (wake from 0 over 1 s, tau 0.2 s; silent for 20 replies, then
// density 0.37); B = LHipPitch with wake {-0.45, 0.25 s}, tau 0.05 s, density
// 0.8; C = HeadYaw with a seed-only wake {0.5, 0 s}, default tau, silent.
// Replies 1..1500, a disarm (Reset), then replies 1501..1600 re-arm.
void WakeAndDecodeTau()
{
    using NmOutputDecode::FReadout;
    const std::size_t Channels[3] = {9, 6, 1};
    const double Lo[3] = {-0.092346, -1.535889, -2.0857};
    const double Hi[3] = {2.112528, 0.48409, 2.0857};
    NmOutputDecode::FGradedJoints J({{FReadout{Channels[0], 1.0}}, {FReadout{Channels[1], 1.0}}, {FReadout{Channels[2], 1.0}}});
    J.SetDecodeTau(0, 0.2);
    J.SetWake(0, 0.0, 1.0, Lo[0], Hi[0]);
    J.SetDecodeTau(1, 0.05);
    J.SetWake(1, -0.45, 0.25, Lo[1], Hi[1]);
    J.SetWake(2, 0.5, 0.0, Lo[2], Hi[2]);
    struct FRow
    {
        int Reply;
        double Values[3][2]; // (control, rate) for A, B, C
    };
    const FRow Reference[] = {
        {1, {{-1.8377161051986446e-6, 0.041465925920211616}, {-0.4503406265336666, 0.5164958067839701}, {0.253938111815581, 0.5608759917091578}}},
        {2, {{-7.314293259257477e-6, 0.04105333306356083}, {-0.4500685216679685, 0.5354542773721742}, {0.03129210823373807, 0.5075015841764726}}},
        {10, {{-0.00017575767588102505, 0.037897002824627955}, {-0.43518415950492084, 0.6292575349488729}, {-1.134474128963002, 0.22803516110586328}}},
        {21, {{-0.0007346550633087554, 0.03394942875573069}, {-0.40722178487498195, 0.6636312718533274}, {-1.769064413466265, 0.07590631119857483}}},
        {125, {{0.11317488826487161, 0.2472003175961467}, {0.10796583816054633, 0.8137979841179271}, {-2.085690363992893, 2.3100175257812116e-6}}},
        {126, {{0.11824221075770995, 0.2546907995896385}, {0.04350936800410432, 0.7818885087439544}, {-2.085691280980209, 2.090190293645687e-6}}},
        {500, {{0.7149185313802094, 0.36612728499687935}, {0.1117508846982797, 0.8156717890127965}, {-2.0857, 1.1955622769046709e-22}}},
        {501, {{0.7068861150845764, 0.3624842576421947}, {0.047146000744258565, 0.7836888406979768}, {-2.0857, 1.0817894837356152e-22}}},
        {1500, {{0.7210995014506245, 0.36893060621633006}, {0.11175088585613419, 0.8156717895859977}, {-2.0857, 4.4475825041495715e-66}}},
        {1501, {{-1.8377161051986446e-6, 0.041465925920211616}, {-0.4503406265336666, 0.5164958067839701}, {0.253938111815581, 0.5608759917091578}}},
        {1502, {{8.044115818928937e-5, 0.05100349931439272}, {-0.4500685216679685, 0.5354542773721742}, {0.03129210823373807, 0.5075015841764726}}},
        {1540, {{0.019366678750649135, 0.15167736767865833}, {-0.3065326918331624, 0.7595248950713683}, {-2.0383412525254, 0.011353202156254462}}},
        {1600, {{0.09116439944378199, 0.248616472968029}, {-0.008830352411706655, 0.8105782582320741}, {-2.085582609401613, 2.8141774556948843e-5}}},
    };
    double Phase[3] = {0.0, 0.0, 0.0};
    std::vector<double> Tail;
    for (int Reply = 1; Reply <= 1600; ++Reply)
    {
        if (Reply == 1501)
            J.Reset();
        float Events[40] = {};
        for (int k = 0; k < 3; ++k)
        {
            Phase[k] += k == 0 ? (Reply > 20 ? 0.37 : 0.0) : k == 1 ? 0.8 : 0.0;
            if (Phase[k] >= 1.0 - 1e-12)
            {
                Phase[k] -= 1.0;
                Events[Channels[k]] = 1.0f;
            }
        }
        J.Accept(Events, 40);
        J.Advance();
        for (const FRow& Row : Reference)
        {
            if (Row.Reply != Reply)
                continue;
            for (std::size_t k = 0; k < 3; ++k)
            {
                const std::string Tag = "wake joint " + std::to_string(k) + " reply " + std::to_string(Reply);
                Near(J.Target(k, Lo[k], Hi[k]), Row.Values[k][0], 1e-12, Tag + " control matches Rust");
                Near(J.Rate(k), Row.Values[k][1], 1e-12, Tag + " rate matches Rust");
            }
        }
        if (Reply <= 20 || (Reply > 1500 && Reply <= 1520))
            Near(J.Target(0, Lo[0], Hi[0]), 0.0, 0.05, "reply " + std::to_string(Reply) + " holds the rest pose");
        if (Reply > 1000 && Reply <= 1500)
            Tail.push_back(J.Target(0, Lo[0], Hi[0]));
    }
    double Sum = 0.0, Worst = 0.0;
    for (double C : Tail)
        Sum += C;
    const double Mean = Sum / static_cast<double>(Tail.size());
    for (double C : Tail)
        Worst = (std::max)(Worst, std::fabs(C - Mean));
    Near(Mean, Lo[0] + 0.37 * (Hi[0] - Lo[0]), 0.03 * (Hi[0] - Lo[0]), "slow decode reaches the density target");
    Require(Worst < 0.02 * (Hi[0] - Lo[0]), "slow decode ripple " + std::to_string(Worst));
    Require(J.ArmedFrames() == 100, "re-armed frame count");
}
} // namespace

int main()
{
    GradedPositionReadout();
    PulseDensityDecodesLinearly();
    Handshake();
    MixedSignAndReset();
    WakeAndDecodeTau();
    if (Failures)
    {
        std::fprintf(stderr, "%d failure(s)\n", Failures);
        return 1;
    }
    std::printf("PASS NmOutputDecode (C++)\n");
    return 0;
}
