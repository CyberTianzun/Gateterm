use std::collections::{HashMap, VecDeque};
use std::io::Read;
use std::sync::Arc;

use base64::Engine;
use base64::engine::general_purpose::{STANDARD, STANDARD_NO_PAD};
use flate2::read::ZlibDecoder;

pub const MAX_GRAPHICS_CACHE_BYTES: usize = 32 * 1024 * 1024;
pub const MAX_IMAGE_DIMENSION: u32 = 4096;

/// Maximum raw bytes in a single uncompressed RGBA image (4096 * 4096 * 4 = 64 MiB).
const MAX_DECOMPRESSED_IMAGE_BYTES: usize =
    (MAX_IMAGE_DIMENSION as usize) * (MAX_IMAGE_DIMENSION as usize) * 4;

/// Maximum raw base64 bytes accumulated in a single APC sequence or multi-chunk transfer.
const MAX_RAW_APC_BYTES: usize = (MAX_DECOMPRESSED_IMAGE_BYTES / 3 + 1) * 4 + 4096;

const ANON_KEY_FLAG: u64 = 1u64 << 32;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ViewportImage {
    pub image_id: u32,
    pub placement_id: u32,
    pub col: u32,
    pub row: i32,
    pub cols: u32,
    pub rows: u32,
    pub z_index: i32,
    pub width_px: u32,
    pub height_px: u32,
    pub rgba: Arc<Vec<u8>>,
}

#[derive(Debug, Clone)]
struct StoredImage {
    image_id: u32,
    width_px: u32,
    height_px: u32,
    rgba: Arc<Vec<u8>>,
}

#[derive(Debug, Clone)]
struct ImagePlacement {
    cache_key: u64,
    image_id: u32,
    placement_id: u32,
    col: u32,
    abs_row: i64,
    cols: u32,
    rows: u32,
    z_index: i32,
}

#[derive(Debug, Clone)]
struct PendingTransfer {
    action: char,
    format: u32,
    compression: Option<char>,
    width: u32,
    height: u32,
    image_id: u32,
    placement_id: u32,
    cols: Option<u32>,
    rows: Option<u32>,
    z_index: i32,
    quiet: u8,
    b64_buf: Vec<u8>,
    committed_cache_key: Option<u64>,
}

#[derive(Default, Debug, Clone, Copy, PartialEq, Eq)]
enum ApcStreamState {
    #[default]
    Ground,
    Esc,
    AfterUnderscore,
    Payload,
    PayloadEsc,
}

#[derive(Default)]
pub struct KittyGraphicsState {
    images: HashMap<u64, StoredImage>,
    lru: VecDeque<u64>,
    placements: Vec<ImagePlacement>,
    total_bytes: usize,
    next_anon_id: u64,
    last_cache_key: Option<u64>,
    pending: Option<PendingTransfer>,
    stream_state: ApcStreamState,
    apc_buf: Vec<u8>,
    apc_overflowed: bool,
}

#[derive(Default)]
struct ParsedControl {
    action: Option<char>,
    transmission: Option<char>,
    format: Option<u32>,
    compression: Option<char>,
    width: Option<u32>,
    height: Option<u32>,
    image_id: Option<u32>,
    placement_id: Option<u32>,
    cols: Option<u32>,
    rows: Option<u32>,
    z_index: Option<i32>,
    more: u8,
    quiet: Option<u8>,
    delete: Option<char>,
}

impl KittyGraphicsState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn reset(&mut self) {
        *self = Self::default();
    }

    pub fn total_cached_bytes(&self) -> usize {
        self.total_bytes
    }

    /// Filter a terminal byte stream, intercepting `\x1b_G...(\x1b\\|\x07)` APC sequences
    /// and resetting graphics state on RIS (`\x1bc`).
    ///
    /// Non-APC bytes preceding an APC sequence are always flushed via `advance_and_cursor`
    /// before the cursor position is read (via `advance_and_cursor(&[])`) so cursor movements
    /// in the same write chunk take effect first.
    pub fn filter<F, R>(&mut self, input: &[u8], mut advance_and_cursor: F, mut on_reply: R)
    where
        F: FnMut(&[u8]) -> (u32, i64, u32, u32),
        R: FnMut(Vec<u8>),
    {
        let mut passthrough_start = 0usize;
        let mut i = 0usize;
        while i < input.len() {
            match self.stream_state {
                ApcStreamState::Ground => {
                    if let Some(rel) = input[i..].iter().position(|&b| b == 0x1b) {
                        let esc_idx = i + rel;
                        let rem_after_esc = input.len() - (esc_idx + 1);
                        if rem_after_esc == 0 {
                            if esc_idx > passthrough_start {
                                let _ = advance_and_cursor(&input[passthrough_start..esc_idx]);
                            }
                            self.stream_state = ApcStreamState::Esc;
                            i = input.len();
                            passthrough_start = i;
                            break;
                        }
                        let next_b = input[esc_idx + 1];
                        if next_b == b'c' {
                            let _ = advance_and_cursor(&input[passthrough_start..esc_idx + 2]);
                            self.reset();
                            i = esc_idx + 2;
                            passthrough_start = i;
                        } else if next_b == b'_' {
                            if rem_after_esc == 1 {
                                if esc_idx > passthrough_start {
                                    let _ = advance_and_cursor(&input[passthrough_start..esc_idx]);
                                }
                                self.stream_state = ApcStreamState::AfterUnderscore;
                                i = input.len();
                                passthrough_start = i;
                                break;
                            }
                            let third_b = input[esc_idx + 2];
                            if third_b == b'G' {
                                if esc_idx > passthrough_start {
                                    let _ = advance_and_cursor(&input[passthrough_start..esc_idx]);
                                }
                                self.apc_buf.clear();
                                self.apc_buf.push(b'G');
                                self.apc_overflowed = false;
                                self.stream_state = ApcStreamState::Payload;
                                i = esc_idx + 3;
                                passthrough_start = i;
                            } else if third_b == 0x1b {
                                i = esc_idx + 2;
                            } else {
                                i = esc_idx + 3;
                            }
                        } else if next_b == 0x1b {
                            i = esc_idx + 1;
                        } else {
                            i = esc_idx + 2;
                        }
                    } else {
                        break;
                    }
                }
                ApcStreamState::Esc => {
                    let b = input[i];
                    i += 1;
                    passthrough_start = i;
                    if b == b'_' {
                        self.stream_state = ApcStreamState::AfterUnderscore;
                    } else if b == b'c' {
                        let _ = advance_and_cursor(b"\x1bc");
                        self.reset();
                    } else if b == 0x1b {
                        let _ = advance_and_cursor(b"\x1b");
                        self.stream_state = ApcStreamState::Esc;
                    } else {
                        let _ = advance_and_cursor(&[0x1b, b]);
                        self.stream_state = ApcStreamState::Ground;
                    }
                }
                ApcStreamState::AfterUnderscore => {
                    let b = input[i];
                    i += 1;
                    passthrough_start = i;
                    if b == b'G' {
                        self.apc_buf.clear();
                        self.apc_buf.push(b'G');
                        self.apc_overflowed = false;
                        self.stream_state = ApcStreamState::Payload;
                    } else if b == 0x1b {
                        let _ = advance_and_cursor(b"\x1b_");
                        self.stream_state = ApcStreamState::Esc;
                    } else {
                        let _ = advance_and_cursor(&[0x1b, b'_', b]);
                        self.stream_state = ApcStreamState::Ground;
                    }
                }
                ApcStreamState::Payload => {
                    let rem = &input[i..];
                    let stop = rem
                        .iter()
                        .position(|&b| matches!(b, 0x07 | 0x1b | 0x18 | 0x1a))
                        .unwrap_or(rem.len());
                    if stop > 0 && !self.apc_overflowed {
                        if self.apc_buf.len().saturating_add(stop) <= MAX_RAW_APC_BYTES {
                            self.apc_buf.extend_from_slice(&rem[..stop]);
                        } else {
                            self.apc_buf.clear();
                            self.apc_buf.shrink_to_fit();
                            self.apc_overflowed = true;
                        }
                    }
                    i += stop;
                    passthrough_start = i;
                    if i >= input.len() {
                        break;
                    }
                    let b = input[i];
                    i += 1;
                    passthrough_start = i;
                    if b == 0x07 {
                        self.finish_stream_apc(&mut advance_and_cursor, &mut on_reply);
                        self.stream_state = ApcStreamState::Ground;
                    } else if b == 0x1b {
                        self.stream_state = ApcStreamState::PayloadEsc;
                    } else {
                        self.apc_buf.clear();
                        self.apc_overflowed = false;
                        self.stream_state = ApcStreamState::Ground;
                    }
                }
                ApcStreamState::PayloadEsc => {
                    let b = input[i];
                    i += 1;
                    passthrough_start = i;
                    if b == b'\\' {
                        self.finish_stream_apc(&mut advance_and_cursor, &mut on_reply);
                        self.stream_state = ApcStreamState::Ground;
                    } else {
                        self.apc_buf.clear();
                        self.apc_overflowed = false;
                        if b == b'_' {
                            self.stream_state = ApcStreamState::AfterUnderscore;
                        } else if b == b'c' {
                            let _ = advance_and_cursor(b"\x1bc");
                            self.reset();
                        } else if b == 0x1b {
                            let _ = advance_and_cursor(b"\x1b");
                            self.stream_state = ApcStreamState::Esc;
                        } else {
                            let _ = advance_and_cursor(&[0x1b, b]);
                            self.stream_state = ApcStreamState::Ground;
                        }
                    }
                }
            }
        }
        if self.stream_state == ApcStreamState::Ground && passthrough_start < input.len() {
            let _ = advance_and_cursor(&input[passthrough_start..]);
        }
    }

    fn finish_stream_apc<F, R>(&mut self, advance_and_cursor: &mut F, on_reply: &mut R)
    where
        F: FnMut(&[u8]) -> (u32, i64, u32, u32),
        R: FnMut(Vec<u8>),
    {
        if self.apc_overflowed {
            self.apc_buf.clear();
            self.apc_overflowed = false;
            self.pending = None;
            return;
        }
        let payload = std::mem::take(&mut self.apc_buf);
        let (cursor_col, abs_cursor_row, term_cols, term_rows) = advance_and_cursor(&[]);
        if let Some(reply) = self.handle_apc(
            &payload,
            cursor_col,
            abs_cursor_row,
            term_cols,
            term_rows,
        ) {
            on_reply(reply);
        }
    }

    pub fn handle_apc(
        &mut self,
        payload: &[u8],
        cursor_col: u32,
        abs_cursor_row: i64,
        _term_cols: u32,
        _term_rows: u32,
    ) -> Option<Vec<u8>> {
        if !payload.starts_with(b"G") {
            return None;
        }
        let rest = &payload[1..];
        let (header_bytes, b64_bytes) = match rest.iter().position(|&b| b == b';') {
            Some(pos) => (&rest[..pos], &rest[pos + 1..]),
            None => (rest, &[][..]),
        };
        let header_str = std::str::from_utf8(header_bytes).ok()?;
        let ctrl = parse_control(header_str);

        let is_continuation = if let Some(ref p) = self.pending {
            let same_or_omitted_id = ctrl.image_id.is_none() || ctrl.image_id == Some(p.image_id);
            let no_new_dims = ctrl.width.is_none() && ctrl.height.is_none();
            let not_other_action = !matches!(ctrl.action, Some('d' | 'p' | 'q'));
            if p.committed_cache_key.is_some() {
                same_or_omitted_id
                    && no_new_dims
                    && ctrl.action.is_none()
                    && ctrl.format.is_none()
            } else {
                same_or_omitted_id && no_new_dims && not_other_action
            }
        } else {
            false
        };

        if !is_continuation {
            self.pending = None;
        }

        if let Some(mut pending) = self.pending.take() {
            if let Some(q) = ctrl.quiet {
                pending.quiet = q;
            }
            if let Some(c) = ctrl.cols {
                pending.cols = Some(c);
            }
            if let Some(r) = ctrl.rows {
                pending.rows = Some(r);
            }
            if let Some(z) = ctrl.z_index {
                pending.z_index = z;
            }
            if let Some(p) = ctrl.placement_id {
                pending.placement_id = p;
            }
            if let Some(t) = ctrl.transmission
                && t != 'd'
            {
                return format_error(
                    pending.image_id,
                    pending.placement_id,
                    pending.quiet,
                    "file transmission disabled",
                );
            }
            if pending
                .b64_buf
                .len()
                .saturating_add(b64_bytes.len())
                > MAX_RAW_APC_BYTES
            {
                return format_error(
                    pending.image_id,
                    pending.placement_id,
                    pending.quiet,
                    "payload too large",
                );
            }
            append_clean_b64(&mut pending.b64_buf, b64_bytes);

            if ctrl.more == 1 {
                if pending.action != 'q'
                    && let Ok((w, h, rgba)) = decode_image(
                        pending.format,
                        pending.compression,
                        pending.width,
                        pending.height,
                        &pending.b64_buf,
                    )
                    && let Ok(key) = self.commit_decoded_image(
                        &pending,
                        w,
                        h,
                        rgba,
                        cursor_col,
                        abs_cursor_row,
                    )
                {
                    pending.committed_cache_key = Some(key);
                }
                self.pending = Some(pending);
                return None;
            } else {
                return self.finalize_transfer(pending, cursor_col, abs_cursor_row);
            }
        }

        let action = ctrl.action.unwrap_or('t');
        let transmission = ctrl.transmission.unwrap_or('d');
        let format = ctrl.format.unwrap_or(32);
        let compression = ctrl.compression;
        let width = ctrl.width.unwrap_or(0);
        let height = ctrl.height.unwrap_or(0);
        let image_id = ctrl.image_id.unwrap_or(0);
        let placement_id = ctrl.placement_id.unwrap_or(0);
        let z_index = ctrl.z_index.unwrap_or(0);
        let quiet = ctrl.quiet.unwrap_or(0);

        match action {
            'd' => {
                self.handle_delete(
                    ctrl.delete.unwrap_or('a'),
                    image_id,
                    placement_id,
                    z_index,
                    cursor_col,
                    abs_cursor_row,
                );
                None
            }
            'p' => {
                let target_key = if image_id > 0 {
                    Some(image_id as u64)
                } else {
                    self.last_cache_key
                };
                let Some(key) = target_key else {
                    return format_error(image_id, placement_id, quiet, "image not found");
                };
                let Some((w, h, actual_id)) = self
                    .images
                    .get(&key)
                    .map(|img| (img.width_px, img.height_px, img.image_id))
                else {
                    return format_error(image_id, placement_id, quiet, "image not found");
                };
                self.touch_lru(key);
                self.record_placement(
                    key,
                    actual_id,
                    placement_id,
                    cursor_col,
                    abs_cursor_row,
                    ctrl.cols,
                    ctrl.rows,
                    z_index,
                    w,
                    h,
                );
                format_ok(actual_id, placement_id, quiet)
            }
            't' | 'T' | 'q' => {
                if transmission != 'd' {
                    return format_error(
                        image_id,
                        placement_id,
                        quiet,
                        "file transmission disabled",
                    );
                }
                if compression.is_some_and(|c| c != 'z') {
                    return format_error(
                        image_id,
                        placement_id,
                        quiet,
                        "unsupported compression",
                    );
                }
                if !matches!(format, 24 | 32 | 100) {
                    return format_error(image_id, placement_id, quiet, "unsupported format");
                }
                if b64_bytes.len() > MAX_RAW_APC_BYTES {
                    return format_error(image_id, placement_id, quiet, "payload too large");
                }
                let mut b64_buf = Vec::with_capacity(b64_bytes.len());
                append_clean_b64(&mut b64_buf, b64_bytes);
                let mut pending = PendingTransfer {
                    action,
                    format,
                    compression,
                    width,
                    height,
                    image_id,
                    placement_id,
                    cols: ctrl.cols,
                    rows: ctrl.rows,
                    z_index,
                    quiet,
                    b64_buf,
                    committed_cache_key: None,
                };
                if ctrl.more == 1 {
                    if pending.action != 'q'
                        && let Ok((w, h, rgba)) = decode_image(
                            pending.format,
                            pending.compression,
                            pending.width,
                            pending.height,
                            &pending.b64_buf,
                        )
                        && let Ok(key) = self.commit_decoded_image(
                            &pending,
                            w,
                            h,
                            rgba,
                            cursor_col,
                            abs_cursor_row,
                        )
                    {
                        pending.committed_cache_key = Some(key);
                    }
                    self.pending = Some(pending);
                    None
                } else {
                    self.finalize_transfer(pending, cursor_col, abs_cursor_row)
                }
            }
            _ => format_error(image_id, placement_id, quiet, "unsupported action"),
        }
    }

    fn finalize_transfer(
        &mut self,
        pending: PendingTransfer,
        cursor_col: u32,
        abs_cursor_row: i64,
    ) -> Option<Vec<u8>> {
        let (width_px, height_px, rgba) = match decode_image(
            pending.format,
            pending.compression,
            pending.width,
            pending.height,
            &pending.b64_buf,
        ) {
            Ok(res) => res,
            Err(reason) => {
                return format_error(pending.image_id, pending.placement_id, pending.quiet, reason);
            }
        };

        if pending.action == 'q' {
            return format_ok(pending.image_id, pending.placement_id, pending.quiet);
        }

        if let Err(reason) = self.commit_decoded_image(
            &pending,
            width_px,
            height_px,
            rgba,
            cursor_col,
            abs_cursor_row,
        ) {
            return format_error(pending.image_id, pending.placement_id, pending.quiet, reason);
        }

        format_ok(pending.image_id, pending.placement_id, pending.quiet)
    }

    fn commit_decoded_image(
        &mut self,
        pending: &PendingTransfer,
        width_px: u32,
        height_px: u32,
        rgba: Vec<u8>,
        cursor_col: u32,
        abs_cursor_row: i64,
    ) -> Result<u64, &'static str> {
        let img_bytes = rgba.len();
        if img_bytes > MAX_GRAPHICS_CACHE_BYTES {
            return Err("image exceeds cache limit");
        }

        let cache_key = if let Some(existing_key) = pending.committed_cache_key {
            existing_key
        } else if pending.image_id > 0 {
            pending.image_id as u64
        } else {
            self.next_anon_id += 1;
            ANON_KEY_FLAG | self.next_anon_id
        };

        if let Some(old) = self.images.remove(&cache_key) {
            self.total_bytes = self.total_bytes.saturating_sub(old.rgba.len());
            self.lru.retain(|&k| k != cache_key);
        }

        while self.total_bytes.saturating_add(img_bytes) > MAX_GRAPHICS_CACHE_BYTES {
            let Some(evicted_key) = self.lru.pop_front() else {
                break;
            };
            if let Some(evicted) = self.images.remove(&evicted_key) {
                self.total_bytes = self.total_bytes.saturating_sub(evicted.rgba.len());
            }
            self.placements.retain(|p| p.cache_key != evicted_key);
            if self.last_cache_key == Some(evicted_key) {
                self.last_cache_key = None;
            }
        }

        self.total_bytes += img_bytes;
        self.images.insert(
            cache_key,
            StoredImage {
                image_id: pending.image_id,
                width_px,
                height_px,
                rgba: Arc::new(rgba),
            },
        );
        self.lru.push_back(cache_key);
        self.last_cache_key = Some(cache_key);

        if pending.action == 'T' {
            if pending.committed_cache_key.is_some() {
                self.placements.retain(|p| p.cache_key != cache_key);
            }
            self.record_placement(
                cache_key,
                pending.image_id,
                pending.placement_id,
                cursor_col,
                abs_cursor_row,
                pending.cols,
                pending.rows,
                pending.z_index,
                width_px,
                height_px,
            );
        }

        Ok(cache_key)
    }

    #[allow(clippy::too_many_arguments)]
    fn record_placement(
        &mut self,
        cache_key: u64,
        image_id: u32,
        placement_id: u32,
        col: u32,
        abs_row: i64,
        cols_opt: Option<u32>,
        rows_opt: Option<u32>,
        z_index: i32,
        width_px: u32,
        height_px: u32,
    ) {
        let cols = cols_opt
            .filter(|&c| c > 0)
            .unwrap_or_else(|| width_px.div_ceil(10).max(1));
        let rows = rows_opt
            .filter(|&r| r > 0)
            .unwrap_or_else(|| height_px.div_ceil(20).max(1));

        if image_id > 0
            && placement_id > 0
            && let Some(existing) = self
                .placements
                .iter_mut()
                .find(|p| p.image_id == image_id && p.placement_id == placement_id)
        {
            existing.cache_key = cache_key;
            existing.col = col;
            existing.abs_row = abs_row;
            existing.cols = cols;
            existing.rows = rows;
            existing.z_index = z_index;
            return;
        }

        self.placements.push(ImagePlacement {
            cache_key,
            image_id,
            placement_id,
            col,
            abs_row,
            cols,
            rows,
            z_index,
        });
    }

    fn touch_lru(&mut self, cache_key: u64) {
        self.lru.retain(|&k| k != cache_key);
        self.lru.push_back(cache_key);
    }

    fn handle_delete(
        &mut self,
        selector: char,
        image_id: u32,
        placement_id: u32,
        z_index: i32,
        cursor_col: u32,
        abs_cursor_row: i64,
    ) {
        match selector {
            'a' => {
                self.placements.clear();
            }
            'A' => {
                self.placements.clear();
                self.images.clear();
                self.lru.clear();
                self.total_bytes = 0;
                self.last_cache_key = None;
            }
            'i' | 'I' => {
                if image_id > 0 {
                    if placement_id > 0 {
                        self.placements.retain(|p| {
                            !(p.image_id == image_id && p.placement_id == placement_id)
                        });
                    } else {
                        self.placements.retain(|p| p.image_id != image_id);
                    }
                    if selector == 'I'
                        && !self.placements.iter().any(|p| p.image_id == image_id)
                    {
                        let key = image_id as u64;
                        if let Some(removed) = self.images.remove(&key) {
                            self.total_bytes = self.total_bytes.saturating_sub(removed.rgba.len());
                        }
                        self.lru.retain(|&k| k != key);
                        if self.last_cache_key == Some(key) {
                            self.last_cache_key = None;
                        }
                    }
                }
            }
            'p' | 'P' => {
                if placement_id > 0 {
                    self.placements.retain(|p| p.placement_id != placement_id);
                }
            }
            'z' | 'Z' => {
                self.placements.retain(|p| p.z_index != z_index);
            }
            'c' | 'C' => {
                self.placements.retain(|p| {
                    let in_cols = cursor_col >= p.col && cursor_col < p.col.saturating_add(p.cols);
                    let in_rows = abs_cursor_row >= p.abs_row
                        && abs_cursor_row < p.abs_row.saturating_add(p.rows as i64);
                    !(in_cols && in_rows)
                });
            }
            'x' | 'X' => {
                self.placements.retain(|p| {
                    !(cursor_col >= p.col && cursor_col < p.col.saturating_add(p.cols))
                });
            }
            'y' | 'Y' => {
                self.placements.retain(|p| {
                    !(abs_cursor_row >= p.abs_row
                        && abs_cursor_row < p.abs_row.saturating_add(p.rows as i64))
                });
            }
            _ => {}
        }
    }

    pub fn visible_images(
        &self,
        viewport_top_abs_row: i64,
        viewport_rows: usize,
    ) -> Vec<ViewportImage> {
        let viewport_bottom = viewport_top_abs_row + viewport_rows as i64;
        let mut out = Vec::new();
        for p in &self.placements {
            let p_bottom = p.abs_row.saturating_add(p.rows as i64);
            if p_bottom <= viewport_top_abs_row || p.abs_row >= viewport_bottom {
                continue;
            }
            let Some(img) = self.images.get(&p.cache_key) else {
                continue;
            };
            let rel_row = (p.abs_row - viewport_top_abs_row)
                .clamp(i32::MIN as i64, i32::MAX as i64) as i32;
            out.push(ViewportImage {
                image_id: p.image_id,
                placement_id: p.placement_id,
                col: p.col,
                row: rel_row,
                cols: p.cols,
                rows: p.rows,
                z_index: p.z_index,
                width_px: img.width_px,
                height_px: img.height_px,
                rgba: img.rgba.clone(),
            });
        }
        out
    }
}

fn append_clean_b64(dst: &mut Vec<u8>, src: &[u8]) {
    dst.extend(src.iter().copied().filter(|b| !b.is_ascii_whitespace()));
}

fn parse_control(header: &str) -> ParsedControl {
    let mut ctrl = ParsedControl::default();
    for pair in header.split(',') {
        let Some((k, v)) = pair.split_once('=') else {
            continue;
        };
        let k = k.trim();
        let v = v.trim();
        match k {
            "a" => ctrl.action = v.chars().next(),
            "t" => ctrl.transmission = v.chars().next(),
            "f" => ctrl.format = v.parse().ok(),
            "o" => ctrl.compression = v.chars().next(),
            "s" => ctrl.width = v.parse().ok(),
            "v" => ctrl.height = v.parse().ok(),
            "i" => ctrl.image_id = v.parse().ok(),
            "p" => ctrl.placement_id = v.parse().ok(),
            "c" => ctrl.cols = v.parse().ok(),
            "r" => ctrl.rows = v.parse().ok(),
            "z" => ctrl.z_index = v.parse().ok(),
            "m" => ctrl.more = v.parse::<u8>().unwrap_or(0).min(1),
            "q" => ctrl.quiet = v.parse().ok(),
            "d" => ctrl.delete = v.chars().next(),
            _ => {}
        }
    }
    ctrl
}

fn decode_image(
    format: u32,
    compression: Option<char>,
    width: u32,
    height: u32,
    b64_buf: &[u8],
) -> Result<(u32, u32, Vec<u8>), &'static str> {
    if b64_buf.is_empty() {
        return Err("empty payload");
    }
    let decoded = STANDARD
        .decode(b64_buf)
        .or_else(|_| STANDARD_NO_PAD.decode(b64_buf))
        .map_err(|_| "invalid base64")?;

    let raw = match compression {
        None => {
            if decoded.len() > MAX_DECOMPRESSED_IMAGE_BYTES {
                return Err("payload exceeds maximum size");
            }
            decoded
        }
        Some('z') => {
            let decoder = ZlibDecoder::new(decoded.as_slice());
            let mut decompressed = Vec::new();
            decoder
                .take((MAX_DECOMPRESSED_IMAGE_BYTES + 1) as u64)
                .read_to_end(&mut decompressed)
                .map_err(|_| "zlib decompression failed")?;
            if decompressed.len() > MAX_DECOMPRESSED_IMAGE_BYTES {
                return Err("decompressed payload exceeds maximum size");
            }
            decompressed
        }
        Some(_) => return Err("unsupported compression"),
    };

    match format {
        32 => {
            validate_dimensions(width, height)?;
            let expected = (width as usize)
                .checked_mul(height as usize)
                .and_then(|px| px.checked_mul(4))
                .ok_or("dimension overflow")?;
            if raw.len() != expected {
                return Err("rgba payload length mismatch");
            }
            Ok((width, height, raw))
        }
        24 => {
            validate_dimensions(width, height)?;
            let pixels = (width as usize)
                .checked_mul(height as usize)
                .ok_or("dimension overflow")?;
            let expected = pixels.checked_mul(3).ok_or("dimension overflow")?;
            if raw.len() != expected {
                return Err("rgb payload length mismatch");
            }
            let mut rgba = Vec::with_capacity(pixels * 4);
            for &[r, g, b] in raw.as_chunks::<3>().0 {
                rgba.extend_from_slice(&[r, g, b, 255]);
            }
            Ok((width, height, rgba))
        }
        100 => decode_png(&raw),
        _ => Err("unsupported format"),
    }
}

fn validate_dimensions(width: u32, height: u32) -> Result<(), &'static str> {
    if width == 0 || height == 0 || width > MAX_IMAGE_DIMENSION || height > MAX_IMAGE_DIMENSION {
        return Err("invalid dimensions");
    }
    Ok(())
}

fn decode_png(raw: &[u8]) -> Result<(u32, u32, Vec<u8>), &'static str> {
    let mut decoder = png::Decoder::new(std::io::Cursor::new(raw));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info().map_err(|_| "invalid png header")?;
    let (width, height) = reader.info().size();
    validate_dimensions(width, height)?;

    let buf_size = reader.output_buffer_size().ok_or("png buffer overflow")?;
    if buf_size > MAX_DECOMPRESSED_IMAGE_BYTES {
        return Err("png frame exceeds maximum size");
    }
    let mut buf = vec![0u8; buf_size];
    let info = reader.next_frame(&mut buf).map_err(|_| "invalid png data")?;
    let bytes = &buf[..info.buffer_size()];
    let pixels = (width as usize) * (height as usize);

    let rgba = match info.color_type {
        png::ColorType::Rgba => {
            if bytes.len() != pixels * 4 {
                return Err("png rgba size mismatch");
            }
            bytes.to_vec()
        }
        png::ColorType::Rgb => {
            if bytes.len() != pixels * 3 {
                return Err("png rgb size mismatch");
            }
            let mut out = Vec::with_capacity(pixels * 4);
            for &[r, g, b] in bytes.as_chunks::<3>().0 {
                out.extend_from_slice(&[r, g, b, 255]);
            }
            out
        }
        png::ColorType::Grayscale => {
            if bytes.len() != pixels {
                return Err("png grayscale size mismatch");
            }
            let mut out = Vec::with_capacity(pixels * 4);
            for &g in bytes {
                out.extend_from_slice(&[g, g, g, 255]);
            }
            out
        }
        png::ColorType::GrayscaleAlpha => {
            if bytes.len() != pixels * 2 {
                return Err("png grayscale-alpha size mismatch");
            }
            let mut out = Vec::with_capacity(pixels * 4);
            for &[g, a] in bytes.as_chunks::<2>().0 {
                out.extend_from_slice(&[g, g, g, a]);
            }
            out
        }
        png::ColorType::Indexed => return Err("unexpanded indexed png"),
    };

    Ok((width, height, rgba))
}

fn format_id_header(image_id: u32, placement_id: u32) -> String {
    match (image_id > 0, placement_id > 0) {
        (true, true) => format!("i={image_id},p={placement_id}"),
        (true, false) => format!("i={image_id}"),
        (false, true) => format!("p={placement_id}"),
        (false, false) => String::new(),
    }
}

fn format_ok(image_id: u32, placement_id: u32, quiet: u8) -> Option<Vec<u8>> {
    if quiet >= 1 {
        return None;
    }
    if image_id == 0 && placement_id == 0 {
        return None;
    }
    let hdr = format_id_header(image_id, placement_id);
    Some(format!("\x1b_G{hdr};OK\x1b\\").into_bytes())
}

fn format_error(image_id: u32, placement_id: u32, quiet: u8, reason: &str) -> Option<Vec<u8>> {
    if quiet >= 2 {
        return None;
    }
    let hdr = format_id_header(image_id, placement_id);
    Some(format!("\x1b_G{hdr};EINVAL:{reason}\x1b\\").into_bytes())
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use base64::Engine;
    use base64::engine::general_purpose::STANDARD;
    use flate2::Compression;
    use flate2::write::ZlibEncoder;

    use crate::graphics::{
        KittyGraphicsState, MAX_GRAPHICS_CACHE_BYTES, MAX_IMAGE_DIMENSION,
    };
    use crate::term::ClientTerm;

    #[test]
    fn parses_chunked_inline_rgba_image_and_places_in_viewport() {
        let mut t = ClientTerm::new(20, 5, 100);
        // 1x1 red RGBA pixel ([255, 0, 0, 255] -> base64 "/wAA/w==")
        // a=T (transmit & display), f=32 (RGBA), s=1, v=1, i=7, c=2, r=1, m=1 then m=0
        t.write(b"\x1b_Ga=T,f=32,s=1,v=1,i=7,c=2,r=1,m=1;/wA");
        t.write(b"A/w==\x1b\\");
        let imgs = t.visible_images();
        assert_eq!(imgs.len(), 1);
        assert_eq!(imgs[0].image_id, 7);
        assert_eq!(imgs[0].cols, 2);
        assert_eq!(imgs[0].rows, 1);
        assert_eq!(&*imgs[0].rgba, &[255, 0, 0, 255]);
    }

    #[test]
    fn rejects_file_transmission_and_honors_quiet_flags() {
        let mut g = crate::graphics::KittyGraphicsState::new();
        // t=f (local file) must be rejected with EINVAL
        let reply = g.handle_apc(b"Ga=T,t=f,i=3;L2V0Yy9wYXNzd2Q=", 0, 0, 80, 24);
        assert_eq!(
            String::from_utf8(reply.unwrap()).unwrap(),
            "\x1b_Gi=3;EINVAL:file transmission disabled\x1b\\"
        );

        // q=1 suppresses OK, but still reports EINVAL
        let ok_q1 = g.handle_apc(b"Ga=T,f=32,s=1,v=1,i=4,q=1;/wAA/w==", 0, 0, 80, 24);
        assert!(ok_q1.is_none());
        let err_q1 = g.handle_apc(b"Ga=T,t=f,i=5,q=1;L2V0Yy9wYXNzd2Q=", 0, 0, 80, 24);
        assert!(err_q1.is_some());

        // q=2 suppresses both OK and EINVAL
        let err_q2 = g.handle_apc(b"Ga=T,t=f,i=6,q=2;L2V0Yy9wYXNzd2Q=", 0, 0, 80, 24);
        assert!(err_q2.is_none());
    }

    #[test]
    fn reassembles_multi_apc_m1_m0_chunks_and_decodes_png_and_zlib() {
        let mut t = ClientTerm::new(20, 5, 100);
        // Multi-APC transfer: first APC has m=1 with partial base64, second APC has m=0 with remainder
        t.write(b"\x1b_Ga=T,f=32,s=1,v=1,i=10,c=3,r=2,m=1;/wA\x1b\\");
        assert!(t.visible_images().is_empty());
        t.write(b"\x1b_Gm=0;A/w==\x1b\\");
        let imgs = t.visible_images();
        assert_eq!(imgs.len(), 1);
        assert_eq!(imgs[0].image_id, 10);
        assert_eq!(imgs[0].cols, 3);
        assert_eq!(imgs[0].rows, 2);
        assert_eq!(&*imgs[0].rgba, &[255, 0, 0, 255]);

        // Zlib-compressed RGB (f=24, o=z) at cursor position (row 3, col 5 -> 0-based row 2, col 4)
        let mut enc = ZlibEncoder::new(Vec::new(), Compression::default());
        enc.write_all(&[0, 255, 0]).unwrap();
        let compressed = enc.finish().unwrap();
        let b64_z = STANDARD.encode(&compressed);
        let cmd = format!("\x1b[3;5H\x1b_Ga=T,f=24,o=z,s=1,v=1,i=11;{b64_z}\x1b\\");
        t.write(cmd.as_bytes());
        let imgs = t.visible_images();
        assert_eq!(imgs.len(), 2);
        assert_eq!(imgs[1].image_id, 11);
        assert_eq!(imgs[1].col, 4);
        assert_eq!(imgs[1].row, 2);
        assert_eq!(&*imgs[1].rgba, &[0, 255, 0, 255]);

        // Inline PNG (f=100)
        let mut png_bytes = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut png_bytes, 1, 1);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            let mut writer = encoder.write_header().unwrap();
            writer.write_image_data(&[0, 0, 255, 128]).unwrap();
        }
        let b64_png = STANDARD.encode(&png_bytes);
        let png_cmd = format!("\x1b_Ga=T,f=100,i=12;{b64_png}\x1b\\");
        t.write(png_cmd.as_bytes());
        let imgs = t.visible_images();
        assert_eq!(imgs.len(), 3);
        assert_eq!(imgs[2].image_id, 12);
        assert_eq!(imgs[2].width_px, 1);
        assert_eq!(imgs[2].height_px, 1);
        assert_eq!(&*imgs[2].rgba, &[0, 0, 255, 128]);
    }

    #[test]
    fn rejects_invalid_dimensions_and_corrupt_payloads_with_einval() {
        let mut g = KittyGraphicsState::new();
        // Exceeds MAX_IMAGE_DIMENSION
        let too_wide = format!(
            "Ga=T,f=32,s={},v=1,i=20;/wAA/w==",
            MAX_IMAGE_DIMENSION + 1
        );
        let reply = g
            .handle_apc(too_wide.as_bytes(), 0, 0, 80, 24)
            .unwrap();
        assert!(String::from_utf8(reply).unwrap().contains("EINVAL:"));

        // Invalid base64
        let bad_b64 = g
            .handle_apc(b"Ga=T,f=32,s=1,v=1,i=21;!!!not_base64!!!", 0, 0, 80, 24)
            .unwrap();
        assert!(String::from_utf8(bad_b64).unwrap().contains("EINVAL:"));

        // Length mismatch (s=2, v=2 expects 16 bytes, given 4 bytes)
        let mismatch = g
            .handle_apc(b"Ga=T,f=32,s=2,v=2,i=22;/wAA/w==", 0, 0, 80, 24)
            .unwrap();
        assert!(String::from_utf8(mismatch).unwrap().contains("EINVAL:"));
    }

    #[test]
    fn evicts_least_recently_used_images_when_cache_exceeds_32_mib() {
        let mut g = KittyGraphicsState::new();
        // Each 1024x1024 RGBA image is 4 MiB. 8 images = 32 MiB (fits in cache).
        // Adding a 9th 4 MiB image must evict the oldest image.
        let raw_4mib = vec![128u8; 1024 * 1024 * 4];
        let b64_4mib = STANDARD.encode(&raw_4mib);

        for id in 1..=8 {
            let cmd = format!("Ga=T,f=32,s=1024,v=1024,i={id};{b64_4mib}");
            let reply = g.handle_apc(cmd.as_bytes(), 0, 0, 80, 24).unwrap();
            assert_eq!(
                String::from_utf8(reply).unwrap(),
                format!("\x1b_Gi={id};OK\x1b\\")
            );
        }
        assert_eq!(g.total_cached_bytes(), MAX_GRAPHICS_CACHE_BYTES);
        assert_eq!(g.visible_images(0, 24).len(), 8);

        // Touch image 1 via a=p so image 2 becomes the least recently used
        g.handle_apc(b"Ga=p,i=1,p=2", 0, 0, 80, 24);

        // Insert image 9 (4 MiB) -> should evict image 2, keeping image 1
        let cmd9 = format!("Ga=T,f=32,s=1024,v=1024,i=9;{b64_4mib}");
        g.handle_apc(cmd9.as_bytes(), 0, 0, 80, 24);
        assert_eq!(g.total_cached_bytes(), MAX_GRAPHICS_CACHE_BYTES);

        let visible_ids: Vec<u32> = g.visible_images(0, 24).iter().map(|v| v.image_id).collect();
        assert!(visible_ids.contains(&1), "touched image 1 should remain");
        assert!(!visible_ids.contains(&2), "LRU image 2 should be evicted");
        assert!(visible_ids.contains(&9), "new image 9 should be present");
    }

    #[test]
    fn tracks_scrollback_offset_and_clears_on_ris() {
        let mut t = ClientTerm::new(20, 3, 100);
        // Place image at row 0
        t.write(b"\x1b_Ga=T,f=32,s=1,v=1,i=1,c=1,r=1;/wAA/w==\x1b\\");
        assert_eq!(t.visible_images().len(), 1);
        assert_eq!(t.visible_images()[0].row, 0);

        // Scroll 5 lines down so row 0 moves into scrollback history
        t.write(b"\r\n1\r\n2\r\n3\r\n4\r\n5");
        assert!(
            t.visible_images().is_empty(),
            "image scrolled into history should not be in bottom viewport"
        );

        // Scroll viewport back up to the top
        t.scroll_by(10);
        let imgs = t.visible_images();
        assert_eq!(imgs.len(), 1);
        assert_eq!(imgs[0].row, 0);

        // RIS (\x1bc) clears graphics state
        t.write(b"\x1bc");
        assert!(t.visible_images().is_empty());
    }
}
