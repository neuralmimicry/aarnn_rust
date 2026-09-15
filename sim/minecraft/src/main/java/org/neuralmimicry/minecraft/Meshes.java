package org.neuralmimicry.minecraft;

import java.util.ArrayList;
import java.util.List;
import java.util.Map;

/** Catalogue-native triangles. Resolution and diffuse shading match the WebGL reference. */
public final class Meshes {
    private Meshes() {}
    private static final Map<String,List<double[]>> SHAPES=Map.of("box",shape("box"),"sphere",shape("sphere"),"cylinder",shape("cylinder"));
    private static void tri(List<double[]> out,double[] a,double[] b,double[] c) { out.add(a);out.add(b);out.add(c); }
    private static double[] point(double lat,double lon) { return new double[]{.5*Math.cos(lat)*Math.cos(lon),.5*Math.cos(lat)*Math.sin(lon),.5*Math.sin(lat)}; }
    private static List<double[]> shape(String shape) {
        var out=new ArrayList<double[]>();
        if(shape.equals("box")) {
            double[][] p={{-.5,-.5,-.5},{.5,-.5,-.5},{.5,.5,-.5},{-.5,.5,-.5},{-.5,-.5,.5},{.5,-.5,.5},{.5,.5,.5},{-.5,.5,.5}};
            int[][] faces={{0,1,2},{0,2,3},{4,6,5},{4,7,6},{0,4,5},{0,5,1},{1,5,6},{1,6,2},{2,6,7},{2,7,3},{3,7,4},{3,4,0}};
            for(var f:faces) tri(out,p[f[0]],p[f[1]],p[f[2]]);
        } else if(shape.equals("cylinder")) {
            for(int i=0;i<16;i++) {
                double a=2*Math.PI*i/16,b=2*Math.PI*(i+1)/16;
                double[] p={.5*Math.cos(a),.5*Math.sin(a),-.5},q={.5*Math.cos(b),.5*Math.sin(b),-.5};
                double[] r={q[0],q[1],.5},s={p[0],p[1],.5};
                tri(out,p,q,r);tri(out,p,r,s);tri(out,new double[]{0,0,.5},s,r);tri(out,new double[]{0,0,-.5},q,p);
            }
        } else {
            for(int j=0;j<8;j++) for(int k=0;k<12;k++) {
                double l=-Math.PI/2+j*Math.PI/8,m=l+Math.PI/8,u=k*2*Math.PI/12,v=(k+1)*2*Math.PI/12;
                tri(out,point(l,u),point(l,v),point(m,v));tri(out,point(l,u),point(m,v),point(m,u));
            }
        }
        return out;
    }
    private static double at(double[] a,int i) { return i<0||i>=a.length?0:a[i]; }
    public static float[] build(List<Content.Part> objects,boolean anatomy,double[] actuators,Content.Profile profile) {
        var data=new ArrayList<Float>();
        for(var o:objects) {
            if(o.internal()&&!anatomy) continue;
            if(anatomy && ((profile.kind().equals("worm")&&o.id().startsWith("cuticle_"))
                    || (profile.kind().equals("fish")&&o.id().startsWith("myomere_")))) continue;
            double bend=0;
            if(o.anchor().startsWith("segment_") && actuators.length>0) {
                int segment=Integer.parseInt(o.anchor().substring(8));
                if(profile.kind().equals("worm")) {
                    var ids=profile.muscle_channels()[segment];
                    bend=(at(actuators,ids[0])+at(actuators,ids[1])-at(actuators,ids[2])-at(actuators,ids[3]))*.025;
                } else bend=(at(actuators,(segment%8)*2)-at(actuators,(segment%8)*2+1))*.035;
            }
            double c=Math.cos(o.yaw()),s=Math.sin(o.yaw());
            var base=SHAPES.get(o.shape());
            for(int i=0;i<base.size();i+=3) {
                double[][] vertices=new double[3][3];
                for(int j=0;j<3;j++) {
                    var p=base.get(i+j); double x=p[0]*o.size()[0],y=p[1]*o.size()[1];
                    vertices[j]=new double[]{o.position()[0]+x*c-y*s,o.position()[1]+x*s+y*c+bend,o.position()[2]+p[2]*o.size()[2]};
                }
                double[] a=new double[3],b=new double[3];
                for(int j=0;j<3;j++) { a[j]=vertices[1][j]-vertices[0][j];b[j]=vertices[2][j]-vertices[0][j]; }
                double nx=a[1]*b[2]-a[2]*b[1],ny=a[2]*b[0]-a[0]*b[2],nz=a[0]*b[1]-a[1]*b[0];
                double length=Math.sqrt(nx*nx+ny*ny+nz*nz); if(length==0) length=1;
                double light=o.material().equals("emissive")?1:.44+.56*Math.abs((nx*-.35+ny*-.5+nz)/(length*Math.sqrt(.35*.35+.25+1)));
                for(var vertex:vertices) {
                    for(double value:vertex) data.add((float)value);
                    for(double value:o.colour()) data.add((float)(value*light));
                }
            }
        }
        float[] result=new float[data.size()];for(int i=0;i<result.length;i++) result[i]=data.get(i);
        return result;
    }
}
