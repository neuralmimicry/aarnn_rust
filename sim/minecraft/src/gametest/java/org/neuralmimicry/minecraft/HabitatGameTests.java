package org.neuralmimicry.minecraft;

import com.google.gson.GsonBuilder;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.Map;
import net.fabricmc.fabric.api.gametest.v1.FabricGameTest;
import net.minecraft.core.BlockPos;
import net.minecraft.gametest.framework.GameTest;
import net.minecraft.gametest.framework.GameTestHelper;
import net.minecraft.nbt.CompoundTag;
import net.minecraft.world.entity.item.ItemEntity;
import net.minecraft.world.item.ItemStack;
import net.minecraft.world.item.Items;
import net.minecraft.world.level.block.Blocks;
import net.minecraft.world.level.GameType;

/** Actual Minecraft world/entity/persistence checks. Synthetic motor impulses are labelled fixtures. */
public final class HabitatGameTests implements FabricGameTest {
    @GameTest(template=FabricGameTest.EMPTY_STRUCTURE,timeoutTicks=600)
    public void allSixProfiles(GameTestHelper helper) throws Exception {
        helper.startSequence().thenWaitUntil(()->helper.assertTrue(LabWorld.ready(helper.getLevel()),"Entity regions loaded"))
            .thenExecute(()->{
                try {validate(helper);} catch(Exception error){throw new IllegalStateException(error);}
            });
    }
    private void validate(GameTestHelper helper) throws Exception {
        var server=helper.getLevel().getServer(); var lab=helper.getLevel();
        LabWorld.buildPlots(lab); helper.assertTrue(LabWorld.scenes(lab).size()==12,"Six habitats and six robots");
        LabWorld.buildPlots(lab); helper.assertTrue(LabWorld.scenes(lab).size()==12,"World creation is idempotent");
        var results=new ArrayList<Map<String,Object>>();
        for(var p:Content.DATA.profiles()) {
            var e=LabWorld.robot(lab,p.id()); var c=LabWorld.centre(p.id());
            double[] input=e.sample(false);helper.assertTrue(input.length==p.sensory(),p.id()+" input dimensions");
            for(double v:input) helper.assertTrue(Double.isFinite(v)&&v>=0&&v<=1,p.id()+" bounded input");
            helper.assertFalse(e.connected(),p.id()+" starts disarmed");
            if(p.kind().equals("fish")) {
                helper.assertTrue(lab.getFluidState(e.blockPosition()).is(net.minecraft.tags.FluidTags.WATER),"Zebrafish body is submerged in actual water");
                helper.assertTrue(lab.getFluidState(e.blockPosition().above()).is(net.minecraft.tags.FluidTags.WATER),"Water covers the fish's dorsal extent");
            }
            var pose=e.position();e.apply(new Gateway.Reply(1,new int[0]));
            helper.assertTrue(e.position().equals(pose),p.id()+" silent outputs do not move");
            e.apply(new Gateway.Reply(2,new int[]{0}));
            helper.assertTrue(e.position().distanceTo(pose)>0,p.id()+" synthetic neural-output fixture moves body");
            e.anatomy(true);
            CompoundTag saved=e.saveWithoutId(new CompoundTag());
            var restored=AarnnMod.ROBOT.create(lab);restored.load(saved);
            helper.assertTrue(restored.profile().id().equals(p.id())&&restored.anatomy(),p.id()+" identity/cutaway persisted");
            helper.assertFalse(restored.connected(),p.id()+" load disarms");
            helper.assertTrue(java.util.Arrays.stream(restored.visualOutputs()).sum()==0,p.id()+" no stale actuation restored");
            helper.assertFalse(saved.toString().contains("token"),p.id()+" credentials absent from save");
            e.anatomy(false);e.stop("disarmed after validation");
            // Native block occlusion is tested independently of canonical ray/field parity.
            var block=new BlockPos((int)c[0]+2,(int)Math.floor(c[1]+p.body_height()*16),(int)c[2]);
            lab.setBlock(block,Blocks.DIAMOND_BLOCK.defaultBlockState(),2);
            double[] origin={0,0,p.body_height()},direction={1,0,0};
            var hit=NativeSenses.rays(e).cast(origin,direction,1,new Senses.Hit(1,0));
            helper.assertTrue(hit.distance()<=.126,p.id()+" native blocks occlude camera/proximity");
            lab.setBlock(block,Blocks.AIR.defaultBlockState(),2);
            var food=new ItemEntity(lab,c[0],c[1]+p.body_height()*16,c[2],new ItemStack(Items.APPLE));lab.addFreshEntity(food);
            var augmented=NativeSenses.withFood(e,Content.habitat(p));
            helper.assertTrue(augmented.objects().size()==Content.habitat(p).objects().size()+1,p.id()+" native food field");
            food.discard();
            var actor=net.minecraft.world.entity.EntityType.VILLAGER.create(lab);
            actor.moveTo(c[0]+.15,c[1]+p.body_height()*16,c[2],0,0);lab.addFreshEntity(actor);
            var participants=NativeSenses.withFood(e,Content.habitat(p));
            helper.assertTrue(participants.objects().stream().anyMatch(part->part.id().equals("participant_"+actor.getUUID())),p.id()+" observes a native NPC through existing scene senses");
            actor.discard();
            results.add(Map.of("profile",p.id(),"sensory",p.sensory(),"output",p.output(),"parts",p.parts().size(),
                "habitat_objects",Content.habitat(p).objects().size(),"status","pass","neural_execution","synthetic output fixture only"));
            // Restore the delivered world to its initial pose and neutral outputs.
            e.configure(p.id(),c[0],c[1],c[2]);e.setYRot(0);
        }
        // A small ordinary-world arrival platform keeps a newly opened save safe.
        var overworld=server.overworld();
        for(int x=-5;x<=5;x++)for(int z=155;z<=165;z++)
            overworld.setBlock(new BlockPos(x,80,z),Blocks.SEA_LANTERN.defaultBlockState(),2);
        overworld.setDefaultSpawnPos(new BlockPos(0,81,160),180);
        server.setDefaultGameType(GameType.CREATIVE);
        server.saveEverything(true,true,true);
        Path report=Path.of("acceptance.json");
        Files.writeString(report,new GsonBuilder().setPrettyPrinting().create().toJson(Map.of(
                "scenario","SIM-MINECRAFT-001","content_digest",Content.DATA.digest(),"profiles",results,
                "world","world","engine","Minecraft 1.21.1 Fabric","neural_validation","not claimed")));
        helper.succeed();
    }
}
