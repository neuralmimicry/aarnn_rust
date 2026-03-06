#!/usr/bin/env python3
"""
Genetic Parameter Search for Neuromorphic Demo

This script implements a Genetic Algorithm (GA) to automatically find optimal
simulation parameters (e.g., tau_m, v_th, learning rate) for the spiking neural network.

The GA optimizes for "fitness", which is currently defined as a balance between
network activity (spiking) and stability. It runs multiple simulations in parallel
using the `multiprocessing` module to speed up the search.

Workflow:
1. Initialize a random population of parameter sets.
2. For each generation:
    a. Run simulations for each parameter set in parallel.
    b. Evaluate the fitness of each individual based on simulation outputs (spikes).
    c. Select the best individuals as parents.
    d. Create a new generation through crossover and mutation.
3. Save the best configuration found.

Usage:
  python3 tools/genetic_param_search.py
"""
import subprocess
import random
import multiprocessing as mp
import json
import os
import shutil
from copy import deepcopy

PARAM_BOUNDS = {
    # Growth and topology
    'num_hidden_layers': (2, 6),
    'num_hidden_per_layer_initial': (2, 64),
    'max_layers': (2, 6),
    'p_in': (0.05, 0.5),
    'p_hidden': (0.01, 0.3),
    'p_out': (0.05, 0.5),
    'growth_enabled': (0, 1),  # 0=False, 1=True
    'saturation_threshold': (0.1, 1.0),
    'saturation_window_ms': (50, 1000),
    'growth_cooldown_ms': (50, 2000),
    'spawn_radius': (0.01, 0.2),
    'migrate_in_prob': (0.0, 1.0),
    'migrate_out_prob': (0.0, 1.0),
    'layer_split_threshold': (8, 64),
    'global_growth_cooldown_ms': (10, 1000),
    # Geometry
    'min_node_sep': (0.005, 0.1),
    'min_segment_sep': (0.001, 0.05),
    'max_place_tries': (4, 32),
    'relax_step': (0.001, 0.02),
    'max_reroute_tries': (2, 16),
    # Growth rates
    'component_decay_rate': (0.0001, 0.1),
    'trunk_growth_rate': (0.0001, 0.1),
    'branch_growth_rate': (0.0001, 0.1),
    'bouton_growth_rate': (0.0001, 0.1),
    # Pruning and connection
    'max_sensory_connections': (2, 32),
    'neuron_removal_delay_ms': (100, 20000),
    'spontaneous_neuron_interval_ms': (100, 20000),
    # AARNN depth + bio dynamics
    'aarnn_layer_depth': (0, 5),
    'aarnn_bio.stp_enabled': (0, 1),
    'aarnn_bio.stp_u': (0.05, 0.6),
    'aarnn_bio.stp_tau_rec_ms': (50, 2000),
    'aarnn_bio.stp_tau_facil_ms': (20, 1000),
    'aarnn_bio.ampa_tau_ms': (1.0, 30.0),
    'aarnn_bio.nmda_tau_ms': (20.0, 250.0),
    'aarnn_bio.gaba_tau_ms': (2.0, 40.0),
    'aarnn_bio.nmda_ratio': (0.0, 0.7),
    'aarnn_bio.synaptic_gain': (0.2, 3.0),
    'aarnn_bio.adaptive_threshold_enabled': (0, 1),
    'aarnn_bio.adaptive_threshold_tau_ms': (20.0, 1000.0),
    'aarnn_bio.adaptive_threshold_increment': (0.1, 2.0),
    'aarnn_bio.adaptive_threshold_min': (-3.0, 0.0),
    'aarnn_bio.adaptive_threshold_max': (1.0, 8.0),
    'aarnn_bio.izh_refractory_ms': (0.0, 5.0),
    'aarnn_bio.homeostasis_target_rate_hz': (0.5, 10.0),
    'aarnn_bio.homeostasis_tau_ms': (200.0, 5000.0),
    'aarnn_bio.homeostasis_gain': (0.0, 1.0),
    'aarnn_bio.neuromodulation_enabled': (0, 1),
    'aarnn_bio.dopamine_gain': (0.5, 2.0),
    'aarnn_bio.acetylcholine_gain': (0.5, 2.0),
    'aarnn_bio.serotonin_gain': (0.5, 2.0),
}
POP_SIZE = 12
N_GENERATIONS = 20
N_ELITE = 2
MUTATION_RATE = 0.2
N_WORKERS = min(POP_SIZE, mp.cpu_count())

CONFIG_PATH = 'config.json'
SIM_BIN = './target/release/neuromorphic_demo'
REBUILD_CMD = ['cargo', 'build', '--release', '--all-features']

# Simulation time settings (increased for meaningful growth)
SIM_TIME_MIN = 10000
SIM_TIME_MAX = 50000
SIM_TIME_STEP = 5000
SIM_TIME_DEFAULT = 20000

# --- UTILITIES ---
def random_param():
    p = {}
    for k, (lo, hi) in PARAM_BOUNDS.items():
        if k in ['growth_enabled', 'aarnn_bio.stp_enabled', 'aarnn_bio.adaptive_threshold_enabled', 'aarnn_bio.neuromodulation_enabled']:
            p[k] = bool(random.randint(lo, hi))
        elif isinstance(lo, int) and isinstance(hi, int):
            p[k] = random.randint(lo, hi)
        else:
            p[k] = random.uniform(lo, hi)
    return p

def mutate_param(p):
    p = deepcopy(p)
    for k, (lo, hi) in PARAM_BOUNDS.items():
        if random.random() < MUTATION_RATE:
            if k in ['growth_enabled', 'aarnn_bio.stp_enabled', 'aarnn_bio.adaptive_threshold_enabled', 'aarnn_bio.neuromodulation_enabled']:
                p[k] = bool(random.randint(lo, hi))
            elif isinstance(lo, int) and isinstance(hi, int):
                p[k] = random.randint(lo, hi)
            else:
                p[k] = random.uniform(lo, hi)
    return p

def crossover(p1, p2):
    c = {}
    for k in PARAM_BOUNDS:
        c[k] = p1[k] if random.random() < 0.5 else p2[k]
    return c

def set_nested(config, key, value):
    if '.' not in key:
        config[key] = value
        return
    parts = key.split('.')
    cur = config
    for part in parts[:-1]:
        if part not in cur or not isinstance(cur[part], dict):
            cur[part] = {}
        cur = cur[part]
    cur[parts[-1]] = value

def write_config(params, config_path=CONFIG_PATH):
    # Always read from the original config.json, never overwrite it
    with open(CONFIG_PATH, 'r') as f:
        config = json.load(f)
    for k, v in params.items():
        set_nested(config, k, v)
    # Always enforce morphology for AARNN search
    config['use_morphology'] = True
    # Always enforce a large simulation time for batch runs
    config['simulation_time_ms'] = SIM_TIME_DEFAULT
    with open(config_path, 'w') as f:
        json.dump(config, f, indent=2)

def run_simulation(params, idx):
    """
    Executes a single simulation run for a given parameter set.
    
    1. Writes the parameters to a temporary config file.
    2. Spawns the Rust simulation binary.
    3. Parses the output (stdout) to extract spike counts and other metrics.
    4. Calculates a fitness score based on the simulation results.
    """
    import time
    print(f"[LOG] run_simulation called (idx={idx}, pid={os.getpid()})")
    # Each worker gets its own config file
    worker_config = f'config_worker_{idx}.json'
    write_config(params, worker_config)
    debug_log = f'genetic_debug_worker_{idx}.log'
    sim_time = SIM_TIME_MIN
    best_score = 0
    best_out = ''
    debug_info = []
    while sim_time <= SIM_TIME_MAX:
        cmd = [SIM_BIN,
               '--brain-id', 'motor',
               '--simulation-time-ms', str(sim_time),
               '--config', worker_config]
        print(f"[LOG] Worker {idx} about to run: {' '.join(cmd)} (sim_time={sim_time})")
        # Log command and config
        with open(worker_config, 'r') as cf:
            config_snapshot = cf.read()
        debug_info.append(f'\n[DEBUG] Running: {" ".join(cmd)}')
        debug_info.append(f'[DEBUG] Config file ({worker_config}):\n{config_snapshot}')
        try:
            t0 = time.time()
            proc = subprocess.run(cmd, stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=sim_time//100+30)
            t1 = time.time()
            out = proc.stdout.decode(errors='ignore')
            err = proc.stderr.decode(errors='ignore')
            debug_info.append(f'[DEBUG] STDOUT (last 1000 chars):\n{out[-1000:]}')
            debug_info.append(f'[DEBUG] STDERR (last 1000 chars):\n{err[-1000:]}')
            print(f"[LOG] Worker {idx} finished subprocess in {t1-t0:.2f}s (sim_time={sim_time})")
        except Exception as e:
            debug_info.append(f'[ERROR] Exception: {e}')
            print(f"[ERROR] Worker {idx} exception: {e}")
            with open(debug_log, 'w') as dbg:
                dbg.write('\n'.join(debug_info))
            return 0, params, f'Error: {e}'
        import re
        layer_sizes = [int(m.group(1)) for m in re.finditer(r'\[summary\]\s+Layer\s+\d+:\s+(\d+)\s+neurons', out)]
        output_match = re.search(r'\[summary\]\s+Output layer neuron count:\s+(\d+)', out)
        output_neurons = int(output_match.group(1)) if output_match else 0
        lt_match = re.search(r'\[summary\]\s+Longterm connections:\s+(\d+)\s+/\s+(\d+)\s+\(([\d\.]+)%\)', out)
        longterm = int(lt_match.group(1)) if lt_match else 0
        total = int(lt_match.group(2)) if lt_match else 0
        longterm_pct = float(lt_match.group(3)) if lt_match else 0.0

        conn_counts = [int(m.group(1)) for m in re.finditer(r'\[summary\]\s+.+:\s+(\d+)', out)]
        total_conn = sum(conn_counts) if conn_counts else total

        total_possible = 0
        if layer_sizes:
            h_in = layer_sizes[0]
            s_count = int(params.get('num_sensory_neurons', 0))
            total_possible += h_in * s_count
            for l in range(len(layer_sizes) - 1):
                total_possible += layer_sizes[l] * layer_sizes[l + 1]  # fwd
                total_possible += layer_sizes[l] * layer_sizes[l + 1]  # bwd
            if output_neurons > 0:
                total_possible += output_neurons * layer_sizes[-1]

        density = (total_conn / total_possible) if total_possible > 0 else 1.0
        target_density = 0.12
        sparse_score = max(0.0, 1.0 - abs(density - target_density) / target_density)
        if density > 0.3:
            sparse_score *= 0.1
        if density > 0.6:
            sparse_score = 0.0

        layer_count = len(layer_sizes)
        layer_score = min(layer_count, 6) / 6.0 if layer_count > 0 else 0.0
        longterm_score = longterm_pct / 100.0

        def in_range(key, lo, hi):
            val = params.get(key, None)
            if val is None:
                return False
            try:
                v = float(val)
            except (TypeError, ValueError):
                return False
            return lo <= v <= hi

        bio_hits = 0.0
        bio_total = 0.0
        def add_bio(ok):
            nonlocal bio_hits, bio_total
            bio_total += 1.0
            if ok:
                bio_hits += 1.0

        add_bio(bool(params.get('aarnn_bio.stp_enabled', False)))
        add_bio(in_range('aarnn_bio.stp_u', 0.1, 0.5))
        add_bio(in_range('aarnn_bio.stp_tau_rec_ms', 100.0, 1500.0))
        add_bio(in_range('aarnn_bio.stp_tau_facil_ms', 50.0, 800.0))
        add_bio(in_range('aarnn_bio.ampa_tau_ms', 2.0, 10.0))
        add_bio(in_range('aarnn_bio.nmda_tau_ms', 40.0, 200.0))
        add_bio(in_range('aarnn_bio.gaba_tau_ms', 5.0, 30.0))
        add_bio(in_range('aarnn_bio.nmda_ratio', 0.1, 0.5))
        add_bio(in_range('aarnn_bio.synaptic_gain', 0.5, 2.0))
        add_bio(bool(params.get('aarnn_bio.adaptive_threshold_enabled', False)))
        add_bio(in_range('aarnn_bio.adaptive_threshold_tau_ms', 50.0, 800.0))
        add_bio(in_range('aarnn_bio.adaptive_threshold_increment', 0.2, 1.0))
        add_bio(in_range('aarnn_bio.homeostasis_target_rate_hz', 1.0, 5.0))
        add_bio(in_range('aarnn_bio.homeostasis_tau_ms', 500.0, 3000.0))
        add_bio(bool(params.get('aarnn_bio.neuromodulation_enabled', False)))
        bio_score = (bio_hits / bio_total) if bio_total > 0 else 0.0

        score = (0.4 * longterm_score) + (0.15 * layer_score) + (0.3 * sparse_score) + (0.15 * bio_score)
        if layer_count < 2:
            score *= 0.2

        if score > best_score:
            best_score = score
            best_out = out[-1000:]

        if score > 0.6 and layer_count >= 2 and sim_time < SIM_TIME_MAX:
            sim_time += SIM_TIME_STEP
            continue
        if score == 0 and sim_time == SIM_TIME_MIN:
            break
        break
    print(f"[LOG] Worker {idx} returning score {best_score}")
    return best_score, params, best_out

def evaluate_population(population):
    print(f"[LOG] Starting evaluate_population with {len(population)} individuals, {N_WORKERS} workers.")
    try:
        with mp.Pool(N_WORKERS, initializer=worker_init) as pool:
            print("[LOG] Pool started. Submitting jobs...")
            results = pool.starmap(run_simulation, [(p, i) for i, p in enumerate(population)])
            print("[LOG] Pool jobs completed.")
        return results
    except Exception as e:
        print(f"[ERROR] Exception in evaluate_population: {e}")
        raise

# Extra: Worker init for logging
def worker_init():
    import os
    print(f"[LOG] Worker process started. PID={os.getpid()}")

# --- MAIN LOOP ---
def main():
    print("[LOG] Starting genetic parameter search main loop")
    population = [random_param() for _ in range(POP_SIZE)]
    best_score = 0
    best_params = None
    for gen in range(N_GENERATIONS):
        print(f'\n=== Generation {gen+1}/{N_GENERATIONS} ===')
        results = evaluate_population(population)
        results.sort(reverse=True, key=lambda x: x[0])
        for rank, (score, params, tail) in enumerate(results):
            print(f'  [{rank+1}] Score: {score} Params: {params}')
            if score > best_score:
                best_score = score
                best_params = deepcopy(params)
        # Elitism
        next_pop = [deepcopy(results[i][1]) for i in range(N_ELITE)]
        # Crossover + mutation
        while len(next_pop) < POP_SIZE:
            p1, p2 = random.sample(results[:POP_SIZE//2], 2)
            child = crossover(p1[1], p2[1])
            child = mutate_param(child)
            next_pop.append(child)
        population = next_pop
    print('\n=== Search Complete ===')
    print(f'Best score: {best_score}')
    print(f'Best params: {best_params}')


if __name__ == '__main__':
    main()
