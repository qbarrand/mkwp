use clap::ValueEnum;
use image::{ImageFormat, ImageReader, RgbImage};
use log::info;
use std::{
    ffi::{CStr, CString},
    path::{Path, PathBuf},
    ptr::NonNull,
    slice,
};
use thiserror::Error;

use crate::{Input, metadata};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, ValueEnum)]
pub enum Preset {
    Ultrafast,
    Superfast,
    Veryfast,
    Faster,
    Fast,
    Medium,
    #[default]
    Slow,
    Slower,
    Veryslow,
    Placebo,
}

impl Preset {
    fn as_str(self) -> &'static str {
        match self {
            Self::Ultrafast => "ultrafast",
            Self::Superfast => "superfast",
            Self::Veryfast => "veryfast",
            Self::Faster => "faster",
            Self::Fast => "fast",
            Self::Medium => "medium",
            Self::Slow => "slow",
            Self::Slower => "slower",
            Self::Veryslow => "veryslow",
            Self::Placebo => "placebo",
        }
    }
}

#[derive(Debug, Error)]
pub enum HeifError {
    #[error("image width is out of range")]
    ImageWidthOutOfRange,
    #[error("image height is out of range")]
    ImageHeightOutOfRange,
    #[error("XMP metadata is too large")]
    MetadataTooLarge,
    #[error("libheif error: {0}")]
    LibHeif(String),
}

#[derive(Debug, Error)]
pub enum ImageLoadError {
    #[error("could not read image '{path}': {source}")]
    Read {
        path: String,
        #[source]
        source: image::ImageError,
    },
    #[error("could not identify the image format for '{path}'")]
    UnknownFormat { path: String },
    #[error("unsupported image format for '{path}': expected JPEG or PNG, found {format:?}")]
    UnsupportedFormat { path: String, format: ImageFormat },
}

#[derive(Debug, Error)]
pub enum ImageToHeifError {
    #[error(transparent)]
    Load(#[from] ImageLoadError),
    #[error(transparent)]
    Convert(#[from] HeifError),
}

#[derive(Debug, Error)]
pub enum BuildHeifError {
    #[error("the input JSON contains no images")]
    NoImages,
    #[error("the input JSON must identify exactly one primary image")]
    InvalidPrimaryImageCount,
    #[error(transparent)]
    Image(#[from] ImageToHeifError),
    #[error(transparent)]
    Heif(#[from] HeifError),
    #[error(transparent)]
    Metadata(#[from] metadata::MetadataError),
    #[error("output path '{path}' is not valid UTF-8")]
    OutputPathNotUtf8 { path: PathBuf },
    #[error("output path contains an embedded NUL byte: {path}")]
    OutputPathContainsNul { path: PathBuf },
}

/// Reads a JPEG or PNG file into RGB pixels without resizing or cropping it.
///
/// Alpha is intentionally discarded: macOS Dynamic Desktop images are opaque,
/// and encoding RGBA makes libheif add a separate auxiliary alpha image.
pub fn load_rgb(path: &Path) -> Result<RgbImage, ImageLoadError> {
    let display_path = path.display().to_string();

    let reader = ImageReader::open(path)
        .map_err(|source| ImageLoadError::Read {
            path: display_path.clone(),
            source: image::ImageError::IoError(source),
        })?
        .with_guessed_format()
        .map_err(|source| ImageLoadError::Read {
            path: display_path.clone(),
            source: image::ImageError::IoError(source),
        })?;

    let format = reader
        .format()
        .ok_or_else(|| ImageLoadError::UnknownFormat {
            path: display_path.clone(),
        })?;

    if !matches!(format, ImageFormat::Jpeg | ImageFormat::Png) {
        return Err(ImageLoadError::UnsupportedFormat {
            path: display_path,
            format,
        });
    }

    reader
        .decode()
        .map(|image| image.to_rgb8())
        .map_err(|source| ImageLoadError::Read {
            path: path.display().to_string(),
            source,
        })
}

/// Reads a JPEG or PNG file, then copies its pixels into a libheif image.
///
/// The decoded image retains its original dimensions; this function does not
/// resize, crop, or otherwise transform it.
pub fn load_into_libheif(path: &Path) -> Result<HeifImage, ImageToHeifError> {
    let rgb = load_rgb(path)?;
    Ok(HeifImage::try_from(&rgb)?)
}

/// Encodes the images named by an input manifest into one multi-image HEIF file.
///
/// Relative image paths are resolved against the JSON file's directory. Images
/// retain their source dimensions; this function does not resize or crop them.
pub fn build_heif(
    inputs: &[Input],
    input_json_path: &Path,
    output_path: &Path,
    preset: Preset,
) -> Result<(), BuildHeifError> {
    if inputs.is_empty() {
        return Err(BuildHeifError::NoImages);
    }
    if inputs.iter().filter(|input| input.is_primary()).count() != 1 {
        return Err(BuildHeifError::InvalidPrimaryImageCount);
    }

    let base_dir = input_json_path.parent().unwrap_or_else(|| Path::new("."));
    let context = HeifContext::new()?;
    let encoder = HeifEncoder::new(context.as_ptr())?;
    encoder.set_quality(90)?;
    encoder.set_preset(preset)?;
    let xmp = metadata::xmp(inputs)?;

    for (index, input) in inputs.iter().enumerate() {
        let image_path = base_dir.join(input.file_name());
        info!(
            index = index + 1,
            total = inputs.len(),
            path = image_path.display().to_string();
            "Processing image"
        );

        let image = load_into_libheif(&image_path)?;
        let encoded = context.encode(&image, &encoder)?;
        if input.is_primary() {
            context.set_primary(&encoded)?;
            context.add_xmp_metadata(&encoded, &xmp)?;
        }

        info!(
            index = index + 1,
            total = inputs.len(),
            path = image_path.display().to_string();
            "Finished processing image"
        );
    }

    let output = output_path
        .to_str()
        .ok_or_else(|| BuildHeifError::OutputPathNotUtf8 {
            path: output_path.into(),
        })?;
    let output = CString::new(output).map_err(|_| BuildHeifError::OutputPathContainsNul {
        path: output_path.into(),
    })?;
    context.write_to_file(&output)?;

    Ok(())
}

fn check_heif_error(error: libheif_sys::heif_error) -> Result<(), HeifError> {
    if error.code == libheif_sys::heif_error_code_heif_error_Ok {
        return Ok(());
    }

    // libheif guarantees that `message` is non-null and points to a C string.
    let message = unsafe { CStr::from_ptr(error.message) }
        .to_string_lossy()
        .into_owned();
    Err(HeifError::LibHeif(message))
}

struct HeifContext {
    ptr: NonNull<libheif_sys::heif_context>,
}

impl HeifContext {
    fn new() -> Result<Self, HeifError> {
        let ptr = unsafe { libheif_sys::heif_context_alloc() };
        let ptr = NonNull::new(ptr).ok_or(HeifError::LibHeif(
            "could not allocate a HEIF context".into(),
        ))?;
        Ok(Self { ptr })
    }

    fn as_ptr(&self) -> *mut libheif_sys::heif_context {
        self.ptr.as_ptr()
    }

    fn encode(&self, image: &HeifImage, encoder: &HeifEncoder) -> Result<EncodedImage, HeifError> {
        let mut handle = std::ptr::null_mut();
        unsafe {
            check_heif_error(libheif_sys::heif_context_encode_image(
                self.as_ptr(),
                image.as_ptr(),
                encoder.as_ptr(),
                std::ptr::null(),
                &mut handle,
            ))?;
        }
        let handle = NonNull::new(handle).ok_or(HeifError::LibHeif(
            "libheif encoded an image without returning a handle".into(),
        ))?;
        Ok(EncodedImage { ptr: handle })
    }

    fn set_primary(&self, image: &EncodedImage) -> Result<(), HeifError> {
        unsafe {
            check_heif_error(libheif_sys::heif_context_set_primary_image(
                self.as_ptr(),
                image.as_ptr(),
            ))
        }
    }

    fn add_xmp_metadata(&self, image: &EncodedImage, xmp: &[u8]) -> Result<(), HeifError> {
        let size = i32::try_from(xmp.len()).map_err(|_| HeifError::MetadataTooLarge)?;
        unsafe {
            check_heif_error(libheif_sys::heif_context_add_XMP_metadata(
                self.as_ptr(),
                image.as_ptr(),
                xmp.as_ptr().cast(),
                size,
            ))
        }
    }

    fn write_to_file(&self, output: &CStr) -> Result<(), HeifError> {
        unsafe {
            check_heif_error(libheif_sys::heif_context_write_to_file(
                self.as_ptr(),
                output.as_ptr(),
            ))
        }
    }
}

impl Drop for HeifContext {
    fn drop(&mut self) {
        unsafe { libheif_sys::heif_context_free(self.as_ptr()) };
    }
}

struct HeifEncoder {
    ptr: NonNull<libheif_sys::heif_encoder>,
}

impl HeifEncoder {
    fn new(context: *mut libheif_sys::heif_context) -> Result<Self, HeifError> {
        let mut encoder = std::ptr::null_mut();
        unsafe {
            check_heif_error(libheif_sys::heif_context_get_encoder_for_format(
                context,
                libheif_sys::heif_compression_format_heif_compression_HEVC,
                &mut encoder,
            ))?;
        }
        let ptr = NonNull::new(encoder).ok_or(HeifError::LibHeif(
            "libheif did not return an HEVC encoder".into(),
        ))?;
        Ok(Self { ptr })
    }

    fn as_ptr(&self) -> *mut libheif_sys::heif_encoder {
        self.ptr.as_ptr()
    }

    fn set_quality(&self, quality: i32) -> Result<(), HeifError> {
        unsafe {
            check_heif_error(libheif_sys::heif_encoder_set_lossy_quality(
                self.as_ptr(),
                quality,
            ))
        }
    }

    fn set_preset(&self, preset: Preset) -> Result<(), HeifError> {
        let parameter = c"preset";
        let value = CString::new(preset.as_str()).expect("preset names do not contain NUL bytes");
        unsafe {
            check_heif_error(libheif_sys::heif_encoder_set_parameter_string(
                self.as_ptr(),
                parameter.as_ptr(),
                value.as_ptr(),
            ))
        }
    }
}

impl Drop for HeifEncoder {
    fn drop(&mut self) {
        unsafe { libheif_sys::heif_encoder_release(self.as_ptr()) };
    }
}

struct EncodedImage {
    ptr: NonNull<libheif_sys::heif_image_handle>,
}

impl EncodedImage {
    fn as_ptr(&self) -> *mut libheif_sys::heif_image_handle {
        self.ptr.as_ptr()
    }
}

impl Drop for EncodedImage {
    fn drop(&mut self) {
        unsafe { libheif_sys::heif_image_handle_release(self.as_ptr()) };
    }
}

/// An owned libheif image. The pixels remain valid until this value is dropped.
pub struct HeifImage {
    ptr: NonNull<libheif_sys::heif_image>,
}

impl HeifImage {
    pub fn as_ptr(&self) -> *mut libheif_sys::heif_image {
        self.ptr.as_ptr()
    }
}

impl Drop for HeifImage {
    fn drop(&mut self) {
        unsafe { libheif_sys::heif_image_release(self.ptr.as_ptr()) };
    }
}

impl TryFrom<&RgbImage> for HeifImage {
    type Error = HeifError;

    /// Copies the source into a libheif image without resizing or cropping it.
    ///
    /// libheif owns the destination allocation. Its row stride may be larger
    /// than `source.width() * 3`, so rows are copied individually rather than
    /// as one contiguous slice.
    fn try_from(source: &RgbImage) -> Result<Self, Self::Error> {
        let width = i32::try_from(source.width()).map_err(|_| HeifError::ImageWidthOutOfRange)?;
        let height =
            i32::try_from(source.height()).map_err(|_| HeifError::ImageHeightOutOfRange)?;

        let mut image = std::ptr::null_mut();
        unsafe {
            check_heif_error(libheif_sys::heif_image_create(
                width,
                height,
                libheif_sys::heif_colorspace_heif_colorspace_RGB,
                libheif_sys::heif_chroma_heif_chroma_interleaved_RGB,
                &mut image,
            ))?;
        }
        let image = NonNull::new(image).expect("libheif returned a null image without an error");
        let image = HeifImage { ptr: image };

        unsafe {
            check_heif_error(libheif_sys::heif_image_add_plane(
                image.as_ptr(),
                libheif_sys::heif_channel_heif_channel_interleaved,
                width,
                height,
                8,
            ))?;
        }

        let mut stride = 0_usize;
        let destination = unsafe {
            libheif_sys::heif_image_get_plane2(
                image.as_ptr(),
                libheif_sys::heif_channel_heif_channel_interleaved,
                &mut stride,
            )
        };
        let destination = NonNull::new(destination)
            .expect("libheif did not allocate the requested interleaved RGB plane");

        let source_stride = source.width() as usize * 3;
        let destination =
            unsafe { slice::from_raw_parts_mut(destination.as_ptr(), stride * height as usize) };
        for (source_row, destination_row) in source
            .as_raw()
            .chunks_exact(source_stride)
            .zip(destination.chunks_exact_mut(stride))
        {
            destination_row[..source_stride].copy_from_slice(source_row);
        }

        Ok(image)
    }
}

#[cfg(test)]
mod tests {
    use super::{HeifImage, Preset, build_heif, check_heif_error, load_rgb};
    use image::{ImageFormat, RgbImage, RgbaImage};
    use std::{
        ffi::CString,
        fs, slice,
        time::{SystemTime, UNIX_EPOCH},
    };

    #[test]
    fn loads_png_as_rgb_and_discards_alpha() {
        let source = RgbaImage::from_raw(1, 1, vec![1, 2, 3, 4]).unwrap();
        let path = std::env::temp_dir().join(format!("mkwp-{}-load.png", std::process::id()));
        source.save_with_format(&path, ImageFormat::Png).unwrap();

        let decoded = load_rgb(&path).unwrap();
        fs::remove_file(path).unwrap();

        assert_eq!(decoded.as_raw(), &[1, 2, 3]);
    }

    #[test]
    fn builds_a_multi_image_heif_from_manifest_files() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let directory = std::env::temp_dir().join(format!("mkwp-{unique}"));
        fs::create_dir(&directory).unwrap();

        RgbaImage::from_raw(2, 2, vec![255; 16])
            .unwrap()
            .save_with_format(directory.join("light.png"), ImageFormat::Png)
            .unwrap();
        RgbaImage::from_raw(2, 2, vec![0; 16])
            .unwrap()
            .save_with_format(directory.join("dark.png"), ImageFormat::Png)
            .unwrap();

        let manifest_path = directory.join("wallpaper.json");
        let manifest = r#"
            [
                {"fileName":"light.png","time":"08:00:00"},
                {"fileName":"dark.png","isPrimary":true,"time":"20:00:00"}
            ]
        "#;
        fs::write(&manifest_path, manifest).unwrap();
        let output_path = directory.join("wallpaper.heif");

        let inputs = crate::parse_json(manifest).unwrap();
        build_heif(&inputs, &manifest_path, &output_path, Preset::Slow).unwrap();

        let context = unsafe { libheif_sys::heif_context_alloc() };
        let output = CString::new(output_path.to_str().unwrap()).unwrap();
        unsafe {
            check_heif_error(libheif_sys::heif_context_read_from_file(
                context,
                output.as_ptr(),
                std::ptr::null(),
            ))
            .unwrap();
            assert_eq!(
                libheif_sys::heif_context_get_number_of_top_level_images(context),
                2
            );
            let mut image_ids = [0; 2];
            assert_eq!(
                libheif_sys::heif_context_get_list_of_top_level_image_IDs(
                    context,
                    image_ids.as_mut_ptr(),
                    image_ids.len() as i32,
                ),
                2
            );
            let mut primary_id = 0;
            check_heif_error(libheif_sys::heif_context_get_primary_image_ID(
                context,
                &mut primary_id,
            ))
            .unwrap();
            assert_eq!(primary_id, image_ids[1]);

            let mut primary_handle = std::ptr::null_mut();
            check_heif_error(libheif_sys::heif_context_get_primary_image_handle(
                context,
                &mut primary_handle,
            ))
            .unwrap();
            assert_eq!(
                libheif_sys::heif_image_handle_get_number_of_auxiliary_images(primary_handle, 0,),
                0
            );
            assert_eq!(
                libheif_sys::heif_image_handle_get_number_of_metadata_blocks(
                    primary_handle,
                    c"mime".as_ptr(),
                ),
                1
            );
            let mut metadata_id = 0;
            assert_eq!(
                libheif_sys::heif_image_handle_get_list_of_metadata_block_IDs(
                    primary_handle,
                    c"mime".as_ptr(),
                    &mut metadata_id,
                    1,
                ),
                1
            );
            let metadata_size =
                libheif_sys::heif_image_handle_get_metadata_size(primary_handle, metadata_id);
            let mut metadata = vec![0; metadata_size];
            check_heif_error(libheif_sys::heif_image_handle_get_metadata(
                primary_handle,
                metadata_id,
                metadata.as_mut_ptr().cast(),
            ))
            .unwrap();
            assert_eq!(metadata, crate::metadata::xmp(&inputs).unwrap());

            libheif_sys::heif_image_handle_release(primary_handle);
            libheif_sys::heif_context_free(context);
        }

        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn copies_rgb_pixels_into_a_same_sized_heif_image() {
        let source = RgbImage::from_raw(
            2,
            2,
            vec![
                1, 2, 3, 4, 5, 6, // first row
                7, 8, 9, 10, 11, 12, // second row
            ],
        )
        .unwrap();

        let heif_image = HeifImage::try_from(&source).unwrap();
        let mut stride = 0_usize;
        let pixels = unsafe {
            assert_eq!(
                libheif_sys::heif_image_get_width(
                    heif_image.as_ptr(),
                    libheif_sys::heif_channel_heif_channel_interleaved,
                ),
                2
            );
            assert_eq!(
                libheif_sys::heif_image_get_height(
                    heif_image.as_ptr(),
                    libheif_sys::heif_channel_heif_channel_interleaved,
                ),
                2
            );

            let pixels = libheif_sys::heif_image_get_plane_readonly2(
                heif_image.as_ptr(),
                libheif_sys::heif_channel_heif_channel_interleaved,
                &mut stride,
            );
            slice::from_raw_parts(pixels, stride * 2)
        };

        assert_eq!(&pixels[..6], &source.as_raw()[..6]);
        assert_eq!(&pixels[stride..stride + 6], &source.as_raw()[6..]);
    }
}
