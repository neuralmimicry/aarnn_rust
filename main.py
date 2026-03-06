
import numpy as np
import sys
import argparse
import matplotlib as mpl
import subprocess
import shlex

# Try to ensure an interactive backend before importing pyplot
def _ensure_interactive_backend():
    try:
        b = mpl.get_backend()
    except Exception:
        b = 'unknown'
    b_lower = str(b).lower()
    interactive = any(k in b_lower for k in ['qt', 'gtk', 'tk', 'wx', 'macosx'])
    if interactive:
        # Probe the backend; if it is MacOSX, avoid due to potential crashes on some macOS/PyObjC combos
        try:
            if 'macosx' in b_lower:
                raise RuntimeError('Avoiding MacOSX backend due to stability issues on this system')
            return
        except Exception:
            pass
    # Attempt to switch to a suitable interactive backend (macOS priority)
    tried = []
    # Use a safe, out-of-process probe to avoid fatal aborts from incompatible GUI backends
    def _probe_in_subprocess(backend_name: str) -> bool:
        cmd = (
            "python - <<'PY'\n"
            "import sys\n"
            "try:\n"
            "    import matplotlib as _mpl\n"
            "    _mpl.use('" + backend_name + "', force=True)\n"
            "    import matplotlib.pyplot as _plt\n"
            "    fig = _plt.figure(); _plt.close(fig)\n"
            "    sys.exit(0)\n"
            "except Exception:\n"
            "    sys.exit(1)\n"
            "PY\n"
        )
        try:
            proc = subprocess.run(cmd, shell=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
            return proc.returncode == 0
        except Exception:
            return False

    if sys.platform == 'darwin':
        # Prefer QtAgg/Qt5Agg on macOS and avoid MacOSX backend which may SIGABRT on some setups
        for cand in ('QtAgg', 'Qt5Agg', 'TkAgg', 'WXAgg'):
            if _probe_in_subprocess(cand):
                try:
                    mpl.use(cand, force=True)
                    return
                except Exception:
                    tried.append(cand)
            else:
                tried.append(cand)
    else:
        for cand in ('QtAgg', 'Qt5Agg', 'TkAgg', 'WXAgg'):
            if _probe_in_subprocess(cand):
                try:
                    mpl.use(cand, force=True)
                    return
                except Exception:
                    tried.append(cand)
            else:
                tried.append(cand)
    # Fall back to non-interactive; live window will be skipped
    try:
        mpl.use('Agg', force=True)
    except Exception:
        pass

# CLI: allow opting-in to UI to avoid backend crashes on systems without Tk/Qt
def _parse_args_once():
    parser = argparse.ArgumentParser(add_help=False)
    parser.add_argument('--ui', action='store_true', help='Enable live interactive UI window')
    parser.add_argument('--no-ui', action='store_true', help='Force-disable live UI window (default)')
    parser.add_argument('--backend', type=str, default=None,
                        help='Force a specific Matplotlib backend (e.g., TkAgg, Qt5Agg, WXAgg, Agg). '
                             'Will be safely probed; falls back to Agg on failure.')
    parser.add_argument('--check-access', action='store_true',
                        help='Probe access to display (GUI backend) and microphone; print a report and continue.')
    # Algorithm selection
    parser.add_argument('--neuron-model', type=str, default='lif', choices=['lif', 'izh'],
                        help='Neuron model to use for hidden/output layers: lif (Leaky IF) or izh (Izhikevich).')
    parser.add_argument('--izh-type', type=str, default='RS',
                        choices=['RS', 'IB', 'CH', 'FS', 'LTS', 'RZ', 'TC', 'P'],
                        help='Izhikevich neuron preset (when --neuron-model=izh).')
    parser.add_argument('--learning', type=str, default='stdp', choices=['stdp', 'hebb', 'oja'],
                        help='Synaptic learning rule to apply: stdp, hebb, or oja.')
    try:
        args, _ = parser.parse_known_args()
    except SystemExit:
        # In environments that pre-parse args, ignore
        class _A: pass
        args = _A(); args.ui = False; args.no_ui = True
        args.backend = None
        args.check_access = False
        args.neuron_model = 'lif'
        args.izh_type = 'RS'
        args.learning = 'stdp'
    return args

_args = _parse_args_once()

# Default to non-interactive backend unless user explicitly requests UI.
# If user forces a backend via --backend, safely probe it first.
if _args.backend:
    # Safe probe in subprocess (reuse logic from _ensure_interactive_backend)
    def _probe_backend_choice(name: str) -> bool:
        cmd = (
            "python - <<'PY'\n"
            "import sys\n"
            "try:\n"
            "    import matplotlib as _mpl\n"
            "    _mpl.use('" + _args.backend + "', force=True)\n"
            "    import matplotlib.pyplot as _plt\n"
            "    fig = _plt.figure(); _plt.close(fig)\n"
            "    sys.exit(0)\n"
            "except Exception:\n"
            "    sys.exit(1)\n"
            "PY\n"
        )
        try:
            proc = subprocess.run(cmd, shell=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
            return proc.returncode == 0
        except Exception:
            return False
    if _probe_backend_choice(_args.backend):
        try:
            mpl.use(_args.backend, force=True)
        except Exception:
            try:
                mpl.use('Agg', force=True)
            except Exception:
                pass
    else:
        # If user asked for UI but backend failed, fall back to auto selection (or Agg)
        if _args.ui and not _args.no_ui:
            _ensure_interactive_backend()
        else:
            try:
                mpl.use('Agg', force=True)
            except Exception:
                pass
else:
    # No explicit backend choice
    if _args.ui and not _args.no_ui:
        _ensure_interactive_backend()
    else:
        try:
            mpl.use('Agg', force=True)
        except Exception:
            pass

import matplotlib.pyplot as plt
from matplotlib import animation
from matplotlib.widgets import Button, CheckButtons, RadioButtons, Slider
from dataclasses import dataclass

np.random.seed(42)

# Report backend so the user knows whether an interactive window is possible
try:
    _backend = mpl.get_backend()
    print(f"Matplotlib backend: {_backend}")
except Exception:
    pass

# ---------------------------------
# Access checks (display and audio)
# ---------------------------------
def _is_gui_backend() -> bool:
    try:
        b = str(mpl.get_backend()).lower()
    except Exception:
        return False
    # Consider backends that provide GUI event loops
    return any(k in b for k in ('qt', 'gtk', 'tk', 'wx', 'macosx'))

def check_display_access(ui_requested: bool):
    """Return (status, detail) where status in {OK, NOT_REQUESTED, NO_GUI_BACKEND, FAILED}.
    Does a lightweight figure create/draw/close to validate the GUI backend works.
    """
    try:
        backend = mpl.get_backend()
    except Exception:
        backend = 'unknown'
    if not ui_requested:
        return 'NOT_REQUESTED', 'UI not requested'
    if not _is_gui_backend():
        return 'NO_GUI_BACKEND', f"Backend '{backend}' is not GUI-capable"
    try:
        import matplotlib.pyplot as _plt  # ensure pyplot bound to current backend
        fig = _plt.figure()
        try:
            fig.canvas.draw()
        except Exception:
            pass
        _plt.close(fig)
        return 'OK', f"GUI backend '{backend}' is available"
    except Exception as e:
        return 'FAILED', f"{type(e).__name__}: {e}"

def _classify_mic_error(err: Exception) -> str:
    msg = (str(err) or '').lower()
    # Heuristics for permission issues on macOS/PortAudio
    if any(k in msg for k in ['permission', 'not allowed', 'denied', 'unauthorized', 'authoriz']):
        return 'PERMISSION_DENIED'
    if any(k in msg for k in ['no default input device', 'invalid input device', 'device', 'busy', 'unavailable']):
        return 'DEVICE_UNAVAILABLE'
    return 'ERROR'

def check_microphone_access():
    """Return (status, detail) where status in {OK, MISSING_DEPS, PERMISSION_DENIED, DEVICE_UNAVAILABLE, ERROR}."""
    try:
        import sounddevice as sd  # type: ignore
    except Exception as e:
        return 'MISSING_DEPS', f"sounddevice not available: {e}"
    try:
        # Use a tiny buffer to quickly open/close
        stream = sd.InputStream(samplerate=16000, channels=1, dtype='float32', blocksize=256)
        stream.start(); stream.stop(); stream.close()
        return 'OK', 'Microphone stream opened successfully'
    except Exception as e:
        return _classify_mic_error(e), str(e)

def print_access_report(ui_requested: bool):
    disp_status, disp_detail = check_display_access(ui_requested)
    mic_status, mic_detail = check_microphone_access()
    print('Access check:')
    print(f" - Display: {disp_status} ({disp_detail})")
    if mic_status == 'PERMISSION_DENIED' and sys.platform == 'darwin':
        print(f" - Microphone: {mic_status} ({mic_detail}). Go to System Settings → Privacy & Security → Microphone and allow access for your Python/Terminal app.")
    else:
        print(f" - Microphone: {mic_status} ({mic_detail})")

# -----------------------------
# Model configuration
# -----------------------------
@dataclass
class LIFParams:
    tau_m: float = 20.0      # membrane time constant (ms)
    v_reset: float = 0.0     # reset potential
    v_th: float = 1.0        # threshold
    refractory: int = 5      # refractory period (time steps)
    dt: float = 1.0          # time step (ms)

@dataclass
class IzhikevichParams:
    # Standard Izhikevich parameters
    a: float = 0.02
    b: float = 0.2
    c: float = -65.0
    d: float = 8.0
    v_th: float = 30.0  # spike threshold (mV)
    dt: float = 1.0

def izh_preset(name: str) -> IzhikevichParams:
    # Presets per Izhikevich (2003)
    presets = {
        'RS':  (0.02, 0.2,  -65.0, 8.0),    # Regular spiking
        'IB':  (0.02, 0.2,  -55.0, 4.0),    # Intrinsically bursting
        'CH':  (0.02, 0.2,  -50.0, 2.0),    # Chattering
        'FS':  (0.1,  0.2,  -65.0, 2.0),    # Fast spiking
        'LTS': (0.02, 0.25, -65.0, 2.0),    # Low-threshold spiking
        'RZ':  (0.1,  0.26, -65.0, 2.0),    # Resonator
        'TC':  (0.02, 0.25, -65.0, 0.05),   # Thalamo-cortical (approx)
        'P':   (0.02, 1.0,  -60.0, 0.0),    # Phasic spiking
    }
    a, b, c, d = presets.get(name.upper(), presets['RS'])
    return IzhikevichParams(a=a, b=b, c=c, d=d, dt=lif.dt)

@dataclass
class STDPParams:
    tau_pre: float = 20.0    # pre trace decay
    tau_post: float = 20.0   # post trace decay
    eta: float = 0.002       # learning rate
    w_min: float = 0.0
    w_max: float = 1.0

@dataclass
class NetworkConfig:
    n_sensory: int = 50
    # Multi-layer hidden configuration
    n_hidden_layers: int = 6
    n_hidden_per_layer: int = 30
    n_output: int = 10
    p_in: float = 0.15        # sensory->first hidden connection prob
    p_hidden: float = 0.10    # between hidden layers connection prob
    p_out: float = 0.15       # last hidden->output connection prob

lif = LIFParams()
stdp = STDPParams()
net = NetworkConfig()

# Selected algorithms from CLI
NEURON_MODEL = (_args.neuron_model if hasattr(_args, 'neuron_model') else 'lif').lower()
LEARNING_RULE = (_args.learning if hasattr(_args, 'learning') else 'stdp').lower()
IZH_PARAMS = izh_preset(getattr(_args, 'izh_type', 'RS') if hasattr(_args, 'izh_type') else 'RS')

# -----------------------------
# Helper functions
# -----------------------------
def build_network(cfg: NetworkConfig):
    """Create sparse connectivity matrices for 6 hidden layers with bidirectional hidden links.
    Returns (W_in, W_hh_fwd_list, W_hh_bwd_list, W_out) and index ranges for clusters.
    - W_in: (H1 x S)
    - W_hh_fwd_list: list length L-1, each (H{l+1} x H{l}) forward (left→right)
    - W_hh_bwd_list: list length L-1, each (H{l} x H{l+1}) backward (right→left)
    - W_out: (O x H_L)
    Also returns idx_s, idx_h_layers (list of arrays), idx_o
    """
    L = cfg.n_hidden_layers
    H = cfg.n_hidden_per_layer

    # Index ranges for plotting/record keeping (flat indices are not essential here)
    idx_s = np.arange(cfg.n_sensory)
    idx_h_layers = [np.arange(H) for _ in range(L)]
    idx_o = np.arange(cfg.n_output)

    # Sensory -> first hidden
    W_in = np.zeros((H, cfg.n_sensory))
    mask_in = (np.random.rand(H, cfg.n_sensory) < cfg.p_in)
    if np.any(mask_in):
        W_in[mask_in] = np.random.rand(np.count_nonzero(mask_in)) * 0.3 + 0.1

    # Hidden -> hidden (between layers) forward and backward
    W_hh_fwd_list = []  # l -> l+1  shape (H, H)
    W_hh_bwd_list = []  # l+1 -> l  shape (H, H)
    for l in range(L - 1):
        # Forward (l -> l+1)
        Wf = np.zeros((H, H))
        maskf = (np.random.rand(H, H) < cfg.p_hidden)
        if np.any(maskf):
            Wf[maskf] = np.random.rand(np.count_nonzero(maskf)) * 0.3 + 0.1
        W_hh_fwd_list.append(Wf)

        # Backward (l+1 -> l)
        Wb = np.zeros((H, H))
        maskb = (np.random.rand(H, H) < cfg.p_hidden)
        if np.any(maskb):
            Wb[maskb] = np.random.rand(np.count_nonzero(maskb)) * 0.3 + 0.1
        W_hh_bwd_list.append(Wb)

    # Last hidden -> output
    W_out = np.zeros((cfg.n_output, H))
    mask_out = (np.random.rand(cfg.n_output, H) < cfg.p_out)
    if np.any(mask_out):
        W_out[mask_out] = np.random.rand(np.count_nonzero(mask_out)) * 0.3 + 0.1

    return W_in, W_hh_fwd_list, W_hh_bwd_list, W_out, idx_s, idx_h_layers, idx_o

def poisson_input_patterns(T, n_sensory, dt=1.0):
    """Create three alternating Poisson stimulus patterns over time.
    Each pattern drives a distinct subset of sensory neurons with elevated rate.
    """
    base_rate = 2.0  # Hz
    burst_rate = 25.0  # Hz when active
    steps = int(T / dt)

    # Split sensory neurons into three disjoint groups
    thirds = np.array_split(np.arange(n_sensory), 3)

    spikes = np.zeros((steps, n_sensory), dtype=np.int8)
    pattern_id = np.zeros(steps, dtype=np.int8)
    chunk = steps // 6  # alternate patterns in chunks
    schedule = [0, 1, 2, 0, 2, 1]

    for k, pat in enumerate(schedule):
        start = k * chunk
        end = (k + 1) * chunk if k < len(schedule) - 1 else steps
        pattern_id[start:end] = pat
        active_group = thirds[pat]

        # Generate Poisson spikes
        p_base = base_rate * dt / 1000.0
        p_burst = burst_rate * dt / 1000.0
        # baseline for all
        spikes[start:end, :] = (np.random.rand(end - start, n_sensory) < p_base).astype(np.int8)
        # elevated for active group
        spikes[start:end, active_group] = (np.random.rand(end - start, active_group.size) < p_burst).astype(np.int8)

    return spikes, pattern_id, thirds

def run_snn(T, W_in, W_hh_fwd_list, W_hh_bwd_list, W_out, lif: LIFParams, stdp: STDPParams,
            idx_s, idx_h_layers, idx_o, sensory_spikes,
            neuron_model: str = NEURON_MODEL, izh: IzhikevichParams = IZH_PARAMS,
            learning: str = LEARNING_RULE):
    """Run discrete-time LIF SNN across 6 hidden layers with bidirectional hidden connectivity and STDP on all stages."""
    dt = lif.dt
    steps = int(T / dt)
    L = len(idx_h_layers)
    H = idx_h_layers[0].size if L > 0 else 0
    n_output = idx_o.size
    n_sensory = idx_s.size

    # States
    V_h = [np.zeros(H) for _ in range(L)]
    U_h = [np.zeros(H) for _ in range(L)] if neuron_model == 'izh' else None
    V_o = np.zeros(n_output)
    U_o = np.zeros(n_output) if neuron_model == 'izh' else None
    refr_h = [np.zeros(H, dtype=int) for _ in range(L)] if neuron_model == 'lif' else None
    refr_o = np.zeros(n_output, dtype=int) if neuron_model == 'lif' else None

    # Traces for STDP
    x_pre_in = np.zeros(n_sensory)
    x_post_h = [np.zeros(H) for _ in range(L)]
    x_pre_h = [np.zeros(H) for _ in range(L)]
    x_post_o = np.zeros(n_output)

    # Recording
    spikes_h = [np.zeros((steps, H), dtype=np.int8) for _ in range(L)]
    spikes_o = np.zeros((steps, n_output), dtype=np.int8)

    # Decay factors
    decay_m = np.exp(-dt / lif.tau_m)
    decay_pre = np.exp(-dt / stdp.tau_pre)
    decay_post = np.exp(-dt / stdp.tau_post)

    def _nan_to_num_inplace(a):
        # Replace NaN and inf with zeros to avoid matmul warnings
        np.nan_to_num(a, copy=False, nan=0.0, posinf=0.0, neginf=0.0)

    # ensure float64 and finite for safety
    W_in = np.nan_to_num(W_in.astype(np.float64, copy=False), nan=0.0, posinf=0.0, neginf=0.0)
    W_hh_fwd_list = [np.nan_to_num(W.astype(np.float64, copy=False), nan=0.0, posinf=0.0, neginf=0.0)
                 for W in W_hh_fwd_list]
    W_hh_bwd_list = [np.nan_to_num(W.astype(np.float64, copy=False), nan=0.0, posinf=0.0, neginf=0.0)
                 for W in W_hh_bwd_list]
    W_out = np.nan_to_num(W_out.astype(np.float64, copy=False), nan=0.0, posinf=0.0, neginf=0.0)

    for t in range(steps):
        s_t = sensory_spikes[t]  # shape (n_sensory,)

        # Update pre traces
        x_pre_in *= decay_pre
        x_pre_in += s_t  # add 1 for spikes
        for l in range(L):
            x_post_h[l] *= decay_post
            x_pre_h[l] *= decay_pre
        x_post_o *= decay_post

        # Hidden layer 1 input current from sensory spikes (safe numerics)
        _nan_to_num_inplace(W_in)
        s_t_f = np.nan_to_num(s_t.astype(np.float64, copy=False), nan=0.0, posinf=0.0, neginf=0.0)
        with np.errstate(invalid='ignore', divide='ignore', over='ignore'):
            I_h = W_in @ s_t_f
        if neuron_model == 'lif':
            V_h[0] = V_h[0] * decay_m + I_h
            np.nan_to_num(V_h[0], copy=False, nan=0.0, posinf=0.0, neginf=0.0)
            V_h[0] = np.clip(V_h[0], -5.0, 5.0)
            active_h = refr_h[0] <= 0
            spk_h0 = (V_h[0] >= lif.v_th) & active_h
            V_h[0][spk_h0] = lif.v_reset
            refr_h[0][spk_h0] = lif.refractory
            refr_h[0][~spk_h0] = np.maximum(refr_h[0][~spk_h0] - 1, 0)
        else:
            # Izhikevich dynamics (Euler)
            v = V_h[0]
            u = U_h[0]
            v = v + izh.dt * (0.04 * v * v + 5.0 * v + 140.0 - u + I_h)
            u = u + izh.dt * (izh.a * (izh.b * v - u))
            spk_h0 = v >= izh.v_th
            # reset
            u = u + izh.d * spk_h0.astype(float)
            v = np.where(spk_h0, izh.c, v)
            V_h[0] = np.nan_to_num(v, copy=False, nan=0.0, posinf=0.0, neginf=0.0)
            U_h[0] = np.nan_to_num(u, copy=False, nan=0.0, posinf=0.0, neginf=0.0)
        spikes_h[0][t] = spk_h0.astype(np.int8)
        x_post_h[0] += spk_h0.astype(float)
        x_pre_h[0] += spk_h0.astype(float)

        # Prepare previous spikes for backward currents
        prev_spikes_h = [spikes_h[l][t-1].astype(np.float64, copy=False) if t > 0 else np.zeros(H) for l in range(L)]

        # Propagate through hidden layers 2..L with forward and backward inputs
        for l in range(1, L):
            _nan_to_num_inplace(W_hh_fwd_list[l-1])
            _nan_to_num_inplace(W_hh_bwd_list[l-1] if l-1 < len(W_hh_bwd_list) else np.zeros((H, H)))
            prev_spk = spikes_h[l-1][t].astype(np.float64, copy=False)
            # forward current from layer l-1 at current step
            with np.errstate(invalid='ignore', divide='ignore', over='ignore'):
                I_f = W_hh_fwd_list[l-1] @ prev_spk
            # backward current from layer l+1 at previous step (if exists)
            if l < L - 1:
                with np.errstate(invalid='ignore', divide='ignore', over='ignore'):
                    I_b = W_hh_bwd_list[l] @ prev_spikes_h[l+1]
            else:
                I_b = 0.0
            I = I_f + I_b
            if neuron_model == 'lif':
                V_h[l] = V_h[l] * decay_m + I
                np.nan_to_num(V_h[l], copy=False, nan=0.0, posinf=0.0, neginf=0.0)
                V_h[l] = np.clip(V_h[l], -5.0, 5.0)
                active = refr_h[l] <= 0
                spk = (V_h[l] >= lif.v_th) & active
                V_h[l][spk] = lif.v_reset
                refr_h[l][spk] = lif.refractory
                refr_h[l][~spk] = np.maximum(refr_h[l][~spk] - 1, 0)
            else:
                v = V_h[l]
                u = U_h[l]
                v = v + izh.dt * (0.04 * v * v + 5.0 * v + 140.0 - u + I)
                u = u + izh.dt * (izh.a * (izh.b * v - u))
                spk = v >= izh.v_th
                u = u + izh.d * spk.astype(float)
                v = np.where(spk, izh.c, v)
                V_h[l] = np.nan_to_num(v, copy=False, nan=0.0, posinf=0.0, neginf=0.0)
                U_h[l] = np.nan_to_num(u, copy=False, nan=0.0, posinf=0.0, neginf=0.0)
            spikes_h[l][t] = spk.astype(np.int8)
            x_post_h[l] += spk.astype(float)
            x_pre_h[l] += spk.astype(float)

        # Output layer (safe numerics)
        _nan_to_num_inplace(W_out)
        with np.errstate(invalid='ignore', divide='ignore', over='ignore'):
            I_o = W_out @ spikes_h[-1][t].astype(np.float64, copy=False)
        if neuron_model == 'lif':
            V_o = V_o * decay_m + I_o
            np.nan_to_num(V_o, copy=False, nan=0.0, posinf=0.0, neginf=0.0)
            V_o = np.clip(V_o, -5.0, 5.0)
            active_o = refr_o <= 0
            spk_o = (V_o >= lif.v_th) & active_o
            V_o[spk_o] = lif.v_reset
            refr_o[spk_o] = lif.refractory
            refr_o[~spk_o] = np.maximum(refr_o[~spk_o] - 1, 0)
        else:
            v = V_o
            u = U_o
            v = v + izh.dt * (0.04 * v * v + 5.0 * v + 140.0 - u + I_o)
            u = u + izh.dt * (izh.a * (izh.b * v - u))
            spk_o = v >= izh.v_th
            u = u + izh.d * spk_o.astype(float)
            v = np.where(spk_o, izh.c, v)
            V_o = np.nan_to_num(v, copy=False, nan=0.0, posinf=0.0, neginf=0.0)
            U_o = np.nan_to_num(u, copy=False, nan=0.0, posinf=0.0, neginf=0.0)
        spikes_o[t] = spk_o.astype(np.int8)

        # Update traces with output spikes
        x_post_o += spk_o.astype(float)

        # Learning rule updates
        pre0 = s_t_f
        post0 = spikes_h[0][t].astype(np.float64, copy=False)
        if learning == 'stdp':
            dW_in = stdp.eta * (np.outer(x_post_h[0], pre0) - np.outer(post0, x_pre_in))
        elif learning == 'hebb':
            dW_in = stdp.eta * np.outer(post0, pre0)
        else:  # oja
            dW_in = stdp.eta * (np.outer(post0, pre0) - (post0[:, None]**2) * W_in)
        W_in += dW_in
        W_in = np.clip(W_in, stdp.w_min, stdp.w_max)

        for l in range(L - 1):
            pre_f = spikes_h[l][t].astype(np.float64, copy=False)
            post_f = spikes_h[l+1][t].astype(np.float64, copy=False)
            if learning == 'stdp':
                dWf = stdp.eta * (np.outer(x_post_h[l+1], pre_f) - np.outer(post_f, x_pre_h[l]))
                dWb = stdp.eta * (np.outer(x_post_h[l], post_f) - np.outer(pre_f, x_pre_h[l+1]))
            elif learning == 'hebb':
                dWf = stdp.eta * np.outer(post_f, pre_f)
                dWb = stdp.eta * np.outer(pre_f, post_f)  # note orientation
            else:  # oja
                dWf = stdp.eta * (np.outer(post_f, pre_f) - (post_f[:, None]**2) * W_hh_fwd_list[l])
                dWb = stdp.eta * (np.outer(pre_f, post_f) - (pre_f[:, None]**2) * W_hh_bwd_list[l])
            W_hh_fwd_list[l] += dWf
            W_hh_fwd_list[l] = np.clip(W_hh_fwd_list[l], stdp.w_min, stdp.w_max)
            W_hh_bwd_list[l] += dWb
            W_hh_bwd_list[l] = np.clip(W_hh_bwd_list[l], stdp.w_min, stdp.w_max)

        pre_last = spikes_h[-1][t].astype(np.float64, copy=False)
        post_o = spk_o.astype(np.float64, copy=False)
        if learning == 'stdp':
            dW_out = stdp.eta * (np.outer(x_post_o, pre_last) - np.outer(post_o, x_pre_h[-1]))
        elif learning == 'hebb':
            dW_out = stdp.eta * np.outer(post_o, pre_last)
        else:
            dW_out = stdp.eta * (np.outer(post_o, pre_last) - (post_o[:, None]**2) * W_out)
        W_out += dW_out
        W_out = np.clip(W_out, stdp.w_min, stdp.w_max)

    return spikes_h, spikes_o, W_in, W_hh_fwd_list, W_hh_bwd_list, W_out

# -----------------------------
# Interactive animation: live AARNN with controls
# -----------------------------
class SNNRunner:
    """Incremental SNN simulator (per-step) with optional output->input feedback.
    Supports multiple hidden layers.
    """
    def __init__(self, W_in, W_hh_fwd_list, W_hh_bwd_list, W_out, lif: LIFParams, stdp: STDPParams,
                 idx_s, idx_h_layers, idx_o, sensory_spikes=None, feedback_map=None, provider=None,
                 p_in: float = 0.15,
                 neuron_model: str = NEURON_MODEL, izh: IzhikevichParams = IZH_PARAMS,
                 learning: str = LEARNING_RULE):
        self.lif = lif
        self.stdp = stdp
        self.idx_s, self.idx_h_layers, self.idx_o = idx_s, idx_h_layers, idx_o
        self.n_s = idx_s.size
        self.L = len(idx_h_layers)
        self.H = idx_h_layers[0].size if self.L > 0 else 0
        self.n_o = idx_o.size
        self.W_in = W_in.copy()
        self.W_hh_fwd_list = [W.copy() for W in W_hh_fwd_list]
        self.W_hh_bwd_list = [W.copy() for W in W_hh_bwd_list]
        self.W_out = W_out.copy()
        # Connection probability hint for adding new sensory columns
        self.p_in = float(p_in)
        # Algorithm selection
        self.neuron_model = (neuron_model or 'lif').lower()
        self.izh = izh
        self.learning = (learning or 'stdp').lower()
        # Sensory sources
        self.provider = provider  # if not None, will be used to produce s_t
        if sensory_spikes is None:
            self.sensory_spikes_base = None
            self.steps = int(1e12)  # effectively unbounded for streaming providers
        else:
            self.sensory_spikes_base = sensory_spikes.copy()
            self.steps = sensory_spikes.shape[0]
        self.t = 0
        # States
        self.V_h = [np.zeros(self.H) for _ in range(self.L)]
        self.U_h = [np.zeros(self.H) for _ in range(self.L)] if self.neuron_model == 'izh' else None
        self.V_o = np.zeros(self.n_o)
        self.U_o = np.zeros(self.n_o) if self.neuron_model == 'izh' else None
        self.refr_h = [np.zeros(self.H, dtype=int) for _ in range(self.L)] if self.neuron_model == 'lif' else None
        self.refr_o = np.zeros(self.n_o, dtype=int) if self.neuron_model == 'lif' else None
        # Traces
        self.x_pre_in = np.zeros(self.n_s)
        self.x_post_h = [np.zeros(self.H) for _ in range(self.L)]
        self.x_pre_h = [np.zeros(self.H) for _ in range(self.L)]
        self.x_post_o = np.zeros(self.n_o)
        # Decay constants
        self.decay_m = np.exp(-lif.dt / lif.tau_m)
        self.decay_pre = np.exp(-lif.dt / stdp.tau_pre)
        self.decay_post = np.exp(-lif.dt / stdp.tau_post)
        # Logging recent spikes
        self.last_spk_h_layers = [np.zeros(self.H, dtype=np.int8) for _ in range(self.L)]
        self.last_spk_o = np.zeros(self.n_o, dtype=np.int8)
        # Feedback mapping: array of sensory indices for each output neuron
        if feedback_map is None:
            rng = np.random.default_rng(0)
            feedback_map = rng.integers(0, self.n_s, size=self.n_o)
        self.feedback_map = feedback_map
        self.feedback_enabled = False

    def _resize_feedback_map(self, n_s_new: int):
        # Invalidate mappings that would point beyond the new sensory count
        fm = self.feedback_map
        invalid = (fm < 0) | (fm >= n_s_new)
        if np.any(invalid):
            fm[invalid] = -1
        self.feedback_map = fm

    def resize_sensory(self, n_s_new: int):
        """Resize the number of sensory input neurons at runtime.
        - Adjust W_in shape to (H, n_s_new) by truncating or appending columns.
        - Reset x_pre_in to correct length.
        - Update internal counters and clean feedback mappings that would become orphaned.
        """
        n_s_new = int(max(1, n_s_new))
        n_s_old = int(self.n_s)
        if n_s_new == n_s_old:
            return
        H = self.H
        # Grow or shrink W_in
        if n_s_new < n_s_old:
            self.W_in = self.W_in[:, :n_s_new]
        else:
            add = n_s_new - n_s_old
            # Create new columns with sparse connectivity using p_in and random small weights
            new_cols = np.zeros((H, add), dtype=np.float64)
            mask = (np.random.rand(H, add) < self.p_in)
            if np.any(mask):
                new_cols[mask] = np.random.rand(np.count_nonzero(mask)) * 0.3 + 0.1
            self.W_in = np.concatenate([self.W_in, new_cols], axis=1)
        # Update traces and state related to sensory input
        self.x_pre_in = np.zeros(n_s_new, dtype=np.float64)
        # Update counters
        self.n_s = n_s_new
        # Clean feedback mappings
        self._resize_feedback_map(n_s_new)

    def reset(self):
        self.t = 0
        # Ensure model-specific state arrays exist for the currently selected model
        if self.neuron_model == 'lif':
            # Allocate refractory arrays if missing
            if self.refr_h is None:
                self.refr_h = [np.zeros(self.H, dtype=int) for _ in range(self.L)]
            if self.refr_o is None:
                self.refr_o = np.zeros(self.n_o, dtype=int)
            # Drop Izhikevich-specific state
            self.U_h = None
            self.U_o = None
        else:  # izh
            # Allocate Izhikevich state if missing
            if self.U_h is None:
                self.U_h = [np.zeros(self.H) for _ in range(self.L)]
            if self.U_o is None:
                self.U_o = np.zeros(self.n_o)
            # Drop LIF-specific state
            self.refr_h = None
            self.refr_o = None

        # Now clear all states
        for l in range(self.L):
            self.V_h[l].fill(0.0)
            if self.neuron_model == 'lif':
                # safe if arrays exist
                if self.refr_h is not None:
                    self.refr_h[l].fill(0)
            else:
                if self.U_h is not None:
                    self.U_h[l].fill(0.0)
            self.x_post_h[l].fill(0.0)
            self.x_pre_h[l].fill(0.0)
            self.last_spk_h_layers[l].fill(0)
        self.V_o.fill(0.0)
        if self.neuron_model == 'lif':
            if self.refr_o is not None:
                self.refr_o.fill(0)
        else:
            if self.U_o is not None:
                self.U_o.fill(0.0)
        self.x_pre_in.fill(0.0)
        self.x_post_o.fill(0.0)
        self.last_spk_o.fill(0)

    def step(self):
        # If using a streaming provider, allow unbounded run
        if self.provider is None and self.t >= self.steps:
            return None  # end
        dt = self.lif.dt
        # Base sensory spikes for this timestep
        if self.provider is not None:
            try:
                s_t = self.provider.next_spikes()
                if s_t is None:
                    s_t = np.zeros(self.n_s, dtype=np.int8)
            except Exception:
                s_t = np.zeros(self.n_s, dtype=np.int8)
        else:
            s_t = self.sensory_spikes_base[self.t].copy()
        # Optional feedback: route previous output spikes to mapped sensory indices
        if self.feedback_enabled and self.t > 0:
            mapped_idx = self.feedback_map[self.last_spk_o.astype(bool)]
            if mapped_idx.size > 0:
                # filter invalid indices (-1 or out of range)
                mapped_idx = mapped_idx[(mapped_idx >= 0) & (mapped_idx < self.n_s)]
                if mapped_idx.size > 0:
                    s_t[mapped_idx] = 1  # OR-in feedback spikes

        # Update traces
        self.x_pre_in *= self.decay_pre
        self.x_pre_in += s_t
        for l in range(self.L):
            self.x_post_h[l] *= self.decay_post
            self.x_pre_h[l] *= self.decay_pre
        self.x_post_o *= self.decay_post

        # Hidden dynamics
        np.nan_to_num(self.W_in, copy=False, nan=0.0, posinf=0.0, neginf=0.0)
        with np.errstate(invalid='ignore', divide='ignore', over='ignore'):
            I_h0 = self.W_in @ s_t.astype(float)
        if self.neuron_model == 'lif':
            self.V_h[0] = self.V_h[0] * self.decay_m + I_h0
            self.V_h[0] = np.clip(self.V_h[0], -5.0, 5.0)
            active_h0 = self.refr_h[0] <= 0
            spk_h0 = (self.V_h[0] >= self.lif.v_th) & active_h0
            self.V_h[0][spk_h0] = self.lif.v_reset
            self.refr_h[0][spk_h0] = self.lif.refractory
            self.refr_h[0][~spk_h0] = np.maximum(self.refr_h[0][~spk_h0] - 1, 0)
        else:
            v = self.V_h[0]
            u = self.U_h[0]
            v = v + self.izh.dt * (0.04 * v * v + 5.0 * v + 140.0 - u + I_h0)
            u = u + self.izh.dt * (self.izh.a * (self.izh.b * v - u))
            spk_h0 = v >= self.izh.v_th
            u = u + self.izh.d * spk_h0.astype(float)
            v = np.where(spk_h0, self.izh.c, v)
            self.V_h[0] = np.nan_to_num(v, copy=False, nan=0.0, posinf=0.0, neginf=0.0)
            self.U_h[0] = np.nan_to_num(u, copy=False, nan=0.0, posinf=0.0, neginf=0.0)
        self.last_spk_h_layers[0] = spk_h0.astype(np.int8)

        # Update traces
        self.x_post_h[0] += spk_h0.astype(float)
        self.x_pre_h[0] += spk_h0.astype(float)

        # Propagate through remaining hidden layers
        for l in range(1, self.L):
            np.nan_to_num(self.W_hh_fwd_list[l-1], copy=False, nan=0.0, posinf=0.0, neginf=0.0)
            # forward from current step spikes of l-1
            with np.errstate(invalid='ignore', divide='ignore', over='ignore'):
                I_f = self.W_hh_fwd_list[l-1] @ self.last_spk_h_layers[l-1].astype(float)
            # backward from previous step spikes of l+1 (if exists)
            if l < self.L - 1:
                np.nan_to_num(self.W_hh_bwd_list[l], copy=False, nan=0.0, posinf=0.0, neginf=0.0)
                with np.errstate(invalid='ignore', divide='ignore', over='ignore'):
                    I_b = self.W_hh_bwd_list[l] @ self.last_spk_h_layers[l+1].astype(float)
            else:
                I_b = 0.0
            I = I_f + I_b
            if self.neuron_model == 'lif':
                self.V_h[l] = self.V_h[l] * self.decay_m + I
                self.V_h[l] = np.clip(self.V_h[l], -5.0, 5.0)
                active = self.refr_h[l] <= 0
                spk = (self.V_h[l] >= self.lif.v_th) & active
                self.V_h[l][spk] = self.lif.v_reset
                self.refr_h[l][spk] = self.lif.refractory
                self.refr_h[l][~spk] = np.maximum(self.refr_h[l][~spk] - 1, 0)
            else:
                v = self.V_h[l]
                u = self.U_h[l]
                v = v + self.izh.dt * (0.04 * v * v + 5.0 * v + 140.0 - u + I)
                u = u + self.izh.dt * (self.izh.a * (self.izh.b * v - u))
                spk = v >= self.izh.v_th
                u = u + self.izh.d * spk.astype(float)
                v = np.where(spk, self.izh.c, v)
                self.V_h[l] = np.nan_to_num(v, copy=False, nan=0.0, posinf=0.0, neginf=0.0)
                self.U_h[l] = np.nan_to_num(u, copy=False, nan=0.0, posinf=0.0, neginf=0.0)
            self.last_spk_h_layers[l] = spk.astype(np.int8)
            self.x_post_h[l] += spk.astype(float)
            self.x_pre_h[l] += spk.astype(float)

        # Output dynamics
        np.nan_to_num(self.W_out, copy=False, nan=0.0, posinf=0.0, neginf=0.0)
        with np.errstate(invalid='ignore', divide='ignore', over='ignore'):
            I_o = self.W_out @ self.last_spk_h_layers[-1].astype(float)
        if self.neuron_model == 'lif':
            self.V_o = self.V_o * self.decay_m + I_o
            self.V_o = np.clip(self.V_o, -5.0, 5.0)
            active_o = self.refr_o <= 0
            spk_o = (self.V_o >= self.lif.v_th) & active_o
            self.V_o[spk_o] = self.lif.v_reset
            self.refr_o[spk_o] = self.lif.refractory
            self.refr_o[~spk_o] = np.maximum(self.refr_o[~spk_o] - 1, 0)
        else:
            v = self.V_o
            u = self.U_o
            v = v + self.izh.dt * (0.04 * v * v + 5.0 * v + 140.0 - u + I_o)
            u = u + self.izh.dt * (self.izh.a * (self.izh.b * v - u))
            spk_o = v >= self.izh.v_th
            u = u + self.izh.d * spk_o.astype(float)
            v = np.where(spk_o, self.izh.c, v)
            self.V_o = np.nan_to_num(v, copy=False, nan=0.0, posinf=0.0, neginf=0.0)
            self.U_o = np.nan_to_num(u, copy=False, nan=0.0, posinf=0.0, neginf=0.0)
        self.last_spk_o = spk_o.astype(np.int8)

        # Update traces
        self.x_post_o += spk_o.astype(float)

        # Learning rules
        pre0 = s_t.astype(float)
        post0 = self.last_spk_h_layers[0].astype(float)
        if self.learning == 'stdp':
            dW_in = self.stdp.eta * (np.outer(self.x_post_h[0], pre0) - np.outer(post0, self.x_pre_in))
        elif self.learning == 'hebb':
            dW_in = self.stdp.eta * np.outer(post0, pre0)
        else:
            dW_in = self.stdp.eta * (np.outer(post0, pre0) - (post0[:, None]**2) * self.W_in)
        self.W_in += dW_in
        self.W_in = np.clip(self.W_in, self.stdp.w_min, self.stdp.w_max)
        for l in range(self.L - 1):
            pre_spk = self.last_spk_h_layers[l].astype(float)
            post_spk = self.last_spk_h_layers[l+1].astype(float)
            if self.learning == 'stdp':
                dWf = self.stdp.eta * (np.outer(self.x_post_h[l+1], pre_spk) - np.outer(post_spk, self.x_pre_h[l]))
                dWb = self.stdp.eta * (np.outer(self.x_post_h[l], post_spk) - np.outer(pre_spk, self.x_pre_h[l+1]))
            elif self.learning == 'hebb':
                dWf = self.stdp.eta * np.outer(post_spk, pre_spk)
                dWb = self.stdp.eta * np.outer(pre_spk, post_spk)
            else:
                dWf = self.stdp.eta * (np.outer(post_spk, pre_spk) - (post_spk[:, None]**2) * self.W_hh_fwd_list[l])
                dWb = self.stdp.eta * (np.outer(pre_spk, post_spk) - (pre_spk[:, None]**2) * self.W_hh_bwd_list[l])
            self.W_hh_fwd_list[l] += dWf
            self.W_hh_fwd_list[l] = np.clip(self.W_hh_fwd_list[l], self.stdp.w_min, self.stdp.w_max)
            self.W_hh_bwd_list[l] += dWb
            self.W_hh_bwd_list[l] = np.clip(self.W_hh_bwd_list[l], self.stdp.w_min, self.stdp.w_max)
        if self.learning == 'stdp':
            dW_out = self.stdp.eta * (np.outer(self.x_post_o, self.last_spk_h_layers[-1].astype(float)) - np.outer(spk_o.astype(float), self.x_pre_h[-1]))
        elif self.learning == 'hebb':
            dW_out = self.stdp.eta * np.outer(spk_o.astype(float), self.last_spk_h_layers[-1].astype(float))
        else:
            po = spk_o.astype(float)
            dW_out = self.stdp.eta * (np.outer(po, self.last_spk_h_layers[-1].astype(float)) - (po[:, None]**2) * self.W_out)
        self.W_out += dW_out
        self.W_out = np.clip(self.W_out, self.stdp.w_min, self.stdp.w_max)

        self.t += 1

        return {
            't': self.t,
            's_t': s_t,
            'spk_h_layers': [arr.copy() for arr in self.last_spk_h_layers],
            'spk_o': self.last_spk_o.copy(),
            'V_h_layers': [v.copy() for v in self.V_h],
            'V_o': self.V_o.copy(),
        }


class SNNAnimator:
    def __init__(self, runner: SNNRunner, xs, ys, xh_layers, yh_layers, xo, yo, stdp: STDPParams):
        self.runner = runner
        self.playing = False
        self.activity_s = np.zeros(runner.n_s)
        self.activity_h_layers = [np.zeros(runner.H) for _ in range(runner.L)]
        self.activity_o = np.zeros(runner.n_o)
        self.decay_vis = 0.9
        # Edge blink states
        self.edge_decay = 0.85
        self.active_color_in = np.array([0.850, 0.325, 0.098])  # orange/red-ish
        self.active_color_out = np.array([0.122, 0.467, 0.706])  # blue-ish
        self.active_color_bwd = np.array([0.494, 0.184, 0.556])  # purple for backward H←H
        self.base_gray = np.array([0.827, 0.827, 0.827])  # lightgray in RGB

        # Figure & axes
        self.fig, self.ax = plt.subplots(figsize=(10, 6))
        self.fig.canvas.manager.set_window_title('Neuromorphic Network — Live Animation')
        self.ax.set_title('Live Spiking Activity (color = recent spikes)')
        self.ax.axis('off')

        # Draw edges (static base), keep structures for animation
        self.edges = []  # list of dicts with keys: type, pre_layer, post_layer, pre_idx, post_idx, line, base_* , w
        # Sensory -> H1
        xh0, yh0 = xh_layers[0], yh_layers[0]
        for j in range(self.runner.H):
            for i in range(self.runner.n_s):
                w = self.runner.W_in[j, i]
                if w > 0:
                    base_alpha = 0.08 + 0.25 * (w / stdp.w_max)
                    base_lw = 0.2 + 1.8 * (w / stdp.w_max)
                    ln, = self.ax.plot([xs[i], xh0[j]], [ys[i], yh0[j]], color='lightgray',
                                        alpha=base_alpha, linewidth=base_lw)
                    self.edges.append({'type': 'S-H', 'pre_layer': -1, 'post_layer': 0,
                                       'pre_idx': i, 'post_idx': j, 'w': float(w),
                                       'line': ln, 'base_alpha': base_alpha, 'base_lw': base_lw})
        # Hidden -> Hidden (forward and backward)
        for l in range(self.runner.L - 1):
            xh_pre, yh_pre = xh_layers[l], yh_layers[l]
            xh_post, yh_post = xh_layers[l+1], yh_layers[l+1]
            # Forward edges (l -> l+1)
            Wf = self.runner.W_hh_fwd_list[l]
            for j in range(self.runner.H):  # post index
                for i in range(self.runner.H):  # pre index
                    w = Wf[j, i]
                    if w > 0:
                        base_alpha = 0.06 + 0.22 * (w / stdp.w_max)
                        base_lw = 0.2 + 1.6 * (w / stdp.w_max)
                        ln, = self.ax.plot([xh_pre[i], xh_post[j]], [yh_pre[i], yh_post[j]], color='lightgray',
                                            alpha=base_alpha, linewidth=base_lw)
                        self.edges.append({'type': 'H-H-F', 'pre_layer': l, 'post_layer': l+1,
                                           'pre_idx': i, 'post_idx': j, 'w': float(w),
                                           'line': ln, 'base_alpha': base_alpha, 'base_lw': base_lw})
            # Backward edges (l+1 -> l)
            Wb = self.runner.W_hh_bwd_list[l]
            for i in range(self.runner.H):  # post index (layer l)
                for j in range(self.runner.H):  # pre index (layer l+1)
                    w = Wb[i, j]
                    if w > 0:
                        base_alpha = 0.05 + 0.18 * (w / stdp.w_max)
                        base_lw = 0.15 + 1.4 * (w / stdp.w_max)
                        ln, = self.ax.plot([xh_post[j], xh_pre[i]], [yh_post[j], yh_pre[i]], color='lightgray',
                                            alpha=base_alpha, linewidth=base_lw)
                        self.edges.append({'type': 'H-H-B', 'pre_layer': l+1, 'post_layer': l,
                                           'pre_idx': j, 'post_idx': i, 'w': float(w),
                                           'line': ln, 'base_alpha': base_alpha, 'base_lw': base_lw})
        # Last Hidden -> Output
        xhL, yhL = xh_layers[-1], yh_layers[-1]
        for k in range(self.runner.n_o):
            for j in range(self.runner.H):
                w = self.runner.W_out[k, j]
                if w > 0:
                    base_alpha = 0.08 + 0.25 * (w / stdp.w_max)
                    base_lw = 0.3 + 2.2 * (w / stdp.w_max)
                    ln, = self.ax.plot([xhL[j], xo[k]], [yhL[j], yo[k]], color='lightgray',
                                        alpha=base_alpha, linewidth=base_lw)
                    self.edges.append({'type': 'H-O', 'pre_layer': self.runner.L-1, 'post_layer': None,
                                       'pre_idx': j, 'post_idx': k, 'w': float(w),
                                       'line': ln, 'base_alpha': base_alpha, 'base_lw': base_lw})

        # Per-edge activation buffers
        self.edge_act = np.zeros(len(self.edges), dtype=np.float64)

        # Node scatters
        self.sc_s = self.ax.scatter(xs, ys, s=25, c='tab:blue', label='Sensory')
        self.sc_h_layers = []
        for l in range(self.runner.L):
            sc = self.ax.scatter(xh_layers[l], yh_layers[l], s=35, c='tab:orange', label='Hidden' if l == 0 else None)
            self.sc_h_layers.append(sc)
        self.sc_o = self.ax.scatter(xo, yo, s=45, c='tab:green', label='Output')
        self.ax.legend(loc='upper left')

        # HUD text
        self.txt = self.ax.text(0.02, 0.95, 't=0 ms', transform=self.ax.transAxes)
        self.txt2 = self.ax.text(0.02, 0.90, 'Feedback: OFF', transform=self.ax.transAxes)

        # Controls area
        plt.subplots_adjust(left=0.06, right=0.86, bottom=0.20)
        ax_start = plt.axes([0.88, 0.78, 0.10, 0.08])
        ax_stop = plt.axes([0.88, 0.68, 0.10, 0.08])
        ax_reset = plt.axes([0.88, 0.58, 0.10, 0.08])
        ax_check = plt.axes([0.88, 0.44, 0.10, 0.12])
        ax_radio = plt.axes([0.88, 0.30, 0.10, 0.12])
        ax_file = plt.axes([0.88, 0.22, 0.10, 0.06])
        ax_mic = plt.axes([0.88, 0.14, 0.10, 0.06])
        ax_slider = plt.axes([0.88, 0.06, 0.10, 0.06])
        # Algorithm selectors
        ax_model = plt.axes([0.70, 0.02, 0.16, 0.08])
        ax_izh = plt.axes([0.52, 0.02, 0.16, 0.08])
        ax_learn = plt.axes([0.34, 0.02, 0.16, 0.08])

        self.btn_start = Button(ax_start, 'Start')
        self.btn_stop = Button(ax_stop, 'Stop')
        self.btn_reset = Button(ax_reset, 'Repeat')
        self.chk_feedback = CheckButtons(ax_check, ['Loop feedback'], [False])
        self.rad_input = RadioButtons(ax_radio, ('Random', 'Audio File', 'Microphone'))
        self.btn_file = Button(ax_file, 'Choose File')
        self.btn_mic = Button(ax_mic, 'Mic Start')
        # Slider to control number of sensory neurons
        self.slider_sens = Slider(ax_slider, 'Sensory', valmin=10, valmax=200, valinit=self.runner.n_s,
                                   valstep=1)
        # Radio buttons for algorithms
        self.rad_model = RadioButtons(ax_model, ('LIF', 'Izh'), active=0 if self.runner.neuron_model=='lif' else 1)
        self.rad_izh = RadioButtons(ax_izh, ('RS','FS','IB','CH','LTS','RZ','TC','P'), active=0)
        self.rad_learn = RadioButtons(ax_learn, ('STDP','Hebb','Oja'), active={'stdp':0,'hebb':1,'oja':2}.get(self.runner.learning,0))

        self.btn_start.on_clicked(self._on_start)
        self.btn_stop.on_clicked(self._on_stop)
        self.btn_reset.on_clicked(self._on_reset)
        self.chk_feedback.on_clicked(self._on_toggle_feedback)
        self.rad_input.on_clicked(self._on_source_changed)
        self.btn_file.on_clicked(self._on_choose_file)
        self.btn_mic.on_clicked(self._on_toggle_mic)
        self.slider_sens.on_changed(self._on_sensory_slider)
        self.rad_model.on_clicked(self._on_model_changed)
        self.rad_izh.on_clicked(self._on_izh_changed)
        self.rad_learn.on_clicked(self._on_learning_changed)

        # Input provider state for UI
        self._mic_running = False
        self._audio_file_path = None
        self._provider_mgr = SensoryProviderManager(self.runner.n_s, self.runner.lif.dt)
        # initialize with Random provider
        self.runner.provider = self._provider_mgr.make_random_provider()

        # Equalizer (graphic) in lower-left corner of the main axes
        self.eq_ax = None
        self.eq_bars = None
        self.eq_vals = None
        self.eq_smooth = 0.7
        self._init_equalizer()

        # Animation
        self.ani = animation.FuncAnimation(
            self.fig,
            self._update,
            interval=max(1, int(self.runner.lif.dt)),
            blit=False,
            cache_frame_data=False,
            save_count=1000,
        )

    # --- Algorithm change handlers ---
    def _on_model_changed(self, label):
        sel = (label or '').strip().lower()
        if sel == 'lif':
            self.runner.neuron_model = 'lif'
        else:
            self.runner.neuron_model = 'izh'
        self.runner.reset()

    def _on_izh_changed(self, label):
        name = (label or 'RS').strip().upper()
        try:
            self.runner.izh = izh_preset(name)
        except Exception:
            self.runner.izh = izh_preset('RS')
        if self.runner.neuron_model == 'izh':
            self.runner.reset()

    def _on_learning_changed(self, label):
        sel = (label or '').strip().lower()
        if sel not in ('stdp', 'hebb', 'oja'):
            sel = 'stdp'
        self.runner.learning = sel
        # Reset traces to avoid mixing statistics across rules
        for l in range(self.runner.L):
            self.runner.x_post_h[l].fill(0.0)
            self.runner.x_pre_h[l].fill(0.0)
        self.runner.x_pre_in.fill(0.0)
        self.runner.x_post_o.fill(0.0)

    def _on_start(self, event):
        self.playing = True

    def _on_stop(self, event):
        self.playing = False

    def _on_reset(self, event):
        self.runner.reset()
        self.activity_s.fill(0.0)
        for l in range(self.runner.L):
            self.activity_h_layers[l].fill(0.0)
        self.activity_o.fill(0.0)
        self.playing = True

    def _on_toggle_feedback(self, label):
        self.runner.feedback_enabled = not self.runner.feedback_enabled
        self.txt2.set_text(f'Feedback: {"ON" if self.runner.feedback_enabled else "OFF"}')

    def _on_source_changed(self, label):
        src = label.strip()
        if src == 'Random':
            self.runner.provider = self._provider_mgr.make_random_provider()
            self._init_equalizer()  # reset EQ to default when random is selected
        elif src == 'Audio File':
            if self._audio_file_path:
                prov = self._provider_mgr.make_audio_file_provider(self._audio_file_path)
                if prov is not None:
                    self.runner.provider = prov
                    self._init_equalizer()  # reconfigure EQ for file bands
                else:
                    # fallback
                    self.rad_input.set_active(0)
                    self.runner.provider = self._provider_mgr.make_random_provider()
                    self._init_equalizer()
            else:
                # prompt user to choose file
                self._on_choose_file(None)
        else:  # Microphone
            prov = self._provider_mgr.make_mic_provider()
            if prov is not None:
                self.runner.provider = prov
                self._mic_running = True
                self.btn_mic.label.set_text('Mic Stop')
                self._init_equalizer()  # reconfigure EQ for mic bands
            else:
                self.rad_input.set_active(0)
                self.runner.provider = self._provider_mgr.make_random_provider()
                self._init_equalizer()

    def _on_choose_file(self, event):
        path = None
        try:
            import tkinter as tk
            from tkinter import filedialog
            root = tk.Tk(); root.withdraw()
            path = filedialog.askopenfilename(title='Select audio file',
                                              filetypes=[('Audio', '*.wav *.flac *.ogg *.mp3'), ('All', '*.*')])
            root.update(); root.destroy()
        except Exception:
            path = None
        if not path:
            # no GUI or canceled; keep current provider
            return
        self._audio_file_path = path
        prov = self._provider_mgr.make_audio_file_provider(path)
        if prov is not None:
            self.runner.provider = prov
            self.rad_input.set_active(1)  # Audio File
            self._init_equalizer()
        else:
            self.rad_input.set_active(0)
            self.runner.provider = self._provider_mgr.make_random_provider()
            self._init_equalizer()

    def _on_toggle_mic(self, event):
        if self._mic_running:
            self._provider_mgr.stop_mic()
            self._mic_running = False
            self.btn_mic.label.set_text('Mic Start')
            # keep current provider; user can switch source explicitly
        else:
            prov = self._provider_mgr.make_mic_provider()
            if prov is not None:
                self.runner.provider = prov
                self._mic_running = True
                self.btn_mic.label.set_text('Mic Stop')
                self.rad_input.set_active(2)
                self._init_equalizer()
            else:
                self._mic_running = False
                self.btn_mic.label.set_text('Mic Start')

    def _on_sensory_slider(self, val):
        # Round and clamp
        try:
            n_new = int(round(float(val)))
        except Exception:
            return
        n_new = int(np.clip(n_new, int(self.slider_sens.valmin), int(self.slider_sens.valmax)))
        if n_new == self.runner.n_s:
            return
        # Resize runner first (weights/traces/feedback)
        self.runner.resize_sensory(n_new)
        # Update provider manager and rebuild provider matching current selection
        self._provider_mgr.set_n_s(n_new)
        self._rebuild_provider_after_resize()
        # Recompute sensory coordinates and scatter
        np.random.seed(1)
        xs = np.random.uniform(0.04, 0.10, self.runner.n_s)
        ys = np.linspace(0.1, 0.9, self.runner.n_s)
        self.sc_s.remove()
        self.sc_s = self.ax.scatter(xs, ys, s=25, c='tab:blue', label=None)
        # Rebuild all edges to reflect new S→H wiring
        self._rebuild_all_edges(xs, ys)
        # Re-init equalizer for possibly new band allocation
        self._init_equalizer()

    def _update(self, frame):
        if self.playing:
            state = self.runner.step()
            if state is None:
                # Reached end; stop playing but leave final screen
                self.playing = False
            else:
                # Update activities (decay and add spikes)
                self.activity_s *= self.decay_vis
                for l in range(self.runner.L):
                    self.activity_h_layers[l] *= self.decay_vis
                self.activity_o *= self.decay_vis
                for l in range(self.runner.L):
                    self.activity_h_layers[l] += state['spk_h_layers'][l]
                self.activity_o += state['spk_o']
                # Edge blink updates
                self._update_edges(state)
                # Update equalizer visualization
                self._update_equalizer()
                # Update colors: base color + red overlay proportional to activity
                self._apply_activity_colors()
                self.txt.set_text(f't={int(self.runner.t*self.runner.lif.dt)} ms')
                self.txt2.set_text(f'Feedback: {"ON" if self.runner.feedback_enabled else "OFF"}')

        return []

    def _apply_activity_colors(self):
        # Blend base colors with red based on activity [0..1..]
        def blend(base_rgb, activity):
            activity = np.clip(activity, 0.0, 1.0)
            red = np.array([1.0, 0.0, 0.0])
            return base_rgb*(1-activity) + red*activity

        # Sensory remains static blue (we don't track sensory spikes live here)
        base_s = np.tile(np.array([[0.121, 0.466, 0.705]]), (self.runner.n_s, 1))
        base_h = np.tile(np.array([[1.000, 0.498, 0.054]]), (self.runner.H, 1))
        base_o = np.tile(np.array([[0.173, 0.627, 0.173]]), (self.runner.n_o, 1))
        cols_o = blend(base_o, np.clip(self.activity_o[:, None], 0, 1))
        for l in range(self.runner.L):
            cols_h = blend(base_h, np.clip(self.activity_h_layers[l][:, None], 0, 1))
            self.sc_h_layers[l].set_facecolors(cols_h)
        self.sc_o.set_facecolors(cols_o)

    def _update_edges(self, state):
        # Decay existing activations
        self.edge_act *= self.edge_decay

        s_t = state['s_t']
        spk_h_layers = state['spk_h_layers']

        # Boost activations for edges whose pre neuron fired at this step
        for idx, e in enumerate(self.edges):
            if e['type'] == 'S-H':
                if s_t[e['pre_idx']]:
                    boost = 0.7 + 0.3 * (e['w'] / self.runner.stdp.w_max)
                    self.edge_act[idx] = min(1.0, self.edge_act[idx] + boost)
            elif e['type'] in ('H-H-F', 'H-H-B'):
                if spk_h_layers[e['pre_layer']][e['pre_idx']]:
                    boost = 0.7 + 0.3 * (e['w'] / self.runner.stdp.w_max)
                    self.edge_act[idx] = min(1.0, self.edge_act[idx] + boost)
            else:  # H-O
                if spk_h_layers[e['pre_layer']][e['pre_idx']]:
                    boost = 0.7 + 0.3 * (e['w'] / self.runner.stdp.w_max)
                    self.edge_act[idx] = min(1.0, self.edge_act[idx] + boost)

        # Apply visual properties based on activation
        def apply_line_style(e, act, active_rgb):
            # Blend color from base gray to active_rgb
            act_clamped = float(np.clip(act, 0.0, 1.0))
            col = self.base_gray * (1.0 - act_clamped) + active_rgb * act_clamped
            alpha = np.clip(e['base_alpha'] + 0.6 * act_clamped, 0.02, 1.0)
            lw = e['base_lw'] + 2.5 * act_clamped
            e['line'].set_color(col)
            e['line'].set_alpha(alpha)
            e['line'].set_linewidth(lw)

        for idx, e in enumerate(self.edges):
            if e['type'] == 'S-H' or e['type'] == 'H-H-F':
                color = self.active_color_in
            elif e['type'] == 'H-H-B':
                color = self.active_color_bwd
            else:
                color = self.active_color_out
            apply_line_style(e, self.edge_act[idx], color)

    # ---- Edge rebuild for sensory resize ----
    def _rebuild_all_edges(self, xs, ys):
        # Remove existing line artists
        for e in self.edges:
            try:
                e['line'].remove()
            except Exception:
                pass
        self.edges = []
        # Rebuild edges using current weights
        # Sensory -> H1
        xh0, yh0 = self.sc_h_layers[0].get_offsets()[:,0], self.sc_h_layers[0].get_offsets()[:,1]
        for j in range(self.runner.H):
            for i in range(self.runner.n_s):
                w = self.runner.W_in[j, i]
                if w > 0:
                    base_alpha = 0.08 + 0.25 * (w / self.runner.stdp.w_max)
                    base_lw = 0.2 + 1.8 * (w / self.runner.stdp.w_max)
                    ln, = self.ax.plot([xs[i], xh0[j]], [ys[i], yh0[j]], color='lightgray',
                                        alpha=base_alpha, linewidth=base_lw)
                    self.edges.append({'type': 'S-H', 'pre_layer': -1, 'post_layer': 0,
                                       'pre_idx': i, 'post_idx': j, 'w': float(w),
                                       'line': ln, 'base_alpha': base_alpha, 'base_lw': base_lw})
        # Hidden -> Hidden (unchanged)
        for l in range(self.runner.L - 1):
            xh_pre = self.sc_h_layers[l].get_offsets()
            xh_post = self.sc_h_layers[l+1].get_offsets()
            xh_pre_x, xh_pre_y = xh_pre[:,0], xh_pre[:,1]
            xh_post_x, xh_post_y = xh_post[:,0], xh_post[:,1]
            Wf = self.runner.W_hh_fwd_list[l]
            for j in range(self.runner.H):
                for i in range(self.runner.H):
                    w = Wf[j, i]
                    if w > 0:
                        base_alpha = 0.06 + 0.22 * (w / self.runner.stdp.w_max)
                        base_lw = 0.2 + 1.6 * (w / self.runner.stdp.w_max)
                        ln, = self.ax.plot([xh_pre_x[i], xh_post_x[j]], [xh_pre_y[i], xh_post_y[j]], color='lightgray',
                                            alpha=base_alpha, linewidth=base_lw)
                        self.edges.append({'type': 'H-H-F', 'pre_layer': l, 'post_layer': l+1,
                                           'pre_idx': i, 'post_idx': j, 'w': float(w),
                                           'line': ln, 'base_alpha': base_alpha, 'base_lw': base_lw})
            Wb = self.runner.W_hh_bwd_list[l]
            for i in range(self.runner.H):
                for j in range(self.runner.H):
                    w = Wb[i, j]
                    if w > 0:
                        base_alpha = 0.05 + 0.18 * (w / self.runner.stdp.w_max)
                        base_lw = 0.15 + 1.4 * (w / self.runner.stdp.w_max)
                        ln, = self.ax.plot([xh_post_x[j], xh_pre_x[i]], [xh_post_y[j], xh_pre_y[i]], color='lightgray',
                                            alpha=base_alpha, linewidth=base_lw)
                        self.edges.append({'type': 'H-H-B', 'pre_layer': l+1, 'post_layer': l,
                                           'pre_idx': j, 'post_idx': i, 'w': float(w),
                                           'line': ln, 'base_alpha': base_alpha, 'base_lw': base_lw})
        # Last Hidden -> Output
        xhL = self.sc_h_layers[-1].get_offsets()
        xhLx, xhLy = xhL[:,0], xhL[:,1]
        xo = self.sc_o.get_offsets()[:,0]
        yo = self.sc_o.get_offsets()[:,1]
        for k in range(self.runner.n_o):
            for j in range(self.runner.H):
                w = self.runner.W_out[k, j]
                if w > 0:
                    base_alpha = 0.08 + 0.25 * (w / self.runner.stdp.w_max)
                    base_lw = 0.3 + 2.2 * (w / self.runner.stdp.w_max)
                    ln, = self.ax.plot([xhLx[j], xo[k]], [xhLy[j], yo[k]], color='lightgray',
                                        alpha=base_alpha, linewidth=base_lw)
                    self.edges.append({'type': 'H-O', 'pre_layer': self.runner.L-1, 'post_layer': None,
                                       'pre_idx': j, 'post_idx': k, 'w': float(w),
                                       'line': ln, 'base_alpha': base_alpha, 'base_lw': base_lw})
        # Reset activations buffer to match number of edges
        self.edge_act = np.zeros(len(self.edges), dtype=np.float64)

    def _rebuild_provider_after_resize(self):
        # Determine current selection
        sel = None
        try:
            sel = (self.rad_input.value_selected or '').strip()
        except Exception:
            sel = None
        if sel == 'Audio File' and self._audio_file_path:
            prov = self._provider_mgr.make_audio_file_provider(self._audio_file_path)
            if prov is not None:
                self.runner.provider = prov
            else:
                self.runner.provider = self._provider_mgr.make_random_provider()
                self.rad_input.set_active(0)
        elif sel == 'Microphone':
            prov = self._provider_mgr.make_mic_provider()
            if prov is not None:
                self.runner.provider = prov
            else:
                self.runner.provider = self._provider_mgr.make_random_provider()
                self.rad_input.set_active(0)
        else:
            self.runner.provider = self._provider_mgr.make_random_provider()

    # ---- Equalizer helpers ----
    def _get_provider_bands(self):
        prov = getattr(self.runner, 'provider', None)
        if prov is None:
            return None
        getb = getattr(prov, 'get_last_bands', None)
        if callable(getb):
            try:
                bands = getb()
                if bands is None:
                    return None
                bands = np.asarray(bands, dtype=float)
                if bands.ndim != 1 or bands.size == 0:
                    return None
                # clip to [0,1]
                bands = np.clip(bands, 0.0, 1.0)
                return bands
            except Exception:
                return None
        return None

    def _init_equalizer(self):
        # Create or reset the EQ axes and bars. Place in lower-left corner of the main axes.
        # Determine band count
        bands = self._get_provider_bands()
        n_b = int(bands.size) if bands is not None else 8
        # Create axes if missing
        if self.eq_ax is None:
            # Try inset axes within self.ax; fallback to figure coords
            try:
                self.eq_ax = self.ax.inset_axes([0.03, 0.03, 0.28, 0.20])
            except Exception:
                # compute box in figure coords near the lower-left of ax
                box = self.ax.get_position()
                left = box.x0 + 0.01*(box.x1 - box.x0)
                bottom = box.y0 + 0.02*(box.y1 - box.y0)
                width = 0.30*(box.x1 - box.x0)
                height = 0.22*(box.y1 - box.y0)
                self.eq_ax = self.fig.add_axes([left, bottom, width, height])
        else:
            self.eq_ax.cla()

        self.eq_ax.set_title('Graphic EQ', fontsize=9)
        self.eq_ax.set_ylim(0, 1)
        self.eq_ax.set_xlim(-0.5, n_b - 0.5)
        self.eq_ax.set_xticks([])
        self.eq_ax.set_yticks([])
        self.eq_ax.spines['top'].set_visible(False)
        self.eq_ax.spines['right'].set_visible(False)
        # Initialize values and bars
        self.eq_vals = np.zeros(n_b, dtype=float) if bands is None else bands[:n_b].copy()
        cols = plt.cm.viridis(np.linspace(0.1, 0.9, n_b))
        self.eq_bars = self.eq_ax.bar(np.arange(n_b), self.eq_vals, color=cols, width=0.8)

    def _update_equalizer(self):
        bands = self._get_provider_bands()
        if bands is None:
            # Decay toward zero when no audio bands available
            if self.eq_vals is None:
                return
            self.eq_vals *= 0.9
        else:
            # If band count changed, rebuild
            if self.eq_vals is None or bands.size != self.eq_vals.size:
                self._init_equalizer()
                # Re-fetch current bands for sizing
                bands = self._get_provider_bands()
                if bands is None:
                    return
            # Smooth update
            self.eq_vals = self.eq_smooth * self.eq_vals + (1 - self.eq_smooth) * bands[:self.eq_vals.size]
        # Apply to bars
        if self.eq_bars is not None:
            for rect, h in zip(self.eq_bars, self.eq_vals):
                rect.set_height(float(np.clip(h, 0.0, 1.0)))


# -----------------------------
# Sensory providers (Random, Audio File, Microphone)
# -----------------------------
class BaseSensoryProvider:
    def next_spikes(self):
        raise NotImplementedError
    def stop(self):
        pass
    def get_last_bands(self):
        """Optional: return latest normalized band magnitudes in [0,1] for equalizer. Default: None."""
        return None


class RandomSensoryProvider(BaseSensoryProvider):
    def __init__(self, n_s, dt_ms, base_hz=2.0, burst_hz=25.0, groups=3):
        self.n_s = n_s
        self.dt = dt_ms
        self.groups = np.array_split(np.arange(n_s), max(1, groups))
        self.gi = 0
        self.p_base = base_hz * dt_ms / 1000.0
        self.p_burst = burst_hz * dt_ms / 1000.0
    def next_spikes(self):
        s = (np.random.rand(self.n_s) < self.p_base).astype(np.int8)
        g = self.groups[self.gi]
        if len(g) > 0:
            s[g] = (np.random.rand(len(g)) < self.p_burst).astype(np.int8)
        self.gi = (self.gi + 1) % len(self.groups)
        return s
    def get_last_bands(self):
        return None


class AudioBandMapper:
    def __init__(self, n_s, sample_rate, dt_ms, n_bands=None, p_min=0.01, p_max=0.6, smooth=0.8):
        self.n_s = n_s
        self.fs = sample_rate
        self.dt_ms = dt_ms
        self.hop = max(1, int(round(self.fs * dt_ms / 1000.0)))
        self.win = max(self.hop * 4, 256)
        self.n_bands = n_bands or max(4, min(16, n_s // 4))
        self.band_edges = np.linspace(0, self.fs/2, self.n_bands + 1)
        parts = np.array_split(np.arange(n_s), self.n_bands)
        self.groups = [np.array(p, dtype=int) for p in parts]
        self.p_min = p_min
        self.p_max = p_max
        self.smooth = smooth
        self.prev_band = np.zeros(self.n_bands, dtype=np.float64)
    def bands_from_chunk(self, x):
        # mono chunk expected
        if x.size < self.win:
            x = np.pad(x, (0, self.win - x.size))
        # Hann window FFT
        X = np.fft.rfft(x[:self.win] * np.hanning(self.win))
        mag = np.abs(X)
        freqs = np.fft.rfftfreq(self.win, 1.0/self.fs)
        # integrate per band
        bands = np.zeros(self.n_bands, dtype=np.float64)
        for b in range(self.n_bands):
            f0, f1 = self.band_edges[b], self.band_edges[b+1]
            m = (freqs >= f0) & (freqs < f1)
            if np.any(m):
                bands[b] = float(np.mean(mag[m]))
        # normalize and smooth
        if np.max(bands) > 0:
            bands = bands / (np.max(bands) + 1e-9)
        bands = self.smooth * self.prev_band + (1 - self.smooth) * bands
        self.prev_band = bands
        return bands
    def spikes_from_bands(self, bands):
        probs = self.p_min + (self.p_max - self.p_min) * np.clip(bands, 0, 1)
        s = np.zeros(self.n_s, dtype=np.int8)
        for b, grp in enumerate(self.groups):
            if grp.size:
                s[grp] = (np.random.rand(grp.size) < probs[b]).astype(np.int8)
        return s


class AudioFileProvider(BaseSensoryProvider):
    def __init__(self, path, n_s, dt_ms):
        import soundfile as sf
        self.data, self.fs = sf.read(path, dtype='float32', always_2d=False)
        if self.data.ndim == 2:
            self.data = self.data.mean(axis=1)
        self.pos = 0
        self.mapper = AudioBandMapper(n_s, self.fs, dt_ms)
        self.hop = self.mapper.hop
        self.finished = False
        self.last_bands = None
    def next_spikes(self):
        if self.pos >= len(self.data):
            self.finished = True
            return np.zeros(self.mapper.n_s, dtype=np.int8)
        chunk = self.data[self.pos:self.pos + self.hop]
        self.pos += self.hop
        bands = self.mapper.bands_from_chunk(chunk)
        self.last_bands = bands
        return self.mapper.spikes_from_bands(bands)
    def get_last_bands(self):
        return None if self.last_bands is None else self.last_bands.copy()


class MicrophoneProvider(BaseSensoryProvider):
    def __init__(self, n_s, dt_ms, device=None):
        import sounddevice as sd
        self.n_s = n_s
        self.dt_ms = dt_ms
        self.fs = 16000
        self.mapper = AudioBandMapper(n_s, self.fs, dt_ms)
        self.hop = self.mapper.hop
        self.buf = np.zeros(self.hop * 10, dtype=np.float32)
        self.write_pos = 0
        self.read_pos = 0
        self.last_bands = None
        self.stream = sd.InputStream(samplerate=self.fs, channels=1, dtype='float32', blocksize=self.hop, device=device,
                                     callback=self._callback)
        self.stream.start()
    def _callback(self, indata, frames, time, status):
        x = indata[:, 0]
        n = x.size
        end = self.write_pos + n
        if end > self.buf.size:
            # wrap: shift leftover to start
            rem = self.buf.size - self.write_pos
            if rem > 0:
                self.buf[self.write_pos:] = x[:rem]
            self.buf[:n - rem] = x[rem:]
            self.write_pos = (n - rem) % self.buf.size
        else:
            self.buf[self.write_pos:end] = x
            self.write_pos = end % self.buf.size
    def next_spikes(self):
        # if not enough new samples, return zeros
        available = (self.write_pos - self.read_pos) % self.buf.size
        if available < self.hop:
            return np.zeros(self.n_s, dtype=np.int8)
        start = self.read_pos
        end = (self.read_pos + self.hop) % self.buf.size
        if start < end:
            chunk = self.buf[start:end].copy()
        else:
            chunk = np.concatenate([self.buf[start:], self.buf[:end]])
        self.read_pos = end
        bands = self.mapper.bands_from_chunk(chunk)
        self.last_bands = bands
        return self.mapper.spikes_from_bands(bands)
    def stop(self):
        try:
            self.stream.stop(); self.stream.close()
        except Exception:
            pass
    def get_last_bands(self):
        return None if self.last_bands is None else self.last_bands.copy()


class SensoryProviderManager:
    def __init__(self, n_s, dt_ms):
        self.n_s = n_s
        self.dt_ms = dt_ms
        self._mic_provider = None
    def set_n_s(self, new_n_s: int):
        # Update sensory size and stop mic if running; providers will be rebuilt by caller
        self.n_s = int(max(1, new_n_s))
        self.stop_mic()
    def make_random_provider(self):
        self.stop_mic()
        return RandomSensoryProvider(self.n_s, self.dt_ms)
    def make_audio_file_provider(self, path):
        self.stop_mic()
        try:
            return AudioFileProvider(path, self.n_s, self.dt_ms)
        except Exception as e:
            print(f"Audio file provider error: {e}")
            return None
    def make_mic_provider(self):
        self.stop_mic()
        try:
            # Proactively check access to provide clearer messages
            status, detail = check_microphone_access()
            if status == 'MISSING_DEPS':
                print(f"Microphone provider unavailable: {detail}")
                return None
            if status == 'PERMISSION_DENIED':
                print(f"Microphone permission denied: {detail}")
                if sys.platform == 'darwin':
                    print('On macOS, grant access in System Settings → Privacy & Security → Microphone for your Python/Terminal app.')
                return None
            if status in ('DEVICE_UNAVAILABLE', 'ERROR'):
                print(f"Microphone not available: {detail}")
                return None
            # Otherwise, construct the streaming provider
            self._mic_provider = MicrophoneProvider(self.n_s, self.dt_ms)
            return self._mic_provider
        except Exception as e:
            # Final safety net
            kind = _classify_mic_error(e)
            if kind == 'PERMISSION_DENIED' and sys.platform == 'darwin':
                print(f"Microphone permission denied: {e}\nGrant access in System Settings → Privacy & Security → Microphone.")
            else:
                print(f"Microphone provider unavailable: {e}")
            self._mic_provider = None
            return None
    def stop_mic(self):
        if self._mic_provider is not None:
            self._mic_provider.stop()
            self._mic_provider = None

# -----------------------------
# Build network and run batch simulation (for saved figures)
# -----------------------------
T = 2000  # ms total simulation time
W_in, W_hh_fwd_list, W_hh_bwd_list, W_out, idx_s, idx_h_layers, idx_o = build_network(net)

sensory_spikes, pattern_id, sensory_groups = poisson_input_patterns(T, net.n_sensory, dt=lif.dt)

# Save initial weights for comparison
W_in_init = W_in.copy()
W_hh_fwd_init = [W.copy() for W in W_hh_fwd_list]
W_hh_bwd_init = [W.copy() for W in W_hh_bwd_list]
W_out_init = W_out.copy()

spikes_h_layers, spikes_o, W_in_learned, W_hh_fwd_learned, W_hh_bwd_learned, W_out_learned = run_snn(
    T, W_in.copy(), [W.copy() for W in W_hh_fwd_list], [W.copy() for W in W_hh_bwd_list], W_out.copy(), lif, stdp,
    idx_s, idx_h_layers, idx_o, sensory_spikes.copy()
)

# -----------------------------
# Visualization 1: Network diagram (clusters & sparse links)
# -----------------------------
fig, ax = plt.subplots(figsize=(10, 6))

# Coordinates for clusters
np.random.seed(1)
xs = np.random.uniform(0.04, 0.10, net.n_sensory)
ys = np.linspace(0.1, 0.9, net.n_sensory)

# Hidden layers across 6 columns
L = net.n_hidden_layers
H = net.n_hidden_per_layer
x_positions = np.linspace(0.20, 0.80, L)
xh_layers = [np.random.uniform(x_positions[l]-0.03, x_positions[l]+0.03, H) for l in range(L)]
yh_layers = [np.linspace(0.1, 0.9, H) for _ in range(L)]

xo = np.random.uniform(0.88, 0.95, net.n_output)
yo = np.linspace(0.2, 0.8, net.n_output)

ax.scatter(xs, ys, s=20, c='tab:blue', label='Sensory')
for l in range(L):
    ax.scatter(xh_layers[l], yh_layers[l], s=30, c='tab:orange', label='Hidden' if l == 0 else None)
ax.scatter(xo, yo, s=40, c='tab:green', label='Output')

# Draw sparse edges (sensory->H1)
for j in range(H):
    for i in range(net.n_sensory):
        if W_in_init[j, i] > 0:
            ax.plot([xs[i], xh_layers[0][j]], [ys[i], yh_layers[0][j]], color='gray', alpha=0.15, linewidth=0.5)

# Draw sparse edges (Hi↔H{i+1}) forward and backward
for l in range(L - 1):
    Wf = W_hh_fwd_init[l]
    for j in range(H):
        for i in range(H):
            if Wf[j, i] > 0:
                ax.plot([xh_layers[l][i], xh_layers[l+1][j]], [yh_layers[l][i], yh_layers[l+1][j]], color='gray', alpha=0.12, linewidth=0.4)
    Wb = W_hh_bwd_init[l]
    for i in range(H):
        for j in range(H):
            if Wb[i, j] > 0:
                ax.plot([xh_layers[l+1][j], xh_layers[l][i]], [yh_layers[l+1][j], yh_layers[l][i]], color='gray', alpha=0.08, linewidth=0.35)

# Draw sparse edges (H6->O)
for k in range(net.n_output):
    for j in range(H):
        if W_out_init[k, j] > 0:
            ax.plot([xh_layers[-1][j], xo[k]], [yh_layers[-1][j], yo[k]], color='gray', alpha=0.20, linewidth=0.7)

ax.set_title('Neuromorphic Network: Sparsely Interconnected Clusters')
ax.axis('off')
ax.legend(loc='upper left')
plt.tight_layout()
plt.savefig('neuromorphic_network_diagram.png', dpi=150)
plt.close(fig)

# -----------------------------
# Visualization 2: Spike raster over time
# -----------------------------
fig, axes = plt.subplots(3, 1, figsize=(12, 8), sharex=True)

# Sensory raster (only show first 30 for compactness)
show_n_sensory = min(30, net.n_sensory)
for i in range(show_n_sensory):
    t_idx = np.where(sensory_spikes[:, i] > 0)[0]
    axes[0].scatter(t_idx, i * np.ones_like(t_idx), s=6, color='tab:blue')
axes[0].set_ylabel('Sensory (subset)')
axes[0].set_title('Spike Raster (Sensory / Hidden (6 layers) / Output)')

# Hidden raster: stack layers with offsets
offset = 0
for l in range(L):
    layer_spikes = spikes_h_layers[l]
    for j in range(H):
        t_idx = np.where(layer_spikes[:, j] > 0)[0]
        axes[1].scatter(t_idx, (offset + j) * np.ones_like(t_idx), s=4, color='tab:orange')
    offset += H
axes[1].set_ylabel('Hidden (stacked)')

# Output raster
for k in range(net.n_output):
    t_idx = np.where(spikes_o[:, k] > 0)[0]
    axes[2].scatter(t_idx, k * np.ones_like(t_idx), s=10, color='tab:green')
axes[2].set_ylabel('Output')
axes[2].set_xlabel('Time (ms)')

plt.tight_layout()
plt.savefig('spike_raster.png', dpi=150)
plt.close(fig)

# -----------------------------
# Visualization 3: Weight distributions pre vs post learning
# -----------------------------
fig, axes = plt.subplots(1, 2, figsize=(12, 4))
axes[0].hist(W_in_init[W_in_init > 0].flatten(), bins=20, alpha=0.7, label='init')
axes[0].hist(W_in_learned[W_in_init > 0].flatten(), bins=20, alpha=0.7, label='learned')
axes[0].set_title('Sensory→H1 weights (pre vs post)')
axes[0].legend()

# Aggregate all hidden↔hidden weights (forward + backward)
W_hh_init_all = np.concatenate([
    *(W[W > 0].flatten() for W in (W_hh_fwd_init if W_hh_fwd_init else [])),
    *(W[W > 0].flatten() for W in (W_hh_bwd_init if W_hh_bwd_init else []))
]) if (W_hh_fwd_init or W_hh_bwd_init) else np.array([])
W_hh_learned_all = np.concatenate([
    *(W[W > 0].flatten() for W in (W_hh_fwd_learned if W_hh_fwd_learned else [])),
    *(W[W > 0].flatten() for W in (W_hh_bwd_learned if W_hh_bwd_learned else []))
]) if (W_hh_fwd_learned or W_hh_bwd_learned) else np.array([])
if W_hh_init_all.size > 0:
    axes[1].hist(W_hh_init_all, bins=20, alpha=0.7, label='init H→H')
if W_hh_learned_all.size > 0:
    axes[1].hist(W_hh_learned_all, bins=20, alpha=0.7, label='learned H→H')
axes[1].set_title('Hidden↔Hidden weights (pre vs post)')
axes[1].legend()

fig2, ax2 = plt.subplots(1, 1, figsize=(6, 4))
ax2.hist(W_out_init[W_out_init > 0].flatten(), bins=20, alpha=0.7, label='init')
ax2.hist(W_out_learned[W_out_init > 0].flatten(), bins=20, alpha=0.7, label='learned')
ax2.set_title('Last Hidden→Output weights (pre vs post)')
ax2.legend()
axes[1].legend()

plt.tight_layout()
plt.savefig('weight_histograms.png', dpi=150)
plt.close(fig)
plt.tight_layout()
fig2.savefig('weight_histograms_output.png', dpi=150)
plt.close(fig2)

# -----------------------------
# Visualization 4: Final network with edge thickness proportional to learned weights
# -----------------------------
fig, ax = plt.subplots(figsize=(10, 6))
ax.scatter(xs, ys, s=20, c='tab:blue', label='Sensory')
for l in range(L):
    ax.scatter(xh_layers[l], yh_layers[l], s=30, c='tab:orange', label='Hidden' if l == 0 else None)
ax.scatter(xo, yo, s=40, c='tab:green', label='Output')

# edges scaled by learned weights
for j in range(H):
    for i in range(net.n_sensory):
        w = W_in_learned[j, i]
        if w > 0:
            ax.plot([xs[i], xh_layers[0][j]], [ys[i], yh_layers[0][j]], color='tab:gray', alpha=0.12 + 0.25 * (w / stdp.w_max),
                    linewidth=0.3 + 2.0 * (w / stdp.w_max))

for l in range(L - 1):
    Wf = W_hh_fwd_learned[l]
    for j in range(H):
        for i in range(H):
            w = Wf[j, i]
            if w > 0:
                ax.plot([xh_layers[l][i], xh_layers[l+1][j]], [yh_layers[l][i], yh_layers[l+1][j]],
                        color='tab:gray', alpha=0.10 + 0.22 * (w / stdp.w_max), linewidth=0.2 + 1.8 * (w / stdp.w_max))
    Wb = W_hh_bwd_learned[l]
    for i in range(H):
        for j in range(H):
            w = Wb[i, j]
            if w > 0:
                ax.plot([xh_layers[l+1][j], xh_layers[l][i]], [yh_layers[l+1][j], yh_layers[l][i]],
                        color='tab:gray', alpha=0.08 + 0.18 * (w / stdp.w_max), linewidth=0.18 + 1.6 * (w / stdp.w_max))

for k in range(net.n_output):
    for j in range(H):
        w = W_out_learned[k, j]
        if w > 0:
            ax.plot([xh_layers[-1][j], xo[k]], [yh_layers[-1][j], yo[k]], color='tab:gray', alpha=0.12 + 0.25 * (w / stdp.w_max),
                    linewidth=0.5 + 2.5 * (w / stdp.w_max))

ax.set_title('Learned Network: Edge thickness ∝ synaptic strength')
ax.axis('off')
ax.legend(loc='upper left')
plt.tight_layout()
plt.savefig('final_weighted_network.png', dpi=150)
plt.close(fig)

print('Files generated:')
print(' - neuromorphic_network_diagram.png')
print(' - spike_raster.png')
print(' - weight_histograms.png')
print(' - final_weighted_network.png')
print(' - weight_histograms_output.png')

# -----------------------------
# Launch live animation window with controls
# -----------------------------

# Use the learned weights to start the live demo (looks more structured)
runner = SNNRunner(
    W_in_learned, W_hh_fwd_learned, W_hh_bwd_learned, W_out_learned,
    lif, stdp, idx_s, idx_h_layers, idx_o, sensory_spikes, feedback_map=None,
    neuron_model=NEURON_MODEL, izh=IZH_PARAMS, learning=LEARNING_RULE
)

# Reuse the same node coordinates from the static diagram
np.random.seed(1)
xs = np.random.uniform(0.04, 0.10, net.n_sensory)
ys = np.linspace(0.1, 0.9, net.n_sensory)
# reuse xh_layers, yh_layers, xo, yo from earlier section

if getattr(_args, 'check_access', False):
    # Print a probe report at startup (non-fatal)
    print_access_report(ui_requested=(_args.ui and not _args.no_ui))

if _args.no_ui or not _args.ui:
    print('Live UI disabled: UI not requested. Run with --ui to enable the interactive window.')
elif not _is_gui_backend():
    print(f"Live UI disabled: Non-GUI backend '{mpl.get_backend()}'. Install/choose Tk/Qt/WX and run with --ui.")
else:
    try:
        animator = SNNAnimator(runner, xs, ys, xh_layers, yh_layers, xo, yo, stdp)
        # Ensure the window blocks until closed
        plt.show(block=True)
    except Exception as e:
        print(f"Live UI could not start: {e}. Falling back to headless mode.")
