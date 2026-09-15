package org.neuralmimicry.minecraft;

import com.google.gson.Gson;
import com.google.gson.JsonObject;
import com.sun.net.httpserver.HttpExchange;
import com.sun.net.httpserver.HttpServer;
import java.io.DataInputStream;
import java.io.IOException;
import java.net.InetSocketAddress;
import java.net.Socket;
import java.nio.ByteBuffer;
import java.nio.ByteOrder;
import java.nio.charset.StandardCharsets;
import java.security.MessageDigest;
import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.Map;
import java.util.concurrent.ArrayBlockingQueue;
import java.util.concurrent.ThreadPoolExecutor;
import java.util.concurrent.TimeUnit;
import java.util.concurrent.locks.ReentrantLock;

/** Loopback-only, token-protected sandbox bridge to the existing Rust robot TCP protocol.
 * No Java neural execution. AER1 replies carry legacy time, without production commit/fence metadata. */
public final class BridgeMain {
    private static final Gson JSON=new Gson();
    private BridgeMain() {}
    public static void main(String[] args) throws Exception {
        if(args.length==1&&args[0].equals("--help")) {
            System.out.println("java -jar aarnn-minecraft-<minecraft>-0.1.0-bridge.jar [--port 62620] [--base-port 7890] [--profiles celegans,...]\nRequires AARNN_MINECRAFT_TOKEN (24+ characters). TCP ports follow profile order. Loopback only.");return;
        }
        int port=62620,base=7890;String profiles=String.join(",",Content.DATA.profiles().stream().map(Content.Profile::id).toList());
        for(int i=0;i<args.length;i+=2) {
            if(i+1==args.length) throw new IllegalArgumentException("Missing option value");
            switch(args[i]) {
                case "--port" -> port=Integer.parseInt(args[i+1]);
                case "--base-port" -> base=Integer.parseInt(args[i+1]);
                case "--profiles" -> profiles=args[i+1];
                default -> throw new IllegalArgumentException("Unknown option");
            }
        }
        if(port<1024||port>65535||base<1024||base>65529) throw new IllegalArgumentException("Port range");
        String token=System.getenv("AARNN_MINECRAFT_TOKEN");
        if(token==null||token.length()<24||token.length()>4096) throw new IllegalArgumentException("Set AARNN_MINECRAFT_TOKEN (24–4096 characters)");
        Map<String,Backend> backends=new LinkedHashMap<>();
        for(String id:profiles.split(",")) {
            var p=Content.profile(id);
            if(backends.putIfAbsent(id,new Backend(p,base+backends.size()))!=null) throw new IllegalArgumentException("Duplicate profile");
        }
        // Warm only the transport/mapping handshake before advertising readiness. The
        // legacy Rust listener clones large imported snapshots when accepting clients.
        // That bounded startup work must not consume a live sensory-frame deadline.
        var startup=java.util.concurrent.Executors.newFixedThreadPool(backends.size());
        try {
            var pending=new ArrayList<java.util.concurrent.Future<?>>();
            for(var backend:backends.values())pending.add(startup.submit(()->{
                try {backend.prepare();} catch(IOException error){throw new java.io.UncheckedIOException(error);}
            }));
            for(var future:pending)future.get(65,TimeUnit.SECONDS);
        } catch(Exception error) {
            backends.values().forEach(Backend::close);throw error;
        } finally {startup.shutdownNow();}
        try(var running=start(port,token,backends)) {
            System.out.println("AARNN sandbox bridge ready on 127.0.0.1:"+running.server.getAddress().getPort()+"; content "+Content.DATA.digest());
            backends.forEach((id,b)->System.out.println(id+" -> Rust TCP 127.0.0.1:"+b.port+" ("+b.profile.sensory()+"/"+b.profile.output()+")"));
            var stopped=new java.util.concurrent.CountDownLatch(1);
            Runtime.getRuntime().addShutdownHook(new Thread(stopped::countDown));
            stopped.await();
        }
    }
    static Running start(int port,String token,Map<String,Backend> backends) throws IOException {
        System.setProperty("jdk.httpserver.maxConnections","16");
        System.setProperty("sun.net.httpserver.maxReqTime","3");
        System.setProperty("sun.net.httpserver.maxRspTime","65");
        var server=HttpServer.create(new InetSocketAddress("127.0.0.1",port),8);
        var pool=new ThreadPoolExecutor(6,6,0,TimeUnit.SECONDS,new ArrayBlockingQueue<>(6));
        server.setExecutor(pool);
        server.createContext("/api/aarnn/health",exchange->{
            try {
                String auth=exchange.getRequestHeaders().getFirst("Authorization");
                if(auth==null||!MessageDigest.isEqual(auth.getBytes(StandardCharsets.UTF_8),("Bearer "+token).getBytes(StandardCharsets.UTF_8)))
                    respond(exchange,401,Map.of("error","unauthorised"));
                else respond(exchange,200,Map.of("content_digest",Content.DATA.digest(),"profiles",backends.keySet()));
            } finally {exchange.close();}
        });
        server.createContext("/api/aer/infer",exchange->{
            try {
                String auth=exchange.getRequestHeaders().getFirst("Authorization");
                if(auth==null||!MessageDigest.isEqual(auth.getBytes(StandardCharsets.UTF_8),("Bearer "+token).getBytes(StandardCharsets.UTF_8))) {
                    respond(exchange,401,Map.of("error","unauthorised"));return;
                }
                if(!exchange.getRequestMethod().equals("POST")||!exchange.getRequestURI().getPath().equals("/api/aer/infer")) {
                    respond(exchange,405,Map.of("error","POST /api/aer/infer required"));return;
                }
                byte[] raw=exchange.getRequestBody().readNBytes(32769);
                if(raw.length>32768) {respond(exchange,413,Map.of("error","frame too large"));return;}
                var request=JSON.fromJson(new String(raw,StandardCharsets.UTF_8),JsonObject.class);
                if(!Content.DATA.digest().equals(request.get("content_digest").getAsString())) throw new IllegalArgumentException("Content digest mismatch");
                String id=request.get("network_id").getAsString();var backend=backends.get(id);
                if(backend==null) {respond(exchange,404,Map.of("error","profile route unavailable"));return;}
                if(request.has("node_id")&&!request.get("node_id").isJsonNull()&&!request.get("node_id").getAsString().isBlank())
                    throw new IllegalArgumentException("node_id must be empty for a pinned TCP route");
                if(!backend.lock.tryLock()) {respond(exchange,409,Map.of("error","one in-flight frame per brain"));return;}
                try {respond(exchange,200,backend.infer(request));}
                finally {backend.lock.unlock();}
            } catch(IllegalArgumentException|NullPointerException error) {
                respond(exchange,400,Map.of("error","invalid bounded profile frame"));
            } catch(Exception error) {
                respond(exchange,503,Map.of("error","Rust endpoint unavailable or ambiguous; restart bridge after inspection"));
            } finally {exchange.close();}
        });server.start();return new Running(server,pool,backends);
    }
    static final class Running implements AutoCloseable {
        final HttpServer server;final ThreadPoolExecutor pool;final Map<String,Backend> backends;
        Running(HttpServer server,ThreadPoolExecutor pool,Map<String,Backend> backends) {this.server=server;this.pool=pool;this.backends=backends;}
        public void close() {server.stop(0);pool.shutdownNow();backends.values().forEach(Backend::close);}
    }
    private static void respond(HttpExchange exchange,int status,Object body) throws IOException {
        byte[] data=JSON.toJson(body).getBytes(StandardCharsets.UTF_8);
        exchange.getResponseHeaders().set("Content-Type","application/json");
        exchange.sendResponseHeaders(status,data.length);exchange.getResponseBody().write(data);
    }
    static final class Backend {
        final Content.Profile profile;final int port;final ReentrantLock lock=new ReentrantLock();
        Socket socket;long lastStep=-1;boolean faulted;
        Backend(Content.Profile p,int port) {profile=p;this.port=port;}
        void prepare() throws IOException {
            if(socket!=null)return;
            socket=new Socket();socket.connect(new InetSocketAddress("127.0.0.1",port),1000);socket.setSoTimeout(60000);
            var hello=new JsonObject();hello.addProperty("sensory",profile.sensory());hello.addProperty("output",profile.output());hello.addProperty("dt_ms",1);
            hello.add("s_names",JSON.toJsonTree(profile.sensor_names()));hello.add("o_names",JSON.toJsonTree(profile.output_names()));
            write(JSON.toJson(hello).getBytes(StandardCharsets.UTF_8));
            var ack=JSON.fromJson(new String(read(4096),StandardCharsets.UTF_8),JsonObject.class);
            if(ack.get("expected_s").getAsInt()!=profile.sensory()||ack.get("expected_o").getAsInt()!=profile.output())
                throw new IOException("Rust channel mismatch");
            socket.setSoTimeout(Content.frameTimeoutMillis(profile)-500);
        }
        Map<String,Object> infer(JsonObject request) throws IOException {
            if(faulted) throw new IOException("Faulted endpoint");
            long step=request.get("step_index").getAsBigDecimal().longValueExact();
            if(step<0||step>16000000||step<=lastStep) throw new IllegalArgumentException("Stale sequence");
            if(request.get("dt_ms").getAsDouble()!=1||request.get("time_ms").getAsDouble()!=step)
                throw new IllegalArgumentException("Sandbox mapping mismatch");
            var values=request.getAsJsonArray("input_values");
            if(values.size()!=profile.sensory()) throw new IllegalArgumentException("Input dimensions");
            double[] sensory=new double[profile.sensory()];int index=0;
            for(var v:values) {
                float n=v.getAsFloat();if(!Float.isFinite(n)||n<0||n>1) throw new IllegalArgumentException("Input range");
                sensory[index++]=n;
            }
            byte[] frame=AerCodec.encode(step*1000,sensory);
            try {
                prepare();
                // Consume sequence before send. A timeout cannot silently cause this input to be replayed.
                lastStep=step;write(frame);var output=AerCodec.decode(read(12+profile.output()*15),profile.output());
                return Map.of("network_id",profile.id(),"output_step_index",output.timestampUs()/1000,"output_spike_indices",output.spikes(),
                    "time_source","legacy_AER1_reply","quality","legacy_uncommitted","content_digest",Content.DATA.digest());
            } catch(Exception error) {
                faulted=true;close();
                System.err.println("Rust route "+profile.id()+" fault: "+error.getClass().getSimpleName());
                throw new IOException("Ambiguous backend",error);
            }
        }
        private void write(byte[] data) throws IOException {
            var out=socket.getOutputStream();out.write(ByteBuffer.allocate(4).order(ByteOrder.LITTLE_ENDIAN).putInt(data.length).array());out.write(data);out.flush();
        }
        private byte[] read(int limit) throws IOException {
            var input=new DataInputStream(socket.getInputStream());int size=Integer.reverseBytes(input.readInt());
            if(size<=0||size>limit) throw new IOException("Frame length");
            byte[] bytes=input.readNBytes(size);if(bytes.length!=size)throw new IOException("Truncated frame");return bytes;
        }
        void close() {if(socket!=null)try{socket.close();}catch(IOException ignored){/* Already faulted/disarmed. */}socket=null;}
    }
}
