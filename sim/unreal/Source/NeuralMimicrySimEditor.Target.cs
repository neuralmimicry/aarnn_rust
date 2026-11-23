// SPDX-FileCopyrightText: NEURALMIMICRY LTD
// SPDX-License-Identifier: MIT
//
// Licensed under the MIT License. See LICENSE in the project root.

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
