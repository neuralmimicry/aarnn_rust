package org.neuralmimicry.minecraft;

import com.mojang.blaze3d.vertex.PoseStack;
import com.mojang.blaze3d.vertex.VertexConsumer;
import com.mojang.math.Axis;
import java.util.Arrays;
import java.util.LinkedHashMap;
import java.util.Map;
import net.fabricmc.api.ClientModInitializer;
import net.fabricmc.fabric.api.client.rendering.v1.EntityRendererRegistry;
import net.fabricmc.fabric.api.client.rendering.v1.hud.HudElementRegistry;
import net.minecraft.client.Minecraft;
import net.minecraft.client.gui.GuiGraphicsExtractor;
import net.minecraft.client.renderer.SubmitNodeCollector;
import net.minecraft.client.renderer.culling.Frustum;
import net.minecraft.client.renderer.entity.EntityRenderer;
import net.minecraft.client.renderer.entity.EntityRendererProvider;
import net.minecraft.client.renderer.entity.state.EntityRenderState;
import net.minecraft.client.renderer.rendertype.RenderTypes;
import net.minecraft.client.renderer.state.level.CameraRenderState;
import net.minecraft.resources.Identifier;
import net.minecraft.world.phys.Vec3;

/** Minecraft 26.2 renderer, which uses extracted render state and submit nodes. */
public final class AarnnClient implements ClientModInitializer {
    private static final Identifier BODY_TEXTURE =
            Identifier.fromNamespaceAndPath("minecraft", "textures/block/white_concrete.png");

    @Override public void onInitializeClient() {
        EntityRendererRegistry.register(AarnnMod.HABITAT, Renderer::new);
        EntityRendererRegistry.register(AarnnMod.ROBOT, Renderer::new);
        HudElementRegistry.addLast(AarnnMod.id("status_hud"),
                (extractor, tickCounter) -> renderStatus(Minecraft.getInstance(), extractor));
    }

    private static void renderStatus(Minecraft client, GuiGraphicsExtractor drawContext) {
        if(client.level==null || client.player==null) return;
        SceneEntity hexapod=null;
        for(var entity:client.level.entitiesForRendering())
            if(entity instanceof SceneEntity scene && !scene.habitatEntity()
                    && scene.profile().id().equals("hexapod")) { hexapod=scene; break; }
        String state=state(hexapod);
        int x=8,y=8;
        // Keep connection state out of chat. This compact, text-free indicator is
        // deliberately modelled after a traffic light: only the current state is
        // bright while the other two lamps remain dim.
        drawContext.fill(x,y,x+18,y+30,0xb0000000);
        drawContext.fill(x+5,y+3,x+13,y+11,lamp(state,"FAULT",0xffef4444));
        drawContext.fill(x+5,y+11,x+13,y+19,lamp(state,"CONNECTING",0xffffcc33));
        drawContext.fill(x+5,y+19,x+13,y+27,lamp(state,"ACTIVE",0xff35c759));
    }

    private static int lamp(String state,String lit,int colour) {
        if (state.equals(lit)) return colour;
        return switch (lit) {
            case "FAULT" -> 0xff4a1717;
            case "CONNECTING" -> 0xff594d14;
            case "ACTIVE" -> 0xff16451f;
            default -> 0xff20242a;
        };
    }

    private static String state(SceneEntity scene) {
        if(scene==null) return "OFFLINE";
        String status=scene.status();
        if(status==null) return "OFFLINE";
        if(status.startsWith("active")) return "ACTIVE";
        if(status.startsWith("pending") || status.startsWith("armed")) return "CONNECTING";
        if(status.startsWith("fault")) return "FAULT";
        return "DISARMED";
    }

    private record Cached(String profile, boolean anatomy, double[] outputs, float[] mesh) {}

    private static final class RenderState extends EntityRenderState {
        private boolean habitat;
        private float yaw;
        private float scale;
        private float[] mesh = new float[0];
        private String chat = "";
    }

    private static final class Renderer extends EntityRenderer<SceneEntity, RenderState> {
        private final Map<Integer, Cached> cache = new LinkedHashMap<>();

        Renderer(EntityRendererProvider.Context context) { super(context); }

        @Override public RenderState createRenderState() { return new RenderState(); }

        @Override public void extractRenderState(SceneEntity entity, RenderState state, float partialTick) {
            super.extractRenderState(entity, state, partialTick);
            var profile = entity.profile();
            double[] outputs = entity.habitatEntity() ? new double[0] : entity.visualOutputs();
            var old = cache.get(entity.getId());
            if (old == null || !old.profile().equals(profile.id()) || old.anatomy() != entity.anatomy()
                    || !Arrays.equals(old.outputs(), outputs)) {
                var objects = entity.habitatEntity() ? Content.habitat(profile).objects() : profile.parts();
                if (entity.habitatEntity()) objects = objects.stream()
                        .filter(object -> !object.material().equals("water")).toList();
                old = new Cached(profile.id(), entity.anatomy(), outputs,
                        Meshes.build(objects, entity.anatomy(), outputs, profile));
                if (cache.size() >= 32) cache.remove(cache.keySet().iterator().next());
                cache.put(entity.getId(), old);
            }
            state.habitat = entity.habitatEntity();
            state.yaw = entity.getYRot(partialTick);
            state.scale = (float) (Content.HALF_EXTENT * (state.habitat ? 1 : profile.body_length()));
            state.mesh = old.mesh();
            state.chat = entity.chatBubble();
        }

        @Override public boolean shouldRender(SceneEntity entity, Frustum frustum, double x, double y, double z) {
            return entity.shouldRenderAtSqrDistance(entity.distanceToSqr(x, y, z))
                    && frustum.isVisible(entity.getBoundingBox().inflate(4));
        }

        @Override public void submit(RenderState state, PoseStack stack, SubmitNodeCollector collector,
                CameraRenderState camera) {
            stack.pushPose();
            if (!state.habitat) stack.mulPose(Axis.YP.rotationDegrees(state.yaw));
            stack.scale(state.scale, state.scale, state.scale);
            float[] mesh = state.mesh;
            // debugQuads is translucent and does not write depth. That makes the
            // robot appear transparent when clouds or terrain are rendered later.
            // Use an opaque, no-cull entity layer for the solid mesh instead.
            collector.submitCustomGeometry(stack, RenderTypes.armorCutoutNoCull(BODY_TEXTURE),
                    (pose, vertex) -> drawMesh(pose, vertex, mesh));
            stack.popPose();
            super.submit(state, stack, collector, camera);
            if (!state.chat.isEmpty()) {
                stack.pushPose();
                stack.translate(0, 1, 0);
                collector.submitNameTag(stack, Vec3.ZERO, state.lightCoords,
                        net.minecraft.network.chat.Component.literal("NAO: " + state.chat), false, 0, camera);
                stack.popPose();
            }
        }

        private static void drawMesh(PoseStack.Pose pose, VertexConsumer vertex, float[] mesh) {
            for (int i = 0; i < mesh.length; i += 18) {
                for (int j : new int[] {0, 1, 2, 2}) {
                    int k = i + j * 6;
                    vertex.addVertex(pose, mesh[k], mesh[k + 2], -mesh[k + 1])
                            .setColor(mesh[k + 3], mesh[k + 4], mesh[k + 5], 1)
                            .setUv(0, 0)
                            .setUv1(0, 0)
                            .setUv2(0xF000, 0x00F0)
                            .setNormal(pose, 0, 1, 0);
                }
            }
        }
    }
}
