// Copyright NeuralMimicry. All Rights Reserved.

using UnrealBuildTool;
using System.Collections.Generic;

public class NeuralMimicrySimEditorTarget : TargetRules
{
    public NeuralMimicrySimEditorTarget(TargetInfo Target) : base(Target)
    {
        Type = TargetType.Editor;
        DefaultBuildSettings = BuildSettingsVersion.Latest;
        IncludeOrderVersion = EngineIncludeOrderVersion.Latest;
        ExtraModuleNames.Add("NeuralMimicrySim");
    }
}
