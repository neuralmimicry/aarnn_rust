package org.neuralmimicry.minecraft;

import com.google.gson.*;
import java.net.URI;
import java.net.http.*;
import java.time.Duration;
import java.util.List;
import java.util.concurrent.CompletableFuture;

/**
 * Bounded asynchronous viewer/environment client. It never senses a robot or
 * carries AER; the authenticated native service is the only physics/AER owner.
 */
public final class PhysicsSession implements AutoCloseable {
  public static final class Config {
    public int schema_version = 1;
    public String endpoint = "http://127.0.0.1:8092", token_environment =
                                                          "AARNN_PHYSICS_TOKEN";
    public String world_id, profile_id, model_digest, io_digest;
    public double[] origin = {0, 64, 0};
  }
  private final Config config;
  private final HttpClient client =
      HttpClient.newBuilder()
          .connectTimeout(Duration.ofSeconds(2))
          .followRedirects(HttpClient.Redirect.NEVER)
          .build();
  private final String token;
  private final URI base;
  private CompletableFuture<JsonObject> pending;
  private JsonObject model, state;
  private boolean active = true;
  private long reset = -1, step = -1, publication = -1;
  public PhysicsSession(Config c) {
    config = c;
    token = System.getenv(c.token_environment);
    base = endpoint(c.endpoint);
    if (c.schema_version != 1 || c.origin == null || c.origin.length != 3 ||
        !java.util.Arrays.stream(c.origin).allMatch(Double::isFinite) ||
        c.world_id == null || c.profile_id == null || c.model_digest == null ||
        c.io_digest == null || token == null || token.length() < 24 ||
        token.length() > 4096)
      throw new IllegalArgumentException(
          "Explicit physics identity, origin and private token required");
  }
  public static URI endpoint(String text) {
    URI u = URI.create(text);
    if (!"http".equals(u.getScheme()) ||
        !List.of("127.0.0.1", "[::1]", "::1").contains(u.getHost()) ||
        u.getPort() < 1 || u.getPort() > 65535 || u.getUserInfo() != null ||
        u.getQuery() != null || u.getFragment() != null ||
        !(u.getPath().isEmpty() || u.getPath().equals("/")))
      throw new IllegalArgumentException(
          "Private physics service requires a loopback literal origin");
    return u;
  }
  private CompletableFuture<JsonObject> request(String path, JsonObject body,
                                                int bound) {
    var builder = HttpRequest.newBuilder(base.resolve(path))
                      .timeout(Duration.ofSeconds(2))
                      .header("Authorization", "Bearer " + token);
    if (body != null)
      builder.header("Content-Type", "application/json")
          .POST(HttpRequest.BodyPublishers.ofString(body.toString()));
    return client
        .sendAsync(builder.build(), ignored -> new Gateway.BoundedBody(bound))
        .thenApply(reply -> {
          if (reply.statusCode() != 200 && reply.statusCode() != 202)
            throw new IllegalStateException(
                "Physics service denied/unavailable");
          return JsonParser
              .parseString(new String(reply.body(),
                                      java.nio.charset.StandardCharsets.UTF_8))
              .getAsJsonObject();
        });
  }
  public Config config() { return config; }
  public JsonObject model() { return model; }
  public JsonObject state() { return state; }
  java.util.concurrent.CompletionStage<?> completion() { return pending; }
  public boolean pending() { return pending != null; }
  public boolean active() { return active; }
  public void fetch() {
    if (!active || pending != null)
      return;
    pending = (model == null
                   ? request("/model", null, 8 * 1024 * 1024)
                         .thenApply(m -> {
                           if (!config.model_digest.equals(
                                   m.get("digest").getAsString()) ||
                               !config.profile_id.equals(
                                   m.get("profile_id").getAsString()) ||
                               !config.io_digest.equals(m.getAsJsonObject("io")
                                                            .get("digest")
                                                            .getAsString()))
                             throw new IllegalArgumentException(
                                 "Pinned model/IO mismatch");
                           return m;
                         })
                         .thenCompose(m -> {
                           model = m;
                           return request("/state", null, 1024 * 1024);
                         })
                   : request("/state", null, 1024 * 1024));
  }
  public JsonObject poll() {
    if (pending == null || !pending.isDone())
      return null;
    var p = pending;
    pending = null;
    try {
      JsonObject next = p.join();
      // Command completion is not a pose; fetch again after its boundary.
      if (!next.has("poses"))
        return null;
      long epoch = next.get("reset_epoch").getAsLong(),
           s = next.get("step").getAsLong(),
           pub = next.get("publication").getAsLong();
      if (!config.world_id.equals(next.get("world_id").getAsString()) ||
          !config.profile_id.equals(next.get("profile_id").getAsString()) ||
          !config.model_digest.equals(next.get("model_digest").getAsString()) ||
          !config.io_digest.equals(next.get("io_digest").getAsString()) ||
          (reset >= 0 && epoch != reset) || s < step || pub < publication ||
          s < 0 || epoch < 0 || next.getAsJsonArray("poses").size() > 1024)
        throw new IllegalArgumentException(
            "Stale/reset/mismatched authoritative pose; explicit reattach " +
            "required");
      reset = epoch;
      step = s;
      publication = pub;
      state = next;
      return next;
    } catch (RuntimeException error) {
      active = false;
      emergencyStop();
      throw new IllegalStateException(
          "Physics view fault; inspect service before reattaching", error);
    }
  }
  public void replaceVoxels(List<double[]> voxels) {
    if (!active || state == null || pending != null || voxels.size() > 32)
      throw new IllegalStateException("Physics scene credit/capacity");
    var command = command("pause");
    var boxes = new Gson().toJsonTree(voxels);
    // Pause -> atomic scene replacement -> resume. A failed stage leaves the
    // service paused; no duplicate request is retried after an ambiguity.
    pending =
        request("/command", command, 65536)
            .thenCompose(ack -> {
              if (!active)
                throw new IllegalStateException("scene cancelled");
              var change = command("collision_set");
              change.add("voxels", boxes);
              change.addProperty("world_config_epoch",
                                 ack.get("world_config_epoch").getAsLong());
              return request("/command", change, 65536);
            })
            .thenCompose(ack -> {
              if (!active)
                throw new IllegalStateException("scene cancelled");
              return request("/command", command("resume"), 65536);
            });
  }
  private JsonObject command(String action) {
    var c = new JsonObject();
    c.addProperty("action", action);
    c.addProperty("model_digest", config.model_digest);
    c.addProperty("reset_epoch", reset);
    return c;
  }
  public void emergencyStop() {
    request("/stop", new JsonObject(), 65536).exceptionally(error -> null);
    if (reset >= 0)
      request("/command", command("pause"), 65536).exceptionally(error -> null);
  }
  public void close() {
    active = false;
    if (pending != null)
      pending.cancel(true);
    pending = null;
    emergencyStop(); /* bounded requests finish; do not block the server thread
                      */
  }
}
