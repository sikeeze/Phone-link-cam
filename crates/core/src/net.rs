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

/// True jika IP ini aman untuk bind server pairing (B1). Link-local dan
/// APIPA tetap dianggap bindable (B1 asli menyebut 169.254/16); pemilihan
/// URL pairing memakai `is_pairing_addr` yang lebih ketat.
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

/// True bila IPv6 link-local (`fe80::/10`) — butuh scope id eksplisit
/// saat bind di Windows dan tidak berguna sebagai URL pairing.
pub fn is_link_local_v6(addr: &IpAddr) -> bool {
    match addr {
        IpAddr::V6(v6) => (v6.segments()[0] & 0xFFC0) == 0xFE80,
        IpAddr::V4(_) => false,
    }
}

/// True bila IPv4 APIPA (`169.254.0.0/16`) — muncul justru saat antarmuka
/// tidak mendapat DHCP (tidak ada gateway, bukan jalur pairing).
pub fn is_apipa(addr: &IpAddr) -> bool {
    match addr {
        IpAddr::V4(v4) => {
            let o = v4.octets();
            o[0] == 169 && o[1] == 254
        }
        IpAddr::V6(_) => false,
    }
}

/// Kandidat URL pairing (B1/B9.1): IP privat `biasa` — link-local dan APIPA
/// dibuang karena tidak bisa diakses lintas perangkat tanpa scope id.
pub fn is_pairing_addr(addr: &IpAddr) -> bool {
    match addr {
        IpAddr::V4(v4) => is_private_ipv4(v4) && !is_apipa(&IpAddr::V4(*v4)),
        IpAddr::V6(v6) => v6.is_loopback() || !is_link_local_v6(&IpAddr::V6(*v6)),
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

impl InterfaceKind {
    /// Prioritas jalur B9.1: kabel (tethering/hotspot) > Wi-Fi.
    /// Angka kecil = prioritas tinggi. Dipakai untuk pengurutan jalur
    /// dan tampilan urutan prioritas di layar Koneksi.
    pub fn path_priority(&self) -> u8 {
        match self {
            InterfaceKind::Tether => 0,
            InterfaceKind::Hotspot => 1,
            InterfaceKind::Lan => 2,
        }
    }

    /// Label jenis jalur untuk UI (B9.1).
    pub fn label_id(&self) -> &'static str {
        match self {
            InterfaceKind::Lan => "Wi-Fi",
            InterfaceKind::Tether => "USB tethering",
            InterfaceKind::Hotspot => "Personal Hotspot",
        }
    }
}

/// Badge kualitas jalur dari RTT (B9.1: Baik < 30 ms, Cukup 30-80 ms,
/// Buruk > 80 ms). `None` bila RTT belum diukur.
pub fn quality_badge(rtt_ms: Option<u32>) -> Option<&'static str> {
    match rtt_ms {
        Some(rtt) if rtt < 30 => Some("Baik"),
        Some(rtt) if rtt <= 80 => Some("Cukup"),
        Some(_) => Some("Buruk"),
        None => None,
    }
}

/// Daftar antarmuka privat yang bisa dipakai server pairing (B1/B9.1).
///
/// IPv6 link-local (`fe80::/10`) dan IPv4 APIPA (`169.254.0.0/16`) dibuang:
/// link-local butuh scope id eksplisit (gagal di-bind di Windows dan tidak
/// bisa dipakai HP sebagai URL pairing tanpa scope), sedangkan APIPA muncul
/// justru saat antarmuka tidak mendapat DHCP.
pub fn list_private_interfaces() -> Result<Vec<NetworkInterface>> {
    let ifas = local_ip_address::list_afinet_netifas()
        .map_err(|e| Error::Net(format!("gagal enumerasi antarmuka: {e}")))?;
    let mut out: Vec<NetworkInterface> = Vec::new();
    for (name, ip) in ifas {
        if !is_pairing_addr(&ip) {
            continue;
        }
        // B9.1: klasifikasi subnet tethering/hotspot berdasarkan IP (B9.3),
        // bukan nama antarmuka (nama berbeda-beda per OS).
        let kind = match &ip {
            IpAddr::V4(v4) => classify_subnet(v4),
            IpAddr::V6(_) => InterfaceKind::Lan,
        };
        if let Some(existing) = out.iter_mut().find(|i| i.name == name) {
            existing.addrs.push(ip);
            // B9.1: satu antarmuka boleh beberapa IP; ambil jenis tertinggi
            // (Tether/Hotspot menang atas Lan) untuk label.
            if kind != InterfaceKind::Lan {
                existing.kind = kind;
            }
        } else {
            out.push(NetworkInterface {
                name,
                addrs: vec![ip],
                kind,
                status: InterfaceStatus::Active,
            });
        }
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

/// Pilih IP bind untuk peer connection WebRTC menuju HP tertentu (B1).
///
/// Mencari antarmuka privat lokal yang berada satu subnet /24 dengan `peer`
/// (kasus LAN umum). Jika tidak ada yang cocok, kembalikan IP privat pertama
/// yang bisa di-bind; kalau itu pun tidak ada, kembalikan 127.0.0.1 agar
/// tetap berfungsi dalam mode ADB/uji.
pub fn bind_ip_for_peer(peer: &IpAddr) -> IpAddr {
    let ifas = match list_private_interfaces() {
        Ok(v) => v,
        Err(_) => return IpAddr::V4(std::net::Ipv4Addr::LOCALHOST),
    };
    let all: Vec<IpAddr> = ifas.iter().flat_map(|i| i.addrs.clone()).collect();
    if let IpAddr::V4(p) = peer {
        let po = p.octets();
        // Ponytail: asumsi /24 (LAN rumah dan tethering umumnya /24).
        // Subnet lain tetap bekerja lewat fallback di bawah.
        if let Some(m) = all.iter().find(|a| match **a {
            IpAddr::V4(l) => {
                l.octets()[0] == po[0] && l.octets()[1] == po[1] && l.octets()[2] == po[2]
            }
            _ => false,
        }) {
            return *m;
        }
    }
    all.into_iter()
        .find(|a| matches!(a, IpAddr::V4(v) if !v.is_loopback()))
        .unwrap_or(IpAddr::V4(std::net::Ipv4Addr::LOCALHOST))
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
    fn classify_multi_ip_interface_picks_tether() {
        // B9.1: antarmuka dengan IP LAN + tether harus dilabel Tether.
        let mut ifa = NetworkInterface {
            name: "usb0".into(),
            addrs: vec!["192.168.1.5".parse().unwrap()],
            kind: InterfaceKind::Lan,
            status: InterfaceStatus::Active,
        };
        let kind = classify_subnet(&"192.168.42.9".parse::<Ipv4Addr>().unwrap());
        ifa.addrs.push("192.168.42.9".parse().unwrap());
        if kind != InterfaceKind::Lan {
            ifa.kind = kind;
        }
        assert_eq!(ifa.kind, InterfaceKind::Tether);
    }

    #[test]
    fn path_priority_cable_beats_wifi_b91() {
        // B9.1: kabel (tether/hotspot) > Wi-Fi.
        assert!(InterfaceKind::Tether.path_priority() < InterfaceKind::Lan.path_priority());
        assert!(InterfaceKind::Hotspot.path_priority() < InterfaceKind::Lan.path_priority());
    }

    #[test]
    fn quality_badge_thresholds_b91() {
        // B9.1: Baik < 30 ms, Cukup 30-80 ms, Buruk > 80 ms.
        assert_eq!(quality_badge(Some(10)), Some("Baik"));
        assert_eq!(quality_badge(Some(29)), Some("Baik"));
        assert_eq!(quality_badge(Some(30)), Some("Cukup"));
        assert_eq!(quality_badge(Some(80)), Some("Cukup"));
        assert_eq!(quality_badge(Some(81)), Some("Buruk"));
        assert_eq!(quality_badge(Some(500)), Some("Buruk"));
        assert_eq!(quality_badge(None), None);
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

    #[test]
    fn pairing_addr_rejects_link_local_and_apipa() {
        // fe80:: dan 169.254.x.x tidak berguna sebagai URL pairing (B1).
        assert!(!is_pairing_addr(&"fe80::1".parse::<IpAddr>().unwrap()));
        assert!(!is_pairing_addr(&"169.254.1.1".parse::<IpAddr>().unwrap()));
        assert!(is_pairing_addr(&"192.168.1.1".parse::<IpAddr>().unwrap()));
        assert!(is_pairing_addr(&"::1".parse::<IpAddr>().unwrap()));
        assert!(!is_pairing_addr(&"8.8.8.8".parse::<IpAddr>().unwrap()));
    }

    #[test]
    fn bind_ip_for_peer_returns_bindable_private() {
        // Mode ADB: HP di 127.0.0.1 → fallback ke IP privat non-loopback
        // atau 127.0.0.1 jika tidak ada antarmuka lain.
        let b = bind_ip_for_peer(&"127.0.0.1".parse::<IpAddr>().unwrap());
        assert!(is_bindable(&b), "{b:?} harus private");
    }

    #[test]
    fn bind_ip_for_peer_error_falls_back_to_loopback() {
        // IP publik yang tidak cocok subnet mana pun tetap harus memberikan
        // IP yang bisa di-bind (B1), bukan panic.
        let b = bind_ip_for_peer(&"8.8.8.8".parse::<IpAddr>().unwrap());
        assert!(is_bindable(&b), "{b:?} harus private");
    }
}
