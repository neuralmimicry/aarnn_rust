// Copyright NeuralMimicry. All Rights Reserved.

using UnrealBuildTool;
using System.Collections.Generic;

public class NeuralMimicrySimTarget : TargetRules
{
    public NeuralMimicrySimTarget(TargetInfo Target) : base(Target)
    {
        Type = TargetType.Game;
        DefaultBuildSettings = BuildSettingsVersion.Latest;
        IncludeOrderVersion = EngineIncludeOrderVersion.Latest;
        ExtraModuleNames.Add("NeuralMimicrySim");
    }
}
