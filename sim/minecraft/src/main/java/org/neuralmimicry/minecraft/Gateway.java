package org.neuralmimicry.minecraft;

import com.google.gson.Gson;
import com.google.gson.JsonObject;
import java.io.ByteArrayOutputStream;
import java.net.URI;
import java.net.http.HttpClient;
import java.net.http.HttpRequest;
import java.net.http.HttpResponse;
import java.nio.ByteBuffer;
import java.time.Duration;
import java.util.List;
import java.util.concurrent.CompletableFuture;
import java.util.concurrent.CompletionStage;
import java.util.concurrent.Flow;

/** Opt-in legacy sandbox gateway. Never asserts production admission or EffectId guarantees. */
public final class Gateway {
    public record Binding(String networkId, String nodeId, int sensory, int output, long firstStep) {}
    public record Reply(long step, int[] spikes) {}
    public record Capture(long sequence, long captureNanos, long step, String contentDigest) {}
    public record Metrics(long inputFrames, long outputFrames, long lastInputStep,
                          long lastOutputStep, int lastOutputSpikes, double lastInputMean) {}
    private static final Gson JSON = new Gson();
    public static final int MAX_RESPONSE = 65536;

    private Gateway() {}
    public static URI endpoint(String value) {
        URI u = URI.create(value);
        boolean local = List.of("127.0.0.1", "localhost", "[::1]", "::1").contains(u.getHost());
        if ((!"https".equals(u.getScheme()) && !("http".equals(u.getScheme()) && local))
                || u.getUserInfo()!=null || u.getFragment()!=null || u.getQuery()!=null
                || !"/api/aer/infer".equals(u.getPath()))
            throw new IllegalArgumentException("Use HTTPS /api/aer/infer, or HTTP on loopback only");
        return u;
    }
    public static final class Session {
        private final Content.Profile profile;
        private final Binding binding;
        private final URI uri;
        private final String token;
        private final HttpClient client;
        private CompletableFuture<HttpResponse<byte[]>> pending;
        private long nextStep, sequence, lastOutput = -1;
        private long inputFrames, outputFrames, lastInputStep = -1, lastOutputStep = -1;
        private int lastOutputSpikes;
        private double lastInputMean;
        private boolean active = true;
        private Capture capture;
        private String status = "armed: legacy sandbox";

        public Session(Content.Profile p, Binding b, URI uri, String token) {
            if (b == null || b.networkId()==null || b.networkId().isBlank() || b.networkId().length()>256
                    || b.sensory()!=p.sensory() || b.output()!=p.output() || b.firstStep()<0 || b.firstStep()>16000000
                    || token==null || token.isBlank() || token.length()>16384)
                throw new IllegalArgumentException("Binding dimensions, firstStep or credential invalid");
            this.profile=p; this.binding=b; this.uri=endpoint(uri.toString()); this.token=token;
            nextStep=b.firstStep();
            client=HttpClient.newBuilder().connectTimeout(Duration.ofSeconds(2))
                    .followRedirects(HttpClient.Redirect.NEVER).build();
        }
        public boolean active() { return active; }
        public boolean ready() { return active && lastOutput>=0; }
        public boolean pending() { return pending != null; }
        public String status() { return status; }
        public Capture capture() { return capture; }
        public Binding binding() { return binding; }
        public Metrics metrics() {
            return new Metrics(inputFrames, outputFrames, lastInputStep, lastOutputStep,
                    lastOutputSpikes, lastInputMean);
        }
        CompletionStage<?> completion() { return pending; }
        public void submit(double[] input, long captureNanos) {
            if (!active || pending!=null) throw new IllegalStateException("Session disarmed or credit occupied");
            if (input.length!=profile.sensory() || nextStep>16000000) throw new IllegalArgumentException("Frame bounds");
            for (double v:input) if (!Double.isFinite(v) || v<0 || v>1) throw new IllegalArgumentException("Sensory range");
            capture=new Capture(sequence++,captureNanos,nextStep,Content.DATA.digest());
            inputFrames++; lastInputStep=nextStep;
            lastInputMean=java.util.Arrays.stream(input).average().orElse(0.0);
            JsonObject body=new JsonObject();
            body.addProperty("content_digest",Content.DATA.digest());
            body.addProperty("capture_sequence",capture.sequence());
            body.addProperty("capture_nanos",capture.captureNanos());
            body.addProperty("network_id",binding.networkId()); body.addProperty("node_id",binding.nodeId());
            body.addProperty("step_index",nextStep); body.addProperty("time_ms",nextStep); body.addProperty("dt_ms",1);
            body.addProperty("timeout_ms",250); body.add("input_values",JSON.toJsonTree(input));
            int timeout=Content.frameTimeoutMillis(profile);
            var request=HttpRequest.newBuilder(uri).timeout(Duration.ofMillis(timeout))
                .header("Content-Type","application/json").header("Authorization","Bearer "+token)
                .POST(HttpRequest.BodyPublishers.ofString(JSON.toJson(body))).build();
            pending=client.sendAsync(request, ignored -> new BoundedBody());
            pending.orTimeout(timeout+500L,java.util.concurrent.TimeUnit.MILLISECONDS);
            // A healthy session deliberately keeps one bounded request in flight.
            // Do not label every normal frame as a connection that is still
            // pending; that made an active robot look disconnected in status.
            status=lastOutput<0 ? "pending: first neural frame" : "active: legacy sandbox (frame pending)";
        }
        /** Called on the server tick only; never waits for network I/O. */
        public Reply poll() {
            if (!active || pending==null || !pending.isDone()) return null;
            var completed=pending; pending=null;
            try {
                var response=completed.join();
                if (response.statusCode()!=200) throw new IllegalArgumentException("HTTP "+response.statusCode());
                Reply reply=decode(response.body(),profile,binding.networkId(),lastOutput);
                lastOutput=reply.step(); lastOutputStep=lastOutput;
                outputFrames++; lastOutputSpikes=reply.spikes().length;
                nextStep=Math.max(nextStep+1,lastOutput+1);
                status="active: legacy sandbox";
                return reply;
            } catch (RuntimeException error) {
                // Admission may have succeeded. Never retry automatically after an ambiguous response.
                stop("fault: admission/output unknown; inspect gateway before reconnecting");
                return null;
            }
        }
        public void stop(String reason) {
            active=false;
            if (pending!=null) pending.cancel(true);
            pending=null; status=reason;
            client.shutdownNow();
        }
    }
    static Reply decode(byte[] bytes, Content.Profile p, String network, long lastOutput) {
        if (bytes.length>MAX_RESPONSE) throw new IllegalArgumentException("Response too large");
        JsonObject data=JSON.fromJson(new String(bytes,java.nio.charset.StandardCharsets.UTF_8),JsonObject.class);
        if (!network.equals(data.get("network_id").getAsString()) || data.get("output_step_index").isJsonNull())
            throw new IllegalArgumentException("Wrong network or unavailable output");
        long step=data.get("output_step_index").getAsBigDecimal().longValueExact();
        if (step<0 || step<=lastOutput || step>16000000) throw new IllegalArgumentException("Stale/out-of-range output");
        var indices=data.getAsJsonArray("output_spike_indices");
        if (indices.size()>p.output()) throw new IllegalArgumentException("Too many spikes");
        int[] spikes=new int[indices.size()]; boolean[] seen=new boolean[p.output()];
        for(int i=0;i<spikes.length;i++) {
            int n=indices.get(i).getAsBigDecimal().intValueExact();
            if(n<0 || n>=p.output() || seen[n]) throw new IllegalArgumentException("Invalid/duplicate output address");
            seen[n]=true; spikes[i]=n;
        }
        return new Reply(step,spikes);
    }
    static final class BoundedBody implements HttpResponse.BodySubscriber<byte[]> {
        private final CompletableFuture<byte[]> result=new CompletableFuture<>();
        private final ByteArrayOutputStream bytes=new ByteArrayOutputStream();
        private Flow.Subscription subscription;
        public CompletionStage<byte[]> getBody() { return result; }
        public void onSubscribe(Flow.Subscription s) { subscription=s; s.request(1); }
        public void onNext(List<ByteBuffer> buffers) {
            for(var buffer:buffers) {
                if(buffer.remaining()>MAX_RESPONSE-bytes.size()) {
                    subscription.cancel(); result.completeExceptionally(new IllegalArgumentException("Response too large")); return;
                }
                byte[] block=new byte[buffer.remaining()]; buffer.get(block); bytes.writeBytes(block);
            }
            subscription.request(1);
        }
        public void onError(Throwable error) { result.completeExceptionally(error); }
        public void onComplete() { result.complete(bytes.toByteArray()); }
    }
}
