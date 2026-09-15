package org.neuralmimicry.minecraft;

import com.mojang.blaze3d.vertex.PoseStack;
import com.mojang.math.Axis;
import java.util.Arrays;
import java.util.LinkedHashMap;
import java.util.Map;
import net.fabricmc.api.ClientModInitializer;
import net.fabricmc.fabric.api.client.rendering.v1.EntityRendererRegistry;
import net.minecraft.client.renderer.MultiBufferSource;
import net.minecraft.client.renderer.RenderType;
import net.minecraft.client.renderer.culling.Frustum;
import net.minecraft.client.renderer.entity.EntityRenderer;
import net.minecraft.client.renderer.entity.EntityRendererProvider;
import net.minecraft.resources.ResourceLocation;

public final class AarnnClient implements ClientModInitializer {
    @Override public void onInitializeClient() {
        EntityRendererRegistry.register(AarnnMod.HABITAT,Renderer::new);
        EntityRendererRegistry.register(AarnnMod.ROBOT,Renderer::new);
    }
    private record Cached(String profile,boolean anatomy,double[] outputs,float[] mesh) {}
    private static final class Renderer extends EntityRenderer<SceneEntity> {
        private final Map<Integer,Cached> cache=new LinkedHashMap<>();
        Renderer(EntityRendererProvider.Context context) { super(context); }
        @Override public ResourceLocation getTextureLocation(SceneEntity e) {
            return ResourceLocation.withDefaultNamespace("textures/block/white_concrete.png");
        }
        @Override public boolean shouldRender(SceneEntity e,Frustum f,double x,double y,double z) {
            return e.shouldRenderAtSqrDistance(e.distanceToSqr(x,y,z)) && f.isVisible(e.getBoundingBox().inflate(4));
        }
        @Override public void render(SceneEntity e,float yaw,float partial,PoseStack stack,MultiBufferSource buffers,int light) {
            var p=e.profile(); double[] outputs=e.habitatEntity()?new double[0]:e.visualOutputs();
            var old=cache.get(e.getId());
            if(old==null || !old.profile.equals(p.id()) || old.anatomy!=e.anatomy() || !Arrays.equals(old.outputs,outputs)) {
                var objects=e.habitatEntity()?Content.habitat(p).objects():p.parts();
                if(e.habitatEntity())objects=objects.stream().filter(o->!o.material().equals("water")).toList();
                old=new Cached(p.id(),e.anatomy(),outputs,Meshes.build(objects,e.anatomy(),outputs,p));
                if(cache.size()>=32) cache.remove(cache.keySet().iterator().next());
                cache.put(e.getId(),old);
            }
            stack.pushPose();
            if(!e.habitatEntity()) stack.mulPose(Axis.YP.rotationDegrees(yaw));
            float scale=(float)(Content.HALF_EXTENT*(e.habitatEntity()?1:p.body_length()));
            stack.scale(scale,scale,scale);
            var vertex=buffers.getBuffer(RenderType.debugQuads());
            float[] mesh=old.mesh;
            for(int i=0;i<mesh.length;i+=18) for(int j:new int[]{0,1,2,2}) {
                int k=i+j*6;
                vertex.addVertex(stack.last(),mesh[k],mesh[k+2],-mesh[k+1]).setColor(mesh[k+3],mesh[k+4],mesh[k+5],1);
            }
            stack.popPose();
            super.render(e,yaw,partial,stack,buffers,light);
            if(!e.chatBubble().isEmpty()) {
                stack.pushPose();stack.translate(0,1,0);
                renderNameTag(e,net.minecraft.network.chat.Component.literal("NAO: "+e.chatBubble()),stack,buffers,light,partial);
                stack.popPose();
            }
        }
    }
}
