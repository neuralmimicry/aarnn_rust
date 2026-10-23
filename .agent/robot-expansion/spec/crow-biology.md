# Biological carrion crow: Corvus corone

## Biological reference and honest model scope

Default adult form: all-black plumage and bill, two wings, two legs, tail and anisodactyl feet (three forward toes plus an opposing hallux). RSPB lists length 45–47 cm, wingspan 93–104 cm and mass 370–650 g. BTO’s summary weight is 508.7 g. The proposed nominal body uses 0.47 m length, 0.98 m wingspan and 0.510 kg; these are selected reference values, not a scanned individual. Exact segment lengths/masses, inertia, joint ranges and aerodynamic coefficients require separate provenance and calibration.

Sources: [RSPB](https://www.rspb.org.uk/birds-and-wildlife/identifying-birds/corvids-how-to-identify-the-crow-family), [BTO](https://www.bto.org/learn/about-birds/birdfacts/carrion-crow). No complete measured crow connectome or validated complete musculoskeletal simulation is supplied. Start with a configurable AARNN sensorimotor network; do not claim neuron-count or cognitive equivalence.

## Physical morphology

Create separate anatomical collision/rigid bodies for torso, head/neck, jaw, articulated wings, tail and legs/feet. Feather surfaces provide a visual skin plus declared aerodynamic panels; they are not massive duplicate collision meshes. Keep symmetry in the rest anatomy while allowing independent left/right wing action. A visual rig’s mirror constraints must not force physically identical wing commands during banking or recovery.

The proposed reduced controller has 31 controlled coordinates: neck yaw/pitch/roll (3), beak gape (1), six per wing (12), tail pitch/yaw/fan (3), and six per leg (12). Each leg’s front-toe curl and hallux curl are reduced grasp synergies, not a claim to model every tendon independently. Expose individual toe contact geometry even where actuation is reduced. Keep passive/compliant joints and feather/tendon constraints explicitly separate from controlled coordinates.

Build and test original low-detail geometry before attaching any optional CC0/CC BY artistic mesh. Rebind an artistic skin to the physical skeleton with explicit bones/weights; do not let an animation drive the physical root. The identified [Nodelete model](https://blendswap.com/blend/7340) requires sign-in to download and is optional. It supplies neither validated mass nor aerodynamics. Open Blender files with automatic embedded-script execution disabled and export a locally reviewed asset.

## Flight and ground behaviour

Implement weight, wing articulation, force/moment generation from local airflow, tail control, finite power/torque limits, profile drag and energy expenditure. A section or reduced aerodynamic model is acceptable when its assumptions and calibration are explicit. [PteraSoftware](https://github.com/camUrban/PteraSoftware) is an MIT optional offline aerodynamic reference; it is not a complete crow controller. Do not copy unlicensed BIRD_SIM code/meshes. No hidden vertical thruster, root teleport, per-spike upward push or always-positive lift is allowed.

The crow must be able to stand, walk, hop, take off from ground/perch, fly under power, glide with sink, bank/turn, land, grasp a perch and release. Contacts, friction and toe constraints carry perch load. A temporary grasp constraint may be created only after verified contact, with explicit impulse/force/energy limits and release; attaching a nearby foot magnetically is not grasping. Beak pickup uses actual object contact and a declared grasp model.

Include gravity, ground/tree/branch geometry, variable perch diameters, obstacles, food, water, moving visual cues, sound sources, wind/gust fields and safe boundaries. Internal energy, hunger, thirst and fatigue influence available effort and sensory projections. Environment/physiology update rates are independent of rendering. Distinguish a minimal stable reflex network from a learned skill policy and from long-horizon cognition.

## Proposed sensory and motor projections

`crow-io.proposed.json` enumerates ordered channel names. Its **492 inputs** are: two reduced 12×8 luminance ON/OFF retinas (384), angle/velocity for 31 coordinates (62), vestibular/head-gravity measurements (9), beak/foot/wing contacts (12), two-ear eight-band audio (16), airspeed (3), physiology (4), and food/water taste (2). These are engineering projections, not the crow’s literal receptors or neuron count. Colour/UV resolution or a larger retina is a future versioned profile, not an unannounced remap.

The **64 outputs** are positive/negative populations for 31 coordinates (62), plus reduced syrinx and respiratory drive (2). Use causal bounded filters and actuator constraints. Motor sign, rest pose, delay, sample rates, normalisation and missing data must be explicit. A gravity-direction sensor is a labelled engineering proxy for reduced vestibular information, not an assertion about a biological sensor organ. Do not expose world target labels, privileged object IDs or global pose as if they were biological senses.

## Networks and learning

Generate a new network/configuration and IO binding without editing existing animal networks. Include configurable reflex/CPG motifs within the neural model, proprioceptive feedback and sensory drive; all evaluated active control must emerge through the AARNN outputs. Mechanical diagnostic controllers may supply reference trajectories during development and supervised data collection, but must be disabled in neural-only evaluation.

Train progressively: joint response, stand/balance, stepping, beak/foot grasp, glide control, flapping/lift, takeoff, stable flight, landing/perching, feeding and gust recovery. Record seeds, datasets, reward/error definitions, held-out environments and resource budgets. Evaluate multiple seeds and unseen disturbances; compare silent, random, diagnostic and trained neural control. Do not claim emergent intelligence from an animation or guarantee sophisticated crow cognition from an untrained small SNN.

## Required physical falsification checks

An unsupported unpowered airborne bird must fall or glide with net energy loss. Reversing/holding relevant wing commands must change signed forces as predicted. Doubling mass must alter acceleration/power requirements. Symmetric motion in still air must not create unexplained yaw; asymmetric commands must produce a plausible bank/turn. Disabling toe contact/closure must prevent a stable perch. Stop/reconnect must not replay a stale flap or grasp. Collision and sensor changes must reach the correct brain. Run these checks independently of the learning score.
