package org.neuralmimicry.minecraft;

import net.minecraft.nbt.CompoundTag;
import net.minecraft.network.chat.Component;
import net.minecraft.network.syncher.EntityDataAccessor;
import net.minecraft.network.syncher.EntityDataSerializers;
import net.minecraft.network.syncher.SynchedEntityData;
import net.minecraft.world.entity.Entity;
import net.minecraft.world.entity.EntityType;
import net.minecraft.world.level.Level;
import java.util.Arrays;
import java.util.HashMap;
import java.util.Map;

/** Persistent visual body; session credentials and neural state are never saved. */
public final class SceneEntity extends Entity {
    private static final EntityDataAccessor<String> PROFILE=SynchedEntityData.defineId(SceneEntity.class,EntityDataSerializers.STRING);
    private static final EntityDataAccessor<Boolean> ANATOMY=SynchedEntityData.defineId(SceneEntity.class,EntityDataSerializers.BOOLEAN);
    private static final EntityDataAccessor<CompoundTag> OUTPUTS=SynchedEntityData.defineId(SceneEntity.class,EntityDataSerializers.COMPOUND_TAG);
    private static final EntityDataAccessor<String> STATUS=SynchedEntityData.defineId(SceneEntity.class,EntityDataSerializers.STRING);
    private static final EntityDataAccessor<String> CHAT=SynchedEntityData.defineId(SceneEntity.class,EntityDataSerializers.STRING);
    private int chatTicks;
    private double originX, originY=64, originZ, heading;
    private double[] actuators=new double[96];
    private final Map<String,Double> history=new HashMap<>();
    private Gateway.Session session;
    private String savedDigest=Content.DATA.digest();
    public SceneEntity(EntityType<?> type,Level world) { super(type,world); noPhysics=true; setNoGravity(true); }
    public boolean habitatEntity() { return getType()==AarnnMod.HABITAT; }
    public Content.Profile profile() { return Content.profile(entityData.get(PROFILE)); }
    public String status() { return entityData.get(STATUS); }
    public String chatBubble() { return entityData.get(CHAT); }
    public void chatBubble(String text,int ticks) { entityData.set(CHAT,text);chatTicks=ticks; }
    public boolean anatomy() { return entityData.get(ANATOMY); }
    public void anatomy(boolean value) { entityData.set(ANATOMY,value); }
    public double originX() { return originX; }
    public double originY() { return originY; }
    public double originZ() { return originZ; }
    public double[] visualOutputs() {
        int[] encoded=entityData.get(OUTPUTS).getIntArray("values");
        double[] result=new double[profile().output()];
        for(int i=0;i<Math.min(encoded.length,result.length);i++) result[i]=Senses.clamp(encoded[i]/255.0,0,1);
        return result;
    }
    public void configure(String id,double x,double y,double z) {
        entityData.set(PROFILE,Content.profile(id).id());
        setUUID(java.util.UUID.nameUUIDFromBytes(("aarnn-lab-v1:"+id+":"+habitatEntity()).getBytes(java.nio.charset.StandardCharsets.UTF_8)));
        originX=x; originY=y; originZ=z; actuators=new double[profile().output()];
        heading=0;setYRot(0);
        setPos(x,y+(habitatEntity()?0:profile().body_height()*Content.HALF_EXTENT),z);
        setCustomName(Component.literal(id+" · "+(habitatEntity()?profile().habitat():"disarmed")));
        setCustomNameVisible(!habitatEntity());
    }
    public Senses.Pose pose() {
        return new Senses.Pose((getX()-originX)/Content.HALF_EXTENT,-(getZ()-originZ)/Content.HALF_EXTENT,heading,actuators);
    }
    public double[] sample(boolean nativeWorld) {
        var habitat=Content.habitat(profile());
        if(!nativeWorld) return Senses.sample(profile(),habitat,pose(),history);
        return Senses.sample(profile(),NativeSenses.withFood(this,habitat),pose(),history,NativeSenses.rays(this));
    }
    public boolean connected() { return session!=null && session.active(); }
    public boolean neuralReady() { return session!=null && session.ready(); }
    public String boundNetwork() { return connected()?session.binding().networkId():null; }
    public void connect(AdapterConfig config) {
        stop("disarmed");
        if(!config.allowLegacySandboxInference || !Content.DATA.digest().equals(savedDigest))
            throw new IllegalArgumentException("Legacy sandbox disabled or saved content needs review");
        var binding=config.bindings.get(profile().id());
        if(binding==null||!profile().id().equals(binding.networkId()))
            throw new IllegalArgumentException("Binding must select this robot's own profile route");
        session=new Gateway.Session(profile(),binding,Gateway.endpoint(config.endpoint),System.getenv(config.tokenEnvironment));
        history.clear(); entityData.set(STATUS,session.status());
    }
    public void stop(String reason) {
        chatBubble("",0);
        if(session!=null) session.stop(reason);
        session=null; Arrays.fill(actuators,0); syncOutputs();
        entityData.set(STATUS,reason);
        if(!habitatEntity()) setCustomName(Component.literal(profile().id()+" · "+reason));
    }
    public void apply(Gateway.Reply reply) {
        for(int i=0;i<actuators.length;i++) actuators[i]*=.82;
        for(int i:reply.spikes()) actuators[i]=1;
        var motion=Senses.motion(profile(),actuators);
        heading+=motion.turn()*.08;
        var pose=pose();
        double[] p={pose.x(),pose.y(),profile().body_height()},d={Math.cos(heading),Math.sin(heading),0};
        var hit=Senses.ray(Content.habitat(profile()),p,d,profile().body_length()*.65);
        hit=NativeSenses.rays(this).cast(p,d,profile().body_length()*.65,hit);
        if(hit.distance()>=profile().body_length()*.6) {
            double x=Senses.clamp(pose.x()+d[0]*motion.drive()*.008,-.85,.85);
            double y=Senses.clamp(pose.y()+d[1]*motion.drive()*.008,-.85,.85);
            setPos(originX+x*Content.HALF_EXTENT,getY(),originZ-y*Content.HALF_EXTENT);
        }
        setYRot((float)Math.toDegrees(heading));
        syncOutputs();
    }
    private void syncOutputs() {
        var tag=new CompoundTag(); int[] values=new int[actuators.length];
        for(int i=0;i<values.length;i++) values[i]=(int)Math.round(actuators[i]*255);
        tag.putIntArray("values",values); entityData.set(OUTPUTS,tag);
    }
    @Override public void tick() {
        if(!level().isClientSide && chatTicks>0 && --chatTicks==0)entityData.set(CHAT,"");
        // These are persistent procedural displays with our own bounded motion. Vanilla
        // baseTick scans the entire 32 x 16 x 32 habitat box for fluids every tick;
        // portals, fire and fluid pushing are not part of this reference transducer.
        if(level().isClientSide || habitatEntity() || session==null) return;
        var reply=session.poll();
        if(reply!=null) apply(reply);
        if(!session.active()) { String fault=session.status(); stop(fault); }
        else if(!session.pending()) {
            try { session.submit(sample(true),System.nanoTime()); }
            catch(RuntimeException error) { stop("fault: sensory frame rejected"); }
        }
        if(session!=null) entityData.set(STATUS,session.status());
        setCustomName(Component.literal(profile().id()+" · "+status()));
    }
    @Override protected void defineSynchedData(SynchedEntityData.Builder builder) {
        builder.define(PROFILE,"celegans"); builder.define(ANATOMY,false);
        builder.define(OUTPUTS,new CompoundTag()); builder.define(STATUS,"disarmed");
        builder.define(CHAT,"");
    }
    @Override protected void addAdditionalSaveData(CompoundTag tag) {
        tag.putInt("Schema",1); tag.putString("ContentDigest",savedDigest);
        tag.putString("Profile",profile().id()); tag.putBoolean("Anatomy",anatomy());
        tag.putDouble("OriginX",originX); tag.putDouble("OriginY",originY); tag.putDouble("OriginZ",originZ);
        tag.putDouble("Heading",heading);
    }
    @Override protected void readAdditionalSaveData(CompoundTag tag) {
        if(tag.getInt("Schema")!=1) throw new IllegalArgumentException("Unsupported AARNN entity schema");
        entityData.set(PROFILE,Content.profile(tag.getString("Profile")).id());
        originX=tag.getDouble("OriginX"); originY=tag.getDouble("OriginY"); originZ=tag.getDouble("OriginZ");
        heading=tag.getDouble("Heading"); savedDigest=tag.getString("ContentDigest");
        if(!Double.isFinite(originX+originY+originZ+heading)) throw new IllegalArgumentException("Invalid saved pose");
        anatomy(tag.getBoolean("Anatomy")); actuators=new double[profile().output()];
        stop(Content.DATA.digest().equals(savedDigest)?"disarmed after load":"disarmed: content mismatch");
    }
    @Override public boolean shouldRenderAtSqrDistance(double distance) { return distance<256*256; }
}
