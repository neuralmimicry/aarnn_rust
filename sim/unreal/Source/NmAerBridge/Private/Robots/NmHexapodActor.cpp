// Copyright NeuralMimicry. All Rights Reserved.

#include "Robots/NmHexapodActor.h"

#include "Components/StaticMeshComponent.h"
#include "Components/SceneCaptureComponent2D.h"
#include "Engine/TextureRenderTarget2D.h"
#include "PhysicsEngine/PhysicsConstraintComponent.h"
#include "Engine/World.h"
#include "UObject/ConstructorHelpers.h"

// ============================================================================
// UNmHexapodComponent
// ============================================================================

UNmHexapodComponent::UNmHexapodComponent()
{
    BrainId = TEXT("hexapod");
    PrimaryComponentTick.bCanEverTick = true;
}

// ----------------------------------------------------------------------------
// CollectSensors — 34 channels
// [0..17]  18 joint positions (6×3 legs)
// [18..23] 6 foot contacts
// [24..26] 3 body accelerometer (velocity derivative)
// [27..29] 3 body gyro
// [30..31] 2 sonar (front/rear)
// [32..33] 2 camera event channels
// ----------------------------------------------------------------------------

void UNmHexapodComponent::CollectSensors(TArray<float>& OutSensors)
{
    OutSensors.SetNumZeroed(NumSensors);

    AActor* Owner = GetOwner();
    UWorld* World = GetWorld();
    if (!Owner || !World)
    {
        return;
    }

    // --- Joint positions [0..17] via current joint angle (deg) ---
    constexpr float MaxJointDeg = 180.f;
    for (int32 j = 0; j < LegJoints.Num() && j < NumJoints; ++j)
    {
        UPhysicsConstraintComponent* Joint = LegJoints[j];
        if (Joint)
        {
            const float AngleDeg = FMath::Max3(FMath::Abs(Joint->GetCurrentTwist()),
                                               FMath::Abs(Joint->GetCurrentSwing1()),
                                               FMath::Abs(Joint->GetCurrentSwing2()));
            OutSensors[j] = FMath::Clamp(AngleDeg / MaxJointDeg, 0.f, 1.f);
        }
    }

    // --- Foot contacts [18..23] ---
    for (int32 f = 0; f < NumLegs; ++f)
    {
        OutSensors[NumJoints + f] = FootContacts[f] ? 1.f : 0.f;
        FootContacts[f] = false;
    }

    // --- Body accelerometer [24..26] ---
    if (BodyMesh)
    {
        const FVector LinVel = BodyMesh->GetPhysicsLinearVelocity();
        constexpr float MaxAccelCmS = 300.f;
        OutSensors[24] = FMath::Clamp((LinVel.X - PrevLinearVel.X) / MaxAccelCmS, -1.f, 1.f) * 0.5f + 0.5f;
        OutSensors[25] = FMath::Clamp((LinVel.Y - PrevLinearVel.Y) / MaxAccelCmS, -1.f, 1.f) * 0.5f + 0.5f;
        OutSensors[26] = FMath::Clamp((LinVel.Z - PrevLinearVel.Z) / MaxAccelCmS, -1.f, 1.f) * 0.5f + 0.5f;
        PrevLinearVel = LinVel;
    }

    // --- Body gyro [27..29] ---
    if (BodyMesh)
    {
        const FVector AngVel = BodyMesh->GetPhysicsAngularVelocityInDegrees();
        constexpr float MaxAngVelDegS = 360.f;
        OutSensors[27] = FMath::Clamp(AngVel.X / MaxAngVelDegS, -1.f, 1.f) * 0.5f + 0.5f;
        OutSensors[28] = FMath::Clamp(AngVel.Y / MaxAngVelDegS, -1.f, 1.f) * 0.5f + 0.5f;
        OutSensors[29] = FMath::Clamp(AngVel.Z / MaxAngVelDegS, -1.f, 1.f) * 0.5f + 0.5f;
        PrevAngularVel = AngVel;
    }

    // --- Sonar front/rear [30..31] ---
    if (BodyMesh)
    {
        const FVector BodyLoc     = BodyMesh->GetComponentLocation();
        const FVector ForwardDir  = BodyMesh->GetForwardVector();
        const ANmHexapodActor* HexActor = Cast<ANmHexapodActor>(Owner);
        const float MaxRange = HexActor ? HexActor->MaxSonarRangeCm : 150.f;

        auto TraceDistance = [&](const FVector& Dir) -> float
        {
            FHitResult Hit;
            const bool bHit = World->LineTraceSingleByChannel(
                Hit, BodyLoc, BodyLoc + Dir * MaxRange, ECC_Visibility);
            return bHit ? (1.f - (Hit.Distance / MaxRange)) : 0.f;
        };

        OutSensors[30] = TraceDistance(ForwardDir);            // front
        OutSensors[31] = TraceDistance(-ForwardDir);           // rear
    }

    // --- Camera event channels [32..33]: luminance delta as on/off events ---
    if (HeadCamera && HeadCamera->TextureTarget)
    {
        FRenderTarget* RT = HeadCamera->TextureTarget->GameThread_GetRenderTargetResource();
        if (RT)
        {
            TArray<FColor> Pixels;
            if (RT->ReadPixels(Pixels) && Pixels.Num() > 0)
            {
                // Mean luminance from full image
                float TotalLum = 0.f;
                for (const FColor& Px : Pixels)
                {
                    TotalLum += (Px.R + Px.G + Px.B) / (3.f * 255.f);
                }
                const float MeanLum = TotalLum / Pixels.Num();

                // Event channels: +change and -change
                const float Delta = MeanLum - PrevLuminance[0];
                OutSensors[32] = FMath::Clamp(Delta, 0.f, 1.f);    // positive event
                OutSensors[33] = FMath::Clamp(-Delta, 0.f, 1.f);   // negative event
                PrevLuminance[0] = MeanLum;
            }
        }
    }
}

// ----------------------------------------------------------------------------
// ApplyActuators — 18 channels (6×3 joints)
// ----------------------------------------------------------------------------

void UNmHexapodComponent::ApplyActuators(const TArray<float>& Actuators)
{
    if (Actuators.Num() < NumActuators)
    {
        return;
    }

    constexpr float MaxAngleDeg = 90.f;
    for (int32 j = 0; j < LegJoints.Num() && j < NumJoints; ++j)
    {
        UPhysicsConstraintComponent* Joint = LegJoints[j];
        if (Joint)
        {
            const float Target = FMath::Clamp(Actuators[j], -1.f, 1.f) * MaxAngleDeg;
            Joint->SetAngularOrientationTarget(FRotator(Target, 0.f, 0.f));
        }
    }
}

void UNmHexapodComponent::GetSensorNames(TArray<FString>& OutNames) const
{
    OutNames.Reset(NumSensors);
    const char* JointNames[] = {"coxa", "femur", "tibia"};
    for (int32 leg = 0; leg < NumLegs; ++leg)
    {
        for (int32 j = 0; j < JointsPerLeg; ++j)
        {
            OutNames.Add(FString::Printf(TEXT("leg%d_%s_pos"), leg, ANSI_TO_TCHAR(JointNames[j])));
        }
    }
    for (int32 f = 0; f < NumLegs; ++f)
    {
        OutNames.Add(FString::Printf(TEXT("foot%d_contact"), f));
    }
    OutNames.Add(TEXT("accel_x"));
    OutNames.Add(TEXT("accel_y"));
    OutNames.Add(TEXT("accel_z"));
    OutNames.Add(TEXT("gyro_x"));
    OutNames.Add(TEXT("gyro_y"));
    OutNames.Add(TEXT("gyro_z"));
    OutNames.Add(TEXT("sonar_front"));
    OutNames.Add(TEXT("sonar_rear"));
    OutNames.Add(TEXT("cam_event_pos"));
    OutNames.Add(TEXT("cam_event_neg"));
}

void UNmHexapodComponent::GetActuatorNames(TArray<FString>& OutNames) const
{
    OutNames.Reset(NumActuators);
    const char* JointNames[] = {"coxa", "femur", "tibia"};
    for (int32 leg = 0; leg < NumLegs; ++leg)
    {
        for (int32 j = 0; j < JointsPerLeg; ++j)
        {
            OutNames.Add(FString::Printf(TEXT("leg%d_%s_drive"), leg, ANSI_TO_TCHAR(JointNames[j])));
        }
    }
}

// ============================================================================
// ANmHexapodActor
// ============================================================================

ANmHexapodActor::ANmHexapodActor()
{
    PrimaryActorTick.bCanEverTick = false;

    static ConstructorHelpers::FObjectFinder<UStaticMesh> CubeMeshFinder(
        TEXT("/Engine/BasicShapes/Cube.Cube"));
    static ConstructorHelpers::FObjectFinder<UStaticMesh> SphereMeshFinder(
        TEXT("/Engine/BasicShapes/Sphere.Sphere"));
    static ConstructorHelpers::FObjectFinder<UStaticMesh> CylinderMeshFinder(
        TEXT("/Engine/BasicShapes/Cylinder.Cylinder"));

    UStaticMesh* CubeMesh   = CubeMeshFinder.Succeeded()   ? CubeMeshFinder.Object  : nullptr;
    UStaticMesh* SphereMesh = SphereMeshFinder.Succeeded() ? SphereMeshFinder.Object : nullptr;
    UStaticMesh* CylinderMesh = CylinderMeshFinder.Succeeded()
        ? CylinderMeshFinder.Object.Get()
        : nullptr;
    if (!CylinderMesh)
    {
        CylinderMesh = SphereMesh;
    }

    // --- Physics root body (kept minimal and mostly hidden) ---
    BodyMesh = CreateDefaultSubobject<UStaticMeshComponent>(TEXT("Body"));
    BodyMesh->SetStaticMesh(CubeMesh);
    BodyMesh->SetWorldScale3D(FVector(0.27f, 0.17f, 0.036f));
    BodyMesh->SetSimulatePhysics(true);
    BodyMesh->SetMassOverrideInKg(NAME_None, 1.200f, true); // Overrides the default huge autocalculated mass (keeps physics stable)
    BodyMesh->SetLinearDamping(1.8f);
    BodyMesh->SetAngularDamping(2.4f);
    BodyMesh->SetVisibility(false, false); // Do NOT propagate to children (keeps visual decks/legs visible!)
    BodyMesh->SetHiddenInGame(true, false); // Do NOT propagate to children (keeps visual decks/legs visible!)
    SetRootComponent(BodyMesh);

    // --- Visible stacked chassis deck (non-physical) ---
    ChassisDeckLower = CreateDefaultSubobject<UStaticMeshComponent>(TEXT("ChassisDeckLower"));
    ChassisDeckLower->SetStaticMesh(CubeMesh);
    ChassisDeckLower->SetWorldScale3D(FVector(0.29f, 0.19f, 0.012f));
    ChassisDeckLower->SetRelativeLocation(FVector(0.f, 0.f, 2.0f));
    ChassisDeckLower->SetupAttachment(BodyMesh);
    ChassisDeckLower->SetSimulatePhysics(false);
    ChassisDeckLower->SetCollisionEnabled(ECollisionEnabled::NoCollision);

    ChassisDeckUpper = CreateDefaultSubobject<UStaticMeshComponent>(TEXT("ChassisDeckUpper"));
    ChassisDeckUpper->SetStaticMesh(CubeMesh);
    ChassisDeckUpper->SetWorldScale3D(FVector(0.25f, 0.15f, 0.010f));
    ChassisDeckUpper->SetRelativeLocation(FVector(0.f, 0.f, 7.0f));
    ChassisDeckUpper->SetupAttachment(BodyMesh);
    ChassisDeckUpper->SetSimulatePhysics(false);
    ChassisDeckUpper->SetCollisionEnabled(ECollisionEnabled::NoCollision);

    ChassisNose = CreateDefaultSubobject<UStaticMeshComponent>(TEXT("ChassisNose"));
    ChassisNose->SetStaticMesh(CubeMesh);
    ChassisNose->SetWorldScale3D(FVector(0.06f, 0.10f, 0.020f));
    ChassisNose->SetRelativeLocation(FVector(13.5f, 0.f, 6.0f));
    ChassisNose->SetupAttachment(BodyMesh);
    ChassisNose->SetSimulatePhysics(false);
    ChassisNose->SetCollisionEnabled(ECollisionEnabled::NoCollision);

    // Leg attachment positions: 3 per side (front/middle/rear) aligned with shortened body
    const FVector LegAttach[6] = {
        FVector( 11.5f,  10.0f, -1.f), FVector( 0.f,  10.0f, -1.f), FVector(-11.5f,  10.0f, -1.f),
        FVector( 11.5f, -10.0f, -1.f), FVector( 0.f, -10.0f, -1.f), FVector(-11.5f, -10.0f, -1.f),
    };

    const float JointLengths[UNmHexapodComponent::JointsPerLeg] = {7.f, 11.f, 14.f}; // cm
    const float JointRadii[UNmHexapodComponent::JointsPerLeg] = {1.7f, 1.5f, 1.3f}; // cm
    const char* JointNames[] = {"coxa", "femur", "tibia"};

    for (int32 leg = 0; leg < UNmHexapodComponent::NumLegs; ++leg)
    {
        FVector PrevOffset = LegAttach[leg];

        for (int32 j = 0; j < UNmHexapodComponent::JointsPerLeg; ++j)
        {
            const FString MeshName  = FString::Printf(TEXT("Leg%d_%s"), leg, ANSI_TO_TCHAR(JointNames[j]));
            const FString JointName = FString::Printf(TEXT("LegJoint%d_%s"), leg, ANSI_TO_TCHAR(JointNames[j]));

            UStaticMeshComponent* Seg = CreateDefaultSubobject<UStaticMeshComponent>(*MeshName);
            Seg->SetStaticMesh(CylinderMesh);
            Seg->SetWorldScale3D(FVector(JointRadii[j] / 50.0f, JointRadii[j] / 50.0f, JointLengths[j] / 100.0f));
            Seg->SetupAttachment(BodyMesh);
            const FVector SegOffset = PrevOffset + FVector(0.f, 0.f, -JointLengths[j] * 0.5f);
            Seg->SetRelativeLocation(SegOffset);
            Seg->SetSimulatePhysics(true);
            Seg->SetMassOverrideInKg(NAME_None, (j == 0) ? 0.040f : ((j == 1) ? 0.150f : 0.140f), true); // Stable realistic mass (coxa=40g, femur=150g, tibia=140g)
            Seg->SetLinearDamping(1.0f);
            Seg->SetAngularDamping(1.6f);
            LegSegments.Add(Seg);

            UPhysicsConstraintComponent* Joint = CreateDefaultSubobject<UPhysicsConstraintComponent>(*JointName);
            Joint->SetupAttachment(BodyMesh);
            Joint->SetRelativeLocation(PrevOffset);
            Joint->SetAngularSwing1Limit(EAngularConstraintMotion::ACM_Limited, 75.f);
            Joint->SetAngularSwing2Limit(EAngularConstraintMotion::ACM_Limited, 55.f);
            Joint->SetAngularTwistLimit(EAngularConstraintMotion::ACM_Limited, 45.f);
            Joint->SetLinearXLimit(ELinearConstraintMotion::LCM_Locked, 0.f);
            Joint->SetLinearYLimit(ELinearConstraintMotion::LCM_Locked, 0.f);
            Joint->SetLinearZLimit(ELinearConstraintMotion::LCM_Locked, 0.f);
            Joint->SetAngularDriveMode(EAngularDriveMode::TwistAndSwing);
            Joint->SetAngularOrientationDrive(true, true);
            Joint->SetAngularDriveAccelerationMode(true);
            Joint->SetAngularDriveParams(350.f, 35.f, 1200000.f);
            Joint->SetDisableCollision(true);
            LegJoints.Add(Joint);

            PrevOffset = SegOffset + FVector(0.f, 0.f, -JointLengths[j] * 0.5f);
        }

        // Foot sphere (rigidly attached to tibia segment, ensuring physical and contact integrity)
        UStaticMeshComponent* TibiaSeg = LegSegments[leg * UNmHexapodComponent::JointsPerLeg + 2];
        UStaticMeshComponent* Foot = CreateDefaultSubobject<UStaticMeshComponent>(
            *FString::Printf(TEXT("Foot%d"), leg));
        Foot->SetStaticMesh(SphereMesh);
        Foot->SetWorldScale3D(FVector(0.045f));
        Foot->SetupAttachment(TibiaSeg);
        Foot->SetRelativeLocation(FVector(0.f, 0.f, -JointLengths[2] * 0.5f)); // Rigidly fixed at tibia tip
        Foot->SetSimulatePhysics(false); // Attached subobject inherits parent physics and collision
        Foot->SetCollisionEnabled(ECollisionEnabled::QueryAndPhysics);
        Foot->OnComponentHit.AddDynamic(this, &ANmHexapodActor::OnFootHit);
        FootSpheres.Add(Foot);
    }

    // --- Head camera mount ---
    USceneCaptureComponent2D* HeadCam = CreateDefaultSubobject<USceneCaptureComponent2D>(
        TEXT("HeadCamera"));
    HeadCam->SetupAttachment(BodyMesh);
    HeadCam->SetRelativeLocation(FVector(13.5f, 0.f, 5.f));
    HeadCam->CaptureSource = ESceneCaptureSource::SCS_FinalColorLDR;
    HeadCam->bCaptureEveryFrame = true;

    // --- Visible sensor package (positioned in BeginPlay; kinematic, follows body) ---
    // Camera housing.
    CameraHousing = CreateDefaultSubobject<UStaticMeshComponent>(TEXT("CameraHousing"));
    CameraHousing->SetStaticMesh(CubeMesh);
    CameraHousing->SetWorldScale3D(FVector(0.05f, 0.09f, 0.06f));
    CameraHousing->SetupAttachment(BodyMesh);
    CameraHousing->SetSimulatePhysics(false);
    CameraHousing->SetCollisionEnabled(ECollisionEnabled::NoCollision);

    // Two ultrasonic "eyes".
    SonarLeft = CreateDefaultSubobject<UStaticMeshComponent>(TEXT("SonarLeft"));
    SonarLeft->SetStaticMesh(SphereMesh);
    SonarLeft->SetWorldScale3D(FVector(0.05f));
    SonarLeft->SetupAttachment(BodyMesh);
    SonarLeft->SetSimulatePhysics(false);
    SonarLeft->SetCollisionEnabled(ECollisionEnabled::NoCollision);

    SonarRight = CreateDefaultSubobject<UStaticMeshComponent>(TEXT("SonarRight"));
    SonarRight->SetStaticMesh(SphereMesh);
    SonarRight->SetWorldScale3D(FVector(0.05f));
    SonarRight->SetupAttachment(BodyMesh);
    SonarRight->SetSimulatePhysics(false);
    SonarRight->SetCollisionEnabled(ECollisionEnabled::NoCollision);

    // --- 4 Brass standoffs connecting the chassis decks ---
    const FVector StandoffOffsets[4] = {
        FVector(  9.5f,  6.0f, 4.5f),
        FVector(  9.5f, -6.0f, 4.5f),
        FVector( -9.5f,  6.0f, 4.5f),
        FVector( -9.5f, -6.0f, 4.5f)
    };
    for (int32 i = 0; i < 4; ++i)
    {
        const FString Name = FString::Printf(TEXT("Standoff_%d"), i);
        UStaticMeshComponent* Standoff = CreateDefaultSubobject<UStaticMeshComponent>(*Name);
        Standoff->SetStaticMesh(CylinderMesh);
        Standoff->SetWorldScale3D(FVector(0.4f / 50.f, 0.4f / 50.f, 5.0f / 100.f));
        Standoff->SetupAttachment(BodyMesh);
        Standoff->SetRelativeLocation(StandoffOffsets[i]);
        Standoff->SetSimulatePhysics(false);
        Standoffs.Add(Standoff);
    }

    // --- 4 Orange/red cosmetic wire runs ---
    struct FWireSpec {
        FVector Loc;
        FRotator Rot;
        float Len;
    };
    FWireSpec WireSpecs[4] = {
        { FVector(0.f,  7.0f, 4.5f), FRotator(0.f, 0.f, 90.f), 20.0f }, // Left wire (longitudinal)
        { FVector(0.f, -7.0f, 4.5f), FRotator(0.f, 0.f, 90.f), 20.0f }, // Right wire (longitudinal)
        { FVector( 7.0f, 0.f, 4.5f), FRotator(90.f, 0.f, 0.f), 13.0f }, // Front cross wire
        { FVector(-7.0f, 0.f, 4.5f), FRotator(90.f, 0.f, 0.f), 13.0f }  // Rear cross wire
    };
    for (int32 i = 0; i < 4; ++i)
    {
        const FString Name = FString::Printf(TEXT("WireRun_%d"), i);
        UStaticMeshComponent* Wire = CreateDefaultSubobject<UStaticMeshComponent>(*Name);
        Wire->SetStaticMesh(CylinderMesh);
        Wire->SetWorldScale3D(FVector(0.2f / 50.f, 0.2f / 50.f, WireSpecs[i].Len / 100.f));
        Wire->SetupAttachment(BodyMesh);
        Wire->SetRelativeLocation(WireSpecs[i].Loc);
        Wire->SetRelativeRotation(WireSpecs[i].Rot);
        Wire->SetSimulatePhysics(false);
        WireRuns.Add(Wire);
    }

    // --- Brain component ---
    HexapodComponent = CreateDefaultSubobject<UNmHexapodComponent>(TEXT("HexapodComponent"));
    HexapodComponent->BodyMesh    = BodyMesh;
    HexapodComponent->LegSegments = LegSegments;
    HexapodComponent->FootSpheres = FootSpheres;
    HexapodComponent->LegJoints   = LegJoints;
    HexapodComponent->HeadCamera  = HeadCam;
}

void ANmHexapodActor::BeginPlay()
{
    // Force absolute dimensions, mass overrides, and proper subobject attachments
    // at BeginPlay to guarantee they completely bypass/override any stale Blueprint serialization data.
    if (BodyMesh)
    {
        BodyMesh->SetMassOverrideInKg(NAME_None, 1.200f, true);
        BodyMesh->SetSimulatePhysics(true);
        BodyMesh->SetVisibility(false, false); // Do NOT propagate to children (keeps visual decks/legs visible!)
        BodyMesh->SetHiddenInGame(true, false); // Do NOT propagate to children (keeps visual decks/legs visible!)
    }

    FVector RootScale = BodyMesh ? BodyMesh->GetRelativeScale3D() : FVector(0.27f, 0.17f, 0.036f);
    if (RootScale.X < 0.001f) RootScale.X = 0.27f;
    if (RootScale.Y < 0.001f) RootScale.Y = 0.17f;
    if (RootScale.Z < 0.001f) RootScale.Z = 0.036f;

    if (BodyMesh && ChassisDeckLower)
    {
        ChassisDeckLower->AttachToComponent(BodyMesh, FAttachmentTransformRules::KeepRelativeTransform);
        ChassisDeckLower->SetRelativeLocation(FVector(0.f, 0.f, 2.0f));
        ChassisDeckLower->SetRelativeScale3D(FVector(0.29f, 0.19f, 0.012f) / RootScale);
        ChassisDeckLower->SetSimulatePhysics(false);
        ChassisDeckLower->SetCollisionEnabled(ECollisionEnabled::NoCollision);
    }

    if (BodyMesh && ChassisDeckUpper)
    {
        ChassisDeckUpper->AttachToComponent(BodyMesh, FAttachmentTransformRules::KeepRelativeTransform);
        ChassisDeckUpper->SetRelativeLocation(FVector(0.f, 0.f, 7.0f));
        ChassisDeckUpper->SetRelativeScale3D(FVector(0.25f, 0.15f, 0.010f) / RootScale);
        ChassisDeckUpper->SetSimulatePhysics(false);
        ChassisDeckUpper->SetCollisionEnabled(ECollisionEnabled::NoCollision);
    }

    if (BodyMesh && ChassisNose)
    {
        ChassisNose->AttachToComponent(BodyMesh, FAttachmentTransformRules::KeepRelativeTransform);
        ChassisNose->SetRelativeLocation(FVector(13.5f, 0.f, 6.0f));
        ChassisNose->SetRelativeScale3D(FVector(0.06f, 0.10f, 0.020f) / RootScale);
        ChassisNose->SetSimulatePhysics(false);
        ChassisNose->SetCollisionEnabled(ECollisionEnabled::NoCollision);
    }

    const FVector StandoffOffsets[4] = {
        FVector(  9.5f,  6.0f, 4.5f),
        FVector(  9.5f, -6.0f, 4.5f),
        FVector( -9.5f,  6.0f, 4.5f),
        FVector( -9.5f, -6.0f, 4.5f)
    };
    for (int32 i = 0; i < Standoffs.Num(); ++i)
    {
        if (BodyMesh && Standoffs[i])
        {
            Standoffs[i]->AttachToComponent(BodyMesh, FAttachmentTransformRules::KeepRelativeTransform);
            Standoffs[i]->SetRelativeLocation(StandoffOffsets[i]);
            Standoffs[i]->SetRelativeScale3D(FVector(0.4f / 50.f, 0.4f / 50.f, 5.0f / 100.f) / RootScale);
            Standoffs[i]->SetSimulatePhysics(false);
            Standoffs[i]->SetCollisionEnabled(ECollisionEnabled::NoCollision);
        }
    }

    struct FWireSpec {
        FVector Loc;
        FRotator Rot;
        float Len;
    };
    FWireSpec WireSpecs[4] = {
        { FVector(0.f,  7.0f, 4.5f), FRotator(0.f, 0.f, 90.f), 20.0f },
        { FVector(0.f, -7.0f, 4.5f), FRotator(0.f, 0.f, 90.f), 20.0f },
        { FVector( 7.0f, 0.f, 4.5f), FRotator(90.f, 0.f, 0.f), 13.0f },
        { FVector(-7.0f, 0.f, 4.5f), FRotator(90.f, 0.f, 0.f), 13.0f }
    };
    for (int32 i = 0; i < WireRuns.Num(); ++i)
    {
        if (BodyMesh && WireRuns[i])
        {
            WireRuns[i]->AttachToComponent(BodyMesh, FAttachmentTransformRules::KeepRelativeTransform);
            WireRuns[i]->SetRelativeLocation(WireSpecs[i].Loc);
            WireRuns[i]->SetRelativeRotation(WireSpecs[i].Rot);
            WireRuns[i]->SetRelativeScale3D(FVector(0.2f / 50.f, 0.2f / 50.f, WireSpecs[i].Len / 100.f) / RootScale);
            WireRuns[i]->SetSimulatePhysics(false);
            WireRuns[i]->SetCollisionEnabled(ECollisionEnabled::NoCollision);
        }
    }

    for (int32 leg = 0; leg < UNmHexapodComponent::NumLegs; ++leg)
    {
        for (int32 j = 0; j < UNmHexapodComponent::JointsPerLeg; ++j)
        {
            const int32 Idx = leg * UNmHexapodComponent::JointsPerLeg + j;
            if (LegSegments.IsValidIndex(Idx) && LegSegments[Idx])
            {
                LegSegments[Idx]->SetSimulatePhysics(true);
                LegSegments[Idx]->SetMassOverrideInKg(NAME_None, (j == 0) ? 0.040f : ((j == 1) ? 0.150f : 0.140f), true);
            }
        }

        // Force rigid non-simulating foot attachment with complete scale safety
        const int32 TibiaIdx = leg * UNmHexapodComponent::JointsPerLeg + 2;
        if (LegSegments.IsValidIndex(TibiaIdx) && LegSegments[TibiaIdx] && FootSpheres.IsValidIndex(leg) && FootSpheres[leg])
        {
            UStaticMeshComponent* TibiaSeg = LegSegments[TibiaIdx];
            UStaticMeshComponent* Foot = FootSpheres[leg];
            Foot->SetSimulatePhysics(false);
            Foot->AttachToComponent(TibiaSeg, FAttachmentTransformRules::KeepRelativeTransform);
            Foot->SetRelativeLocation(FVector(0.f, 0.f, -14.0f * 0.5f)); // Fixed at tibia tip
            
            FVector TibiaScale = TibiaSeg->GetRelativeScale3D();
            if (TibiaScale.X < 0.001f) TibiaScale.X = 1.0f;
            if (TibiaScale.Y < 0.001f) TibiaScale.Y = 1.0f;
            if (TibiaScale.Z < 0.001f) TibiaScale.Z = 1.0f;
            
            Foot->SetRelativeScale3D(FVector(0.045f) / TibiaScale);
            Foot->SetCollisionEnabled(ECollisionEnabled::QueryAndPhysics);
        }
    }

    Super::BeginPlay();

    const FVector Base  = GetActorLocation();
    const FVector Fwd   = GetActorForwardVector();
    const FVector Right = GetActorRightVector();
    const FVector Up    = GetActorUpVector();

    const FVector BodyHalf =
        (BodyMesh ? BodyMesh->GetComponentScale() : FVector(0.9f, 0.3f, 0.6f)) * 50.f;

    const float AlongX[3] = { BodyHalf.X * 0.6f, 0.f, -BodyHalf.X * 0.6f };
    for (int32 leg = 0; leg < UNmHexapodComponent::NumLegs; ++leg)
    {
        const float Side = (leg < 3) ? 1.f : -1.f;
        const int32 Col  = leg % 3;
        const float ForeBias = (Col == 0) ? 0.55f : (Col == 2 ? -0.55f : 0.0f);
        const FVector Attach = Base
            + Fwd   * AlongX[Col]
            + Right * (Side * BodyHalf.Y * 1.05f)
            - Up    * (BodyHalf.Z * 0.15f);

        const FVector SegDir[3] = {
            (Right * Side * 1.1f + Fwd * ForeBias * 0.6f).GetSafeNormal(),
            (Right * Side * 0.7f + Fwd * ForeBias * 0.35f - Up * 1.05f).GetSafeNormal(),
            (Fwd * ForeBias * 0.20f - Up).GetSafeNormal(),
        };

        FVector Pos = Attach;
        for (int32 j = 0; j < UNmHexapodComponent::JointsPerLeg; ++j)
        {
            const int32 Idx = leg * UNmHexapodComponent::JointsPerLeg + j;
            if (!LegSegments.IsValidIndex(Idx) || !LegSegments[Idx])
            {
                continue;
            }
            UStaticMeshComponent* Seg = LegSegments[Idx];
            const float Len = FMath::Max(Seg->GetComponentScale().Z * 100.f, 4.f);
            const FVector Dir = SegDir[j];
            Seg->SetWorldRotation(FRotationMatrix::MakeFromZ(Dir).Rotator());
            Seg->SetWorldLocation(Pos + Dir * (Len * 0.5f));
            Pos += Dir * Len;
        }
        // Attached foot follows its parent tibia segment automatically in splayed pose
    }

    // Sensor package on the body front: camera housing flanked by two sonar eyes.
    const FVector Front = Base + Fwd * (BodyHalf.X + 3.0f) + Up * (BodyHalf.Z * 0.5f);
    if (CameraHousing) { CameraHousing->SetWorldLocation(Front); }
    if (SonarLeft)     { SonarLeft->SetWorldLocation(Front + Right * (-BodyHalf.Y * 0.7f) + Fwd * 1.5f); }
    if (SonarRight)    { SonarRight->SetWorldLocation(Front + Right * ( BodyHalf.Y * 0.7f) + Fwd * 1.5f); }

    // Wire each leg joint at the world midpoint of the two bodies it connects.
    for (int32 leg = 0; leg < UNmHexapodComponent::NumLegs; ++leg)
    {
        for (int32 j = 0; j < UNmHexapodComponent::JointsPerLeg; ++j)
        {
            const int32 Idx = leg * UNmHexapodComponent::JointsPerLeg + j;
            if (!LegJoints.IsValidIndex(Idx) || !LegSegments.IsValidIndex(Idx))
            {
                continue;
            }
            UStaticMeshComponent* Parent = (j == 0) ? ToRawPtr(BodyMesh) : ToRawPtr(LegSegments[Idx - 1]);
            UStaticMeshComponent* Child  = ToRawPtr(LegSegments[Idx]);
            if (Parent && Child)
            {
                LegJoints[Idx]->SetWorldLocation(
                    0.5f * (Parent->GetComponentLocation() + Child->GetComponentLocation()));
                LegJoints[Idx]->OverrideComponent1 = Parent;
                LegJoints[Idx]->OverrideComponent2 = Child;
                LegJoints[Idx]->InitComponentConstraint();
                LegJoints[Idx]->SetAngularDriveMode(EAngularDriveMode::TwistAndSwing);
                LegJoints[Idx]->SetAngularOrientationDrive(true, true);
                LegJoints[Idx]->SetAngularDriveAccelerationMode(true);
                LegJoints[Idx]->SetAngularDriveParams(350.f, 35.f, 1200000.f);
            }
        }
    }
}

void ANmHexapodActor::OnFootHit(UPrimitiveComponent* HitComp, AActor* /*OtherActor*/,
                                 UPrimitiveComponent* /*OtherComp*/, FVector /*NormalImpulse*/,
                                 const FHitResult& /*Hit*/)
{
    for (int32 f = 0; f < FootSpheres.Num(); ++f)
    {
        if (FootSpheres[f] == HitComp)
        {
            HexapodComponent->FootContacts[f] = true;
            break;
        }
    }
}
