use std::{
    collections::{HashMap, HashSet, VecDeque},
    path::PathBuf,
    sync::{Arc, mpsc},
    time::{Duration, Instant, UNIX_EPOCH},
};

use gpui::{RenderImage, Size};
use image::{Frame, RgbaImage};

use crate::image_budget::{release_image_bytes, try_reserve_image_bytes};
use crate::terminal_ui::{TerminalBackgroundFit, TerminalBackgroundPreferences};

const DEFAULT_BACKGROUND_IMAGE_CACHE_BYTES: usize = 64 * 1024 * 1024;
const BACKGROUND_METADATA_RECHECK_INTERVAL: Duration = Duration::from_secs(2);
// Quantizes the display target so small viewport changes reuse the cached
// texture instead of re-decoding the background on every intermediate resize.
const BACKGROUND_TARGET_ALIGN: u32 = 64;

/// The device-pixel size the background image should be downscaled to.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct BackgroundImageTargetSize {
    width: u32,
    height: u32,
}

impl BackgroundImageTargetSize {
    pub(crate) fn new(width: u32, height: u32) -> Self {
        Self { width, height }
    }
}

/// Quantizes a logical display size to an aligned device-pixel target so the
/// background cache key stays stable during incremental window resizes.
pub fn background_display_target(size: Size<gpui::Pixels>, scale_factor: f32) -> BackgroundImageTargetSize {
    let device = |logical: gpui::Pixels| ((logical.as_f32() * scale_factor).ceil().max(1.0) as u32);
    BackgroundImageTargetSize {
        width: align_up(device(size.width)),
        height: align_up(device(size.height)),
    }
}

fn align_up(value: u32) -> u32 {
    let value = u64::from(value);
    let align = u64::from(BACKGROUND_TARGET_ALIGN);
    ((value + align - 1) / align * align) as u32
}

/// Computes the exact pixel size to decode the background into so the GPU's
/// object-fit pass never has to scale a texture by more than the quantized
/// margin. The aspect ratio always follows the source image; only the scale is
/// derived from the display target.
pub(crate) fn background_image_resize_target(
    source: (u32, u32),
    display: BackgroundImageTargetSize,
    fit: TerminalBackgroundFit,
) -> (u32, u32) {
    let (source_width, source_height) = source;
    if source_width == 0 || source_height == 0 {
        return (1, 1);
    }
    let source_width = f64::from(source_width);
    let source_height = f64::from(source_height);
    let display_width = f64::from(display.width);
    let display_height = f64::from(display.height);
    if display_width == 0.0 || display_height == 0.0 {
        return (source_width as u32, source_height as u32);
    }
    let target = match fit {
        TerminalBackgroundFit::Fill => (display_width, display_height),
        TerminalBackgroundFit::Cover => {
            let scale = (display_width / source_width).max(display_height / source_height);
            (source_width * scale, source_height * scale)
        }
        TerminalBackgroundFit::Contain => {
            let scale = (display_width / source_width).min(display_height / source_height);
            (source_width * scale, source_height * scale)
        }
        TerminalBackgroundFit::Tile => {
            let source_longest = source_width.max(source_height);
            let display_longest = display_width.max(display_height);
            let scale = display_longest / source_longest;
            (source_width * scale, source_height * scale)
        }
    };
    (
        target.0.ceil().max(1.0) as u32,
        target.1.ceil().max(1.0) as u32,
    )
}

pub struct BackgroundImageRenderCache {
    entries: HashMap<BackgroundImageCacheKey, CachedBackgroundImage>,
    key_cache: HashMap<BackgroundImageRequestKey, CachedBackgroundImageKey>,
    order: VecDeque<BackgroundImageCacheKey>,
    pending: HashSet<BackgroundImageCacheKey>,
    retired_images: Vec<Arc<RenderImage>>,
    sender: mpsc::Sender<BackgroundImageLoadResult>,
    receiver: mpsc::Receiver<BackgroundImageLoadResult>,
    bytes: usize,
    byte_limit: usize,
}

struct CachedBackgroundImage {
    image: Arc<RenderImage>,
    bytes: usize,
}

struct CachedBackgroundImageKey {
    key: BackgroundImageCacheKey,
    checked_at: Instant,
}

enum BackgroundImageLoadResult {
    Loaded {
        key: BackgroundImageCacheKey,
        image: Arc<RenderImage>,
        bytes: usize,
    },
    Failed {
        key: BackgroundImageCacheKey,
    },
}

#[derive(Clone, Hash, Eq, PartialEq)]
struct BackgroundImageRequestKey {
    path: PathBuf,
    blur_millis: u32,
    target: BackgroundImageTargetSize,
}

#[derive(Clone, Hash, Eq, PartialEq)]
struct BackgroundImageCacheKey {
    path: PathBuf,
    blur_millis: u32,
    target: BackgroundImageTargetSize,
    modified_millis: Option<u128>,
    len: Option<u64>,
}

impl BackgroundImageRenderCache {
    pub fn set_byte_limit(&mut self, byte_limit: usize) {
        self.byte_limit = byte_limit;
        self.evict_over_budget();
    }

    pub fn take_retired_images(&mut self) -> Vec<Arc<RenderImage>> {
        std::mem::take(&mut self.retired_images)
    }

    pub fn render_background_image(
        &mut self,
        background: &TerminalBackgroundPreferences,
        target: BackgroundImageTargetSize,
    ) -> Option<Arc<RenderImage>> {
        self.drain_completed();

        let key = self.cached_key_for_background(background, target);
        if self.entries.contains_key(&key) {
            self.touch(&key);
            return self.entries.get(&key).map(|entry| entry.image.clone());
        }

        if self.pending.insert(key.clone()) {
            let sender = self.sender.clone();
            let background = background.clone();
            std::thread::spawn(move || {
                let result = match load_background_image(key.clone(), &background, target) {
                    Some((image, bytes)) => BackgroundImageLoadResult::Loaded { key, image, bytes },
                    None => BackgroundImageLoadResult::Failed { key },
                };
                let _ = sender.send(result);
            });
        }

        None
    }

    fn cached_key_for_background(
        &mut self,
        background: &TerminalBackgroundPreferences,
        target: BackgroundImageTargetSize,
    ) -> BackgroundImageCacheKey {
        let request = BackgroundImageRequestKey::new(background, target);
        if let Some(cached) = self.key_cache.get(&request)
            && cached.checked_at.elapsed() < BACKGROUND_METADATA_RECHECK_INTERVAL
        {
            return cached.key.clone();
        }

        // The cache key includes file metadata so a changed image is eventually
        // reloaded, but metadata() must not run on every render/scroll frame.
        let key = BackgroundImageCacheKey::new(background, target);
        // Key entries are keyed by the quantized target size, so they would
        // grow without bound across every distinct resize step. Entries older
        // than the recheck interval are useless anyway, so drop them on insert.
        self.key_cache
            .retain(|_, cached| cached.checked_at.elapsed() < BACKGROUND_METADATA_RECHECK_INTERVAL);
        self.key_cache.insert(
            request,
            CachedBackgroundImageKey {
                key: key.clone(),
                checked_at: Instant::now(),
            },
        );
        key
    }

    pub fn drain_completed(&mut self) -> bool {
        let mut changed = false;
        while let Ok(result) = self.receiver.try_recv() {
            match result {
                BackgroundImageLoadResult::Loaded { key, image, bytes } => {
                    self.pending.remove(&key);
                    if let Some(existing) = self.entries.remove(&key) {
                        self.bytes = self.bytes.saturating_sub(existing.bytes);
                        release_image_bytes(existing.bytes);
                        self.retired_images.push(existing.image);
                    }
                    self.evict_for_admission(bytes);
                    if !try_reserve_image_bytes(bytes) {
                        self.retired_images.push(image);
                        changed = true;
                        continue;
                    }
                    self.entries.insert(
                        key.clone(),
                        CachedBackgroundImage {
                            image: image.clone(),
                            bytes,
                        },
                    );
                    self.touch(&key);
                    self.bytes += bytes;
                    self.evict_over_budget();
                    changed = true;
                }
                BackgroundImageLoadResult::Failed { key } => {
                    self.pending.remove(&key);
                    self.key_cache.retain(|_, cached| cached.key != key);
                    changed = true;
                }
            }
        }
        changed
    }

    pub fn has_pending(&self) -> bool {
        !self.pending.is_empty()
    }

    fn touch(&mut self, key: &BackgroundImageCacheKey) {
        self.order.retain(|existing| existing != key);
        self.order.push_back(key.clone());
    }

    fn evict_over_budget(&mut self) {
        while self.bytes > self.byte_limit {
            let Some(key) = self.order.pop_front() else {
                self.bytes = 0;
                break;
            };
            if let Some(entry) = self.entries.remove(&key) {
                self.bytes = self.bytes.saturating_sub(entry.bytes);
                release_image_bytes(entry.bytes);
                self.retired_images.push(entry.image);
            }
        }
    }

    fn evict_for_admission(&mut self, bytes: usize) {
        while self.bytes.saturating_add(bytes) > self.byte_limit {
            let Some(key) = self.order.pop_front() else {
                break;
            };
            if let Some(entry) = self.entries.remove(&key) {
                self.bytes = self.bytes.saturating_sub(entry.bytes);
                release_image_bytes(entry.bytes);
                self.retired_images.push(entry.image);
            }
        }
    }
}

impl Drop for BackgroundImageRenderCache {
    fn drop(&mut self) {
        release_image_bytes(self.bytes);
        self.bytes = 0;
    }
}

impl Default for BackgroundImageRenderCache {
    fn default() -> Self {
        let (sender, receiver) = mpsc::channel();
        Self {
            entries: HashMap::new(),
            key_cache: HashMap::new(),
            order: VecDeque::new(),
            pending: HashSet::new(),
            retired_images: Vec::new(),
            sender,
            receiver,
            bytes: 0,
            byte_limit: DEFAULT_BACKGROUND_IMAGE_CACHE_BYTES,
        }
    }
}

impl BackgroundImageRequestKey {
    fn new(
        background: &TerminalBackgroundPreferences,
        target: BackgroundImageTargetSize,
    ) -> Self {
        Self {
            path: background.path.clone(),
            blur_millis: (background.blur.max(0.0) * 1000.0).round() as u32,
            target,
        }
    }
}

impl BackgroundImageCacheKey {
    fn new(
        background: &TerminalBackgroundPreferences,
        target: BackgroundImageTargetSize,
    ) -> Self {
        let metadata = background.path.metadata().ok();
        let modified_millis = metadata
            .as_ref()
            .and_then(|metadata| metadata.modified().ok())
            .and_then(|modified| modified.duration_since(UNIX_EPOCH).ok())
            .map(|duration| duration.as_millis());

        Self {
            path: background.path.clone(),
            blur_millis: (background.blur.max(0.0) * 1000.0).round() as u32,
            target,
            modified_millis,
            len: metadata.map(|metadata| metadata.len()),
        }
    }
}

fn convert_rgba_pixels_to_gpui_bgra(pixels: &mut [u8]) {
    // GPUI 0.2.2 RenderImage consumes BGRA bytes. Keep the app-facing image
    // data in normal RGBA and isolate the texture contract here.
    for pixel in pixels.chunks_exact_mut(4) {
        pixel.swap(0, 2);
    }
}

fn load_background_image(
    key: BackgroundImageCacheKey,
    background: &TerminalBackgroundPreferences,
    target: BackgroundImageTargetSize,
) -> Option<(Arc<RenderImage>, usize)> {
    if key != BackgroundImageCacheKey::new(background, target) {
        return None;
    }

    let image = image::open(&background.path).ok()?;
    let (source_width, source_height) = image.dimensions();
    let (resize_width, resize_height) =
        background_image_resize_target((source_width, source_height), target, background.fit);
    // Only ever downscale; a source smaller than the target would get blurred
    // by upscaling, so keep it at native resolution and let the GPU object-fit
    // pass handle the final scale.
    let image = if resize_width < source_width || resize_height < source_height {
        image.resize_exact(
            resize_width,
            resize_height,
            image::imageops::FilterType::Triangle,
        )
    } else {
        image
    };

    // Blur runs after the downscale so the Gaussian cost scales with the
    // display size instead of the source resolution.
    let pixels = if background.blur > 0.01 {
        image.blur(background.blur).into_rgba8()
    } else {
        image.into_rgba8()
    };
    let width = pixels.width();
    let height = pixels.height();
    let bytes = pixels.len();
    let mut pixels = pixels.into_raw();
    convert_rgba_pixels_to_gpui_bgra(&mut pixels);
    let buffer = RgbaImage::from_raw(width, height, pixels)?;
    Some((Arc::new(RenderImage::new(vec![Frame::new(buffer)])), bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn display(width: u32, height: u32) -> BackgroundImageTargetSize {
        BackgroundImageTargetSize::new(width, height)
    }

    #[test]
    fn cover_targets_cover_the_display_without_distortion() {
        // A wide source over a wide display must scale up to the display size.
        let (width, height) =
            background_image_resize_target((6400, 3400), display(1920, 1080), TerminalBackgroundFit::Cover);
        assert!(width >= 1920 && height >= 1080);
        let scale_x = f64::from(width) / 6400.0;
        let scale_y = f64::from(height) / 3400.0;
        assert!((scale_x - scale_y).abs() < 0.01, "aspect must be preserved");
    }

    #[test]
    fn contain_targets_fit_within_the_display() {
        // Wide source must shrink so it no longer exceeds the display.
        let (width, height) = background_image_resize_target(
            (6400, 3400),
            display(1920, 1080),
            TerminalBackgroundFit::Contain,
        );
        assert!(width <= 1920 && height <= 1080);
        let scale_x = f64::from(width) / 6400.0;
        let scale_y = f64::from(height) / 3400.0;
        assert!((scale_x - scale_y).abs() < 0.01, "aspect must be preserved");
    }

    #[test]
    fn fill_targets_match_the_display_exactly() {
        let (width, height) =
            background_image_resize_target((6400, 3400), display(1920, 1080), TerminalBackgroundFit::Fill);
        assert_eq!((width, height), (1920, 1080));
    }

    #[test]
    fn tile_scales_the_longest_source_edge_to_the_longest_display_edge() {
        let (width, height) =
            background_image_resize_target((6400, 3400), display(1920, 1080), TerminalBackgroundFit::Tile);
        assert_eq!(width.max(height), 1920);
        let scale_x = f64::from(width) / 6400.0;
        let scale_y = f64::from(height) / 3400.0;
        assert!((scale_x - scale_y).abs() < 0.01, "aspect must be preserved");
    }

    #[test]
    fn display_target_quantizes_to_aligned_device_pixels() {
        let scale = 1.0;
        let target = background_display_target(Size::new(gpui::px(2560.0), gpui::px(1440.0)), scale);
        assert_eq!(target.width % BACKGROUND_TARGET_ALIGN, 0);
        assert_eq!(target.height % BACKGROUND_TARGET_ALIGN, 0);
        assert!(target.width >= 2560);
    }

    #[test]
    fn display_target_scales_logical_pixels_by_scale_factor() {
        // 2x HiDPI doubles the device-pixel target before alignment.
        let target = background_display_target(
            Size::new(gpui::px(1920.0), gpui::px(1080.0)),
            2.0,
        );
        assert!(target.width >= 3840);
        assert!(target.height >= 2160);
        assert_eq!(target.width % BACKGROUND_TARGET_ALIGN, 0);
        assert_eq!(target.height % BACKGROUND_TARGET_ALIGN, 0);
    }

    #[test]
    fn cover_target_still_covers_the_display_for_small_sources() {
        // A small source must still count as covering the display in the resize
        // math so load_background_image can decide to keep it at native size.
        let (width, height) = background_image_resize_target(
            (640, 400),
            display(1920, 1080),
            TerminalBackgroundFit::Cover,
        );
        assert!(width >= 1920 && height >= 1080);
        let scale_x = f64::from(width) / 640.0;
        let scale_y = f64::from(height) / 400.0;
        assert!((scale_x - scale_y).abs() < 0.01, "aspect must be preserved");
    }

    #[test]
    fn display_target_never_quantizes_to_zero() {
        let target = background_display_target(Size::new(gpui::px(0.0), gpui::px(0.0)), 1.0);
        assert!(target.width > 0 && target.height > 0);
    }
}
