package org.neuralmimicry.minecraft;

import com.google.gson.Gson;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.HashMap;
import java.util.List;
import java.util.stream.Stream;
import org.junit.jupiter.api.DynamicTest;
import org.junit.jupiter.api.Test;
import org.junit.jupiter.api.TestFactory;
import static org.junit.jupiter.api.Assertions.*;

final class ParityTest {
    record Frame(double x,double y,double heading,double[] values) {}
    record Mesh(boolean anatomy,int count,int[] indices,float[] values) {}
    record Case(String profile,Senses.Motion motion,double[] outputs,List<Frame> frames,List<Mesh> meshes) {}
    record Oracle(String digest,List<Case> cases) {}
    @TestFactory Stream<DynamicTest> everyRobotAgainstWebGL() throws Exception {
        var oracle=new Gson().fromJson(Files.readString(Path.of("build/reference.json")),Oracle.class);
        assertEquals(Content.DATA.digest(),oracle.digest());
        return oracle.cases().stream().map(c->DynamicTest.dynamicTest(c.profile(),()-> {
            var p=Content.profile(c.profile()); var h=Content.habitat(p);var history=new HashMap<String,Double>();
            for(var frame:c.frames()) {
                var pose=new Senses.Pose(frame.x(),frame.y(),frame.heading(),c.outputs());
                var values=Senses.sample(p,h,pose,history);
                assertEquals(p.sensory(),values.length);
                assertArrayEquals(frame.values(),values,1e-10,"All ordered sensory channels");
                for(double v:values) assertTrue(Double.isFinite(v)&&v>=0&&v<=1);
            }
            var motion=Senses.motion(p,c.outputs());
            assertEquals(c.motion().drive(),motion.drive(),1e-12); assertEquals(c.motion().turn(),motion.turn(),1e-12);
            for(var mesh:c.meshes()) {
                var values=Meshes.build(p.parts(),mesh.anatomy(),c.outputs(),p);
                assertEquals(mesh.count(),values.length);
                for(int i=0;i<mesh.indices().length;i++) assertEquals(mesh.values()[i],values[mesh.indices()[i]],1e-6,"Mesh component "+mesh.indices()[i]);
            }
        }));
    }
    @Test void fieldsAndWormMusclesHaveIndependentOracles() {
        var p=Content.profile("celegans");var h=Content.habitat(p);
        assertTrue(Senses.field(h,"chemical",new double[]{.42,.2,.035})>Senses.field(h,"chemical",new double[]{-.9,.9,.035})+.2);
        double[] out=new double[96];out[p.output_names().indexOf("celegans_o_095_MVULVA")]=1;
        assertEquals(new Senses.Motion(0,0),Senses.motion(p,out));
        assertEquals(-1,p.muscle_channels()[23][2]);
        assertEquals(95,p.parts().stream().filter(o->o.id().startsWith("muscle_")).count());
        assertArrayEquals(new double[]{2,6,-4},Content.toMinecraft(new double[]{1,2,3},2));
    }
    @Test void importedFlyOutputsRemainDistinct() {
        assertNotEquals(Content.profile("drosophila_banc").output_names(),Content.profile("drosophila_fafb").output_names());
    }

    @Test void hexapodMotorChannelsUseNamedEndpointsAndMoveTheirLegGeometry() {
        var p=Content.profile("hexapod");
        var expected=List.of(
                "hex_o_000_lf_coxa","hex_o_001_lf_femur","hex_o_002_lf_tibia",
                "hex_o_003_lm_coxa","hex_o_004_lm_femur","hex_o_005_lm_tibia",
                "hex_o_006_lr_coxa","hex_o_007_lr_femur","hex_o_008_lr_tibia",
                "hex_o_009_rf_coxa","hex_o_010_rf_femur","hex_o_011_rf_tibia",
                "hex_o_012_rm_coxa","hex_o_013_rm_femur","hex_o_014_rm_tibia",
                "hex_o_015_rr_coxa","hex_o_016_rr_femur","hex_o_017_rr_tibia");
        assertEquals(expected,p.output_names(),"Minecraft endpoint order must match the authored AER output map");
        assertEquals(0,Meshes.hexapodActuatorIndex(p,"lf_coxa"));
        assertEquals(8,Meshes.hexapodActuatorIndex(p,"lr_tibia"));
        assertEquals(9,Meshes.hexapodActuatorIndex(p,"rf_coxa"));
        assertEquals(17,Meshes.hexapodActuatorIndex(p,"rr_tibia"));

        var neutral=Meshes.build(p.parts(),false,new double[p.output()],p);
        for(int channel=0;channel<p.output();channel++) {
            double[] active=new double[p.output()]; active[channel]=1;
            assertFalse(java.util.Arrays.equals(neutral,Meshes.build(p.parts(),false,active,p)),
                    "motor channel "+channel+" must reach a visible leg endpoint");
        }
    }
}
