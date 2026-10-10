//! `aarnn-knowledge-serve <mesh.json> <listen-addr> [shards]`
//!
//! Hosts one Evelyn knowledge region as a network service, so regions can be
//! placed on any estate node. Each connection is served on its own thread.
//!
//! Wire protocol (little-endian), repeated per request on one connection:
//! - request:  `u32 n` then `n` f32 input values (`n` = region input width)
//! - response: `u32 status` (0 = ok), `u32 m`, then `m` f32 output values
//!
//! Malformed requests get a non-zero status and the connection is closed;
//! the server keeps running.

use aarnn_rust::knowledge_region::FfnRegion;
use std::io::{BufReader, BufWriter, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::process::ExitCode;
use std::sync::Arc;
use std::time::{Duration, Instant};

fn read_u32(r: &mut impl Read) -> std::io::Result<u32> {
    let mut b = [0u8; 4];
    r.read_exact(&mut b)?;
    Ok(u32::from_le_bytes(b))
}

fn serve(region: Arc<FfnRegion>, stream: TcpStream, shards: usize) -> std::io::Result<()> {
    stream.set_nodelay(true)?;
    // A request may queue behind others on a busy node, but an idle client
    // should not hold a thread for ever.
    stream.set_read_timeout(Some(Duration::from_secs(3600)))?;
    let peer = stream.peer_addr().ok();
    let mut r = BufReader::new(stream.try_clone()?);
    let mut w = BufWriter::new(stream);
    loop {
        let n = match read_u32(&mut r) {
            Ok(n) => n as usize,
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(()),
            Err(e) => return Err(e),
        };
        if n != region.inputs() {
            w.write_all(&1u32.to_le_bytes())?;
            w.write_all(&0u32.to_le_bytes())?;
            w.flush()?;
            return Ok(());
        }
        let mut buf = vec![0u8; n * 4];
        r.read_exact(&mut buf)?;
        let x: Vec<f32> = buf
            .chunks_exact(4)
            .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
            .collect();
        let t = Instant::now();
        match region.run(&x, shards) {
            Ok((y, stats)) => {
                w.write_all(&0u32.to_le_bytes())?;
                w.write_all(&(y.len() as u32).to_le_bytes())?;
                for v in &y {
                    w.write_all(&v.to_le_bytes())?;
                }
                w.flush()?;
                eprintln!(
                    "{peer:?}: {} neurons in {:.2}s",
                    stats.neurons,
                    t.elapsed().as_secs_f32()
                );
            }
            Err(e) => {
                eprintln!("{peer:?}: run failed: {e}");
                w.write_all(&2u32.to_le_bytes())?;
                w.write_all(&0u32.to_le_bytes())?;
                w.flush()?;
                return Ok(());
            }
        }
    }
}

fn main() -> ExitCode {
    let a: Vec<String> = std::env::args().collect();
    if a.len() == 2 && matches!(a[1].as_str(), "-h" | "--help") {
        println!("usage: aarnn-knowledge-serve <mesh.json> <listen-addr> [shards]");
        return ExitCode::SUCCESS;
    }
    if a.len() < 3 {
        eprintln!("usage: aarnn-knowledge-serve <mesh.json> <listen-addr> [shards]");
        return ExitCode::from(2);
    }
    let shards: usize = a.get(3).and_then(|s| s.parse().ok()).unwrap_or(0);
    let region = match FfnRegion::load(Path::new(&a[1])) {
        Ok(r) => Arc::new(r),
        Err(e) => {
            eprintln!("load mesh: {e}");
            return ExitCode::from(2);
        }
    };
    let listener = match TcpListener::bind(&a[2]) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("bind {}: {e}", a[2]);
            return ExitCode::from(2);
        }
    };
    eprintln!(
        "serving knowledge region on {} ({} -> {})",
        a[2],
        region.inputs(),
        region.outputs()
    );
    for stream in listener.incoming() {
        match stream {
            Ok(s) => {
                let region = Arc::clone(&region);
                std::thread::spawn(move || {
                    if let Err(e) = serve(region, s, shards) {
                        eprintln!("connection error: {e}");
                    }
                });
            }
            Err(e) => eprintln!("accept: {e}"),
        }
    }
    ExitCode::SUCCESS
}
