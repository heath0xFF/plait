//! Image decoding is background work; frames only draw prepared pixels.
use super::{Plait, theme::*, thread_controls::TextTooltip};
use gpui::{prelude::*, *};
use plait_core::{
    host::Client,
    images::{self, ImageRef},
};
use std::{
    collections::{HashMap, HashSet, VecDeque},
    sync::Arc,
};

pub(super) fn decode(bytes: &[u8], edge: u32) -> Result<Arc<RenderImage>, String> {
    let info = images::inspect(bytes)?;
    let format = if info.media_type == "image/png" {
        image::ImageFormat::Png
    } else {
        image::ImageFormat::Jpeg
    };
    let mut reader = image::ImageReader::with_format(std::io::Cursor::new(bytes), format);
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(8192);
    limits.max_image_height = Some(8192);
    limits.max_alloc = Some(128 * 1024 * 1024);
    reader.limits(limits);
    use image::ImageDecoder;
    let mut decoder = reader
        .into_decoder()
        .map_err(|_| "The image could not be decoded.")?;
    if decoder.dimensions() != (info.width, info.height) {
        return Err("Image dimensions changed while decoding.".into());
    }
    let orientation = decoder
        .orientation()
        .map_err(|_| "The image orientation could not be read.")?;
    let mut decoded = image::DynamicImage::from_decoder(decoder)
        .map_err(|_| "The image could not be decoded.")?;
    decoded.apply_orientation(orientation);
    let mut pixels = decoded.thumbnail(edge, edge).into_rgba8();
    for pixel in pixels.as_chunks_mut::<4>().0 {
        pixel.swap(0, 2);
    }
    Ok(Arc::new(RenderImage::new(vec![image::Frame::new(pixels)])))
}

type PreparedImage = Result<Arc<RenderImage>, String>;
type EnlargedImage = (ImageRef, Option<PreparedImage>);

#[derive(Default)]
pub(super) struct ImageCache {
    key: Option<(u64, u64, String)>,
    ready: HashMap<String, Result<Arc<RenderImage>, String>>,
    pending: HashSet<String>,
    order: VecDeque<String>,
    pub enlarged: Option<EnlargedImage>,
    large_generation: u64,
}

/// Both host connection and selection must still be the ones that requested the pixels.
pub(super) fn accepts(expected: &(u64, u64, String), actual: &(u64, u64, String)) -> bool {
    expected == actual
}

impl ImageCache {
    pub(super) fn complete(
        &mut self,
        expected: &(u64, u64, String),
        current: &(u64, u64, String),
        id: String,
        result: PreparedImage,
    ) -> bool {
        if !accepts(expected, current) || self.key.as_ref() != Some(expected) {
            return false;
        }
        self.pending.remove(&id);
        while self.ready.len() >= 32 {
            if let Some(old) = self.order.pop_front() {
                self.ready.remove(&old);
            } else {
                break;
            }
        }
        self.order.push_back(id.clone());
        self.ready.insert(id, result);
        true
    }
    #[cfg(test)]
    pub(super) fn len(&self) -> usize {
        self.ready.len()
    }
}

impl Plait {
    pub(super) fn image_row(&self, references: &[ImageRef], cx: &mut Context<Self>) -> AnyElement {
        let thread = self.host.selected.clone().unwrap_or_default();
        let key = (self.host.epoch, self.host.watch_epoch, thread.clone());
        let mut missing = Vec::new();
        let row = row()
            .flex_wrap()
            .gap_2()
            .children(references.iter().enumerate().map(|(index, reference)| {
                let ready = (self.images.key.as_ref() == Some(&key))
                    .then(|| self.images.ready.get(&reference.id))
                    .flatten();
                if ready.is_none()
                    && (self.images.key.as_ref() != Some(&key)
                        || (!self.images.pending.contains(&reference.id)
                            && self.images.pending.len() < 4))
                    && self.host.accepts_watch(key.0, key.1, &key.2)
                {
                    missing.push(reference.clone());
                }
                let image = reference.clone();
                let detail = ready
                    .and_then(|r| r.as_ref().err())
                    .cloned()
                    .unwrap_or_else(|| {
                        format!(
                            "{} × {} · {} bytes · Open image",
                            reference.width, reference.height, reference.size
                        )
                    });
                button(SharedString::from(format!("image-{}", reference.id)), "")
                    .aria_label(format!("Open attached image {}", index + 1))
                    .w(px(96.))
                    .h(px(80.))
                    .p_1()
                    .border_1()
                    .border_color(rgb(colors().border))
                    .child(match ready {
                        Some(Ok(pixels)) => img(pixels.clone())
                            .size_full()
                            .object_fit(ObjectFit::Contain)
                            .into_any_element(),
                        Some(Err(_)) => div().child("Unavailable").into_any_element(),
                        None => div().child("Loading").into_any_element(),
                    })
                    .tooltip(move |_, cx| cx.new(|_| TextTooltip(detail.clone())).into())
                    .on_activate(cx, move |this, window, cx| {
                        this.enlarge_image(image.clone(), window, cx)
                    })
            }));
        // Schedule only visible rows, after this render has returned.
        if !missing.is_empty() {
            let weak = cx.weak_entity();
            cx.defer(move |cx| {
                let _ = weak.update(cx, |this, cx| this.load_images(key, missing, cx));
            });
        }
        row.into_any_element()
    }
    fn load_images(
        &mut self,
        key: (u64, u64, String),
        references: Vec<ImageRef>,
        cx: &mut Context<Self>,
    ) {
        if !self.host.accepts_watch(key.0, key.1, &key.2) {
            return;
        }
        if self.images.key.as_ref() != Some(&key) {
            self.images = ImageCache {
                key: Some(key.clone()),
                ..Default::default()
            };
        }
        for reference in references {
            if self.images.ready.contains_key(&reference.id)
                || self.images.pending.contains(&reference.id)
                || self.images.pending.len() >= 4
            {
                continue;
            }
            self.images.pending.insert(reference.id.clone());
            let expected = key.clone();
            let id = reference.id.clone();
            let task = cx.background_executor().spawn(async move {
                let bytes = Client::from_environment()
                    .and_then(|client| client.image(&expected.2, &reference))
                    .map_err(|error| error.to_string())?;
                decode(&bytes, 192)
            });
            let key = key.clone();
            cx.spawn(async move |this, cx| {
                let result = task.await;
                let _ = this.update(cx, |this, cx| {
                    let current = (
                        this.host.epoch,
                        this.host.watch_epoch,
                        this.host.selected.clone().unwrap_or_default(),
                    );
                    if this.images.complete(&key, &current, id, result) {
                        cx.notify();
                    }
                });
            })
            .detach();
        }
    }
    fn enlarge_image(&mut self, reference: ImageRef, window: &mut Window, cx: &mut Context<Self>) {
        let Some(thread) = self.host.selected.clone() else {
            return;
        };
        let key = (self.host.epoch, self.host.watch_epoch, thread);
        self.images.large_generation += 1;
        let generation = self.images.large_generation;
        self.images.enlarged = Some((reference.clone(), None));
        window.focus(&self.focus, cx);
        cx.notify();
        let expected = key.clone();
        let task = cx.background_executor().spawn(async move {
            let bytes = Client::from_environment()
                .and_then(|client| client.image(&expected.2, &reference))
                .map_err(|error| error.to_string())?;
            decode(&bytes, 1600)
        });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                let actual = (
                    this.host.epoch,
                    this.host.watch_epoch,
                    this.host.selected.clone().unwrap_or_default(),
                );
                if !accepts(&key, &actual) || this.images.large_generation != generation {
                    return;
                }
                if let Some((_, pixels)) = &mut this.images.enlarged {
                    *pixels = Some(result);
                    cx.notify();
                }
            });
        })
        .detach();
    }
    pub(super) fn image_overlay(
        &self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let (reference, pixels) = self.images.enlarged.as_ref()?;
        let bounds = window.viewport_size();
        Some(
            div()
                .absolute()
                .inset_0()
                .bg(rgba(0x000000bb))
                .flex()
                .items_center()
                .justify_center()
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .child(
                    column()
                        .w((bounds.width - px(64.)).min(px(1080.)))
                        .gap_2()
                        .p_3()
                        .bg(rgb(colors().panel))
                        .rounded_lg()
                        .child(
                            row()
                                .justify_between()
                                .child(format!("{} × {}", reference.width, reference.height))
                                .child(button("close-image", "Close").on_activate(
                                    cx,
                                    |this, window, cx| {
                                        this.images.enlarged = None;
                                        this.images.large_generation += 1;
                                        this.focus_composer(window, cx);
                                        cx.notify();
                                    },
                                )),
                        )
                        .child(
                            div()
                                .w_full()
                                .h((bounds.height - px(140.)).max(px(100.)))
                                .flex()
                                .items_center()
                                .justify_center()
                                .child(match pixels {
                                    Some(Ok(pixels)) => img(pixels.clone())
                                        .size_full()
                                        .object_fit(ObjectFit::Contain)
                                        .into_any_element(),
                                    Some(Err(error)) => {
                                        div().child(error.clone()).into_any_element()
                                    }
                                    None => div().child("Loading").into_any_element(),
                                }),
                        ),
                )
                .into_any_element(),
        )
    }
}

#[cfg(test)]
impl ImageCache {
    pub(in crate::ui) fn scene(
        key: (u64, u64, String),
        pixels: HashMap<String, Result<Arc<RenderImage>, String>>,
    ) -> Self {
        Self {
            key: Some(key),
            order: pixels.keys().cloned().collect(),
            ready: pixels,
            ..Default::default()
        }
    }
}

#[cfg(test)]
#[core::prelude::v1::test]
#[ignore = "Run scripts/bench-images.sh with its private offline host"]
fn thumbnail_transfer_bench() {
    use std::time::Instant;
    #[derive(serde::Deserialize)]
    struct Input {
        thread: String,
        image: ImageRef,
    }
    let input: Input = serde_json::from_str(&std::env::var("PLAIT_IMAGE_BENCH").unwrap()).unwrap();
    let client = Client::from_environment().unwrap();
    let mut samples = Vec::new();
    for index in 0..12 {
        let started = Instant::now();
        let bytes = client.image(&input.thread, &input.image).unwrap();
        let pixels = decode(&bytes, 192).unwrap();
        std::hint::black_box(pixels);
        if index >= 2 {
            samples.push(started.elapsed().as_secs_f64() * 1000.);
        }
    }
    samples.sort_by(f64::total_cmp);
    println!(
        "THUMBNAIL_BENCH 10 samples after 2 warmups: median {:.3} ms, p95/max {:.3} ms",
        (samples[4] + samples[5]) / 2.,
        samples[9]
    );
}
