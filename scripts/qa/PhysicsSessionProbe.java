package org.neuralmimicry.minecraft;
import com.google.gson.Gson;
import java.nio.file.*;
import java.util.List;
import java.util.concurrent.TimeUnit;
/** Foreign Java client against the actual native HTTP owner; no game mocks. */
public final class PhysicsSessionProbe {
    static void complete(PhysicsSession s)throws Exception{s.completion().toCompletableFuture().get(4,TimeUnit.SECONDS);s.poll();}
    public static void main(String[] args)throws Exception {
        var config=new Gson().fromJson(Files.readString(Path.of(args[0])),PhysicsSession.Config.class);
        try(var session=new PhysicsSession(config)){
            session.fetch();complete(session);
            if(session.state()==null||session.model()==null)throw new AssertionError("native model/state not received");
            session.replaceVoxels(List.of(new double[]{2,1,.5}));complete(session);
            session.fetch();complete(session);
            if(!session.state().getAsJsonObject("obstacles").has("0"))throw new AssertionError("real collision mirror missing");
            if(!session.active())throw new AssertionError("viewer unexpectedly disarmed");
        }
        System.out.println("{\"status\":\"pass\",\"scope\":\"Java client and real native service; game rendering/tasks untested\"}");
    }
}
