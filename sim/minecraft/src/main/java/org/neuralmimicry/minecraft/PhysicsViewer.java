package org.neuralmimicry.minecraft;

import com.google.gson.*;
import com.mojang.math.Transformation;
import java.nio.file.*;
import java.util.*;
import net.minecraft.core.BlockPos;
import net.minecraft.server.MinecraftServer;
import net.minecraft.server.level.ServerLevel;
import net.minecraft.world.entity.Display;
import net.minecraft.world.entity.EntityType;
import net.minecraft.world.level.block.Block;
import net.minecraft.world.level.block.Blocks;
import net.minecraft.world.phys.AABB;
import org.joml.Quaternionf;
import org.joml.Vector3f;

/**
 * Server-only authoritative-physics view. Displays have no local AI/gravity or
 * sensing. A metre is one block. Geometry is labelled bounding-box rendering.
 * Current mirror tier admits full collision blocks above a matching flat floor;
 * partial blocks/players/capacity exhaustion pause and disarm instead of
 * ghosting.
 */
public final class PhysicsViewer {
  private static final Map<MinecraftServer, PhysicsViewer> RUNS =
      new WeakHashMap<>();
  private static final org.slf4j.Logger LOG =
      org.slf4j.LoggerFactory.getLogger("aarnn-physics");
  private final PhysicsSession session;
  private final ServerLevel level;
  private final List<Display.BlockDisplay> displays = new ArrayList<>();
  private List<JsonObject> geoms;
  private String previousScene = "";
  private PhysicsViewer(MinecraftServer server, PhysicsSession.Config c) {
    session = new PhysicsSession(c);
    level = server.overworld();
  }
  public static void start(MinecraftServer server) {
    String path = System.getenv("AARNN_PHYSICS_CONFIG");
    if (path == null || path.isBlank())
      return;
    try {
      Path file = Path.of(path);
      if (Files.size(file) > 16384)
        throw new IllegalArgumentException("physics config bound");
      var config = new Gson().fromJson(Files.readString(file),
                                       PhysicsSession.Config.class);
      stop(server);
      var view = new PhysicsViewer(server, config);
      RUNS.put(server, view);
      view.session.fetch();
      LOG.info("Authoritative physics viewer requested: bounding-box "
               + "display, full-block mirrors, one metre per block; "
               + "partial-block/player scenes pause");
    } catch (Exception error) {
      LOG.error("Physics viewer disabled: verify explicit identities and "
                + "private configuration");
    }
  }
  public static void stop(MinecraftServer server) {
    var v = RUNS.remove(server);
    if (v != null) {
      v.session.close();
      v.displays.forEach(Display.BlockDisplay::discard);
    }
  }
  public static void tick(MinecraftServer server) {
    var v = RUNS.get(server);
    if (v == null)
      return;
    try {
      JsonObject next = v.session.poll();
      if (next != null) {
        if (v.geoms == null)
          v.construct();
        v.render(next);
        List<double[]> boxes = v.blocks(next);
        String scene = new Gson().toJson(boxes);
        if (!scene.equals(v.previousScene) && !v.session.pending()) {
          v.session.replaceVoxels(boxes);
          v.previousScene = scene;
        }
      }
      if (!v.session.pending())
        v.session.fetch();
    } catch (RuntimeException error) {
      LOG.error("Physics viewer stopped: {}", error.getMessage());
      stop(server);
    }
  }
  private static double[] vec(JsonObject o, String key, int n) {
    var a = o.getAsJsonArray(key);
    if (a.size() != n)
      throw new IllegalArgumentException("pose shape");
    double[] v = new double[n];
    for (int i = 0; i < n; i++) {
      v[i] = a.get(i).getAsDouble();
      if (!Double.isFinite(v[i]))
        throw new IllegalArgumentException("nonfinite pose");
    }
    return v;
  }
  private static Vector3f vector(double[] p) {
    return new Vector3f((float)p[0], (float)p[1], (float)p[2]);
  }
  private static Quaternionf quaternion(double[] q) {
    return new Quaternionf((float)q[1], (float)q[2], (float)q[3], (float)q[0]);
  }
  private static Vector3f game(Vector3f p) {
    return new Vector3f(p.x, p.z, -p.y);
  }
  private static Quaternionf game(Quaternionf q) {
    return new Quaternionf(q.x, q.z, -q.y, q.w);
  }
  private void construct() {
    JsonObject model = session.model();
    geoms = new ArrayList<>();
    for (var item : model.getAsJsonArray("geoms")) {
      var g = item.getAsJsonObject();
      if (g.get("type").getAsInt() != 0 && g.getAsJsonArray("rgba").get(3).getAsDouble() >= .5)
        geoms.add(g);
    }
    if (geoms.size() > 1024)
      throw new IllegalArgumentException("display budget exceeded");
    String tag = "aarnn-physics:" + session.config().world_id;
    double[] o = session.config().origin;
    level
        .getEntitiesOfClass(Display.BlockDisplay.class,
                            new AABB(o[0] - 32, o[1] - 32, o[2] - 32, o[0] + 32,
                                     o[1] + 32, o[2] + 32),
                            e -> PhysicsCompat.tagged(e, tag))
        .forEach(Display.BlockDisplay::discard);
    for (var g : geoms) {
      var d = PhysicsCompat.display(level);
      d.noPhysics = true;
      d.setNoGravity(true);
      d.setViewRange(4);
      d.addTag(tag);
      level.addFreshEntity(d);
      displays.add(d);
    }
  }
  private void render(JsonObject state) {
    Map<String, JsonObject> poses = new HashMap<>();
    for (var p : state.getAsJsonArray("poses")) {
      var b = p.getAsJsonObject();
      poses.put(b.get("name").getAsString(), b);
    }
    var bodies = session.model().getAsJsonArray("bodies");
    double[] origin = session.config().origin;
    for (int i = 0; i < geoms.size(); i++) {
      var g = geoms.get(i);
      int bodyId=g.get("body").getAsInt();
      Quaternionf q=new Quaternionf();
      Vector3f position=vector(vec(g,"position",3));
      if(bodyId>0) {
        String name=bodies.get(bodyId-1).getAsJsonObject().get("name").getAsString();
        var body=poses.get(name);
        if(body==null)throw new IllegalArgumentException("named physics pose missing");
        q=quaternion(vec(body,"quaternion_wxyz",4));
        position=q.transform(position).add(vector(vec(body,"position",3)));
      }
      q.mul(quaternion(vec(g,"quaternion_wxyz",4)));
      position = game(position);
      q = game(q);
      double[] h = vec(g, "half_size_m", 3);
      int type = g.get("type").getAsInt();
      if (type == 2)
        h = new double[] {h[0], h[0], h[0]};
      if (type == 3)
        h = new double[] {h[0], h[0], h[0] + h[1]};
      if (type == 5)
        h = new double[] {h[0], h[0], h[1]};
      Vector3f scale = game(vector(h)).absolute().mul(2);
      scale.max(new Vector3f(.00001f));
      var display = displays.get(i);
      if (i == 0) {
        display.setCustomName(net.minecraft.network.chat.Component.literal(
            session.config().profile_id + " · " +
            state.get("controller_mode").getAsString()));
        display.setCustomNameVisible(true);
      }

      display.setPos(origin[0] + position.x, origin[1] + position.y,
                     origin[2] + position.z);
      // Block-display origin is its corner; translate before applying the
      // composed orientation. This is rendering, never a physical grasp.
      Vector3f shift = q.transform(new Vector3f(scale).mul(-.5f));
      display.setTransformation(
          new Transformation(shift, q, scale, new Quaternionf()));
    }
  }
  private List<double[]> blocks(JsonObject state) {
    String rootName = session.model()
                          .getAsJsonArray("bodies")
                          .get(0)
                          .getAsJsonObject()
                          .get("name")
                          .getAsString();
    JsonObject root = null;
    for (var p : state.getAsJsonArray("poses"))
      if (p.getAsJsonObject().get("name").getAsString().equals(rootName))
        root = p.getAsJsonObject();
    if (root == null)
      throw new IllegalArgumentException("root pose absent");
    double[] p = vec(root, "position", 3), o = session.config().origin;
    int x = (int)Math.floor(o[0] + p[0]), y = (int)Math.floor(o[1] + p[2]),
        z = (int)Math.floor(o[2] - p[1]);
    if (!level
             .getEntitiesOfClass(
                 net.minecraft.world.entity.player.Player.class,
                 new AABB(x - 3, y - 3, z - 3, x + 3, y + 3, z + 3))
             .isEmpty())
      throw new IllegalStateException(
          "Player collision mirror unavailable; world paused");
    List<double[]> boxes = new ArrayList<>();
    for (int a = x - 2; a <= x + 2; a++)
      for (int b = y - 2; b <= y + 2; b++)
        for (int c = z - 2; c <= z + 2; c++) {
          if (b + 1 <= o[1])
            continue; // Matching infinite flat native floor is explicit.
          var pos = new BlockPos(a, b, c);
          if (!level.hasChunkAt(pos))
            throw new IllegalStateException("Mirror chunk unavailable");
          var block = level.getBlockState(pos);
          var shape = block.getCollisionShape(level, pos);
          if (shape.isEmpty())
            continue;
          if (!Block.isShapeFullBlock(shape))
            throw new IllegalStateException(
                "Partial-block collision unsupported; world paused");
          boxes.add(
              new double[] {a + .5 - o[0], -(c + .5 - o[2]), b + .5 - o[1]});
          if (boxes.size() > 32)
            throw new IllegalStateException(
                "Collision mirror capacity exceeded; world paused");
        }
    return boxes;
  }
}
