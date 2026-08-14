EESchema Schematic File Version 4
LIBS:tb_aarnn_kernels-cache
EELAYER 29 0
EELAYER END
$Descr A3 16535 11693
encoding utf-8
Sheet 1 1
Title "AARNN_TRIPLET implementation reference"
Date "2026-08-12"
Rev "1.3"
Comp "NeuralMimicry Ltd"
Comment1 "Expanded from aarnn_kernels.lib"
Comment2 "Equation-level SPICE; not a transistor-level or PCB implementation"
Comment3 "Standalone reference sheet; main testbench uses the external .lib model"
Comment4 "1 V = 1 normalised AARNN unit"
$EndDescr
Text Notes 700 600 0    100  ~ 20
AARNN_TRIPLET
Text Notes 700 850 0    50   ~ 0
Source: aarnn_kernels.lib | 8 elements | pin order preserved exactly
Text Notes 700 1100 0    60   ~ 12
DEFAULT PARAMETERS
Text Notes 700 1350 0    40   ~ 0
TAU_PRE=50m TAU_POST=50m TAU_RATE=2 LTP_GAIN=0.25 LTD_GAIN=0.15 CSTATE=1u
Text Notes 700 2050 0    60   ~ 12
EXPOSED SUBCIRCUIT PINS
Text HLabel 1000 2350 0    50   Input ~ 0
PRE
Text Notes 1350 2365 0    40   ~ 0
pin 1 / input
Text HLabel 1000 2600 0    50   Input ~ 0
POST
Text Notes 1350 2615 0    40   ~ 0
pin 2 / input
Text HLabel 1000 2850 0    50   Input ~ 0
RATE
Text Notes 1350 2865 0    40   ~ 0
pin 3 / input
Text HLabel 15500 2350 2    50   Output ~ 0
PRE_TRACE
Text Notes 14200 2365 0    40   ~ 0
pin 4 / output
Text HLabel 15500 2600 2    50   Output ~ 0
POST_TRACE
Text Notes 14200 2615 0    40   ~ 0
pin 5 / output
Text HLabel 15500 2850 2    50   Output ~ 0
RATE_TRACE
Text Notes 14200 2865 0    40   ~ 0
pin 6 / output
Text HLabel 15500 3100 2    50   Output ~ 0
ETA
Text Notes 14200 3115 0    40   ~ 0
pin 7 / output
Text HLabel 8000 2500 1    50   BiDi ~ 0
VSS
Text Notes 8150 2520 0    40   ~ 0
pin 8 / analogue reference
$Comp
L AARNN_BSOURCE BP
U 1 1 6603F300
P 3000 4200
F 0 "BP" H 3000 3900 50  0000 C CNN
F 1 "I={CSTATE*(max(V(PRE,VSS),0)-V(PRE_TRACE,VSS))/TAU_PRE}" H 3000 4500 50  0001 C CNN
F 2 "" H 3000 4200 50  0001 C CNN
F 3 "" H 3000 4200 50  0001 C CNN
F 4 "B" H 3000 4200 50  0001 C CNN "Spice_Primitive"
F 5 "1 2" H 3000 4200 50  0001 C CNN "Spice_Node_Sequence"
F 6 "Y" H 3000 4200 50  0001 C CNN "Spice_Netlist_Enabled"
	1    3000 4200
	1    0    0    -1
$EndComp
Text Label 2550 4200 2    45   ~ 0
PRE_TRACE
Text Label 3450 4200 0    45   ~ 0
VSS
Text Notes 2350 4650 0    35   ~ 0
B: I={CSTATE*(max(V(PRE,VSS),0)-V(PRE_TRACE,VSS))/TAU_PRE}
$Comp
L AARNN_BSOURCE BPOST
U 1 1 6603F301
P 8000 4200
F 0 "BPOST" H 8000 3900 50  0000 C CNN
F 1 "I={CSTATE*(max(V(POST,VSS),0)-V(POST_TRACE,VSS))/TAU_POST}" H 8000 4500 50  0001 C CNN
F 2 "" H 8000 4200 50  0001 C CNN
F 3 "" H 8000 4200 50  0001 C CNN
F 4 "B" H 8000 4200 50  0001 C CNN "Spice_Primitive"
F 5 "1 2" H 8000 4200 50  0001 C CNN "Spice_Node_Sequence"
F 6 "Y" H 8000 4200 50  0001 C CNN "Spice_Netlist_Enabled"
	1    8000 4200
	1    0    0    -1
$EndComp
Text Label 7550 4200 2    45   ~ 0
POST_TRACE
Text Label 8450 4200 0    45   ~ 0
VSS
Text Notes 7350 4650 0    35   ~ 0
B:
Text Notes 7350 4800 0    35   ~ 0
I={CSTATE*(max(V(POST,VSS),0)-V(POST_TRACE,VSS))/TAU_POST}
$Comp
L AARNN_BSOURCE BR
U 1 1 6603F302
P 13000 4200
F 0 "BR" H 13000 3900 50  0000 C CNN
F 1 "I={CSTATE*(max(V(RATE,VSS),0)-V(RATE_TRACE,VSS))/TAU_RATE}" H 13000 4500 50  0001 C CNN
F 2 "" H 13000 4200 50  0001 C CNN
F 3 "" H 13000 4200 50  0001 C CNN
F 4 "B" H 13000 4200 50  0001 C CNN "Spice_Primitive"
F 5 "1 2" H 13000 4200 50  0001 C CNN "Spice_Node_Sequence"
F 6 "Y" H 13000 4200 50  0001 C CNN "Spice_Netlist_Enabled"
	1    13000 4200
	1    0    0    -1
$EndComp
Text Label 12550 4200 2    45   ~ 0
RATE_TRACE
Text Label 13450 4200 0    45   ~ 0
VSS
Text Notes 12350 4650 0    35   ~ 0
B:
Text Notes 12350 4800 0    35   ~ 0
I={CSTATE*(max(V(RATE,VSS),0)-V(RATE_TRACE,VSS))/TAU_RATE}
$Comp
L AARNN_BSOURCE BETA
U 1 1 6603F303
P 3000 5900
F 0 "BETA" H 3000 5600 50  0000 C CNN
F 1 "V={min(max(1+LTP_GAIN*V(PRE_TRACE,VSS)*V(POST_TRACE,VSS)-LTD_GAIN*V(RATE_TRACE,VSS),0.05),5)}" H 3000 6200 50  0001 C CNN
F 2 "" H 3000 5900 50  0001 C CNN
F 3 "" H 3000 5900 50  0001 C CNN
F 4 "B" H 3000 5900 50  0001 C CNN "Spice_Primitive"
F 5 "1 2" H 3000 5900 50  0001 C CNN "Spice_Node_Sequence"
F 6 "Y" H 3000 5900 50  0001 C CNN "Spice_Netlist_Enabled"
	1    3000 5900
	1    0    0    -1
$EndComp
Text Label 2550 5900 2    45   ~ 0
ETA
Text Label 3450 5900 0    45   ~ 0
VSS
Text Notes 2350 6350 0    35   ~ 0
B:
Text Notes 2350 6500 0    35   ~ 0
V={min(max(1+LTP_GAIN*V(PRE_TRACE,VSS)*V(POST_TRACE,VSS)-LTD_GAIN*V(RATE_TRACE,VSS),0.05),5)}
$Comp
L AARNN_RESISTOR RPT
U 1 1 6603F304
P 8000 5900
F 0 "RPT" H 8000 5600 50  0000 C CNN
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
PRE_TRACE
Text Label 8450 5900 0    45   ~ 0
VSS
Text Notes 7350 6350 0    35   ~ 0
R: 1G
$Comp
L AARNN_RESISTOR RQT
U 1 1 6603F305
P 13000 5900
F 0 "RQT" H 13000 5600 50  0000 C CNN
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
POST_TRACE
Text Label 13450 5900 0    45   ~ 0
VSS
Text Notes 12350 6350 0    35   ~ 0
R: 1G
$Comp
L AARNN_RESISTOR RRT
U 1 1 6603F306
P 3000 7600
F 0 "RRT" H 3000 7300 50  0000 C CNN
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
RATE_TRACE
Text Label 3450 7600 0    45   ~ 0
VSS
Text Notes 2350 8050 0    35   ~ 0
R: 1G
$Comp
L AARNN_RESISTOR RETA
U 1 1 6603F307
P 8000 7600
F 0 "RETA" H 8000 7300 50  0000 C CNN
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
ETA
Text Label 8450 7600 0    45   ~ 0
VSS
Text Notes 7350 8050 0    35   ~ 0
R: 1G
Text Notes 700 11100 0    38   ~ 0
Behavioural expressions are kept as SPICE symbol values; named labels reproduce every source-library node.
$EndSCHEMATC
