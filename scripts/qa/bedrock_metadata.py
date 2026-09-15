"""Enable Script API experiments in metadata produced by BDS for a fresh QA world.

Preserve all other native fields byte-for-byte, including storage/game versions.
Never use a hand-built subset as a complete level.dat or edit an installed world.
"""
import struct


def enable_script_experiments(raw):
    if len(raw)<12 or len(raw)>4*1024*1024 or struct.unpack_from('<I',raw,4)[0]!=len(raw)-8:
        raise ValueError('Invalid bounded Bedrock metadata header')
    offset=8
    def take(n):
        nonlocal offset
        if n<0 or offset+n>len(raw):raise ValueError('Truncated Bedrock metadata')
        value=raw[offset:offset+n];offset+=n;return value
    def number(fmt):return struct.unpack(fmt,take(struct.calcsize(fmt)))[0]
    def string():return take(number('<H')).decode('utf8')
    def skip(kind,depth):
        if depth>32:raise ValueError('NBT nesting bound')
        if kind in (1,2,3,4,5,6):take({1:1,2:2,3:4,4:8,5:4,6:8}[kind])
        elif kind==8:string()
        elif kind in (7,11,12):take(number('<i')*{7:1,11:4,12:8}[kind])
        elif kind==9:
            t=number('<B');count=number('<i')
            if not 0<=count<=65536:raise ValueError('NBT list bound')
            for _ in range(count):skip(t,depth+1)
        elif kind==10:fields(depth+1)
        else:raise ValueError('Unknown NBT tag')
    def fields(depth=0):
        result=[]
        while True:
            start=offset;kind=number('<B')
            if kind==0:return result
            name=string();payload=offset;skip(kind,depth)
            result.append((kind,name,start,offset,payload))
            if len(result)>65536:raise ValueError('NBT compound bound')
    if number('<B')!=10:raise ValueError('Expected Bedrock compound root')
    string();start=offset;entries=fields()
    if offset!=len(raw):raise ValueError('Trailing Bedrock metadata')
    flags=('gametest','experiments_ever_used','saved_with_toggled_experiments')
    existing=b''
    for kind,name,a,b,payload in entries:
        if name=='experiments':
            if kind!=10:raise ValueError('Expected experiments compound')
            offset=payload
            existing=b''.join(raw[a:b] for _,n,a,b,_ in fields() if n not in flags)
    def name_bytes(name):
        value=name.encode();return struct.pack('<H',len(value))+value
    experiments=b'\x0a'+name_bytes('experiments')+existing+b''.join(b'\x01'+name_bytes(n)+b'\x01' for n in flags)+b'\x00'
    body=raw[8:start]+b''.join(raw[a:b] for _,n,a,b,_ in entries if n!='experiments')+experiments+b'\0'
    return raw[:4]+struct.pack('<I',len(body))+body
