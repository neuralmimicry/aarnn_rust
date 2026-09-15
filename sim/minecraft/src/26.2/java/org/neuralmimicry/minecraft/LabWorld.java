package org.neuralmimicry.minecraft;

import java.util.ArrayList;
import java.util.List;
import net.minecraft.core.BlockPos;
import net.minecraft.core.registries.Registries;
import net.minecraft.resources.ResourceKey;
import net.minecraft.server.MinecraftServer;
import net.minecraft.server.level.ServerLevel;
import net.minecraft.world.level.Level;
import net.minecraft.world.level.block.Blocks;
import net.minecraft.world.level.ChunkPos;
import net.minecraft.world.entity.EntitySpawnReason;

/** Six reproducible plots in a dedicated void dimension. Never edits an ordinary player world. */
public final class LabWorld {
    public static final ResourceKey<Level> KEY=ResourceKey.create(Registries.DIMENSION,AarnnMod.id("lab"));
    private LabWorld() {}
    public static ServerLevel level(MinecraftServer server) {
        var level=server.getLevel(KEY);
        if(level==null) throw new IllegalStateException("AARNN lab dimension unavailable; restart with the mod installed");
        return level;
    }
    public static double[] centre(String profile) {
        int index=Content.DATA.profiles().indexOf(Content.profile(profile));
        return new double[]{(index%3)*48,64,(index/3)*48};
    }
    public static List<SceneEntity> scenes(ServerLevel level) {
        var result=new ArrayList<SceneEntity>();
        level.getEntities(net.minecraft.world.level.entity.EntityTypeTest.forClass(SceneEntity.class),e->true,result,13);
        if(result.size()>12) throw new IllegalStateException("Lab entity budget exceeded");
        return result;
    }
    public static SceneEntity robot(ServerLevel level,String profile) {
        return scenes(level).stream().filter(e->!e.habitatEntity()&&e.profile().id().equals(profile)).findFirst()
                .orElseThrow(()->new IllegalStateException("Run /aarnn world first"));
    }
    public static void build(ServerLevel level) {
        if(!level.dimension().equals(KEY)) throw new IllegalArgumentException("World generation is restricted to aarnn:lab");
        buildPlots(level);
    }
    /** Explicitly acknowledges a catalogue upgrade for the existing disarmed lab. */
    public static int reviewContent(ServerLevel level) {
        if(!level.dimension().equals(KEY)) throw new IllegalArgumentException("Content review is restricted to aarnn:lab");
        if(!ready(level)) throw new IllegalStateException("Lab chunks are loading; repeat the command shortly");
        var existing=scenes(level);
        if(existing.size()!=Content.DATA.profiles().size()*2)
            throw new IllegalStateException("Lab is incomplete; run /aarnn world before reviewing content");
        for(var p:Content.DATA.profiles()) {
            if(existing.stream().noneMatch(e->e.profile().id().equals(p.id()) && e.habitatEntity())
                    || existing.stream().noneMatch(e->e.profile().id().equals(p.id()) && !e.habitatEntity()))
                throw new IllegalStateException("Lab profile set is incomplete; run /aarnn world before reviewing content");
        }
        existing.forEach(SceneEntity::reviewContent);
        return existing.size();
    }
    /** Entity region files load asynchronously after block chunks. Never create replacement
     * entities until that load completes, or reopening a save duplicates every body. */
    public static boolean ready(ServerLevel level) {
        boolean ready=true;
        for(int x=-1;x<=7;x++) for(int z=-1;z<=4;z++) {
            level.setChunkForced(x,z,true);
            level.getChunk(x,z);
            ready &= level.areEntitiesLoaded(ChunkPos.pack(x,z));
        }
        return ready;
    }
    // Package-private seam for Minecraft's test server, which instantiates only an overworld.
    static void buildPlots(ServerLevel level) {
        // Load only the bounded lab region before looking for persistent entities.
        if(!ready(level)) throw new IllegalStateException("Lab chunks are loading; repeat the command shortly");
        var existing=scenes(level);
        for(var p:Content.DATA.profiles()) {
            double[] c=centre(p.id());
            boolean habitat=existing.stream().anyMatch(e->e.habitatEntity()&&e.profile().id().equals(p.id()));
            boolean robot=existing.stream().anyMatch(e->!e.habitatEntity()&&e.profile().id().equals(p.id()));
            if(!habitat) {
                // Solid substrate is just below the exact catalogue surface, preserving the rays.
                for(int x=-16;x<16;x++) for(int z=-16;z<16;z++)
                    level.setBlock(new BlockPos((int)c[0]+x,62,(int)c[2]+z),Blocks.SMOOTH_STONE.defaultBlockState(),2);
                if(p.kind().equals("fish")) {
                    // Actual stationary Minecraft water, bounded by a glass tank. Ten
                    // blocks correspond to the catalogue's .625 half-extent depth.
                    for(int x=-17;x<=16;x++)for(int z=-17;z<=16;z++)for(int y=63;y<=74;y++) {
                        boolean wall=x==-17||x==16||z==-17||z==16||y==63;
                        if(wall||y<74)level.setBlock(new BlockPos((int)c[0]+x,y,(int)c[2]+z),
                            (wall?Blocks.GLASS:Blocks.WATER).defaultBlockState(),2);
                    }
                }
                var e=AarnnMod.HABITAT.create(level,EntitySpawnReason.COMMAND); e.configure(p.id(),c[0],c[1],c[2]);
                if(!level.addFreshEntity(e)) throw new IllegalStateException("Cannot create habitat");
            }
            if(!robot) {
                var e=AarnnMod.ROBOT.create(level,EntitySpawnReason.COMMAND); e.configure(p.id(),c[0],c[1],c[2]);
                if(!level.addFreshEntity(e)) throw new IllegalStateException("Cannot create robot");
            }
        }
    }
}
