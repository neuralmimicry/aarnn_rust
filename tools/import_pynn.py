#!/usr/bin/env python3
import argparse
import sys

def main():
    parser = argparse.ArgumentParser(description='Import PyNN (Placeholder)')
    parser.add_argument('--in', dest='input_file', required=True)
    parser.add_argument('--out-network', required=True)
    args = parser.parse_args()

    print("Error: PyNN import is not automated because PyNN scripts are Python code, not a static data format.")
    print("Please use the NeuroML import for model exchange, as most PyNN simulators support NeuroML export.")
    sys.exit(1)

if __name__ == '__main__':
    main()
