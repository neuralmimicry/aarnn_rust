# Resilience

## Independent dimensions

The profile's `[resilience]` table is stored expanded in every result; a preset name alone is never evidence. Each dimension is configured separately:

- **Required function:** the task suite.
- **Fault population:** `eligible_population` plus optional `fault_regions`.
- **Fault exposure:**
  - `targeted_single_faults_required`: exhaustive single-neuron loss over the population;
  - `random_neuron_loss_fraction` with `trials_per_sampled_claim`.
- **Recovery policy:** `none` means acute, with frozen plasticity. `plasticity` (adaptive recovery) is declared but not exercised by the authored suite.
- **Evidence procedure:** `minimum_success_probability`, `confidence` and `confidence_scope`.

## Fault semantics

- **Neuron loss.** A neuron fault is a structural mask applied before the first step, in the cold-start-empty policy.
- **Random selection.** Random selections are seeded and deterministic, with round-half-up counts of at least one cell. They draw without replacement and lesion simultaneously.
- **Not modelled.** Biological transmission failure, as distinct from transport loss, is not modelled implicitly. Release is reliable (`p_release_default` 1.0) unless a source declares otherwise. Quantisation and timing faults need a numerical hardware model, so they are not implemented by corrupting time.

## Evidence

- **Exhaustive single faults.** The claim passes only if every eligible single loss passes the suite. This proves the declared single-fault scope only.
- **Sampled claims.**
  - `P(success) ≥ p_min` uses the one-sided exact Clopper-Pearson lower bound (`stats::clopper_pearson_lower`):
    - `passed` when the lower bound is ≥ p_min;
    - `failed` when the upper bound is < p_min;
    - otherwise `inconclusive`, never rounded to a pass.
  - Golden cases: 299/299 at 95 % gives 0.990031 > 0.99; 298/298 gives 0.989997; 95/100 gives 0.8977466, matching `scipy.stats.beta.ppf(0.05, 95, 6)`.
- **Multiple claims.** With `confidence_scope = "simultaneous-required-claims"`, each claim's confidence is Bonferroni-adjusted: 1 − (1 − c)/k for k claims.
- **Invalid trials.** A crashed or missing worker result is an infrastructure error, not a failed trial. A valid trial that loses function is a failure.

## Authored comparison

The demo checks the compact candidate against two profiles:

- **Compact profile** (`configs/explorer/default.toml`): no resilience requirement. The selected candidate drops one go chain and one relay per steering side.
- **Resilient profile** (`configs/explorer/resilient.toml`):
  - every single loss in the go and steer regions must be survived;
  - P(success | random 7 % loss in those regions) ≥ 0.99 must hold at 95 % family confidence, with 20 trials per claim.

Under the resilient profile, the compact candidate fails the single-fault claim: losing its only go chain loses the go response. The resilient search keeps the redundant pathways and removes only what single faults allow. Its sampled claim is `inconclusive`, because 20/20 successes give a lower bound of about 0.83, below 0.99. That is the explicit insufficient-evidence result. Establishing the bound would need about 300 failure-free trials per claim after the Bonferroni adjustment.
