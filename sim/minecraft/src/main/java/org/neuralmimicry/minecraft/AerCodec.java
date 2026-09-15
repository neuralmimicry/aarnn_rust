package org.neuralmimicry.minecraft;

import java.io.ByteArrayOutputStream;
import java.nio.ByteBuffer;
import java.nio.ByteOrder;
import java.util.ArrayList;

/** Existing AER1 compatibility framing (src/aer.rs), not the future AARNN-AER/1 protocol. */
public final class AerCodec {
    private AerCodec() {}
    public record Output(long timestampUs,int[] spikes) {}
    public static byte[] encode(long timestampUs,double[] values) {
        if(timestampUs<0||values.length>8192)throw new IllegalArgumentException("Frame bounds");
        var out=new ByteArrayOutputStream();out.writeBytes(new byte[]{'A','E','R','1'});
        out.writeBytes(ByteBuffer.allocate(8).order(ByteOrder.LITTLE_ENDIAN).putLong(timestampUs).array());
        for(int i=0;i<values.length;i++) {
            if(!Double.isFinite(values[i])||values[i]<0||values[i]>1)throw new IllegalArgumentException("Sensory range");
            if(values[i]>.5) {write(out,0);write(out,4096+i);write(out,1);}
        }
        return out.toByteArray();
    }
    public static Output decode(byte[] bytes,int count) {
        if(count<1||count>512||bytes.length<12||bytes.length>12+count*15)throw new IllegalArgumentException("Frame bounds");
        var input=ByteBuffer.wrap(bytes).order(ByteOrder.LITTLE_ENDIAN);
        if(input.getInt()!=0x31524541)throw new IllegalArgumentException("AER1 magic");
        long time=input.getLong();if(time<0)throw new IllegalArgumentException("Timestamp overflow");
        var spikes=new ArrayList<Integer>();boolean[] seen=new boolean[count];int events=0;
        while(input.hasRemaining()) {
            if(++events>count)throw new IllegalArgumentException("Event bound");
            time=Math.addExact(time,read(input));long address=read(input),value=read(input);
            if(address<16384||address>=16384+count||value>1||seen[(int)address-16384])throw new IllegalArgumentException("Output address/value");
            seen[(int)address-16384]=true;if(value!=0)spikes.add((int)address-16384);
        }
        return new Output(time,spikes.stream().mapToInt(Integer::intValue).toArray());
    }
    private static void write(ByteArrayOutputStream out,long value) {
        while(value>=128){out.write((int)(value&127)|128);value>>>=7;}out.write((int)value);
    }
    private static long read(ByteBuffer input) {
        long value=0;
        for(int shift=0;shift<63;shift+=7) {
            if(!input.hasRemaining())throw new IllegalArgumentException("Truncated varint");
            int b=input.get()&255;value|=(long)(b&127)<<shift;if((b&128)==0)return value;
        }
        throw new IllegalArgumentException("Varint overflow");
    }
}
