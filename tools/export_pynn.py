#!/usr/bin/env python3
import argparse
import json

def load_snapshot(path):
    with open(path, 'r') as f:
        return json.load(f)

def main():
    parser = argparse.ArgumentParser(description='Export PyNN script from network snapshot')
    parser.add_argument('--in-network', required=True, help='Input network JSON snapshot')
    parser.add_argument('--out-pynn', required=True, help='Output PyNN Python script (.py)')
    args = parser.parse_args()

    snapshot = load_snapshot(args.in_network)
    net_config = snapshot.get('net', {})
    
    with open(args.out_pynn, 'w') as f:
        f.write("#!/usr/bin/env python3\n")
        f.write("# Generated PyNN script from aarnn_rust\n\n")
        f.write("import pyNN.neuron as sim\n")
        f.write("import numpy as np\n\n")
        
        f.write("sim.setup(timestep=1.0)\n\n")
        
        # Cell parameters (approximate mapping from LIF)
        f.write("cell_params = {\n")
        f.write("    'tau_m': 20.0,\n")
        f.write("    'v_rest': -70.0,\n")
        f.write("    'v_thresh': -50.0,\n")
        f.write("    'v_reset': -70.0,\n")
        f.write("    'cm': 1.0,\n")
        f.write("    'tau_refrac': 2.0\n")
        f.write("}\n\n")
        
        # Populations
        num_sensory = net_config.get('num_sensory_neurons', 0)
        if num_sensory > 0:
            f.write(f"sensory = sim.Population({num_sensory}, sim.IF_curr_exp(**cell_params), label='Sensory')\n")
            
        num_hidden_layers = net_config.get('num_hidden_layers', 0)
        f.write("hidden = []\n")
        for i in range(num_hidden_layers):
            if i == 0:
                size = snapshot.get('w_in', {}).get('rows', 0)
            else:
                size = snapshot.get('w_hh_fwd', [])[i-1].get('rows', 0)
            
            if size > 0:
                f.write(f"hidden.append(sim.Population({size}, sim.IF_curr_exp(**cell_params), label='Hidden_{i}'))\n")
        
        num_output = net_config.get('num_output_neurons', 0)
        if num_output > 0:
            f.write(f"output = sim.Population({num_output}, sim.IF_curr_exp(**cell_params), label='Output')\n")
        
        f.write("\n# Projections\n")
        
        def write_projection(name, pre_pop, post_pop, weights):
            rows = weights.get('rows', 0)
            cols = weights.get('cols', 0)
            data = weights.get('data', [])
            if not data or rows == 0 or cols == 0:
                return
            
            f.write(f"\n# {name}\n")
            f.write(f"weights_{name} = np.array({data}).reshape({rows}, {cols})\n")
            f.write(f"connector_{name} = []\n")
            f.write(f"for r in range({rows}):\n")
            f.write(f"    for c in range({cols}):\n")
            f.write(f"        w = weights_{name}[r, c]\n")
            f.write(f"        if abs(w) > 1e-9:\n")
            f.write(f"            connector_{name}.append((c, r, w, 1.0)) # pre, post, weight, delay\n")
            
            f.write(f"sim.Projection({pre_pop}, {post_pop}, sim.FromListConnector(connector_{name}), label='{name}')\n")

        write_projection('In', 'sensory', 'hidden[0]', snapshot.get('w_in', {}))
        
        w_fwd = snapshot.get('w_hh_fwd', [])
        for i, w in enumerate(w_fwd):
            write_projection(f'Hidden_Fwd_{i}', f'hidden[{i}]', f'hidden[{i+1}]', w)
            
        w_rec = snapshot.get('w_hh_rec', [])
        for i, w in enumerate(w_rec):
            write_projection(f'Hidden_Rec_{i}', f'hidden[{i}]', f'hidden[{i}]', w)
            
        write_projection('Out', f'hidden[{num_hidden_layers-1}]', 'output', snapshot.get('w_out', {}))
        
        f.write("\nprint('Network assembled in PyNN')\n")
        f.write("# sim.run(1000.0)\n")
        f.write("# sim.end()\n")

    print(f"Exported PyNN script to {args.out_pynn}")

if __name__ == '__main__':
    main()
