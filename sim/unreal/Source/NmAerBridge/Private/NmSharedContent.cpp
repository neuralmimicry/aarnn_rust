#include "NmSharedContent.h"
#include "Robots/NmCelegansActor.h"
#include "Robots/NmZebrafishActor.h"
#include "NmSimContent.generated.inl"
#include "Dom/JsonObject.h"
#include "Serialization/JsonSerializer.h"
#include "Serialization/JsonReader.h"
#include "Engine/World.h"
#include "Engine/StaticMesh.h"
#include "Engine/Texture.h"
#include "Engine/StaticMeshActor.h"
#include "Components/StaticMeshComponent.h"
#include "Components/DirectionalLightComponent.h"
#include "Engine/DirectionalLight.h"
#include "Engine/PointLight.h"
#include "Components/PointLightComponent.h"
#include "EngineUtils.h"
#include "Materials/MaterialInstanceDynamic.h"

namespace
{
TSharedPtr<FJsonObject> Catalogue()
{
    static TSharedPtr<FJsonObject> Data;
    if (!Data)
    {
        const FString Source = UTF8_TO_TCHAR(NmContentJson);
        auto Reader = TJsonReaderFactory<>::Create(Source);
        if (!FJsonSerializer::Deserialize(Reader, Data) || !Data || Data->GetIntegerField(TEXT("schema_version")) != 1)
        {
            UE_LOG(LogTemp, Error, TEXT("NmContent: invalid generated catalogue; regenerate scripts/sim_content.py"));
            Data.Reset();
        }
    }
    return Data;
}
TSharedPtr<FJsonObject> Find(const TCHAR* Collection, const FString& Id)
{
    auto Data = Catalogue();
    if (!Data) return nullptr;
    for (const auto& Value : Data->GetArrayField(Collection))
    {
        auto Item = Value->AsObject();
        if (Item->GetStringField(TEXT("id")) == Id) return Item;
    }
    return nullptr;
}
FVector Vector(const TSharedPtr<FJsonObject>& O, const TCHAR* Name)
{
    const auto& A = O->GetArrayField(Name);
    // Authored Y-left maps to Unreal Y-right; lengths are converted separately.
    return FVector(A[0]->AsNumber(), -A[1]->AsNumber(), A[2]->AsNumber());
}
UStaticMesh* MeshFor(const FString& Shape)
{
    const TCHAR* Path = Shape == TEXT("box") ? TEXT("/Engine/BasicShapes/Cube.Cube")
        : Shape == TEXT("cylinder") ? TEXT("/Engine/BasicShapes/Cylinder.Cylinder")
        : TEXT("/Engine/BasicShapes/Sphere.Sphere");
    return LoadObject<UStaticMesh>(nullptr, Path);
}
void SetAppearance(UStaticMeshComponent* Mesh, const TSharedPtr<FJsonObject>& O)
{
    const bool bWater = O->GetStringField(TEXT("material")) == TEXT("water");
    auto Base = LoadObject<UMaterialInterface>(nullptr, bWater
        ? TEXT("/Engine/EngineMaterials/Widget3DPassThrough_Translucent.Widget3DPassThrough_Translucent")
        : TEXT("/Engine/BasicShapes/BasicShapeMaterial.BasicShapeMaterial"));
    static bool bLoggedMaterial = false;
    if (Base && !bLoggedMaterial)
    {
        TArray<FMaterialParameterInfo> Parameters;
        TArray<FGuid> Ids;
        Base->GetAllVectorParameterInfo(Parameters, Ids);
        for (const auto& Parameter : Parameters)
            UE_LOG(LogTemp, Log, TEXT("NmContent: material vector parameter=%s"), *Parameter.Name.ToString());
        bLoggedMaterial = true;
    }
    Mesh->SetMaterial(0, Base);
    auto Mat = Mesh->CreateAndSetMaterialInstanceDynamic(0);
    if (!Mat) return;
    const auto& C = O->GetArrayField(TEXT("colour"));
    const FLinearColor Colour(C[0]->AsNumber(), C[1]->AsNumber(), C[2]->AsNumber());
    Mat->SetVectorParameterValue(TEXT("Color"), Colour);
    Mat->SetVectorParameterValue(TEXT("BaseColor"), Colour);
    Mat->SetScalarParameterValue(TEXT("Roughness"), .65f);
    Mat->SetScalarParameterValue(TEXT("Metallic"), O->GetStringField(TEXT("material")) == TEXT("metal") ? .5f : 0.f);
    if (bWater)
    {
        // This engine material is compiled for translucency. The engine's
        // misleadingly named WaterMaterial is opaque and would hide the fish.
        Mat->SetTextureParameterValue(TEXT("SlateUI"), LoadObject<UTexture>(nullptr,
            TEXT("/Engine/EngineResources/WhiteSquareTexture.WhiteSquareTexture")));
        Mat->SetVectorParameterValue(TEXT("TintColorAndOpacity"), FLinearColor(Colour.R, Colour.G, Colour.B, .18f));
        Mat->SetScalarParameterValue(TEXT("OpacityFromTexture"), 1.f);
        Mesh->SetCastShadow(false);
        Mesh->SetTranslucentSortPriority(1);
    }
}
FVector Size(const TSharedPtr<FJsonObject>& O, float Scale)
{
    return Vector(O, TEXT("size")).GetAbs() * Scale / 100.f;
}
FString ProfileId(const FString& Id)
{
    if (Id.StartsWith(TEXT("drosophila_fafb"))) return TEXT("drosophila_fafb");
    if (Id.StartsWith(TEXT("drosophila"))) return TEXT("drosophila_banc");
    for (const TCHAR* Name : {TEXT("celegans"), TEXT("hexapod"), TEXT("nao"), TEXT("zebrafish")})
        if (Id.StartsWith(Name)) return Name;
    return Id;
}
}

bool NmSharedContent::SpawnHabitat(UWorld* World, const FString& Profile,
    const FVector& Centre, float RadiusCm)
{
    auto P = Find(TEXT("profiles"), ProfileId(Profile));
    auto H = P ? Find(TEXT("habitats"), P->GetStringField(TEXT("habitat"))) : nullptr;
    if (!H || !World || RadiusCm <= 0) return false;
    for (const auto& Value : H->GetArrayField(TEXT("objects")))
    {
        auto O = Value->AsObject();
        auto Actor = World->SpawnActor<AStaticMeshActor>(AStaticMeshActor::StaticClass(),
            Centre + Vector(O, TEXT("position")) * RadiusCm,
            FRotator(0, -FMath::RadiansToDegrees(O->GetNumberField(TEXT("yaw"))), 0));
        if (!Actor) return false;
        Actor->Tags.Add(FName(*("NmContent:" + O->GetStringField(TEXT("id")))));
        auto Mesh = Actor->GetStaticMeshComponent();
        Mesh->SetMobility(EComponentMobility::Movable);
        Mesh->SetStaticMesh(MeshFor(O->GetStringField(TEXT("shape"))));
        Mesh->SetCollisionEnabled(O->GetBoolField(TEXT("collision")) ? ECollisionEnabled::QueryAndPhysics : ECollisionEnabled::NoCollision);
        Mesh->SetCollisionResponseToAllChannels(ECR_Block);
        Actor->SetActorScale3D(Size(O, RadiusCm));
        SetAppearance(Mesh, O);
        if (O->GetStringField(TEXT("cue")) == TEXT("light"))
        {
            auto Beacon = World->SpawnActor<APointLight>(Centre + Vector(O, TEXT("position")) * RadiusCm, FRotator::ZeroRotator);
            if (Beacon)
            {
                auto Light = Beacon->PointLightComponent;
                Light->SetMobility(EComponentMobility::Movable);
                Light->SetLightColor(FLinearColor(.99f, .86f, .52f));
                Light->SetIntensity(600.f);
                Light->SetAttenuationRadius(RadiusCm * 3.f);
            }
        }
    }
    if (!TActorIterator<ADirectionalLight>(World))
    {
        auto Sun = World->SpawnActor<ADirectionalLight>(FVector::ZeroVector, FRotator(-55.f, -35.f, 0.f));
        if (Sun) Sun->GetLightComponent()->SetIntensity(1.f);
    }
    UE_LOG(LogTemp, Log, TEXT("NmContent: habitat=%s objects=%d digest=%s"),
        *H->GetStringField(TEXT("id")), H->GetArrayField(TEXT("objects")).Num(),
        *Catalogue()->GetStringField(TEXT("digest")));
    return true;
}

bool NmSharedContent::DressRobot(AActor* Robot, const FString& Profile, bool bAnatomyCutaway)
{
    auto P = Find(TEXT("profiles"), ProfileId(Profile));
    if (!Robot || !P) return false;
    TArray<UStaticMeshComponent*> Bodies;
    Robot->GetComponents(Bodies);
    if (Bodies.IsEmpty()) return false;
    FVector Centre, Extent;
    Robot->GetActorBounds(true, Centre, Extent);
    const FString Kind = P->GetStringField(TEXT("kind"));
    const float LengthCm = FMath::Max(1.f, (Kind == TEXT("nao") ? Extent.Z : Extent.X) * 2.f);
    if (Kind == TEXT("nao")) Centre.Z -= Extent.Z;
    FQuat VisualRotation = Robot->GetActorQuat();
    // In these existing chains segment zero is the head and the tail grows +X.
    // Align authored head-forward anatomy with actual rig endpoints, not actor yaw.
    UStaticMeshComponent* HeadBody = nullptr;
    UStaticMeshComponent* TailBody = nullptr;
    if (auto Worm = Robot->FindComponentByClass<UNmCelegansComponent>())
    {
        if (Worm->SegmentMeshes.Num() > 1) { HeadBody = Worm->SegmentMeshes[0]; TailBody = Worm->SegmentMeshes.Last(); }
    }
    if (auto Fish = Robot->FindComponentByClass<UNmZebrafishComponent>())
    {
        if (Fish->SegmentMeshes.Num() > 1) { HeadBody = Fish->SegmentMeshes[0]; TailBody = Fish->SegmentMeshes.Last(); }
    }
    if (HeadBody && TailBody)
        VisualRotation = FRotationMatrix::MakeFromXZ(HeadBody->GetComponentLocation() - TailBody->GetComponentLocation(), Robot->GetActorUpVector()).ToQuat();
    // Keep all original physics/camera/constraint bodies; only their visible skin
    // is replaced. Every new piece is attached to an existing articulated body.
    for (auto Body : Bodies) Body->SetVisibility(false, false);
    for (const auto& Value : P->GetArrayField(TEXT("parts")))
    {
        auto O = Value->AsObject();
        if (O->GetBoolField(TEXT("internal")) && !bAnatomyCutaway) continue;
        const FString Id = O->GetStringField(TEXT("id"));
        if (bAnatomyCutaway && (Id.StartsWith(TEXT("cuticle_")) || Id.StartsWith(TEXT("myomere_")))) continue;
        const FVector Position = Centre + VisualRotation.RotateVector(Vector(O, TEXT("position")) * LengthCm);
        UStaticMeshComponent* Anchor = Bodies[0];
        double Best = TNumericLimits<double>::Max();
        for (auto Body : Bodies)
        {
            const double Distance = FVector::DistSquared(Position, Body->GetComponentLocation());
            if (Distance < Best) { Anchor = Body; Best = Distance; }
        }
        const FName Name(*("NmVisual_" + O->GetStringField(TEXT("id"))));
        auto Mesh = NewObject<UStaticMeshComponent>(Robot, Name);
        Robot->AddInstanceComponent(Mesh);
        Mesh->SetMobility(EComponentMobility::Movable);
        Mesh->SetStaticMesh(MeshFor(O->GetStringField(TEXT("shape"))));
        Mesh->SetCollisionEnabled(ECollisionEnabled::NoCollision);
        Mesh->SetSimulatePhysics(false);
        Mesh->RegisterComponent();
        Mesh->SetWorldLocation(Position);
        Mesh->SetWorldRotation(VisualRotation * FQuat(FVector::UpVector, -O->GetNumberField(TEXT("yaw"))));
        Mesh->SetWorldScale3D(Size(O, LengthCm));
        Mesh->AttachToComponent(Anchor, FAttachmentTransformRules::KeepWorldTransform);
        SetAppearance(Mesh, O);
    }
    UE_LOG(LogTemp, Log, TEXT("NmContent: visual profile=%s parts=%d (physics rig preserved)"),
        *Profile, P->GetArrayField(TEXT("parts")).Num());
    return true;
}

bool NmSharedContent::Channels(const FString& Profile, bool bOutputs, TArray<FString>& Names)
{
    auto P = Find(TEXT("profiles"), ProfileId(Profile));
    if (!P) return false;
    Names.Reset();
    for (const auto& V : P->GetArrayField(bOutputs ? TEXT("output_names") : TEXT("sensor_names"))) Names.Add(V->AsString());
    return !Names.IsEmpty();
}
float NmSharedContent::SampleField(const FString& Profile, const FString& Cue,
    const FVector& Position, const FVector& Centre, float RadiusCm)
{
    auto P = Find(TEXT("profiles"), ProfileId(Profile));
    auto H = P ? Find(TEXT("habitats"), P->GetStringField(TEXT("habitat"))) : nullptr;
    if (!H || RadiusCm <= 0) return 0.f;
    const FVector Local = (Position - Centre) / RadiusCm;
    float Sum = 0;
    for (const auto& V : H->GetArrayField(TEXT("objects")))
    {
        auto O = V->AsObject();
        const float Radius = O->GetNumberField(TEXT("radius"));
        if (O->GetStringField(TEXT("cue")) != Cue || Radius <= 0) continue;
        const float D2 = FVector::DistSquared(Local, Vector(O, TEXT("position"))) / (Radius * Radius);
        Sum += O->GetNumberField(TEXT("strength")) / (1 + 4 * D2);
    }
    return FMath::Clamp(Sum, 0.f, 1.f);
}
