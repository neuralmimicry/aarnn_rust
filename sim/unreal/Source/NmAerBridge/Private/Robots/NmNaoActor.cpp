// Copyright NeuralMimicry. All Rights Reserved.

#include "Robots/NmNaoActor.h"

#include "Components/StaticMeshComponent.h"
#include "Components/SceneCaptureComponent2D.h"
#include "Engine/TextureRenderTarget2D.h"
#include "PhysicsEngine/PhysicsConstraintComponent.h"
#include "Engine/World.h"
#include "Engine/OverlapResult.h"
#include "UObject/ConstructorHelpers.h"

// ============================================================================
// UNmNaoComponent
// ============================================================================

UNmNaoComponent::UNmNaoComponent()
{
    BrainId = TEXT("nao");
    PrimaryComponentTick.bCanEverTick = true;
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
    // Offset to 4 corners of a typical NAO foot (6 cm × 10 cm)
    const FVector Offsets[4] = {
        FVector( 3.f,  5.f, -1.f),
        FVector(-3.f,  5.f, -1.f),
        FVector( 3.f, -5.f, -1.f),
        FVector(-3.f, -5.f, -1.f),
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
// ApplyActuators — 40 channels
// [0..23]  26 joint drives (ENaoJoint enum order)
// [26..33] 8 LED placeholder floats
// [34..39] reserved
// ----------------------------------------------------------------------------

void UNmNaoComponent::ApplyActuators(const TArray<float>& Actuators)
{
    if (Actuators.Num() < 26)
    {
        return;
    }

    constexpr float MaxAngleDeg = 120.f;

    for (int32 j = 0; j < JointConstraints.Num() && j < NumJoints; ++j)
    {
        UPhysicsConstraintComponent* Joint = JointConstraints[j];
        if (Joint)
        {
            const float Target = FMath::Clamp(Actuators[j], -1.f, 1.f) * MaxAngleDeg;
            Joint->SetAngularOrientationTarget(FRotator(Target, 0.f, 0.f));
        }
    }

    // LED floats
    if (Actuators.Num() > 33)
    {
        LedChestR = FMath::Clamp(Actuators[26], 0.f, 1.f);
        LedChestG = FMath::Clamp(Actuators[27], 0.f, 1.f);
        LedChestB = FMath::Clamp(Actuators[28], 0.f, 1.f);
        LedFootL  = FMath::Clamp(Actuators[29], 0.f, 1.f);
        LedFootR  = FMath::Clamp(Actuators[30], 0.f, 1.f);
        LedEarL   = FMath::Clamp(Actuators[31], 0.f, 1.f);
        LedEarR   = FMath::Clamp(Actuators[32], 0.f, 1.f);
        LedEyeL   = FMath::Clamp(Actuators[33], 0.f, 1.f);
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
    // Mirror ENaoJoint order
    const char* JointNameStr[] = {
        "HeadYaw","HeadPitch",
        "LShoulderPitch","LShoulderRoll","LElbowYaw","LElbowRoll","LWristYaw",
        "RShoulderPitch","RShoulderRoll","RElbowYaw","RElbowRoll","RWristYaw",
        "LHipYawPitch","LHipRoll","LHipPitch","LKneePitch","LAnklePitch","LAnkleRoll",
        "RHipYawPitch","RHipRoll","RHipPitch","RKneePitch","RAnklePitch","RAnkleRoll",
    };
    for (const char* Name : JointNameStr)
    {
        OutNames.Add(ANSI_TO_TCHAR(Name));
    }
    // Remaining 2 joints + 8 LEDs = 10 more
    OutNames.Add(TEXT("joint_reserved_24"));
    OutNames.Add(TEXT("joint_reserved_25"));
    OutNames.Add(TEXT("led_chest_r")); OutNames.Add(TEXT("led_chest_g")); OutNames.Add(TEXT("led_chest_b"));
    OutNames.Add(TEXT("led_foot_l"));  OutNames.Add(TEXT("led_foot_r"));
    OutNames.Add(TEXT("led_ear_l"));   OutNames.Add(TEXT("led_ear_r"));
    OutNames.Add(TEXT("led_eye_l"));
    while (OutNames.Num() < NumActuators)
    {
        OutNames.Add(FString::Printf(TEXT("reserved_%d"), OutNames.Num()));
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

    // --- Torso (15×8×22 cm) ---
    TorsoMesh = CreateDefaultSubobject<UStaticMeshComponent>(TEXT("Torso"));
    TorsoMesh->SetStaticMesh(CubeMesh);
    TorsoMesh->SetWorldScale3D(FVector(0.15f, 0.08f, 0.22f));
    TorsoMesh->SetSimulatePhysics(true);
    SetRootComponent(TorsoMesh);

    // --- Head (7 cm sphere) ---
    HeadMesh = CreateDefaultSubobject<UStaticMeshComponent>(TEXT("Head"));
    HeadMesh->SetStaticMesh(SphereMesh);
    HeadMesh->SetWorldScale3D(FVector(0.07f));
    HeadMesh->SetupAttachment(TorsoMesh);
    HeadMesh->SetRelativeLocation(FVector(0.f, 0.f, 14.5f));
    HeadMesh->SetSimulatePhysics(true);

    // Helper: create a box segment
    auto MakeBox = [&](const FString& Name, FVector Scale, FVector RelLoc,
                        USceneComponent* Parent) -> UStaticMeshComponent*
    {
        UStaticMeshComponent* M = CreateDefaultSubobject<UStaticMeshComponent>(*Name);
        M->SetStaticMesh(CubeMesh);
        M->SetWorldScale3D(Scale);
        M->SetupAttachment(Parent ? Parent : TorsoMesh);
        M->SetRelativeLocation(RelLoc);
        M->SetSimulatePhysics(true);
        return M;
    };

    // Helper: create a joint constraint
    auto MakeJoint = [&](const FString& Name, FVector RelLoc, float Swing1, float Swing2, float Twist)
        -> UPhysicsConstraintComponent*
    {
        UPhysicsConstraintComponent* J = CreateDefaultSubobject<UPhysicsConstraintComponent>(*Name);
        J->SetupAttachment(TorsoMesh);
        J->SetRelativeLocation(RelLoc);
        J->SetAngularSwing1Limit(EAngularConstraintMotion::ACM_Limited, Swing1);
        J->SetAngularSwing2Limit(EAngularConstraintMotion::ACM_Limited, Swing2);
        J->SetAngularTwistLimit(EAngularConstraintMotion::ACM_Limited, Twist);
        J->SetLinearXLimit(ELinearConstraintMotion::LCM_Locked, 0.f);
        J->SetLinearYLimit(ELinearConstraintMotion::LCM_Locked, 0.f);
        J->SetLinearZLimit(ELinearConstraintMotion::LCM_Locked, 0.f);
        J->SetAngularDriveMode(EAngularDriveMode::TwistAndSwing);
        J->SetAngularOrientationDrive(true, true);
        J->SetAngularDriveParams(300.f, 30.f, 0.f);
        return J;
    };

    // Ensure JointConstraints has exactly NumJoints slots
    JointConstraints.SetNum(UNmNaoComponent::NumJoints);
    int32 JIdx = 0;

    // Head joints (HeadYaw, HeadPitch)
    JointConstraints[JIdx++] = MakeJoint(TEXT("J_HeadYaw"),   FVector(0.f, 0.f, 11.f), 60.f, 5.f,  120.f);
    JointConstraints[JIdx++] = MakeJoint(TEXT("J_HeadPitch"), FVector(0.f, 0.f, 13.f), 45.f, 5.f,  5.f);

    // Left arm
    const float LX = 8.f;
    UStaticMeshComponent* LUArm = MakeBox(TEXT("LUpperArm"), FVector(0.04f,0.04f,0.10f), FVector( LX, 0.f, 8.f), TorsoMesh);
    UStaticMeshComponent* LFArm = MakeBox(TEXT("LForeArm"),  FVector(0.03f,0.03f,0.09f), FVector( LX, 0.f,-2.f), TorsoMesh);
    UStaticMeshComponent* LHand = MakeBox(TEXT("LHand"),     FVector(0.03f,0.02f,0.04f), FVector( LX, 0.f,-7.f), TorsoMesh);
    ArmSegments.Add(LUArm); ArmSegments.Add(LFArm); ArmSegments.Add(LHand);
    JointConstraints[JIdx++] = MakeJoint(TEXT("J_LShoulderPitch"), FVector( LX,0.f, 11.f), 120.f,5.f, 5.f);
    JointConstraints[JIdx++] = MakeJoint(TEXT("J_LShoulderRoll"),  FVector( LX,0.f, 11.f), 5.f,76.f, 5.f);
    JointConstraints[JIdx++] = MakeJoint(TEXT("J_LElbowYaw"),      FVector( LX,0.f,  3.f), 5.f, 5.f,120.f);
    JointConstraints[JIdx++] = MakeJoint(TEXT("J_LElbowRoll"),     FVector( LX,0.f,  3.f), 5.f,88.f, 5.f);
    JointConstraints[JIdx++] = MakeJoint(TEXT("J_LWristYaw"),      FVector( LX,0.f, -4.f), 5.f, 5.f,105.f);

    // Right arm
    const float RX = -8.f;
    UStaticMeshComponent* RUArm = MakeBox(TEXT("RUpperArm"), FVector(0.04f,0.04f,0.10f), FVector(RX, 0.f, 8.f), TorsoMesh);
    UStaticMeshComponent* RFArm = MakeBox(TEXT("RForeArm"),  FVector(0.03f,0.03f,0.09f), FVector(RX, 0.f,-2.f), TorsoMesh);
    UStaticMeshComponent* RHand = MakeBox(TEXT("RHand"),     FVector(0.03f,0.02f,0.04f), FVector(RX, 0.f,-7.f), TorsoMesh);
    ArmSegments.Add(RUArm); ArmSegments.Add(RFArm); ArmSegments.Add(RHand);
    JointConstraints[JIdx++] = MakeJoint(TEXT("J_RShoulderPitch"), FVector(RX,0.f, 11.f), 120.f,5.f, 5.f);
    JointConstraints[JIdx++] = MakeJoint(TEXT("J_RShoulderRoll"),  FVector(RX,0.f, 11.f), 5.f,76.f, 5.f);
    JointConstraints[JIdx++] = MakeJoint(TEXT("J_RElbowYaw"),      FVector(RX,0.f,  3.f), 5.f, 5.f,120.f);
    JointConstraints[JIdx++] = MakeJoint(TEXT("J_RElbowRoll"),     FVector(RX,0.f,  3.f), 5.f,88.f, 5.f);
    JointConstraints[JIdx++] = MakeJoint(TEXT("J_RWristYaw"),      FVector(RX,0.f, -4.f), 5.f, 5.f,105.f);

    // Left leg
    UStaticMeshComponent* LThigh = MakeBox(TEXT("LThigh"), FVector(0.06f,0.06f,0.12f), FVector(4.f,0.f,-7.f),  TorsoMesh);
    UStaticMeshComponent* LShank = MakeBox(TEXT("LShank"), FVector(0.05f,0.05f,0.10f), FVector(4.f,0.f,-22.f), TorsoMesh);
    UStaticMeshComponent* LFoot  = MakeBox(TEXT("LFoot"),  FVector(0.10f,0.05f,0.02f), FVector(4.f,0.f,-30.f), TorsoMesh);
    LegSegments.Add(LThigh); LegSegments.Add(LShank); LegSegments.Add(LFoot);
    FeetMeshes.Add(LFoot);
    LFoot->OnComponentHit.AddDynamic(this, &ANmNaoActor::OnFootHit);
    JointConstraints[JIdx++] = MakeJoint(TEXT("J_LHipYawPitch"), FVector(4.f,0.f,-3.f),  45.f,21.f,21.f);
    JointConstraints[JIdx++] = MakeJoint(TEXT("J_LHipRoll"),      FVector(4.f,0.f,-3.f),  5.f,45.f, 5.f);
    JointConstraints[JIdx++] = MakeJoint(TEXT("J_LHipPitch"),     FVector(4.f,0.f,-3.f),  88.f,5.f, 5.f);
    JointConstraints[JIdx++] = MakeJoint(TEXT("J_LKneePitch"),    FVector(4.f,0.f,-15.f), 120.f,5.f,5.f);
    JointConstraints[JIdx++] = MakeJoint(TEXT("J_LAnklePitch"),   FVector(4.f,0.f,-26.f), 68.f,5.f, 5.f);
    JointConstraints[JIdx++] = MakeJoint(TEXT("J_LAnkleRoll"),    FVector(4.f,0.f,-26.f), 5.f,45.f, 5.f);

    // Right leg
    UStaticMeshComponent* RThigh = MakeBox(TEXT("RThigh"), FVector(0.06f,0.06f,0.12f), FVector(-4.f,0.f,-7.f),  TorsoMesh);
    UStaticMeshComponent* RShank = MakeBox(TEXT("RShank"), FVector(0.05f,0.05f,0.10f), FVector(-4.f,0.f,-22.f), TorsoMesh);
    UStaticMeshComponent* RFoot  = MakeBox(TEXT("RFoot"),  FVector(0.10f,0.05f,0.02f), FVector(-4.f,0.f,-30.f), TorsoMesh);
    LegSegments.Add(RThigh); LegSegments.Add(RShank); LegSegments.Add(RFoot);
    FeetMeshes.Add(RFoot);
    RFoot->OnComponentHit.AddDynamic(this, &ANmNaoActor::OnFootHit);
    JointConstraints[JIdx++] = MakeJoint(TEXT("J_RHipYawPitch"), FVector(-4.f,0.f,-3.f),  45.f,21.f,21.f);
    JointConstraints[JIdx++] = MakeJoint(TEXT("J_RHipRoll"),      FVector(-4.f,0.f,-3.f),  5.f,45.f, 5.f);
    JointConstraints[JIdx++] = MakeJoint(TEXT("J_RHipPitch"),     FVector(-4.f,0.f,-3.f),  88.f,5.f, 5.f);
    JointConstraints[JIdx++] = MakeJoint(TEXT("J_RKneePitch"),    FVector(-4.f,0.f,-15.f), 120.f,5.f,5.f);
    JointConstraints[JIdx++] = MakeJoint(TEXT("J_RAnklePitch"),   FVector(-4.f,0.f,-26.f), 68.f,5.f, 5.f);
    JointConstraints[JIdx++] = MakeJoint(TEXT("J_RAnkleRoll"),    FVector(-4.f,0.f,-26.f), 5.f,45.f, 5.f);

    // Eye camera (head mount)
    USceneCaptureComponent2D* EyeCam = CreateDefaultSubobject<USceneCaptureComponent2D>(TEXT("EyeCamera"));
    EyeCam->SetupAttachment(HeadMesh);
    EyeCam->SetRelativeLocation(FVector(3.5f, 0.f, 0.f));
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
        if (Parent && Child)
        {
            // Constraint at the world midpoint so it holds the bodies apart.
            JointConstraints[JIdx]->SetWorldLocation(
                0.5f * (Parent->GetComponentLocation() + Child->GetComponentLocation()));
        }
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
