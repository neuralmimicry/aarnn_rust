/* Bounded legacy sandbox state. Rust remains the only neural executor. */
export class Session {
  constructor(profile, digest, request, firstStep = 0) {
    if (!Number.isSafeInteger(firstStep) || firstStep < 0 || firstStep > 16000000) throw Error('Invalid initial sequence');
    this.profile = profile; this.digest = digest; this.request = request;
    this.step = firstStep; this.lastOutput = -1; this.sequence = 0; this.pending = false; this.active = true;
    this.generation = 0; this.reply = null; this.status = 'armed';
  }
  stop(reason = 'disarmed') {
    this.active = false; this.generation++; this.reply = null; this.status = reason;
    // An outstanding request retains its credit until completion. It is never retried.
  }
  submit(values, captureTick) {
    if (!this.active || this.pending) throw Error('Session unavailable or pending');
    if (values.length !== this.profile.sensory || values.some(v => !Number.isFinite(v) || v < 0 || v > 1)) throw Error('Invalid sensory frame');
    const generation = this.generation, submitted = this.step;
    const body = {network_id:this.profile.id,node_id:null,step_index:submitted,time_ms:submitted,dt_ms:1,
      content_digest:this.digest,capture_sequence:this.sequence++,capture_nanos:captureTick*50000000,
      capture_clock:'bedrock_server_tick',input_values:values};
    this.pending = true;
    const timeout = this.profile.kind === 'fly' ? 60 : this.profile.kind === 'fish' ? 10 : 2.5;
    return Promise.resolve().then(() => this.request(body, timeout)).then(response => {
      if (!this.active || this.generation !== generation) return;
      if (response.status !== 200 || typeof response.body !== 'string' || response.body.length > 65536) throw Error('Invalid bridge response');
      const reply = JSON.parse(response.body), spikes = reply.output_spike_indices;
      if (reply.network_id !== this.profile.id || reply.content_digest !== this.digest ||
          !Number.isSafeInteger(reply.output_step_index) || reply.output_step_index <= this.lastOutput || reply.output_step_index > 16000000 ||
          !Array.isArray(spikes) || spikes.length > this.profile.output || new Set(spikes).size !== spikes.length ||
          spikes.some(v => !Number.isInteger(v) || v < 0 || v >= this.profile.output)) throw Error('Stale or malformed output');
      // Legacy AER1 returns the Rust executor's step, independently of the
      // submitted request sequence. Require monotonic output, as the Java
      // adapter does; do not invent a biological time relation between them.
      this.reply = reply; this.lastOutput = reply.output_step_index;
      this.step = Math.max(submitted + 1, reply.output_step_index + 1);
    }).catch(() => { if (generation === this.generation) this.stop('fault: inspect bridge before reconnecting'); })
      .finally(() => { this.pending = false; });
  }
  poll() { const reply = this.reply; this.reply = null; return reply; }
}
