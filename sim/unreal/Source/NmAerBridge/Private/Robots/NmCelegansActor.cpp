// Copyright NeuralMimicry. All Rights Reserved.

#include "Robots/NmCelegansActor.h"
#include "NmSharedContent.h"

#include "Components/SceneComponent.h"
#include "Components/StaticMeshComponent.h"
#include "PhysicsEngine/PhysicsConstraintComponent.h"
#include "DrawDebugHelpers.h"
#include "Engine/World.h"
#include "UObject/ConstructorHelpers.h"

namespace
{
// Mirror the Webots celegans body profile so Unreal shows a recognizable worm
// rather than a loose cluster of equal-sized spheres.
constexpr float kSpineRadiusCm = 2.1f;
constexpr float kSegmentLongitudinalScale = 1.9f;
constexpr float kVisualLinkRadiusScale = 0.95f;
constexpr float kVisualLinkLengthScale = 1.08f;

float SegmentRadiusCm(int32 SegIdx)
{
    const float Center = (UNmCelegansComponent::NumSegments - 1) * 0.5f;
    const float Dist   = FMath::Abs(static_cast<float>(SegIdx) - Center) / FMath::Max(1.0f, Center);
    return kSpineRadiusCm * (1.0f - 0.34f * Dist);
}

float ContractFromOutput(float Raw, float& Trace)
{
    float Clamped = Raw;
    if (!FMath::IsFinite(Clamped))
    {
        Clamped = 0.5f;
    }
    Clamped = FMath::Clamp(Clamped, 0.0f, 1.0f);

    // Accept both binary spikes (0/1) and graded commands around neutral 0.5.
    const float GradedDrive = FMath::Max(0.0f, (Clamped - 0.5f) * 2.0f);
    const float SpikeBoost = Clamped >= 0.999f ? 1.0f : 0.0f;
    const float Drive = FMath::Max(GradedDrive, SpikeBoost);

    Trace = FMath::Clamp(0.92f * Trace + 0.62f * Drive, 0.0f, 1.0f);
    return 0.5f + 0.5f * Trace; // neutral-centered contraction command
}
} // namespace

// ============================================================================
// UNmCelegansComponent
// ============================================================================

UNmCelegansComponent::UNmCelegansComponent()
{
    BrainId = TEXT("celegans");
    PrimaryComponentTick.bCanEverTick = true;

    MdlTrace.Init(0.0f, NumSegments);
    MdrTrace.Init(0.0f, NumSegments);
    MvlTrace.Init(0.0f, NumSegments);
    MvrTrace.Init(0.0f, NumSegments);
    FilteredDvDrive.Init(0.0f, NumSegments);
    FilteredLrDrive.Init(0.0f, NumSegments);
}

// ----------------------------------------------------------------------------
// CollectSensors — 24 channels
// ----------------------------------------------------------------------------
// Canonical catalogue order: inertial [0..5], touch [6..7], light/heat [8..11],
// taste/chemical [12..18], flow [19..20], far proximity [21..23].
// These are engineering transducers, not fitted biological receptor models.
// ----------------------------------------------------------------------------

void UNmCelegansComponent::CollectSensors(TArray<float>& OutSensors)
{
    OutSensors.Init(0.f, NumSensors);
    if (SegmentMeshes.IsEmpty() || !GetOwner() || !GetWorld()) return;
    auto Root = SegmentMeshes[0];
    if (!Root) return;
    const FVector Head = Root->GetComponentLocation();
    const FVector Tail = SegmentMeshes.Last()->GetComponentLocation();
    const FVector Forward = -Root->GetForwardVector(); // this rig grows +X from head to tail
    const FVector Side = -Root->GetRightVector();
    const FVector Velocity = Root->GetPhysicsLinearVelocity();
    const FVector Accel = Root->GetComponentQuat().UnrotateVector((Velocity - PreviousLinearVelocity) /
        FMath::Max(.001f, GetWorld()->GetDeltaSeconds()) / 100.f);
    PreviousLinearVelocity = Velocity;
    const FVector Angular = Root->GetComponentQuat().UnrotateVector(Root->GetPhysicsAngularVelocityInRadians());
    for (int32 I = 0; I < 3; ++I)
    {
        const float Sign = I == 1 ? -1.f : 1.f;
        OutSensors[I] = FMath::Clamp(.5f + Sign * Accel[I] / 20.f, 0.f, 1.f);
        OutSensors[3 + I] = FMath::Clamp(.5f + Sign * Angular[I] / 20.f, 0.f, 1.f);
    }
    auto Probe = [&](const FVector& Start, const FVector& Dir, float Range)
    {
        FCollisionQueryParams Params; Params.AddIgnoredActor(GetOwner());
        FHitResult Hit;
        const bool Found = GetWorld()->LineTraceSingleByChannel(Hit, Start, Start + Dir.GetSafeNormal() * Range, ECC_Visibility, Params);
        return Found ? FMath::Clamp(1.f - Hit.Distance / Range, 0.f, 1.f) : 0.f;
    };
    OutSensors[6] = Probe(Head, Forward, 4.f);
    OutSensors[7] = Probe(Tail, -Forward, 4.f);
    for (int32 I = 8; I <= 18; ++I)
    {
        const FString Cue = I < 10 ? TEXT("light") : I < 12 ? TEXT("heat") : TEXT("chemical");
        const float Offset = I == 12 ? 0.f : (I == 8 || I == 10 || I == 13 || I == 15 || I == 17 ? -2.f : 2.f);
        const FVector Point = (I >= 17 ? Tail : Head) + Side * Offset;
        OutSensors[I] = NmSharedContent::SampleField(BrainId, Cue, Point, HabitatCentre, HabitatRadiusCm);
    }
    OutSensors[19] = NmSharedContent::SampleField(BrainId, TEXT("flow"), Head, HabitatCentre, HabitatRadiusCm);
    OutSensors[20] = NmSharedContent::SampleField(BrainId, TEXT("flow"), Tail, HabitatCentre, HabitatRadiusCm);
    OutSensors[21] = Probe(Head, Forward - Side * .4f, 50.f);
    OutSensors[22] = Probe(Head, Forward + Side * .4f, 50.f);
    OutSensors[23] = Probe(Tail, -Forward, 50.f);
}

// ----------------------------------------------------------------------------
// ApplyActuators — 96 channels
// Named MDL/MDR/MVL/MVR readouts map to each segment; vector indices are grouped
// by muscle name. MVL24 is absent and MVULVA is not a body-wall motor.
// ----------------------------------------------------------------------------

void UNmCelegansComponent::ApplyActuators(const TArray<float>& Actuators)
{
    if (Actuators.Num() < NumActuators)
    {
        return;
    }

    if (MdlTrace.Num() != NumSegments)
    {
        MdlTrace.Init(0.0f, NumSegments);
        MdrTrace.Init(0.0f, NumSegments);
        MvlTrace.Init(0.0f, NumSegments);
        MvrTrace.Init(0.0f, NumSegments);
    }
    if (FilteredDvDrive.Num() != NumSegments)
    {
        FilteredDvDrive.Init(0.0f, NumSegments);
        FilteredLrDrive.Init(0.0f, NumSegments);
    }

    TArray<float> SegmentDvDrive;
    TArray<float> SegmentLrDrive;
    SegmentDvDrive.Init(0.0f, NumSegments);
    SegmentLrDrive.Init(0.0f, NumSegments);

    TArray<FString> OutputNames;
    GetActuatorNames(OutputNames);
    auto Muscle = [&](const TCHAR* Group, int32 Segment)
    {
        const FString Suffix = FString::Printf(TEXT("_%s%02d"), Group, Segment + 1);
        const int32 Index = OutputNames.IndexOfByPredicate([&](const FString& Name) { return Name.EndsWith(Suffix); });
        return Index != INDEX_NONE && Actuators.IsValidIndex(Index) ? Actuators[Index] : 0.f;
    };
    float AbsDriveSum = 0.0f;
    float AbsDriveMax = 0.0f;
    for (int32 Seg = 0; Seg < NumSegments; ++Seg)
    {
        const float MDL = ContractFromOutput(Muscle(TEXT("MDL"), Seg), MdlTrace[Seg]);
        const float MDR = ContractFromOutput(Muscle(TEXT("MDR"), Seg), MdrTrace[Seg]);
        const float MVL = ContractFromOutput(Muscle(TEXT("MVL"), Seg), MvlTrace[Seg]);
        const float MVR = ContractFromOutput(Muscle(TEXT("MVR"), Seg), MvrTrace[Seg]);

        const float Dorsal = 0.5f * (MDL + MDR);
        const float Ventral = 0.5f * (MVL + MVR);
        const float Left = 0.5f * (MDL + MVL);
        const float Right = 0.5f * (MDR + MVR);

        // Primary locomotion drive is ventral-dorsal imbalance.
        float DvDrive = Ventral - Dorsal;
        // Small yaw bias near the head.
        float LrDrive = Right - Left;
        if (Seg <= 3)
        {
            DvDrive += 0.16f * LrDrive;
        }
        if (Seg < 5 || Seg > 20)
        {
            DvDrive *= 0.72f;
            LrDrive *= 0.72f;
        }

        SegmentDvDrive[Seg] = FMath::Clamp(DvDrive, -1.0f, 1.0f);
        SegmentLrDrive[Seg] = FMath::Clamp(LrDrive, -1.0f, 1.0f);
        const float AbsDrive = FMath::Abs(SegmentDvDrive[Seg]);
        AbsDriveSum += AbsDrive;
        AbsDriveMax = FMath::Max(AbsDriveMax, AbsDrive);
    }

    const float AbsDriveMean = AbsDriveSum / static_cast<float>(NumSegments);
    // Smooth per-segment drives before applying to constraints.
    for (int32 Seg = 0; Seg < NumSegments; ++Seg)
    {
        FilteredDvDrive[Seg] = FMath::Clamp(
            0.65f * FilteredDvDrive[Seg] + 0.35f * SegmentDvDrive[Seg],
            -1.0f,
            1.0f);
        FilteredLrDrive[Seg] = FMath::Clamp(
            0.70f * FilteredLrDrive[Seg] + 0.30f * SegmentLrDrive[Seg],
            -1.0f,
            1.0f);
    }

    const int32 NumJoints = Joints.Num();
    float MaxJointTargetAbsDeg = 0.0f;
    for (int32 JointIdx = 0; JointIdx < NumJoints; ++JointIdx)
    {
        UPhysicsConstraintComponent* Joint = Joints[JointIdx];
        if (!Joint)
        {
            continue;
        }

        const int32 SegA = FMath::Clamp(JointIdx, 0, NumSegments - 1);
        const int32 SegB = FMath::Clamp(JointIdx + 1, 0, NumSegments - 1);
        const float Dv = 0.5f * (FilteredDvDrive[SegA] + FilteredDvDrive[SegB]);
        const float Lr = 0.5f * (FilteredLrDrive[SegA] + FilteredLrDrive[SegB]);

        constexpr float MaxDvAngleDeg = 44.0f;
        constexpr float MaxLrAngleDeg = 20.0f;
        const float Swing1 = Dv * MaxDvAngleDeg;
        const float Swing2 = Lr * MaxLrAngleDeg;
        MaxJointTargetAbsDeg = FMath::Max(
            MaxJointTargetAbsDeg,
            FMath::Max(FMath::Abs(Swing1), FMath::Abs(Swing2)));

        Joint->SetAngularOrientationTarget(FRotator(Swing1, 0.f, Swing2));
    }

    const bool bNeedWake = MaxJointTargetAbsDeg > 1.0f;
    if (bNeedWake)
    {
        for (UStaticMeshComponent* Seg : SegmentMeshes)
        {
            if (Seg)
            {
                Seg->WakeAllRigidBodies();
            }
        }
    }

    if (++DriveDiagDecimator >= 240)
    {
        DriveDiagDecimator = 0;
        UE_LOG(LogTemp, Log,
               TEXT("NmCelegansDrive: abs_mean=%.4f abs_max=%.4f max_target_deg=%.2f"),
               AbsDriveMean, AbsDriveMax, MaxJointTargetAbsDeg);
    }
}

// ----------------------------------------------------------------------------
// GetSensorNames
// ----------------------------------------------------------------------------

void UNmCelegansComponent::GetSensorNames(TArray<FString>& OutNames) const
{
    NmSharedContent::Channels(TEXT("celegans"), false, OutNames);
}
void UNmCelegansComponent::GetActuatorNames(TArray<FString>& OutNames) const
{
    NmSharedContent::Channels(TEXT("celegans"), true, OutNames);
}

// ============================================================================
// ANmCelegansActor
// ============================================================================

ANmCelegansActor::ANmCelegansActor()
{
    PrimaryActorTick.bCanEverTick = true;

    // Load sphere mesh for segments; apply a tapered radius profile so the
    // chained body visually matches Webots' celegans spine proportions.
    static ConstructorHelpers::FObjectFinder<UStaticMesh> SphereMeshFinder(
        TEXT("/Engine/BasicShapes/Sphere.Sphere"));
    static ConstructorHelpers::FObjectFinder<UStaticMesh> CylinderMeshFinder(
        TEXT("/Engine/BasicShapes/Cylinder.Cylinder"));
    UStaticMesh* SphereMesh = SphereMeshFinder.Succeeded() ? SphereMeshFinder.Object : nullptr;
    UStaticMesh* CylinderMesh = CylinderMeshFinder.Succeeded()
        ? static_cast<UStaticMesh*>(CylinderMeshFinder.Object)
        : SphereMesh;

    SceneRoot = CreateDefaultSubobject<USceneComponent>(TEXT("SceneRoot"));
    SetRootComponent(SceneRoot);

    // Root segment
    UStaticMeshComponent* RootSeg = CreateDefaultSubobject<UStaticMeshComponent>(
        TEXT("Segment_00"));
    RootSeg->SetStaticMesh(SphereMesh);
    {
        const float RadiusCm = SegmentRadiusCm(0);
        RootSeg->SetRelativeScale3D(FVector((RadiusCm * kSegmentLongitudinalScale) / 50.0f,
                                            RadiusCm / 50.0f,
                                            RadiusCm / 50.0f));
    }
    RootSeg->SetSimulatePhysics(true);
    RootSeg->SetLinearDamping(5.0f);
    RootSeg->SetAngularDamping(5.0f);
    RootSeg->SetupAttachment(SceneRoot);
    RootSeg->SetVisibility(true, true);
    RootSeg->SetHiddenInGame(false, true);
    SegmentMeshes.Add(RootSeg);

    // Remaining segments and joints
    for (int32 i = 1; i < UNmCelegansComponent::NumSegments; ++i)
    {
        UStaticMeshComponent* Seg = CreateDefaultSubobject<UStaticMeshComponent>(
            *FString::Printf(TEXT("Segment_%02d"), i));
        Seg->SetStaticMesh(SphereMesh);
        {
            const float RadiusCm = SegmentRadiusCm(i);
            Seg->SetRelativeScale3D(FVector((RadiusCm * kSegmentLongitudinalScale) / 50.0f,
                                            RadiusCm / 50.0f,
                                            RadiusCm / 50.0f));
        }
        Seg->SetSimulatePhysics(true);
        Seg->SetLinearDamping(5.0f);
        Seg->SetAngularDamping(5.0f);
        Seg->SetupAttachment(SceneRoot);
        Seg->SetVisibility(true, true);
        Seg->SetHiddenInGame(false, true);
        SegmentMeshes.Add(Seg);

        // Joint between Segment[i-1] and Segment[i]
        UPhysicsConstraintComponent* Joint = CreateDefaultSubobject<UPhysicsConstraintComponent>(
            *FString::Printf(TEXT("Joint_%02d"), i));
        Joint->SetupAttachment(SceneRoot);
        // Angular limits
        Joint->SetAngularSwing1Limit(EAngularConstraintMotion::ACM_Limited, 45.f);
        Joint->SetAngularSwing2Limit(EAngularConstraintMotion::ACM_Limited, 45.f);
        Joint->SetAngularTwistLimit(EAngularConstraintMotion::ACM_Free, 0.f);
        // Linear locked
        Joint->SetLinearXLimit(ELinearConstraintMotion::LCM_Locked, 0.f);
        Joint->SetLinearYLimit(ELinearConstraintMotion::LCM_Locked, 0.f);
        Joint->SetLinearZLimit(ELinearConstraintMotion::LCM_Locked, 0.f);
        // Angular drive
        Joint->SetAngularDriveMode(EAngularDriveMode::TwistAndSwing);
        Joint->SetAngularOrientationDrive(true, true);
        Joint->SetAngularDriveParams(100.f, 10.f, 0.f);
        Joint->SetDisableCollision(true);
        Joints.Add(Joint);
    }

    // Render-only links between adjacent segments so the worm reads as one body
    // instead of isolated beads.
    for (int32 i = 0; i < UNmCelegansComponent::NumSegments - 1; ++i)
    {
        UStaticMeshComponent* Link = CreateDefaultSubobject<UStaticMeshComponent>(
            *FString::Printf(TEXT("VisualLink_%02d"), i));
        Link->SetStaticMesh(CylinderMesh);
        Link->SetCollisionEnabled(ECollisionEnabled::NoCollision);
        Link->SetSimulatePhysics(false);
        Link->SetVisibility(true, true);
        Link->SetHiddenInGame(false, true);
        Link->SetupAttachment(SceneRoot);
        VisualLinks.Add(Link);
    }

    // Create brain component and wire mesh refs
    CelegansComponent = CreateDefaultSubobject<UNmCelegansComponent>(
        TEXT("CelegansComponent"));
    CelegansComponent->SegmentMeshes = SegmentMeshes;
    CelegansComponent->Joints        = Joints;
}

void ANmCelegansActor::BeginPlay()
{
    Super::BeginPlay();

    // Lay the segments out in WORLD space using their *actual world* diameter.
    // This keeps spacing robust against actor scaling and avoids collapsing into
    // a tiny overlap cluster.
    const FVector BaseLoc = GetActorLocation();
    const FVector Fwd = GetActorForwardVector();
    float SegWorldDiameter = 6.0f;
    if (SegmentMeshes.Num() > 0 && SegmentMeshes[0])
    {
        // Use radial Y scale for spacing, not elongated X scale.
        SegWorldDiameter = SegmentMeshes[0]->GetComponentScale().Y * 100.0f;
    }
    const float Spacing = SegWorldDiameter * 0.82f;
    const float ChainCenter = 0.5f * static_cast<float>(SegmentMeshes.Num() - 1);
    for (int32 i = 0; i < SegmentMeshes.Num(); ++i)
    {
        if (SegmentMeshes[i])
        {
            const float Along = (static_cast<float>(i) - ChainCenter) * Spacing;
            SegmentMeshes[i]->SetWorldLocation(BaseLoc + Fwd * Along);
        }
    }

    // Wire joint component names
    for (int32 i = 0; i < Joints.Num(); ++i)
    {
        UPhysicsConstraintComponent* Joint = Joints[i];
        if (Joint && SegmentMeshes[i] && SegmentMeshes[i + 1])
        {
            // Place the constraint at the midpoint between the two segments so its
            // reference frames hold them apart at the correct spacing (otherwise a
            // joint left at the actor origin pulls both bodies together → collapse).
            const FVector Mid = 0.5f * (SegmentMeshes[i]->GetComponentLocation()
                                        + SegmentMeshes[i + 1]->GetComponentLocation());
            Joint->SetWorldLocation(Mid);

            Joint->ComponentName1.ComponentName = SegmentMeshes[i]->GetFName();
            Joint->ComponentName2.ComponentName = SegmentMeshes[i + 1]->GetFName();
            Joint->OverrideComponent1 = SegmentMeshes[i];
            Joint->OverrideComponent2 = SegmentMeshes[i + 1];
            Joint->InitComponentConstraint();

            // Re-apply motor settings after InitComponentConstraint() so the
            // instantiated runtime joint definitely has active angular drives.
            Joint->SetAngularDriveMode(EAngularDriveMode::TwistAndSwing);
            Joint->SetAngularOrientationDrive(true, true);
            Joint->SetAngularDriveAccelerationMode(true);
            Joint->SetAngularDriveParams(2200.f, 220.f, 2000000.f);
        }
    }

    UpdateVisualBody();

    if (SegmentMeshes.Num() > 1 && SegmentMeshes[0] && SegmentMeshes.Last())
    {
        int32 VisibleSegments = 0;
        for (UStaticMeshComponent* Seg : SegmentMeshes)
        {
            if (Seg && Seg->IsVisible() && !Seg->bHiddenInGame)
            {
                ++VisibleSegments;
            }
        }
        UE_LOG(LogTemp, Log,
               TEXT("NmCelegansActor: segments=%d visible_segments=%d visual_links=%d head=%s tail=%s spacing=%.2f"),
               SegmentMeshes.Num(),
               VisibleSegments,
               VisualLinks.Num(),
               *SegmentMeshes[0]->GetComponentLocation().ToString(),
               *SegmentMeshes.Last()->GetComponentLocation().ToString(),
               Spacing);
    }
}

void ANmCelegansActor::Tick(float DeltaSeconds)
{
    Super::Tick(DeltaSeconds);
    UpdateVisualBody();

    MotionDiagAccum += DeltaSeconds;
    if (MotionDiagAccum >= 2.0f && SegmentMeshes.Num() > 1 && SegmentMeshes[0] && SegmentMeshes.Last())
    {
        MotionDiagAccum = 0.0f;
        float MeanSpeed = 0.0f;
        int32 SpeedCount = 0;
        for (UStaticMeshComponent* Seg : SegmentMeshes)
        {
            if (!Seg)
            {
                continue;
            }
            MeanSpeed += Seg->GetPhysicsLinearVelocity().Size();
            ++SpeedCount;
        }
        MeanSpeed = SpeedCount > 0 ? (MeanSpeed / static_cast<float>(SpeedCount)) : 0.0f;

        const FVector HeadLoc = SegmentMeshes[0]->GetComponentLocation();
        const FVector TailLoc = SegmentMeshes.Last()->GetComponentLocation();
        const float HeadSpeed = SegmentMeshes[0]->GetPhysicsLinearVelocity().Size();
        const float TailSpeed = SegmentMeshes.Last()->GetPhysicsLinearVelocity().Size();

        UE_LOG(LogTemp, Log,
               TEXT("NmCelegansMotion: head=%s tail=%s head_speed=%.2f tail_speed=%.2f mean_speed=%.2f"),
               *HeadLoc.ToString(), *TailLoc.ToString(),
               HeadSpeed, TailSpeed, MeanSpeed);
    }
}

void ANmCelegansActor::UpdateVisualBody()
{
    const int32 NumLinks = FMath::Min(VisualLinks.Num(), SegmentMeshes.Num() - 1);
    for (int32 i = 0; i < NumLinks; ++i)
    {
        UStaticMeshComponent* Link = VisualLinks[i];
        UStaticMeshComponent* A = SegmentMeshes[i];
        UStaticMeshComponent* B = SegmentMeshes[i + 1];
        if (!Link || !A || !B)
        {
            continue;
        }

        FVector Delta = B->GetComponentLocation() - A->GetComponentLocation();
        float Length = Delta.Size();
        if (Length < KINDA_SMALL_NUMBER)
        {
            Delta = FVector::ForwardVector;
            Length = 1.0f;
        }
        const FVector Dir = Delta / Length;
        const FVector Mid = 0.5f * (A->GetComponentLocation() + B->GetComponentLocation());

        const float RadiusCm = 0.5f * (SegmentRadiusCm(i) + SegmentRadiusCm(i + 1))
                               * kVisualLinkRadiusScale;

        // BasicShape cylinder has default radius=50 cm, height=100 cm and points
        // along +Z, so XY scale sets thickness and Z scale sets link length.
        Link->SetWorldLocation(Mid);
        Link->SetWorldRotation(FQuat::FindBetweenNormals(FVector::UpVector, Dir));
        Link->SetWorldScale3D(
            FVector(RadiusCm / 50.0f, RadiusCm / 50.0f,
                    (Length * kVisualLinkLengthScale) / 100.0f));
    }
}
