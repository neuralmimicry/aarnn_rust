// Copyright NeuralMimicry. All Rights Reserved.

#include "Robots/NmZebrafishActor.h"

#include "Components/StaticMeshComponent.h"
#include "Components/SceneCaptureComponent2D.h"
#include "Engine/TextureRenderTarget2D.h"
#include "PhysicsEngine/PhysicsConstraintComponent.h"
#include "Engine/World.h"
#include "Engine/OverlapResult.h"
#include "UObject/ConstructorHelpers.h"

// ============================================================================
// UNmZebrafishComponent
// ============================================================================

UNmZebrafishComponent::UNmZebrafishComponent()
{
    BrainId = TEXT("zebrafish");
    PrimaryComponentTick.bCanEverTick = true;
}

// ----------------------------------------------------------------------------
// SampleSegmentLateralProximity — overlap sphere at one side of a segment
// ----------------------------------------------------------------------------

float UNmZebrafishComponent::SampleSegmentLateralProximity(int32 SegIdx, bool bLeftSide) const
{
    if (!SegmentMeshes.IsValidIndex(SegIdx) || !SegmentMeshes[SegIdx])
    {
        return 0.f;
    }

    UWorld* World = GetWorld();
    if (!World)
    {
        return 0.f;
    }

    UStaticMeshComponent* Seg = SegmentMeshes[SegIdx];
    const FVector SegLoc     = Seg->GetComponentLocation();
    const FVector RightVec   = Seg->GetRightVector();
    const FVector SideOffset = (bLeftSide ? RightVec : -RightVec) * (LateralLineRadiusCm * 0.5f);
    const FVector TestCenter = SegLoc + SideOffset;

    TArray<FOverlapResult> Overlaps;
    const FCollisionShape Sphere = FCollisionShape::MakeSphere(LateralLineRadiusCm);
    const bool bHit = World->OverlapMultiByChannel(
        Overlaps, TestCenter, FQuat::Identity, ECC_Visibility, Sphere);

    return bHit ? 1.f : 0.f;
}

// ----------------------------------------------------------------------------
// ApplyBuoyancy — called from actor Tick
// ----------------------------------------------------------------------------

void UNmZebrafishComponent::ApplyBuoyancy()
{
    // Acceleration-based buoyancy that balances gravity for near-neutral float,
    // independent of body mass/scale (a raw force launched the scaled fish).
    const float GravZ = GetWorld() ? GetWorld()->GetGravityZ() : -980.f; // negative
    const float UpAccel = -GravZ;                                        // cancels gravity
    for (UStaticMeshComponent* Seg : SegmentMeshes)
    {
        if (!Seg || !Seg->IsSimulatingPhysics())
        {
            continue;
        }
        const float SegZ = Seg->GetComponentLocation().Z;
        if (SegZ < WaterSurfaceZ)
        {
            const float Depth = WaterSurfaceZ - SegZ;
            const float Frac = FMath::Min(Depth / 30.f, 1.f);
            // ~1.05 g when fully submerged → drifts gently up, then settles just
            // under the surface where buoyancy (∝ depth) balances gravity.
            Seg->AddForce(FVector(0.f, 0.f, UpAccel * 1.05f * Frac), NAME_None, true);
        }
    }
}

// ----------------------------------------------------------------------------
// CollectSensors — 32 channels
// [0..15]  16 lateral line (8 segments × 2 sides)
// [16..23] 8 optical flow (4 quadrant lum deltas × 2 channels each)
// [24..27] 4 tail joint angles (tail segments 7-10)
// [28..29] 2 swim bladder (Z relative to water surface, L+R same)
// [30..31] 2 vestibular (root pitch + roll angular velocity)
// ----------------------------------------------------------------------------

void UNmZebrafishComponent::CollectSensors(TArray<float>& OutSensors)
{
    OutSensors.SetNumZeroed(NumSensors);

    AActor* Owner = GetOwner();
    UWorld* World = GetWorld();
    if (!Owner || !World || SegmentMeshes.Num() == 0 || !SegmentMeshes[0])
    {
        return;
    }

    UStaticMeshComponent* RootSeg = SegmentMeshes[0];
    const FVector HeadLoc  = RootSeg->GetComponentLocation();
    const FVector HeadFwd  = RootSeg->GetForwardVector();
    const FVector HeadRight = RootSeg->GetRightVector();
    const FVector HeadUp    = RootSeg->GetUpVector();

    // --- Lateral Line [0..15]: sample 8 segments, L+R ---
    const int32 LLStep = FMath::Max(1, NumSegments / 8);
    for (int32 i = 0; i < 8; ++i)
    {
        const int32 SegIdx = FMath::Min(i * LLStep, SegmentMeshes.Num() - 1);
        OutSensors[i * 2 + 0] = SampleSegmentLateralProximity(SegIdx, true);   // left
        OutSensors[i * 2 + 1] = SampleSegmentLateralProximity(SegIdx, false);  // right
    }

    // --- Eyes [16..19]: left/right luminance & gradients (ON/OFF) ---
    float LeftLum = 0.5f;
    float RightLum = 0.5f;
    if (EyeCamera && EyeCamera->TextureTarget)
    {
        FRenderTarget* RT = EyeCamera->TextureTarget->GameThread_GetRenderTargetResource();
        if (RT)
        {
            TArray<FColor> Pixels;
            if (RT->ReadPixels(Pixels) && Pixels.Num() > 0)
            {
                const int32 W = EyeCamera->TextureTarget->SizeX;
                const int32 H = EyeCamera->TextureTarget->SizeY;
                const int32 HW = W / 2;

                auto RegionMean = [&](int32 x0, int32 x1) -> float
                {
                    float Sum = 0.f;
                    int32 Cnt = 0;
                    for (int32 y = 0; y < H; ++y)
                    {
                        for (int32 x = x0; x < x1 && x < W; ++x)
                        {
                            const FColor& P = Pixels[y * W + x];
                            Sum += (P.R + P.G + P.B) / (3.f * 255.f);
                            ++Cnt;
                        }
                    }
                    return Cnt > 0 ? Sum / Cnt : 0.f;
                };

                LeftLum = RegionMean(0, HW);  // Left half of capture
                RightLum = RegionMean(HW, W); // Right half of capture
            }
        }
    }

    // Left eye [16..17]: luminance & change gradient
    const float LeftDelta = LeftLum - PrevQuadLuminance[0];
    OutSensors[16] = LeftLum;
    OutSensors[17] = FMath::Clamp(LeftDelta * 5.0f, -1.f, 1.f) * 0.5f + 0.5f;
    PrevQuadLuminance[0] = LeftLum;

    // Right eye [18..19]: luminance & change gradient
    const float RightDelta = RightLum - PrevQuadLuminance[1];
    OutSensors[18] = RightLum;
    OutSensors[19] = FMath::Clamp(RightDelta * 5.0f, -1.f, 1.f) * 0.5f + 0.5f;
    PrevQuadLuminance[1] = RightLum;

    // --- Olfactory / Warmth / Temperature [20..23] ---
    // Warmth/cold changes dynamically with depth (water is warmer near surface, colder at depth)
    const float SubmergedDepth = FMath::Max(0.f, WaterSurfaceZ - HeadLoc.Z);
    const float NormalizedDepth = FMath::Clamp(SubmergedDepth / 150.f, 0.f, 1.f); // over 1.5 meters depth

    // Olfactory Left/Right: horizontal warm currents/gradients based on heading
    const float HorizontalFactor = 0.5f + 0.5f * HeadFwd.X;
    OutSensors[20] = FMath::Clamp((1.0f - NormalizedDepth) * HorizontalFactor, 0.f, 1.f); // Olfactory L
    OutSensors[21] = FMath::Clamp((1.0f - NormalizedDepth) * (1.0f - HorizontalFactor), 0.f, 1.f); // Olfactory R

    // Warmth (Snout / Front Olfactory): Warmest near the surface
    OutSensors[22] = FMath::Clamp(1.0f - NormalizedDepth, 0.f, 1.f);

    // Cold (Tail / Rear Olfactory): Coldest at depth
    OutSensors[23] = NormalizedDepth;

    // --- Flow [24..25]: anterior & posterior flow based on velocity ---
    const FVector Velocity = Owner->GetVelocity();
    const float FwdSpeed = FVector::DotProduct(Velocity, HeadFwd);
    OutSensors[24] = FMath::Clamp(0.5f + FwdSpeed / 100.f, 0.f, 1.f); // Flow Anterior (swimming forward increases head flow)
    OutSensors[25] = FMath::Clamp(0.5f - FwdSpeed / 100.f, 0.f, 1.f); // Flow Posterior

    // --- Pressure / Depth & Density [26..27] ---
    // pressure_depth: senses water pressure and density changes at depth (increases linearly with depth)
    OutSensors[26] = NormalizedDepth;

    // pressure_pitch: pressure differential due to pitch tilt
    OutSensors[27] = FMath::Clamp(0.5f + HeadFwd.Z, 0.f, 1.f);

    // --- Accelerometer / Equilibrium (Which Way is Up) [28..29] ---
    // local gravity vector tells the fish exactly which way is up relative to its body axes
    const FVector LocalUp = Owner->GetActorRotation().UnrotateVector(FVector(0.f, 0.f, 1.f)); // local up vector
    OutSensors[28] = FMath::Clamp(0.5f + LocalUp.Z * 0.5f, 0.f, 1.f); // Senses gravitational equilibrium: vertical up alignment (1.0 = perfectly upright)
    OutSensors[29] = FMath::Clamp(0.5f + LocalUp.Y * 0.5f, 0.f, 1.f); // Senses lateral roll/tilt equilibrium

    // --- Gyro / Rotation rate [30..31] ---
    const FVector AngVel = RootSeg->GetPhysicsAngularVelocityInDegrees();
    constexpr float MaxAngVelDeg = 180.f;
    OutSensors[30] = FMath::Clamp(AngVel.Y / MaxAngVelDeg, -1.f, 1.f) * 0.5f + 0.5f; // Pitch rate
    OutSensors[31] = FMath::Clamp(AngVel.X / MaxAngVelDeg, -1.f, 1.f) * 0.5f + 0.5f; // Roll rate
}

void UNmZebrafishComponent::ApplyActuators(const TArray<float>& Actuators)
{
    if (Actuators.Num() < NumActuators)
    {
        return;
    }

    constexpr float MaxDorsalDeg   = 15.f;
    constexpr float MaxLateralDeg  = 30.f;

    for (int32 seg = 0; seg < NumSegments - 1 && seg < SegmentJoints.Num(); ++seg)
    {
        UPhysicsConstraintComponent* Joint = SegmentJoints[seg];
        if (!Joint)
        {
            continue;
        }

        const int32 Base    = seg * 2;
        const float Dorsal  = FMath::Clamp(Actuators[Base + 0], -1.f, 1.f);
        const float Ventral = FMath::Clamp(Actuators[Base + 1], -1.f, 1.f);

        // Dorsal-ventral → Swing2 (dorsal = positive, ventral = negative)
        const float DV  = (Dorsal - Ventral) * MaxDorsalDeg;
        // Lateral undulation: average of both channels drives Swing1
        const float Lat = (Dorsal + Ventral) * 0.5f * MaxLateralDeg;

        Joint->SetAngularOrientationTarget(FRotator(Lat, 0.f, DV));
    }

    // Fin stubs [22..31]: set drive target on hypothetical fin constraints
    // (no-op if no fin joints configured)
}

void UNmZebrafishComponent::GetSensorNames(TArray<FString>& OutNames) const
{
    OutNames.Reset(NumSensors);
    for (int32 i = 0; i < 8; ++i)
    {
        OutNames.Add(FString::Printf(TEXT("ll_seg%d_left"),  i));
        OutNames.Add(FString::Printf(TEXT("ll_seg%d_right"), i));
    }
    for (int32 q = 0; q < 4; ++q)
    {
        OutNames.Add(FString::Printf(TEXT("optflow_q%d_on"),  q));
        OutNames.Add(FString::Printf(TEXT("optflow_q%d_off"), q));
    }
    for (int32 t = 0; t < NumTailAngles; ++t)
    {
        OutNames.Add(FString::Printf(TEXT("tail_angle_%d"), t));
    }
    OutNames.Add(TEXT("swim_bladder_l"));
    OutNames.Add(TEXT("swim_bladder_r"));
    OutNames.Add(TEXT("vestibular_pitch"));
    OutNames.Add(TEXT("vestibular_roll"));
}

void UNmZebrafishComponent::GetActuatorNames(TArray<FString>& OutNames) const
{
    OutNames.Reset(NumActuators);
    for (int32 seg = 0; seg < NumSegments; ++seg)
    {
        OutNames.Add(FString::Printf(TEXT("seg%02d_dorsal"),  seg));
        OutNames.Add(FString::Printf(TEXT("seg%02d_ventral"), seg));
    }
    for (int32 f = 0; f < NumFinActuators; ++f)
    {
        OutNames.Add(FString::Printf(TEXT("fin_drive_%02d"), f));
    }
}

// ============================================================================
// ANmZebrafishActor
// ============================================================================

ANmZebrafishActor::ANmZebrafishActor()
{
    PrimaryActorTick.bCanEverTick = true;

    static ConstructorHelpers::FObjectFinder<UStaticMesh> SphereMeshFinder(
        TEXT("/Engine/BasicShapes/Sphere.Sphere"));
    UStaticMesh* SphereMesh = SphereMeshFinder.Succeeded() ? SphereMeshFinder.Object : nullptr;

    // Segment dimensions: body segments slightly larger than tail segments
    // Each capsule approximated as scaled sphere
    // Body: radius ~0.2 cm, length ~0.5 cm; tail: radius ~0.1 cm, length ~0.4 cm

    UStaticMeshComponent* RootSeg = CreateDefaultSubobject<UStaticMeshComponent>(TEXT("Segment_00"));
    RootSeg->SetStaticMesh(SphereMesh);
    RootSeg->SetWorldScale3D(FVector(0.02f, 0.02f, 0.05f));
    RootSeg->SetSimulatePhysics(true);
    SetRootComponent(RootSeg);
    SegmentMeshes.Add(RootSeg);

    for (int32 i = 1; i < UNmZebrafishComponent::NumSegments; ++i)
    {
        const bool bTail   = (i >= UNmZebrafishComponent::NumBodySegments);
        const float Radius = bTail ? 0.01f : 0.02f;
        const float Length = bTail ? 0.04f : 0.05f;

        UStaticMeshComponent* Seg = CreateDefaultSubobject<UStaticMeshComponent>(
            *FString::Printf(TEXT("Segment_%02d"), i));
        Seg->SetStaticMesh(SphereMesh);
        Seg->SetWorldScale3D(FVector(Radius, Radius, Length));
        Seg->SetSimulatePhysics(true);
        Seg->SetupAttachment(RootSeg);
        SegmentMeshes.Add(Seg);

        UPhysicsConstraintComponent* Joint = CreateDefaultSubobject<UPhysicsConstraintComponent>(
            *FString::Printf(TEXT("SegJoint_%02d"), i));
        Joint->SetupAttachment(RootSeg);

        // Body joints: lateral ±30°, dorsal ±15°
        // Tail joints: looser lateral
        const float LateralLimitDeg = bTail ? 35.f : 30.f;
        const float DorsalLimitDeg  = 15.f;

        Joint->SetAngularSwing1Limit(EAngularConstraintMotion::ACM_Limited, LateralLimitDeg);
        Joint->SetAngularSwing2Limit(EAngularConstraintMotion::ACM_Limited, DorsalLimitDeg);
        Joint->SetAngularTwistLimit(EAngularConstraintMotion::ACM_Free, 0.f);
        Joint->SetLinearXLimit(ELinearConstraintMotion::LCM_Locked, 0.f);
        Joint->SetLinearYLimit(ELinearConstraintMotion::LCM_Locked, 0.f);
        Joint->SetLinearZLimit(ELinearConstraintMotion::LCM_Locked, 0.f);
        Joint->SetAngularDriveMode(EAngularDriveMode::TwistAndSwing);
        Joint->SetAngularOrientationDrive(true, true);
        Joint->SetAngularDriveParams(80.f, 8.f, 0.f);
        SegmentJoints.Add(Joint);
    }

    // Head eye camera
    USceneCaptureComponent2D* EyeCam = CreateDefaultSubobject<USceneCaptureComponent2D>(TEXT("EyeCamera"));
    EyeCam->SetupAttachment(RootSeg);
    EyeCam->SetRelativeLocation(FVector(2.f, 0.f, 0.f));
    EyeCam->CaptureSource = ESceneCaptureSource::SCS_FinalColorLDR;
    EyeCam->bCaptureEveryFrame = true;

    static ConstructorHelpers::FObjectFinder<UStaticMesh> CubeMeshFinder(
        TEXT("/Engine/BasicShapes/Cube.Cube"));
    UStaticMesh* CubeMesh = CubeMeshFinder.Succeeded() ? CubeMeshFinder.Object : nullptr;

    // --- Caudal Fin (Tail Fin) on Segment 10 ---
    if (SegmentMeshes.IsValidIndex(10) && SegmentMeshes[10])
    {
        CaudalFin = CreateDefaultSubobject<UStaticMeshComponent>(TEXT("CaudalFin"));
        CaudalFin->SetStaticMesh(CubeMesh);
        CaudalFin->SetWorldScale3D(FVector(0.002f, 0.05f, 0.04f));
        CaudalFin->SetupAttachment(SegmentMeshes[10]);
        CaudalFin->SetRelativeLocation(FVector(0.f, 0.f, -2.5f));
        CaudalFin->SetSimulatePhysics(false);
        CaudalFin->SetCollisionEnabled(ECollisionEnabled::NoCollision);
    }

    // --- Dorsal Fin (Top Fin) on Segment 5 ---
    if (SegmentMeshes.IsValidIndex(5) && SegmentMeshes[5])
    {
        DorsalFin = CreateDefaultSubobject<UStaticMeshComponent>(TEXT("DorsalFin"));
        DorsalFin->SetStaticMesh(CubeMesh);
        DorsalFin->SetWorldScale3D(FVector(0.002f, 0.04f, 0.03f));
        DorsalFin->SetupAttachment(SegmentMeshes[5]);
        DorsalFin->SetRelativeLocation(FVector(0.f, 1.5f, 0.f));
        DorsalFin->SetSimulatePhysics(false);
        DorsalFin->SetCollisionEnabled(ECollisionEnabled::NoCollision);
    }

    // --- Anal Fin (Bottom Fin) on Segment 8 ---
    if (SegmentMeshes.IsValidIndex(8) && SegmentMeshes[8])
    {
        AnalFin = CreateDefaultSubobject<UStaticMeshComponent>(TEXT("AnalFin"));
        AnalFin->SetStaticMesh(CubeMesh);
        AnalFin->SetWorldScale3D(FVector(0.002f, 0.03f, 0.025f));
        AnalFin->SetupAttachment(SegmentMeshes[8]);
        AnalFin->SetRelativeLocation(FVector(0.f, -1.2f, 0.f));
        AnalFin->SetSimulatePhysics(false);
        AnalFin->SetCollisionEnabled(ECollisionEnabled::NoCollision);
    }

    // --- Pectoral Fins (Left & Right Side Fins) on Segment 1 ---
    if (SegmentMeshes.IsValidIndex(1) && SegmentMeshes[1])
    {
        PectoralFinLeft = CreateDefaultSubobject<UStaticMeshComponent>(TEXT("PectoralFinLeft"));
        PectoralFinLeft->SetStaticMesh(CubeMesh);
        PectoralFinLeft->SetWorldScale3D(FVector(0.03f, 0.002f, 0.02f));
        PectoralFinLeft->SetupAttachment(SegmentMeshes[1]);
        PectoralFinLeft->SetRelativeLocation(FVector(-1.5f, -0.3f, 0.f));
        PectoralFinLeft->SetRelativeRotation(FRotator(0.f, -30.f, -15.f));
        PectoralFinLeft->SetSimulatePhysics(false);
        PectoralFinLeft->SetCollisionEnabled(ECollisionEnabled::NoCollision);

        PectoralFinRight = CreateDefaultSubobject<UStaticMeshComponent>(TEXT("PectoralFinRight"));
        PectoralFinRight->SetStaticMesh(CubeMesh);
        PectoralFinRight->SetWorldScale3D(FVector(0.03f, 0.002f, 0.02f));
        PectoralFinRight->SetupAttachment(SegmentMeshes[1]);
        PectoralFinRight->SetRelativeLocation(FVector(1.5f, -0.3f, 0.f));
        PectoralFinRight->SetRelativeRotation(FRotator(0.f, 30.f, 15.f));
        PectoralFinRight->SetSimulatePhysics(false);
        PectoralFinRight->SetCollisionEnabled(ECollisionEnabled::NoCollision);
    }

    // --- Big Fish Eyes on Segment 0 (Head) ---
    if (RootSeg)
    {
        EyeLeft = CreateDefaultSubobject<UStaticMeshComponent>(TEXT("EyeLeft"));
        EyeLeft->SetStaticMesh(SphereMesh);
        EyeLeft->SetWorldScale3D(FVector(0.012f, 0.012f, 0.005f));
        EyeLeft->SetupAttachment(RootSeg);
        EyeLeft->SetRelativeLocation(FVector(-1.2f, 0.2f, 1.2f));
        EyeLeft->SetRelativeRotation(FRotator(0.f, -90.f, 0.f));
        EyeLeft->SetSimulatePhysics(false);
        EyeLeft->SetCollisionEnabled(ECollisionEnabled::NoCollision);

        EyeRight = CreateDefaultSubobject<UStaticMeshComponent>(TEXT("EyeRight"));
        EyeRight->SetStaticMesh(SphereMesh);
        EyeRight->SetWorldScale3D(FVector(0.012f, 0.012f, 0.005f));
        EyeRight->SetupAttachment(RootSeg);
        EyeRight->SetRelativeLocation(FVector(1.2f, 0.2f, 1.2f));
        EyeRight->SetRelativeRotation(FRotator(0.f, 90.f, 0.f));
        EyeRight->SetSimulatePhysics(false);
        EyeRight->SetCollisionEnabled(ECollisionEnabled::NoCollision);
    }

    // Brain component
    ZebrafishComponent = CreateDefaultSubobject<UNmZebrafishComponent>(TEXT("ZebrafishComponent"));
    ZebrafishComponent->SegmentMeshes  = SegmentMeshes;
    ZebrafishComponent->SegmentJoints  = SegmentJoints;
    ZebrafishComponent->EyeCamera      = EyeCam;
    ZebrafishComponent->WaterSurfaceZ  = WaterSurfaceZ;
}

void ANmZebrafishActor::BeginPlay()
{
    // Force absolute attachments and dimensions at BeginPlay to bypass stale Blueprint serialization
    auto SafeRelativeScale = [](UStaticMeshComponent* Comp) -> FVector {
        FVector Scale = Comp ? Comp->GetRelativeScale3D() : FVector(1.f);
        if (Scale.X < 0.001f) Scale.X = 1.0f;
        if (Scale.Y < 0.001f) Scale.Y = 1.0f;
        if (Scale.Z < 0.001f) Scale.Z = 1.0f;
        return Scale;
    };

    // Apply smooth body-to-tail tapering and stable mass overrides (eliminates joint drooping/stretching)
    for (int32 i = 0; i < SegmentMeshes.Num(); ++i)
    {
        if (SegmentMeshes[i])
        {
            const float frac = (float)i / 10.f;
            const float taper_r = 1.0f - 0.55f * frac; // smooth body-to-tail width taper
            const float taper_l = 1.0f - 0.45f * frac; // smooth body-to-tail length taper
            
            // Set correct tapered dimensions scaled up by 12.0f
            const FVector TargetScale = FVector(0.02f * taper_r, 0.02f * taper_r, 0.05f * taper_l) * 12.0f;
            SegmentMeshes[i]->SetWorldScale3D(TargetScale);

            // Overrides default huge mass with realistic physical masses (tapers from 1.2 kg down to 0.24 kg)
            const float SegMass = FMath::Max(0.24f, 1.200f * (1.0f - 0.80f * frac));
            SegmentMeshes[i]->SetMassOverrideInKg(NAME_None, SegMass, true);
        }
    }

    if (SegmentMeshes.IsValidIndex(10) && SegmentMeshes[10] && CaudalFin)
    {
        CaudalFin->AttachToComponent(SegmentMeshes[10], FAttachmentTransformRules::KeepRelativeTransform);
        CaudalFin->SetRelativeLocation(FVector(0.f, 0.f, -2.5f));
        CaudalFin->SetRelativeScale3D(FVector(0.002f, 0.05f, 0.04f) / SafeRelativeScale(SegmentMeshes[10]));
        CaudalFin->SetSimulatePhysics(false);
        CaudalFin->SetCollisionEnabled(ECollisionEnabled::NoCollision);
    }

    if (SegmentMeshes.IsValidIndex(5) && SegmentMeshes[5] && DorsalFin)
    {
        DorsalFin->AttachToComponent(SegmentMeshes[5], FAttachmentTransformRules::KeepRelativeTransform);
        DorsalFin->SetRelativeLocation(FVector(0.f, 1.5f, 0.f));
        DorsalFin->SetRelativeScale3D(FVector(0.002f, 0.04f, 0.03f) / SafeRelativeScale(SegmentMeshes[5]));
        DorsalFin->SetSimulatePhysics(false);
        DorsalFin->SetCollisionEnabled(ECollisionEnabled::NoCollision);
    }

    if (SegmentMeshes.IsValidIndex(8) && SegmentMeshes[8] && AnalFin)
    {
        AnalFin->AttachToComponent(SegmentMeshes[8], FAttachmentTransformRules::KeepRelativeTransform);
        AnalFin->SetRelativeLocation(FVector(0.f, -1.2f, 0.f));
        AnalFin->SetRelativeScale3D(FVector(0.002f, 0.03f, 0.025f) / SafeRelativeScale(SegmentMeshes[8]));
        AnalFin->SetSimulatePhysics(false);
        AnalFin->SetCollisionEnabled(ECollisionEnabled::NoCollision);
    }

    if (SegmentMeshes.IsValidIndex(1) && SegmentMeshes[1] && PectoralFinLeft)
    {
        PectoralFinLeft->AttachToComponent(SegmentMeshes[1], FAttachmentTransformRules::KeepRelativeTransform);
        PectoralFinLeft->SetRelativeLocation(FVector(-1.5f, -0.3f, 0.f));
        PectoralFinLeft->SetRelativeRotation(FRotator(0.f, -30.f, -15.f));
        PectoralFinLeft->SetRelativeScale3D(FVector(0.03f, 0.002f, 0.02f) / SafeRelativeScale(SegmentMeshes[1]));
        PectoralFinLeft->SetSimulatePhysics(false);
        PectoralFinLeft->SetCollisionEnabled(ECollisionEnabled::NoCollision);
    }

    if (SegmentMeshes.IsValidIndex(1) && SegmentMeshes[1] && PectoralFinRight)
    {
        PectoralFinRight->AttachToComponent(SegmentMeshes[1], FAttachmentTransformRules::KeepRelativeTransform);
        PectoralFinRight->SetRelativeLocation(FVector(1.5f, -0.3f, 0.f));
        PectoralFinRight->SetRelativeRotation(FRotator(0.f, 30.f, 15.f));
        PectoralFinRight->SetRelativeScale3D(FVector(0.03f, 0.002f, 0.02f) / SafeRelativeScale(SegmentMeshes[1]));
        PectoralFinRight->SetSimulatePhysics(false);
        PectoralFinRight->SetCollisionEnabled(ECollisionEnabled::NoCollision);
    }

    if (SegmentMeshes.IsValidIndex(0) && SegmentMeshes[0] && EyeLeft)
    {
        EyeLeft->AttachToComponent(SegmentMeshes[0], FAttachmentTransformRules::KeepRelativeTransform);
        EyeLeft->SetRelativeLocation(FVector(-1.2f, 0.2f, 1.2f));
        EyeLeft->SetRelativeRotation(FRotator(0.f, -90.f, 0.f));
        EyeLeft->SetRelativeScale3D(FVector(0.012f, 0.012f, 0.005f) / SafeRelativeScale(SegmentMeshes[0]));
        EyeLeft->SetSimulatePhysics(false);
        EyeLeft->SetCollisionEnabled(ECollisionEnabled::NoCollision);
    }

    if (SegmentMeshes.IsValidIndex(0) && SegmentMeshes[0] && EyeRight)
    {
        EyeRight->AttachToComponent(SegmentMeshes[0], FAttachmentTransformRules::KeepRelativeTransform);
        EyeRight->SetRelativeLocation(FVector(1.2f, 0.2f, 1.2f));
        EyeRight->SetRelativeRotation(FRotator(0.f, 90.f, 0.f));
        EyeRight->SetRelativeScale3D(FVector(0.012f, 0.012f, 0.005f) / SafeRelativeScale(SegmentMeshes[0]));
        EyeRight->SetSimulatePhysics(false);
        EyeRight->SetCollisionEnabled(ECollisionEnabled::NoCollision);
    }

    Super::BeginPlay();

    // Lay segments out in WORLD space (relative offsets collapse under the
    // sub-1.0-scaled root). Slight overlap gives a continuous fish body.
    const FVector BaseLoc = GetActorLocation();
    const FVector Fwd = GetActorForwardVector();
    const float SegWorld = (SegmentMeshes.Num() > 0 && SegmentMeshes[0])
        ? 100.f * SegmentMeshes[0]->GetComponentScale().X : 24.f;
    const float WorldSpacing = SegWorld * 0.6f;
    for (int32 i = 0; i < SegmentMeshes.Num(); ++i)
    {
        if (SegmentMeshes[i])
        {
            SegmentMeshes[i]->SetWorldLocation(BaseLoc + Fwd * (i * WorldSpacing));
            // Water viscosity — damps motion so buoyancy/swimming dont oscillate.
            SegmentMeshes[i]->SetLinearDamping(4.0f);
            SegmentMeshes[i]->SetAngularDamping(4.0f);
        }
    }

    // Detach segments 1-10 from the Scene component hierarchy on BeginPlay.
    // This removes conflicts between Scene component attachment and Physics simulation,
    // which prevents the chain joints from stretching or breaking.
    for (int32 i = 1; i < SegmentMeshes.Num(); ++i)
    {
        if (SegmentMeshes[i])
        {
            SegmentMeshes[i]->DetachFromComponent(FDetachmentTransformRules::KeepWorldTransform);
        }
    }

    // Wire joint constraints
    for (int32 i = 0; i < SegmentJoints.Num(); ++i)
    {
        if (!SegmentJoints[i])
        {
            continue;
        }

        // Joint i connects Segment[i] → Segment[i+1]
        UStaticMeshComponent* Parent = SegmentMeshes[i];
        UStaticMeshComponent* Child  = SegmentMeshes.IsValidIndex(i + 1) ? SegmentMeshes[i + 1] : nullptr;

        if (Parent && Child)
        {
            // Position the constraint at the world midpoint of the two segments so
            // its reference frames hold them apart (a joint at the actor origin
            // collapses the body).
            SegmentJoints[i]->SetWorldLocation(
                0.5f * (Parent->GetComponentLocation() + Child->GetComponentLocation()));
            SegmentJoints[i]->OverrideComponent1 = Parent;
            SegmentJoints[i]->OverrideComponent2 = Child;
            SegmentJoints[i]->InitComponentConstraint();
        }
    }

    // Propagate water surface to component
    ZebrafishComponent->WaterSurfaceZ = WaterSurfaceZ;
}

void ANmZebrafishActor::Tick(float DeltaSeconds)
{
    Super::Tick(DeltaSeconds);

    // Apply procedural water buoyancy each frame
    if (ZebrafishComponent)
    {
        ZebrafishComponent->ApplyBuoyancy();
    }
}
