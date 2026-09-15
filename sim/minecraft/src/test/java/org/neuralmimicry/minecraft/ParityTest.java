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
}
