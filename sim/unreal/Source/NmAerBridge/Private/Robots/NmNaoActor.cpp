// SPDX-FileCopyrightText: NEURALMIMICRY LTD
// SPDX-License-Identifier: MIT
//
// Licensed under the MIT License. See LICENSE in the project root.

#include "Robots/NmNaoActor.h"
#include "Robots/NmNaoOem.generated.h"
#include "NmNaoChatComponent.h"

#include "Components/StaticMeshComponent.h"
#include "Components/SceneCaptureComponent2D.h"
#include "Engine/TextureRenderTarget2D.h"
#include "PhysicsEngine/PhysicsConstraintComponent.h"
#include "Engine/World.h"
#include "Engine/OverlapResult.h"
#include "UObject/ConstructorHelpers.h"

// ============================================================================
// OEM NAO tables (NmNaoOem.generated.h, scripts/generate_nao_engine_tables.py)
// ============================================================================

namespace
{
// ENaoJoint order.
const TCHAR* const NaoJointNames[UNmNaoComponent::NumJoints] = {
    TEXT("HeadYaw"), TEXT("HeadPitch"),
    TEXT("LShoulderPitch"), TEXT("LShoulderRoll"), TEXT("LElbowYaw"), TEXT("LElbowRoll"), TEXT("LWristYaw"),
    TEXT("RShoulderPitch"), TEXT("RShoulderRoll"), TEXT("RElbowYaw"), TEXT("RElbowRoll"), TEXT("RWristYaw"),
    TEXT("LHipYawPitch"), TEXT("LHipRoll"), TEXT("LHipPitch"), TEXT("LKneePitch"), TEXT("LAnklePitch"), TEXT("LAnkleRoll"),
    TEXT("RHipYawPitch"), TEXT("RHipRoll"), TEXT("RHipPitch"), TEXT("RKneePitch"), TEXT("RAnklePitch"), TEXT("RAnkleRoll"),
};

/** Index of a record link (joint) by name; -1 when absent. */
int32 OemLink(const TCHAR* Name)
{
    for (int32 i = 0; i < NmNaoOem::NumLinks; ++i)
    {
        if (FCString::Strcmp(NmNaoOem::Links[i].Name, Name) == 0)
        {
            return i;
        }
    }
    return -1;
}

/** Record link of ENaoJoint j. */
int32 JointLink(int32 j)
{
    static const TArray<int32> Map = []
    {
        TArray<int32> M;
        for (const TCHAR* Name : NaoJointNames)
        {
            M.Add(OemLink(Name));
        }
        return M;
    }();
    return Map.IsValidIndex(j) ? Map[j] : -1;
}

/** Rest pose of a joint (its wake pose, else the zero pose), rad. */
double RestRad(const NmNaoOem::FLink& K)
{
    return FMath::Clamp(K.bWake ? K.WakeFrom : 0.0, K.Lo, K.Hi);
}

/** MuJoCo frame (x fwd, y left, z up; m) to Unreal (x fwd, y right, z up; cm). */
FVector Oem(const double P[3])
{
    return FVector(P[0] * 100.0, -P[1] * 100.0, P[2] * 100.0);
}

/** Link origin of the OEM zero pose relative to the torso frame, cm. */
FVector LinkOrigin(int32 L)
{
    FVector P = FVector::ZeroVector;
    for (int32 i = L; i >= 0; i = NmNaoOem::Links[i].Parent)
    {
        P += Oem(NmNaoOem::Links[i].PositionM);
    }
    return P;
}

FVector JointOrigin(const TCHAR* Name)
{
    const int32 L = OemLink(Name);
    return L >= 0 ? LinkOrigin(L) : FVector::ZeroVector;
}

/** Summed OEM mass of named links; a hand includes its phalanges. */
double OemMass(std::initializer_list<const TCHAR*> Names)
{
    double Kg = 0.0;
    for (const TCHAR* Name : Names)
    {
        const int32 L = OemLink(Name);
        if (L < 0)
        {
            continue;
        }
        Kg += NmNaoOem::Links[L].MassKg;
        for (int32 c = 0; c < NmNaoOem::NumLinks; ++c)
        {
            if (NmNaoOem::Links[c].Parent == L && FCString::Strstr(NmNaoOem::Links[c].Name, TEXT("Phalanx")))
            {
                Kg += NmNaoOem::Links[c].MassKg;
            }
        }
    }
    return Kg;
}

/** The record's readouts, decode_tau_s and wake: the reference Actuation decode. */
NmOutputDecode::FGradedJoints MakeOemDecoder()
{
    std::vector<std::vector<NmOutputDecode::FReadout>> Readouts(NmNaoOem::NumLinks);
    for (int32 i = 0; i < NmNaoOem::NumLinks; ++i)
    {
        Readouts[i].push_back(NmOutputDecode::FReadout{static_cast<std::size_t>(NmNaoOem::Links[i].Channel), 1.0});
    }
    NmOutputDecode::FGradedJoints Decoder(std::move(Readouts));
    for (int32 i = 0; i < NmNaoOem::NumLinks; ++i)
    {
        const NmNaoOem::FLink& K = NmNaoOem::Links[i];
        const std::size_t j = static_cast<std::size_t>(i);
        if (K.DecodeTauS > 0.0)
        {
            Decoder.SetDecodeTau(j, K.DecodeTauS);
        }
        if (K.bWake)
        {
            Decoder.SetWake(j, K.WakeFrom, K.WakeSeconds, K.Lo, K.Hi);
        }
    }
    return Decoder;
}
} // namespace

// ============================================================================
// UNmNaoComponent
// ============================================================================

UNmNaoComponent::UNmNaoComponent()
{
    BrainId = TEXT("nao");
    PrimaryComponentTick.bCanEverTick = true;
    GradedJoints = MakeOemDecoder();
}

// ----------------------------------------------------------------------------
// SampleFootPressure — sphere overlap at foot corner point
// ----------------------------------------------------------------------------

float UNmNaoComponent::SampleFootPressure(int32 FootIndex, int32 PointIndex) const
{
    // Each foot has 4 pressure points at corners.
    // We use a small sphere overlap to detect ground contact.
    if (!FeetMeshes.IsValidIndex(FootIndex) || !FeetMeshes[FootIndex])
    {
        return 0.f;
    }

    UWorld* World = GetWorld();
    if (!World)
    {
        return 0.f;
    }

    const FVector FootLoc = FeetMeshes[FootIndex]->GetComponentLocation();
    // Corners just below the OEM sole box (16 x 8.8 x 2.4 cm, centred on
    // the foot mesh): toe left/right, heel left/right.
    const FVector Offsets[4] = {
        FVector( 7.2f, -4.0f, -1.8f),
        FVector( 7.2f,  4.0f, -1.8f),
        FVector(-7.2f, -4.0f, -1.8f),
        FVector(-7.2f,  4.0f, -1.8f),
    };

    const FVector TestLoc = FootLoc + Offsets[PointIndex % 4];
    TArray<FOverlapResult> Overlaps;
    const FCollisionShape Sphere = FCollisionShape::MakeSphere(0.5f); // 0.5 cm radius
    const bool bHit = World->OverlapMultiByChannel(Overlaps, TestLoc, FQuat::Identity,
                          ECC_Visibility, Sphere);
    return bHit ? 1.f : 0.f;
}

// ----------------------------------------------------------------------------
// CollectSensors — 250 channels
// [0..1]   2 sonar
// [2..4]   3 accel
// [5..6]   2 gyro
// [7..9]   3 GPS (world position, normalized)
// [10..12] 3 inertial RPY
// [13..20] 8 foot pressure (4L + 4R)
// [21..22] 2 bumpers
// [23..46] 24 joint position channels
// [47..57] 11 joint velocity channels
// [58..249] 192 eye event channels (L-on/L-off/R-on/R-off)
// ----------------------------------------------------------------------------

void UNmNaoComponent::CollectSensors(TArray<float>& OutSensors)
{
    OutSensors.SetNumZeroed(NumSensors);

    AActor* Owner = GetOwner();
    UWorld* World = GetWorld();
    if (!Owner || !World)
    {
        return;
    }

    const ANmNaoActor* NaoActor = Cast<ANmNaoActor>(Owner);
    const float MaxSonar = NaoActor ? NaoActor->MaxSonarRangeCm : 255.f;

    // --- Sonar [0..1]: sphere trace forward-left/right ---
    if (TorsoMesh)
    {
        const FVector TorsoLoc = TorsoMesh->GetComponentLocation();
        const FVector Fwd = TorsoMesh->GetForwardVector();
        const FVector Right = TorsoMesh->GetRightVector();

        auto SphereSonar = [&](const FVector& Dir) -> float
        {
            FHitResult Hit;
            const bool bHit = World->SweepSingleByChannel(
                Hit, TorsoLoc, TorsoLoc + Dir * MaxSonar,
                FQuat::Identity, ECC_Visibility, FCollisionShape::MakeSphere(3.f));
            return bHit ? (1.f - (Hit.Distance / MaxSonar)) : 0.f;
        };

        OutSensors[0] = SphereSonar(Fwd + Right * 0.3f);   // front-left
        OutSensors[1] = SphereSonar(Fwd - Right * 0.3f);   // front-right
    }

    // --- Accelerometer [2..4] ---
    if (TorsoMesh)
    {
        const FVector LinVel = TorsoMesh->GetPhysicsLinearVelocity();
        constexpr float MaxAccel = 500.f;
        OutSensors[2] = FMath::Clamp((LinVel.X - PrevLinearVel.X) / MaxAccel, -1.f, 1.f) * 0.5f + 0.5f;
        OutSensors[3] = FMath::Clamp((LinVel.Y - PrevLinearVel.Y) / MaxAccel, -1.f, 1.f) * 0.5f + 0.5f;
        OutSensors[4] = FMath::Clamp((LinVel.Z - PrevLinearVel.Z) / MaxAccel, -1.f, 1.f) * 0.5f + 0.5f;
        PrevLinearVel = LinVel;
    }

    // --- Gyro [5..6] (XY only per Webots spec) ---
    if (TorsoMesh)
    {
        const FVector AngVel = TorsoMesh->GetPhysicsAngularVelocityInDegrees();
        constexpr float MaxAngVel = 360.f;
        OutSensors[5] = FMath::Clamp(AngVel.X / MaxAngVel, -1.f, 1.f) * 0.5f + 0.5f;
        OutSensors[6] = FMath::Clamp(AngVel.Y / MaxAngVel, -1.f, 1.f) * 0.5f + 0.5f;
        PrevAngularVel = AngVel;
    }

    // --- GPS [7..9]: world position normalized over 1000 cm range ---
    {
        const FVector WorldPos = Owner->GetActorLocation();
        constexpr float PosRange = 1000.f;
        OutSensors[7] = FMath::Clamp(WorldPos.X / PosRange, -1.f, 1.f) * 0.5f + 0.5f;
        OutSensors[8] = FMath::Clamp(WorldPos.Y / PosRange, -1.f, 1.f) * 0.5f + 0.5f;
        OutSensors[9] = FMath::Clamp(WorldPos.Z / PosRange, -1.f, 1.f) * 0.5f + 0.5f;
    }

    // --- Inertial RPY [10..12]: actor rotation Euler normalized ÷ 180° ---
    {
        const FRotator Rot = Owner->GetActorRotation();
        OutSensors[10] = FMath::Clamp(Rot.Roll  / 180.f, -1.f, 1.f) * 0.5f + 0.5f;
        OutSensors[11] = FMath::Clamp(Rot.Pitch / 180.f, -1.f, 1.f) * 0.5f + 0.5f;
        OutSensors[12] = FMath::Clamp(Rot.Yaw   / 180.f, -1.f, 1.f) * 0.5f + 0.5f;
    }

    // --- Foot pressure [13..20]: 4 points per foot ---
    for (int32 Foot = 0; Foot < 2; ++Foot)
    {
        for (int32 Pt = 0; Pt < 4; ++Pt)
        {
            OutSensors[13 + Foot * 4 + Pt] = SampleFootPressure(Foot, Pt);
        }
    }

    // --- Bumpers [21..22] (foot hit proxies) ---
    OutSensors[21] = FootContacts[0] ? 1.f : 0.f;
    OutSensors[22] = FootContacts[1] ? 1.f : 0.f;
    FootContacts[0] = false;
    FootContacts[1] = false;

    // --- Joint position channels [23..46] ---
    if (PrevJointAnglesDeg.Num() != NumJoints)
    {
        PrevJointAnglesDeg.Init(0.f, NumJoints);
    }
    TArray<float> CurrentJointAngles;
    CurrentJointAngles.Init(0.f, NumJoints);
    constexpr float MaxJointDeg = 180.f;
    for (int32 j = 0; j < NumJoints; ++j)
    {
        float AngleDeg = 0.f;
        if (JointConstraints.IsValidIndex(j) && JointConstraints[j])
        {
            UPhysicsConstraintComponent* Joint = JointConstraints[j];
            AngleDeg = FMath::Max3(FMath::Abs(Joint->GetCurrentTwist()),
                                   FMath::Abs(Joint->GetCurrentSwing1()),
                                   FMath::Abs(Joint->GetCurrentSwing2()));
        }
        CurrentJointAngles[j] = AngleDeg;
        OutSensors[23 + j] = FMath::Clamp(AngleDeg / MaxJointDeg, 0.f, 1.f);
    }

    // --- Joint velocity channels [47..57] ---
    const float Dt = FMath::Max(World->GetDeltaSeconds(), 1.0e-3f);
    constexpr float MaxJointVelDegS = 240.f;
    for (int32 j = 0; j < NumJointVelSensors; ++j)
    {
        const float Vel = FMath::Abs(CurrentJointAngles[j] - PrevJointAnglesDeg[j]) / Dt;
        OutSensors[47 + j] = FMath::Clamp(Vel / MaxJointVelDegS, 0.f, 1.f);
    }
    PrevJointAnglesDeg = MoveTemp(CurrentJointAngles);

    // --- Eye channels [58..249]: 192 channels from 8x6 render grid ---
    if (EyeCamera && EyeCamera->TextureTarget)
    {
        FRenderTarget* RT = EyeCamera->TextureTarget->GameThread_GetRenderTargetResource();
        if (RT)
        {
            TArray<FColor> Pixels;
            if (RT->ReadPixels(Pixels) && Pixels.Num() > 0)
            {
                const int32 RTW    = EyeCamera->TextureTarget->SizeX;
                const int32 RTH    = EyeCamera->TextureTarget->SizeY;
                const int32 GridW  = 8;
                const int32 GridH  = 6;
                const int32 CellW  = FMath::Max(1, RTW / GridW);
                const int32 CellH  = FMath::Max(1, RTH / GridH);
                const int32 PixCount = GridW * GridH;
                TArray<float> Brightness;
                Brightness.Init(0.f, PixCount);

                int32 PixIdx = 0;
                for (int32 cy = 0; cy < GridH && PixIdx < PixCount; ++cy)
                {
                    for (int32 cx = 0; cx < GridW && PixIdx < PixCount; ++cx)
                    {
                        float CellLum = 0.f;
                        int32 Count = 0;
                        for (int32 py = cy * CellH; py < (cy + 1) * CellH && py < RTH; ++py)
                        {
                            for (int32 px = cx * CellW; px < (cx + 1) * CellW && px < RTW; ++px)
                            {
                                const FColor& Pix = Pixels[py * RTW + px];
                                CellLum += (Pix.R + Pix.G + Pix.B) / (3.f * 255.f);
                                ++Count;
                            }
                        }
                        Brightness[PixIdx++] = (Count > 0) ? (CellLum / Count) : 0.f;
                    }
                }

                const int32 EyeBase = NumBaseSensors; // 58
                constexpr float Threshold = 0.5f;
                for (int32 i = 0; i < PixCount; ++i)
                {
                    const int32 Row = i / GridW;
                    const int32 Col = i % GridW;
                    const int32 Mirror = Row * GridW + (GridW - 1 - Col);
                    const float L = Brightness[i];
                    const float R = Brightness[Mirror];
                    OutSensors[EyeBase + i] = (L > Threshold) ? L : 0.f;
                    OutSensors[EyeBase + PixCount + i] = (L <= Threshold) ? (1.f - L) : 0.f;
                    OutSensors[EyeBase + 2 * PixCount + i] = (R > Threshold) ? R : 0.f;
                    OutSensors[EyeBase + 3 * PixCount + i] = (R <= Threshold) ? (1.f - R) : 0.f;
                }
            }
        }
    }
}

// ----------------------------------------------------------------------------
// ApplyActuators — 40 channels: the reference NAO output contract
// (NmNaoOem::OutputNames). Each joint reads its record readout channel; the
// 16 phalanx channels have no joint in this body.
// ----------------------------------------------------------------------------

void UNmNaoComponent::ApplyActuators(const TArray<float>& Actuators)
{
    if (Actuators.Num() < NmNaoOem::NumLinks)
    {
        return;
    }

    // A pulse-density brain: reference decode (actuation.rs) with the record's
    // decode_tau_s and servo wake-up, onto each joint's OEM range.
    const bool bGraded = IsGraded();
    const bool bFresh = IsFreshFrame();
    if (bGraded && bFresh)
    {
        GradedJoints.Accept(Actuators.GetData(), static_cast<size_t>(Actuators.Num()));
        GradedJoints.Advance();
    }

    for (int32 j = 0; j < JointConstraints.Num() && j < NumJoints; ++j)
    {
        UPhysicsConstraintComponent* Joint = JointConstraints[j];
        const int32 L = JointLink(j);
        if (!Joint || L < 0)
        {
            continue;
        }
        const NmNaoOem::FLink& K = NmNaoOem::Links[L];
        // Spikes: activation 0..1 over the upper half of the range (silence
        // never commands the lower limit); a tick without a reply holds rest.
        const double Rad = bGraded ? GradedJoints.Target(static_cast<size_t>(L), K.Lo, K.Hi)
                         : bFresh  ? K.Lo + (0.5 + 0.5 * FMath::Clamp(static_cast<double>(Actuators[K.Channel]), 0.0, 1.0)) * (K.Hi - K.Lo)
                                   : RestRad(K);
        Joint->SetAngularOrientationTarget(FRotator(static_cast<float>(FMath::RadiansToDegrees(Rad)), 0.f, 0.f));
    }
}

void UNmNaoComponent::GetSensorNames(TArray<FString>& OutNames) const
{
    OutNames.Reset(NumSensors);
    OutNames.Add(TEXT("sonar_l"));
    OutNames.Add(TEXT("sonar_r"));
    OutNames.Add(TEXT("accel_x")); OutNames.Add(TEXT("accel_y")); OutNames.Add(TEXT("accel_z"));
    OutNames.Add(TEXT("gyro_x"));  OutNames.Add(TEXT("gyro_y"));
    OutNames.Add(TEXT("gps_x"));   OutNames.Add(TEXT("gps_y"));   OutNames.Add(TEXT("gps_z"));
    OutNames.Add(TEXT("roll"));    OutNames.Add(TEXT("pitch"));    OutNames.Add(TEXT("yaw"));
    for (int32 f = 0; f < 2; ++f)
    {
        for (int32 p = 0; p < 4; ++p)
        {
            OutNames.Add(FString::Printf(TEXT("foot%c_pressure_%d"), f == 0 ? 'L' : 'R', p));
        }
    }
    OutNames.Add(TEXT("bumper_l"));
    OutNames.Add(TEXT("bumper_r"));
    for (int32 j = 0; j < NumJoints; ++j)
    {
        OutNames.Add(FString::Printf(TEXT("joint_pos_%02d"), j));
    }
    for (int32 j = 0; j < NumJointVelSensors; ++j)
    {
        OutNames.Add(FString::Printf(TEXT("joint_vel_%02d"), j));
    }
    constexpr int32 GridW = 8;
    constexpr int32 GridH = 6;
    for (int32 i = 0; i < GridW * GridH; ++i)
    {
        OutNames.Add(FString::Printf(TEXT("cam_l_on_%03d"), i));
    }
    for (int32 i = 0; i < GridW * GridH; ++i)
    {
        OutNames.Add(FString::Printf(TEXT("cam_l_off_%03d"), i));
    }
    for (int32 i = 0; i < GridW * GridH; ++i)
    {
        OutNames.Add(FString::Printf(TEXT("cam_r_on_%03d"), i));
    }
    for (int32 i = 0; i < GridW * GridH; ++i)
    {
        OutNames.Add(FString::Printf(TEXT("cam_r_off_%03d"), i));
    }
    while (OutNames.Num() < NumSensors)
    {
        OutNames.Add(FString::Printf(TEXT("sensor_reserved_%d"), OutNames.Num()));
    }
}

void UNmNaoComponent::GetActuatorNames(TArray<FString>& OutNames) const
{
    OutNames.Reset(NumActuators);
    for (const TCHAR* Name : NmNaoOem::OutputNames)
    {
        OutNames.Add(Name);
    }
}

// ============================================================================
// ANmNaoActor
// ============================================================================

ANmNaoActor::ANmNaoActor()
{
    PrimaryActorTick.bCanEverTick = false;

    static ConstructorHelpers::FObjectFinder<UStaticMesh> CubeMeshFinder(
        TEXT("/Engine/BasicShapes/Cube.Cube"));
    static ConstructorHelpers::FObjectFinder<UStaticMesh> SphereMeshFinder(
        TEXT("/Engine/BasicShapes/Sphere.Sphere"));

    UStaticMesh* CubeMesh   = CubeMeshFinder.Succeeded()   ? CubeMeshFinder.Object  : nullptr;
    UStaticMesh* SphereMesh = SphereMeshFinder.Succeeded() ? SphereMeshFinder.Object : nullptr;

    // OEM NAO geometry (NmNaoOem, the MuJoCo reference link tree) in the OEM
    // zero pose: arms forward, legs straight; joint angle 0 = built pose. The
    // root mesh is the torso collision box; every other location is relative
    // to it. Segment masses are the summed OEM link masses.
    static_assert(UNmNaoComponent::NumActuators == NmNaoOem::NumLinks, "NAO output contract");
    const FVector Root = Oem(NmNaoOem::TorsoBoxPositionM);
    const FVector TorsoScale = FVector(NmNaoOem::TorsoBoxHalfM[0], NmNaoOem::TorsoBoxHalfM[1], NmNaoOem::TorsoBoxHalfM[2]) * 2.0;
    // A child's relative location is in the (scaled) torso box space.
    auto Rel = [&](const FVector& TorsoFrame) { return (TorsoFrame - Root) / TorsoScale; };
    auto Mass = [](UStaticMeshComponent* M, double Kg) { M->SetMassOverrideInKg(NAME_None, static_cast<float>(Kg), true); };

    // --- Torso: OEM collision box ---
    TorsoMesh = CreateDefaultSubobject<UStaticMeshComponent>(TEXT("Torso"));
    TorsoMesh->SetStaticMesh(CubeMesh);
    TorsoMesh->SetWorldScale3D(TorsoScale);
    TorsoMesh->SetSimulatePhysics(true);
    Mass(TorsoMesh, NmNaoOem::TorsoMassKg);
    SetRootComponent(TorsoMesh);

    // --- Head: OEM head ellipsoid as a sphere on HeadPitch ---
    const NmNaoOem::FLink& HeadLink = NmNaoOem::Links[OemLink(TEXT("HeadPitch"))];
    const FVector HeadCentre = JointOrigin(TEXT("HeadPitch")) + Oem(HeadLink.ColliderPositionM);
    const double HeadScale = 2.0 * (HeadLink.ColliderHalfM[0] + HeadLink.ColliderHalfM[1] + HeadLink.ColliderHalfM[2]) / 3.0;
    {
        HeadMesh = CreateDefaultSubobject<UStaticMeshComponent>(TEXT("Head"));
        HeadMesh->SetStaticMesh(SphereMesh);
        HeadMesh->SetWorldScale3D(FVector(HeadScale));
        HeadMesh->SetupAttachment(TorsoMesh);
        HeadMesh->SetRelativeLocation(Rel(HeadCentre));
        HeadMesh->SetSimulatePhysics(true);
        Mass(HeadMesh, OemMass({TEXT("HeadYaw"), TEXT("HeadPitch")}));
    }

    // Box segment spanning joint A -> joint B (axis-aligned in the zero pose).
    auto MakeSegment = [&](const FString& Name, const FVector& A, const FVector& B, double ThicknessCm, double Kg)
        -> UStaticMeshComponent*
    {
        UStaticMeshComponent* M = CreateDefaultSubobject<UStaticMeshComponent>(*Name);
        M->SetStaticMesh(CubeMesh);
        const FVector D = (B - A).GetAbs();
        M->SetWorldScale3D(FVector(FMath::Max(D.X, ThicknessCm), FMath::Max(D.Y, ThicknessCm), FMath::Max(D.Z, ThicknessCm)) / 100.0);
        M->SetupAttachment(TorsoMesh);
        M->SetRelativeLocation(Rel(0.5 * (A + B)));
        M->SetSimulatePhysics(true);
        Mass(M, Kg);
        return M;
    };
    // Box at a link's OEM collision box (hand, sole).
    auto MakeBoxOf = [&](const FString& Name, const TCHAR* Joint, double Kg) -> UStaticMeshComponent*
    {
        const NmNaoOem::FLink& K = NmNaoOem::Links[OemLink(Joint)];
        UStaticMeshComponent* M = CreateDefaultSubobject<UStaticMeshComponent>(*Name);
        M->SetStaticMesh(CubeMesh);
        M->SetWorldScale3D(FVector(K.ColliderHalfM[0], K.ColliderHalfM[1], K.ColliderHalfM[2]) * 2.0);
        M->SetupAttachment(TorsoMesh);
        M->SetRelativeLocation(Rel(JointOrigin(Joint) + Oem(K.ColliderPositionM)));
        M->SetSimulatePhysics(true);
        Mass(M, Kg);
        return M;
    };

    // Joint constraint at the OEM joint origin. The rig keeps one constraint
    // per joint on shared segments, so its symmetric angular limits are an
    // envelope of the OEM range [lo, hi] (the command is clamped to [lo, hi]).
    // Drives: OEM kp, kd and torque limit in force mode, SI -> Unreal
    // (N m = 1e4 kg cm2/s2).
    enum class EDof : uint8 { Swing1, Swing2, Twist };
    auto MakeJoint = [&](const TCHAR* Joint, EDof Dof) -> UPhysicsConstraintComponent*
    {
        const NmNaoOem::FLink& K = NmNaoOem::Links[OemLink(Joint)];
        const float Envelope = static_cast<float>(FMath::RadiansToDegrees(FMath::Max(FMath::Abs(K.Lo), FMath::Abs(K.Hi))));
        UPhysicsConstraintComponent* J = CreateDefaultSubobject<UPhysicsConstraintComponent>(*(FString(TEXT("J_")) + Joint));
        J->SetupAttachment(TorsoMesh);
        J->SetRelativeLocation(Rel(JointOrigin(Joint)));
        J->SetAngularSwing1Limit(EAngularConstraintMotion::ACM_Limited, Dof == EDof::Swing1 ? Envelope : 5.f);
        J->SetAngularSwing2Limit(EAngularConstraintMotion::ACM_Limited, Dof == EDof::Swing2 ? Envelope : 5.f);
        J->SetAngularTwistLimit(EAngularConstraintMotion::ACM_Limited, Dof == EDof::Twist ? Envelope : 5.f);
        J->SetLinearXLimit(ELinearConstraintMotion::LCM_Locked, 0.f);
        J->SetLinearYLimit(ELinearConstraintMotion::LCM_Locked, 0.f);
        J->SetLinearZLimit(ELinearConstraintMotion::LCM_Locked, 0.f);
        J->SetAngularDriveMode(EAngularDriveMode::TwistAndSwing);
        J->SetAngularOrientationDrive(true, true);
        J->SetAngularDriveAccelerationMode(false);
        J->SetAngularDriveParams(static_cast<float>(K.Kp * 1e4), static_cast<float>(K.Kd * 1e4), static_cast<float>(K.EffortNm * 1e4));
        J->SetDisableCollision(true);
        return J;
    };

    // Ensure JointConstraints has exactly NumJoints slots (ENaoJoint order).
    JointConstraints.SetNum(UNmNaoComponent::NumJoints);
    int32 JIdx = 0;

    // Head joints (HeadYaw, HeadPitch)
    JointConstraints[JIdx++] = MakeJoint(TEXT("HeadYaw"),   EDof::Twist);
    JointConstraints[JIdx++] = MakeJoint(TEXT("HeadPitch"), EDof::Swing1);

    for (const TCHAR Side : {TEXT('L'), TEXT('R')})
    {
        auto N = [Side](const TCHAR* Joint) { return FString::Printf(TEXT("%c%s"), Side, Joint); };
        const double ArmCm = 2.0 * NmNaoOem::Links[OemLink(*N(TEXT("ShoulderRoll")))].ColliderHalfM[0] * 100.0;
        const double ForeCm = 2.0 * NmNaoOem::Links[OemLink(*N(TEXT("ElbowRoll")))].ColliderHalfM[0] * 100.0;
        UStaticMeshComponent* UArm = MakeSegment(N(TEXT("UpperArm")), JointOrigin(*N(TEXT("ShoulderPitch"))), JointOrigin(*N(TEXT("ElbowYaw"))),
                                                 ArmCm, OemMass({*N(TEXT("ShoulderPitch")), *N(TEXT("ShoulderRoll"))}));
        UStaticMeshComponent* FArm = MakeSegment(N(TEXT("ForeArm")), JointOrigin(*N(TEXT("ElbowYaw"))), JointOrigin(*N(TEXT("WristYaw"))),
                                                 ForeCm, OemMass({*N(TEXT("ElbowYaw")), *N(TEXT("ElbowRoll"))}));
        UStaticMeshComponent* Hand = MakeBoxOf(N(TEXT("Hand")), *N(TEXT("WristYaw")), OemMass({*N(TEXT("WristYaw"))}));
        ArmSegments.Add(UArm); ArmSegments.Add(FArm); ArmSegments.Add(Hand);
        JointConstraints[JIdx++] = MakeJoint(*N(TEXT("ShoulderPitch")), EDof::Swing1);
        JointConstraints[JIdx++] = MakeJoint(*N(TEXT("ShoulderRoll")),  EDof::Swing2);
        JointConstraints[JIdx++] = MakeJoint(*N(TEXT("ElbowYaw")),      EDof::Twist);
        JointConstraints[JIdx++] = MakeJoint(*N(TEXT("ElbowRoll")),     EDof::Swing2);
        JointConstraints[JIdx++] = MakeJoint(*N(TEXT("WristYaw")),      EDof::Twist);
    }

    for (const TCHAR Side : {TEXT('L'), TEXT('R')})
    {
        auto N = [Side](const TCHAR* Joint) { return FString::Printf(TEXT("%c%s"), Side, Joint); };
        const double ThighCm = 2.0 * NmNaoOem::Links[OemLink(*N(TEXT("HipPitch")))].ColliderHalfM[0] * 100.0;
        const double ShinCm = 2.0 * NmNaoOem::Links[OemLink(*N(TEXT("KneePitch")))].ColliderHalfM[0] * 100.0;
        UStaticMeshComponent* Thigh = MakeSegment(N(TEXT("Thigh")), JointOrigin(*N(TEXT("HipPitch"))), JointOrigin(*N(TEXT("KneePitch"))),
                                                  ThighCm, OemMass({*N(TEXT("HipYawPitch")), *N(TEXT("HipRoll")), *N(TEXT("HipPitch"))}));
        UStaticMeshComponent* Shank = MakeSegment(N(TEXT("Shank")), JointOrigin(*N(TEXT("KneePitch"))), JointOrigin(*N(TEXT("AnklePitch"))),
                                                  ShinCm, OemMass({*N(TEXT("KneePitch"))}));
        UStaticMeshComponent* Foot = MakeBoxOf(N(TEXT("Foot")), *N(TEXT("AnkleRoll")), OemMass({*N(TEXT("AnklePitch")), *N(TEXT("AnkleRoll"))}));
        LegSegments.Add(Thigh); LegSegments.Add(Shank); LegSegments.Add(Foot);
        FeetMeshes.Add(Foot);
        Foot->OnComponentHit.AddDynamic(this, &ANmNaoActor::OnFootHit);
        JointConstraints[JIdx++] = MakeJoint(*N(TEXT("HipYawPitch")), EDof::Swing1);
        JointConstraints[JIdx++] = MakeJoint(*N(TEXT("HipRoll")),     EDof::Swing2);
        JointConstraints[JIdx++] = MakeJoint(*N(TEXT("HipPitch")),    EDof::Swing1);
        JointConstraints[JIdx++] = MakeJoint(*N(TEXT("KneePitch")),   EDof::Swing1);
        JointConstraints[JIdx++] = MakeJoint(*N(TEXT("AnklePitch")),  EDof::Swing1);
        JointConstraints[JIdx++] = MakeJoint(*N(TEXT("AnkleRoll")),   EDof::Swing2);
    }

    // Eye camera (head mount)
    USceneCaptureComponent2D* EyeCam = CreateDefaultSubobject<USceneCaptureComponent2D>(TEXT("EyeCamera"));
    EyeCam->SetupAttachment(HeadMesh);
    // Reference CameraTop: torso frame (5.39, 0, 19.44) cm, in head-sphere space.
    EyeCam->SetRelativeLocation((FVector(5.39, 0.0, 19.44) - HeadCentre) / HeadScale);
    EyeCam->FOVAngle = 60.7f;
    EyeCam->CaptureSource = ESceneCaptureSource::SCS_FinalColorLDR;
    EyeCam->bCaptureEveryFrame = true;

    // --- Brain component ---
    NaoComponent = CreateDefaultSubobject<UNmNaoComponent>(TEXT("NaoComponent"));
    NaoComponent->TorsoMesh        = TorsoMesh;
    NaoComponent->HeadMesh         = HeadMesh;
    NaoComponent->ArmSegments      = ArmSegments;
    NaoComponent->LegSegments      = LegSegments;
    NaoComponent->FeetMeshes       = FeetMeshes;
    NaoComponent->JointConstraints = JointConstraints;
    NaoComponent->EyeCamera        = EyeCam;
}

void ANmNaoActor::BeginPlay()
{
    Super::BeginPlay();
    if (!FPlatformMisc::GetEnvironmentVariable(TEXT("NM_NAO_SESSION_FILE")).IsEmpty())
    {
        auto Chat = NewObject<UNmNaoChatComponent>(this);
        Chat->RegisterComponent();
    }

    // Wire all joint constraints: Component1=parent body, Component2=child segment
    // For simplicity, all joints are attached to the torso root; each constraint
    // connects TorsoMesh → the nearest child segment.  A production implementation
    // would walk the joint chain carefully per joint type.
    auto BindJoint = [&](int32 JIdx, UStaticMeshComponent* Parent, UStaticMeshComponent* Child)
    {
        if (!JointConstraints.IsValidIndex(JIdx) || !JointConstraints[JIdx])
        {
            return;
        }
        // The constraint stays at its OEM joint origin (constructor).
        JointConstraints[JIdx]->OverrideComponent1 = Parent;
        JointConstraints[JIdx]->OverrideComponent2 = Child;
        JointConstraints[JIdx]->InitComponentConstraint();
    };

    // Head
    BindJoint(0, TorsoMesh, HeadMesh);
    BindJoint(1, TorsoMesh, HeadMesh);

    // Left arm: 5 joints → upper arm, forearm, hand
    if (ArmSegments.Num() >= 6)
    {
        BindJoint(2, TorsoMesh,    ArmSegments[0]); // LShoulderPitch → LUpperArm
        BindJoint(3, TorsoMesh,    ArmSegments[0]); // LShoulderRoll
        BindJoint(4, ArmSegments[0], ArmSegments[1]); // LElbowYaw
        BindJoint(5, ArmSegments[0], ArmSegments[1]); // LElbowRoll
        BindJoint(6, ArmSegments[1], ArmSegments[2]); // LWristYaw

        BindJoint(7,  TorsoMesh,    ArmSegments[3]); // RShoulderPitch
        BindJoint(8,  TorsoMesh,    ArmSegments[3]);
        BindJoint(9,  ArmSegments[3], ArmSegments[4]);
        BindJoint(10, ArmSegments[3], ArmSegments[4]);
        BindJoint(11, ArmSegments[4], ArmSegments[5]);
    }

    // Left leg
    if (LegSegments.Num() >= 6)
    {
        BindJoint(12, TorsoMesh,    LegSegments[0]);
        BindJoint(13, TorsoMesh,    LegSegments[0]);
        BindJoint(14, TorsoMesh,    LegSegments[0]);
        BindJoint(15, LegSegments[0], LegSegments[1]);
        BindJoint(16, LegSegments[1], LegSegments[2]);
        BindJoint(17, LegSegments[1], LegSegments[2]);

        BindJoint(18, TorsoMesh,    LegSegments[3]);
        BindJoint(19, TorsoMesh,    LegSegments[3]);
        BindJoint(20, TorsoMesh,    LegSegments[3]);
        BindJoint(21, LegSegments[3], LegSegments[4]);
        BindJoint(22, LegSegments[4], LegSegments[5]);
        BindJoint(23, LegSegments[4], LegSegments[5]);
    }
}

void ANmNaoActor::OnFootHit(UPrimitiveComponent* HitComp, AActor* /*OtherActor*/,
                             UPrimitiveComponent* /*OtherComp*/, FVector /*NormalImpulse*/,
                             const FHitResult& /*Hit*/)
{
    if (FeetMeshes.Num() > 0 && FeetMeshes[0] == HitComp)
    {
        NaoComponent->FootContacts[0] = true;
    }
    else if (FeetMeshes.Num() > 1 && FeetMeshes[1] == HitComp)
    {
        NaoComponent->FootContacts[1] = true;
    }
}
