// SPDX-FileCopyrightText: 2026 AARNN contributors
// SPDX-License-Identifier: MIT
//
// Licensed under the MIT License. See sim/minecraft/LICENSE in the project root.
import { start } from './runtime.js';
start({available:()=>false,request:()=>Promise.reject(Error('Offline pack: BDS connector unavailable'))});
