package org.neuralmimicry.minecraft;

import com.google.gson.Gson;
import com.google.gson.JsonObject;
import com.mojang.brigadier.CommandDispatcher;
import com.mojang.brigadier.arguments.StringArgumentType;
import java.net.URI;
import java.net.http.HttpClient;
import java.net.http.HttpRequest;
import java.net.http.HttpResponse;
import java.nio.charset.StandardCharsets;
import java.time.Duration;
import java.util.HashMap;
import java.util.Map;
import java.util.UUID;
import java.util.concurrent.CompletableFuture;
import net.minecraft.commands.CommandSourceStack;
import net.minecraft.commands.Commands;
import net.minecraft.network.chat.Component;
import net.minecraft.server.MinecraftServer;
import net.minecraft.server.level.ServerPlayer;
import net.minecraft.server.level.ServerLevel;
import net.minecraft.world.entity.Entity;
import net.minecraft.world.entity.LivingEntity;

/** Ordinary-player social channel. Server-derived identity/proximity; no operator powers. */
public final class NaoChat {
    private static final Gson JSON = new Gson();
    private final Map<UUID,Pending> turns = new HashMap<>();
    private final Map<UUID,Entity> encountered = new HashMap<>();
    private final HttpClient client = HttpClient.newBuilder().connectTimeout(Duration.ofSeconds(2))
        .followRedirects(HttpClient.Redirect.NEVER).build();
    private static final class Pending {
        final MinecraftServer server;
        final SceneEntity body;
        final Entity speaker;
        final String principal;
        final int deadline;
        String id;
        int nextPoll;
        CompletableFuture<HttpResponse<byte[]>> request;
        Pending(Entity player,SceneEntity body) {
            this.server=player.level().getServer();this.body=body;this.speaker=player;
            principal="java:"+player.getUUID();deadline=server.getTickCount()+1200;
        }
    }
    public void register(CommandDispatcher<CommandSourceStack> dispatcher) {
        dispatcher.register(Commands.literal("nao").requires(s->s.getEntity() instanceof ServerPlayer)
            .executes(c->{c.getSource().sendSuccess(()->Component.literal("Talk near NAO: /nao say <message>; /nao stop. Optional dictation: the NAO conversation window."),false);return 1;})
            .then(Commands.literal("say").then(Commands.argument("message",StringArgumentType.greedyString())
                .executes(c->say(c.getSource(),StringArgumentType.getString(c,"message")))))
            .then(Commands.literal("stop").executes(c->{stop(c.getSource().getPlayerOrException());return 1;})));
    }
    private SceneEntity nearby(Entity player) {
        if(!((ServerLevel)player.level()).dimension().equals(LabWorld.KEY))
            throw new IllegalArgumentException("Visit NAO in the AARNN lab first");
        var body=LabWorld.robot(((ServerLevel)player.level()),"nao");
        if(!body.neuralReady() || player.distanceToSqr(body)>24*24)
            throw new IllegalArgumentException("NAO must receive neural frames and be within 24 blocks");
        return body;
    }
    private int say(CommandSourceStack source,String message) {
        try {
            var player=source.getPlayerOrException();var body=nearby(player);
            if(message.isBlank() || message.getBytes(StandardCharsets.UTF_8).length>256 || message.codePoints().anyMatch(Character::isISOControl))
                throw new IllegalArgumentException("Use 1–256 UTF-8 bytes without control characters");
            if(turns.containsKey(player.getUUID()) || turns.size()>=16)
                throw new IllegalArgumentException("A conversation turn is already pending; wait or /nao stop");
            var pending=new Pending(player,body);var data=new JsonObject();
            data.addProperty("player",pending.principal);data.addProperty("text",message);
            long capture=System.nanoTime();data.addProperty("sequence",capture/1000);data.addProperty("capture_ns",capture);
            data.addProperty("modality","typed");
            data.add("position",JSON.toJsonTree(new double[]{(player.getX()-body.getX())/24,(player.getY()-body.getY())/24,(player.getZ()-body.getZ())/24}));
            pending.request=request("/api/turn",data);turns.put(player.getUUID(),pending);
            player.sendSystemMessage(Component.literal("You → NAO: "+message));return 1;
        } catch(Exception error) {
            source.sendFailure(Component.literal(error instanceof IllegalArgumentException?error.getMessage():"NAO chat unavailable; check the local social session"));return 0;
        }
    }
    private CompletableFuture<HttpResponse<byte[]>> request(String path,JsonObject body) {
        String token=System.getenv("NM_NAO_ADAPTER_TOKEN");
        String address=System.getenv().getOrDefault("NM_NAO_CHAT_URL","http://127.0.0.1:62621");
        URI root=URI.create(address);
        if(token==null || token.length()<24 || token.length()>4096 || !"http".equals(root.getScheme())
                || !"127.0.0.1".equals(root.getHost()) || root.getRawUserInfo()!=null || root.getRawQuery()!=null || root.getRawFragment()!=null
                || !(root.getPath().isEmpty() || root.getPath().equals("/")))
            throw new IllegalArgumentException("NAO social session unavailable; start scripts/run_nao_social.py");
        var request=HttpRequest.newBuilder(root.resolve(path)).timeout(Duration.ofSeconds(5))
            .header("Content-Type","application/json").header("Authorization","Bearer "+token)
            .POST(HttpRequest.BodyPublishers.ofString(JSON.toJson(body))).build();
        return client.sendAsync(request,ignored->new Gateway.BoundedBody());
    }
    private static void tell(Entity receiver,String text) {
        if(receiver instanceof ServerPlayer player)player.sendSystemMessage(Component.literal(text));
    }
    public void tick(MinecraftServer server) {
        // Engine-derived participants only. Presence is a receptor sample, never
        // a made-up player utterance. One invitation per encounter, bounded.
        if(server.getTickCount()%20==0 && System.getenv("NM_NAO_ADAPTER_TOKEN")!=null) {
            encountered.entrySet().removeIf(e->e.getValue().isRemoved());
            try {
                ServerLevel level=server.getLevel(LabWorld.KEY);
                if(level!=null) {
                    SceneEntity body=LabWorld.robot(level,"nao");
                    var speakers=new java.util.ArrayList<Entity>();
                    // Arming can precede the first frame after an idle launcher.
                    // Do not consume an encounter while the proxy is still idle.
                    if(body.neuralReady())level.getEntities(net.minecraft.world.level.entity.EntityTypeTest.forClass(Entity.class),body.getBoundingBox().inflate(24),
                        e->e!=body && (e instanceof ServerPlayer || isVillager(e) || e instanceof SceneEntity robot && !robot.habitatEntity() && robot.profile().id().equals("nao")),speakers,64);
                    for(Entity actor:speakers) {
                        if(turns.size()>=8 || encountered.size()>=64)break;
                        if(encountered.containsKey(actor.getUUID()) || turns.containsKey(actor.getUUID()))continue;
                        if(actor.distanceToSqr(body)>24*24)continue;
                        Pending turn=new Pending(actor,body);JsonObject data=new JsonObject();
                        long capture=System.nanoTime();data.addProperty("sequence",capture/1000);data.addProperty("capture_ns",capture);
                        data.addProperty("player",turn.principal);data.addProperty("kind",actor instanceof ServerPlayer?"player":"npc");
                        data.add("position",JSON.toJsonTree(new double[]{(actor.getX()-body.getX())/24,(actor.getY()-body.getY())/24,(actor.getZ()-body.getZ())/24}));
                        turn.request=request("/api/encounter",data);turns.put(actor.getUUID(),turn);encountered.put(actor.getUUID(),actor);
                    }
                }
            } catch(IllegalArgumentException ignored) { /* Missing/unconnected lab: no encounter admission. */ }
        }
        var iterator=turns.entrySet().iterator();
        while(iterator.hasNext()) {
            var entry=iterator.next();var p=entry.getValue();if(p.server!=server)continue;
            var player=p.speaker;
            try {
                if(player.isRemoved() || nearby(player)!=p.body || server.getTickCount()>p.deadline)
                    throw new IllegalArgumentException("Conversation stopped: player, body or deadline changed");
                if(p.request==null) {
                    if(server.getTickCount()<p.nextPoll)continue;
                    var data=new JsonObject();data.addProperty("player",p.principal);data.addProperty("id",p.id);
                    p.request=request("/api/poll",data);
                }
                if(!p.request.isDone())continue;
                var response=p.request.join();p.request=null;
                if(response.statusCode()!=200)throw new IllegalArgumentException("NAO conversation unavailable; join a running social session");
                JsonObject data=JSON.fromJson(new String(response.body(),StandardCharsets.UTF_8),JsonObject.class);
                if(!"NAO-SOCIAL/1".equals(data.get("schema").getAsString()))throw new IllegalArgumentException("NAO social schema mismatch");
                String state=data.get("state").getAsString();
                if(state.equals("observed")){iterator.remove();continue;}
                p.id=data.get("id").getAsString();
                if(state.equals("queued") || state.equals("active")){p.nextPoll=server.getTickCount()+4;continue;}
                iterator.remove();
                if(state.equals("replied") && data.has("reply") && !data.get("reply").isJsonNull()) {
                    String text=data.getAsJsonObject("reply").get("text").getAsString();
                    if(text.length()>512)throw new IllegalArgumentException("NAO reply exceeds bound");
                    tell(player,"NAO: "+text);p.body.chatBubble(text,200);
                } else tell(player,"NAO channel: "+state);
            } catch(Exception error) {
                // Removing after terminal handling above would invalidate the iterator.
                if(turns.containsKey(entry.getKey()))iterator.remove();
                cancel(p);
                if(!player.isRemoved())tell(player,"NAO conversation stopped; no reply was replayed. Use /nao say to try a new turn.");
            }
        }
    }
    private void cancel(Pending pending) {
        if(pending.request!=null)pending.request.cancel(true);
        try {var data=new JsonObject();data.addProperty("player",pending.principal);request("/api/stop",data).exceptionally(error->null);}
        catch(RuntimeException ignored) { /* Local cancellation has already suppressed presentation. */ }
    }
    private static boolean isVillager(Entity entity) {
        for(Class<?> type=entity.getClass();type!=null;type=type.getSuperclass())
            if(type.getName().equals("net.minecraft.world.entity.npc.villager.AbstractVillager")) return true;
        return false;
    }
    public void stop(ServerPlayer player) {
        if(encountered.size()<64)encountered.put(player.getUUID(),player);
        var pending=turns.remove(player.getUUID());if(pending!=null)cancel(pending);
        player.sendSystemMessage(Component.literal("NAO conversation stopped."));
    }
    public void close(MinecraftServer server) {
        turns.entrySet().removeIf(e->{if(e.getValue().server!=server)return false;cancel(e.getValue());return true;});
    }
}
