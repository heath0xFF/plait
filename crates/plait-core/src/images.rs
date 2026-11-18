//! Bounded originals and protocol transfer. Every filesystem/socket call is background-only.
use crate::host::{Client, Error, Request};
use base64::{Engine, engine::general_purpose::STANDARD};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, OpenOptions},
    io::{Read, Write},
    os::unix::fs::{MetadataExt, OpenOptionsExt},
    path::Path,
};

pub const IMAGE_BYTES: usize = 2 * 1024 * 1024;
pub const MESSAGE_BYTES: usize = 4 * 1024 * 1024;
pub const COUNT: usize = 4;
pub const CHUNK: usize = 32768;
pub const PIXELS: u64 = 16 * 1024 * 1024;

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ImageRef {
    pub id: String,
    pub media_type: String,
    pub size: usize,
    pub width: u32,
    pub height: u32,
}
impl ImageRef {
    pub fn validate(&self) -> Result<(), String> {
        if self.id.len() != 64
            || !self
                .id
                .bytes()
                .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
        {
            return Err("Invalid image ID.".into());
        }
        if !matches!(self.media_type.as_str(), "image/png" | "image/jpeg") {
            return Err("Only PNG and JPEG images are supported.".into());
        }
        if self.size == 0 || self.size > IMAGE_BYTES {
            return Err("Each image must be at most 2 MiB.".into());
        }
        dimensions(self.width, self.height)
    }
}
fn dimensions(width: u32, height: u32) -> Result<(), String> {
    if width == 0 || height == 0 {
        return Err("Image dimensions could not be read.".into());
    }
    if width > 8192 || height > 8192 || u64::from(width) * u64::from(height) > PIXELS {
        return Err(
            "Images must fit within 8192 pixels per side and 16,777,216 pixels total.".into(),
        );
    }
    Ok(())
}
pub fn validate_message(images: &[ImageRef]) -> Result<(), String> {
    if images.len() > COUNT {
        return Err("A message can have at most 4 images.".into());
    }
    let mut seen = std::collections::HashSet::new();
    for image in images {
        image.validate()?;
        if !seen.insert(&image.id) {
            return Err("This image is already attached.".into());
        }
    }
    if images.iter().map(|image| image.size).sum::<usize>() > MESSAGE_BYTES {
        return Err("Images in a message must total at most 4 MiB.".into());
    }
    Ok(())
}
/// No pixel allocation: check bytes and dimensions before decoding or retaining anything.
pub fn inspect(bytes: &[u8]) -> Result<ImageRef, String> {
    if bytes.is_empty() || bytes.len() > IMAGE_BYTES {
        return Err("Each image must be at most 2 MiB.".into());
    }
    let (media_type, width, height) = if bytes.len() >= 33
        && bytes.starts_with(b"\x89PNG\r\n\x1a\n")
        && &bytes[12..16] == b"IHDR"
        && bytes[8..12] == [0, 0, 0, 13]
    {
        (
            "image/png",
            u32::from_be_bytes(bytes[16..20].try_into().unwrap()),
            u32::from_be_bytes(bytes[20..24].try_into().unwrap()),
        )
    } else if bytes.starts_with(&[255, 216]) {
        let mut offset = 2;
        let mut size = None;
        while offset + 4 <= bytes.len() {
            if bytes[offset] != 255 {
                break;
            }
            offset += 1;
            while bytes.get(offset) == Some(&255) {
                offset += 1;
            }
            let Some(&marker) = bytes.get(offset) else {
                break;
            };
            offset += 1;
            if matches!(marker, 217 | 218) || offset + 2 > bytes.len() {
                break;
            }
            let length = usize::from(u16::from_be_bytes([bytes[offset], bytes[offset + 1]]));
            if length < 2 || offset + length > bytes.len() {
                break;
            }
            if matches!(
                marker,
                192 | 193 | 194 | 195 | 197 | 198 | 199 | 201 | 202 | 203 | 205 | 206 | 207
            ) && length >= 8
            {
                size = Some((
                    u32::from(u16::from_be_bytes([bytes[offset + 5], bytes[offset + 6]])),
                    u32::from(u16::from_be_bytes([bytes[offset + 3], bytes[offset + 4]])),
                ));
                break;
            }
            offset += length;
        }
        let (width, height) = size.ok_or("Image dimensions could not be read.")?;
        ("image/jpeg", width, height)
    } else {
        return Err("Only PNG and JPEG images are supported.".into());
    };
    dimensions(width, height)?;
    Ok(ImageRef {
        id: format!("{:x}", Sha256::digest(bytes)),
        media_type: media_type.into(),
        size: bytes.len(),
        width,
        height,
    })
}
/// Refuse links, devices, directories and a file growing beyond the bound.
pub fn read_file(path: &Path) -> Result<Vec<u8>, String> {
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
        .map_err(|_| "The image file could not be opened.")?;
    let metadata = file
        .metadata()
        .map_err(|_| "The image file could not be read.")?;
    if !metadata.is_file() {
        return Err("Choose a regular PNG or JPEG file.".into());
    }
    if metadata.len() > IMAGE_BYTES as u64 {
        return Err("Each image must be at most 2 MiB.".into());
    }
    let mut bytes = Vec::new();
    file.take(IMAGE_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "The image file could not be read.")?;
    inspect(&bytes)?;
    Ok(bytes)
}
fn directory() -> Result<std::path::PathBuf, String> {
    let directory = crate::session::locations()?.0.join("images");
    fs::create_dir_all(&directory).map_err(|_| "Cannot save draft images.")?;
    let metadata =
        fs::symlink_metadata(&directory).map_err(|_| "Cannot inspect draft image storage.")?;
    if !metadata.is_dir() || metadata.uid() != unsafe { libc::geteuid() } {
        return Err("Draft image storage is not a private directory.".into());
    }
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(&directory, fs::Permissions::from_mode(0o700))
        .map_err(|_| "Cannot protect draft image storage.")?;
    Ok(directory)
}
pub fn persist(bytes: &[u8]) -> Result<ImageRef, String> {
    let image = inspect(bytes)?;
    let path = directory()?.join(&image.id);
    match OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&path)
    {
        Ok(mut file) => {
            file.write_all(bytes)
                .and_then(|_| file.sync_all())
                .map_err(|_| "Cannot save draft image.")?;
        }
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            if read_file(&path)? != bytes {
                return Err("Saved draft image differs from its ID.".into());
            }
        }
        Err(_) => return Err("Cannot save draft image.".into()),
    }
    std::fs::File::open(directory()?)
        .and_then(|file| file.sync_all())
        .map_err(|_| "Cannot sync draft image storage.")?;
    Ok(image)
}
pub fn local(image: &ImageRef) -> Result<Vec<u8>, String> {
    image.validate()?;
    let bytes = read_file(&directory()?.join(&image.id))?;
    if &inspect(&bytes)? != image {
        return Err("Saved draft image differs from its reference.".into());
    }
    Ok(bytes)
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Upload {
    offset: usize,
    image: Option<ImageRef>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Part {
    image: ImageRef,
    offset: usize,
    data: String,
}
impl Client {
    pub fn upload_images(&self, thread: &str, images: &[ImageRef]) -> Result<(), Error> {
        validate_message(images).map_err(Error::Protocol)?;
        for image in images {
            let bytes = local(image).map_err(Error::Protocol)?;
            let mut offset = 0;
            while offset < bytes.len() {
                let end = (offset + CHUNK).min(bytes.len());
                let reply: Upload = self.request(&Request::ImagePut {
                    protocol_version: 6,
                    thread_id: thread.into(),
                    image_id: image.id.clone(),
                    offset,
                    total: bytes.len(),
                    data: STANDARD.encode(&bytes[offset..end]),
                })?;
                if reply.offset < end
                    || reply.offset > bytes.len()
                    || (reply.offset == bytes.len() && reply.image.as_ref() != Some(image))
                {
                    return Err(Error::Protocol(
                        "Host returned an invalid image upload acknowledgement.".into(),
                    ));
                }
                offset = reply.offset;
            }
        }
        Ok(())
    }
    pub fn image(&self, thread: &str, image: &ImageRef) -> Result<Vec<u8>, Error> {
        image.validate().map_err(Error::Protocol)?;
        let mut bytes = Vec::with_capacity(image.size);
        while bytes.len() < image.size {
            let part: Part = self.request(&Request::ImageRead {
                protocol_version: 6,
                thread_id: thread.into(),
                image_id: image.id.clone(),
                offset: bytes.len(),
            })?;
            if part.image != *image
                || part.offset != bytes.len()
                || part.data.len() > CHUNK.div_ceil(3) * 4
            {
                return Err(Error::Protocol(
                    "Host returned an invalid image part.".into(),
                ));
            }
            let chunk = STANDARD
                .decode(&part.data)
                .map_err(|_| Error::Protocol("Invalid image encoding.".into()))?;
            if chunk.is_empty() || chunk.len() > CHUNK || bytes.len() + chunk.len() > image.size {
                return Err(Error::Protocol(
                    "Host image exceeds its declared size.".into(),
                ));
            }
            bytes.extend(chunk);
        }
        if inspect(&bytes).map_err(Error::Protocol)? != *image {
            return Err(Error::Protocol(
                "Host image differs from its reference.".into(),
            ));
        }
        Ok(bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    // Header fixture: this module reads dimensions, never decodes pixels.
    const PNG: &[u8] = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR\0\0\x01\x40\0\0\0\xb4\x08\x02\0\0\0\0\0\0\0";
    #[test]
    fn bytes_decide_type_and_limits_precede_storage() {
        let image = inspect(PNG).unwrap();
        assert_eq!(image.media_type, "image/png");
        assert_eq!((image.width, image.height), (320, 180));
        assert!(
            inspect(b"a PNG named picture.jpg")
                .unwrap_err()
                .contains("PNG and JPEG")
        );
        assert!(
            inspect(&vec![0; IMAGE_BYTES + 1])
                .unwrap_err()
                .contains("2 MiB")
        );
        let mut enormous = PNG.to_vec();
        enormous[16..20].copy_from_slice(&9000u32.to_be_bytes());
        assert!(inspect(&enormous).is_err());
        enormous[16..20].copy_from_slice(&8192u32.to_be_bytes());
        enormous[20..24].copy_from_slice(&8192u32.to_be_bytes());
        assert!(inspect(&enormous).is_err());
        assert!(validate_message(&vec![image.clone(); 5]).is_err());
        let mut refs = Vec::new();
        for n in 0..3 {
            let mut image = image.clone();
            image.id = format!("{n:064x}");
            image.size = IMAGE_BYTES;
            refs.push(image);
        }
        assert!(validate_message(&refs).unwrap_err().contains("4 MiB"));
        let jpeg = [
            255, 216, 255, 192, 0, 11, 8, 0, 16, 0, 32, 1, 1, 17, 0, 255, 217,
        ];
        assert_eq!(inspect(&jpeg).unwrap().media_type, "image/jpeg");
        for length in 0..jpeg.len() - 3 {
            assert!(inspect(&jpeg[..length]).is_err());
        }
    }
    #[test]
    fn files_are_bounded_and_special_files_are_refused() {
        let root = std::env::temp_dir().join(format!("plait-images-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let path = root.join("misleading.jpg");
        fs::write(&path, PNG).unwrap();
        assert_eq!(
            inspect(&read_file(&path).unwrap()).unwrap().media_type,
            "image/png"
        );
        let link = root.join("link");
        std::os::unix::fs::symlink(&path, &link).unwrap();
        assert!(read_file(&link).is_err());
        assert!(read_file(&root).is_err());
        fs::write(&path, vec![0; IMAGE_BYTES + 1]).unwrap();
        assert!(read_file(&path).is_err());
        fs::remove_dir_all(root).unwrap();
    }
}
