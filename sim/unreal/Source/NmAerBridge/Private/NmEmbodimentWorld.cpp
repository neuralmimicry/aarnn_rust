#include "NmEmbodimentWorld.h"
#include "Camera/CameraActor.h"
#include "Camera/CameraComponent.h"
#include "Components/DirectionalLightComponent.h"
#include "Components/StaticMeshComponent.h"
#include "Dom/JsonObject.h"
#include "Engine/DirectionalLight.h"
#include "Engine/StaticMesh.h"
#include "Engine/StaticMeshActor.h"
#include "Engine/World.h"
#include "GameFramework/WorldSettings.h"
#include "GameFramework/PlayerController.h"
#include "HAL/PlatformMisc.h"
#include "HAL/PlatformProcess.h"
#include "Materials/MaterialInstanceDynamic.h"
#include "Misc/App.h"
#include "Misc/FileHelper.h"
#include "Misc/Paths.h"
#include "PhysicalMaterials/PhysicalMaterial.h"
#include "Physics/PhysicsInterfaceCore.h"
#include "Physics/PhysicsInterfaceDeclares.h"
#include "Physics/PhysicsInterfaceUtils.h"
#include "PhysicsEngine/BodyInstance.h"
#include "PhysicsEngine/PhysicsConstraintComponent.h"
#include "PhysicsEngine/PhysicsSettings.h"
#include "ProceduralMeshComponent.h"
#include "Serialization/JsonReader.h"
#include "Serialization/JsonSerializer.h"
#include "UnrealClient.h"
#include "nm_embodiment.h"
#include <cmath>

namespace {
using O = TSharedPtr<FJsonObject>;
O Read(const FString &Path) {
  FString Text;
  O Data;
  if (!FFileHelper::LoadFileToString(Text, *Path) ||
      !FJsonSerializer::Deserialize(TJsonReaderFactory<>::Create(Text), Data) ||
      !Data)
    throw std::runtime_error("invalid scene/graph JSON");
  return Data;
}
FVector Raw(const TArray<TSharedPtr<FJsonValue>> &A) {
  return FVector(A[0]->AsNumber(), A[1]->AsNumber(), A[2]->AsNumber());
}
FVector Raw(O D, const TCHAR *K) { return Raw(D->GetArrayField(K)); }
FVector Polar(FVector V) { return FVector(V.X, -V.Y, V.Z); }
FVector Point(O D, const TCHAR *K) { return 100. * Polar(Raw(D, K)); }
FQuat Rotation(O D, const TCHAR *K) {
  const auto &A = D->GetArrayField(K);
  return FQuat(-A[1]->AsNumber(), A[2]->AsNumber(), -A[3]->AsNumber(),
               A[0]->AsNumber());
}
FString Json(O D) {
  FString S;
  FJsonSerializer::Serialize(D.ToSharedRef(), TJsonWriterFactory<>::Create(&S));
  return S;
}
TArray<FVector> GeometrySection(AActor *Owner, UProceduralMeshComponent *C, int32 Section, O D, O Mesh) {
  const auto &Vert = Mesh->GetArrayField(TEXT("vertices_xyz"));
  const auto &Face = Mesh->GetArrayField(TEXT("triangles_ijk"));
  FTransform T(Rotation(D, TEXT("quaternion_wxyz")),
       Point(D, TEXT("position")));
  TArray<FVector> Vertices;
  TArray<int32> Triangles;
  for (int32 K = 0; K < Vert.Num(); K += 3)
    Vertices.Add(T.TransformPosition(
        Polar(FVector(Vert[K]->AsNumber(), Vert[K + 1]->AsNumber(),
      Vert[K + 2]->AsNumber())) *
        100.));
  for (int32 K = 0; K < Face.Num(); K += 3) {
    Triangles.Add(Face[K]->AsNumber());
    Triangles.Add(Face[K + 2]->AsNumber());
    Triangles.Add(Face[K + 1]->AsNumber());
  }
  auto &Rgba = D->GetArrayField(TEXT("rgba"));
  FLinearColor Colour(Rgba[0]->AsNumber(), Rgba[1]->AsNumber(),
      Rgba[2]->AsNumber(), Rgba[3]->AsNumber());
  TArray<FLinearColor> Colours;
  Colours.Init(Colour, Vertices.Num());
  C->CreateMeshSection_LinearColor(
      Section, Vertices, Triangles, TArray<FVector>(),
      TArray<FVector2D>(), Colours, TArray<FProcMeshTangent>(), false);
  auto *Material = UMaterialInstanceDynamic::Create(
      LoadObject<UMaterialInterface>(
          nullptr, TEXT("/Engine/BasicShapes/"
        "BasicShapeMaterial.BasicShapeMaterial")),
      Owner);
  Material->SetVectorParameterValue(TEXT("Color"), Colour);
  C->SetMaterial(Section, Material);
  return Vertices;
}
double Clamp(double V, double L, double H) { return FMath::Clamp(V, L, H); }
struct FApi {
  void *Library = nullptr;
#define NM_FN(Name) decltype(&Name) Name = nullptr
  NM_FN(nm_embodiment_version);
  NM_FN(nm_embodiment_error);
  NM_FN(nm_embodiment_verify_graph);
  NM_FN(nm_reference_open);
  NM_FN(nm_control_mode);
  NM_FN(nm_reference_close);
  NM_FN(nm_reference_disarm);
  NM_FN(nm_reference_reset);
  NM_FN(nm_reference_step);
  NM_FN(nm_reference_motor_mask);
  NM_FN(nm_reference_status);
  NM_FN(nm_reference_project);
  NM_FN(nm_panel_forces);
#undef NM_FN
  void Load(const FString &Path) {
    Library = FPlatformProcess::GetDllHandle(*Path);
    if (!Library)
      throw std::runtime_error("shared Rust embodiment library unavailable");
#define NM_LOAD(Name)                                                          \
  Name = reinterpret_cast<decltype(Name)>(                                     \
      FPlatformProcess::GetDllExport(Library, TEXT(#Name)));                   \
  if (!Name)                                                                   \
  throw std::runtime_error("missing embodiment ABI export")
    NM_LOAD(nm_embodiment_version);
    NM_LOAD(nm_embodiment_error);
    NM_LOAD(nm_embodiment_verify_graph);
    NM_LOAD(nm_reference_open);
    NM_LOAD(nm_control_mode);
    NM_LOAD(nm_reference_close);
    NM_LOAD(nm_reference_disarm);
    NM_LOAD(nm_reference_reset);
    NM_LOAD(nm_reference_step);
    NM_LOAD(nm_reference_motor_mask);
    NM_LOAD(nm_reference_status);
    NM_LOAD(nm_reference_project);
    NM_LOAD(nm_panel_forces);
#undef NM_LOAD
    if (nm_embodiment_version() != 1)
      throw std::runtime_error("embodiment ABI version");
  }
  void Check(int32 Code) {
    if (Code < 0) {
      uint8 B[4097] = {};
      int32 N = nm_embodiment_error(B, 4096);
      throw std::runtime_error(N < 0 ? "native ABI error"
                                     : TCHAR_TO_UTF8(UTF8_TO_TCHAR(
                                           reinterpret_cast<const char *>(B))));
    }
  }
  ~FApi() {
    if (Library)
      FPlatformProcess::FreeDllHandle(Library);
  }
};
struct FJoint {
  O Data, Target;
  UProceduralMeshComponent *Parent = nullptr, *Child = nullptr;
  double Q = 0, V = 0, LastAngle = 0;
  bool bFirst = true;
  FTransform ParentFrame() const {
    return FTransform(Rotation(Data, TEXT("parent_quaternion_wxyz")),
                      Point(Data, TEXT("parent_position")));
  }
  FTransform ChildFrame() const {
    return FTransform(Rotation(Data, TEXT("child_quaternion_wxyz")),
                      Point(Data, TEXT("child_position")));
  }
  bool Linear() const { return Data->GetIntegerField(TEXT("type")) == 2; }
  FVector Axis() const {
    return Parent->GetComponentQuat().RotateVector(
               ParentFrame().GetRotation().RotateVector(
                   FVector::ForwardVector)) *
           (Linear() ? 1. : -1.);
  }
  void Measure() {
    const FVector A = Axis();
    if (Linear()) {
      const FVector P = Parent->GetComponentTransform().TransformPosition(
                        ParentFrame().GetLocation()),
                    C = Child->GetComponentTransform().TransformPosition(
                        ChildFrame().GetLocation());
      Q = FVector::DotProduct(C - P, A) / 100.;
      V = FVector::DotProduct(Child->GetPhysicsLinearVelocityAtPoint(C) -
                                  Parent->GetPhysicsLinearVelocityAtPoint(P),
                              A) /
          100.;
    } else {
      const FQuat D =
          (Parent->GetComponentQuat() * ParentFrame().GetRotation()).Inverse() *
          (Child->GetComponentQuat() * ChildFrame().GetRotation());
      const double Angle = -2. * std::atan2(D.X, D.W);
      if (bFirst) {
        Q = Angle;
        bFirst = false;
      } else
        Q += FMath::UnwindRadians(Angle - LastAngle);
      LastAngle = Angle;
      V = FVector::DotProduct(Child->GetPhysicsAngularVelocityInRadians() -
                                  Parent->GetPhysicsAngularVelocityInRadians(),
                              A);
    }
  }
  void Effort(double E) {
    if (!std::isfinite(E))
      throw std::runtime_error("nonfinite effort");
    const FVector F = Axis() * E;
    if (Linear()) {
      Child->AddForceAtLocation(
          F * 100., Child->GetComponentTransform().TransformPosition(
                        ChildFrame().GetLocation()));
      Parent->AddForceAtLocation(
          -F * 100., Parent->GetComponentTransform().TransformPosition(
                         ParentFrame().GetLocation()));
    } else {
      Child->AddTorqueInRadians(F * 10000.);
      Parent->AddTorqueInRadians(-F * 10000.);
    }
  }
};
struct FInstance {
  O Graph, Entry;
  TMap<int32, UProceduralMeshComponent *> Bodies;
  TMap<FString, FJoint> Joints;
  TMap<FString, int32> Actuators;
  TArray<uint8> MotorMask;
  TArray<double> Controls, Sensors, Measurements;
  TSet<UPrimitiveComponent *> Contacts;
  FApi *Api = nullptr;
  uint64_t Handle = 0;
  double Energy = 20000.;
  bool bCrow = false, bLegacy = false, bNpcActive = false;
  uint64 ActiveSteps = 0;
  double InitialMechanicalEnergy = 0, MaxMechanicalEnergy = 0;
  FVector InitialRoot, PreviousHeadVelocity = FVector::ZeroVector;
  UProceduralMeshComponent *Root() const {
    return Bodies.FindChecked(
        Graph->GetArrayField(TEXT("bodies"))[0]->AsObject()->GetIntegerField(
            TEXT("id")));
  }
  // SI rigid-body kinetic + gravitational + declared coupling energy.
  // A silent run may dissipate energy, but solver/contact injection is a
  // separate failing calibration check, never evidence of locomotion.
  double MechanicalEnergy() const {
    double E = 0;
    for (const auto &V : Graph->GetArrayField(TEXT("bodies"))) {
      O B = V->AsObject();
      auto *C = Bodies.FindChecked(B->GetIntegerField(TEXT("id")));
      const double M = B->GetNumberField(TEXT("mass_kg"));
      FVector Linear = C->GetPhysicsLinearVelocity() / 100.;
      FQuat Axes = C->GetComponentQuat() * Rotation(B, TEXT("principal_axes_wxyz"));
      FVector Angular = Axes.UnrotateVector(C->GetPhysicsAngularVelocityInRadians());
      FVector Inertia = Raw(B, TEXT("principal_inertia_kgm2"));
      E += M * (9.81 * C->GetCenterOfMass().Z / 100. + .5 * Linear.SizeSquared());
      E += .5 * FVector::DotProduct(Inertia, Angular * Angular);
    }
    for (const auto &V : Graph->GetArrayField(TEXT("constraints"))) {
      O C = V->AsObject();
      double D = Joints.FindChecked(C->GetStringField(TEXT("joint1"))).Q -
                 C->GetNumberField(TEXT("ratio")) * Joints.FindChecked(C->GetStringField(TEXT("joint2"))).Q -
                 C->GetNumberField(TEXT("offset"));
      E += .5 * C->GetNumberField(TEXT("stiffness")) * D * D;
    }
    if (!std::isfinite(E)) throw std::runtime_error("nonfinite mechanical energy");
    return E;
  }
  UProceduralMeshComponent *Named(const FString &Name) const {
    for (const auto &V : Graph->GetArrayField(TEXT("source_body_owners"))) {
      O B = V->AsObject();
      if (B->GetStringField(TEXT("name")) == Name)
        return Bodies.FindChecked(B->GetIntegerField(TEXT("owner")));
    }
    throw std::runtime_error("named body absent");
  }
  void Disarm() {
    for (double &C : Controls)
      C = 0;
    for (uint8 &M : MotorMask)
      M = 0;
    if (Handle)
      Api->Check(Api->nm_reference_disarm(Handle));
  }
  void Close() {
    if (Handle) {
      Api->nm_reference_disarm(Handle);
      Api->nm_reference_close(Handle);
      Handle = 0;
    }
  }
  void Reset() {
    Disarm();
    Api->Check(Api->nm_reference_reset(Handle));
    FVector Offset = Point(Entry, TEXT("offset_m"));
    for (const auto &V : Graph->GetArrayField(TEXT("bodies"))) {
      O B = V->AsObject();
      auto *C = Bodies.FindChecked(B->GetIntegerField(TEXT("id")));
      C->SetWorldLocationAndRotation(Offset + Point(B, TEXT("world_position")),
          Rotation(B, TEXT("world_quaternion_wxyz")), false, nullptr, ETeleportType::TeleportPhysics);
      C->SetPhysicsLinearVelocity(FVector::ZeroVector);
      C->SetPhysicsAngularVelocityInRadians(FVector::ZeroVector);
      if (auto *Body = C->GetBodyInstance()) { Body->ClearForces(); Body->ClearTorques(); }
    }
    for (auto &J : Joints) { J.Value.Q = J.Value.V = J.Value.LastAngle = 0; J.Value.bFirst = true; }
    Contacts.Empty();
    PreviousHeadVelocity = FVector::ZeroVector;
    Energy = 20000.;
  }
  ~FInstance() { Close(); }
  double Ray(UWorld *World, FVector P, FVector D, bool Light) {
    FCollisionQueryParams Params;
    for (auto &Pair : Bodies)
      Params.AddIgnoredComponent(Pair.Value);
    FHitResult Hit;
    if (!World->LineTraceSingleByChannel(Hit, P, P + D.GetSafeNormal() * 300.,
                                         ECC_Visibility, Params))
      return Light ? .5 : 3.;
    if (!Light)
      return Hit.Distance / 100.;
    FLinearColor Colour(.5, .5, .5);
    if (auto *C = Hit.GetComponent())
      if (auto *M = C->GetMaterial(0))
        M->GetVectorParameterValue(FMaterialParameterInfo(TEXT("Color")),
                                   Colour);
    return Clamp(.2126 * Colour.R + .7152 * Colour.G + .0722 * Colour.B, 0.,
                 1.);
  }
  void Capture(UWorld *World, uint64 Step) {
    if (bLegacy) {
      ControlStep(Step);
      return;
    }
    for (auto &Pair : Joints)
      Pair.Value.Measure();
    for (auto &A : Actuators) {
      auto &J = Joints.FindChecked(A.Key);
      Measurements[2 * A.Value] = J.Q;
      Measurements[2 * A.Value + 1] = J.V;
    }
    int32 N = Actuators.Num() * 2;
    auto *Head = bCrow ? Named(TEXT("neck_roll_link")) : Root();
    FVector Gyro = -Polar(Head->GetComponentQuat().UnrotateVector(
        Head->GetPhysicsAngularVelocityInRadians()));
    for (int32 I = 0; I < 3; I++)
      Measurements[N + I] = Gyro[I];
    for (int32 I = 0; I < 8; I++) {
      double A = 2. * PI * I / 8.;
      Measurements[N + 3 + I] =
          Ray(World,
              Root()->GetComponentLocation() +
                  Root()->GetComponentQuat().RotateVector(FVector(0, 0, 10)),
              Root()->GetComponentQuat().RotateVector(
                  Polar(FVector(std::cos(A), std::sin(A), 0))),
              false);
    }
    if (bCrow) {
      int32 K = N + 11;
      for (const TCHAR *Side : {TEXT("retina_left"), TEXT("retina_right")})
        for (const auto &V : Graph->GetArrayField(TEXT("cameras"))) {
          O C = V->AsObject();
          if (C->GetStringField(TEXT("name")) != Side)
            continue;
          auto *B = Bodies.FindChecked(C->GetIntegerField(TEXT("owner")));
          FVector P = B->GetComponentTransform().TransformPosition(
              Point(C, TEXT("position")));
          FQuat R =
              B->GetComponentQuat() * Rotation(C, TEXT("quaternion_wxyz"));
          for (int32 Y = 0; Y < 8; Y++)
            for (int32 X = 0; X < 12; X++)
              Measurements[K++] =
                  Ray(World, P,
                      R.RotateVector(
                          Polar(FVector((X - 5.5) / 8., (3.5 - Y) / 8., -1))),
                      true);
        }
      FVector Velocity = Head->GetPhysicsLinearVelocity();
      FVector Accel = Polar(Head->GetComponentQuat().UnrotateVector(
                          (Velocity - PreviousHeadVelocity) / .002 -
                          FVector(0, 0, -981.))) /
                      100.;
      PreviousHeadVelocity = Velocity;
      FVector Gravity =
          Polar(Head->GetComponentQuat().UnrotateVector(FVector(0, 0, -1)));
      for (int32 I = 0; I < 3; I++)
        Measurements[K++] = Accel[I];
      for (int32 I = 0; I < 3; I++)
        Measurements[K++] = Gravity[I];
      for (const TCHAR *Name : {TEXT("beak_gape_link"), TEXT("beak_gape_link"),
                                TEXT("left_leg_front_toes_curl_link"),
                                TEXT("left_leg_front_toes_curl_link"),
                                TEXT("left_leg_front_toes_curl_link"),
                                TEXT("left_leg_hallux_curl_link"),
                                TEXT("right_leg_front_toes_curl_link"),
                                TEXT("right_leg_front_toes_curl_link"),
                                TEXT("right_leg_front_toes_curl_link"),
                                TEXT("right_leg_hallux_curl_link"),
                                TEXT("left_wing_wrist_twist_link"),
                                TEXT("right_wing_wrist_twist_link")})
        Measurements[K++] = Contacts.Contains(Named(Name)) ? 1. : 0.;
      for (int32 Index = 0; Index < 16; Index++)
        Measurements[K++] = Index % 8 == 2 ? Controls[Controls.Num() - 2] : 0.;
      FVector Air = Polar(Root()->GetComponentQuat().UnrotateVector(
                        Root()->GetPhysicsLinearVelocity())) /
                    100.;
      for (int32 Index = 0; Index < 3; Index++)
        Measurements[K++] = Air[Index];
      Measurements[K++] = 1. - Energy / 20000.;
      Measurements[K++] = 0;
      Measurements[K++] = 1. - Energy / 20000.;
      Measurements[K++] = Energy / 20000.;
      Measurements[K++] = 0;
      Measurements[K++] = 0;
      if (K != Measurements.Num())
        throw std::runtime_error("crow measured projection width");
    }
    Api->Check(Api->nm_reference_project(Handle, Measurements.GetData(),
                                         Measurements.Num(), Sensors.GetData(),
                                         Sensors.Num()));
    ControlStep(Step);
  }
  void ControlStep(uint64 Step) {
    const int32 Code =
        Api->nm_reference_step(Handle, Step, Sensors.GetData(), Sensors.Num(),
                               Controls.GetData(), Controls.Num());
    Api->Check(Code);
    bNpcActive = Code == 2;
    Api->Check(Api->nm_reference_motor_mask(Handle, MotorMask.GetData(),
                                            MotorMask.Num()));
  }
  void Prepare(O Scene, uint64 Step) {
    Contacts.Empty();
    TMap<FString, double> Drives;
    double Power = 0;
    for (auto &Pair : Joints) {
      FJoint &J = Pair.Value;
      J.Measure();
      double Drive = 0;
      if (auto *Id = Actuators.Find(Pair.Key); Id && MotorMask[*Id] != 0) {
        O T = J.Target;
        auto &Bias = T->GetArrayField(TEXT("bias"));
        auto &Range = T->GetArrayField(TEXT("force_range"));
        Drive = Clamp(T->GetNumberField(TEXT("gain")) * Controls[*Id] +
                          Bias[0]->AsNumber() + Bias[1]->AsNumber() * J.Q +
                          Bias[2]->AsNumber() * J.V,
                      Range[0]->AsNumber(), Range[1]->AsNumber()) *
                T->GetNumberField(TEXT("gear"));
        Power += std::abs(Drive * J.V);
      }
      Drives.Add(Pair.Key, Drive);
    }
    const double Scale = bCrow ? FMath::Min(1., 25. / FMath::Max(25., Power)) *
                                     FMath::Min(1., Energy / 2000.)
                               : 1.;
    for (auto &Pair : Joints) {
      auto &J = Pair.Value;
      J.Effort(Scale * Drives[Pair.Key] -
               J.Target->GetNumberField(TEXT("damping")) * J.V);
    }
    if (bCrow) {
      Energy = FMath::Max(
          0., Energy - (Power * Scale / (.20 + .05 * Controls.Last()) + 1. +
                        2. * Controls[Controls.Num() - 2]) *
                           .002);
      if (Energy == 0)
        Disarm();
    }
    for (const auto &V : Graph->GetArrayField(TEXT("constraints"))) {
      O C = V->AsObject();
      FJoint &A = Joints.FindChecked(C->GetStringField(TEXT("joint1")));
      FJoint &B = Joints.FindChecked(C->GetStringField(TEXT("joint2")));
      double R = C->GetNumberField(TEXT("ratio"));
      double L = C->GetNumberField(TEXT("stiffness")) *
                     (A.Q - R * B.Q - C->GetNumberField(TEXT("offset"))) +
                 C->GetNumberField(TEXT("damping")) * (A.V - R * B.V);
      A.Effort(-L);
      B.Effort(R * L);
    }
    FString Diagnostic;
    Scene->TryGetStringField(TEXT("diagnostic_joint"), Diagnostic);
    if (!Diagnostic.IsEmpty() &&
        Step < static_cast<uint64>(
                   Scene->GetIntegerField(TEXT("diagnostic_steps"))))
      Joints.FindChecked(Diagnostic)
          .Effort(Scene->GetNumberField(TEXT("diagnostic_effort")));
    if (bCrow) {
      struct Panel {
        const TCHAR *Name;
        FVector Point;
        double Area;
      };
      for (const Panel &P :
           {Panel{TEXT("left_wing_wrist_twist_link"), FVector(0, .06, 0), .045},
            Panel{TEXT("right_wing_wrist_twist_link"), FVector(0, -.06, 0),
                  .045},
            Panel{TEXT("tail_fan_link"), FVector(-.03, 0, 0), .008}}) {
        auto *B = Named(P.Name);
        FVector Point =
            B->GetComponentTransform().TransformPosition(Polar(P.Point) * 100.);
        FVector V = Polar(B->GetPhysicsLinearVelocityAtPoint(Point)) / 100.;
        FQuat R = B->GetComponentQuat();
        FVector X = Polar(R.RotateVector(Polar(FVector::ForwardVector))),
                Y = Polar(R.RotateVector(Polar(FVector::RightVector))),
                Z = Polar(R.RotateVector(Polar(FVector::UpVector)));
        double Input[13] = {V.X, V.Y, V.Z, X.X, Y.X, Z.X,   X.Y,
                            Y.Y, Z.Y, X.Z, Y.Z, Z.Z, P.Area},
               Output[3] = {};
        Api->Check(Api->nm_panel_forces(Input, 1, Output, 3));
        B->AddForceAtLocation(
            Polar(FVector(Output[0], Output[1], Output[2])) * 100., Point);
      }
    }
    for (const auto &Pair : Drives)
      if (std::abs(Pair.Value * Scale) > 1e-9) {
        if (!bNpcActive)
          ActiveSteps++;
        break;
      }
  }
};
} // namespace
struct ANmEmbodimentWorld::FImpl {
  FApi Api;
  O Scene;
  TArray<TUniquePtr<FInstance>> Instances;
  uint64 Step = 0, Iterations = 0, Epoch = 0;
  bool bFailed = false;
  bool bAwaitCapture = false;
  void Stop() {
    for (auto &I : Instances)
      I->Disarm();
  }
};
void ANmEmbodimentWorld::FImplDelete::operator()(FImpl *P) const { delete P; }
ANmEmbodimentWorld::ANmEmbodimentWorld() {
  PrimaryActorTick.bCanEverTick = true;
  PrimaryActorTick.TickGroup = TG_PrePhysics;
  Impl.Reset(new FImpl);
}
ANmEmbodimentWorld::~ANmEmbodimentWorld() = default;
bool ANmEmbodimentWorld::Configure(const FString &Manifest) {
  try {
    Impl->Scene = Read(Manifest);
    O Scene = Impl->Scene;
    if (Scene->GetIntegerField(TEXT("schema_version")) != 1 ||
        Scene->GetIntegerField(TEXT("dt_us")) != 2000)
      throw std::runtime_error("native scene version/clock");
    Impl->Api.Load(Scene->GetStringField(TEXT("rust_library")));
    FApp::SetUseFixedTimeStep(true);
    FApp::SetFixedDeltaTime(.002);
    UPhysicsSettings *Settings = UPhysicsSettings::Get();
    Settings->bSubstepping = false;
    Settings->bTickPhysicsAsync = false;
    GetWorld()->GetWorldSettings()->bGlobalGravitySet = true;
    GetWorld()->GetWorldSettings()->GlobalGravityZ = -981.;
    const auto &Entries = Scene->GetArrayField(TEXT("instances"));
    if (Entries.Num() < 1 || Entries.Num() > 16)
      throw std::runtime_error("bounded native population required");
    for (const auto &E : Entries) {
      auto State = MakeUnique<FInstance>();
      FInstance &I = *State;
      I.Api = &Impl->Api;
      I.Entry = E->AsObject();
      FString Model = I.Entry->GetStringField(TEXT("model")),
              GraphPath = I.Entry->GetStringField(TEXT("graph"));
      FTCHARToUTF8 M(*Model), G(*GraphPath);
      I.Api->Check(I.Api->nm_embodiment_verify_graph(
          reinterpret_cast<const uint8 *>(M.Get()), M.Length(),
          reinterpret_cast<const uint8 *>(G.Get()), G.Length()));
      I.Graph = Read(GraphPath);
      I.bCrow = I.Graph->GetStringField(TEXT("profile_id")) ==
                TEXT("crow_corvus_corone");
      I.bLegacy = !I.Graph->GetBoolField(TEXT("standard_projection"));
      TMap<int32, O> Meshes;
      for (auto &V : I.Graph->GetArrayField(TEXT("meshes"))) {
        O D = V->AsObject();
        Meshes.Add(D->GetIntegerField(TEXT("id")), D);
      }
      FVector Offset = Point(I.Entry, TEXT("offset_m"));
      for (const auto &V : I.Graph->GetArrayField(TEXT("bodies"))) {
        O B = V->AsObject();
        int32 Id = B->GetIntegerField(TEXT("id"));
        auto *C = NewObject<UProceduralMeshComponent>(
            this,
            *FString::Printf(TEXT("%s_%s"),
                             *I.Entry->GetStringField(TEXT("instance_id")),
                             *B->GetStringField(TEXT("name"))));
        AddInstanceComponent(C);
        C->SetMobility(EComponentMobility::Movable);
        C->bUseComplexAsSimpleCollision = false;
        C->bUseAsyncCooking = false;
        C->SetWorldLocationAndRotation(
            Offset + Point(B, TEXT("world_position")),
            Rotation(B, TEXT("world_quaternion_wxyz")));
        C->SetCollisionObjectType(ECC_PhysicsBody);
        C->SetCollisionEnabled(ECollisionEnabled::QueryAndPhysics);
        C->SetCollisionResponseToAllChannels(ECR_Block);
        I.Bodies.Add(Id, C);
        TArray<TArray<FVector>> Hulls;
        int32 Section = 0;
        TArray<FVector> Fallback;
        for (const auto &W : I.Graph->GetArrayField(TEXT("geoms"))) {
          O D = W->AsObject();
          if (D->GetIntegerField(TEXT("owner")) != Id)
            continue;
          O Mesh = Meshes.FindChecked(D->GetIntegerField(TEXT("render_mesh")));
          TArray<FVector> Vertices=GeometrySection(this,C,Section++,D,Mesh);
          if (D->GetBoolField(TEXT("collision")))
            Hulls.Add(Vertices);
          if (Fallback.IsEmpty())
            Fallback = Vertices;
        }
        bool NoCollision = Hulls.IsEmpty();
        if (NoCollision && !Fallback.IsEmpty())
          Hulls.Add(Fallback);
        if (NoCollision)
          C->SetCollisionResponseToAllChannels(ECR_Ignore);
        C->SetCollisionConvexMeshes(Hulls);
        auto *Material = NewObject<UPhysicalMaterial>(this);
        Material->Friction = .8;
        Material->Restitution = 0;
        C->SetPhysMaterialOverride(Material);
        C->SetNotifyRigidBodyCollision(true);
        C->OnComponentHit.AddDynamic(this, &ANmEmbodimentWorld::Contact);
        C->RegisterComponent();
        C->SetSimulatePhysics(!B->GetBoolField(TEXT("anchored")));
        C->SetEnableGravity(true);
        C->SetLinearDamping(0);
        C->SetAngularDamping(0);
        C->SetMassOverrideInKg(NAME_None, B->GetNumberField(TEXT("mass_kg")),
                               true);
        if (FBodyInstance *Body = C->GetBodyInstance())
        {
          Body->SetOverrideIterationCounts(true);
          Body->SetPositionSolverIterationCount(16);
          Body->SetVelocitySolverIterationCount(8);
          FPhysicsCommand::ExecuteWrite(
              Body->GetPhysicsActorHandle(),
              [&](const FPhysicsActorHandle &Actor) {
                FPhysicsInterface::SetMass_AssumesLocked(
                    Actor, B->GetNumberField(TEXT("mass_kg")));
                FPhysicsInterface::SetMassSpaceInertiaTensor_AssumesLocked(
                    Actor, Raw(B, TEXT("principal_inertia_kgm2")) * 10000.);
                FPhysicsInterface::SetComLocalPose_AssumesLocked(
                    Actor, FTransform(Rotation(B, TEXT("principal_axes_wxyz")),
                                      Point(B, TEXT("com_m"))));
              });
        }
      }
      // Canonical scenery is static and independent of body drop height.
      FVector SceneryOffset=Offset;SceneryOffset.Z=0;
      for(const auto &V:I.Graph->GetArrayField(TEXT("world_geoms"))) {
        O D=V->AsObject();
        if(D->GetArrayField(TEXT("rgba"))[3]->AsNumber()<.5) continue; // Aquatic translucency awaits a reviewed native material; no opaque substitute.
        auto *C=NewObject<UProceduralMeshComponent>(this,*FString::Printf(TEXT("%s_scene_%d"),*I.Entry->GetStringField(TEXT("instance_id")),D->GetIntegerField(TEXT("id"))));
        AddInstanceComponent(C);C->SetMobility(EComponentMobility::Static);
        C->SetWorldLocation(SceneryOffset);C->bUseAsyncCooking=false;
        TArray<FVector> Vertices=GeometrySection(this,C,0,D,Meshes.FindChecked(D->GetIntegerField(TEXT("render_mesh"))));
        C->SetCollisionObjectType(ECC_WorldStatic);
        C->SetCollisionConvexMeshes({Vertices});
        C->SetCollisionEnabled(D->GetBoolField(TEXT("collision"))?ECollisionEnabled::QueryAndPhysics:ECollisionEnabled::QueryOnly);
        C->SetCollisionResponseToAllChannels(D->GetBoolField(TEXT("collision"))?ECR_Block:ECR_Ignore);
        C->SetCollisionResponseToChannel(ECC_Visibility,ECR_Block);
        auto *Material=NewObject<UPhysicalMaterial>(this);Material->Friction=.8;Material->Restitution=0;
        C->SetPhysMaterialOverride(Material);C->RegisterComponent();C->SetSimulatePhysics(false);
      }
      TMap<FString, O> Targets;
      for (auto &V : I.Graph->GetArrayField(TEXT("targets"))) {
        O T = V->AsObject();
        Targets.Add(T->GetStringField(TEXT("joint")), T);
      }
      for (auto &V : I.Graph->GetArrayField(TEXT("joints"))) {
        O J = V->AsObject();
        int32 Type = J->GetIntegerField(TEXT("type"));
        if (Type != 2 && Type != 3)
          throw std::runtime_error("joint type requires native implementation");
        FString Name = J->GetStringField(TEXT("name"));
        FJoint StateJoint;
        StateJoint.Data = J;
        StateJoint.Target = Targets.FindChecked(Name);
        StateJoint.Parent =
            I.Bodies.FindChecked(J->GetIntegerField(TEXT("parent")));
        StateJoint.Child =
            I.Bodies.FindChecked(J->GetIntegerField(TEXT("child")));
        I.Joints.Add(Name, StateJoint);
        auto *C = NewObject<UPhysicsConstraintComponent>(this);
        AddInstanceComponent(C);
        C->RegisterComponent();
        C->SetLinearXLimit(LCM_Locked, 0);
        C->SetLinearYLimit(LCM_Locked, 0);
        C->SetLinearZLimit(LCM_Locked, 0);
        C->SetAngularTwistLimit(ACM_Locked, 0);
        C->SetAngularSwing1Limit(ACM_Locked, 0);
        C->SetAngularSwing2Limit(ACM_Locked, 0);
        FTransform P = StateJoint.ParentFrame(),
                   Child = StateJoint.ChildFrame();
        auto &Limits = J->GetArrayField(TEXT("range"));
        bool Limited = J->GetBoolField(TEXT("limited"));
        double Half = (Limits[1]->AsNumber() - Limits[0]->AsNumber()) * .5,
               Mid = (Limits[1]->AsNumber() + Limits[0]->AsNumber()) * .5;
        if (Type == 3) {
          C->SetAngularTwistLimit(Limited ? ACM_Limited : ACM_Free,
                                  FMath::RadiansToDegrees(Half));
          if (Limited)
            P.SetRotation(P.GetRotation() *
                          FQuat(FVector::ForwardVector, -Mid));
        } else {
          C->SetLinearXLimit(Limited ? LCM_Limited : LCM_Free, Half * 100.);
          if (Limited)
            P.AddToTranslation(
                P.GetRotation().RotateVector(FVector(Mid * 100., 0, 0)));
        }
        C->SetDisableCollision(true);
        C->SetConstrainedComponents(StateJoint.Parent, NAME_None,
                                    StateJoint.Child, NAME_None);
        C->SetConstraintReferenceFrame(EConstraintFrame::Frame1, P);
        C->SetConstraintReferenceFrame(EConstraintFrame::Frame2, Child);
      }
      // All current profiles have either mutually compatible source masks or
      // complete self-contact exclusion. A private object channel per instance
      // preserves external contact without adding solver joints. Fail closed
      // if a future graph needs a partial body-pair filter implementation.
      TSet<int32> Collidable;
      for (const auto &V : I.Graph->GetArrayField(TEXT("geoms"))) {
        O GeomData = V->AsObject();
        if (GeomData->GetBoolField(TEXT("collision"))) Collidable.Add(GeomData->GetIntegerField(TEXT("owner")));
      }
      int32 Excluded = 0;
      for (const auto &V : I.Graph->GetArrayField(TEXT("collision_exclusions"))) {
        O Pair = V->AsObject();
        if (Collidable.Contains(Pair->GetIntegerField(TEXT("body1"))) &&
            Collidable.Contains(Pair->GetIntegerField(TEXT("body2")))) Excluded++;
      }
      const int32 Pairs = Collidable.Num() * (Collidable.Num() - 1) / 2;
      if (Excluded != 0 && Excluded != Pairs)
        throw std::runtime_error("partial source collision masks need a native pair-filter adapter");
      const auto Channel = static_cast<ECollisionChannel>(ECC_GameTraceChannel1 + Impl->Instances.Num());
      for (auto &B : I.Bodies) {
        B.Value->SetCollisionObjectType(Channel);
        if (Pairs > 0 && Excluded == Pairs) B.Value->SetCollisionResponseToChannel(Channel, ECR_Ignore);
      }
      int32 Aid = 0;
      for (auto &V : I.Graph->GetArrayField(TEXT("actuators")))
        I.Actuators.Add(V->AsObject()->GetStringField(TEXT("joint")), Aid++);
      I.MotorMask.Init(0, Aid);
      I.Controls.Init(0, Aid + 2);
      I.Measurements.Init(0, Aid * 2 + 11 + (I.bCrow ? 235 : 0));
      I.Sensors.Init(0, I.Graph->GetObjectField(TEXT("io"))
                            ->GetArrayField(TEXT("sensors"))
                            .Num());
      I.InitialRoot = I.Root()->GetComponentLocation();
      FString Binding;
      I.Entry->TryGetStringField(TEXT("binding_json"), Binding);
      FString Diagnostic;
      Scene->TryGetStringField(TEXT("diagnostic_joint"), Diagnostic);
      if (I.bLegacy && !Binding.IsEmpty())
        throw std::runtime_error("Legacy sensory adapter requires the MuJoCo service; native Chaos legacy neural transfer is unavailable");
      if (!Binding.IsEmpty() && !Diagnostic.IsEmpty())
        throw std::runtime_error("diagnostic in neural mode");
      FTCHARToUTF8 B(*Binding);
      I.Api->Check(I.Api->nm_reference_open(
          reinterpret_cast<const uint8 *>(M.Get()), M.Length(),
          reinterpret_cast<const uint8 *>(B.Get()), B.Length(), &I.Handle));
      FString Control = TEXT("auto");
      Scene->TryGetStringField(TEXT("control"), Control);
      uint32 Mode = Control == TEXT("auto")     ? 0
                    : Control == TEXT("npc")    ? 1
                    : Control == TEXT("silent") ? 2
                    : Control == TEXT("aarnn")  ? 3
                                                : 4;
      I.Api->Check(I.Api->nm_control_mode(I.Handle, Mode));

      I.InitialMechanicalEnergy = I.MechanicalEnergy();
      I.MaxMechanicalEnergy = I.InitialMechanicalEnergy;

      Impl->Instances.Add(MoveTemp(State));
    }
    auto *Sun = GetWorld()->SpawnActor<ADirectionalLight>();
    Sun->SetActorRotation(FRotator(-45., -30., 0.));
    Sun->GetLightComponent()->SetIntensity(5.);
    // Presentation fill keeps animals visible behind tall enclosure shadows.
    auto *Fill = GetWorld()->SpawnActor<ADirectionalLight>();
    Fill->SetActorRotation(FRotator(-75., 150., 0.));
    Fill->GetLightComponent()->SetIntensity(2.);
    Fill->GetLightComponent()->SetCastShadows(false);
    return true;
  } catch (const std::exception &E) {
    UE_LOG(LogTemp, Error, TEXT("Native embodiment setup: %s"),
           UTF8_TO_TCHAR(E.what()));
    Impl->bFailed = true;
    return false;
  }
}
void ANmEmbodimentWorld::Tick(float Delta) {
  Super::Tick(Delta);
  if (Impl->bFailed || Impl->bAwaitCapture)
    return;
  try {
    if (!FMath::IsNearlyEqual(Delta, .002f, 1e-6f))
      throw std::runtime_error(
          "native owner requires exactly 2ms engine steps");
    if (Impl->Step)
      for (auto &I : Impl->Instances) {
        I->Capture(GetWorld(), Impl->Step);
        I->MaxMechanicalEnergy = FMath::Max(I->MaxMechanicalEnergy, I->MechanicalEnergy());
      }
    if (Impl->Iterations >=
        static_cast<uint64>(Impl->Scene->GetIntegerField(TEXT("steps")))) {
      Impl->Stop();
      TArray<TSharedPtr<FJsonValue>> Results;
      for (auto &I : Impl->Instances) {
        uint8 Buffer[65536] = {};
        int32 N = Impl->Api.nm_reference_status(I->Handle, Buffer, 65535);
        Impl->Api.Check(N);
        O Report;
        FString Text = UTF8_TO_TCHAR(reinterpret_cast<char *>(Buffer));
        FJsonSerializer::Deserialize(TJsonReaderFactory<>::Create(Text),
                                     Report);
        Report->SetStringField(TEXT("instance_id"),
                               I->Entry->GetStringField(TEXT("instance_id")));
        Report->SetNumberField(TEXT("active_steps"), I->ActiveSteps);
        Report->SetStringField(TEXT("graph_digest"), I->Graph->GetStringField(TEXT("digest")));
        Report->SetNumberField(TEXT("initial_mechanical_energy_J"), I->InitialMechanicalEnergy);
        Report->SetNumberField(TEXT("max_mechanical_energy_J"), I->MaxMechanicalEnergy);
        Report->SetNumberField(TEXT("final_mechanical_energy_J"), I->MechanicalEnergy());
        Report->SetStringField(TEXT("solver_policy"), TEXT("chaos-si-v1: position=16 velocity=8; 2ms; no mass or drive scaling"));
        auto Pose = [](FVector V) {
          return MakeShared<FJsonValueArray>(TArray<TSharedPtr<FJsonValue>>{
              MakeShared<FJsonValueNumber>(V.X / 100.),
              MakeShared<FJsonValueNumber>(-V.Y / 100.),
              MakeShared<FJsonValueNumber>(V.Z / 100.)});
        };
        Report->SetField(TEXT("initial_root"), Pose(I->InitialRoot));
        Report->SetField(TEXT("final_root"),
                         Pose(I->Root()->GetComponentLocation()));
        Report->SetStringField(
            TEXT("sensor_scope"),
            I->bCrow
                ? TEXT("reduced crow projection; native cluster contacts and "
                       "material/ray luminance, target calibration pending")
                : TEXT("native joints, range and gyro; ideal reference "
                       "projection"));
        Results.Add(MakeShared<FJsonValueObject>(Report));
      }
      O Result = MakeShared<FJsonObject>();
      Result->SetNumberField(TEXT("schema_version"), 1);
      Result->SetStringField(TEXT("scope"),
                             TEXT("native mechanics/reference ABI; target "
                                  "calibration/trained tasks remain open"));
      Result->SetNumberField(TEXT("steps"), Impl->Iterations);
      Result->SetNumberField(TEXT("source_step"), Impl->Step);
      Result->SetNumberField(TEXT("reset_epoch"), Impl->Epoch);
      Result->SetArrayField(TEXT("instances"), Results);
      FString Output = Impl->Scene->GetStringField(TEXT("output"));
      FFileHelper::SaveStringToFile(Json(Result), *Output);
      FString Capture;
      if (Impl->Scene->TryGetStringField(TEXT("capture"), Capture) &&
          !Capture.IsEmpty()) {
        FVector Center,Extent;
        GetActorBounds(false,Center,Extent);
        FBox Bounds(Center-Extent,Center+Extent);
        auto *Camera = GetWorld()->SpawnActor<ACameraActor>();
        auto *Lens = Camera->GetCameraComponent();
        Lens->SetFieldOfView(55.);
        Lens->SetAspectRatio(4.f/3.f);
        Lens->PostProcessSettings.bOverride_AutoExposureBias = true;
        Lens->PostProcessSettings.AutoExposureBias = 0.;
        const double Distance = FMath::Max(Bounds.GetExtent().Size() * 1.75, 100.);
        // Look over the enclosure rim so the habitat and its occupants remain visible.
        FVector Location = Bounds.GetCenter() + FVector(0, -Distance, Distance * 1.3);
        Camera->SetActorLocationAndRotation(Location, (Bounds.GetCenter() - Location).Rotation());
        if (auto *PC = GetWorld()->GetFirstPlayerController()) PC->SetViewTarget(Camera);
        for (auto &I : Impl->Instances)
          for (auto &B : I->Bodies)
            B.Value->SetSimulatePhysics(false);
        Impl->bAwaitCapture = true;
        FScreenshotRequest::OnScreenshotRequestProcessed().AddWeakLambda(
            this, []() { FPlatformMisc::RequestExit(false); });
        FScreenshotRequest::RequestScreenshot(Capture, false, false);
      } else {
        Impl->bFailed = true;
        FPlatformMisc::RequestExit(false);
      }

      return;
    }
    if (Impl->Scene->GetIntegerField(TEXT("reset_at")) == static_cast<int64>(Impl->Iterations)) {
      for (auto &I : Impl->Instances) I->Reset();
      Impl->Epoch++;
      Impl->Step = 0;
    }
    for (auto &I : Impl->Instances)
      I->Prepare(Impl->Scene, Impl->Step);
    Impl->Step++;
    Impl->Iterations++;
  } catch (const std::exception &E) {
    UE_LOG(LogTemp, Error, TEXT("Native embodiment fault: %s"),
           UTF8_TO_TCHAR(E.what()));
    Impl->Stop();
    Impl->bFailed = true;
    FPlatformMisc::RequestExitWithStatus(false, 1);
  }
}
void ANmEmbodimentWorld::EndPlay(const EEndPlayReason::Type Reason) {
  Impl->Stop();
  Impl->Instances.Empty();
  Super::EndPlay(Reason);
}

void ANmEmbodimentWorld::Contact(UPrimitiveComponent *Hit, AActor *,
                                 UPrimitiveComponent *Other, FVector,
                                 const FHitResult &) {
  for (auto &I : Impl->Instances) {
    bool OwnHit = false, OwnOther = false;
    for (auto &B : I->Bodies) {
      OwnHit |= B.Value == Hit;
      OwnOther |= B.Value == Other;
    }
    if (OwnHit && !OwnOther)
      I->Contacts.Add(Hit);
  }
}
