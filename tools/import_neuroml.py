#!/usr/bin/env python3
import argparse
import json
import xml.etree.ElementTree as ET
import os

def main():
    parser = argparse.ArgumentParser(description='Import NeuroML 2.0 into network snapshot JSON')
    parser.add_argument('--in', dest='input_file', required=True, help='Input NeuroML file (.nml)')
    parser.add_argument('--out-network', required=True, help='Output network JSON snapshot')
    args = parser.parse_args()

    if not os.path.exists(args.input_file):
        print(f"Error: file {args.input_file} not found")
        exit(1)

    try:
        tree = ET.parse(args.input_file)
        root = tree.getroot()
    except Exception as e:
        print(f"Error parsing NeuroML: {e}")
        exit(1)

    # Use namespaces
    ns = {'nml': 'http://www.neuroml.org/schema/neuroml2'}
    
    network = root.find('nml:network', ns)
    if network is None:
        # try without namespace if not found
        network = root.find('network')
        if network is None:
            print("Error: <network> element not found in NeuroML")
            exit(1)
        ns = {} # reset namespaces if not used

    populations = {}
    for pop in network.findall('nml:population', ns) if ns else network.findall('population'):
        pop_id = pop.get('id')
        size = int(pop.get('size', 0))
        populations[pop_id] = size

    # Prepare snapshot structure
    # We'll try to find Sensory, Hidden_X, and Output populations
    num_sensory = populations.get('Sensory', 0)
    num_output = populations.get('Output', 0)
    
    hidden_layer_ids = sorted([k for k in populations.keys() if k.startswith('Hidden_')], 
                              key=lambda x: int(x.split('_')[1]) if '_' in x else 0)
    num_hidden_layers = len(hidden_layer_ids)
    
    hidden_sizes = [populations[hid] for hid in hidden_layer_ids]

    def get_matrix(rows, cols):
        return {"rows": rows, "cols": cols, "data": [0.0] * (rows * cols)}

    snapshot = {
        "net": {
            "num_sensory_neurons": num_sensory,
            "num_hidden_layers": num_hidden_layers,
            "num_hidden_per_layer_initial": hidden_sizes[0] if hidden_sizes else 0,
            "num_output_neurons": num_output,
            "p_in": 0.15,
            "p_hidden": 0.10,
            "p_out": 0.15
        },
        "w_in": get_matrix(hidden_sizes[0] if hidden_sizes else 0, num_sensory),
        "w_hh_fwd": [get_matrix(hidden_sizes[i+1], hidden_sizes[i]) for i in range(num_hidden_layers - 1)],
        "w_hh_bwd": [get_matrix(hidden_sizes[i], hidden_sizes[i+1]) for i in range(num_hidden_layers - 1)],
        "w_hh_rec": [get_matrix(hidden_sizes[i], hidden_sizes[i]) for i in range(num_hidden_layers)],
        "w_out": get_matrix(num_output, hidden_sizes[-1] if hidden_sizes else 0)
    }

    # Projections
    for proj in network.findall('nml:projection', ns) if ns else network.findall('projection'):
        pre_pop = proj.get('presynapticPopulation')
        post_pop = proj.get('postsynapticPopulation')
        
        target_matrix = None
        if pre_pop == 'Sensory' and post_pop == 'Hidden_0':
            target_matrix = snapshot['w_in']
        elif pre_pop == 'Output' or post_pop == 'Sensory':
            continue # not supported or back-loop
        elif pre_pop.startswith('Hidden_') and post_pop.startswith('Hidden_'):
            pre_idx = int(pre_pop.split('_')[1])
            post_idx = int(post_pop.split('_')[1])
            if post_idx == pre_idx + 1:
                target_matrix = snapshot['w_hh_fwd'][pre_idx]
            elif post_idx == pre_idx - 1:
                target_matrix = snapshot['w_hh_bwd'][post_idx]
            elif post_idx == pre_idx:
                target_matrix = snapshot['w_hh_rec'][pre_idx]
        elif pre_pop.startswith('Hidden_') and post_pop == 'Output':
            target_matrix = snapshot['w_out']

        if target_matrix:
            rows = target_matrix['rows']
            cols = target_matrix['cols']
            for conn in proj.findall('nml:connectionWD', ns) if ns else proj.findall('connectionWD'):
                # preCellId format: ../Pop/idx/cell
                pre_id_str = conn.get('preCellId')
                post_id_str = conn.get('postCellId')
                weight = float(conn.get('weight', 0.0))
                
                try:
                    pre_id = int(pre_id_str.split('/')[-2])
                    post_id = int(post_id_str.split('/')[-2])
                    if post_id < rows and pre_id < cols:
                        target_matrix['data'][post_id * cols + pre_id] = weight
                except:
                    continue

    with open(args.out_network, 'w') as f:
        json.dump(snapshot, f, indent=2)
    
    print(f"Imported NeuroML to {args.out_network}")

if __name__ == '__main__':
    main()
