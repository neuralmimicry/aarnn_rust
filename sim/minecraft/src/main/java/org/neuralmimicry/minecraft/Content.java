package org.neuralmimicry.minecraft;

import com.google.gson.Gson;
import java.io.InputStreamReader;
import java.nio.charset.StandardCharsets;
import java.util.List;
import java.util.Objects;

/** Generated catalogue only; no neural state or model-specific integrator lives here. */
public final class Content {
    public record Part(String id, String shape, double[] position, double[] size,
                       double[] colour, double yaw, boolean collision, String material,
                       String cue, double radius, double strength, String anchor, boolean internal) {}
    public record Habitat(String id, double[] substrate, double half_extent_m, List<Part> objects) {}
    public record Profile(String id, String kind, String habitat, double body_length,
                          double body_height, int sensory, int output, List<String> sensor_names,
                          List<String> output_names, int[][] muscle_channels, List<Part> parts) {}
    public record Catalogue(int schema_version, String digest, List<Profile> profiles, List<Habitat> habitats) {}
    public static final Catalogue DATA = load();
    public static final double HALF_EXTENT = 16.0;
    private Content() {}
    private static Catalogue load() {
        try (var stream = Objects.requireNonNull(Content.class.getResourceAsStream("/aarnn/content.generated.json"));
             var reader = new InputStreamReader(stream, StandardCharsets.UTF_8)) {
            Catalogue c = new Gson().fromJson(reader, Catalogue.class);
            if (c.schema_version != 1 || c.profiles.size() != 6 || c.habitats.size() != 5)
                throw new IllegalArgumentException("Unsupported simulation catalogue");
            for (Profile p : c.profiles) {
                if (p.sensory < 1 || p.sensory > 8192 || p.output < 1 || p.output > 512 || p.parts.size() > 512)
                    throw new IllegalArgumentException("Unbounded robot profile");
            }
            return c;
        } catch (Exception error) { throw new ExceptionInInitializerError(error); }
    }
    public static Profile profile(String id) {
        return DATA.profiles.stream().filter(p -> p.id.equals(id)).findFirst()
            .orElseThrow(() -> new IllegalArgumentException("Unknown profile: " + id));
    }
    public static Habitat habitat(Profile p) {
        return DATA.habitats.stream().filter(h -> h.id.equals(p.habitat)).findFirst().orElseThrow();
    }
    /** Wall-clock transport budget only; never changes Rust's negotiated biological step. */
    public static int frameTimeoutMillis(Profile p) {
        // A hexapod frame is a three-hop AER exchange when the dual-brain
        // launcher is active. Leave enough wall-clock budget for the first
        // growing runner step and the return path before failing closed.
        return p.id().equals("hexapod")?10000:p.kind().equals("fly")?60000:p.kind().equals("fish")?10000:2500;
    }
    /** Right-handed catalogue (X forward, Y left, Z up) -> Minecraft (X east, Y up, Z south). */
    public static double[] toMinecraft(double[] v, double scale) {
        return new double[]{v[0] * scale, v[2] * scale, -v[1] * scale};
    }
}
