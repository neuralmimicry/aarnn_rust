package org.neuralmimicry.minecraft;

import java.util.ArrayList;
import java.util.Comparator;
import net.minecraft.world.entity.item.ItemEntity;
import net.minecraft.world.item.Items;
import net.minecraft.world.level.ClipContext;
import net.minecraft.world.phys.AABB;
import net.minecraft.world.phys.HitResult;
import net.minecraft.world.phys.Vec3;

/** Bounded Minecraft extension: placed blocks occlude rays; dropped food supplies chemical cues. */
public final class NativeSenses {
    private NativeSenses() {}
    public static Senses.NativeRay rays(SceneEntity robot) {
        return (p,d,range,shared) -> {
            double k=Content.HALF_EXTENT;
            var start=new Vec3(robot.originX()+p[0]*k,robot.originY()+p[2]*k,robot.originZ()-p[1]*k);
            var end=start.add(d[0]*range*k,d[2]*range*k,-d[1]*range*k);
            var hit=robot.level().clip(new ClipContext(start,end,ClipContext.Block.COLLIDER,ClipContext.Fluid.NONE,robot));
            if(hit.getType()==HitResult.Type.MISS) return shared;
            double distance=hit.getLocation().distanceTo(start)/k;
            if(distance<1e-5 || distance>=shared.distance()) return shared;
            int rgb=robot.level().getBlockState(hit.getBlockPos()).getMapColor(robot.level(),hit.getBlockPos()).col;
            double lum=(((rgb>>16)&255)*.2126+((rgb>>8)&255)*.7152+(rgb&255)*.0722)/255;
            return new Senses.Hit(distance,lum);
        };
    }
    public static Content.Habitat withFood(SceneEntity robot,Content.Habitat original) {
        var parts=new ArrayList<>(original.objects());
        var bounds=new AABB(robot.originX()-16,robot.originY(),robot.originZ()-16,
                            robot.originX()+16,robot.originY()+16,robot.originZ()+16);
        var items=new ArrayList<ItemEntity>();
        robot.level().getEntities(net.minecraft.world.level.entity.EntityTypeTest.forClass(ItemEntity.class),
                bounds,NativeSenses::isFood,items,32);
        items.stream().sorted(Comparator.comparingInt(ItemEntity::getId)).limit(32).forEach(item -> {
            double k=Content.HALF_EXTENT;
            parts.add(new Content.Part("food_"+item.getId(),"sphere",
                new double[]{(item.getX()-robot.originX())/k,-(item.getZ()-robot.originZ())/k,(item.getY()-robot.originY())/k},
                new double[]{.03,.03,.03},new double[]{.8,.3,.1},0,false,"organic","chemical",.25,.8,"root",false));
        });
        // Neural-driven bodies, ordinary players and NPCs are visible/touchable
        // sensory participants. No text channel is invented for non-speaking species.
        var actors=new ArrayList<net.minecraft.world.entity.Entity>();
        robot.level().getEntities(net.minecraft.world.level.entity.EntityTypeTest.forClass(net.minecraft.world.entity.Entity.class),
            bounds,e->e!=robot && (e instanceof net.minecraft.world.entity.LivingEntity || e instanceof SceneEntity scene && !scene.habitatEntity()),actors,64);
        actors.sort(Comparator.comparing(e->e.getUUID().toString()));
        for(var actor:actors) {
            double k=Content.HALF_EXTENT;var box=actor.getBoundingBox();var centre=box.getCenter();
            parts.add(new Content.Part("participant_"+actor.getUUID(),"box",
                new double[]{(centre.x-robot.originX())/k,-(centre.z-robot.originZ())/k,(centre.y-robot.originY())/k},
                new double[]{Math.max(.01,box.getXsize()/k),Math.max(.01,box.getZsize()/k),Math.max(.01,box.getYsize()/k)},
                new double[]{.25,.65,.85},0,false,"participant","",0,0,"root",true));
        }
        return new Content.Habitat(original.id(),original.substrate(),original.half_extent_m(),parts);
    }
    private static boolean isFood(ItemEntity e) {
        var item=e.getItem();
        return item.is(Items.APPLE)||item.is(Items.SUGAR)||item.is(Items.SWEET_BERRIES)||item.is(Items.HONEY_BOTTLE);
    }
}
