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
import net.minecraft.resources.Identifier;
import net.minecraft.server.MinecraftServer;
import net.minecraft.world.entity.EntityType;
import net.minecraft.world.entity.MobCategory;
import net.minecraft.world.level.portal.TeleportTransition;
import net.minecraft.world.phys.Vec3;

public final class AarnnMod implements ModInitializer {
    private final NaoChat naoChat=new NaoChat();
    private record Deferred(CommandSourceStack source,String action,String profile,int deadline) {}
    private final java.util.List<Deferred> pending=new java.util.ArrayList<>();
    private final java.util.Map<MinecraftServer,AdapterConfig> automaticRuns=new java.util.WeakHashMap<>();
    private final java.util.Set<MinecraftServer> automaticBuilds=java.util.Collections.newSetFromMap(new java.util.WeakHashMap<>());
    private final java.util.Set<MinecraftServer> automaticAttempts=java.util.Collections.newSetFromMap(new java.util.WeakHashMap<>());
    private final java.util.Set<MinecraftServer> automaticVisits=java.util.Collections.newSetFromMap(new java.util.WeakHashMap<>());
    private static final org.slf4j.Logger LOG=org.slf4j.LoggerFactory.getLogger("aarnn");
    public static Identifier id(String path) { return Identifier.fromNamespaceAndPath("aarnn",path); }
    public static final EntityType<SceneEntity> HABITAT=Registry.register(BuiltInRegistries.ENTITY_TYPE,id("habitat"),
            EntityType.Builder.<SceneEntity>of(SceneEntity::new,MobCategory.MISC).sized(32,16)
                .clientTrackingRange(16).updateInterval(20).build(net.minecraft.resources.ResourceKey.create(net.minecraft.core.registries.Registries.ENTITY_TYPE,id("habitat"))));
    public static final EntityType<SceneEntity> ROBOT=Registry.register(BuiltInRegistries.ENTITY_TYPE,id("robot"),
            EntityType.Builder.<SceneEntity>of(SceneEntity::new,MobCategory.MISC).sized(4,4)
                .clientTrackingRange(16).updateInterval(1).build(net.minecraft.resources.ResourceKey.create(net.minecraft.core.registries.Registries.ENTITY_TYPE,id("robot"))));
    @Override public void onInitialize() {
        CommandRegistrationCallback.EVENT.register((dispatcher,registry,environment)-> {
            naoChat.register(dispatcher);
            var root=Commands.literal("aarnn").requires(s->s.permissions().hasPermission(net.minecraft.server.permissions.Permissions.COMMANDS_GAMEMASTER));
            root.then(Commands.literal("world").executes(c->run(c.getSource(),"world",null)));
            root.then(Commands.literal("stop").executes(c->run(c.getSource(),"stop",null)));
            root.then(Commands.literal("status").executes(c->run(c.getSource(),"status",null)));
            root.then(Commands.literal("review").executes(c->run(c.getSource(),"review",null)));
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
            automaticRuns.remove(server); automaticBuilds.remove(server);
            automaticAttempts.remove(server); automaticVisits.remove(server);
            var level=server.getLevel(LabWorld.KEY);
            if(level!=null) LabWorld.scenes(level).forEach(e->e.stop("server stopping"));
        });
        ServerTickEvents.END_SERVER_TICK.register(server->{
            naoChat.tick(server);
            autoStartConfiguredLab(server);
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

    /** Opt-in launcher path that makes a configured neural run visible in-game. */
    private void autoStartConfiguredLab(MinecraftServer server) {
        try {
            var config=automaticRuns.get(server);
            if(config==null) {
                config=AdapterConfig.read(FabricLoader.getInstance().getConfigDir().resolve("aarnn.json"));
                if(!config.autoConnectOnStart || config.autoConnectProfile==null
                        || config.autoConnectProfile.isBlank()) return;
                automaticRuns.put(server,config);
            }
            var level=LabWorld.level(server);
            if(!LabWorld.ready(level)) return;
            if(!automaticBuilds.contains(server)) {
                LabWorld.build(level);
                automaticBuilds.add(server);
            }
            var robot=LabWorld.robot(level,config.autoConnectProfile);
            if(automaticAttempts.add(server)) {
                try {
                    robot.connect(config);
                    LOG.info("AARNN auto-connected profile={} network={} endpoint=loopback", robot.profile().id(), robot.boundNetwork());
                } catch(Exception error) {
                    robot.stop("fault: automatic connection failed");
                    LOG.warn("AARNN automatic connection failed: {}", safeMessage(error));
                }
            }
            if(config.autoVisitProfile && !automaticVisits.contains(server)) {
                var player=server.getPlayerList().getPlayers().stream().findFirst().orElse(null);
                if(player!=null) {
                    var c=LabWorld.centre(config.autoConnectProfile);
                    player.teleport(new TeleportTransition(level,new Vec3(c[0],c[1]+1,c[2]+12),Vec3.ZERO,180,15,TeleportTransition.DO_NOTHING));
                    automaticVisits.add(server);
                }
            }
        } catch(Exception error) {
            LOG.warn("AARNN automatic launcher setup pending: {}", safeMessage(error));
        }
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
                    var e=LabWorld.robot(level,"hexapod");
                    source.sendSuccess(()->Component.literal("AARNN hexapod: "+e.status()+" · "+e.telemetry()),false);
                }
                case "review" -> {
                    int count=LabWorld.reviewContent(level);
                    source.sendSuccess(()->Component.literal("Reviewed "+count+" saved lab entities for content "+Content.DATA.digest()+"; all remain disarmed"),true);
                }
                case "visit" -> {
                    LabWorld.build(level);
                    LabWorld.robot(level,profile);
                    var c=LabWorld.centre(profile);
                    source.getPlayerOrException().teleport(new TeleportTransition(level,new Vec3(c[0],c[1]+1,c[2]+12),Vec3.ZERO,180,15,TeleportTransition.DO_NOTHING));
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
