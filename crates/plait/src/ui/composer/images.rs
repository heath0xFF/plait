use super::super::{images::decode, thread_controls::TextTooltip};
use super::*;
use plait_core::images::{self, ImageRef};
use std::{collections::HashMap, path::PathBuf};

#[derive(Default)]
pub(super) struct Attachments {
    pub references: Vec<ImageRef>,
    pub pixels: HashMap<String, Arc<RenderImage>>,
    pub generation: u64,
    pub loading: bool,
    pub error: Option<String>,
    pub blocked: Option<(String, String)>,
}
impl Composer {
    pub fn saved_images(&self) -> &[ImageRef] {
        &self.images.references
    }
    pub fn set_image_support(
        &mut self,
        protocol: u32,
        support: Option<&str>,
        cx: &mut Context<Self>,
    ) {
        let blocked = if protocol < 6 {
            Some((
                "Update host".into(),
                "Images need a newer host (version 6). Update the host before sending images."
                    .into(),
            ))
        } else if support == Some("no") {
            Some((
                "Text only".into(),
                "This model cannot take images. Remove them or choose another model.".into(),
            ))
        } else {
            None
        };
        if self.images.blocked != blocked {
            self.images.blocked = blocked;
            cx.notify();
        }
    }
    pub fn restore_images(&mut self, references: Vec<ImageRef>, cx: &mut Context<Self>) {
        self.images.generation += 1;
        self.images.references = references.clone();
        self.images.pixels.clear();
        self.images.loading = !references.is_empty();
        let generation = self.images.generation;
        let task = cx.background_executor().spawn(async move {
            images::validate_message(&references)?;
            references
                .into_iter()
                .map(|reference| {
                    let bytes = images::local(&reference)?;
                    Ok((reference.id, decode(&bytes, 192)?))
                })
                .collect::<Result<HashMap<_, _>, String>>()
        });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                if this.images.generation != generation {
                    return;
                }
                this.images.loading = false;
                match result {
                    Ok(pixels) => this.images.pixels = pixels,
                    Err(error) => this.images.error = Some(error),
                }
                cx.notify();
            });
        })
        .detach();
    }
    pub(super) fn image_send_blocked(&self) -> bool {
        self.images.loading || (!self.images.references.is_empty() && self.images.blocked.is_some())
    }
    fn refuse_image(&mut self, reason: String, cx: &mut Context<Self>) {
        self.images.error = Some(reason);
        cx.notify();
    }
    pub(super) fn paste_images(&mut self, bytes: Vec<u8>, cx: &mut Context<Self>) {
        // Clipboard allocation is GPUI's; reject immediately, before retaining or decoding it.
        let info = match images::inspect(&bytes) {
            Ok(info) => info,
            Err(error) => return self.refuse_image(error, cx),
        };
        let mut all = self.images.references.clone();
        all.push(info);
        if let Err(error) = images::validate_message(&all) {
            return self.refuse_image(error, cx);
        }
        self.add_images(Vec::new(), Some(bytes), cx);
    }
    pub(super) fn drop_images(&mut self, paths: Vec<PathBuf>, cx: &mut Context<Self>) {
        if paths.len() + self.images.references.len() > images::COUNT {
            return self.refuse_image("A message can have at most 4 images.".into(), cx);
        }
        self.add_images(paths, None, cx);
    }
    fn add_images(&mut self, paths: Vec<PathBuf>, bytes: Option<Vec<u8>>, cx: &mut Context<Self>) {
        if self.images.loading {
            return self.refuse_image("Wait for the previous images to finish loading.".into(), cx);
        }
        self.images.loading = true;
        self.images.error = None;
        self.images.generation += 1;
        let generation = self.images.generation;
        let current = self.images.references.clone();
        let task = cx.background_executor().spawn(async move {
            let mut inputs = Vec::new();
            if let Some(bytes) = bytes {
                inputs.push(bytes);
            }
            for path in paths {
                inputs.push(images::read_file(&path)?);
            }
            let mut all = current;
            for bytes in &inputs {
                all.push(images::inspect(bytes)?);
                images::validate_message(&all)?;
            }
            inputs
                .into_iter()
                .map(|bytes| {
                    let pixels = decode(&bytes, 192)?;
                    let reference = images::persist(&bytes)?;
                    Ok((reference, pixels))
                })
                .collect::<Result<Vec<_>, String>>()
        });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                if this.images.generation != generation {
                    return;
                }
                this.images.loading = false;
                match result {
                    Ok(added) => {
                        for (reference, pixels) in added {
                            this.images.pixels.insert(reference.id.clone(), pixels);
                            this.images.references.push(reference);
                        }
                        this.draft.revision += 1;
                    }
                    Err(error) => this.images.error = Some(error),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }
    pub(super) fn image_strip(&self, cx: &mut Context<Self>) -> AnyElement {
        let mut strip = theme::row().flex_wrap().gap_2();
        for reference in &self.images.references {
            let id = reference.id.clone();
            strip = strip.child(
                theme::column()
                    .gap_1()
                    .child(
                        div()
                            .w(px(80.))
                            .h(px(64.))
                            .child(match self.images.pixels.get(&id) {
                                Some(pixels) => img(pixels.clone())
                                    .size_full()
                                    .object_fit(ObjectFit::Contain)
                                    .into_any_element(),
                                None => div().child("Loading").into_any_element(),
                            }),
                    )
                    .child(
                        theme::button(SharedString::from(format!("remove-{id}")), "Remove")
                            .h(px(22.))
                            .aria_label("Remove attached image")
                            .on_activate(cx, move |this, _, cx| {
                                this.images
                                    .references
                                    .retain(|reference| reference.id != id);
                                this.images.pixels.remove(&id);
                                this.images.error = None;
                                this.draft.revision += 1;
                                cx.notify();
                            }),
                    ),
            );
        }
        let state = if let Some(error) = &self.images.error {
            Some(("Refused".to_owned(), error.clone()))
        } else if self.images.loading {
            Some(("Loading".into(), "Preparing image thumbnails.".into()))
        } else if !self.images.references.is_empty() {
            self.images.blocked.clone()
        } else {
            None
        };
        if let Some((label, detail)) = state {
            strip = strip.child(
                theme::button("image-state", label)
                    .text_color(rgb(colors().amber))
                    .tooltip(move |_, cx| cx.new(|_| TextTooltip(detail.clone())).into()),
            );
        }
        strip.into_any_element()
    }
}

#[cfg(test)]
impl Composer {
    pub(in crate::ui) fn scene_images(
        &mut self,
        references: Vec<ImageRef>,
        pixels: HashMap<String, Arc<RenderImage>>,
        cx: &mut Context<Self>,
    ) {
        self.images.references = references;
        self.images.pixels = pixels;
        self.images.loading = false;
        self.draft.revision += 1;
        cx.notify();
    }
    pub(in crate::ui) fn scene_refuse(&mut self, bytes: Vec<u8>, cx: &mut Context<Self>) {
        self.paste_images(bytes, cx);
    }
    pub(in crate::ui) fn scene_drop(&mut self, paths: Vec<PathBuf>, cx: &mut Context<Self>) {
        self.drop_images(paths, cx);
    }
    pub(in crate::ui) fn scene_submit(&mut self, cx: &mut Context<Self>) {
        self.submit(false, cx);
    }
}
