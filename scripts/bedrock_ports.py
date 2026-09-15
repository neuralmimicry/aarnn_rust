"""Reserve BDS listener ports and isolate per-launch settings without changing originals."""
from contextlib import contextmanager
import errno
import ipaddress
import json
import os
from pathlib import Path
import re
import socket
import tempfile


def properties(text):
    return {k.strip():v.strip() for line in text.splitlines()
            if '=' in line and not line.lstrip().startswith('#')
            for k,v in [line.split('=',1)]}


class Ports:
    def __init__(self):self.sockets=[];self.changes={};self.endpoints=[];self.notes=[]

    def reserve(self,family,kind,host,number,dual=False,fallback=True):
        if not 0<=number<=65535:raise ValueError('Bedrock port outside 0..65535')
        sock=socket.socket(family,kind)
        try:
            if family==socket.AF_INET6:sock.setsockopt(socket.IPPROTO_IPV6,socket.IPV6_V6ONLY,0 if dual else 1)
            if os.name=='nt':sock.setsockopt(socket.SOL_SOCKET,socket.SO_EXCLUSIVEADDRUSE,1)
            try:sock.bind((host,number))
            except OSError as error:
                if not fallback or error.errno not in (errno.EADDRINUSE,errno.EACCES):raise
                sock.bind((host,0))
            self.sockets.append(sock)
            return sock.getsockname()[1]
        except BaseException:sock.close();raise

    def close(self):
        for sock in self.sockets:sock.close()
        self.sockets.clear()

    def select(self,settings):
        transport=settings.get('transport','raknet').lower()
        if transport=='raknet':
            for key,default,family,host in [('server-port',19132,socket.AF_INET,'0.0.0.0'),('server-portv6',19133,socket.AF_INET6,'::')]:
                if family==socket.AF_INET6 and not socket.has_ipv6:continue
                requested=int(settings.get(key,str(default)))
                chosen=self.reserve(family,socket.SOCK_DGRAM,host,requested)
                self.changes[key]=str(chosen);self.endpoints.append(dict(protocol='UDP',address=host,port=chosen,requested=requested))
            if settings.get('enable-lan-visibility','true').lower()=='true':
                try:
                    for family,host,number in [(socket.AF_INET,'0.0.0.0',19132),(socket.AF_INET6,'::',19133)]:
                        if family==socket.AF_INET6 and not socket.has_ipv6:continue
                        if not any(s.family==family and s.getsockname()[1]==number for s in self.sockets):
                            self.reserve(family,socket.SOCK_DGRAM,host,number,fallback=False)
                except OSError:
                    self.changes['enable-lan-visibility']='false'
                    self.notes.append('LAN broadcast disabled for this run because default discovery ports are occupied; connect using the displayed port.')
        elif transport=='nethernet':
            host=settings.get('server-ip','')
            family=socket.AF_INET6 if (':' in host or not host and socket.has_ipv6) else socket.AF_INET
            if host:ipaddress.ip_address(host)
            host=host or ('::' if family==socket.AF_INET6 else '0.0.0.0')
            requested=int(settings.get('server-port','19132'))
            chosen=self.reserve(family,socket.SOCK_STREAM,host,requested,dual=family==socket.AF_INET6 and host=='::')
            self.changes['server-port']=str(chosen)
            self.endpoints.append(dict(protocol='TCP/HTTP signaling',address=host,port=chosen,requested=requested))
            # Without a range, BDS obtains free UDP peer ports from the OS itself.
            udp=settings.get('server-udp-ports','').strip()
            if udp:
                ports=set()
                for entry in udp.split(','):
                    internal=entry.rsplit(':',1)[-1]
                    if not re.fullmatch(r'\d+(?:-\d+)?',internal):raise ValueError('Invalid server-udp-ports')
                    ends=[int(v) for v in internal.split('-')];first=ends[0];last=ends[-1]
                    if not 1<=first<=last<=65535 or last-first>4095:raise ValueError('UDP range must contain at most 4096 valid ports')
                    ports.update(range(first,last+1))
                if len(ports)>4096:raise ValueError('UDP reservation budget exceeded')
                try:
                    for number in sorted(ports):self.reserve(family,socket.SOCK_DGRAM,host,number,dual=family==socket.AF_INET6 and host=='::',fallback=False)
                except OSError as error:
                    if error.errno not in (errno.EADDRINUSE,errno.EACCES):raise
                    chosen=self.reserve(family,socket.SOCK_DGRAM,host,0,dual=family==socket.AF_INET6 and host=='::')
                    self.changes['server-udp-ports']=str(chosen)
                    self.endpoints.append(dict(protocol='UDP peer',address=host,port=chosen))
                    self.notes.append('Occupied UDP range replaced for this run; any external NAT/firewall mapping must target the displayed UDP port.')
        else:raise ValueError('Unsupported Bedrock transport: '+transport)
        return self


def lock(stream):
    if os.name=='nt':
        import msvcrt
        stream.seek(0);msvcrt.locking(stream.fileno(),msvcrt.LK_NBLCK,1)
    else:
        import fcntl
        fcntl.lockf(stream,fcntl.LOCK_EX|fcntl.LOCK_NB)


def check_world_available(source,level):
    source=Path(source).resolve()
    # Modern BDS's LevelDB fork does not always create LOCK. On Linux also
    # identify an existing server using this directory or its shared worlds.
    if Path('/proc').is_dir():
        for entry in Path('/proc').iterdir():
            if not entry.name.isdigit():continue
            try:
                if not (entry/'comm').read_text().strip().startswith('bedrock_server'):continue
                cwd=(entry/'cwd').resolve(strict=True)
                if cwd==source or (cwd/'worlds').resolve()==(source/'worlds').resolve():
                    raise RuntimeError('This Bedrock installation or its worlds are already open in another server; select a separate lab directory')
            except (OSError,ProcessLookupError):continue
    database_lock=source/'worlds'/level/'db/LOCK'
    if database_lock.is_file():
        with database_lock.open('r+b') as database:
            try:lock(database)
            except OSError:raise RuntimeError('The selected Bedrock world is already open; stop its server or select a separate lab world') from None


@contextmanager
def launch_directory(report):
    source=Path(report['directory']).resolve();reservation=Ports()
    # Kernel-held locks are released on crash. Never delete LevelDB LOCK files.
    with (source/'.aarnn-launch.lock').open('a+b') as owner:
        try:lock(owner)
        except OSError:raise RuntimeError('This Bedrock installation already has an AARNN managed launch') from None
        check_world_available(source,report['world'])
        original=(source/'server.properties').read_text();settings=properties(original)
        # Keep repeated UDP rules; all ranges must be checked.
        udp=[line.split('=',1)[1].strip() for line in original.splitlines() if line.strip().startswith('server-udp-ports=')]
        if udp:settings['server-udp-ports']=','.join(udp)
        try:
            reservation.select(settings)
            base=source/'.aarnn-runs';base.mkdir(exist_ok=True)
            directory=Path(tempfile.mkdtemp(prefix='run-',dir=base))
            for path in source.iterdir():
                if path.name.startswith('.aarnn') or path.name=='server.properties' or path.name.startswith('ContentLog'):continue
                (directory/path.name).symlink_to(path,target_is_directory=path.is_dir())
            lines=[line for line in original.splitlines() if not ('=' in line and line.split('=',1)[0].strip() in reservation.changes)]
            lines += [key+'='+value for key,value in reservation.changes.items()]
            (directory/'server.properties').write_text('\n'.join(lines)+'\n')
            info=dict(endpoints=reservation.endpoints,notes=reservation.notes,source=str(source),runtime_directory=str(directory))
            (directory/'aarnn-launch.json').write_text(json.dumps(info,indent=2)+'\n')
            print('Bedrock launch: '+json.dumps(info),flush=True)
            yield dict(report,directory=str(directory)),reservation
        finally:reservation.close()
