"""Versioned convex primitive tessellation shared by game-engine factories.

Sphere/ellipsoid/capsule/cylinder collision hulls are finite convex
approximations. Authored inertia is retained; native solver/contact calibration
is required. MuJoCo/Webots keep their native analytic collision primitives.
"""
import math

def primitive(kind,size,segments=16,rings=8):
    if kind==6:
        vertices=[[x*size[0],y*size[1],z*size[2]] for x,y,z in [(-1,-1,-1),(1,-1,-1),(1,1,-1),(-1,1,-1),(-1,-1,1),(1,-1,1),(1,1,1),(-1,1,1)]]
        faces=[[0,2,1],[0,3,2],[4,5,6],[4,6,7],[0,1,5],[0,5,4],[1,2,6],[1,6,5],[2,3,7],[2,7,6],[3,0,4],[3,4,7]]
        return vertices,faces
    if kind==5:
        r,h=size[:2];vertices=[[r*math.cos(2*math.pi*j/segments),r*math.sin(2*math.pi*j/segments),z] for z in [-h,h] for j in range(segments)];faces=[]
        for j in range(segments):
            k=(j+1)%segments;faces.extend([[j,k,segments+k],[j,segments+k,segments+j]])
        for j in range(1,segments-1):faces.extend([[0,j+1,j],[segments,segments+j,segments+j+1]])
        return vertices,faces
    if kind not in (2,3,4):raise ValueError('unsupported game collision primitive '+str(kind))
    scale=size if kind==4 else [size[0]]*3;half=size[1] if kind==3 else 0
    vertices=[];faces=[]
    # Two hemispheres with a cylinder between equators for capsules.
    latitudes=[math.pi*i/rings for i in range(rings+1)]
    if kind==3:latitudes.insert(rings//2,math.pi/2)
    for i,theta in enumerate(latitudes):
        z=math.cos(theta)*scale[2]+(half if i<=rings//2 else -half)
        for j in range(segments):
            phi=2*math.pi*j/segments
            vertices.append([math.sin(theta)*math.cos(phi)*scale[0],math.sin(theta)*math.sin(phi)*scale[1],z])
    for i in range(len(latitudes)-1):
        for j in range(segments):
            a=i*segments+j;b=i*segments+(j+1)%segments;c=b+segments;d=a+segments
            faces.extend([[a,b,c],[a,c,d]])
    # Drop zero-area polar triangles: Chaos/PhysX reject degenerate faces.
    def valid(f):
        a,b,c=[vertices[x] for x in f];u=[b[i]-a[i] for i in range(3)];v=[c[i]-a[i] for i in range(3)]
        cross=[u[1]*v[2]-u[2]*v[1],u[2]*v[0]-u[0]*v[2],u[0]*v[1]-u[1]*v[0]]
        return sum(x*x for x in cross)>1e-30
    return vertices,[f for f in faces if valid(f)]
