// SPDX-FileCopyrightText: 2026 AARNN contributors
// SPDX-License-Identifier: MIT
//
// Licensed under the MIT License. See sim/minecraft/LICENSE in the project root.
package org.neuralmimicry.minecraft;

import com.google.gson.JsonElement;

/**
 * Brain-output decode for the kinematic Minecraft preview, ported from the
 * MuJoCo reference (crates/aarnn-embodiment: aer.rs OutputCode, actuation.rs
 * graded readout, celegans.rs muscles). Values are per-output activations in
 * [0,1] that drive the preview's bounded movement and visible anatomy; they
 * imply no joint dynamics, contacts or muscle mechanics.
 *
 * <p>The brain advertises its output code once, in the AER1 handshake reply
 * ({@code output_code}). Absent/null or {@code "spikes"} keeps the preview's
 * spike display (each exchanged frame scales activity by 0.82; an event sets
 * 1). {@code "pulse_density"} carries a graded activation as the fraction of
 * frames with an event; it is decoded linearly by the reference first-order
 * filter (20 ms readout, 55 ms C. elegans muscle), so a steady activation
 * {@code a} settles at {@code a}. Any other code is refused.
 */
public final class OutputDecoder {
    public enum Code {
        SPIKES("spikes"), PULSE_DENSITY("pulse_density");
        private final String wire;
        Code(String wire) { this.wire=wire; }
        public String wire() { return wire; }
        /** Absent/null means spikes; anything other than the two known codes is refused. */
        public static Code parse(JsonElement value) {
            if(value==null||value.isJsonNull()) return SPIKES;
            if(value.isJsonPrimitive()&&value.getAsJsonPrimitive().isString())
                for(var code:values()) if(code.wire.equals(value.getAsString())) return code;
            throw new IllegalArgumentException("Unsupported brain output_code "+value);
        }
    }
    /** Reference readout filter (motor::RATE_TAU_S) and C. elegans muscle time constants. */
    public static final double READOUT_TAU_MS=20, MUSCLE_TAU_MS=55;
    /** Existing preview spike display: per-frame persistence of a spike. */
    public static final double SPIKE_DISPLAY_DECAY=.82;
    private OutputDecoder() {}
    /**
     * Advances {@code activations} by one exchanged frame in place.
     * {@code spikes} lists the validated, distinct output channels with an
     * event in that frame; {@code frameMs} is the frame's negotiated step.
     */
    public static double[] advance(Content.Profile profile,double[] activations,int[] spikes,Code code,double frameMs) {
        if(code==Code.PULSE_DENSITY) {
            if(!(frameMs>0)||!Double.isFinite(frameMs)) throw new IllegalArgumentException("Frame period");
            double keep=Math.exp(-frameMs/(profile.kind().equals("worm")?MUSCLE_TAU_MS:READOUT_TAU_MS));
            // A pulse-density frame carries at most one event per channel.
            boolean[] event=new boolean[activations.length];
            for(int i:spikes) event[i]=true;
            for(int i=0;i<activations.length;i++) activations[i]=activations[i]*keep+(event[i]?1-keep:0);
        } else {
            for(int i=0;i<activations.length;i++) activations[i]*=SPIKE_DISPLAY_DECAY;
            for(int i:spikes) activations[i]=1;
        }
        return activations;
    }
}
