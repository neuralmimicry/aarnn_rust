#!/usr/bin/env python3
import argparse
import json
import xml.etree.ElementTree as ET
from xml.dom import minidom

def load_snapshot(path):
    with open(path, 'r') as f:
        return json.load(f)

def prettify(elem):
    rough_string = ET.tostring(elem, 'utf-8')
    reparsed = minidom.parseString(rough_string)
    return reparsed.toprettyxml(indent="    ")

def add_projection(network, id, pre_pop, post_id, synapse, weights, delay="1ms"):
    rows = weights.get('rows', 0)
    cols = weights.get('cols', 0)
    data = weights.get('data', [])
    
    if not data or rows == 0 or cols == 0:
        return

    projection = ET.SubElement(network, 'projection', {
        'id': id,
        'presynapticPopulation': pre_pop,
        'postsynapticPopulation': post_id,
        'synapse': synapse
    })

    count = 0
    for r in range(rows):
        for c in range(cols):
            weight = data[r * cols + c]
            if abs(weight) > 1e-9:
                ET.SubElement(projection, 'connectionWD', {
                    'id': str(count),
                    'preCellId': f'../{pre_pop}/{c}/cell',
                    'postCellId': f'../{post_id}/{r}/cell',
                    'weight': str(weight),
                    'delay': delay
                })
                count += 1

def main():
    parser = argparse.ArgumentParser(description='Export NeuroML from network snapshot')
    parser.add_argument('--in-network', required=True, help='Input network JSON snapshot')
    parser.add_argument('--out-neuroml', required=True, help='Output NeuroML file (.nml)')
    args = parser.parse_args()

    snapshot = load_snapshot(args.in_network)
    net_config = snapshot.get('net', {})
    
    root = ET.Element('neuroml', {
        'xmlns': 'http://www.neuroml.org/schema/neuroml2',
        'xmlns:xs': 'http://www.w3.org/2001/XMLSchema',
        'xmlns:xsi': 'http://www.w3.org/2001/XMLSchema-instance',
        'xsi:schemaLocation': 'http://www.neuroml.org/schema/neuroml2 https://raw.github.com/NeuroML/NeuroML2/development/Schemas/NeuroML2/NeuroML_v2.3.1.xsd',
        'id': 'exported_network'
    })

    # Add generic synapse
    ET.SubElement(root, 'expTwoSynapse', {
        'id': 'synapse_generic',
        'gbase': '1nS',
        'erev': '0mV',
        'tauDecay': '10ms',
        'tauRise': '3ms'
    })

    # Add generic cell
    ET.SubElement(root, 'iafCell', {
        'id': 'cell',
        'leakReversal': '-70mV',
        'thresh': '-50mV',
        'reset': '-70mV',
        'C': '1nF',
        'leakConductance': '10nS'
    })

    network = ET.SubElement(root, 'network', {'id': 'net'})

    # Populations
    num_sensory = net_config.get('num_sensory_neurons', 0)
    if num_sensory > 0:
        ET.SubElement(network, 'population', {
            'id': 'Sensory',
            'component': 'cell',
            'size': str(num_sensory),
            'type': 'populationList'
        })

    num_hidden_layers = net_config.get('num_hidden_layers', 0)
    for i in range(num_hidden_layers):
        # We need the actual size of the layer from the weights or topology
        # Usually rows of w_hh_fwd[i-1] or cols of w_hh_fwd[i]
        # Better yet, check the snapshot structure for v_h or similar
        size = 0
        if i == 0:
            size = snapshot.get('w_in', {}).get('rows', 0)
        else:
            size = snapshot.get('w_hh_fwd', [])[i-1].get('rows', 0)
        
        if size > 0:
            ET.SubElement(network, 'population', {
                'id': f'Hidden_{i}',
                'component': 'cell',
                'size': str(size),
                'type': 'populationList'
            })

    num_output = net_config.get('num_output_neurons', 0)
    if num_output > 0:
        ET.SubElement(network, 'population', {
            'id': 'Output',
            'component': 'cell',
            'size': str(num_output),
            'type': 'populationList'
        })

    # Projections
    add_projection(network, 'Proj_In', 'Sensory', 'Hidden_0', 'synapse_generic', snapshot.get('w_in', {}))
    
    w_fwd = snapshot.get('w_hh_fwd', [])
    for i, w in enumerate(w_fwd):
        add_projection(network, f'Proj_Hidden_Fwd_{i}', f'Hidden_{i}', f'Hidden_{i+1}', 'synapse_generic', w)
        
    w_rec = snapshot.get('w_hh_rec', [])
    for i, w in enumerate(w_rec):
        add_projection(network, f'Proj_Hidden_Rec_{i}', f'Hidden_{i}', f'Hidden_{i}', 'synapse_generic', w)

    add_projection(network, 'Proj_Out', f'Hidden_{num_hidden_layers-1}', 'Output', 'synapse_generic', snapshot.get('w_out', {}))

    with open(args.out_neuroml, 'w') as f:
        f.write(prettify(root))
    
    print(f"Exported NeuroML to {args.out_neuroml}")

if __name__ == '__main__':
    main()
