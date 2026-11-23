// SPDX-FileCopyrightText: 2026 AARNN contributors
// SPDX-License-Identifier: MIT
//
// Licensed under the MIT License. See sim/minecraft/LICENSE in the project root.
package org.neuralmimicry.minecraft;

import com.google.gson.JsonParser;
import org.junit.jupiter.api.Test;
import static org.junit.jupiter.api.Assertions.*;

/** Vectors replicated from crates/aarnn-embodiment (actuation.rs, celegans.rs). */
final class OutputDecoderTest {
    private static final double REFERENCE_FRAME_MS=2;
    private static int[] frame(boolean event,int channel) { return event?new int[]{channel}:new int[0]; }

    /** graded_position_readout_spans_the_full_joint_range (hexapod femur [-1.4, -0.2]). */
    @Test void gradedPositionReadoutSpansTheFullJointRange() {
        var p=Content.profile("hexapod");
        int channel=p.output_names().indexOf("hex_o_001_lf_femur");
        assertEquals(1,channel);
        double lo=-1.4,hi=-.2;
        for(double density:new double[]{0,.5,1}) {
            double[] a=new double[p.output()];double phase=0;
            for(int step=1;step<=400;step++) {
                phase+=density;boolean event=false;
                if(phase>=1-1e-12) {phase-=1;event=true;}
                OutputDecoder.advance(p,a,frame(event,channel),OutputDecoder.Code.PULSE_DENSITY,REFERENCE_FRAME_MS);
            }
            double target=lo+Senses.clamp(a[channel],0,1)*(hi-lo),expected=lo+density*(hi-lo);
            assertTrue(Math.abs(target-expected)<.08*(hi-lo),"density "+density+": "+target+" vs "+expected);
        }
    }

    /** pulse_density_code_decodes_linearly (C. elegans MDR05; spiking keeps the existing display). */
    @Test void pulseDensityCodeDecodesLinearly() {
        var p=Content.profile("celegans");
        int mdr=-1;
        for(int i=0;i<p.output();i++) if(p.output_names().get(i).endsWith("MDR05")) mdr=i;
        assertTrue(mdr>=0);
        for(double[] c:new double[][]{{.2,.6},{.5,.8}}) {
            double density=c[0];
            double[] graded=new double[p.output()],spiking=new double[p.output()];
            double phase=0,sumGraded=0,sumSpiking=0;int n=0;
            for(int f=0;f<5000;f++) {
                phase+=density;boolean event=false;
                if(phase>=1) {phase-=1;event=true;}
                OutputDecoder.advance(p,graded,frame(event,mdr),OutputDecoder.Code.PULSE_DENSITY,REFERENCE_FRAME_MS);
                OutputDecoder.advance(p,spiking,frame(event,mdr),OutputDecoder.Code.SPIKES,REFERENCE_FRAME_MS);
                if(f>=2500) {sumGraded+=graded[mdr];sumSpiking+=spiking[mdr];n++;}
            }
            assertTrue(Math.abs(sumGraded/n-density)<.01,"p "+density+": "+sumGraded/n);
            assertTrue(sumSpiking/n>c[1],"spike display compresses: "+sumSpiking/n);
        }
        // The worm's graded filter is the 55 ms muscle constant; others use the 20 ms readout.
        double[] worm=new double[p.output()];
        OutputDecoder.advance(p,worm,new int[]{mdr},OutputDecoder.Code.PULSE_DENSITY,REFERENCE_FRAME_MS);
        assertEquals(1-Math.exp(-2.0/55),worm[mdr],1e-15);
        var hexapod=Content.profile("hexapod");double[] hex=new double[hexapod.output()];
        OutputDecoder.advance(hexapod,hex,new int[]{0},OutputDecoder.Code.PULSE_DENSITY,REFERENCE_FRAME_MS);
        assertEquals(1-Math.exp(-2.0/20),hex[0],1e-15);
    }

    @Test void handshakeOutputCodeNegotiation() {
        var hs=(java.util.function.Function<String,OutputDecoder.Code>)json->
                OutputDecoder.Code.parse(JsonParser.parseString(json).getAsJsonObject().get("output_code"));
        assertEquals(OutputDecoder.Code.SPIKES,hs.apply("{\"expected_o\":96}"));
        assertEquals(OutputDecoder.Code.SPIKES,hs.apply("{\"output_code\":null}"));
        assertEquals(OutputDecoder.Code.SPIKES,hs.apply("{\"output_code\":\"spikes\"}"));
        assertEquals(OutputDecoder.Code.PULSE_DENSITY,hs.apply("{\"output_code\":\"pulse_density\"}"));
        for(String bad:new String[]{"\"analog\"","1","true","[\"pulse_density\"]"})
            assertThrows(IllegalArgumentException.class,()->hs.apply("{\"output_code\":"+bad+"}"));
    }

    @Test void spikingDisplayIsUnchanged() {
        var p=Content.profile("zebrafish");double[] a=new double[p.output()];a[3]=.5;
        OutputDecoder.advance(p,a,new int[]{0},OutputDecoder.Code.SPIKES,Gateway.DT_MS);
        assertEquals(1,a[0]);assertEquals(.5*.82,a[3],1e-15);assertEquals(0,a[1]);
    }
}
