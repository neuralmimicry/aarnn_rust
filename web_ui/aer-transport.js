/* AARNN-AER/1 browser adapter. Generated/portable protocol logic only. */
(function (global) {
  "use strict";
  const VERSION = 1;
  const MAX_PAYLOAD = 1024 * 1024 - 256;

  function crc16(bytes) {
    let crc = 0xffff;
    for (let i = 0; i < bytes.length; i += 1) {
      crc ^= bytes[i] << 8;
      for (let bit = 0; bit < 8; bit += 1) {
        crc = (crc & 0x8000) !== 0 ? ((crc << 1) ^ 0x1021) & 0xffff : (crc << 1) & 0xffff;
      }
    }
    return crc;
  }

  function u64(value) {
    const result = typeof value === "bigint" ? value : BigInt(value);
    if (result < 0n || result > 18446744073709551615n) throw new Error("AER u64 is out of range");
    return result;
  }

  function directionCode(direction) {
    if (direction === "producer" || direction === 0) return 0;
    if (direction === "consumer" || direction === 1) return 1;
    if (direction === "duplex" || direction === 2) return 2;
    throw new Error("invalid AER direction");
  }

  function payloadTypeCode(payloadType) {
    if (payloadType === "events" || payloadType === 0) return 0;
    if (payloadType === "spike_indices" || payloadType === 1) return 1;
    if (payloadType === "gap" || payloadType === 2) return 2;
    if (payloadType === "effect" || payloadType === 3) return 3;
    throw new Error("invalid AER payload type");
  }

  // Must remain byte-for-byte identical to AerFrame::canonical_bytes_without_crc.
  function canonicalBytes(frame) {
    const payload = Uint8Array.from(frame.payload || []);
    const gap = frame.gap || null;
    const reason = gap ? new TextEncoder().encode(String(gap.reason || "")) : new Uint8Array(0);
    const size = 2 + 8 + (8 * 8) + 3 + 4 + payload.length + (gap ? 8 + 4 + reason.length : 0) + (frame.effect_id != null ? 8 : 0);
    const bytes = new Uint8Array(size);
    const view = new DataView(bytes.buffer);
    let offset = 0;
    view.setUint16(offset, Number(frame.protocol_version), false); offset += 2;
    view.setBigUint64(offset, u64(frame.session_id), false); offset += 8;
    ["endpoint_epoch", "device_epoch", "source_sequence", "capture_timestamp_ns", "clock_mapping_version", "clock_uncertainty_ns", "address_space_version", "frame_sequence"].forEach(function (name) {
      view.setBigUint64(offset, u64(frame[name]), false); offset += 8;
    });
    bytes[offset++] = directionCode(frame.direction);
    bytes[offset++] = frame.polarity ? 1 : 0;
    bytes[offset++] = payloadTypeCode(frame.payload_type);
    view.setUint32(offset, payload.length, false); offset += 4;
    bytes.set(payload, offset); offset += payload.length;
    if (gap) {
      view.setBigUint64(offset, u64(gap.first_missing_sequence), false); offset += 8;
      view.setUint32(offset, Number(gap.count), false); offset += 4;
      bytes.set(reason, offset); offset += reason.length;
    }
    if (frame.effect_id != null) view.setBigUint64(offset, u64(frame.effect_id), false);
    return bytes;
  }

  class BrowserAerSession {
    constructor(sessionId, endpointEpoch, deviceEpoch, credits) {
      if (!sessionId || !endpointEpoch || !deviceEpoch || !credits) throw new Error("invalid AER session");
      this.sessionId = sessionId;
      this.endpointEpoch = endpointEpoch;
      this.deviceEpoch = deviceEpoch;
      this.pathEpoch = 1;
      this.nextSequence = 0;
      this.credits = credits;
      this.creditLimit = credits;
      this.received = new Set();
    }

    nextFrame(frame) {
      if (this.credits <= 0) throw new Error("AER credit window exhausted");
      const next = Object.assign({}, frame, {
        protocol_version: VERSION,
        session_id: this.sessionId,
        endpoint_epoch: this.endpointEpoch,
        device_epoch: this.deviceEpoch,
        frame_sequence: this.nextSequence
      });
      if (!Array.isArray(next.payload) || next.payload.length > MAX_PAYLOAD) throw new Error("AER payload exceeds bound");
      next.crc16 = crc16(canonicalBytes(next));
      this.nextSequence += 1;
      this.credits -= 1;
      return next;
    }

    receive(frame) {
      if (frame.protocol_version !== VERSION || frame.session_id !== this.sessionId || frame.endpoint_epoch !== this.endpointEpoch || frame.device_epoch !== this.deviceEpoch) throw new Error("stale AER endpoint");
      if (frame.crc16 !== crc16(canonicalBytes(frame))) throw new Error("AER CRC mismatch");
      if (this.received.has(frame.frame_sequence)) return false;
      this.received.add(frame.frame_sequence);
      return true;
    }

    acknowledge(creditReturn) {
      this.credits = Math.min(this.creditLimit, this.credits + Math.max(0, Number(creditReturn) || 0));
      return { endpoint_epoch: this.endpointEpoch, credit_window: this.credits, path_epoch: this.pathEpoch };
    }

    migratePath() {
      this.pathEpoch += 1;
      return this.pathEpoch;
    }
  }

  global.AARNNBrowserAerSession = BrowserAerSession;
  global.AARNN_AER_PROTOCOL_VERSION = VERSION;
})(window);
