//! Diagnostik bind port (jalankan: cargo run -p phonecam-desktop --example netdiag).
fn main() {
    let ifaces = phonecam_core::net::list_private_interfaces().unwrap_or_default();
    println!("antarmuka privat: {:?}", ifaces);
    let mut ips = Vec::new();
    for i in ifaces {
        for ip in i.addrs {
            if !ips.contains(&ip) {
                ips.push(ip);
            }
        }
    }
    println!("bind ips: {:?}", ips);
    if ips.is_empty() {
        println!("TIDAK ADA IP -> serve() gagal di sini");
        return;
    }
    for p in 8443..8448 {
        let res: Vec<_> = ips
            .iter()
            .map(|ip| {
                let r = std::net::TcpListener::bind(std::net::SocketAddr::new(*ip, p));
                r.map(|l| {
                    drop(l);
                    "OK"
                })
                .unwrap_or("FAIL")
            })
            .collect();
        println!("port {p} -> {res:?}");
    }
}
