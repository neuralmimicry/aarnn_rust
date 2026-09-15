package org.neuralmimicry.minecraft;

import java.util.Map;
import java.util.regex.Pattern;

/** IllustrativeSimulatorContentV1: port of webgl-world.js, verified against its fixtures.
 * Distances are habitat half extents; outputs are unitless [0,1] proxies, not calibrated biology. */
public final class Senses {
    private Senses() {}
    public record Pose(double x, double y, double heading, double[] actuators) {}
    public record Hit(double distance, double luminance) {}
    public record Motion(double drive, double turn) {}
    @FunctionalInterface public interface NativeRay { Hit cast(double[] p, double[] d, double range, Hit shared); }
    private static final Pattern PIXEL = Pattern.compile("r(\\d+)c(\\d+)");
    private static final Pattern PROX = Pattern.compile("prox_(\\d+)");
    public static double clamp(double v, double lo, double hi) { return Math.max(lo, Math.min(hi, v)); }
    private static boolean has(String s, String expression) { return Pattern.compile(expression).matcher(s).find(); }
    private static double at(double[] values, int i) { return i >= 0 && i < values.length ? values[i] : 0; }
    public static double field(Content.Habitat h, String kind, double[] p) {
        double value = 0;
        for (var o : h.objects()) {
            if (!o.cue().equals(kind) || o.radius() <= 0) continue;
            double d = 0;
            for (int i = 0; i < 3; i++) d += Math.pow(p[i] - o.position()[i], 2);
            value += o.strength() / (1 + 4 * d / (o.radius() * o.radius()));
        }
        return clamp(value, 0, 1);
    }
    public static Hit ray(Content.Habitat h, double[] origin, double[] direction, double range) {
        double best = range;
        double[] colour = h.substrate();
        objects: for (var o : h.objects()) {
            if (o.internal()||o.material().equals("water")) continue;
            double c = Math.cos(o.yaw()), s = Math.sin(o.yaw());
            double x = origin[0] - o.position()[0], y = origin[1] - o.position()[1];
            double[] p = {x*c+y*s, -x*s+y*c, origin[2]-o.position()[2]};
            double[] v = {direction[0]*c+direction[1]*s, -direction[0]*s+direction[1]*c, direction[2]};
            double t0 = 0, t1 = best;
            for (int a = 0; a < 3; a++) {
                double half = o.size()[a] * .5;
                if (Math.abs(v[a]) < 1e-9) {
                    if (Math.abs(p[a]) > half) continue objects;
                } else {
                    double u = (-half-p[a])/v[a], w = (half-p[a])/v[a];
                    t0 = Math.max(t0, Math.min(u,w)); t1 = Math.min(t1, Math.max(u,w));
                    if (t1 < t0) continue objects;
                }
            }
            if (t0 > 1e-5 && t0 < best) { best = t0; colour = o.colour(); }
        }
        return new Hit(best, colour[0]*.2126 + colour[1]*.7152 + colour[2]*.0722);
    }
    public static Motion motion(Content.Profile p, double[] outputs) {
        double left = 0, right = 0; int count = 0;
        if (p.kind().equals("worm")) {
            for (var group : p.muscle_channels()) for (int q = 0; q < 4; q++) {
                if (group[q] < 0) continue;
                if (q == 0 || q == 2) left += at(outputs, group[q]); else right += at(outputs, group[q]);
                count++;
            }
        } else if (p.kind().equals("fish") || p.kind().equals("hexapod")) {
            boolean fish = p.kind().equals("fish");
            for (int i = 0; i < (fish ? 16 : 18); i++) {
                if (fish ? i%2==0 : i<9) left += at(outputs,i); else right += at(outputs,i);
                count++;
            }
        } else {
            // Fly imported IDs lack a validated muscle/joint map; aggregate preview only.
            for (double value : outputs) { left += value*.5; right += value*.5; count++; }
        }
        return new Motion((left+right)/Math.max(1,count), (right-left)/Math.max(1,count));
    }
    public static double[] sample(Content.Profile profile, Content.Habitat habitat, Pose robot,
                                  Map<String, Double> previous) {
        return sample(profile, habitat, robot, previous, (p,d,r,h) -> h);
    }
    public static double[] sample(Content.Profile profile, Content.Habitat habitat, Pose robot,
                                  Map<String, Double> previous, NativeRay nativeRay) {
        double[] values = new double[profile.sensory()];
        double[] p = {robot.x, robot.y, profile.body_height()};
        class Probe {
            Hit ray(double angle, double elevation, double range) {
                double a = robot.heading + angle;
                double[] d = {Math.cos(a)*Math.cos(elevation), Math.sin(a)*Math.cos(elevation), Math.sin(elevation)};
                return nativeRay.cast(p,d,range,Senses.ray(habitat,p,d,range));
            }
            double paired(String field, double side) {
                double a = robot.heading + Math.PI/2;
                return Senses.field(habitat,field,new double[]{p[0]+Math.cos(a)*side*.04,p[1]+Math.sin(a)*side*.04,p[2]});
            }
        }
        var probe = new Probe();
        for (int i = 0; i < profile.sensor_names().size(); i++) {
            String name = profile.sensor_names().get(i);
            if (has(name,"accel|gyro")) values[i] = .5;
            else if (has(name,"coxa|femur|tibia")) values[i] = .5 + at(robot.actuators,i)*.35;
            else if (has(name,"\\.on\\.|\\.off\\.")) {
                var match = PIXEL.matcher(name); int r = 0, c = 0;
                if (match.find()) { r = Integer.parseInt(match.group(1)); c = Integer.parseInt(match.group(2)); }
                int cols = profile.kind().equals("fly") ? 12 : 1, rows = profile.kind().equals("fly") ? 8 : 1;
                int side = name.contains("right") ? -1 : 1;
                String key = (side>0?"l":"r") + r + "_" + c;
                double lum = probe.ray(side*.6+((c+.5)/cols-.5)*1.7,(.5-(r+.5)/rows)*1.15,2).luminance;
                double delta = lum-previous.getOrDefault(key,lum);
                boolean on = name.contains(".on.");
                values[i] = clamp((on ? delta : -delta)*4,0,1);
                if (!on) previous.put(key,lum);
            } else if (has(name,"eye_.*_(lum|grad)")) {
                int side = name.contains("right") ? -1 : 1; String key = "fishEye"+side;
                double lum = probe.ray(side*.8,0,2).luminance;
                values[i] = name.endsWith("_lum") ? lum : .5+clamp((lum-previous.getOrDefault(key,lum))*5,-1,1)*.5;
                if (name.endsWith("_grad")) previous.put(key,lum);
            } else if (has(name,"chem|taste|olfactory")) values[i] = probe.paired("chemical",has(name,"right|_r\\b")?-1:1);
            else if (name.contains("heat")) values[i] = probe.paired("heat",name.contains("right")?-1:1);
            else if (has(name,"light|eye_.*lum")) values[i] = probe.paired("light",name.contains("right")?-1:1);
            else if (name.contains("flow")) values[i] = field(habitat,"flow",p);
            else if (name.contains("depth")) {
                var water=habitat.objects().stream().filter(o->o.id().equals("water_volume")).findFirst();
                values[i]=water.map(o->clamp((o.position()[2]+o.size()[2]/2-p[2])/o.size()[2],0,1)).orElse(0.0);
            }
            else if (name.contains("pitch")) values[i] = .5;
            else if (name.contains("foot")) values[i] = 1;
            else if (has(name,"touch|prox|far|lateralline|ultrasonic")) {
                double angle = has(name,"rear|posterior") ? Math.PI : 0;
                if (has(name,"left|lateralline_l")) angle += Math.PI/2;
                if (has(name,"right|lateralline_r")) angle -= Math.PI/2;
                var prox = PROX.matcher(name); if (prox.find()) angle = Integer.parseInt(prox.group(1))*Math.PI*2/24;
                double distance = probe.ray(angle,0,.6).distance;
                values[i] = name.contains("touch") ? (distance<profile.body_length()*.55?1:0) : 1-distance/.6;
            }
        }
        if (profile.kind().equals("nao")) {
            values[0] = 1-probe.ray(.25,0,.6).distance/.6; values[1] = 1-probe.ray(-.25,0,.6).distance/.6;
            for (int i=2;i<8;i++) values[i]=.5;
            for (int j=23;j<49;j++) values[j]=.5+at(robot.actuators,j-23)*.35;
            for (int eye=0;eye<2;eye++) for (int px=0;px<48;px++) {
                String key="nao"+eye+"_"+px;
                double lum=probe.ray((eye==0?.15:-.15)+(px%8/7.0-.5)*1.3,(.5-(px/8)/5.0),2).luminance;
                double delta=lum-previous.getOrDefault(key,lum);
                values[58+eye*96+px]=clamp(delta*4,0,1); values[106+eye*96+px]=clamp(-delta*4,0,1);
                previous.put(key,lum);
            }
        }
        return values;
    }
}
