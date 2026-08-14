EESchema Schematic File Version 4
LIBS:tb_aarnn_kernels-cache
EELAYER 29 0
EELAYER END
$Descr A3 16535 11693
encoding utf-8
Sheet 1 1
Title "AARNN_GAP_PAIR implementation reference"
Date "2026-08-12"
Rev "1.3"
Comp "NeuralMimicry Ltd"
Comment1 "Expanded from aarnn_kernels.lib"
Comment2 "Equation-level SPICE; not a transistor-level or PCB implementation"
Comment3 "Standalone reference sheet; main testbench uses the external .lib model"
Comment4 "1 V = 1 normalised AARNN unit"
$EndDescr
Text Notes 700 600 0    100  ~ 20
AARNN_GAP_PAIR
Text Notes 700 850 0    50   ~ 0
Source: aarnn_kernels.lib | 1 elements | pin order preserved exactly
Text Notes 700 1100 0    60   ~ 12
DEFAULT PARAMETERS
Text Notes 700 1350 0    40   ~ 0
GAP_G=1m
Text Notes 700 2050 0    60   ~ 12
EXPOSED SUBCIRCUIT PINS
Text HLabel 1000 2350 0    50   Input ~ 0
V1
Text Notes 1350 2365 0    40   ~ 0
pin 1 / input
Text HLabel 1000 2600 0    50   Input ~ 0
V2
Text Notes 1350 2615 0    40   ~ 0
pin 2 / input
Text HLabel 8000 2500 1    50   BiDi ~ 0
VSS
Text Notes 8150 2520 0    40   ~ 0
pin 3 / analogue reference
$Comp
L AARNN_VCCS GAP
U 1 1 66043200
P 3000 4200
F 0 "GAP" H 3000 3900 50  0000 C CNN
F 1 "{GAP_G}" H 3000 4500 50  0001 C CNN
F 2 "" H 3000 4200 50  0001 C CNN
F 3 "" H 3000 4200 50  0001 C CNN
F 4 "G" H 3000 4200 50  0001 C CNN "Spice_Primitive"
F 5 "1 2 3 4" H 3000 4200 50  0001 C CNN "Spice_Node_Sequence"
F 6 "Y" H 3000 4200 50  0001 C CNN "Spice_Netlist_Enabled"
	1    3000 4200
	1    0    0    -1
$EndComp
Text Label 2550 4100 2    45   ~ 0
V1
Text Label 2550 4300 2    45   ~ 0
V2
Text Label 3450 4100 0    45   ~ 0
V1
Text Label 3450 4300 0    45   ~ 0
V2
Text Notes 2350 4650 0    35   ~ 0
G: {GAP_G}
Text Notes 700 11100 0    38   ~ 0
Behavioural expressions are kept as SPICE symbol values; named labels reproduce every source-library node.
$EndSCHEMATC
