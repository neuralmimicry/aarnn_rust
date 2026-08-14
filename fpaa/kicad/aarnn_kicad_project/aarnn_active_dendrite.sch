EESchema Schematic File Version 4
LIBS:tb_aarnn_kernels-cache
EELAYER 29 0
EELAYER END
$Descr A3 16535 11693
encoding utf-8
Sheet 1 1
Title "AARNN_ACTIVE_DENDRITE implementation reference"
Date "2026-08-12"
Rev "1.3"
Comp "NeuralMimicry Ltd"
Comment1 "Expanded from aarnn_kernels.lib"
Comment2 "Equation-level SPICE; not a transistor-level or PCB implementation"
Comment3 "Standalone reference sheet; main testbench uses the external .lib model"
Comment4 "1 V = 1 normalised AARNN unit"
$EndDescr
Text Notes 700 600 0    100  ~ 20
AARNN_ACTIVE_DENDRITE
Text Notes 700 850 0    50   ~ 0
Source: aarnn_kernels.lib | 8 elements | pin order preserved exactly
Text Notes 700 1100 0    60   ~ 12
DEFAULT PARAMETERS
Text Notes 700 1350 0    40   ~ 0
TAU_CA=120m TAU_PLATEAU=350m CA_GAIN=0.10 PLATEAU_THRESHOLD=1 PLATEAU_GAIN=0.40 SIGN_SMOOTH=0.001 CSTATE=1u
Text Notes 700 2050 0    60   ~ 12
EXPOSED SUBCIRCUIT PINS
Text HLabel 1000 2350 0    50   Input ~ 0
CURR
Text Notes 1350 2365 0    40   ~ 0
pin 1 / input
Text HLabel 1000 2600 0    50   Input ~ 0
LOCAL
Text Notes 1350 2615 0    40   ~ 0
pin 2 / input
Text HLabel 1000 2850 0    50   Input ~ 0
BRANCH
Text Notes 1350 2865 0    40   ~ 0
pin 3 / input
Text HLabel 15500 2350 2    50   Output ~ 0
CA
Text Notes 14200 2365 0    40   ~ 0
pin 4 / output
Text HLabel 15500 2600 2    50   Output ~ 0
PLATEAU
Text Notes 14200 2615 0    40   ~ 0
pin 5 / output
Text HLabel 15500 2850 2    50   Output ~ 0
OUT
Text Notes 14200 2865 0    40   ~ 0
pin 6 / output
Text HLabel 8000 2500 1    50   BiDi ~ 0
VSS
Text Notes 8150 2520 0    40   ~ 0
pin 7 / analogue reference
$Comp
L AARNN_BSOURCE BCA
U 1 1 66063900
P 3000 4200
F 0 "BCA" H 3000 3900 50  0000 C CNN
F 1 "I={CSTATE*((CA_GAIN*(0.75*max(V(CURR,VSS),0)+0.25*max(V(LOCAL,VSS),0)*min(max(V(BRANCH,VSS),1),3))-V(CA,VSS))/TAU_CA)}" H 3000 4500 50  0001 C CNN
F 2 "" H 3000 4200 50  0001 C CNN
F 3 "" H 3000 4200 50  0001 C CNN
F 4 "B" H 3000 4200 50  0001 C CNN "Spice_Primitive"
F 5 "1 2" H 3000 4200 50  0001 C CNN "Spice_Node_Sequence"
F 6 "Y" H 3000 4200 50  0001 C CNN "Spice_Netlist_Enabled"
	1    3000 4200
	1    0    0    -1
$EndComp
Text Label 2550 4200 2    45   ~ 0
CA
Text Label 3450 4200 0    45   ~ 0
VSS
Text Notes 2350 4650 0    35   ~ 0
B:
Text Notes 2350 4800 0    35   ~ 0
I={CSTATE*((CA_GAIN*(0.75*max(V(CURR,VSS),0)+0.25*max(V(LOCAL,VSS),0)*min(max(V(BRANCH,VSS),1),3))-V(CA,VSS))/TAU_CA)}
$Comp
L AARNN_BSOURCE BTRIGGER
U 1 1 66063901
P 8000 4200
F 0 "BTRIGGER" H 8000 3900 50  0000 C CNN
F 1 "V={min(max(max(V(CA,VSS)-PLATEAU_THRESHOLD,0)/(1+max(V(CA,VSS)-PLATEAU_THRESHOLD,0)),0),1)}" H 8000 4500 50  0001 C CNN
F 2 "" H 8000 4200 50  0001 C CNN
F 3 "" H 8000 4200 50  0001 C CNN
F 4 "B" H 8000 4200 50  0001 C CNN "Spice_Primitive"
F 5 "1 2" H 8000 4200 50  0001 C CNN "Spice_Node_Sequence"
F 6 "Y" H 8000 4200 50  0001 C CNN "Spice_Netlist_Enabled"
	1    8000 4200
	1    0    0    -1
$EndComp
Text Label 7550 4200 2    45   ~ 0
TRIGGER
Text Label 8450 4200 0    45   ~ 0
VSS
Text Notes 7350 4650 0    35   ~ 0
B:
Text Notes 7350 4800 0    35   ~ 0
V={min(max(max(V(CA,VSS)-PLATEAU_THRESHOLD,0)/(1+max(V(CA,VSS)-PLATEAU_THRESHOLD,0)),0),1)}
$Comp
L AARNN_BSOURCE BPLATEAU
U 1 1 66063902
P 13000 4200
F 0 "BPLATEAU" H 13000 3900 50  0000 C CNN
F 1 "I={CSTATE*(V(TRIGGER,VSS)-V(PLATEAU,VSS))/TAU_PLATEAU}" H 13000 4500 50  0001 C CNN
F 2 "" H 13000 4200 50  0001 C CNN
F 3 "" H 13000 4200 50  0001 C CNN
F 4 "B" H 13000 4200 50  0001 C CNN "Spice_Primitive"
F 5 "1 2" H 13000 4200 50  0001 C CNN "Spice_Node_Sequence"
F 6 "Y" H 13000 4200 50  0001 C CNN "Spice_Netlist_Enabled"
	1    13000 4200
	1    0    0    -1
$EndComp
Text Label 12550 4200 2    45   ~ 0
PLATEAU
Text Label 13450 4200 0    45   ~ 0
VSS
Text Notes 12350 4650 0    35   ~ 0
B: I={CSTATE*(V(TRIGGER,VSS)-V(PLATEAU,VSS))/TAU_PLATEAU}
$Comp
L AARNN_BSOURCE BOUT
U 1 1 66063903
P 3000 5900
F 0 "BOUT" H 3000 5600 50  0000 C CNN
F 1 "V={V(CURR,VSS)*(1+PLATEAU_GAIN*V(PLATEAU,VSS)*min(max(V(BRANCH,VSS),1),3)*(0.625+0.375*tanh(V(CURR,VSS)/SIGN_SMOOTH)))}" H 3000 6200 50  0001 C CNN
F 2 "" H 3000 5900 50  0001 C CNN
F 3 "" H 3000 5900 50  0001 C CNN
F 4 "B" H 3000 5900 50  0001 C CNN "Spice_Primitive"
F 5 "1 2" H 3000 5900 50  0001 C CNN "Spice_Node_Sequence"
F 6 "Y" H 3000 5900 50  0001 C CNN "Spice_Netlist_Enabled"
	1    3000 5900
	1    0    0    -1
$EndComp
Text Label 2550 5900 2    45   ~ 0
OUT
Text Label 3450 5900 0    45   ~ 0
VSS
Text Notes 2350 6350 0    35   ~ 0
B:
Text Notes 2350 6500 0    35   ~ 0
V={V(CURR,VSS)*(1+PLATEAU_GAIN*V(PLATEAU,VSS)*min(max(V(BRANCH,VSS),1),3)*(0.625+0.375*tanh(V(CURR,VSS)/SIGN_SMOOTH)))}
$Comp
L AARNN_RESISTOR RCA
U 1 1 66063904
P 8000 5900
F 0 "RCA" H 8000 5600 50  0000 C CNN
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
CA
Text Label 8450 5900 0    45   ~ 0
VSS
Text Notes 7350 6350 0    35   ~ 0
R: 1G
$Comp
L AARNN_RESISTOR RPLATEAU
U 1 1 66063905
P 13000 5900
F 0 "RPLATEAU" H 13000 5600 50  0000 C CNN
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
PLATEAU
Text Label 13450 5900 0    45   ~ 0
VSS
Text Notes 12350 6350 0    35   ~ 0
R: 1G
$Comp
L AARNN_RESISTOR RTRIGGER
U 1 1 66063906
P 3000 7600
F 0 "RTRIGGER" H 3000 7300 50  0000 C CNN
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
TRIGGER
Text Label 3450 7600 0    45   ~ 0
VSS
Text Notes 2350 8050 0    35   ~ 0
R: 1G
$Comp
L AARNN_RESISTOR ROUT
U 1 1 66063907
P 8000 7600
F 0 "ROUT" H 8000 7300 50  0000 C CNN
F 1 "1G" H 8000 7900 50  0001 C CNN
F 2 "" H 8000 7600 50  0001 C CNN
F 3 "" H 8000 7600 50  0001 C CNN
F 4 "R" H 8000 7600 50  0001 C CNN "Spice_Primitive"
F 5 "1 2" H 8000 7600 50  0001 C CNN "Spice_Node_Sequence"
F 6 "Y" H 8000 7600 50  0001 C CNN "Spice_Netlist_Enabled"
	1    8000 7600
	1    0    0    -1
$EndComp
Text Label 7550 7600 2    45   ~ 0
OUT
Text Label 8450 7600 0    45   ~ 0
VSS
Text Notes 7350 8050 0    35   ~ 0
R: 1G
Text Notes 700 11100 0    38   ~ 0
Behavioural expressions are kept as SPICE symbol values; named labels reproduce every source-library node.
$EndSCHEMATC
