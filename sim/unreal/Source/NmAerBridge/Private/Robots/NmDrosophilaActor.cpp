// Copyright NeuralMimicry. All Rights Reserved.

#include "Robots/NmDrosophilaActor.h"

#include "Components/StaticMeshComponent.h"
#include "Components/SceneCaptureComponent2D.h"
#include "Engine/TextureRenderTarget2D.h"
#include "PhysicsEngine/PhysicsConstraintComponent.h"
#include "Engine/World.h"
#include "UObject/ConstructorHelpers.h"

// ============================================================================
// UNmDrosophilaComponent
// ============================================================================

UNmDrosophilaComponent::UNmDrosophilaComponent()
{
    BrainId = TEXT("drosophila_banc");
    PrimaryComponentTick.bCanEverTick = true;
}

// ----------------------------------------------------------------------------
// CollectSensors — 418 channels
// [0..23]      24 leg joint positions
// [24..29]      6 foot contacts
// [30..31]      2 antenna traces
// [32..33]      2 compact IMU summaries
// [34..417]   384 compound-eye event channels
//               [34..129]   left ON
//               [130..225]  left OFF
//               [226..321]  right ON
//               [322..417]  right OFF
// ----------------------------------------------------------------------------

void UNmDrosophilaComponent::CollectSensors(TArray<float>& OutSensors)
{
    OutSensors.SetNumZeroed(NumSensors);

    AActor* Owner = GetOwner();
    UWorld* World = GetWorld();
    if (!Owner || !World)
    {
        return;
    }

    // --- Leg joint positions [0..23] ---
    // Read the actual joint angle (twist/swing, in degrees) as the proprioceptive
    // position signal, normalised over the joint's expected range.
    constexpr float MaxJointDeg = 180.f;
    for (int32 j = 0; j < LegJoints.Num() && j < NumLegJoints; ++j)
    {
        UPhysicsConstraintComponent* Joint = LegJoints[j];
        if (Joint)
        {
            const float AngleDeg = FMath::Max3(FMath::Abs(Joint->GetCurrentTwist()),
                                               FMath::Abs(Joint->GetCurrentSwing1()),
                                               FMath::Abs(Joint->GetCurrentSwing2()));
            const float Pos = FMath::Clamp(AngleDeg / MaxJointDeg, 0.f, 1.f);
            OutSensors[j] = Pos;
        }
    }

    // --- Foot contacts [24..29] ---
    for (int32 f = 0; f < NumLegs; ++f)
    {
        OutSensors[24 + f] = FootContacts[f] ? 1.f : 0.f;
        FootContacts[f] = false;  // reset each frame (set by hit callback)
    }

    // --- Antennae distance [30..31] ---
    if (Head)
    {
        constexpr float AntennaRangeCm = 10.f; // ~0.1 m
        const FVector HeadLoc = Head->GetComponentLocation();
        // Left antenna: trace at +30° yaw from head forward
        const FVector LeftDir  = FRotator(0.f, 30.f, 0.f).RotateVector(Head->GetForwardVector());
        // Right antenna: trace at -30° yaw
        const FVector RightDir = FRotator(0.f, -30.f, 0.f).RotateVector(Head->GetForwardVector());

        auto TraceAntenna = [&](const FVector& Dir) -> float
        {
            FHitResult Hit;
            const bool bHit = World->LineTraceSingleByChannel(
                Hit, HeadLoc, HeadLoc + Dir * AntennaRangeCm, ECC_Visibility);
            return bHit ? (1.f - (Hit.Distance / AntennaRangeCm)) : 0.f;
        };

        OutSensors[30] = TraceAntenna(LeftDir);
        OutSensors[31] = TraceAntenna(RightDir);
    }

    // --- Compact IMU summaries [32..33] ---
    if (Thorax)
    {
        const FVector LinVel = Thorax->GetPhysicsLinearVelocity();
        const FVector AngVel = Thorax->GetPhysicsAngularVelocityInDegrees();
        const FVector Fwd = Thorax->GetForwardVector();

        // Forward speed proxy.
        constexpr float MaxForwardSpeedCmS = 120.f;
        const float ForwardSpeed = FVector::DotProduct(Fwd, LinVel);
        OutSensors[32] = FMath::Clamp((ForwardSpeed / MaxForwardSpeedCmS) * 0.5f + 0.5f, 0.f, 1.f);

        // Yaw-rate proxy.
        constexpr float MaxTurnRateDegS = 360.f;
        OutSensors[33] = FMath::Clamp((AngVel.Z / MaxTurnRateDegS) * 0.5f + 0.5f, 0.f, 1.f);

        PrevLinearVel = LinVel;
        PrevAngularVel = AngVel;
    }

    // --- Eye event channels [34..417] ---
    auto FillEyeEvents = [&](USceneCaptureComponent2D* Capture,
                             TArray<float>& PrevLuminance,
                             int32 OnBase,
                             int32 OffBase)
    {
        if (!Capture || !Capture->TextureTarget)
        {
            return;
        }

        Capture->CaptureScene();
        FRenderTarget* RenderTarget = Capture->TextureTarget->GameThread_GetRenderTargetResource();
        if (!RenderTarget)
        {
            return;
        }

        TArray<FColor> Pixels;
        if (!RenderTarget->ReadPixels(Pixels) || Pixels.Num() == 0)
        {
            return;
        }

        if (PrevLuminance.Num() != NumEyePixels)
        {
            PrevLuminance.Init(0.f, NumEyePixels);
        }

        const int32 Limit = FMath::Min(NumEyePixels, Pixels.Num());
        for (int32 i = 0; i < Limit; ++i)
        {
            const FColor& C = Pixels[i];
            const float L = (static_cast<float>(C.R) + static_cast<float>(C.G) + static_cast<float>(C.B))
                            / (3.f * 255.f);
            const float Delta = L - PrevLuminance[i];
            OutSensors[OnBase + i] = FMath::Clamp(Delta > 0.f ? Delta * 4.f : 0.f, 0.f, 1.f);
            OutSensors[OffBase + i] = FMath::Clamp(Delta < 0.f ? -Delta * 4.f : 0.f, 0.f, 1.f);
            PrevLuminance[i] = L;
        }
    };

    const int32 EyeBase = NumCoreSensors;
    FillEyeEvents(EyeLeft, PrevEyeLeftLum, EyeBase, EyeBase + NumEyePixels);
    FillEyeEvents(EyeRight, PrevEyeRightLum, EyeBase + 2 * NumEyePixels, EyeBase + 3 * NumEyePixels);
}

// ----------------------------------------------------------------------------
// ApplyActuators — 48 channels (first 28 used)
// [0..23]  6×4 leg joint angular drive targets (normalized -1..1 → ±60°)
// [24..27] 2×2 wing joint drives
// [28..47] unused (reserved)
// ----------------------------------------------------------------------------

void UNmDrosophilaComponent::ApplyActuators(const TArray<float>& Actuators)
{
    if (Actuators.Num() < 28)
    {
        return;
    }

    constexpr float MaxLegAngleDeg  = 60.f;
    constexpr float MaxWingAngleDeg = 90.f;

    // Leg joints [0..23]
    for (int32 j = 0; j < LegJoints.Num() && j < NumLegJoints; ++j)
    {
        UPhysicsConstraintComponent* Joint = LegJoints[j];
        if (Joint)
        {
            const float Target = FMath::Clamp(Actuators[j], -1.f, 1.f) * MaxLegAngleDeg;
            Joint->SetAngularOrientationTarget(FRotator(Target, 0.f, 0.f));
        }
    }

    // Wing joints [24..27]
    for (int32 w = 0; w < WingJointComps.Num() && w < 4; ++w)
    {
        UPhysicsConstraintComponent* Joint = WingJointComps[w];
        if (Joint)
        {
            const float Target = FMath::Clamp(Actuators[24 + w], -1.f, 1.f) * MaxWingAngleDeg;
            Joint->SetAngularOrientationTarget(FRotator(Target, 0.f, 0.f));
        }
    }
}

void UNmDrosophilaComponent::GetSensorNames(TArray<FString>& OutNames) const
{
    OutNames.Reset(NumSensors);
    const char* SegNames[] = {"coxa", "femur", "tibia", "tarsus"};
    for (int32 leg = 0; leg < NumLegs; ++leg)
    {
        for (int32 seg = 0; seg < LegSegments; ++seg)
        {
            OutNames.Add(FString::Printf(TEXT("leg%d_%s_pos"), leg, ANSI_TO_TCHAR(SegNames[seg])));
        }
    }
    for (int32 f = 0; f < NumLegs; ++f)
    {
        OutNames.Add(FString::Printf(TEXT("foot%d_contact"), f));
    }
    OutNames.Add(TEXT("antenna_left_dist"));
    OutNames.Add(TEXT("antenna_right_dist"));
    OutNames.Add(TEXT("imu_forward_speed"));
    OutNames.Add(TEXT("imu_yaw_rate"));
    for (int32 i = 0; i < NumEyePixels; ++i)
    {
        OutNames.Add(FString::Printf(TEXT("eye_l_on_%03d"), i));
    }
    for (int32 i = 0; i < NumEyePixels; ++i)
    {
        OutNames.Add(FString::Printf(TEXT("eye_l_off_%03d"), i));
    }
    for (int32 i = 0; i < NumEyePixels; ++i)
    {
        OutNames.Add(FString::Printf(TEXT("eye_r_on_%03d"), i));
    }
    for (int32 i = 0; i < NumEyePixels; ++i)
    {
        OutNames.Add(FString::Printf(TEXT("eye_r_off_%03d"), i));
    }
}

void UNmDrosophilaComponent::GetActuatorNames(TArray<FString>& OutNames) const
{
    OutNames.Reset(NumActuators);
    const char* SegNames[] = {"coxa", "femur", "tibia", "tarsus"};
    for (int32 leg = 0; leg < NumLegs; ++leg)
    {
        for (int32 seg = 0; seg < LegSegments; ++seg)
        {
            OutNames.Add(FString::Printf(TEXT("leg%d_%s_drive"), leg, ANSI_TO_TCHAR(SegNames[seg])));
        }
    }
    for (int32 w = 0; w < NumWings * WingJoints; ++w)
    {
        OutNames.Add(FString::Printf(TEXT("wing%d_drive"), w));
    }
    // Pad remaining reserved channels
    for (int32 i = NumLegJoints + NumWings * WingJoints; i < NumActuators; ++i)
    {
        OutNames.Add(FString::Printf(TEXT("reserved_%02d"), i));
    }
}

// ============================================================================
// ANmDrosophilaActor
// ============================================================================

ANmDrosophilaActor::ANmDrosophilaActor()
{
    PrimaryActorTick.bCanEverTick = false;

    static ConstructorHelpers::FObjectFinder<UStaticMesh> CubeMeshFinder(
        TEXT("/Engine/BasicShapes/Cube.Cube"));
    static ConstructorHelpers::FObjectFinder<UStaticMesh> SphereMeshFinder(
        TEXT("/Engine/BasicShapes/Sphere.Sphere"));
    static ConstructorHelpers::FObjectFinder<UStaticMesh> PlaneMeshFinder(
        TEXT("/Engine/BasicShapes/Plane.Plane"));

    UStaticMesh* CubeMesh   = CubeMeshFinder.Succeeded()   ? CubeMeshFinder.Object   : nullptr;
    UStaticMesh* SphereMesh = SphereMeshFinder.Succeeded() ? SphereMeshFinder.Object  : nullptr;
    UStaticMesh* PlaneMesh  = PlaneMeshFinder.Succeeded()  ? PlaneMeshFinder.Object   : nullptr;

    // --- Thorax (root, 4×3×6 cm) ---
    ThoraxMesh = CreateDefaultSubobject<UStaticMeshComponent>(TEXT("Thorax"));
    ThoraxMesh->SetStaticMesh(SphereMesh); // Rounded organic sphere-based thorax
    ThoraxMesh->SetWorldScale3D(FVector(0.032f, 0.024f, 0.032f));
    ThoraxMesh->SetSimulatePhysics(true);
    SetRootComponent(ThoraxMesh);

    // --- Head (1.5 cm sphere) ---
    HeadMesh = CreateDefaultSubobject<UStaticMeshComponent>(TEXT("Head"));
    HeadMesh->SetStaticMesh(SphereMesh);
    HeadMesh->SetWorldScale3D(FVector(0.015f));
    HeadMesh->SetupAttachment(ThoraxMesh);
    HeadMesh->SetRelativeLocation(FVector(2.2f, 0.f, 1.2f));
    HeadMesh->SetSimulatePhysics(false); // Fuses head rigidly to thorax (resolves physics conflict)

    // --- Abdomen Mesh (tapered insect tail) ---
    AbdomenMesh = CreateDefaultSubobject<UStaticMeshComponent>(TEXT("Abdomen"));
    AbdomenMesh->SetStaticMesh(SphereMesh);
    AbdomenMesh->SetWorldScale3D(FVector(0.016f, 0.014f, 0.032f));
    AbdomenMesh->SetupAttachment(ThoraxMesh);
    AbdomenMesh->SetRelativeLocation(FVector(-1.2f, 0.f, -0.2f));
    AbdomenMesh->SetSimulatePhysics(false);
    AbdomenMesh->SetCollisionEnabled(ECollisionEnabled::NoCollision);

    // --- Big Red Compound Eyes ---
    EyeLeftMesh = CreateDefaultSubobject<UStaticMeshComponent>(TEXT("EyeLeftVisual"));
    EyeLeftMesh->SetStaticMesh(SphereMesh);
    EyeLeftMesh->SetWorldScale3D(FVector(0.012f, 0.012f, 0.006f));
    EyeLeftMesh->SetupAttachment(HeadMesh);
    EyeLeftMesh->SetRelativeLocation(FVector(1.0f, 0.8f, 0.5f));
    EyeLeftMesh->SetRelativeRotation(FRotator(0.f, -45.f, 0.f));
    EyeLeftMesh->SetSimulatePhysics(false);
    EyeLeftMesh->SetCollisionEnabled(ECollisionEnabled::NoCollision);

    EyeRightMesh = CreateDefaultSubobject<UStaticMeshComponent>(TEXT("EyeRightVisual"));
    EyeRightMesh->SetStaticMesh(SphereMesh);
    EyeRightMesh->SetWorldScale3D(FVector(0.012f, 0.012f, 0.006f));
    EyeRightMesh->SetupAttachment(HeadMesh);
    EyeRightMesh->SetRelativeLocation(FVector(1.0f, -0.8f, 0.5f));
    EyeRightMesh->SetRelativeRotation(FRotator(0.f, 45.f, 0.f));
    EyeRightMesh->SetSimulatePhysics(false);
    EyeRightMesh->SetCollisionEnabled(ECollisionEnabled::NoCollision);

    // --- Legs: 6 legs × 4 segments ---
    const FVector LegOrigins[6] = {
        FVector( 1.5f,  2.f, -1.f), FVector(0.f,  2.f, -1.f), FVector(-1.5f,  2.f, -1.f),
        FVector( 1.5f, -2.f, -1.f), FVector(0.f, -2.f, -1.f), FVector(-1.5f, -2.f, -1.f),
    };
    const float LegSegLengths[] = {0.8f, 1.2f, 1.5f, 0.5f}; // coxa,femur,tibia,tarsus (cm)
    const char* LegSegNames[]   = {"coxa", "femur", "tibia", "tarsus"};

    for (int32 leg = 0; leg < UNmDrosophilaComponent::NumLegs; ++leg)
    {
        UStaticMeshComponent* PrevSeg = ThoraxMesh;
        FVector PrevOffset = LegOrigins[leg];

        for (int32 s = 0; s < UNmDrosophilaComponent::LegSegments; ++s)
        {
            const FString MeshName  = FString::Printf(TEXT("Leg%d_%s"), leg, ANSI_TO_TCHAR(LegSegNames[s]));
            const FString JointName = FString::Printf(TEXT("LegJoint%d_%s"), leg, ANSI_TO_TCHAR(LegSegNames[s]));

            UStaticMeshComponent* Seg = CreateDefaultSubobject<UStaticMeshComponent>(*MeshName);
            Seg->SetStaticMesh(SphereMesh);
            Seg->SetWorldScale3D(FVector(0.008f, 0.008f, LegSegLengths[s] * 0.01f));
            Seg->SetupAttachment(ThoraxMesh);
            const FVector SegOffset = PrevOffset + FVector(0.f, 0.f, -LegSegLengths[s] * 0.5f);
            Seg->SetRelativeLocation(SegOffset);
            Seg->SetSimulatePhysics(true);
            LegSegmentMeshes.Add(Seg);

            UPhysicsConstraintComponent* Joint = CreateDefaultSubobject<UPhysicsConstraintComponent>(*JointName);
            Joint->SetupAttachment(ThoraxMesh);
            Joint->SetRelativeLocation(PrevOffset);
            Joint->SetAngularSwing1Limit(EAngularConstraintMotion::ACM_Limited, 60.f);
            Joint->SetAngularSwing2Limit(EAngularConstraintMotion::ACM_Limited, 60.f);
            Joint->SetAngularTwistLimit(EAngularConstraintMotion::ACM_Limited, 30.f);
            Joint->SetLinearXLimit(ELinearConstraintMotion::LCM_Locked, 0.f);
            Joint->SetLinearYLimit(ELinearConstraintMotion::LCM_Locked, 0.f);
            Joint->SetLinearZLimit(ELinearConstraintMotion::LCM_Locked, 0.f);
            Joint->SetAngularDriveMode(EAngularDriveMode::TwistAndSwing);
            Joint->SetAngularOrientationDrive(true, true);
            Joint->SetAngularDriveParams(50.f, 5.f, 0.f);
            LegJoints.Add(Joint);

            // Bind hit for last tarsus segment (foot)
            if (s == UNmDrosophilaComponent::LegSegments - 1)
            {
                Seg->OnComponentHit.AddDynamic(this, &ANmDrosophilaActor::OnLegHit);
            }

            PrevSeg   = Seg;
            PrevOffset = SegOffset + FVector(0.f, 0.f, -LegSegLengths[s] * 0.5f);
        }
    }

    // --- Wings (2 flat planes) ---
    const FVector WingOffsets[2] = {FVector(0.f, 2.5f, 1.f), FVector(0.f, -2.5f, 1.f)};
    for (int32 w = 0; w < UNmDrosophilaComponent::NumWings; ++w)
    {
        UStaticMeshComponent* Wing = CreateDefaultSubobject<UStaticMeshComponent>(
            *FString::Printf(TEXT("Wing_%d"), w));
        Wing->SetStaticMesh(PlaneMesh);
        Wing->SetWorldScale3D(FVector(0.04f, 0.02f, 0.001f));
        Wing->SetupAttachment(ThoraxMesh);
        Wing->SetRelativeLocation(WingOffsets[w]);
        Wing->SetSimulatePhysics(false); // Wings are kinematic for simplicity
        WingMeshes.Add(Wing);

        // Two hinge constraints per wing
        for (int32 hinge = 0; hinge < UNmDrosophilaComponent::WingJoints; ++hinge)
        {
            UPhysicsConstraintComponent* WingJoint = CreateDefaultSubobject<UPhysicsConstraintComponent>(
                *FString::Printf(TEXT("WingJoint_%d_%d"), w, hinge));
            WingJoint->SetupAttachment(ThoraxMesh);
            WingJoint->SetRelativeLocation(WingOffsets[w]);
            WingJoint->SetAngularSwing1Limit(EAngularConstraintMotion::ACM_Limited, 90.f);
            WingJoint->SetAngularSwing2Limit(EAngularConstraintMotion::ACM_Locked, 0.f);
            WingJoint->SetAngularTwistLimit(EAngularConstraintMotion::ACM_Locked, 0.f);
            WingJoint->SetLinearXLimit(ELinearConstraintMotion::LCM_Locked, 0.f);
            WingJoint->SetLinearYLimit(ELinearConstraintMotion::LCM_Locked, 0.f);
            WingJoint->SetLinearZLimit(ELinearConstraintMotion::LCM_Locked, 0.f);
            WingJointComps.Add(WingJoint);
        }
    }

    // --- Eye scene captures ---
    USceneCaptureComponent2D* EyeL = CreateDefaultSubobject<USceneCaptureComponent2D>(TEXT("EyeLeft"));
    EyeL->SetupAttachment(HeadMesh);
    EyeL->SetRelativeLocation(FVector(0.5f, 0.3f, 0.f));
    EyeL->SetRelativeRotation(FRotator(0.f, 30.f, 0.f));
    EyeL->CaptureSource = ESceneCaptureSource::SCS_FinalColorLDR;
    EyeL->bCaptureEveryFrame = false;
    EyeL->bCaptureOnMovement = false;

    USceneCaptureComponent2D* EyeR = CreateDefaultSubobject<USceneCaptureComponent2D>(TEXT("EyeRight"));
    EyeR->SetupAttachment(HeadMesh);
    EyeR->SetRelativeLocation(FVector(0.5f, -0.3f, 0.f));
    EyeR->SetRelativeRotation(FRotator(0.f, -30.f, 0.f));
    EyeR->CaptureSource = ESceneCaptureSource::SCS_FinalColorLDR;
    EyeR->bCaptureEveryFrame = false;
    EyeR->bCaptureOnMovement = false;

    // --- Brain component ---
    DrosophilaComponent = CreateDefaultSubobject<UNmDrosophilaComponent>(TEXT("DrosophilaComponent"));
    DrosophilaComponent->Thorax          = ThoraxMesh;
    DrosophilaComponent->Head            = HeadMesh;
    DrosophilaComponent->LegSegmentMeshes = LegSegmentMeshes;
    DrosophilaComponent->WingMeshes       = WingMeshes;
    DrosophilaComponent->LegJoints        = LegJoints;
    DrosophilaComponent->WingJointComps   = WingJointComps;
    DrosophilaComponent->EyeLeft          = EyeL;
    DrosophilaComponent->EyeRight         = EyeR;
}

void ANmDrosophilaActor::BeginPlay()
{
    // Force absolute dimensions, mass overrides, and proper subobject attachments
    // at BeginPlay to guarantee they completely bypass/override any stale Blueprint serialization data.
    if (ThoraxMesh)
    {
        ThoraxMesh->SetMassOverrideInKg(NAME_None, 1.200f, true);
        ThoraxMesh->SetSimulatePhysics(true);
        ThoraxMesh->SetLinearDamping(3.0f);
        ThoraxMesh->SetAngularDamping(4.0f);
    }

    const FVector RootScale = ThoraxMesh ? ThoraxMesh->GetRelativeScale3D() : FVector(0.6f, 0.45f, 0.9f);
    const FVector SafeRootScale = FVector(
        RootScale.X < 0.001f ? 1.0f : RootScale.X,
        RootScale.Y < 0.001f ? 1.0f : RootScale.Y,
        RootScale.Z < 0.001f ? 1.0f : RootScale.Z
    );

    if (ThoraxMesh && AbdomenMesh)
    {
        AbdomenMesh->AttachToComponent(ThoraxMesh, FAttachmentTransformRules::KeepRelativeTransform);
        AbdomenMesh->SetRelativeLocation(FVector(-1.2f, 0.f, -0.2f));
        AbdomenMesh->SetRelativeScale3D(FVector(0.016f, 0.014f, 0.032f) / SafeRootScale);
        AbdomenMesh->SetSimulatePhysics(false);
        AbdomenMesh->SetCollisionEnabled(ECollisionEnabled::NoCollision);
    }

    if (HeadMesh && EyeLeftMesh)
    {
        EyeLeftMesh->AttachToComponent(HeadMesh, FAttachmentTransformRules::KeepRelativeTransform);
        EyeLeftMesh->SetRelativeLocation(FVector(1.0f, 0.8f, 0.5f));
        EyeLeftMesh->SetRelativeRotation(FRotator(0.f, -45.f, 0.f));
        EyeLeftMesh->SetRelativeScale3D(FVector(0.012f, 0.012f, 0.006f) / (HeadMesh->GetRelativeScale3D()));
        EyeLeftMesh->SetSimulatePhysics(false);
        EyeLeftMesh->SetCollisionEnabled(ECollisionEnabled::NoCollision);
    }

    if (HeadMesh && EyeRightMesh)
    {
        EyeRightMesh->AttachToComponent(HeadMesh, FAttachmentTransformRules::KeepRelativeTransform);
        EyeRightMesh->SetRelativeLocation(FVector(1.0f, -0.8f, 0.5f));
        EyeRightMesh->SetRelativeRotation(FRotator(0.f, 45.f, 0.f));
        EyeRightMesh->SetRelativeScale3D(FVector(0.012f, 0.012f, 0.006f) / (HeadMesh->GetRelativeScale3D()));
        EyeRightMesh->SetSimulatePhysics(false);
        EyeRightMesh->SetCollisionEnabled(ECollisionEnabled::NoCollision);
    }

    if (HeadMesh)
    {
        HeadMesh->SetMassOverrideInKg(NAME_None, 0.300f, true);
        HeadMesh->SetSimulatePhysics(false); // Head rigidly follows thorax
    }

    // Tapered masses, physics, and slender splay scales setup for all 24 leg segments
    const float LegSegLengths[] = {0.8f, 1.2f, 1.5f, 0.5f}; // coxa,femur,tibia,tarsus (cm)
    for (int32 j = 0; j < LegSegmentMeshes.Num(); ++j)
    {
        if (LegSegmentMeshes[j])
        {
            LegSegmentMeshes[j]->SetSimulatePhysics(true);
            const int32 SegIdx = j % UNmDrosophilaComponent::LegSegments;
            
            // Forcibly set slender splayed scales in BeginPlay to prevent Blueprint overlapping sphere explosions
            const float Len = LegSegLengths[SegIdx];
            LegSegmentMeshes[j]->SetWorldScale3D(FVector(0.005f, 0.005f, Len * 0.007f) * 15.0f);

            // Stable realistic segment mass (coxa=40g, femur=150g, tibia=140g, tarsus=50g)
            const float SegMass = (SegIdx == 0) ? 0.040f : ((SegIdx == 1) ? 0.150f : ((SegIdx == 2) ? 0.140f : 0.050f));
            LegSegmentMeshes[j]->SetMassOverrideInKg(NAME_None, SegMass, true);
            LegSegmentMeshes[j]->SetLinearDamping(1.0f);
            LegSegmentMeshes[j]->SetAngularDamping(1.6f);
        }
    }

    Super::BeginPlay();

    auto EnsureEyeTarget = [this](USceneCaptureComponent2D* Capture,
                                  TObjectPtr<UTextureRenderTarget2D>& Target,
                                  const TCHAR* Name)
    {
        if (!Capture)
        {
            return;
        }
        if (!Target)
        {
            Target = NewObject<UTextureRenderTarget2D>(this, Name);
            if (Target)
            {
                Target->InitAutoFormat(UNmDrosophilaComponent::RetinaWidth, UNmDrosophilaComponent::RetinaHeight);
                Target->ClearColor = FLinearColor::Black;
                Target->UpdateResourceImmediate(true);
            }
        }
        if (Target)
        {
            Capture->TextureTarget = Target;
            Capture->CaptureSource = ESceneCaptureSource::SCS_FinalColorLDR;
            Capture->bCaptureEveryFrame = false;
            Capture->bCaptureOnMovement = false;
        }
    };

    if (DrosophilaComponent)
    {
        EnsureEyeTarget(DrosophilaComponent->EyeLeft, DrosophilaComponent->EyeLeftRT, TEXT("NmDrosEyeLeftRT"));
        EnsureEyeTarget(DrosophilaComponent->EyeRight, DrosophilaComponent->EyeRightRT, TEXT("NmDrosEyeRightRT"));
        DrosophilaComponent->PrevEyeLeftLum.Init(0.f, UNmDrosophilaComponent::NumEyePixels);
        DrosophilaComponent->PrevEyeRightLum.Init(0.f, UNmDrosophilaComponent::NumEyePixels);
    }

    // Lay segments out in WORLD space to avoid initial overlaps.
    const FVector Base  = GetActorLocation();
    const FVector Fwd   = GetActorForwardVector();
    const FVector Right = GetActorRightVector();
    const FVector Up    = GetActorUpVector();

    const FVector ThoraxHalf =
        (ThoraxMesh ? ThoraxMesh->GetComponentScale() : FVector(0.6f, 0.45f, 0.9f)) * 50.f;

    // Head: kinematic (it has no joint) so it rigidly follows the thorax; in front.
    if (HeadMesh && ThoraxMesh)
    {
        HeadMesh->AttachToComponent(ThoraxMesh, FAttachmentTransformRules::KeepRelativeTransform);
        HeadMesh->SetRelativeLocation(FVector(ThoraxHalf.X + 15.f, 0.f, ThoraxHalf.Z * 0.3f));
    }

    // Legs: front/mid/hind x left/right, splayed and positioned in world space
    const float AlongX[3] = { ThoraxHalf.X * 0.6f, 0.f, -ThoraxHalf.X * 0.6f };
    for (int32 leg = 0; leg < UNmDrosophilaComponent::NumLegs; ++leg)
    {
        const float Side = (leg < 3) ? 1.f : -1.f;
        const int32 Col  = leg % 3;
        const FVector Attach = Base
            + Fwd   * AlongX[Col]
            + Right * (Side * (ThoraxHalf.Y + 15.f))
            - Up    * (ThoraxHalf.Z * 0.4f);
        float Z = Attach.Z;
        for (int32 s = 0; s < UNmDrosophilaComponent::LegSegments; ++s)
        {
            const int32 Idx = leg * UNmDrosophilaComponent::LegSegments + s;
            if (!LegSegmentMeshes.IsValidIndex(Idx) || !LegSegmentMeshes[Idx])
            {
                continue;
            }
            UStaticMeshComponent* Seg = LegSegmentMeshes[Idx];
            const float SegLen = FMath::Max(Seg->GetComponentScale().Z * 100.f, 8.f);
            Z -= SegLen * 0.5f;
            Seg->SetWorldLocation(FVector(Attach.X, Attach.Y + Side * (s * 3.f), Z));
            Z -= SegLen * 0.5f;
        }
    }

    // Detach simulating leg segments from scene hierarchy to prevent physics conflicts!
    for (int32 j = 0; j < LegSegmentMeshes.Num(); ++j)
    {
        if (LegSegmentMeshes[j])
        {
            LegSegmentMeshes[j]->DetachFromComponent(FDetachmentTransformRules::KeepWorldTransform);
        }
    }

    // Wings: on top of the thorax, kinematic (follow the body).
    const float WingSide[2] = { 1.f, -1.f };
    for (int32 w = 0; w < WingMeshes.Num(); ++w)
    {
        if (!WingMeshes[w] || !ThoraxMesh)
        {
            continue;
        }
        WingMeshes[w]->AttachToComponent(ThoraxMesh, FAttachmentTransformRules::KeepRelativeTransform);
        WingMeshes[w]->SetRelativeLocation(FVector(0.f, WingSide[w] * ThoraxHalf.Y * 0.9f, ThoraxHalf.Z + 5.f));
        WingMeshes[w]->SetSimulatePhysics(false);
    }

    // Wire every leg joint at the world midpoint of the two bodies it connects.
    for (int32 j = 0; j < LegJoints.Num(); ++j)
    {
        if (!LegJoints[j] || !LegSegmentMeshes.IsValidIndex(j) || !LegSegmentMeshes[j])
        {
            continue;
        }
        const bool bRoot = (j % UNmDrosophilaComponent::LegSegments == 0);
        UStaticMeshComponent* Parent = bRoot
            ? ToRawPtr(ThoraxMesh)
            : (LegSegmentMeshes.IsValidIndex(j - 1) ? ToRawPtr(LegSegmentMeshes[j - 1]) : nullptr);
        UStaticMeshComponent* Child = ToRawPtr(LegSegmentMeshes[j]);
        if (Parent && Child)
        {
            LegJoints[j]->SetWorldLocation(
                0.5f * (Parent->GetComponentLocation() + Child->GetComponentLocation()));
            LegJoints[j]->OverrideComponent1 = Parent;
            LegJoints[j]->OverrideComponent2 = Child;
            LegJoints[j]->InitComponentConstraint();
            LegJoints[j]->SetDisableCollision(true);
        }
    }
}

void ANmDrosophilaActor::OnLegHit(UPrimitiveComponent* HitComp, AActor* /*OtherActor*/,
                                   UPrimitiveComponent* /*OtherComp*/, FVector /*NormalImpulse*/,
                                   const FHitResult& /*Hit*/)
{
    // Determine which leg this tarsus belongs to
    // Tarsus segments are at indices 3, 7, 11, 15, 19, 23 (every 4th starting at 3)
    for (int32 leg = 0; leg < UNmDrosophilaComponent::NumLegs; ++leg)
    {
        const int32 TarsusIdx = leg * UNmDrosophilaComponent::LegSegments
                                + (UNmDrosophilaComponent::LegSegments - 1);
        if (LegSegmentMeshes.IsValidIndex(TarsusIdx) && LegSegmentMeshes[TarsusIdx] == HitComp)
        {
            DrosophilaComponent->FootContacts[leg] = true;
            break;
        }
    }
}
