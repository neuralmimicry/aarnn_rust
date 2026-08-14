EESchema Schematic File Version 4
LIBS:tb_aarnn_kernels-cache
EELAYER 29 0
EELAYER END
$Descr A3 16535 11693
encoding utf-8
Sheet 1 1
Title "AARNN_VOLUME_FIELD implementation reference"
Date "2026-08-12"
Rev "1.3"
Comp "NeuralMimicry Ltd"
Comment1 "Expanded from aarnn_kernels.lib"
Comment2 "Equation-level SPICE; not a transistor-level or PCB implementation"
Comment3 "Standalone reference sheet; main testbench uses the external .lib model"
Comment4 "1 V = 1 normalised AARNN unit"
$EndDescr
Text Notes 700 600 0    100  ~ 20
AARNN_VOLUME_FIELD
Text Notes 700 850 0    50   ~ 0
Source: aarnn_kernels.lib | 2 elements | pin order preserved exactly
Text Notes 700 1100 0    60   ~ 12
DEFAULT PARAMETERS
Text Notes 700 1350 0    40   ~ 0
RADIUS=0.12 STRENGTH=0.10 TONE=1.5 DISTANCE=0
Text Notes 700 2050 0    60   ~ 12
EXPOSED SUBCIRCUIT PINS
Text HLabel 1000 2350 0    50   Input ~ 0
SOURCE
Text Notes 1350 2365 0    40   ~ 0
pin 1 / input
Text HLabel 15500 2350 2    50   Output ~ 0
FIELD
Text Notes 14200 2365 0    40   ~ 0
pin 2 / output
Text HLabel 8000 2500 1    50   BiDi ~ 0
VSS
Text Notes 8150 2520 0    40   ~ 0
pin 3 / analogue reference
$Comp
L AARNN_BSOURCE BFIELD
U 1 1 66056A00
P 3000 4200
F 0 "BFIELD" H 3000 3900 50  0000 C CNN
F 1 "V={min(max(1+STRENGTH*max(V(SOURCE,VSS),0)*min(max(TONE,0),3)*exp(-(DISTANCE*DISTANCE)/(2*RADIUS*RADIUS)),0.5),2.5)}" H 3000 4500 50  0001 C CNN
F 2 "" H 3000 4200 50  0001 C CNN
F 3 "" H 3000 4200 50  0001 C CNN
F 4 "B" H 3000 4200 50  0001 C CNN "Spice_Primitive"
F 5 "1 2" H 3000 4200 50  0001 C CNN "Spice_Node_Sequence"
F 6 "Y" H 3000 4200 50  0001 C CNN "Spice_Netlist_Enabled"
	1    3000 4200
	1    0    0    -1
$EndComp
Text Label 2550 4200 2    45   ~ 0
FIELD
Text Label 3450 4200 0    45   ~ 0
VSS
Text Notes 2350 4650 0    35   ~ 0
B:
Text Notes 2350 4800 0    35   ~ 0
V={min(max(1+STRENGTH*max(V(SOURCE,VSS),0)*min(max(TONE,0),3)*exp(-(DISTANCE*DISTANCE)/(2*RADIUS*RADIUS)),0.5),2.5)}
$Comp
L AARNN_RESISTOR RFIELD
U 1 1 66056A01
P 8000 4200
F 0 "RFIELD" H 8000 3900 50  0000 C CNN
F 1 "1G" H 8000 4500 50  0001 C CNN
F 2 "" H 8000 4200 50  0001 C CNN
F 3 "" H 8000 4200 50  0001 C CNN
F 4 "R" H 8000 4200 50  0001 C CNN "Spice_Primitive"
F 5 "1 2" H 8000 4200 50  0001 C CNN "Spice_Node_Sequence"
F 6 "Y" H 8000 4200 50  0001 C CNN "Spice_Netlist_Enabled"
	1    8000 4200
	1    0    0    -1
$EndComp
Text Label 7550 4200 2    45   ~ 0
FIELD
Text Label 8450 4200 0    45   ~ 0
VSS
Text Notes 7350 4650 0    35   ~ 0
R: 1G
Text Notes 700 11100 0    38   ~ 0
Behavioural expressions are kept as SPICE symbol values; named labels reproduce every source-library node.
$EndSCHEMATC
