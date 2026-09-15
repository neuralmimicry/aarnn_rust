package org.neuralmimicry.minecraft;

import java.nio.file.Files;
import java.nio.file.Path;
import java.util.concurrent.CompletableFuture;
import net.fabricmc.api.ClientModInitializer;
import net.fabricmc.fabric.api.client.event.lifecycle.v1.ClientTickEvents;
import net.minecraft.client.Screenshot;
import net.minecraft.world.level.GameType;

/** Development-only actual framebuffer capture; excluded from both distributable JARs. */
public final class VisualProbe implements ClientModInitializer {
    private int phase,settled,totalTicks;
    private CompletableFuture<Void> positioning;
    private volatile boolean capturing;
    @Override public void onInitializeClient() {
        ClientTickEvents.END_CLIENT_TICK.register(client->{
            client.options.pauseOnLostFocus=false;
            if(client.screen instanceof net.minecraft.client.gui.screens.PauseScreen)client.setScreen(null);
            if(++totalTicks>6000) throw new IllegalStateException("Visual acceptance timed out");
            if(client.level==null||client.player==null||client.getSingleplayerServer()==null)return;
            if(phase==12) {
                try {Files.writeString(Path.of("visual-result.json"),"{\"profiles\":6,\"screenshots\":12,\"content_digest\":\""+Content.DATA.digest()+"\",\"status\":\"pass\"}");}
                catch(Exception error){throw new IllegalStateException(error);}
                client.stop();return;
            }
            var profile=Content.DATA.profiles().get(phase/2);boolean anatomy=phase%2==1;
            if(positioning==null) {
                System.out.println("AARNN_VISUAL_POSITION "+profile.id()+" anatomy="+anatomy);
                positioning=new CompletableFuture<>();client.options.hideGui=true;
                var server=client.getSingleplayerServer();var playerId=client.player.getUUID();
                server.execute(()->{
                    try {
                        var lab=LabWorld.level(server);
                        if(!LabWorld.ready(lab)) {positioning.complete(null);return;}
                        if(LabWorld.scenes(lab).size()!=12)throw new IllegalStateException("Exported save must already contain six pairs");
                        LabWorld.build(lab);
                        if(LabWorld.scenes(lab).size()!=12)throw new IllegalStateException("Loaded world must have six pairs");
                        var robot=LabWorld.robot(lab,profile.id());robot.anatomy(anatomy);
                        if(profile.kind().equals("fish")&&!lab.getFluidState(robot.blockPosition()).is(net.minecraft.tags.FluidTags.WATER))
                            throw new IllegalStateException("Exported fish tank has no water");
                        if(!lab.getBlockState(new net.minecraft.core.BlockPos((int)robot.originX(),60,(int)robot.originZ())).isAir())
                            throw new IllegalStateException("Export contains unrelated test terrain");
                        var c=LabWorld.centre(profile.id());var player=server.getPlayerList().getPlayer(playerId);
                        player.setGameMode(GameType.SPECTATOR);
                        double size=profile.body_length()*16;
                        double dx=anatomy?size*1.3:21,dz=anatomy?size*1.7:28;
                        double targetY=c[1]+profile.body_height()*16+(profile.kind().equals("nao")?size*.5:0);
                        double dy=anatomy?size*.85:20;
                        float yaw=(float)Math.toDegrees(Math.atan2(dx,-dz));
                        float pitch=(float)Math.toDegrees(Math.atan2(dy,Math.hypot(dx,dz)));
                        player.teleportTo(lab,c[0]+dx,targetY+dy,c[2]+dz,yaw,pitch);
                        positioning.complete(null);
                    } catch(Throwable error){positioning.completeExceptionally(error);}
                });return;
            }
            if(!positioning.isDone())return;positioning.join();
            if(!client.level.dimension().equals(LabWorld.KEY)) {positioning=null;return;}
            int found=0;
            for(var e:client.level.entitiesForRendering()) if(e instanceof SceneEntity s&&s.profile().id().equals(profile.id()))found++;
            if(found!=2) {
                if(totalTicks%100==0)System.out.println("AARNN_VISUAL_WAIT entities="+found+" profile="+profile.id());
                return;
            }
            if(++settled<40||capturing)return;
            capturing=true;
            String name=profile.id()+(anatomy?"-anatomy":"-habitat")+".png";
            Screenshot.grab(Path.of(".").toFile(),name,client.getMainRenderTarget(),message->client.execute(()->{
                if(!Files.isRegularFile(Path.of("screenshots",name)))throw new IllegalStateException("Screenshot missing");
                System.out.println("AARNN_VISUAL_CAPTURE "+name);
                phase++;settled=0;positioning=null;capturing=false;
            }));
        });
    }
}
