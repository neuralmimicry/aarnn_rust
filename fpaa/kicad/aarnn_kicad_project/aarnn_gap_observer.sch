EESchema Schematic File Version 4
LIBS:tb_aarnn_kernels-cache
EELAYER 29 0
EELAYER END
$Descr A3 16535 11693
encoding utf-8
Sheet 1 1
Title "AARNN_GAP_OBSERVER implementation reference"
Date "2026-08-12"
Rev "1.3"
Comp "NeuralMimicry Ltd"
Comment1 "Expanded from aarnn_kernels.lib"
Comment2 "Equation-level SPICE; not a transistor-level or PCB implementation"
Comment3 "Standalone reference sheet; main testbench uses the external .lib model"
Comment4 "1 V = 1 normalised AARNN unit"
$EndDescr
Text Notes 700 600 0    100  ~ 20
AARNN_GAP_OBSERVER
Text Notes 700 850 0    50   ~ 0
Source: aarnn_kernels.lib | 4 elements | pin order preserved exactly
Text Notes 700 1100 0    60   ~ 12
DEFAULT PARAMETERS
Text Notes 700 1350 0    40   ~ 0
GAP_STRENGTH=0.03
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
Text HLabel 15500 2350 2    50   Output ~ 0
DELTA1
Text Notes 14200 2365 0    40   ~ 0
pin 3 / output
Text HLabel 15500 2600 2    50   Output ~ 0
DELTA2
Text Notes 14200 2615 0    40   ~ 0
pin 4 / output
Text HLabel 8000 2500 1    50   BiDi ~ 0
VSS
Text Notes 8150 2520 0    40   ~ 0
pin 5 / analogue reference
$Comp
L AARNN_BSOURCE BDELTA1
U 1 1 66056E00
P 3000 4200
F 0 "BDELTA1" H 3000 3900 50  0000 C CNN
F 1 "V={GAP_STRENGTH*(V(V2,VSS)-V(V1,VSS))}" H 3000 4500 50  0001 C CNN
F 2 "" H 3000 4200 50  0001 C CNN
F 3 "" H 3000 4200 50  0001 C CNN
F 4 "B" H 3000 4200 50  0001 C CNN "Spice_Primitive"
F 5 "1 2" H 3000 4200 50  0001 C CNN "Spice_Node_Sequence"
F 6 "Y" H 3000 4200 50  0001 C CNN "Spice_Netlist_Enabled"
	1    3000 4200
	1    0    0    -1
$EndComp
Text Label 2550 4200 2    45   ~ 0
DELTA1
Text Label 3450 4200 0    45   ~ 0
VSS
Text Notes 2350 4650 0    35   ~ 0
B: V={GAP_STRENGTH*(V(V2,VSS)-V(V1,VSS))}
$Comp
L AARNN_BSOURCE BDELTA2
U 1 1 66056E01
P 8000 4200
F 0 "BDELTA2" H 8000 3900 50  0000 C CNN
F 1 "V={GAP_STRENGTH*(V(V1,VSS)-V(V2,VSS))}" H 8000 4500 50  0001 C CNN
F 2 "" H 8000 4200 50  0001 C CNN
F 3 "" H 8000 4200 50  0001 C CNN
F 4 "B" H 8000 4200 50  0001 C CNN "Spice_Primitive"
F 5 "1 2" H 8000 4200 50  0001 C CNN "Spice_Node_Sequence"
F 6 "Y" H 8000 4200 50  0001 C CNN "Spice_Netlist_Enabled"
	1    8000 4200
	1    0    0    -1
$EndComp
Text Label 7550 4200 2    45   ~ 0
DELTA2
Text Label 8450 4200 0    45   ~ 0
VSS
Text Notes 7350 4650 0    35   ~ 0
B: V={GAP_STRENGTH*(V(V1,VSS)-V(V2,VSS))}
$Comp
L AARNN_RESISTOR RDELTA1
U 1 1 66056E02
P 13000 4200
F 0 "RDELTA1" H 13000 3900 50  0000 C CNN
F 1 "1G" H 13000 4500 50  0001 C CNN
F 2 "" H 13000 4200 50  0001 C CNN
F 3 "" H 13000 4200 50  0001 C CNN
F 4 "R" H 13000 4200 50  0001 C CNN "Spice_Primitive"
F 5 "1 2" H 13000 4200 50  0001 C CNN "Spice_Node_Sequence"
F 6 "Y" H 13000 4200 50  0001 C CNN "Spice_Netlist_Enabled"
	1    13000 4200
	1    0    0    -1
$EndComp
Text Label 12550 4200 2    45   ~ 0
DELTA1
Text Label 13450 4200 0    45   ~ 0
VSS
Text Notes 12350 4650 0    35   ~ 0
R: 1G
$Comp
L AARNN_RESISTOR RDELTA2
U 1 1 66056E03
P 3000 5900
F 0 "RDELTA2" H 3000 5600 50  0000 C CNN
F 1 "1G" H 3000 6200 50  0001 C CNN
F 2 "" H 3000 5900 50  0001 C CNN
F 3 "" H 3000 5900 50  0001 C CNN
F 4 "R" H 3000 5900 50  0001 C CNN "Spice_Primitive"
F 5 "1 2" H 3000 5900 50  0001 C CNN "Spice_Node_Sequence"
F 6 "Y" H 3000 5900 50  0001 C CNN "Spice_Netlist_Enabled"
	1    3000 5900
	1    0    0    -1
$EndComp
Text Label 2550 5900 2    45   ~ 0
DELTA2
Text Label 3450 5900 0    45   ~ 0
VSS
Text Notes 2350 6350 0    35   ~ 0
R: 1G
Text Notes 700 11100 0    38   ~ 0
Behavioural expressions are kept as SPICE symbol values; named labels reproduce every source-library node.
$EndSCHEMATC
