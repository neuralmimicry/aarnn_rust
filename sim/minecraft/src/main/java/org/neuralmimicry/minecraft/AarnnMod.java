package org.neuralmimicry.minecraft;

import com.mojang.brigadier.arguments.StringArgumentType;
import net.fabricmc.api.ModInitializer;
import net.fabricmc.fabric.api.command.v2.CommandRegistrationCallback;
import net.fabricmc.fabric.api.event.lifecycle.v1.ServerEntityEvents;
import net.fabricmc.fabric.api.event.lifecycle.v1.ServerLifecycleEvents;
import net.fabricmc.fabric.api.event.lifecycle.v1.ServerTickEvents;
import net.fabricmc.loader.api.FabricLoader;
import net.minecraft.commands.CommandSourceStack;
import net.minecraft.commands.Commands;
import net.minecraft.core.Registry;
import net.minecraft.core.registries.BuiltInRegistries;
import net.minecraft.network.chat.Component;
import net.minecraft.resources.ResourceLocation;
import net.minecraft.world.entity.EntityType;
import net.minecraft.world.entity.MobCategory;

public final class AarnnMod implements ModInitializer {
    private final NaoChat naoChat=new NaoChat();
    private record Deferred(CommandSourceStack source,String action,String profile,int deadline) {}
    private final java.util.List<Deferred> pending=new java.util.ArrayList<>();
    public static ResourceLocation id(String path) { return ResourceLocation.fromNamespaceAndPath("aarnn",path); }
    public static final EntityType<SceneEntity> HABITAT=Registry.register(BuiltInRegistries.ENTITY_TYPE,id("habitat"),
            EntityType.Builder.<SceneEntity>of(SceneEntity::new,MobCategory.MISC).sized(32,16)
                .clientTrackingRange(16).updateInterval(20).build("aarnn:habitat"));
    public static final EntityType<SceneEntity> ROBOT=Registry.register(BuiltInRegistries.ENTITY_TYPE,id("robot"),
            EntityType.Builder.<SceneEntity>of(SceneEntity::new,MobCategory.MISC).sized(4,4)
                .clientTrackingRange(16).updateInterval(1).build("aarnn:robot"));
    @Override public void onInitialize() {
        CommandRegistrationCallback.EVENT.register((dispatcher,registry,environment)-> {
            naoChat.register(dispatcher);
            var root=Commands.literal("aarnn").requires(s->s.hasPermission(2));
            root.then(Commands.literal("world").executes(c->run(c.getSource(),"world",null)));
            root.then(Commands.literal("stop").executes(c->run(c.getSource(),"stop",null)));
            root.then(Commands.literal("status").executes(c->run(c.getSource(),"status",null)));
            for(String action:new String[]{"visit","connect","disconnect","anatomy","senses"}) {
                root.then(Commands.literal(action).then(Commands.argument("profile",StringArgumentType.word())
                    .suggests((context,builder)-> { Content.DATA.profiles().forEach(p->builder.suggest(p.id())); return builder.buildFuture(); })
                    .executes(c->run(c.getSource(),action,StringArgumentType.getString(c,"profile")))));
            }
            dispatcher.register(root);
        });
        ServerEntityEvents.ENTITY_UNLOAD.register((entity,level)-> {
            if(entity instanceof SceneEntity scene) scene.stop("disarmed on unload");
        });
        ServerLifecycleEvents.SERVER_STOPPING.register(server-> {
            naoChat.close(server);
            pending.removeIf(command->command.source().getServer()==server);
            var level=server.getLevel(LabWorld.KEY);
            if(level!=null) LabWorld.scenes(level).forEach(e->e.stop("server stopping"));
        });
        ServerTickEvents.END_SERVER_TICK.register(server->{
            naoChat.tick(server);
            var commands=new java.util.ArrayList<Deferred>();
            var iterator=pending.iterator();
            while(iterator.hasNext()) {
                var command=iterator.next();
                if(command.source().getServer()!=server)continue;
                if(server.getTickCount()>command.deadline()) {
                    iterator.remove();command.source().sendFailure(Component.literal("AARNN lab loading timed out; inspect server storage"));
                } else if(LabWorld.ready(LabWorld.level(server))) {iterator.remove();commands.add(command);}
            }
            for(var command:commands)run(command.source(),command.action(),command.profile());
        });
        ServerLifecycleEvents.SERVER_STARTED.register(server-> {
            try { AdapterConfig.read(FabricLoader.getInstance().getConfigDir().resolve("aarnn.json")); }
            catch(Exception error) { org.slf4j.LoggerFactory.getLogger("aarnn").error("AARNN config invalid; inference remains disarmed"); }
        });
    }
    private int run(CommandSourceStack source,String action,String profile) {
        try {
            var level=LabWorld.level(source.getServer());
            if(profile!=null) Content.profile(profile);
            if((action.equals("world")||action.equals("visit"))&&!LabWorld.ready(level)) {
                if(pending.size()>=8)throw new IllegalStateException("Lab command queue full");
                pending.add(new Deferred(source,action,profile,source.getServer().getTickCount()+200));
                source.sendSuccess(()->Component.literal("Loading AARNN lab…"),false);return 1;
            }
            switch(action) {
                case "world" -> { LabWorld.build(level); source.sendSuccess(()->Component.literal("AARNN lab ready: six profiles. /aarnn visit celegans"),false); }
                case "stop" -> LabWorld.scenes(level).forEach(e->e.stop("disarmed by operator"));
                case "status" -> {
                    source.sendSuccess(()->Component.literal("Content "+Content.DATA.digest()+" · reference sandbox"),false);
                    for(var e:LabWorld.scenes(level)) if(!e.habitatEntity())
                        source.sendSuccess(()->Component.literal(e.profile().id()+": "+e.profile().sensory()+" / "+e.profile().output()+" · "+e.status()),false);
                }
                case "visit" -> {
                    LabWorld.build(level);
                    LabWorld.robot(level,profile);
                    var c=LabWorld.centre(profile);
                    source.getPlayerOrException().teleportTo(level,c[0],c[1]+1,c[2]+12,180,15);
                }
                case "disconnect" -> LabWorld.robot(level,profile).stop("disarmed by operator");
                case "anatomy" -> { var e=LabWorld.robot(level,profile); e.anatomy(!e.anatomy()); }
                case "senses" -> {
                    var e=LabWorld.robot(level,profile); double[] values=e.sample(true);
                    double sum=java.util.Arrays.stream(values).sum();
                    source.sendSuccess(()->Component.literal(profile+": "+values.length+" sensory channels; mean "+String.format(java.util.Locale.ROOT,"%.4f",sum/values.length)),false);
                }
                case "connect" -> {
                    var config=AdapterConfig.read(FabricLoader.getInstance().getConfigDir().resolve("aarnn.json"));
                    var binding=config.bindings.get(profile);
                    if(binding==null) throw new IllegalArgumentException("No profile binding");
                    for(var e:LabWorld.scenes(level)) if(e.connected() && !e.profile().id().equals(profile)) {
                        if(binding.networkId().equals(e.boundNetwork()))
                            throw new IllegalArgumentException("A lab robot already uses that brain; independent binding required");
                    }
                    LabWorld.robot(level,profile).connect(config);
                    source.sendSuccess(()->Component.literal(profile+" armed for legacy sandbox inference; /aarnn stop disarms locally"),true);
                }
                default -> throw new IllegalArgumentException("Unknown command");
            }
            return 1;
        } catch(Exception error) {
            // Never include credentials, URLs with credentials or remote payloads in command feedback.
            source.sendFailure(Component.literal("AARNN command failed: "+safeMessage(error)));
            return 0;
        }
    }
    private static String safeMessage(Exception error) {
        return error instanceof IllegalArgumentException || error instanceof IllegalStateException
                ? error.getMessage() : "check installation/configuration and server log";
    }
}
