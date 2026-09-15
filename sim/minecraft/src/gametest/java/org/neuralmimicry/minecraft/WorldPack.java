package org.neuralmimicry.minecraft;

import com.google.gson.JsonParser;
import java.io.ByteArrayOutputStream;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.zip.ZipEntry;
import net.minecraft.nbt.CompoundTag;
import net.minecraft.nbt.ListTag;
import net.minecraft.nbt.Tag;
import net.minecraft.world.level.Level;
import net.minecraft.world.level.ChunkPos;
import net.minecraft.world.level.chunk.storage.RegionFile;
import net.minecraft.world.level.chunk.storage.RegionStorageInfo;
import java.util.zip.ZipOutputStream;
import net.minecraft.nbt.NbtAccounter;
import net.minecraft.nbt.NbtIo;
import net.minecraft.nbt.TagParser;

/** Exports only our lab chunks and neutral entities; no player, credential or game binary distribution. */
public final class WorldPack {
    public static void main(String[] args) throws Exception {
        Path run=Path.of("build/gametest"),world=run.resolve("world"),dist=Path.of("build/distributions");
        var certificate=JsonParser.parseString(Files.readString(run.resolve("clean-exit.json"))).getAsJsonObject();
        if(!certificate.get("status").getAsString().equals("pass")
                || !certificate.get("content_digest").getAsString().equals(Content.DATA.digest()))
            throw new IllegalStateException("Run the Minecraft world QA lane to certify clean shutdown");
        for(var item:certificate.getAsJsonObject("sha256").entrySet()) {
            var hash=java.security.MessageDigest.getInstance("SHA-256").digest(Files.readAllBytes(Path.of(item.getKey())));
            if(!java.util.HexFormat.of().formatHex(hash).equals(item.getValue().getAsString()))
                throw new IllegalStateException("World acceptance is stale: "+item.getKey());
        }
        var report=JsonParser.parseString(Files.readString(run.resolve("acceptance.json"))).getAsJsonObject();
        if(!Content.DATA.digest().equals(report.get("content_digest").getAsString())
                || report.getAsJsonArray("profiles").size()!=6) throw new IllegalStateException("Fresh six-profile acceptance required");
        for(var p:report.getAsJsonArray("profiles")) if(!p.getAsJsonObject().get("status").getAsString().equals("pass"))
            throw new IllegalStateException("Profile failed");
        var root=NbtIo.readCompressed(world.resolve("level.dat"),NbtAccounter.create(4*1024*1024));
        var data=root.getCompound("Data");
        data.putString("LevelName","AARNN Sensory Lab");data.putBoolean("allowCommands",true);data.putInt("GameType",1);
        data.putInt("SpawnX",0);data.putInt("SpawnY",81);data.putInt("SpawnZ",160);data.remove("Player");
        data.getCompound("GameRules").putString("spawnRadius","0");
        var generation=data.getCompound("WorldGenSettings");generation.putLong("seed",481516);
        var dimension=TagParser.parseTag("{type:\"minecraft:overworld\",generator:{type:\"minecraft:flat\",settings:{biome:\"minecraft:the_void\",lakes:0b,features:0b,layers:[],structure_overrides:[]}}}");
        generation.getCompound("dimensions").put("aarnn:lab",dimension);
        generation.getCompound("dimensions").put("minecraft:overworld",dimension.copy());
        Files.createDirectories(dist);
        try(var zip=new ZipOutputStream(Files.newOutputStream(dist.resolve("AARNN-Sensory-Lab.zip")))) {
            var buffer=new ByteArrayOutputStream();NbtIo.writeCompressed(root,buffer);add(zip,"level.dat",buffer.toByteArray());
            copyChunks(zip,world,"region","dimensions/aarnn/lab/region",-1,7,-1,4,3);
            copyChunks(zip,world,"entities","dimensions/aarnn/lab/entities",-1,7,-1,4,0);
            copyChunks(zip,world,"region","region",-1,0,9,10,4);
            add(zip,"AARNN-WORLD.txt",("Minecraft 1.21.1 + Fabric + AARNN mod required.\n/aarnn visit celegans\n/aarnn status\nContent: "+Content.DATA.digest()+"\nAll robots start disarmed. No neural checkpoints included.\n").getBytes(java.nio.charset.StandardCharsets.UTF_8));
        }
        var receipt=new com.google.gson.JsonObject();
        receipt.addProperty("content_digest",Content.DATA.digest());
        receipt.addProperty("world_sha256",sha256(dist.resolve("AARNN-Sensory-Lab.zip")));
        receipt.addProperty("certificate_sha256",sha256(run.resolve("clean-exit.json")));
        Files.writeString(dist.resolve("acceptance.json"),receipt.toString()+"\n");
        System.out.println("World package: "+dist.resolve("AARNN-Sensory-Lab.zip"));
    }
    private static String sha256(Path path) throws Exception {
        return java.util.HexFormat.of().formatHex(java.security.MessageDigest.getInstance("SHA-256").digest(Files.readAllBytes(path)));
    }
    private static void copyChunks(ZipOutputStream zip,Path world,String folder,String destination,
            int minX,int maxX,int minZ,int maxZ,int minSection) throws Exception {
        // Region files contain 32 x 32 chunks. Crop the test world's unrelated terrain
        // and entities; only the bounded lab and arrival platform are distributable.
        var info=new RegionStorageInfo("AARNN-Sensory-Lab",Level.OVERWORLD,folder);
        Path stage=Files.createTempDirectory("aarnn-region-export-");
        try {
            for(int rx=Math.floorDiv(minX,32);rx<=Math.floorDiv(maxX,32);rx++)
                for(int rz=Math.floorDiv(minZ,32);rz<=Math.floorDiv(maxZ,32);rz++) {
                    String name="r."+rx+"."+rz+".mca";Path source=world.resolve(folder).resolve(name);
                    if(!Files.exists(source))continue;
                    Path target=stage.resolve(name);
                    try(var input=new RegionFile(info,source,source.getParent(),false);
                        var output=new RegionFile(info,target,stage,false)) {
                        for(int x=minX;x<=maxX;x++)for(int z=minZ;z<=maxZ;z++) {
                            if(Math.floorDiv(x,32)!=rx||Math.floorDiv(z,32)!=rz)continue;
                            var pos=new ChunkPos(x,z);
                            try(var stream=input.getChunkDataInputStream(pos)) {
                                if(stream==null)continue;
                                CompoundTag chunk=NbtIo.read(stream,NbtAccounter.create(8*1024*1024));
                                if(folder.equals("region")) {
                                    var sections=new ListTag();
                                    for(var section:chunk.getList("sections",Tag.TAG_COMPOUND))
                                        if(((CompoundTag)section).getByte("Y")>=minSection)sections.add(section);
                                    chunk.put("sections",sections);chunk.remove("Heightmaps");chunk.putBoolean("isLightOn",false);
                                } else {
                                    var entities=new ListTag();
                                    for(var entity:chunk.getList("Entities",Tag.TAG_COMPOUND)) {
                                        String id=((CompoundTag)entity).getString("id");
                                        if(id.equals("aarnn:robot")||id.equals("aarnn:habitat"))entities.add(entity);
                                    }
                                    if(entities.isEmpty())continue;
                                    chunk.put("Entities",entities);
                                }
                                try(var streamOut=output.getChunkDataOutputStream(pos)){NbtIo.write(chunk,streamOut);}
                            }
                        }
                        output.flush();
                    }
                    add(zip,destination+"/"+name,Files.readAllBytes(target));
                }
        } finally {
            try(var files=Files.walk(stage)){for(var file:files.sorted(java.util.Comparator.reverseOrder()).toList())Files.delete(file);}
        }
    }
    private static void add(ZipOutputStream zip,String path,byte[] bytes) throws Exception {
        var entry=new ZipEntry("AARNN-Sensory-Lab/"+path);entry.setTime(0);zip.putNextEntry(entry);zip.write(bytes);zip.closeEntry();
    }
}
