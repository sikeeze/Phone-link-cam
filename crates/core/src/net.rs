//! Utilitas jaringan: antarmuka privat, bind policy (README B1/B9).
//!
//! Aturan B1: bind hanya ke antarmuka privat (10/8, 172.16/12, 192.168/16,
//! link-local), termasuk subnet tethering dan hotspot. Jangan bind ke publik.
//! Aturan A9: tidak ada cfg(target_os) di crate ini.

use crate::error::{Error, Result};
use serde::{Deserialize, Serialize};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};

/// Representasi string subnet privat (B1).
pub fn is_private_ipv4(addr: &Ipv4Addr) -> bool {
    let o = addr.octets();
    // 10/8
    o[0] == 10
    // 172.16/12
        || (o[0] == 172 && (o[1] & 0xF0) == 0x10)
    // 192.168/16
        || (o[0] == 192 && o[1] == 168)
    // link-local 169.254/16 (B1, juga digunakan oleh beberapa mode hotspot)
        || (o[0] == 169 && o[1] == 254)
    // loopback untuk pengujian dan mode ADB (B9.2: 127.0.0.1:8080)
        || o[0] == 127
}

/// True jika IP ini aman untuk bind server pairing (B1).
pub fn is_bindable(addr: &IpAddr) -> bool {
    match addr {
        IpAddr::V4(v4) => is_private_ipv4(v4),
        // IPv6: terima link-local fe80::/10 dan loopback ::1 saja (B1).
        IpAddr::V6(v6) => {
            let seg = v6.segments();
            (seg[0] & 0xFFC0) == 0xFE80 || v6.is_loopback()
        }
    }
}

/// Antarmuka jaringan dengan statusnya (B9.1).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NetworkInterface {
    pub name: String,
    pub addrs: Vec<IpAddr>,
    /// Jenis jalur yang terdeteksi (B9.1).
    pub kind: InterfaceKind,
    /// Status antarmuka (B9.1: Aktif, Terdeteksi, Perlu setup, Tidak tersedia).
    pub status: InterfaceStatus,
}

/// Jenis antarmuka berdasarkan kemampuan jalur (B9).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum InterfaceKind {
    /// Wi-Fi / Ethernet LAN.
    #[default]
    Lan,
    /// USB tethering Android (umumnya 192.168.42.x, B9.3).
    Tether,
    /// USB Personal Hotspot iPhone (umumnya 172.20.10.x, B9.3).
    Hotspot,
}

/// Status jalur (B9.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum InterfaceStatus {
    /// Sudah memiliki IP dan bisa melayani pairing.
    #[default]
    Active,
    /// Terdeteksi tapi belum siap (mis. tethering tanpa IP, B9.4 TETHER_NO_IP).
    NeedsSetup,
    /// Ada secara fisik tetapi tidak bisa dipakai (mis. hotspot mati).
    Unavailable,
}

impl InterfaceStatus {
    pub fn label_id(&self) -> &'static str {
        match self {
            InterfaceStatus::Active => "Aktif",
            InterfaceStatus::NeedsSetup => "Perlu setup",
            InterfaceStatus::Unavailable => "Tidak tersedia",
        }
    }
}

/// Daftar antarmuka privat yang bisa dipakai server pairing (B1/B9.1).
/// Menggunakan local_ip_address::list_afinet_netifas (terverifikasi di docs.rs).
pub fn list_private_interfaces() -> Result<Vec<NetworkInterface>> {
    let ifas = local_ip_address::list_afinet_netifas()
        .map_err(|e| Error::Net(format!("gagal enumerasi antarmuka: {e}")))?;
    let mut out: Vec<NetworkInterface> = Vec::new();
    for (name, ip) in ifas {
        if !is_bindable(&ip) {
            continue;
        }
        if let Some(existing) = out.iter_mut().find(|i| i.name == name) {
            existing.addrs.push(ip);
        } else {
            out.push(NetworkInterface {
                name,
                addrs: vec![ip],
                kind: InterfaceKind::Lan,
                status: InterfaceStatus::Active,
            });
        }
        // catatan: pengklasifikasian Tether/Hotspot memerlukan info tambahan
        // (subnet 192.168.42.0/24 dan 172.20.10.0/28, B9.3); akan ditambahkan
        // saat Connection Manager fase 3B mengikat deteksi USB.
    }
    Ok(out)
}

/// Klasifikasi subnet tethering (B9.3): kenali subnet umum tetapi jangan
/// bergantung pada nama antarmuka.
pub fn classify_subnet(addr: &Ipv4Addr) -> InterfaceKind {
    let o = addr.octets();
    if o[0] == 192 && o[1] == 168 && o[2] == 42 {
        InterfaceKind::Tether
    } else if o[0] == 172 && o[1] == 20 {
        InterfaceKind::Hotspot
    } else {
        InterfaceKind::Lan
    }
}

/// Alamat listen untuk server HTTPS: bind ke semua antarmuka privat (B1).
/// Mengembalikan daftar SocketAddr per IP privat.
pub fn https_bind_addrs(port: u16) -> Result<Vec<SocketAddr>> {
    let ifas = list_private_interfaces()?;
    let mut addrs = Vec::new();
    for iface in ifas {
        for ip in iface.addrs {
            if is_bindable(&ip) {
                addrs.push(SocketAddr::new(ip, port));
            }
        }
    }
    Ok(addrs)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v4(s: &str) -> IpAddr {
        s.parse().unwrap()
    }

    #[test]
    fn private_subnets_are_bindable() {
        for s in [
            "10.0.0.1",
            "10.255.255.254",
            "172.16.0.1",
            "172.31.255.254",
            "192.168.1.24",
            "169.254.1.1",
            "127.0.0.1",
        ] {
            assert!(is_bindable(&v4(s)), "{s} harusnya private/bindable");
        }
    }

    #[test]
    fn public_subnets_not_bindable() {
        for s in [
            "8.8.8.8",
            "1.1.1.1",
            "172.32.0.1",
            "172.15.0.1",
            "192.169.1.1",
            "11.0.0.1",
        ] {
            assert!(!is_bindable(&v4(s)), "{s} harusnya public");
        }
    }

    #[test]
    fn ipv6_link_local_and_loopback_bindable() {
        assert!(is_bindable(&"fe80::1".parse::<IpAddr>().unwrap()));
        assert!(is_bindable(&"::1".parse::<IpAddr>().unwrap()));
        assert!(!is_bindable(&"2606:4700::1".parse::<IpAddr>().unwrap()));
    }

    #[test]
    fn subnet_classification_b93() {
        assert_eq!(
            classify_subnet(&"192.168.42.129".parse().unwrap()),
            InterfaceKind::Tether
        );
        assert_eq!(
            classify_subnet(&"172.20.10.5".parse().unwrap()),
            InterfaceKind::Hotspot
        );
        assert_eq!(
            classify_subnet(&"192.168.1.24".parse().unwrap()),
            InterfaceKind::Lan
        );
    }

    #[test]
    fn list_private_interfaces_runs() {
        // Pada mesin uji umumnya ada loopback dan minimal satu antarmuka.
        let ifas = list_private_interfaces();
        assert!(ifas.is_ok(), "{ifas:?}");
    }
}
