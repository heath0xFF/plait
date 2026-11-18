use super::*;
use plait_core::images::ImageRef;
use std::collections::HashMap;

pub(super) fn fixtures() -> Vec<(ImageRef, Arc<gpui::RenderImage>)> {
    [
        include_bytes!("../../../../../runtime/test/support/images/0.png").as_slice(),
        include_bytes!("../../../../../runtime/test/support/images/1.png").as_slice(),
        include_bytes!("../../../../../runtime/test/support/images/2.png").as_slice(),
    ]
    .into_iter()
    .map(|bytes| {
        (
            plait_core::images::inspect(bytes).unwrap(),
            super::super::images::decode(bytes, 192).unwrap(),
        )
    })
    .collect()
}
pub(super) fn install(harness: &mut Harness, count: usize) {
    let fixture = fixtures();
    let references = (0..count)
        .map(|index| {
            if count <= 3 {
                fixture[index % fixture.len()].0.clone()
            } else {
                let mut bytes =
                    include_bytes!("../../../../../runtime/test/support/images/0.png").to_vec();
                bytes.extend_from_slice(&index.to_be_bytes());
                plait_core::images::inspect(&bytes).unwrap()
            }
        })
        .collect::<Vec<_>>();
    harness.update(|plait, _, cx| {
        plait.host.status.as_mut().unwrap().protocol_version = 6;
        let mut snapshot = (**plait.host.snapshot.as_ref().unwrap()).clone();
        snapshot.image_input = Some("yes".into());
        snapshot.messages = (0..count)
            .map(|index| Message {
                id: format!("image-request-{index}"),
                role: MessageRole::User,
                text: format!("Review image {}.", index + 1),
                images: vec![references[index].clone()],
                ..Default::default()
            })
            .collect();
        if count == 3 {
            snapshot.messages = vec![Message {
                id: "image-request-0".into(),
                role: MessageRole::User,
                text: "Compare these three screens.".into(),
                images: references.clone(),
                ..Default::default()
            }];
        }
        snapshot.tools.clear();
        snapshot.live_text.clear();
        snapshot.status = ThreadStatus::Idle;
        plait.host.snapshot = Some(Arc::new(snapshot));
        plait.history = Default::default();
        plait.formatted = Arc::new(plait_core::markdown::Transcript::update(
            &Default::default(),
            plait.host.snapshot.as_ref().unwrap(),
        ));
        plait.images = super::super::images::ImageCache::scene(
            (plait.host.epoch, plait.host.watch_epoch, THREAD.into()),
            references
                .iter()
                .rev()
                .take(32)
                .map(|reference| {
                    let pixels = fixture
                        .iter()
                        .find(|(image, _)| image.id == reference.id)
                        .unwrap_or(&fixture[0])
                        .1
                        .clone();
                    (reference.id.clone(), Ok(pixels))
                })
                .collect(),
        );
        plait.refresh_live_list(None);
        plait.live_scroll.scroll_to_end();
        plait.sync_composer(cx);
    });
}
pub(super) fn walk(harness: &mut Harness, repository: &Path, suffix: &str) {
    harness.fresh_composer(repository);
    install(harness, 1);
    let fixture = fixtures();
    harness.composer(|composer, window, cx| {
        composer.type_text("Compare the attached screens.", window, cx);
        composer.scene_images(
            vec![fixture[0].0.clone()],
            HashMap::from([(fixture[0].0.id.clone(), fixture[0].1.clone())]),
            cx,
        );
    });
    harness.capture(&format!("images-composer-one-{suffix}"));
    harness.composer(|composer, _, cx| {
        composer.scene_images(
            fixture.iter().map(|(r, _)| r.clone()).collect(),
            fixture
                .iter()
                .map(|(r, p)| (r.id.clone(), p.clone()))
                .collect(),
            cx,
        )
    });
    harness.capture(&format!("images-composer-several-{suffix}"));
    harness.composer(|composer, _, cx| composer.set_image_support(6, Some("no"), cx));
    harness.capture(&format!("images-text-only-{suffix}"));
    harness.composer(|composer, _, cx| composer.set_image_support(5, Some("yes"), cx));
    harness.capture(&format!("images-older-host-{suffix}"));
    harness.composer(|composer, _, cx| {
        composer.scene_images(Vec::new(), HashMap::new(), cx);
        composer.set_image_support(6, Some("yes"), cx);
    });
    install(harness, 3);
    harness.capture(&format!("images-conversation-{suffix}"));
    harness.update(|plait, _, _| {
        plait.images.enlarged = Some((
            fixture[0].0.clone(),
            Some(Ok(super::super::images::decode(
                include_bytes!("../../../../../runtime/test/support/images/0.png"),
                1600,
            )
            .unwrap())),
        ))
    });
    harness.capture(&format!("images-enlarged-{suffix}"));
    harness.update(|plait, _, _| plait.images.enlarged = None);
}
