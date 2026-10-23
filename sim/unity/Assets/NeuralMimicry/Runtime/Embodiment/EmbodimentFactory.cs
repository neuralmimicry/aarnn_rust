// Native rigid-body factory for the opt-in physical embodiment route.
// SI mechanics are generated once. No legacy scale/drive boosts are used.
using System;
using System.IO;
using System.Text;
using System.Collections.Generic;
using UnityEngine;
namespace NeuralMimicry {
[Serializable]
public sealed class EmbodimentScene {
  public int schema_version, dt_us, steps, reset_at = -1;
  public string output, diagnostic_joint, control;
  public double diagnostic_effort;
  public int diagnostic_steps;
  public EmbodimentEntry[] instances;
}
[Serializable]
public sealed class EmbodimentEntry {
  public string instance_id, model, graph, binding_json;
  public double[] offset_m;
}
[Serializable]
sealed class RigidGraph {
  public int schema_version, dt_us;
  public string profile_id, model_digest, io_digest, controller_transfer;
  public bool standard_projection;
  public GraphBody[] bodies;
  public GraphActuator[] actuators;
  public GraphGeom[] geoms;
  public GraphGeom[] world_geoms;
  public GraphJoint[] joints;
  public GraphTarget[] targets;
  public GraphMesh[] meshes;
  public GraphCoupling[] constraints;
  public GraphCollisionExclusion[] collision_exclusions;
  public GraphCamera[] cameras;
  public GraphOwner[] source_body_owners;
  public GraphIo io;
}
[Serializable]
sealed class GraphActuator {
  public string joint;
}
[Serializable]
sealed class GraphBody {
  public int id;
  public string name;
  public bool anchored;
  public double mass_kg;
  public double[] world_position, world_quaternion_wxyz, com_m,
      principal_inertia_kgm2, principal_axes_wxyz;
}
[Serializable]
sealed class GraphGeom {
  public int id, owner, render_mesh;
  public string name;
  public bool collision;
  public double[] position, quaternion_wxyz, rgba;
}
[Serializable]
sealed class GraphJoint {
  public int parent, child, type;
  public string name;
  public bool limited;
  public double[] parent_position, child_position, parent_quaternion_wxyz,
      child_quaternion_wxyz, range;
}
[Serializable]
sealed class GraphTarget {
  public string joint, kind;
  public double gain, gear, damping;
  public double[] bias, force_range;
}
[Serializable]
sealed class GraphMesh {
  public int id;
  public double[] vertices_xyz;
  public int[] triangles_ijk;
}
[Serializable]
sealed class GraphCoupling {
  public string joint1, joint2;
  public double ratio, offset, stiffness, damping;
}
[Serializable]
sealed class GraphCollisionExclusion {
  public int body1, body2;
}
[Serializable]
sealed class GraphCamera {
  public string name;
  public int owner;
  public double[] position, quaternion_wxyz;
}
[Serializable]
sealed class GraphOwner {
  public int id, owner;
  public string name;
}
[Serializable]
sealed class GraphIo {
  public GraphSensor[] sensors;
  public string[] output_names;
}
[Serializable]
sealed class GraphSensor {
  public string joint, kind;
}
sealed class EmbodimentColour : MonoBehaviour {
  public float luminance;
}
sealed class EmbodimentContact : MonoBehaviour {
  public List<Collider> own;
  public bool touching;
  void OnCollisionEnter(Collision c) {
    if (!own.Contains(c.collider) && c.contactCount > 0)
      touching = true;
  }
  void OnCollisionStay(Collision c) {
    if (!own.Contains(c.collider) && c.contactCount > 0)
      touching = true;
  }
}

// Basis S maps canonical X-forward/Y-left/Z-up to Unity Z-forward/X-right/Y-up.
// det(S)=-1: polar vectors use S, angular vectors use -S; q maps S R S^T.
static class EmbodimentBasis {
  public static Vector3
  Polar(double[] a) => new Vector3((float)-a[1], (float)a[2], (float)a[0]);
  public static Vector3 Polar(Vector3 a) => new Vector3(-a.y, a.z, a.x);
  public static Vector3 Inverse(Vector3 a) => new Vector3(a.z, -a.x, a.y);
  public static Quaternion Rotation(double[] q) => new Quaternion((float)q[2],
                                                                  (float)-q[3],
                                                                  (float)-q[1],
                                                                  (float)q[0]);
}
sealed class NativeEmbodiment : IDisposable {
  readonly RigidGraph graph;
  readonly EmbodimentEntry entry;
  readonly GameObject root;
  readonly Dictionary<int, Rigidbody> bodies = new Dictionary<int, Rigidbody>();
  readonly Dictionary<string, GraphJoint> joints =
      new Dictionary<string, GraphJoint>();
  readonly Dictionary<string, int> actuators = new Dictionary<string, int>();
  readonly Dictionary<string, double> continuous =
                                          new Dictionary<string, double>(),
                                      lastAngles =
                                          new Dictionary<string, double>();
  readonly Dictionary<string, double> positions =
                                          new Dictionary<string, double>(),
                                      velocities =
                                          new Dictionary<string, double>();
  readonly Dictionary<int, List<Collider>> colliders =
      new Dictionary<int, List<Collider>>();
  readonly Dictionary<int, Mesh> meshes = new Dictionary<int, Mesh>();
  readonly List<Collider> own = new List<Collider>();
  readonly double[] controls, raw, sensors;
  readonly byte[] motorMask;
  readonly byte[] statusBuffer = new byte[65536];
  ulong handle;
  double energy = 20000;
  Vector3 previousHeadVelocity;
  bool crow, done, npcActive;
  public int ActiveSteps { get; private set; }
  public NativeEmbodiment(EmbodimentEntry e, string control) {
    entry = e;
    var p = Encoding.UTF8.GetBytes(e.model);
    var g = Encoding.UTF8.GetBytes(e.graph);
    Check(EmbodimentNative.nm_embodiment_verify_graph(p, (uint)p.Length, g,
                                                      (uint)g.Length));
    graph = JsonUtility.FromJson<RigidGraph>(File.ReadAllText(e.graph));
    if (graph.schema_version != 1 || graph.dt_us != 2000)
      throw new InvalidDataException("rigid graph version/clock");
    if (!graph.standard_projection &&
        !String.IsNullOrEmpty(e.binding_json))
      throw new InvalidDataException("uncalibrated legacy transfer");
    crow = graph.profile_id == "crow_corvus_corone";
    int count = graph.actuators.Length;
    motorMask = new byte[count];
    controls = new double[count + 2];
    raw = new double[count * 2 + 11 + (crow ? 235 : 0)];
    sensors = new double[graph.io.sensors.Length];
    root = new GameObject(e.instance_id);
    try {
      foreach (var m in graph.meshes) {
        var v = new Vector3[m.vertices_xyz.Length / 3];
        for (int i = 0; i < v.Length; i++)
          v[i] = EmbodimentBasis.Polar(new[] { m.vertices_xyz[i * 3],
                                               m.vertices_xyz[i * 3 + 1],
                                               m.vertices_xyz[i * 3 + 2] });
        var t = (int[])m.triangles_ijk.Clone();
        for (int i = 0; i < t.Length; i += 3) {
          int b = t[i + 1];
          t[i + 1] = t[i + 2];
          t[i + 2] = b;
        }
        var mesh =
            new Mesh { indexFormat = UnityEngine.Rendering.IndexFormat.UInt32,
                       vertices = v, triangles = t };
        mesh.RecalculateNormals();
        mesh.RecalculateBounds();
        meshes.Add(m.id, mesh);
      }
      var offset = EmbodimentBasis.Polar(e.offset_m);
      // Static scene geometry has no Rigidbody and contributes no robot mass.
      // Body drop-height diagnostics must not lift their own support floor.
      var sceneryOffset=new Vector3(offset.x,0,offset.z);
      foreach(var g0 in graph.world_geoms ?? Array.Empty<GraphGeom>()) {
        var go=new GameObject(g0.name);
        go.transform.SetParent(root.transform,false);
        go.transform.SetPositionAndRotation(sceneryOffset+EmbodimentBasis.Polar(g0.position),EmbodimentBasis.Rotation(g0.quaternion_wxyz));
        go.AddComponent<MeshFilter>().sharedMesh=meshes[g0.render_mesh];
        var material=new Material(Shader.Find("Standard"));
        material.color=new Color((float)g0.rgba[0],(float)g0.rgba[1],(float)g0.rgba[2],(float)g0.rgba[3]);
        if(g0.rgba[3]<.5) {
          material.SetFloat("_Mode",3);material.SetInt("_SrcBlend",(int)UnityEngine.Rendering.BlendMode.SrcAlpha);
          material.SetInt("_DstBlend",(int)UnityEngine.Rendering.BlendMode.OneMinusSrcAlpha);material.SetInt("_ZWrite",0);
          material.EnableKeyword("_ALPHABLEND_ON");material.renderQueue=3000;
        }
        go.AddComponent<MeshRenderer>().sharedMaterial=material;
        go.AddComponent<EmbodimentColour>().luminance=(float)(.2126*g0.rgba[0]+.7152*g0.rgba[1]+.0722*g0.rgba[2]);
        if(g0.collision) {
          var collider=go.AddComponent<MeshCollider>();collider.sharedMesh=meshes[g0.render_mesh];
          collider.material=new PhysicMaterial {dynamicFriction=.8f,staticFriction=.8f,bounciness=0};
        }
      }
      foreach (var b in graph.bodies) {
        var go = new GameObject(b.name);
        go.transform.SetParent(root.transform);
        go.transform.SetPositionAndRotation(
            offset + EmbodimentBasis.Polar(b.world_position),
            EmbodimentBasis.Rotation(b.world_quaternion_wxyz));
        var rb = go.AddComponent<Rigidbody>();
        rb.isKinematic = b.anchored;
        rb.useGravity = true;
        rb.mass = (float)b.mass_kg;
        rb.drag = 0;
        rb.angularDrag = 0;
        rb.maxAngularVelocity = 1000;
        rb.solverIterations = 32;
        rb.solverVelocityIterations = 16;
        go.AddComponent<EmbodimentContact>().own = own;
        bodies.Add(b.id, rb);
        colliders.Add(b.id, new List<Collider>());
      }
      foreach (var g0 in graph.geoms) {
        var go = new GameObject(g0.name);
        go.transform.SetParent(bodies[g0.owner].transform, false);
        go.transform.localPosition = EmbodimentBasis.Polar(g0.position);
        go.transform.localRotation =
            EmbodimentBasis.Rotation(g0.quaternion_wxyz);
        go.AddComponent<MeshFilter>().sharedMesh = meshes[g0.render_mesh];
        var mr = go.AddComponent<MeshRenderer>();
        var material = new Material(Shader.Find("Standard"));
        material.color = new Color((float)g0.rgba[0], (float)g0.rgba[1],
                                   (float)g0.rgba[2], (float)g0.rgba[3]);
        mr.sharedMaterial = material;
        go.AddComponent<EmbodimentColour>().luminance =
            (float)(.2126 * g0.rgba[0] + .7152 * g0.rgba[1] +
                    .0722 * g0.rgba[2]);
        if (g0.collision) {
          var c = go.AddComponent<MeshCollider>();
          c.sharedMesh = meshes[g0.render_mesh];
          c.convex = true;
          c.material =
              new PhysicMaterial { dynamicFriction = .8f, staticFriction = .8f,
                                   bounciness = 0 };
          colliders[g0.owner].Add(c);
          own.Add(c);
        }
      }
      foreach (var b in graph.bodies) {
        var rb = bodies[b.id];
        rb.centerOfMass = EmbodimentBasis.Polar(b.com_m);
        rb.inertiaTensor = new Vector3((float)b.principal_inertia_kgm2[1],
                                       (float)b.principal_inertia_kgm2[2],
                                       (float)b.principal_inertia_kgm2[0]);
        rb.inertiaTensorRotation =
            EmbodimentBasis.Rotation(b.principal_axes_wxyz);
      }
      foreach (var j in graph.joints) {
        if (j.type != 2 && j.type != 3)
          throw new InvalidDataException(
              "joint needs explicit native adapter: " + j.name);
        joints.Add(j.name, j);
        var child = bodies[j.child];
        var parent = bodies[j.parent];
        var c = child.gameObject.AddComponent<ConfigurableJoint>();
        c.connectedBody = parent;
        c.autoConfigureConnectedAnchor = false;
        c.anchor = EmbodimentBasis.Polar(j.child_position);
        c.connectedAnchor = EmbodimentBasis.Polar(j.parent_position);
        var cr = EmbodimentBasis.Rotation(j.child_quaternion_wxyz);
        c.axis = cr * Vector3.forward;
        c.secondaryAxis = cr * Vector3.left;
        c.xMotion = c.yMotion = c.zMotion = ConfigurableJointMotion.Locked;
        c.angularXMotion = c.angularYMotion = c.angularZMotion =
            ConfigurableJointMotion.Locked;
        if (j.type == 3) {
          c.angularXMotion = j.limited ? ConfigurableJointMotion.Limited
                                       : ConfigurableJointMotion.Free;
          if (j.limited) {
            c.lowAngularXLimit =
                new SoftJointLimit { limit =
                                         (float)(-j.range[1] * Mathf.Rad2Deg) };
            c.highAngularXLimit =
                new SoftJointLimit { limit =
                                         (float)(-j.range[0] * Mathf.Rad2Deg) };
          }
        } else {
          c.xMotion = j.limited ? ConfigurableJointMotion.Limited
                                : ConfigurableJointMotion.Free;
          if (j.limited) {
            if (Math.Abs(j.range[0] + j.range[1]) > 1e-8)
              throw new InvalidDataException(
                  "asymmetric linear limit needs shifted native frame");
            c.linearLimit = new SoftJointLimit { limit = (float)j.range[1] };
          }
        }
        foreach (var a in colliders[j.child])
          foreach (var b in colliders[j.parent])
            Physics.IgnoreCollision(a, b);
        positions[j.name] = 0;
        velocities[j.name] = 0;
      }
      foreach (var pair in graph.collision_exclusions)
        foreach (var a in colliders[pair.body1])
          foreach (var b in colliders[pair.body2])
              Physics.IgnoreCollision(a, b);
      for (int i = 0; i < graph.actuators.Length; i++)
        actuators.Add(graph.actuators[i].joint, i);
      var binding = Encoding.UTF8.GetBytes(e.binding_json ?? "");
      Check(EmbodimentNative.nm_reference_open(
          p, (uint)p.Length, binding, (uint)binding.Length, out handle));
      uint mode = control == "auto"     ? 0u
                  : control == "npc"    ? 1u
                  : control == "silent" ? 2u
                  : control == "aarnn"
                      ? 3u
                      : throw new InvalidDataException("control mode");
      Check(EmbodimentNative.nm_control_mode(handle, mode));

    } catch {
      Dispose();
      throw;
    }
  }
  public static void Check(int code) {
    if (code < 0) {
      var b = new byte[4096];
      int n = EmbodimentNative.nm_embodiment_error(b, 4096);
      throw new InvalidOperationException(
          n < 0 ? "embodiment ABI failed" : Encoding.UTF8.GetString(b, 0, n));
    }
  }
  Vector3 Axis(GraphJoint j) =>
      bodies[j.parent].rotation *
      (EmbodimentBasis.Rotation(j.parent_quaternion_wxyz) * Vector3.forward) *
      (j.type == 3 ? -1 : 1);
  double Position(GraphJoint j) {
    var p = bodies[j.parent];
    var c = bodies[j.child];
    if (j.type == 2)
      return Vector3.Dot(
          c.transform.TransformPoint(EmbodimentBasis.Polar(j.child_position)) -
              p.transform.TransformPoint(
                  EmbodimentBasis.Polar(j.parent_position)),
          Axis(j));
    var d = Quaternion.Inverse(p.rotation * EmbodimentBasis.Rotation(
                                                j.parent_quaternion_wxyz)) *
            (c.rotation * EmbodimentBasis.Rotation(j.child_quaternion_wxyz));
    double angle = -2 * Math.Atan2(d.z, d.w);
    if (!lastAngles.ContainsKey(j.name)) {
      lastAngles[j.name] = angle;
      continuous[j.name] = angle;
    } else {
      double delta =
          Math.IEEERemainder(angle - lastAngles[j.name], 2 * Math.PI);
      continuous[j.name] += delta;
      lastAngles[j.name] = angle;
    }
    return continuous[j.name];
  }
  double Velocity(GraphJoint j) {
    var p = bodies[j.parent];
    var c = bodies[j.child];
    return j.type == 3
               ? Vector3.Dot(c.angularVelocity - p.angularVelocity, Axis(j))
               : Vector3.Dot(c.GetPointVelocity(c.transform.TransformPoint(
                                 EmbodimentBasis.Polar(j.child_position))) -
                                 p.GetPointVelocity(p.transform.TransformPoint(
                                     EmbodimentBasis.Polar(j.parent_position))),
                             Axis(j));
  }
  void Effort(GraphJoint j, double effort) {
    if (Double.IsNaN(effort) || Double.IsInfinity(effort))
      throw new InvalidOperationException("nonfinite effort");
    var axis = Axis(j) * (float)effort;
    var p = bodies[j.parent];
    var c = bodies[j.child];
    if (j.type == 3) {
      c.AddTorque(axis, ForceMode.Force);
      p.AddTorque(-axis, ForceMode.Force);
    } else {
      c.AddForceAtPosition(axis, c.transform.TransformPoint(
                                     EmbodimentBasis.Polar(j.child_position)));
      p.AddForceAtPosition(
          -axis,
          p.transform.TransformPoint(EmbodimentBasis.Polar(j.parent_position)));
    }
  }
  public void Prepare(EmbodimentScene scene, int step) {
    foreach (var rb in bodies.Values)
      rb.GetComponent<EmbodimentContact>().touching = false;
    double power = 0;
    var efforts = new Dictionary<string, double>();
    foreach (var t in graph.targets) {
      var j = joints[t.joint];
      double q = Position(j), v = Velocity(j);
      positions[t.joint] = q;
      velocities[t.joint] = v;
      double effort = -t.damping * v;
      if (!done && actuators.TryGetValue(t.joint, out int i) &&
          motorMask[i] != 0) {
        double drive =
            t.gain * controls[i] + t.bias[0] + t.bias[1] * q + t.bias[2] * v;
        drive = Math.Max(t.force_range[0], Math.Min(t.force_range[1], drive)) *
                t.gear;
        effort += drive;
        power += Math.Abs(drive * v);
      }
      efforts[t.joint] = effort;
    }
    double scale = crow ? Math.Min(1, 25 / Math.Max(25, power)) *
                              Math.Min(1, energy / 2000)
                        : 1;
    foreach (var t in graph.targets) {
      double passive = -t.damping * velocities[t.joint];
      Effort(joints[t.joint], passive + (efforts[t.joint] - passive) * scale);
    }
    if (crow) {
      energy = Math.Max(
          0, energy -
                 (power * scale / (.20 + .05 * controls[controls.Length - 1]) +
                  1 + 2 * controls[controls.Length - 2]) *
                     .002);
      if (energy == 0)
        Disarm();
    }
    foreach (var c in graph.constraints) {
      double lambda =
          c.stiffness *
              (positions[c.joint1] - c.ratio * positions[c.joint2] - c.offset) +
          c.damping * (velocities[c.joint1] - c.ratio * velocities[c.joint2]);
      Effort(joints[c.joint1], -lambda);
      Effort(joints[c.joint2], c.ratio * lambda);
    }
    if (!String.IsNullOrEmpty(scene.diagnostic_joint) &&
        step < scene.diagnostic_steps)
      Effort(joints[scene.diagnostic_joint], scene.diagnostic_effort);
    if (crow)
      foreach (var panel in new[] {
                 ("left_wing_wrist_twist_link", new Vector3(0, .06f, 0), .045),
                 ("right_wing_wrist_twist_link", new Vector3(0, -.06f, 0),
                  .045),
                 ("tail_fan_link", new Vector3(-.03f, 0, 0), .008)
               }) {
        var rb = Body(panel.Item1);
        var point =
            rb.transform.TransformPoint(EmbodimentBasis.Polar(panel.Item2));
        var velocity = EmbodimentBasis.Inverse(rb.GetPointVelocity(point));
        var r = rb.rotation;
        // canonical local-to-world R = S^T R_engine S; row-major packing.
        var x =
            EmbodimentBasis.Inverse(r * EmbodimentBasis.Polar(Vector3.right));
        var y = EmbodimentBasis.Inverse(r * EmbodimentBasis.Polar(Vector3.up));
        var z =
            EmbodimentBasis.Inverse(r * EmbodimentBasis.Polar(Vector3.forward));
        var input = new double[] { velocity.x, velocity.y, velocity.z, x.x, y.x,
                                   z.x,        x.y,        y.y,        z.y, x.z,
                                   y.z,        z.z,        panel.Item3 };
        var output = new double[3];
        Check(EmbodimentNative.nm_panel_forces(input, 1, output, 3));
        rb.AddForceAtPosition(EmbodimentBasis.Polar(output), point);
      }
    foreach (var t in graph.targets) {
      if (Math.Abs((efforts[t.joint] + t.damping * velocities[t.joint]) *
                   scale) > 1e-9) {
        if (!npcActive)
          ActiveSteps++;
        break;
      }
    }
  }
  Rigidbody Body(string name) {
    foreach (var o in graph.source_body_owners)
      if (o.name == name)
        return bodies[o.owner];
    throw new InvalidDataException("body absent " + name);
  }
  float Ray(Vector3 point, Vector3 direction, bool light) {
    float best = 3, value = light ? .5f : 3;
    foreach (var hit in Physics.RaycastAll(point, direction.normalized, 3)) {
      if (own.Contains(hit.collider) || hit.distance >= best)
        continue;
      best = hit.distance;
      if (light) {
        var c = hit.collider.GetComponent<EmbodimentColour>();
        value = c == null ? .5f : c.luminance;
      } else
        value = best;
    }
    return value;
  }
  public void Capture(int step) {
    if (done)
      return;
    if (!graph.standard_projection) {
      ControlStep(step);
      return;
    }
    foreach (var a in actuators) {
      raw[2 * a.Value] = Position(joints[a.Key]);
      raw[2 * a.Value + 1] = Velocity(joints[a.Key]);
    }
    int n = actuators.Count * 2;
    var rootBody = bodies[graph.bodies[0].id];
    var head = crow ? Body("neck_roll_link") : rootBody;
    var gyro = -EmbodimentBasis.Inverse(Quaternion.Inverse(head.rotation) *
                                        head.angularVelocity);
    raw[n] = gyro.x;
    raw[n + 1] = gyro.y;
    raw[n + 2] = gyro.z;
    for (int i = 0; i < 8; i++) {
      double a = i * 2 * Math.PI / 8;
      raw[n + 3 + i] = Ray(
          rootBody.position +
              rootBody.rotation * EmbodimentBasis.Polar(new Vector3(0, 0, .1f)),
          rootBody.rotation * EmbodimentBasis.Polar(new Vector3(
                                  (float)Math.Cos(a), (float)Math.Sin(a), 0)),
          false);
    }
    if (crow) {
      int k = n + 11;
      foreach (string side in new[] { "left", "right" }) {
        var camera = Array.Find(graph.cameras, c => c.name == "retina_" + side);
        var rb = bodies[camera.owner];
        var p =
            rb.transform.TransformPoint(EmbodimentBasis.Polar(camera.position));
        var r = rb.rotation * EmbodimentBasis.Rotation(camera.quaternion_wxyz);
        for (int y = 0; y < 8; y++)
          for (int x = 0; x < 12; x++)
            raw[k++] = Ray(p,
                           r * EmbodimentBasis.Polar(new Vector3(
                                   (x - 5.5f) / 8, (3.5f - y) / 8, -1)),
                           true);
      }
      var acceleration = EmbodimentBasis.Inverse(
          Quaternion.Inverse(head.rotation) *
          ((head.velocity - previousHeadVelocity) / .002f - Physics.gravity));
      previousHeadVelocity = head.velocity;
      raw[k++] = acceleration.x;
      raw[k++] = acceleration.y;
      raw[k++] = acceleration.z;
      var gravity = EmbodimentBasis.Inverse(Quaternion.Inverse(head.rotation) *
                                            Physics.gravity.normalized);
      raw[k++] = gravity.x;
      raw[k++] = gravity.y;
      raw[k++] = gravity.z;
      foreach (string body in new[] {
                 "beak_gape_link", "beak_gape_link",
                 "left_leg_front_toes_curl_link",
                 "left_leg_front_toes_curl_link",
                 "left_leg_front_toes_curl_link", "left_leg_hallux_curl_link",
                 "right_leg_front_toes_curl_link",
                 "right_leg_front_toes_curl_link",
                 "right_leg_front_toes_curl_link", "right_leg_hallux_curl_link",
                 "left_wing_wrist_twist_link", "right_wing_wrist_twist_link"
               })
        raw[k++] = Touch(Body(body));
      for (int i = 0; i < 16; i++)
        raw[k++] = i % 8 == 2 ? controls[controls.Length - 2] : 0;
      var air = EmbodimentBasis.Inverse(Quaternion.Inverse(rootBody.rotation) *
                                        rootBody.velocity);
      raw[k++] = air.x;
      raw[k++] = air.y;
      raw[k++] = air.z;
      raw[k++] = 1 - energy / 20000;
      raw[k++] = 0;
      raw[k++] = 1 - energy / 20000;
      raw[k++] = energy / 20000;
      raw[k++] = 0;
      raw[k++] = 0;
    }
    Check(EmbodimentNative.nm_reference_project(handle, raw, (uint)raw.Length,
                                                sensors, (uint)sensors.Length));
    ControlStep(step);
  }
  void ControlStep(int step) {
    int code = EmbodimentNative.nm_reference_step(
        handle, (ulong)step, sensors, (uint)sensors.Length, controls,
        (uint)controls.Length);
    Check(code);
    npcActive = code == 2;
    Check(EmbodimentNative.nm_reference_motor_mask(handle, motorMask,
                                                   (uint)motorMask.Length));
  }
  float Touch(Rigidbody rb) => rb.GetComponent<EmbodimentContact>().touching
                                   ? 1
                                   : 0;
  public void Disarm() {
    Array.Clear(controls, 0, controls.Length);
    Array.Clear(motorMask, 0, motorMask.Length);
    if (handle != 0)
      Check(EmbodimentNative.nm_reference_disarm(handle));
  }
  public string Evidence() {
    int n = EmbodimentNative.nm_reference_status(handle, statusBuffer,
                                                 (uint)statusBuffer.Length);
    Check(n);
    return Encoding.UTF8.GetString(statusBuffer, 0, n);
  }
  public void Dispose() {
    if (handle != 0) {
      EmbodimentNative.nm_reference_disarm(handle);
      EmbodimentNative.nm_reference_close(handle);
      handle = 0;
    }
    foreach (var rb in bodies.Values) {
      rb.detectCollisions = false;
      rb.isKinematic = true;
    }
    if (root != null)
      UnityEngine.Object.Destroy(root);
    foreach (var m in meshes.Values)
      UnityEngine.Object.Destroy(m);
  }
}
public sealed class EmbodimentFactory : MonoBehaviour {
  EmbodimentScene scene;
  readonly List<NativeEmbodiment> robots = new List<NativeEmbodiment>();
  int step, iterations;
  bool failed;
  static void Exit(int code) {
#if UNITY_EDITOR
    if (Application.isBatchMode)
      UnityEditor.EditorApplication.Exit(code);
    else
      UnityEditor.EditorApplication.isPlaying = false;
#else
    Application.Quit(code);
#endif
  }
  [RuntimeInitializeOnLoadMethod(RuntimeInitializeLoadType.AfterSceneLoad)]
  static void Boot() {
    string path = Environment.GetEnvironmentVariable("NM_EMBODIMENT_SCENE");
    if (String.IsNullOrEmpty(path))
      return;
    var go = new GameObject("Native embodiment physics authority");
    var owner = go.AddComponent<EmbodimentFactory>();
    try {
      owner.scene =
          JsonUtility.FromJson<EmbodimentScene>(File.ReadAllText(path));
      owner.Initialise();
    } catch (Exception e) {
      Debug.LogException(e);
      owner.failed = true;
      Exit(1);
    }
  }
  void Initialise() {
    if (EmbodimentNative.nm_embodiment_version() != 1 ||
        scene.schema_version != 1 || scene.dt_us != 2000 ||
        scene.instances.Length < 1 || scene.instances.Length > 16 ||
        scene.steps < 1)
      throw new InvalidDataException("scene/ABI contract");
    Time.fixedDeltaTime = .002f;
    Physics.autoSimulation = false;
    Physics.gravity = new Vector3(0, -9.81f, 0);
    foreach (var e in scene.instances) {
      if (!String.IsNullOrEmpty(scene.diagnostic_joint) &&
          !String.IsNullOrEmpty(e.binding_json))
        throw new InvalidDataException("diagnostic in neural mode");
      robots.Add(new NativeEmbodiment(e, scene.control ?? "auto"));
    }
  }
  void Update() {
    if (failed)
      return;
    try {
      if (iterations == scene.reset_at) {
        foreach (var r in robots)
          r.Dispose();
        robots.Clear();
        foreach (var e in scene.instances) {
          e.binding_json = null;
          robots.Add(new NativeEmbodiment(e, scene.control ?? "auto"));
        }
        step = 0;
      }
      foreach (var r in robots)
        r.Prepare(scene, step);
      Physics.Simulate(.002f);
      step++;
      iterations++;
      foreach (var r in robots)
        r.Capture(step);
      if (iterations >= scene.steps) {
        var reports = new List<string>();
        foreach (var r in robots) {
          r.Disarm();
          reports.Add(r.Evidence());
        }
        Directory.CreateDirectory(Path.GetDirectoryName(scene.output));
        File.WriteAllText(
            scene.output,
            "{\"schema_version\":1,\"scope\":\"native mechanics/reference " +
                "ABI only; calibration and trained tasks open\",\"steps\":" +
                step + ",\"instances\":[" + String.Join(",", reports) + "]}");
        failed = true;
        Exit(0);
      }
    } catch (Exception e) {
      failed = true;
      foreach (var r in robots)
        r.Disarm();
      Debug.LogException(e);
      Exit(1);
    }
  }
  void OnDestroy() {
    foreach (var r in robots)
      r.Dispose();
    Physics.autoSimulation = true;
  }
}
}
