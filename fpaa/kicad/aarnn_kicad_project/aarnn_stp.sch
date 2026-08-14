EESchema Schematic File Version 4
LIBS:tb_aarnn_kernels-cache
EELAYER 29 0
EELAYER END
$Descr A3 16535 11693
encoding utf-8
Sheet 1 1
Title "AARNN_STP implementation reference"
Date "2026-08-12"
Rev "1.3"
Comp "NeuralMimicry Ltd"
Comment1 "Expanded from aarnn_kernels.lib"
Comment2 "Equation-level SPICE; not a transistor-level or PCB implementation"
Comment3 "Standalone reference sheet; main testbench uses the external .lib model"
Comment4 "1 V = 1 normalised AARNN unit"
$EndDescr
Text Notes 700 600 0    100  ~ 20
AARNN_STP
Text Notes 700 850 0    50   ~ 0
Source: aarnn_kernels.lib | 7 elements | pin order preserved exactly
Text Notes 700 1100 0    60   ~ 12
DEFAULT PARAMETERS
Text Notes 700 1350 0    40   ~ 0
BASE_U=0.2 TAU_REC=800m TAU_FACIL=200m SPIKE_RATE=100k SPIKE_THRESHOLD=0.5 SPIKE_SMOOTH=0.01 CSTATE=1u
Text Notes 700 2050 0    60   ~ 12
EXPOSED SUBCIRCUIT PINS
Text HLabel 1000 2350 0    50   Input ~ 0
SPIKE
Text Notes 1350 2365 0    40   ~ 0
pin 1 / input
Text HLabel 15500 2350 2    50   Output ~ 0
UTIL
Text Notes 14200 2365 0    40   ~ 0
pin 2 / output
Text HLabel 15500 2600 2    50   Output ~ 0
RES
Text Notes 14200 2615 0    40   ~ 0
pin 3 / output
Text HLabel 15500 2850 2    50   Output ~ 0
RELEASE
Text Notes 14200 2865 0    40   ~ 0
pin 4 / output
Text HLabel 8000 2500 1    50   BiDi ~ 0
VSS
Text Notes 8150 2520 0    40   ~ 0
pin 5 / analogue reference
$Comp
L AARNN_BSOURCE BSPK_GATE
U 1 1 6602C600
P 3000 4200
F 0 "BSPK_GATE" H 3000 3900 50  0000 C CNN
F 1 "V={0.5*(1+tanh((V(SPIKE,VSS)-SPIKE_THRESHOLD)/SPIKE_SMOOTH))}" H 3000 4500 50  0001 C CNN
F 2 "" H 3000 4200 50  0001 C CNN
F 3 "" H 3000 4200 50  0001 C CNN
F 4 "B" H 3000 4200 50  0001 C CNN "Spice_Primitive"
F 5 "1 2" H 3000 4200 50  0001 C CNN "Spice_Node_Sequence"
F 6 "Y" H 3000 4200 50  0001 C CNN "Spice_Netlist_Enabled"
	1    3000 4200
	1    0    0    -1
$EndComp
Text Label 2550 4200 2    45   ~ 0
SPK_GATE
Text Label 3450 4200 0    45   ~ 0
VSS
Text Notes 2350 4650 0    35   ~ 0
B:
Text Notes 2350 4800 0    35   ~ 0
V={0.5*(1+tanh((V(SPIKE,VSS)-SPIKE_THRESHOLD)/SPIKE_SMOOTH))}
$Comp
L AARNN_BSOURCE BUTIL
U 1 1 6602C601
P 8000 4200
F 0 "BUTIL" H 8000 3900 50  0000 C CNN
F 1 "I={CSTATE*((BASE_U-V(UTIL,VSS))/TAU_FACIL+V(SPK_GATE,VSS)*SPIKE_RATE*BASE_U*(1-V(UTIL,VSS)))}" H 8000 4500 50  0001 C CNN
F 2 "" H 8000 4200 50  0001 C CNN
F 3 "" H 8000 4200 50  0001 C CNN
F 4 "B" H 8000 4200 50  0001 C CNN "Spice_Primitive"
F 5 "1 2" H 8000 4200 50  0001 C CNN "Spice_Node_Sequence"
F 6 "Y" H 8000 4200 50  0001 C CNN "Spice_Netlist_Enabled"
	1    8000 4200
	1    0    0    -1
$EndComp
Text Label 7550 4200 2    45   ~ 0
UTIL
Text Label 8450 4200 0    45   ~ 0
VSS
Text Notes 7350 4650 0    35   ~ 0
B:
Text Notes 7350 4800 0    35   ~ 0
I={CSTATE*((BASE_U-V(UTIL,VSS))/TAU_FACIL+V(SPK_GATE,VSS)*SPIKE_RATE*BASE_U*(1-V(UTIL,VSS)))}
$Comp
L AARNN_BSOURCE BRES
U 1 1 6602C602
P 13000 4200
F 0 "BRES" H 13000 3900 50  0000 C CNN
F 1 "I={CSTATE*((1-V(RES,VSS))/TAU_REC-V(SPK_GATE,VSS)*SPIKE_RATE*min(max(V(UTIL,VSS)*V(RES,VSS),0),1))}" H 13000 4500 50  0001 C CNN
F 2 "" H 13000 4200 50  0001 C CNN
F 3 "" H 13000 4200 50  0001 C CNN
F 4 "B" H 13000 4200 50  0001 C CNN "Spice_Primitive"
F 5 "1 2" H 13000 4200 50  0001 C CNN "Spice_Node_Sequence"
F 6 "Y" H 13000 4200 50  0001 C CNN "Spice_Netlist_Enabled"
	1    13000 4200
	1    0    0    -1
$EndComp
Text Label 12550 4200 2    45   ~ 0
RES
Text Label 13450 4200 0    45   ~ 0
VSS
Text Notes 12350 4650 0    35   ~ 0
B:
Text Notes 12350 4800 0    35   ~ 0
I={CSTATE*((1-V(RES,VSS))/TAU_REC-V(SPK_GATE,VSS)*SPIKE_RATE*min(max(V(UTIL,VSS)*V(RES,VSS),0),1))}
$Comp
L AARNN_BSOURCE BRELEASE
U 1 1 6602C603
P 3000 5900
F 0 "BRELEASE" H 3000 5600 50  0000 C CNN
F 1 "V={V(SPK_GATE,VSS)*min(max(V(UTIL,VSS)*V(RES,VSS),0),1)}" H 3000 6200 50  0001 C CNN
F 2 "" H 3000 5900 50  0001 C CNN
F 3 "" H 3000 5900 50  0001 C CNN
F 4 "B" H 3000 5900 50  0001 C CNN "Spice_Primitive"
F 5 "1 2" H 3000 5900 50  0001 C CNN "Spice_Node_Sequence"
F 6 "Y" H 3000 5900 50  0001 C CNN "Spice_Netlist_Enabled"
	1    3000 5900
	1    0    0    -1
$EndComp
Text Label 2550 5900 2    45   ~ 0
RELEASE
Text Label 3450 5900 0    45   ~ 0
VSS
Text Notes 2350 6350 0    35   ~ 0
B:
Text Notes 2350 6500 0    35   ~ 0
V={V(SPK_GATE,VSS)*min(max(V(UTIL,VSS)*V(RES,VSS),0),1)}
$Comp
L AARNN_RESISTOR RUTIL
U 1 1 6602C604
P 8000 5900
F 0 "RUTIL" H 8000 5600 50  0000 C CNN
F 1 "1G" H 8000 6200 50  0001 C CNN
F 2 "" H 8000 5900 50  0001 C CNN
F 3 "" H 8000 5900 50  0001 C CNN
F 4 "R" H 8000 5900 50  0001 C CNN "Spice_Primitive"
F 5 "1 2" H 8000 5900 50  0001 C CNN "Spice_Node_Sequence"
F 6 "Y" H 8000 5900 50  0001 C CNN "Spice_Netlist_Enabled"
	1    8000 5900
	1    0    0    -1
$EndComp
Text Label 7550 5900 2    45   ~ 0
UTIL
Text Label 8450 5900 0    45   ~ 0
VSS
Text Notes 7350 6350 0    35   ~ 0
R: 1G
$Comp
L AARNN_RESISTOR RRES
U 1 1 6602C605
P 13000 5900
F 0 "RRES" H 13000 5600 50  0000 C CNN
F 1 "1G" H 13000 6200 50  0001 C CNN
F 2 "" H 13000 5900 50  0001 C CNN
F 3 "" H 13000 5900 50  0001 C CNN
F 4 "R" H 13000 5900 50  0001 C CNN "Spice_Primitive"
F 5 "1 2" H 13000 5900 50  0001 C CNN "Spice_Node_Sequence"
F 6 "Y" H 13000 5900 50  0001 C CNN "Spice_Netlist_Enabled"
	1    13000 5900
	1    0    0    -1
$EndComp
Text Label 12550 5900 2    45   ~ 0
RES
Text Label 13450 5900 0    45   ~ 0
VSS
Text Notes 12350 6350 0    35   ~ 0
R: 1G
$Comp
L AARNN_RESISTOR RRELEASE
U 1 1 6602C606
P 3000 7600
F 0 "RRELEASE" H 3000 7300 50  0000 C CNN
F 1 "1G" H 3000 7900 50  0001 C CNN
F 2 "" H 3000 7600 50  0001 C CNN
F 3 "" H 3000 7600 50  0001 C CNN
F 4 "R" H 3000 7600 50  0001 C CNN "Spice_Primitive"
F 5 "1 2" H 3000 7600 50  0001 C CNN "Spice_Node_Sequence"
F 6 "Y" H 3000 7600 50  0001 C CNN "Spice_Netlist_Enabled"
	1    3000 7600
	1    0    0    -1
$EndComp
Text Label 2550 7600 2    45   ~ 0
RELEASE
Text Label 3450 7600 0    45   ~ 0
VSS
Text Notes 2350 8050 0    35   ~ 0
R: 1G
Text Notes 700 10400 0    40   ~ 0
.ic V(UTIL)={BASE_U} V(RES)=1
Text Notes 700 11100 0    38   ~ 0
Behavioural expressions are kept as SPICE symbol values; named labels reproduce every source-library node.
$EndSCHEMATC
