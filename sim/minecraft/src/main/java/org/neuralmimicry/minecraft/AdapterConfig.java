package org.neuralmimicry.minecraft;

import com.google.gson.Gson;
import com.google.gson.GsonBuilder;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.LinkedHashMap;
import java.util.Map;

/** Credentials stay in the server environment, never in entities, packets or world exports. */
public final class AdapterConfig {
    public int schemaVersion=1;
    public boolean allowLegacySandboxInference=false;
    public String endpoint="http://127.0.0.1:62620/api/aer/infer";
    public String tokenEnvironment="AARNN_MINECRAFT_TOKEN";
    public String contentDigest=Content.DATA.digest();
    public Map<String,Gateway.Binding> bindings=new LinkedHashMap<>();
    public static AdapterConfig read(Path path) throws java.io.IOException {
        if(!Files.exists(path)) {
            var example=new AdapterConfig();
            for(var p:Content.DATA.profiles()) example.bindings.put(p.id(),new Gateway.Binding("",null,p.sensory(),p.output(),0));
            Files.createDirectories(path.getParent());
            Files.writeString(path,new GsonBuilder().setPrettyPrinting().create().toJson(example));
        }
        if(Files.size(path)>16384) throw new IllegalArgumentException("Configuration too large");
        var config=new Gson().fromJson(Files.readString(path),AdapterConfig.class);
        if(config.schemaVersion!=1 || !Content.DATA.digest().equals(config.contentDigest)
                || config.bindings==null || config.bindings.size()>6)
            throw new IllegalArgumentException("Configuration schema/content mismatch; review new channel identities");
        Gateway.endpoint(config.endpoint);
        return config;
    }
}
