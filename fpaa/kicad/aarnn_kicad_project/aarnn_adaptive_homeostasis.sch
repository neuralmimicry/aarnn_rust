EESchema Schematic File Version 4
LIBS:tb_aarnn_kernels-cache
EELAYER 29 0
EELAYER END
$Descr A3 16535 11693
encoding utf-8
Sheet 1 1
Title "AARNN_ADAPTIVE_HOMEOSTASIS implementation reference"
Date "2026-08-12"
Rev "1.3"
Comp "NeuralMimicry Ltd"
Comment1 "Expanded from aarnn_kernels.lib"
Comment2 "Equation-level SPICE; not a transistor-level or PCB implementation"
Comment3 "Standalone reference sheet; main testbench uses the external .lib model"
Comment4 "1 V = 1 normalised AARNN unit"
$EndDescr
Text Notes 700 600 0    100  ~ 20
AARNN_ADAPTIVE_HOMEOSTASIS
Text Notes 700 850 0    50   ~ 0
Source: aarnn_kernels.lib | 7 elements | pin order preserved exactly
Text Notes 700 1100 0    60   ~ 12
DEFAULT PARAMETERS
Text Notes 700 1350 0    40   ~ 0
TAU_THRESH=200m THR_INC=0.5 THR_MIN=-2 THR_MAX=5 TAU_RATE=2 RATE_TARGET=0.003 HOMEOSTASIS_GAIN=0.25 SPIKE_RATE=100k
Text Notes 700 1530 0    40   ~ 0
SPIKE_SMOOTH=0.01 RATE_IMPULSE=0.0005 CSTATE=1u
Text Notes 700 2050 0    60   ~ 12
EXPOSED SUBCIRCUIT PINS
Text HLabel 1000 2350 0    50   Input ~ 0
SPIKE
Text Notes 1350 2365 0    40   ~ 0
pin 1 / input
Text HLabel 15500 2350 2    50   Output ~ 0
THRESH
Text Notes 14200 2365 0    40   ~ 0
pin 2 / output
Text HLabel 15500 2600 2    50   Output ~ 0
RATE
Text Notes 14200 2615 0    40   ~ 0
pin 3 / output
Text HLabel 15500 2850 2    50   Output ~ 0
THRESH_LIMITED
Text Notes 14200 2865 0    40   ~ 0
pin 4 / output
Text HLabel 8000 2500 1    50   BiDi ~ 0
VSS
Text Notes 8150 2520 0    40   ~ 0
pin 5 / analogue reference
$Comp
L AARNN_BSOURCE BSPK_GATE
U 1 1 6607CB00
P 3000 4200
F 0 "BSPK_GATE" H 3000 3900 50  0000 C CNN
F 1 "V={0.5*(1+tanh((V(SPIKE,VSS)-0.5)/SPIKE_SMOOTH))}" H 3000 4500 50  0001 C CNN
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
B: V={0.5*(1+tanh((V(SPIKE,VSS)-0.5)/SPIKE_SMOOTH))}
$Comp
L AARNN_BSOURCE BRATE
U 1 1 6607CB01
P 8000 4200
F 0 "BRATE" H 8000 3900 50  0000 C CNN
F 1 "I={CSTATE*((0-V(RATE,VSS))/TAU_RATE+V(SPK_GATE,VSS)*SPIKE_RATE*RATE_IMPULSE)}" H 8000 4500 50  0001 C CNN
F 2 "" H 8000 4200 50  0001 C CNN
F 3 "" H 8000 4200 50  0001 C CNN
F 4 "B" H 8000 4200 50  0001 C CNN "Spice_Primitive"
F 5 "1 2" H 8000 4200 50  0001 C CNN "Spice_Node_Sequence"
F 6 "Y" H 8000 4200 50  0001 C CNN "Spice_Netlist_Enabled"
	1    8000 4200
	1    0    0    -1
$EndComp
Text Label 7550 4200 2    45   ~ 0
RATE
Text Label 8450 4200 0    45   ~ 0
VSS
Text Notes 7350 4650 0    35   ~ 0
B:
Text Notes 7350 4800 0    35   ~ 0
I={CSTATE*((0-V(RATE,VSS))/TAU_RATE+V(SPK_GATE,VSS)*SPIKE_RATE*RATE_IMPULSE)}
$Comp
L AARNN_BSOURCE BTHRESH
U 1 1 6607CB02
P 13000 4200
F 0 "BTHRESH" H 13000 3900 50  0000 C CNN
F 1 "I={CSTATE*((0-V(THRESH,VSS))/TAU_THRESH+V(SPK_GATE,VSS)*SPIKE_RATE*THR_INC+HOMEOSTASIS_GAIN*(V(RATE,VSS)-RATE_TARGET)/TAU_THRESH)}" H 13000 4500 50  0001 C CNN
F 2 "" H 13000 4200 50  0001 C CNN
F 3 "" H 13000 4200 50  0001 C CNN
F 4 "B" H 13000 4200 50  0001 C CNN "Spice_Primitive"
F 5 "1 2" H 13000 4200 50  0001 C CNN "Spice_Node_Sequence"
F 6 "Y" H 13000 4200 50  0001 C CNN "Spice_Netlist_Enabled"
	1    13000 4200
	1    0    0    -1
$EndComp
Text Label 12550 4200 2    45   ~ 0
THRESH
Text Label 13450 4200 0    45   ~ 0
VSS
Text Notes 12350 4650 0    35   ~ 0
B:
Text Notes 12350 4800 0    35   ~ 0
I={CSTATE*((0-V(THRESH,VSS))/TAU_THRESH+V(SPK_GATE,VSS)*SPIKE_RATE*THR_INC+HOMEOSTASIS_GAIN*(V(RATE,VSS)-RATE_TARGET)/TAU_THRESH)}
$Comp
L AARNN_BSOURCE BTHRESH_CLAMP
U 1 1 6607CB03
P 3000 5900
F 0 "BTHRESH_CLAMP" H 3000 5600 50  0000 C CNN
F 1 "V={min(max(V(THRESH,VSS),THR_MIN),THR_MAX)}" H 3000 6200 50  0001 C CNN
F 2 "" H 3000 5900 50  0001 C CNN
F 3 "" H 3000 5900 50  0001 C CNN
F 4 "B" H 3000 5900 50  0001 C CNN "Spice_Primitive"
F 5 "1 2" H 3000 5900 50  0001 C CNN "Spice_Node_Sequence"
F 6 "Y" H 3000 5900 50  0001 C CNN "Spice_Netlist_Enabled"
	1    3000 5900
	1    0    0    -1
$EndComp
Text Label 2550 5900 2    45   ~ 0
THRESH_LIMITED
Text Label 3450 5900 0    45   ~ 0
VSS
Text Notes 2350 6350 0    35   ~ 0
B: V={min(max(V(THRESH,VSS),THR_MIN),THR_MAX)}
$Comp
L AARNN_RESISTOR RTHRESH
U 1 1 6607CB04
P 8000 5900
F 0 "RTHRESH" H 8000 5600 50  0000 C CNN
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
THRESH
Text Label 8450 5900 0    45   ~ 0
VSS
Text Notes 7350 6350 0    35   ~ 0
R: 1G
$Comp
L AARNN_RESISTOR RRATE
U 1 1 6607CB05
P 13000 5900
F 0 "RRATE" H 13000 5600 50  0000 C CNN
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
RATE
Text Label 13450 5900 0    45   ~ 0
VSS
Text Notes 12350 6350 0    35   ~ 0
R: 1G
$Comp
L AARNN_RESISTOR RCLAMP
U 1 1 6607CB06
P 3000 7600
F 0 "RCLAMP" H 3000 7300 50  0000 C CNN
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
THRESH_LIMITED
Text Label 3450 7600 0    45   ~ 0
VSS
Text Notes 2350 8050 0    35   ~ 0
R: 1G
Text Notes 700 11100 0    38   ~ 0
Behavioural expressions are kept as SPICE symbol values; named labels reproduce every source-library node.
$EndSCHEMATC
