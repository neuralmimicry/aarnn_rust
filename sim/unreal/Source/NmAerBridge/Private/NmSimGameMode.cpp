// Copyright NeuralMimicry. All Rights Reserved.

#include "NmSimGameMode.h"

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
        C.WallTopZ = 220.f;
        C.BodyHalfLenX = 11 * 0.6f * C.Scale * 0.5f;
        C.SpawnLiftZ = 120.f; // mid-water
    }
    return C;
}

UMaterialInterface* GetBasicShapeMaterial()
{
    static UMaterialInterface* Mat = LoadObject<UMaterialInterface>(
        nullptr, TEXT("/Engine/BasicShapes/BasicShapeMaterial.BasicShapeMaterial"));
    return Mat;
}

void TintStaticMesh(UStaticMeshComponent* Mesh,
                    const FLinearColor& Color,
                    float Roughness = 0.72f,
                    float Metallic = 0.02f,
                    bool bForceTintableMaterial = true)
{
    if (!Mesh)
    {
        return;
    }

    if (bForceTintableMaterial)
    {
        if (UMaterialInterface* BaseMat = GetBasicShapeMaterial())
        {
            Mesh->SetMaterial(0, BaseMat);
        }
    }

    UMaterialInstanceDynamic* MID = Mesh->CreateAndSetMaterialInstanceDynamic(0);
    if (!MID)
    {
        if (UMaterialInterface* BaseMat = GetBasicShapeMaterial())
        {
            Mesh->SetMaterial(0, BaseMat);
            MID = Mesh->CreateAndSetMaterialInstanceDynamic(0);
        }
    }
    if (!MID)
    {
        return;
    }

    // Support both common parameter names used by basic shape/debug materials.
    MID->SetVectorParameterValue(TEXT("Color"), Color);
    MID->SetVectorParameterValue(TEXT("BaseColor"), Color);
    MID->SetVectorParameterValue(TEXT("Base Color"), Color);
    MID->SetVectorParameterValue(TEXT("Tint"), Color);
    MID->SetScalarParameterValue(TEXT("Roughness"), Roughness);
    MID->SetScalarParameterValue(TEXT("Metallic"), Metallic);
}

FLinearColor RobotTintForClass(UClass* Cls)
{
    if (Cls == ANmCelegansActor::StaticClass())
    {
        return FLinearColor(0.96f, 0.54f, 0.26f);
    }
    if (Cls == ANmDrosophilaActor::StaticClass())
    {
        return FLinearColor(0.55f, 0.33f, 0.16f);
    }
    if (Cls == ANmDrosophilaActor::StaticClass())
    {
        return FLinearColor(0.55f, 0.33f, 0.16f);
    }

    if (Cls == ANmHexapodActor::StaticClass())
    {
        return FLinearColor(0.22f, 0.52f, 0.82f);
    }
    if (Cls == ANmNaoActor::StaticClass())
    {
        return FLinearColor(0.82f, 0.84f, 0.88f);
    }
    if (Cls == ANmZebrafishActor::StaticClass())
    {
        return FLinearColor(0.20f, 0.68f, 0.82f);
    }
    return FLinearColor(0.72f, 0.72f, 0.72f);
}

void TintRobotMeshes(AActor* Robot, UClass* Cls)
{
    if (!Robot)
    {
        return;
    }

    const FLinearColor Base = RobotTintForClass(Cls);
    TArray<UStaticMeshComponent*> Meshes;
    Robot->GetComponents(Meshes);
    int32 TintedCount = 0;

    if (Cls == ANmZebrafishActor::StaticClass())
    {
        // Custom gorgeous zebrafish stripes and translucent fins (anatomically correct)
        for (UStaticMeshComponent* Mesh : Meshes)
        {
            if (!Mesh)
            {
                continue;
            }

            FLinearColor Color = FLinearColor(0.20f, 0.68f, 0.82f); // default blueish
            float Roughness = 0.22f;
            float Metallic = 0.85f; // shiny fish scales!

            const FString Name = Mesh->GetName().ToLower();
            if (Name.Contains(TEXT("fin")))
            {
                // Semi-translucent silver-cyan aquatic fins
                Color = FLinearColor(0.35f, 0.72f, 0.90f);
                Roughness = 0.45f;
                Metallic = 0.60f;
            }
            else if (Name.Contains(TEXT("eye")))
            {
                // Wet glossy black eyes
                Color = FLinearColor(0.01f, 0.01f, 0.015f);
                Roughness = 0.04f;
                Metallic = 0.95f;
            }
            else if (Name.Contains(TEXT("segment")))
            {
                // Alternating deep dark blue and brilliant gold/silver zebra stripes!
                int32 SegNum = 0;
                FString SegNumStr = Name.Replace(TEXT("segment_"), TEXT(""));
                if (SegNumStr.IsNumeric())
                {
                    SegNum = FCString::Atoi(*SegNumStr);
                }
                
                if (SegNum % 2 == 0)
                {
                    // Dark zebra stripe (metallic deep blue/black)
                    Color = FLinearColor(0.012f, 0.038f, 0.160f);
                    Roughness = 0.18f;
                    Metallic = 0.90f;
                }
                else
                {
                    // Light zebra stripe (metallic gold/silver)
                    Color = FLinearColor(0.920f, 0.810f, 0.540f);
                    Roughness = 0.15f;
                    Metallic = 0.92f;
                }
            }

            TintStaticMesh(Mesh, Color, Roughness, Metallic);
            ++TintedCount;
        }

        UE_LOG(LogTemp, Log,
               TEXT("NmVisual: %s class=%s tinted_meshes=%d (Zebrafish custom zebra stripes)"),
               *Robot->GetName(), *GetNameSafe(Cls), TintedCount);
        return;
    }

    if (Cls == ANmDrosophilaActor::StaticClass())
    {
        // Custom gorgeous drosophila stripes, bright red eyes, and translucent wings
        for (UStaticMeshComponent* Mesh : Meshes)
        {
            if (!Mesh)
            {
                continue;
            }

            FLinearColor Color = Base; // default gold-brown
            float Roughness = 0.32f;
            float Metallic = 0.05f;

            const FString Name = Mesh->GetName().ToLower();
            if (Name.Contains(TEXT("abdomen")))
            {
                // Dark brown with gold/black stripe base for organic abdomen
                Color = FLinearColor(0.28f, 0.16f, 0.06f);
                Roughness = 0.28f;
                Metallic = 0.12f;
            }
            else if (Name.Contains(TEXT("thorax")) || Name.Contains(TEXT("head")))
            {
                // Shiny organic gold-brown thorax/head chitin
                Color = FLinearColor(0.55f, 0.33f, 0.16f);
                Roughness = 0.22f;
                Metallic = 0.20f;
            }
            if (Name.Contains(TEXT("eye")))
            {
                // Shiny bright red compound eyes!
                Color = FLinearColor(0.95f, 0.05f, 0.02f);
                Roughness = 0.15f;
                Metallic = 0.15f;
            }
            else if (Name.Contains(TEXT("wing")))
            {
                // Translucent grey-blue wings
                Color = FLinearColor(0.82f, 0.84f, 0.88f, 0.45f);
                Roughness = 0.50f;
                Metallic = 0.00f;
            }
            else if (Name.Contains(TEXT("segment")) || Name.Contains(TEXT("leg")) || Name.Contains(TEXT("coxa")) || Name.Contains(TEXT("femur")) || Name.Contains(TEXT("tibia")))
            {
                // Striped brown-gold legs
                Color = FLinearColor(0.55f, 0.33f, 0.16f);
                Roughness = 0.45f;
                Metallic = 0.02f;
            }

            TintStaticMesh(Mesh, Color, Roughness, Metallic);
            ++TintedCount;
        }

        UE_LOG(LogTemp, Log,
               TEXT("NmVisual: %s class=%s tinted_meshes=%d (Drosophila custom red eyes)"),
               *Robot->GetName(), *GetNameSafe(Cls), TintedCount);
        return;
    }

    if (Cls == ANmHexapodActor::StaticClass())
    {
        // Custom gorgeous hexapod coloring to match Webots R2025a precisely.
        for (UStaticMeshComponent* Mesh : Meshes)
        {
            if (!Mesh)
            {
                continue;
            }

            FLinearColor Color = FLinearColor(0.02f, 0.02f, 0.02f); // Sleek black acrylic base
            float Roughness = 0.34f;
            float Metallic = 0.03f;

            const FString Name = Mesh->GetName().ToLower();
            if (Name.Contains(TEXT("standoff")))
            {
                // Brass/gold standoffs
                Color = FLinearColor(0.95f, 0.67f, 0.22f);
                Roughness = 0.25f;
                Metallic = 0.75f;
            }
            else if (Name.Contains(TEXT("wire")) || Name.Contains(TEXT("cable")))
            {
                // Orange/red wire runs
                Color = FLinearColor(0.90f, 0.24f, 0.03f);
                Roughness = 0.52f;
                Metallic = 0.00f;
            }
            else if (Name.Contains(TEXT("foot")) || Name.Contains(TEXT("sphere")))
            {
                // Dark grey feet
                Color = FLinearColor(0.13f, 0.14f, 0.15f);
                Roughness = 0.68f;
                Metallic = 0.02f;
            }
            else if (Name.Contains(TEXT("sonar")) || Name.Contains(TEXT("eye")))
            {
                // Sonar "eyes" are a lighter, metallic aluminum/silver look
                Color = FLinearColor(0.76f, 0.77f, 0.78f);
                Roughness = 0.32f;
                Metallic = 0.85f;
            }
            else if (Name.Contains(TEXT("camera")) || Name.Contains(TEXT("housing")))
            {
                // Dark camera housing
                Color = FLinearColor(0.015f, 0.015f, 0.018f);
                Roughness = 0.45f;
                Metallic = 0.10f;
            }
            else if (Name.Contains(TEXT("leg")) || Name.Contains(TEXT("coxa")) || Name.Contains(TEXT("femur")) || Name.Contains(TEXT("tibia")))
            {
                // Matte black acrylic legs
                Color = FLinearColor(0.006f, 0.006f, 0.007f);
                Roughness = 0.34f;
                Metallic = 0.03f;
            }
            else if (Name.Contains(TEXT("deck")) || Name.Contains(TEXT("nose")) || Name.Contains(TEXT("body")))
            {
                // Glossy black acrylic main body decks
                Color = FLinearColor(0.012f, 0.012f, 0.014f);
                Roughness = 0.28f;
                Metallic = 0.05f;
            }

            TintStaticMesh(Mesh, Color, Roughness, Metallic);
            ++TintedCount;
        }

        UE_LOG(LogTemp, Log,
               TEXT("NmVisual: %s class=%s tinted_meshes=%d (Hexapod custom palette)"),
               *Robot->GetName(), *GetNameSafe(Cls), TintedCount);
        return;
    }

    for (UStaticMeshComponent* Mesh : Meshes)
    {
        if (!Mesh)
        {
            continue;
        }

        FLinearColor Color = Base;
        const FString Name = Mesh->GetName().ToLower();
        if (Name.Contains(TEXT("head")) || Name.Contains(TEXT("eye"))
            || Name.Contains(TEXT("sonar")) || Name.Contains(TEXT("camera")))
        {
            Color = Base + FLinearColor(0.08f, 0.08f, 0.08f, 0.0f);
        }
        else if (Name.Contains(TEXT("foot")) || Name.Contains(TEXT("hand")))
        {
            Color = Base * FLinearColor(0.75f, 0.75f, 0.75f, 1.0f);
        }
        else if (Name.Contains(TEXT("wing")))
        {
            Color = FLinearColor(0.84f, 0.86f, 0.92f);
        }

        TintStaticMesh(Mesh, Color);
        ++TintedCount;
    }

    UE_LOG(LogTemp, Log,
           TEXT("NmVisual: %s class=%s tinted_meshes=%d"),
           *Robot->GetName(), *GetNameSafe(Cls), TintedCount);
}

// Spawn a movable cube-mesh box of the given full size (cm) at Loc/Rot.
AStaticMeshActor* SpawnBox(UWorld* World, UStaticMesh* CubeMesh,
                           const FVector& Loc, const FRotator& Rot,
                           const FVector& SizeCm,
                           const FLinearColor& Color = FLinearColor(0.72f, 0.74f, 0.77f))
{
    if (!World || !CubeMesh)
    {
        return nullptr;
    }
    AStaticMeshActor* Box = World->SpawnActor<AStaticMeshActor>(
        AStaticMeshActor::StaticClass(), Loc, Rot);
    if (!Box)
    {
        return nullptr;
    }
    UStaticMeshComponent* MC = Box->GetStaticMeshComponent();
    MC->SetMobility(EComponentMobility::Movable);
    MC->SetStaticMesh(CubeMesh);
    TintStaticMesh(MC, Color);
    Box->SetActorScale3D(SizeCm / 100.f); // engine cube is 100 cm
    return Box;
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
    TintRobotMeshes(Robot, RobotClass);

    // Give the (heavier, scaled) body enough joint-drive authority to actuate.
    BoostRobotDrives(Robot, Scale);

    return Robot;
}

// ---------------------------------------------------------------------------
// Habitats — one per robot type
// ---------------------------------------------------------------------------

void ANmSimGameMode::SpawnHabitat(ENmHabitat Kind, const FVector& Center,
                                  float RadiusCm, float WallTopZ)
{
    UWorld* World = GetWorld();
    if (!World)
    {
        return;
    }

    UStaticMesh* Cube =
        LoadObject<UStaticMesh>(nullptr, TEXT("/Engine/BasicShapes/Cube.Cube"));
    UStaticMesh* Sphere =
        LoadObject<UStaticMesh>(nullptr, TEXT("/Engine/BasicShapes/Sphere.Sphere"));

    switch (Kind)
    {
    case ENmHabitat::Dish:
    {
        // Celegans dish: circular rim + obstacle layout mirroring the Webots
        // lab-station style (bars, ramp/platform) plus food spots.
        constexpr int32 NumWalls = 40;
        const float SegLen = (2.f * PI * RadiusCm / NumWalls) * 1.15f;
        const FLinearColor RimColor(0.18f, 0.26f, 0.38f);
        for (int32 i = 0; i < NumWalls; ++i)
        {
            const float A = (2.f * PI * i) / NumWalls;
            const FVector Loc = Center + FVector(FMath::Cos(A) * RadiusCm,
                                                 FMath::Sin(A) * RadiusCm,
                                                 WallTopZ * 0.5f);
            SpawnBox(World, Cube, Loc,
                     FRotator(0.f, FMath::RadiansToDegrees(A) + 90.f, 0.f),
                     FVector(SegLen, 8.f, WallTopZ), RimColor);
        }

        // Explicit floor so the worm is always visible even in maps without a
        // default ground plane.
        SpawnBox(World, Cube, Center + FVector(0.f, 0.f, 2.f),
                 FRotator::ZeroRotator,
                 FVector(2.f * RadiusCm * 0.95f, 2.f * RadiusCm * 0.95f, 4.f),
                 FLinearColor(0.67f, 0.65f, 0.58f));

        // Parallel rails.
        SpawnBox(World, Cube, Center + FVector(-85.f,  95.f, 18.f),
                 FRotator::ZeroRotator, FVector(300.f, 40.f, 36.f),
                 FLinearColor(0.23f, 0.56f, 0.73f));
        SpawnBox(World, Cube, Center + FVector(-115.f, -95.f, 18.f),
                 FRotator::ZeroRotator, FVector(300.f, 40.f, 36.f),
                 FLinearColor(0.40f, 0.69f, 0.54f));

        // Right-side vertical barrier.
        SpawnBox(World, Cube, Center + FVector(170.f, 30.f, 58.f),
                 FRotator::ZeroRotator, FVector(26.f, 170.f, 116.f),
                 FLinearColor(0.78f, 0.50f, 0.32f));

        // Upper-right angled bar.
        SpawnBox(World, Cube, Center + FVector(70.f, 200.f, 36.f),
                 FRotator(0.f, 18.f, 0.f), FVector(160.f, 34.f, 36.f),
                 FLinearColor(0.92f, 0.70f, 0.36f));

        // Front ramp/platform block.
        SpawnBox(World, Cube, Center + FVector(35.f, -225.f, 44.f),
                 FRotator(0.f, -10.f, 0.f), FVector(190.f, 145.f, 88.f),
                 FLinearColor(0.86f, 0.77f, 0.46f));

        // Flat pad on the right.
        SpawnBox(World, Cube, Center + FVector(240.f, -85.f, 6.f),
                 FRotator::ZeroRotator, FVector(210.f, 130.f, 12.f),
                 FLinearColor(0.56f, 0.72f, 0.63f));

        // Food spots around the worm's central task area.
        const FVector FoodOffsets[] = {
            FVector(-35.f,  -5.f, 16.f),
            FVector(-15.f,  30.f, 16.f),
            FVector(-25.f,  65.f, 16.f),
            FVector( 20.f, -40.f, 16.f),
            FVector( 70.f, -60.f, 16.f),
            FVector( 45.f,  18.f, 16.f),
        };
        for (const FVector& Off : FoodOffsets)
        {
            AStaticMeshActor* Food = World->SpawnActor<AStaticMeshActor>(
                AStaticMeshActor::StaticClass(), Center + Off, FRotator::ZeroRotator);
            if (!Food)
            {
                continue;
            }
            UStaticMeshComponent* FoodMC = Food->GetStaticMeshComponent();
            FoodMC->SetMobility(EComponentMobility::Movable);
            FoodMC->SetStaticMesh(Sphere);
            FoodMC->SetCollisionEnabled(ECollisionEnabled::NoCollision);
            TintStaticMesh(FoodMC, FLinearColor(0.82f, 0.86f, 0.28f), 0.45f, 0.00f);
            // Keep food probes visibly smaller than the worm body.
            Food->SetActorScale3D(FVector(0.015f));
        }
        break;
    }
    case ENmHabitat::Terrain:
    {
        // Uneven ground: scattered blocks of varying size/height to traverse.
        // Keep the center clear so ground robots spawn visibly on open floor.
        const float ClearSpawnRadius = RadiusCm * 0.55f;
        for (int32 i = 0; i < 28; ++i)
        {
            const float A = FMath::FRandRange(0.f, 2.f * PI);
            const float R = FMath::FRandRange(ClearSpawnRadius, RadiusCm * 0.92f);
            const float H = FMath::FRandRange(20.f, 140.f);
            const float W = FMath::FRandRange(60.f, 220.f);
            const float D = FMath::FRandRange(60.f, 220.f);
            const float Shade = FMath::FRandRange(-0.07f, 0.07f);
            const FLinearColor BlockColor(0.40f + Shade, 0.36f + Shade * 0.5f, 0.27f + Shade * 0.4f);
            SpawnBox(World, Cube,
                     Center + FVector(FMath::Cos(A) * R, FMath::Sin(A) * R, H * 0.5f),
                     FRotator(0.f, FMath::FRandRange(0.f, 90.f), 0.f),
                     FVector(W, D, H),
                     BlockColor);
        }
        break;
    }
    case ENmHabitat::Room:
    {
        // Flat room: four tall thin walls + a low step platform to climb.
        const float S = RadiusCm;
        const float T = 15.f;
        const FLinearColor WallColor(0.68f, 0.72f, 0.78f);
        SpawnBox(World, Cube, Center + FVector( S, 0, WallTopZ * 0.5f), FRotator::ZeroRotator,
                 FVector(T, 2 * S, WallTopZ), WallColor);
        SpawnBox(World, Cube, Center + FVector(-S, 0, WallTopZ * 0.5f), FRotator::ZeroRotator,
                 FVector(T, 2 * S, WallTopZ), WallColor);
        SpawnBox(World, Cube, Center + FVector(0,  S, WallTopZ * 0.5f), FRotator::ZeroRotator,
                 FVector(2 * S, T, WallTopZ), WallColor);
        SpawnBox(World, Cube, Center + FVector(0, -S, WallTopZ * 0.5f), FRotator::ZeroRotator,
                 FVector(2 * S, T, WallTopZ), WallColor);
        SpawnBox(World, Cube, Center + FVector(S * 0.4f, 0, 15.f), FRotator::ZeroRotator,
                 FVector(200.f, 300.f, 30.f), FLinearColor(0.84f, 0.56f, 0.34f));
        break;
    }
    case ENmHabitat::FlightArena:
    {
        // Fruit-fly habitat: a low bowl with a cluster of rounded "fruit" for the
        // fly to explore, plus a few slender plant stalks to fly around — not the
        // old industrial poles.
        constexpr int32 NumRim = 40;
        const float RimLen = (2.f * PI * RadiusCm / NumRim) * 1.15f;
        const FLinearColor RimColor(0.28f, 0.18f, 0.24f);
        for (int32 i = 0; i < NumRim; ++i)
        {
            const float A = (2.f * PI * i) / NumRim;
            SpawnBox(World, Cube,
                     Center + FVector(FMath::Cos(A) * RadiusCm, FMath::Sin(A) * RadiusCm, 8.f),
                     FRotator(0.f, FMath::RadiansToDegrees(A) + 90.f, 0.f),
                     FVector(RimLen, 8.f, 16.f), RimColor); // low rim
        }
        // Rounded fruit mounds of varying size, clustered near the middle.
        for (int32 i = 0; i < 7; ++i)
        {
            const float A = (2.f * PI * i) / 7 + 0.3f;
            const float R = RadiusCm * FMath::FRandRange(0.1f, 0.45f);
            const float D = FMath::FRandRange(30.f, 70.f); // fruit diameter (cm)
            AStaticMeshActor* Fruit = World->SpawnActor<AStaticMeshActor>(
                AStaticMeshActor::StaticClass(),
                Center + FVector(FMath::Cos(A) * R, FMath::Sin(A) * R, D * 0.5f),
                FRotator::ZeroRotator);
            if (Fruit)
            {
                Fruit->GetStaticMeshComponent()->SetMobility(EComponentMobility::Movable);
                Fruit->GetStaticMeshComponent()->SetStaticMesh(Sphere);
                const FLinearColor FruitColor = (i % 2 == 0)
                    ? FLinearColor(0.88f, 0.72f, 0.22f)
                    : FLinearColor(0.86f, 0.35f, 0.24f);
                TintStaticMesh(Fruit->GetStaticMeshComponent(), FruitColor, 0.48f, 0.0f);
                Fruit->SetActorScale3D(FVector(D / 100.f));
            }
        }
        // A few slender plant stalks (thin, varied height) to fly around.
        for (int32 i = 0; i < 3; ++i)
        {
            const float A = (2.f * PI * i) / 3 + 1.1f;
            const float R = RadiusCm * 0.65f;
            const float H = FMath::FRandRange(200.f, 360.f);
            SpawnBox(World, Cube,
                     Center + FVector(FMath::Cos(A) * R, FMath::Sin(A) * R, H * 0.5f),
                     FRotator::ZeroRotator, FVector(6.f, 6.f, H),
                     FLinearColor(0.29f, 0.52f, 0.32f));
        }
        break;
    }
    case ENmHabitat::Tank:
    {
        // Glass aquarium: four tall thin walls + a base, water surface at WallTopZ.
        const float S = RadiusCm;
        const float T = 12.f;
        const FLinearColor GlassColor(0.34f, 0.52f, 0.62f);
        SpawnBox(World, Cube, Center + FVector( S, 0, WallTopZ * 0.5f), FRotator::ZeroRotator,
                 FVector(T, 2 * S, WallTopZ), GlassColor);
        SpawnBox(World, Cube, Center + FVector(-S, 0, WallTopZ * 0.5f), FRotator::ZeroRotator,
                 FVector(T, 2 * S, WallTopZ), GlassColor);
        SpawnBox(World, Cube, Center + FVector(0,  S, WallTopZ * 0.5f), FRotator::ZeroRotator,
                 FVector(2 * S, T, WallTopZ), GlassColor);
        SpawnBox(World, Cube, Center + FVector(0, -S, WallTopZ * 0.5f), FRotator::ZeroRotator,
                 FVector(2 * S, T, WallTopZ), GlassColor);
        SpawnBox(World, Cube, Center + FVector(0, 0, 5.f), FRotator::ZeroRotator,
                 FVector(2 * S, 2 * S, 10.f), FLinearColor(0.28f, 0.34f, 0.40f)); // base

        // Translucent water volume filling the tank (visual only — no collision so
        // the fish swims freely; buoyancy uses WaterSurfaceZ = WallTopZ).
        // Neutral translucent so the fish shows through (the engine WaterMaterial
        // rendered opaque red without a water body set up).
        UMaterialInterface* WaterMat = LoadObject<UMaterialInterface>(
            nullptr, TEXT("/Engine/EngineDebugMaterials/M_SimpleTranslucent.M_SimpleTranslucent"));
        AStaticMeshActor* Water = SpawnBox(
            World, Cube, Center + FVector(0.f, 0.f, WallTopZ * 0.5f),
            FRotator::ZeroRotator,
            FVector(2 * S - 2 * T, 2 * S - 2 * T, WallTopZ - 10.f));
        if (Water)
        {
            UStaticMeshComponent* WMC = Water->GetStaticMeshComponent();
            WMC->SetCollisionEnabled(ECollisionEnabled::NoCollision);
            if (WaterMat)
            {
                WMC->SetMaterial(0, WaterMat);
            }
            // Preserve the translucent water material while still applying a blue tint.
            TintStaticMesh(WMC, FLinearColor(0.12f, 0.34f, 0.52f), 0.20f, 0.0f, false);
        }
        break;
    }
    }
}

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
        SpawnHabitat(Cfg.Habitat, RegionCenter, Cfg.RegionRadius, Cfg.WallTopZ);

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
