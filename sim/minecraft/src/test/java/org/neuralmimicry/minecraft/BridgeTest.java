package org.neuralmimicry.minecraft;

import com.google.gson.JsonParser;
import java.io.DataInputStream;
import java.net.InetAddress;
import java.net.ServerSocket;
import java.net.URI;
import java.nio.ByteBuffer;
import java.nio.ByteOrder;
import java.nio.charset.StandardCharsets;
import java.util.Map;
import java.util.concurrent.CompletableFuture;
import java.util.concurrent.TimeUnit;
import org.junit.jupiter.api.Test;
import static org.junit.jupiter.api.Assertions.*;

final class BridgeTest {
    private static byte[] read(DataInputStream in) throws Exception {
        int size=Integer.reverseBytes(in.readInt());assertTrue(size>0&&size<32768);return in.readNBytes(size);
    }
    private static void write(java.io.OutputStream out,byte[] frame) throws Exception {
        out.write(ByteBuffer.allocate(4).order(ByteOrder.LITTLE_ENDIAN).putInt(frame.length).array());out.write(frame);out.flush();
    }
    @Test void sixProfilesTraverseHttpAndAer1Tcp() throws Exception {
        for(var p:Content.DATA.profiles()) {
            try(var tcp=new ServerSocket(0,1,InetAddress.getByName("127.0.0.1"))) {
                tcp.setSoTimeout(5000);
                var wire=CompletableFuture.runAsync(()-> {
                    try(var peer=tcp.accept()) {
                        peer.setSoTimeout(3000);var in=new DataInputStream(peer.getInputStream());var out=peer.getOutputStream();
                        var hello=JsonParser.parseString(new String(read(in),StandardCharsets.UTF_8)).getAsJsonObject();
                        assertEquals(p.sensory(),hello.get("sensory").getAsInt());assertEquals(1,hello.get("dt_ms").getAsInt());
                        assertEquals(p.output_names().size(),hello.getAsJsonArray("o_names").size());
                        write(out,("{\"expected_s\":"+p.sensory()+",\"expected_o\":"+p.output()+"}").getBytes(StandardCharsets.UTF_8));
                        byte[] frame=read(in);assertArrayEquals(new byte[]{65,69,82,49},java.util.Arrays.copyOf(frame,4));
                        assertEquals(0,ByteBuffer.wrap(frame,4,8).order(ByteOrder.LITTLE_ENDIAN).getLong());
                        // Independent golden: at 1000 us, output address 16384 (channel 0), value 1.
                        write(out,new byte[]{65,69,82,49,(byte)232,3,0,0,0,0,0,0,0,(byte)128,(byte)128,1,1});
                    } catch(Exception e) {throw new RuntimeException(e);}
                });
                try(var server=BridgeMain.start(0,"test-token",Map.of(p.id(),new BridgeMain.Backend(p,tcp.getLocalPort())))) {
                    var session=new Gateway.Session(p,new Gateway.Binding(p.id(),null,p.sensory(),p.output(),0),
                        URI.create("http://127.0.0.1:"+server.server.getAddress().getPort()+"/api/aer/infer"),"test-token");
                    try {
                        session.submit(new double[p.sensory()],77);session.completion().toCompletableFuture().get(4,TimeUnit.SECONDS);
                        var reply=session.poll();assertNotNull(reply,p.id());assertEquals(1,reply.step());assertArrayEquals(new int[]{0},reply.spikes());
                        wire.get(4,TimeUnit.SECONDS);
                    } finally {session.stop("test cleanup");}
                }
            }
        }
    }
    @Test void codecRejectsTruncationOverflowAndIncorrectAddress() {
        assertEquals(60000,Content.frameTimeoutMillis(Content.profile("drosophila_banc")));
        assertEquals(60000,Content.frameTimeoutMillis(Content.profile("drosophila_fafb")));
        assertEquals(10000,Content.frameTimeoutMillis(Content.profile("zebrafish")));
        byte[] silent=AerCodec.encode(1000,new double[24]);assertEquals(12,silent.length);
        assertEquals(1000,AerCodec.decode(silent,96).timestampUs());
        assertThrows(RuntimeException.class,()->AerCodec.decode(new byte[11],96));
        byte[] wrong=java.util.Arrays.copyOf(silent,15);wrong[12]=0;wrong[13]=0;wrong[14]=1;
        assertThrows(RuntimeException.class,()->AerCodec.decode(wrong,96));
        byte[] truncated=java.util.Arrays.copyOf(silent,13);truncated[12]=(byte)128;
        assertThrows(RuntimeException.class,()->AerCodec.decode(truncated,96));
        assertThrows(RuntimeException.class,()->AerCodec.encode(0,new double[]{Double.NaN}));
    }
}
