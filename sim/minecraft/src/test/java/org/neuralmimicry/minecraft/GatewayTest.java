package org.neuralmimicry.minecraft;

import com.google.gson.JsonParser;
import com.sun.net.httpserver.HttpServer;
import java.net.InetSocketAddress;
import java.net.URI;
import java.nio.charset.StandardCharsets;
import java.util.concurrent.CountDownLatch;
import java.util.concurrent.TimeUnit;
import java.util.concurrent.atomic.AtomicReference;
import org.junit.jupiter.api.Test;
import static org.junit.jupiter.api.Assertions.*;

final class GatewayTest {
    static byte[] reply(String network,long step,String spikes) {
        return ("{\"network_id\":\""+network+"\",\"output_step_index\":"+step+",\"output_spike_indices\":"+spikes+"}").getBytes(StandardCharsets.UTF_8);
    }
    @Test void hostileAndStaleResponsesFailClosed() {
        var p=Content.profile("hexapod");
        assertThrows(RuntimeException.class,()->Gateway.decode(reply("wrong",2,"[0]"),p,"brain",0));
        for(String indices:new String[]{"[-1]","[18]","[0,0]","[0.5]"})
            assertThrows(RuntimeException.class,()->Gateway.decode(reply("brain",2,indices),p,"brain",0));
        assertThrows(RuntimeException.class,()->Gateway.decode(reply("brain",2,"[0]"),p,"brain",2));
        assertThrows(RuntimeException.class,()->Gateway.decode(new byte[65537],p,"brain",0));
        for(String uri:new String[]{"http://example.com/api/aer/infer","https://a:b@example.com/api/aer/infer","https://example.com/other"})
            assertThrows(IllegalArgumentException.class,()->Gateway.endpoint(uri));
    }
    @Test void allProfilesUseBoundedAuthenticatedRequests() throws Exception {
        var received=new AtomicReference<String>();var auth=new AtomicReference<String>();
        var server=HttpServer.create(new InetSocketAddress("127.0.0.1",0),0);
        server.createContext("/api/aer/infer",exchange->{
            received.set(new String(exchange.getRequestBody().readNBytes(65536),StandardCharsets.UTF_8));
            auth.set(exchange.getRequestHeaders().getFirst("Authorization"));
            byte[] output=reply("brain",42,"[0]");exchange.sendResponseHeaders(200,output.length);
            exchange.getResponseBody().write(output);exchange.close();
        });server.start();
        try {
            for(var p:Content.DATA.profiles()) {
                var session=new Gateway.Session(p,new Gateway.Binding("brain",null,p.sensory(),p.output(),4),
                        URI.create("http://127.0.0.1:"+server.getAddress().getPort()+"/api/aer/infer"),"fixture-token");
                try {
                    assertFalse(session.ready());
                    session.submit(new double[p.sensory()],123456);
                    assertEquals("pending: first neural frame",session.status());
                    assertThrows(IllegalStateException.class,()->session.submit(new double[p.sensory()],0));
                    session.completion().toCompletableFuture().get(4,TimeUnit.SECONDS);
                    assertFalse(session.ready());
                    var output=session.poll();assertNotNull(output);assertEquals(42,output.step());
                    assertTrue(session.ready());
                    assertEquals("active: legacy sandbox",session.status());
                    var metrics=session.metrics();
                    assertEquals(1,metrics.inputFrames()); assertEquals(1,metrics.outputFrames());
                    assertEquals(4,metrics.lastInputStep()); assertEquals(42,metrics.lastOutputStep());
                    assertEquals(1,metrics.lastOutputSpikes()); assertEquals(0.0,metrics.lastInputMean());
                    assertNull(session.poll()); assertEquals("Bearer fixture-token",auth.get());
                    var request=JsonParser.parseString(received.get()).getAsJsonObject();
                    assertEquals(p.sensory(),request.getAsJsonArray("input_values").size());
                    assertEquals(4,request.get("time_ms").getAsInt());
                    assertEquals(123456,session.capture().captureNanos());
                    session.stop("test stop");assertFalse(session.active());assertNull(session.poll());
                    assertFalse(session.ready());
                } finally {session.stop("cleanup");}
            }
        } finally {server.stop(0);}
    }
    @Test void stopRejectsLateReplyAndTimeoutDisarms() throws Exception {
        var arrived=new CountDownLatch(1);var release=new CountDownLatch(1);
        var server=HttpServer.create(new InetSocketAddress("127.0.0.1",0),0);
        server.createContext("/api/aer/infer",exchange->{
            arrived.countDown();try {release.await(5,TimeUnit.SECONDS);} catch(InterruptedException error){Thread.currentThread().interrupt();}
            exchange.close();
        });server.start();
        var p=Content.profile("nao");
        var uri=URI.create("http://127.0.0.1:"+server.getAddress().getPort()+"/api/aer/infer");
        var session=new Gateway.Session(p,new Gateway.Binding("brain",null,250,40,0),uri,"fixture");
        try {
            session.submit(new double[250],11);assertTrue(arrived.await(3,TimeUnit.SECONDS));
            session.stop("operator stop");release.countDown();assertNull(session.poll());assertFalse(session.active());
        } finally {session.stop("cleanup");server.stop(0);}
        var stalled=HttpServer.create(new InetSocketAddress("127.0.0.1",0),0);
        var finish=new CountDownLatch(1);
        stalled.createContext("/api/aer/infer",exchange->{try {finish.await(5,TimeUnit.SECONDS);}catch(InterruptedException e){Thread.currentThread().interrupt();}exchange.close();});stalled.start();
        var timed=new Gateway.Session(p,new Gateway.Binding("brain",null,250,40,0),
            URI.create("http://127.0.0.1:"+stalled.getAddress().getPort()+"/api/aer/infer"),"fixture");
        try {
            timed.submit(new double[250],22);
            assertThrows(Exception.class,()->timed.completion().toCompletableFuture().get(4,TimeUnit.SECONDS));
            assertNull(timed.poll());assertFalse(timed.active());assertTrue(timed.status().contains("unknown"));
        } finally {timed.stop("cleanup");finish.countDown();stalled.stop(0);}
    }
}
