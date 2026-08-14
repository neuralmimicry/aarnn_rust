EESchema Schematic File Version 4
LIBS:tb_aarnn_kernels-cache
EELAYER 29 0
EELAYER END
$Descr A3 16535 11693
encoding utf-8
Sheet 1 1
Title "AARNN_SYNAPTIC_FILTER implementation reference"
Date "2026-08-12"
Rev "1.3"
Comp "NeuralMimicry Ltd"
Comment1 "Expanded from aarnn_kernels.lib"
Comment2 "Equation-level SPICE; not a transistor-level or PCB implementation"
Comment3 "Standalone reference sheet; main testbench uses the external .lib model"
Comment4 "1 V = 1 normalised AARNN unit"
$EndDescr
Text Notes 700 600 0    100  ~ 20
AARNN_SYNAPTIC_FILTER
Text Notes 700 850 0    50   ~ 0
Source: aarnn_kernels.lib | 11 elements | pin order preserved exactly
Text Notes 700 1100 0    60   ~ 12
DEFAULT PARAMETERS
Text Notes 700 1350 0    40   ~ 0
TAU_AMPA=5m TAU_NMDA=100m TAU_GABA=10m NMDA_RATIO=0.25 SYN_GAIN=1 NMDA_SENS=0.04 ACH_GAIN=1 CSTATE=1u
Text Notes 700 2050 0    60   ~ 12
EXPOSED SUBCIRCUIT PINS
Text HLabel 1000 2350 0    50   Input ~ 0
RAW
Text Notes 1350 2365 0    40   ~ 0
pin 1 / input
Text HLabel 1000 2600 0    50   Input ~ 0
VM
Text Notes 1350 2615 0    40   ~ 0
pin 2 / input
Text HLabel 15500 2350 2    50   Output ~ 0
AMPA
Text Notes 14200 2365 0    40   ~ 0
pin 3 / output
Text HLabel 15500 2600 2    50   Output ~ 0
NMDA
Text Notes 14200 2615 0    40   ~ 0
pin 4 / output
Text HLabel 15500 2850 2    50   Output ~ 0
GABA
Text Notes 14200 2865 0    40   ~ 0
pin 5 / output
Text HLabel 15500 3100 2    50   Output ~ 0
OUT
Text Notes 14200 3115 0    40   ~ 0
pin 6 / output
Text HLabel 8000 2500 1    50   BiDi ~ 0
VSS
Text Notes 8150 2520 0    40   ~ 0
pin 7 / analogue reference
$Comp
L AARNN_BSOURCE BEXC
U 1 1 66065F00
P 3000 4200
F 0 "BEXC" H 3000 3900 50  0000 C CNN
F 1 "V={max(V(RAW,VSS),0)}" H 3000 4500 50  0001 C CNN
F 2 "" H 3000 4200 50  0001 C CNN
F 3 "" H 3000 4200 50  0001 C CNN
F 4 "B" H 3000 4200 50  0001 C CNN "Spice_Primitive"
F 5 "1 2" H 3000 4200 50  0001 C CNN "Spice_Node_Sequence"
F 6 "Y" H 3000 4200 50  0001 C CNN "Spice_Netlist_Enabled"
	1    3000 4200
	1    0    0    -1
$EndComp
Text Label 2550 4200 2    45   ~ 0
EXC
Text Label 3450 4200 0    45   ~ 0
VSS
Text Notes 2350 4650 0    35   ~ 0
B: V={max(V(RAW,VSS),0)}
$Comp
L AARNN_BSOURCE BINH
U 1 1 66065F01
P 8000 4200
F 0 "BINH" H 8000 3900 50  0000 C CNN
F 1 "V={max(-V(RAW,VSS),0)}" H 8000 4500 50  0001 C CNN
F 2 "" H 8000 4200 50  0001 C CNN
F 3 "" H 8000 4200 50  0001 C CNN
F 4 "B" H 8000 4200 50  0001 C CNN "Spice_Primitive"
F 5 "1 2" H 8000 4200 50  0001 C CNN "Spice_Node_Sequence"
F 6 "Y" H 8000 4200 50  0001 C CNN "Spice_Netlist_Enabled"
	1    8000 4200
	1    0    0    -1
$EndComp
Text Label 7550 4200 2    45   ~ 0
INH
Text Label 8450 4200 0    45   ~ 0
VSS
Text Notes 7350 4650 0    35   ~ 0
B: V={max(-V(RAW,VSS),0)}
$Comp
L AARNN_BSOURCE BNMDAG
U 1 1 66065F02
P 13000 4200
F 0 "BNMDAG" H 13000 3900 50  0000 C CNN
F 1 "V={1/(1+exp(-NMDA_SENS*(V(VM,VSS)/1m+40)))}" H 13000 4500 50  0001 C CNN
F 2 "" H 13000 4200 50  0001 C CNN
F 3 "" H 13000 4200 50  0001 C CNN
F 4 "B" H 13000 4200 50  0001 C CNN "Spice_Primitive"
F 5 "1 2" H 13000 4200 50  0001 C CNN "Spice_Node_Sequence"
F 6 "Y" H 13000 4200 50  0001 C CNN "Spice_Netlist_Enabled"
	1    13000 4200
	1    0    0    -1
$EndComp
Text Label 12550 4200 2    45   ~ 0
NMDA_GATE
Text Label 13450 4200 0    45   ~ 0
VSS
Text Notes 12350 4650 0    35   ~ 0
B: V={1/(1+exp(-NMDA_SENS*(V(VM,VSS)/1m+40)))}
$Comp
L AARNN_BSOURCE BAMPA_DRV
U 1 1 66065F03
P 3000 5900
F 0 "BAMPA_DRV" H 3000 5600 50  0000 C CNN
F 1 "I={CSTATE*((1-NMDA_RATIO)*V(EXC,VSS)-V(AMPA,VSS))/TAU_AMPA}" H 3000 6200 50  0001 C CNN
F 2 "" H 3000 5900 50  0001 C CNN
F 3 "" H 3000 5900 50  0001 C CNN
F 4 "B" H 3000 5900 50  0001 C CNN "Spice_Primitive"
F 5 "1 2" H 3000 5900 50  0001 C CNN "Spice_Node_Sequence"
F 6 "Y" H 3000 5900 50  0001 C CNN "Spice_Netlist_Enabled"
	1    3000 5900
	1    0    0    -1
$EndComp
Text Label 2550 5900 2    45   ~ 0
AMPA
Text Label 3450 5900 0    45   ~ 0
VSS
Text Notes 2350 6350 0    35   ~ 0
B:
Text Notes 2350 6500 0    35   ~ 0
I={CSTATE*((1-NMDA_RATIO)*V(EXC,VSS)-V(AMPA,VSS))/TAU_AMPA}
$Comp
L AARNN_BSOURCE BNMDA_DRV
U 1 1 66065F04
P 8000 5900
F 0 "BNMDA_DRV" H 8000 5600 50  0000 C CNN
F 1 "I={CSTATE*(NMDA_RATIO*V(EXC,VSS)*V(NMDA_GATE,VSS)-V(NMDA,VSS))/TAU_NMDA}" H 8000 6200 50  0001 C CNN
F 2 "" H 8000 5900 50  0001 C CNN
F 3 "" H 8000 5900 50  0001 C CNN
F 4 "B" H 8000 5900 50  0001 C CNN "Spice_Primitive"
F 5 "1 2" H 8000 5900 50  0001 C CNN "Spice_Node_Sequence"
F 6 "Y" H 8000 5900 50  0001 C CNN "Spice_Netlist_Enabled"
	1    8000 5900
	1    0    0    -1
$EndComp
Text Label 7550 5900 2    45   ~ 0
NMDA
Text Label 8450 5900 0    45   ~ 0
VSS
Text Notes 7350 6350 0    35   ~ 0
B:
Text Notes 7350 6500 0    35   ~ 0
I={CSTATE*(NMDA_RATIO*V(EXC,VSS)*V(NMDA_GATE,VSS)-V(NMDA,VSS))/TAU_NMDA}
$Comp
L AARNN_BSOURCE BGABA_DRV
U 1 1 66065F05
P 13000 5900
F 0 "BGABA_DRV" H 13000 5600 50  0000 C CNN
F 1 "I={CSTATE*(V(INH,VSS)-V(GABA,VSS))/TAU_GABA}" H 13000 6200 50  0001 C CNN
F 2 "" H 13000 5900 50  0001 C CNN
F 3 "" H 13000 5900 50  0001 C CNN
F 4 "B" H 13000 5900 50  0001 C CNN "Spice_Primitive"
F 5 "1 2" H 13000 5900 50  0001 C CNN "Spice_Node_Sequence"
F 6 "Y" H 13000 5900 50  0001 C CNN "Spice_Netlist_Enabled"
	1    13000 5900
	1    0    0    -1
$EndComp
Text Label 12550 5900 2    45   ~ 0
GABA
Text Label 13450 5900 0    45   ~ 0
VSS
Text Notes 12350 6350 0    35   ~ 0
B: I={CSTATE*(V(INH,VSS)-V(GABA,VSS))/TAU_GABA}
$Comp
L AARNN_BSOURCE BOUT
U 1 1 66065F06
P 3000 7600
F 0 "BOUT" H 3000 7300 50  0000 C CNN
F 1 "V={SYN_GAIN*ACH_GAIN*(V(AMPA,VSS)+V(NMDA,VSS)-V(GABA,VSS))}" H 3000 7900 50  0001 C CNN
F 2 "" H 3000 7600 50  0001 C CNN
F 3 "" H 3000 7600 50  0001 C CNN
F 4 "B" H 3000 7600 50  0001 C CNN "Spice_Primitive"
F 5 "1 2" H 3000 7600 50  0001 C CNN "Spice_Node_Sequence"
F 6 "Y" H 3000 7600 50  0001 C CNN "Spice_Netlist_Enabled"
	1    3000 7600
	1    0    0    -1
$EndComp
Text Label 2550 7600 2    45   ~ 0
OUT
Text Label 3450 7600 0    45   ~ 0
VSS
Text Notes 2350 8050 0    35   ~ 0
B:
Text Notes 2350 8200 0    35   ~ 0
V={SYN_GAIN*ACH_GAIN*(V(AMPA,VSS)+V(NMDA,VSS)-V(GABA,VSS))}
$Comp
L AARNN_RESISTOR RSTATE_A
U 1 1 66065F07
P 8000 7600
F 0 "RSTATE_A" H 8000 7300 50  0000 C CNN
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
AMPA
Text Label 8450 7600 0    45   ~ 0
VSS
Text Notes 7350 8050 0    35   ~ 0
R: 1G
$Comp
L AARNN_RESISTOR RSTATE_N
U 1 1 66065F08
P 13000 7600
F 0 "RSTATE_N" H 13000 7300 50  0000 C CNN
F 1 "1G" H 13000 7900 50  0001 C CNN
F 2 "" H 13000 7600 50  0001 C CNN
F 3 "" H 13000 7600 50  0001 C CNN
F 4 "R" H 13000 7600 50  0001 C CNN "Spice_Primitive"
F 5 "1 2" H 13000 7600 50  0001 C CNN "Spice_Node_Sequence"
F 6 "Y" H 13000 7600 50  0001 C CNN "Spice_Netlist_Enabled"
	1    13000 7600
	1    0    0    -1
$EndComp
Text Label 12550 7600 2    45   ~ 0
NMDA
Text Label 13450 7600 0    45   ~ 0
VSS
Text Notes 12350 8050 0    35   ~ 0
R: 1G
$Comp
L AARNN_RESISTOR RSTATE_G
U 1 1 66065F09
P 4800 9300
F 0 "RSTATE_G" H 4800 9000 50  0000 C CNN
F 1 "1G" H 4800 9600 50  0001 C CNN
F 2 "" H 4800 9300 50  0001 C CNN
F 3 "" H 4800 9300 50  0001 C CNN
F 4 "R" H 4800 9300 50  0001 C CNN "Spice_Primitive"
F 5 "1 2" H 4800 9300 50  0001 C CNN "Spice_Node_Sequence"
F 6 "Y" H 4800 9300 50  0001 C CNN "Spice_Netlist_Enabled"
	1    4800 9300
	1    0    0    -1
$EndComp
Text Label 4350 9300 2    45   ~ 0
GABA
Text Label 5250 9300 0    45   ~ 0
VSS
Text Notes 4150 9750 0    35   ~ 0
R: 1G
$Comp
L AARNN_RESISTOR ROUT
U 1 1 66065F0A
P 11200 9300
F 0 "ROUT" H 11200 9000 50  0000 C CNN
F 1 "1G" H 11200 9600 50  0001 C CNN
F 2 "" H 11200 9300 50  0001 C CNN
F 3 "" H 11200 9300 50  0001 C CNN
F 4 "R" H 11200 9300 50  0001 C CNN "Spice_Primitive"
F 5 "1 2" H 11200 9300 50  0001 C CNN "Spice_Node_Sequence"
F 6 "Y" H 11200 9300 50  0001 C CNN "Spice_Netlist_Enabled"
	1    11200 9300
	1    0    0    -1
$EndComp
Text Label 10750 9300 2    45   ~ 0
OUT
Text Label 11650 9300 0    45   ~ 0
VSS
Text Notes 10550 9750 0    35   ~ 0
R: 1G
Text Notes 700 11100 0    38   ~ 0
Behavioural expressions are kept as SPICE symbol values; named labels reproduce every source-library node.
$EndSCHEMATC
