// Copyright NeuralMimicry. All Rights Reserved.

#include "NmSimGameMode.h"
#include "NmSharedContent.h"

#include "NmRobotBase.h"
#include "NmSimManager.h"
#include "Robots/NmCelegansActor.h"
#include "Robots/NmDrosophilaActor.h"
#include "Robots/NmHexapodActor.h"
#include "Robots/NmNaoActor.h"
#include "Robots/NmZebrafishActor.h"

#include "GameFramework/DefaultPawn.h"
#include "GameFramework/PlayerController.h"
#include "GameFramework/Pawn.h"
#include "Engine/StaticMeshActor.h"
#include "Engine/StaticMesh.h"
#include "Components/StaticMeshComponent.h"
#include "Components/PrimitiveComponent.h"
#include "PhysicsEngine/PhysicsConstraintComponent.h"
#include "Materials/MaterialInterface.h"
#include "Materials/MaterialInstanceDynamic.h"
#include "HAL/PlatformMisc.h"
#include "Engine/World.h"
#include "UnrealClient.h"
#include "TimerManager.h"
#include "UObject/ConstructorHelpers.h"

// ---------------------------------------------------------------------------
// Per-type presentation config: visualisation scale, habitat kind, region size.
// The base bodies are built at biological cm-scale (a 6 cm worm, a ~5 cm fly);
// small species are scaled up so they are actually visible in the world.
// ---------------------------------------------------------------------------

namespace
{
struct FTypeCfg
{
    float Scale        = 1.f;    // uniform actor scale
    float RegionRadius = 500.f;  // habitat half-size (cm)
    ENmHabitat Habitat = ENmHabitat::Dish;
    float WallTopZ     = 60.f;   // habitat wall / water-surface height (cm)
    float BodyHalfLenX = 0.f;    // half body length along +X (to centre long bodies)
    float SpawnLiftZ   = 60.f;   // initial height above the floor (cm)
};

FTypeCfg GetTypeCfg(UClass* Cls)
{
    FTypeCfg C;
    if (Cls == ANmCelegansActor::StaticClass())
    {
        // Match the Webots celegans body proportions (tapered ~34 cm spine)
        // while upscaling enough for readability in the Unreal dish.
        C.Scale = 1.8f;  C.RegionRadius = 450.f; C.Habitat = ENmHabitat::Dish;
        C.WallTopZ = 40.f;
        // Celegans actor now self-centres its segment chain around actor origin.
        C.BodyHalfLenX = 0.f;
        C.SpawnLiftZ = 8.f;
    }
    else if (Cls == ANmDrosophilaActor::StaticClass())
    {
        C.Scale = 15.f;  C.RegionRadius = 300.f; C.Habitat = ENmHabitat::FlightArena;
        C.WallTopZ = 60.f; C.SpawnLiftZ = 60.f;
    }
    else if (Cls == ANmHexapodActor::StaticClass())
    {
        C.Scale = 3.f;   C.RegionRadius = 300.f; C.Habitat = ENmHabitat::Terrain;
        C.WallTopZ = 40.f; C.SpawnLiftZ = 90.f;
    }
    else if (Cls == ANmNaoActor::StaticClass())
    {
        C.Scale = 2.5f;  C.RegionRadius = 300.f; C.Habitat = ENmHabitat::Room;
        C.WallTopZ = 300.f; C.SpawnLiftZ = 80.f;
    }
    else if (Cls == ANmZebrafishActor::StaticClass())
    {
        C.Scale = 12.f;  C.RegionRadius = 250.f; C.Habitat = ENmHabitat::Tank;
        C.WallTopZ = C.RegionRadius * .625f;
        C.BodyHalfLenX = 11 * 0.6f * C.Scale * 0.5f;
        C.SpawnLiftZ = 120.f; // mid-water
    }
    return C;
}

// Give an up-scaled robot's joint drives enough authority to actuate the larger
// (heavier) body. Mass is left at its natural (scaled) value for stability — the
// earlier mass-normalisation made bodies near-massless and buoyancy launched the
// fish — so the drives are boosted proportionally to the linear scale.
void BoostRobotDrives(AActor* Robot, float Scale)
{
    if (!Robot)
    {
        return;
    }

    // Universal damping — dissipates energy so imperfect joint chains settle
    // instead of exploding (and gives a water/air-drag feel).
    TArray<UPrimitiveComponent*> Prims;
    Robot->GetComponents(Prims);
    for (UPrimitiveComponent* Prim : Prims)
    {
        Prim->SetLinearDamping(3.0f);
        Prim->SetAngularDamping(4.0f);
    }

    if (Scale <= 1.01f)
    {
        return;
    }
    // Heavier body (mass ~S^3) needs stiffer drives, but cap the gain so large
    // scales (e.g. the ×15 fly) don't get violent, explosive drive torques.
    const float Gain = FMath::Min(Scale * Scale, 10.f);
    TArray<UPhysicsConstraintComponent*> Joints;
    Robot->GetComponents(Joints);
    for (UPhysicsConstraintComponent* J : Joints)
    {
        if (!J)
        {
            continue;
        }
        const FAngularDriveConstraint& AD =
            J->ConstraintInstance.ProfileInstance.AngularDrive;
        float BaseS = FMath::Max3(AD.SwingDrive.Stiffness,
                                  AD.TwistDrive.Stiffness,
                                  AD.SlerpDrive.Stiffness);
        float BaseD = FMath::Max3(AD.SwingDrive.Damping,
                                  AD.TwistDrive.Damping,
                                  AD.SlerpDrive.Damping);
        if (BaseS <= 1.f)  { BaseS = 100.f; }
        if (BaseD <= 0.1f) { BaseD = 10.f; }
        J->SetAngularDriveParams(BaseS * Gain, BaseD * Gain, 0.f);
        // Don't let the two jointed bodies collide — overlapping segments (from the
        // compact limb layout) otherwise generate huge contact forces that explode
        // the ragdoll.
        J->SetDisableCollision(true);
    }
}
} // namespace

ANmSimGameMode::ANmSimGameMode()
{
    // A flyable spectator so the operator can look around the spawned robots.
    DefaultPawnClass = ADefaultPawn::StaticClass();
}

// ---------------------------------------------------------------------------
// Type resolution — mirrors the alias set in scripts/run_sim.sh
// ---------------------------------------------------------------------------

UClass* ANmSimGameMode::ResolveRobotClass(const FString& TypeToken, FString* OutCanonicalType)
{
    FString Key = TypeToken.ToLower();
    Key.TrimStartAndEndInline();
    // Match script-side normalization: fold non [a-z0-9] to '_'.
    FString Normalized;
    Normalized.Reserve(Key.Len());
    bool bLastWasUnderscore = false;
    for (const TCHAR Ch : Key)
    {
        if (FChar::IsAlnum(Ch))
        {
            Normalized.AppendChar(Ch);
            bLastWasUnderscore = false;
        }
        else if (!bLastWasUnderscore)
        {
            Normalized.AppendChar(TEXT('_'));
            bLastWasUnderscore = true;
        }
    }
    while (Normalized.StartsWith(TEXT("_")))
    {
        Normalized.RightChopInline(1);
    }
    while (Normalized.EndsWith(TEXT("_")))
    {
        Normalized.LeftChopInline(1);
    }
    Key = MoveTemp(Normalized);

    auto Has = [&Key](const TCHAR* Sub) { return Key.Contains(Sub); };

    if (Key == TEXT("celegans") || Key == TEXT("worm") || Key == TEXT("worms")
        || Key == TEXT("c_elegans") || Has(TEXT("celegans")))
    {
        if (OutCanonicalType) { *OutCanonicalType = TEXT("celegans"); }
        return ANmCelegansActor::StaticClass();
    }
    if (Has(TEXT("fafb")))
    {
        if (OutCanonicalType) { *OutCanonicalType = TEXT("drosophila_fafb"); }
        return ANmDrosophilaActor::StaticClass();
    }
    if (Has(TEXT("banc")) || Has(TEXT("drosophila")) || Has(TEXT("fly"))
        || Has(TEXT("flies")) || Has(TEXT("fruitfly")))
    {
        if (OutCanonicalType) { *OutCanonicalType = TEXT("drosophila_banc"); }
        return ANmDrosophilaActor::StaticClass();
    }
    if (Has(TEXT("hexapod")) || Has(TEXT("hex")) || Has(TEXT("freenove"))
        || Has(TEXT("six_legged")))
    {
        if (OutCanonicalType) { *OutCanonicalType = TEXT("hexapod"); }
        return ANmHexapodActor::StaticClass();
    }
    if (Key == TEXT("nao") || Key == TEXT("naos"))
    {
        if (OutCanonicalType) { *OutCanonicalType = TEXT("nao"); }
        return ANmNaoActor::StaticClass();
    }
    if (Has(TEXT("zebra")) || Has(TEXT("danio")) || Key == TEXT("fish")
        || Key == TEXT("zfish") || Key == TEXT("zf"))
    {
        if (OutCanonicalType) { *OutCanonicalType = TEXT("zebrafish"); }
        return ANmZebrafishActor::StaticClass();
    }
    if (OutCanonicalType)
    {
        OutCanonicalType->Reset();
    }
    return nullptr;
}

// ---------------------------------------------------------------------------
// Spawn one robot with its brain component wired before BeginPlay
// ---------------------------------------------------------------------------

AActor* ANmSimGameMode::SpawnRobot(UClass* RobotClass, const FString& BrainId,
                                   const FString& Host, int32 Port,
                                   const FVector& Location, float Scale, float WaterTopZ)
{
    UWorld* World = GetWorld();
    if (!World || !RobotClass)
    {
        return nullptr;
    }

    const FTransform SpawnXform(FRotator::ZeroRotator, Location, FVector(Scale));

    AActor* Robot = World->SpawnActorDeferred<AActor>(
        RobotClass, SpawnXform, nullptr, nullptr,
        ESpawnActorCollisionHandlingMethod::AlwaysSpawn);
    if (!Robot)
    {
        UE_LOG(LogTemp, Warning, TEXT("NmSimGameMode: failed to spawn %s"),
               *RobotClass->GetName());
        return nullptr;
    }

    // Wire the brain connection parameters before FinishSpawning triggers BeginPlay.
    if (UNmRobotBase* Bridge = Robot->FindComponentByClass<UNmRobotBase>())
    {
        if (!BrainId.IsEmpty())
        {
            Bridge->BrainId = BrainId;
        }
        Bridge->TcpHost = Host;
        Bridge->TcpPort = Port;
        UE_LOG(LogTemp, Log, TEXT("NmSimGameMode: %s → brain '%s' @ %s:%d"),
               *RobotClass->GetName(), *Bridge->BrainId, *Host, Port);
    }
    else
    {
        UE_LOG(LogTemp, Warning, TEXT("NmSimGameMode: %s has no UNmRobotBase component"),
               *RobotClass->GetName());
    }

    // For the zebrafish, set the water surface so buoyancy holds it in the tank.
    if (WaterTopZ > 0.f)
    {
        if (ANmZebrafishActor* Fish = Cast<ANmZebrafishActor>(Robot))
        {
            Fish->WaterSurfaceZ = WaterTopZ;
        }
        if (UNmZebrafishComponent* ZC = Robot->FindComponentByClass<UNmZebrafishComponent>())
        {
            ZC->WaterSurfaceZ = WaterTopZ;
        }
    }

    Robot->FinishSpawning(SpawnXform);

    // Apply a per-robot visual palette after components exist.

    // Give the (heavier, scaled) body enough joint-drive authority to actuate.
    BoostRobotDrives(Robot, Scale);

    return Robot;
}

// ---------------------------------------------------------------------------
// Habitats — one per robot type
// ---------------------------------------------------------------------------



// ---------------------------------------------------------------------------
// BeginPlay — parse the spec, group by type, build habitats + robots
// ---------------------------------------------------------------------------

void ANmSimGameMode::BeginPlay()
{
    Super::BeginPlay();

    UWorld* World = GetWorld();
    if (!World)
    {
        return;
    }

    FString Spec = FPlatformMisc::GetEnvironmentVariable(TEXT("NM_UE_ROBOTS"));
    if (Spec.IsEmpty())
    {
        Spec = TEXT("celegans=1");
    }

    const FString HostEnv = FPlatformMisc::GetEnvironmentVariable(TEXT("NM_AARNN_HOST"));
    const FString Host = HostEnv.IsEmpty() ? TEXT("127.0.0.1") : HostEnv;

    const FString PortEnv = FPlatformMisc::GetEnvironmentVariable(TEXT("NM_AARNN_BASE_PORT"));
    int32 BasePort = 7890;
    if (!PortEnv.IsEmpty())
    {
        BasePort = FCString::Atoi(*PortEnv);
    }

    UE_LOG(LogTemp, Log, TEXT("NmSimGameMode: spec='%s' host=%s base_port=%d"),
           *Spec, *Host, BasePort);

    // Split "type=count,type=count" — accept both ',' and ';' separators.
    TArray<FString> Tokens;
    Spec.ParseIntoArray(Tokens, TEXT(","), true);
    {
        TArray<FString> Extra;
        for (const FString& T : Tokens)
        {
            TArray<FString> Semi;
            T.ParseIntoArray(Semi, TEXT(";"), true);
            Extra.Append(Semi);
        }
        Tokens = MoveTemp(Extra);
    }

    // Enumerate brains in spec order (ports must match run_sim.sh), grouping by
    // canonical type so IDs remain stable across backends.
    TArray<FString> TypeOrder;                   // first-appearance order
    TMap<FString, UClass*> TypeClass;            // canonical type -> actor class
    TMap<FString, TArray<int32>> TypePorts;      // canonical type -> assigned ports
    int32 GlobalIndex = 0;

    for (const FString& Token : Tokens)
    {
        FString TypePart, CountPart;
        if (!Token.Split(TEXT("="), &TypePart, &CountPart))
        {
            continue;
        }
        TypePart.TrimStartAndEndInline();
        CountPart.TrimStartAndEndInline();

        FString CanonicalType;
        UClass* RobotClass = ResolveRobotClass(TypePart, &CanonicalType);
        if (!RobotClass)
        {
            UE_LOG(LogTemp, Warning, TEXT("NmSimGameMode: unknown robot type '%s'"), *TypePart);
            continue;
        }

        const int32 Count = FMath::Max(0, FCString::Atoi(*CountPart));
        for (int32 i = 0; i < Count; ++i)
        {
            if (!TypePorts.Contains(CanonicalType))
            {
                TypeOrder.Add(CanonicalType);
                TypeClass.Add(CanonicalType, RobotClass);
            }
            TypePorts.FindOrAdd(CanonicalType).Add(BasePort + GlobalIndex);
            ++GlobalIndex;
        }
    }

    if (GlobalIndex == 0)
    {
        UE_LOG(LogTemp, Warning, TEXT("NmSimGameMode: spec '%s' produced no robots"), *Spec);
        return;
    }

    // Lay out one habitat region per type in a row along +Y.
    float RunningY = 0.f;
    float MinY = 0.f, MaxY = 0.f, MaxRegion = 0.f;

    for (const FString& TypeKey : TypeOrder)
    {
        UClass* const* ClsPtr = TypeClass.Find(TypeKey);
        if (!ClsPtr || !*ClsPtr)
        {
            continue;
        }
        UClass* Cls = *ClsPtr;
        const FTypeCfg Cfg = GetTypeCfg(Cls);
        const TArray<int32>& Ports = TypePorts.FindChecked(TypeKey);
        const int32 N = Ports.Num();

        const FVector RegionCenter(0.f, RunningY + Cfg.RegionRadius, 0.f);
        if (!NmSharedContent::SpawnHabitat(World, TypeKey, RegionCenter, Cfg.RegionRadius))
        {
            UE_LOG(LogTemp, Error, TEXT("Shared habitat failed for %s"), *TypeKey);
            return;
        }

        for (int32 j = 0; j < N; ++j)
        {
            // Arrange multiple same-type instances in a ring inside the region.
            FVector Offset(-Cfg.BodyHalfLenX, 0.f, Cfg.SpawnLiftZ);
            if (N > 1)
            {
                const float A = (2.f * PI * j) / N;
                const float R = Cfg.RegionRadius * 0.45f;
                Offset += FVector(FMath::Cos(A) * R, FMath::Sin(A) * R, 0.f);
            }
            const FString BrainId = FString::Printf(TEXT("%s_%d"), *TypeKey, j);
            const float WaterZ = (Cfg.Habitat == ENmHabitat::Tank) ? Cfg.WallTopZ : 0.f;
            if (AActor* R = SpawnRobot(Cls, BrainId, Host, Ports[j],
                                       RegionCenter + Offset, Cfg.Scale, WaterZ))
            {
                if (auto Brain = R->FindComponentByClass<UNmRobotBase>())
                {
                    Brain->HabitatCentre = RegionCenter;
                    Brain->HabitatRadiusCm = Cfg.RegionRadius;
                }
                NmSharedContent::DressRobot(R, TypeKey, bAnatomyCutaway);
                SpawnedRobots.Add(R);
            }
        }

        MinY = FMath::Min(MinY, RunningY);
        MaxY = FMath::Max(MaxY, RunningY + 2.f * Cfg.RegionRadius);
        MaxRegion = FMath::Max(MaxRegion, Cfg.RegionRadius);
        RunningY += 2.f * Cfg.RegionRadius + 400.f; // gap between habitats
    }

    // Frame the whole scene.
    SceneCenter = FVector(0.f, (MinY + MaxY) * 0.5f, 40.f);
    SceneRadius = FMath::Max((MaxY - MinY) * 0.5f, MaxRegion) + 200.f;
    SceneFocus  = SceneRadius;
    if (TypeOrder.Num() == 1 && TypeOrder[0] == TEXT("celegans"))
    {
        // Worm-only runs benefit from a tighter initial framing.
        SceneFocus *= 0.45f;
        MinCameraDistance = 280.f;
    }
    else
    {
        MinCameraDistance = 500.f;
    }

    // Spawn the tracking/overlay manager (auto-discovers the robots on Start).
    World->SpawnActor<ANmSimManager>(ANmSimManager::StaticClass(), FTransform::Identity);

    // Frame the camera once the player pawn exists (looping timer clears itself).
    GetWorldTimerManager().SetTimer(
        FrameTimerHandle, this, &ANmSimGameMode::FrameCamera, 0.4f, true);

    // One-shot diagnostic snapshot of robot positions/sizes after physics settles.
    GetWorldTimerManager().SetTimer(
        DiagTimerHandle, this, &ANmSimGameMode::LogRobotDiag, 5.0f, false);

    UE_LOG(LogTemp, Log, TEXT("NmSimGameMode: spawned %d robot(s) across %d habitat(s)."),
           GlobalIndex, TypeOrder.Num());
}

// ---------------------------------------------------------------------------
// Camera framing — aim the spectator pawn at the spawned robots
// ---------------------------------------------------------------------------

void ANmSimGameMode::FrameCamera()
{
    UWorld* World = GetWorld();
    if (!World)
    {
        return;
    }
    APlayerController* PC = World->GetFirstPlayerController();
    APawn* P = PC ? PC->GetPawn() : nullptr;
    if (!PC || !P)
    {
        return; // pawn not ready yet — the looping timer will retry
    }

    const float Dist = FMath::Max(SceneFocus * 1.8f, MinCameraDistance);
    const FVector CamLoc =
        SceneCenter + FVector(-Dist * 0.75f, 0.f, Dist * 0.6f);
    const FRotator LookAt = (SceneCenter - CamLoc).Rotation();

    P->SetActorLocationAndRotation(CamLoc, LookAt);
    PC->SetControlRotation(LookAt);

    GetWorldTimerManager().ClearTimer(FrameTimerHandle);

    UE_LOG(LogTemp, Log, TEXT("NmSimGameMode: camera framed at %s"), *CamLoc.ToString());
}

// ---------------------------------------------------------------------------
// Diagnostic — where did the robots actually end up?
// ---------------------------------------------------------------------------

void ANmSimGameMode::LogRobotDiag()
{
    // Optional own-framebuffer QA evidence, after camera framing and render warmup.
    const FString Capture = FPlatformMisc::GetEnvironmentVariable(TEXT("NM_SIM_CONTENT_CAPTURE"));
    if (!Capture.IsEmpty()) FScreenshotRequest::RequestScreenshot(Capture, false, false);
    for (const TObjectPtr<AActor>& R : SpawnedRobots)
    {
        if (!R)
        {
            continue;
        }
        FVector Origin, Extent;
        R->GetActorBounds(true, Origin, Extent);
        const FVector Loc = R->GetActorLocation();
        const FVector Scl = R->GetActorScale3D();
        UE_LOG(LogTemp, Log,
               TEXT("NmDiag: %s loc=%s scale=%s boundsOrigin=%s boundsExtent=%s"),
               *R->GetName(), *Loc.ToString(), *Scl.ToString(),
               *Origin.ToString(), *Extent.ToString());
    }
}
