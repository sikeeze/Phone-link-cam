//! Compositor (B6): scale ke ukuran output tetap, mode fit/fill, transform
//! mirror/rotate/zoom digital.

use crate::CompositorConfig;

/// Konfigurasi output composator untuk satu frame (B6).
#[derive(Debug, Clone)]
pub struct ComposeRequest {
    pub source_width: u32,
    pub source_height: u32,
    pub config: CompositorConfig,
}

/// Hasil composite: frame siap untuk virtual camera (B6).
#[derive(Debug, Clone)]
pub struct ComposedFrame {
    pub data: bytes::Bytes,
    pub width: u32,
    pub height: u32,
}

/// Hitung geometry target setelah transform (B6). Pure function, mudah diuji.
pub fn target_geometry(req: &ComposeRequest) -> (u32, u32) {
    let (sw, sh) = apply_rotate_zoom(req);
    // Rotasi 90/270 memutar output juga (B6): portrait in, portrait out.
    let (ow, oh) = if req.config.rotate % 180 == 90 {
        (req.config.height, req.config.width)
    } else {
        (req.config.width, req.config.height)
    };

    match req.config.fit {
        crate::FitMode::Fit => {
            // Letterbox: muat seluruh sumber ke output.
            let scale = (ow as f64 / sw as f64).min(oh as f64 / sh as f64);
            let w = (sw as f64 * scale).round().max(1.0) as u32;
            let h = (sh as f64 * scale).round().max(1.0) as u32;
            (w, h)
        }
        crate::FitMode::Fill => {
            // Crop: isi output, potong yang berlebih.
            let scale = (ow as f64 / sw as f64).max(oh as f64 / sh as f64);
            let w = (sw as f64 * scale).round().max(1.0) as u32;
            let h = (sh as f64 * scale).round().max(1.0) as u32;
            (w, h)
        }
    }
}

fn apply_rotate_zoom(req: &ComposeRequest) -> (u32, u32) {
    let zoom = req.config.zoom.max(0.05) as f64;
    let sw = (req.source_width as f64 / zoom).round().max(1.0) as u32;
    let sh = (req.source_height as f64 / zoom).round().max(1.0) as u32;
    if req.config.rotate % 180 == 90 {
        (sh, sw)
    } else {
        (sw, sh)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req(sw: u32, sh: u32, cfg: CompositorConfig) -> ComposeRequest {
        ComposeRequest {
            source_width: sw,
            source_height: sh,
            config: cfg,
        }
    }

    #[test]
    fn fit_16_9_into_16_9_no_change() {
        let cfg = CompositorConfig::default(); // 1280x720
        let g = target_geometry(&req(1280, 720, cfg));
        assert_eq!(g, (1280, 720));
    }

    #[test]
    fn fit_4_3_into_16_9_letterboxes() {
        let cfg = CompositorConfig::default(); // 1280x720
        let g = target_geometry(&req(640, 480, cfg));
        assert_eq!(g, (960, 720));
    }

    #[test]
    fn fill_crops_instead_of_letterboxing() {
        let cfg = CompositorConfig::default();
        let g = target_geometry(&req(640, 480, cfg.clone()));
        assert_eq!(g, (960, 720)); // fit
        let mut fill_cfg = cfg;
        fill_cfg.fit = crate::FitMode::Fill;
        let g = target_geometry(&req(640, 480, fill_cfg));
        assert_eq!(g, (1280, 960)); // fill: lebih besar dari output, dipotong
    }

    #[test]
    fn rotate_90_swaps_dimensions() {
        let cfg = CompositorConfig::default();
        let mut r = req(1280, 720, cfg.clone());
        r.config.rotate = 90;
        let g = target_geometry(&r);
        assert_eq!(g, (720, 1280));
    }

    #[test]
    fn zoom_in_shrinks_source() {
        let cfg = CompositorConfig::default();
        let mut r = req(1280, 720, cfg);
        r.config.zoom = 2.0;
        // Zoom memperkecil wilayah sumber; output tetap 1280x720 (fixed, B6).
        assert_eq!(apply_rotate_zoom(&r), (640, 360));
        assert_eq!(target_geometry(&r), (1280, 720));
    }
}
