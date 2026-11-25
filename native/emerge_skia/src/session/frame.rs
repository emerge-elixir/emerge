//! The binary headless output is a VideoInterop frame, not a signal that a
//! draw was scheduled. Message tags other than the default stay binaries.
use rustler::{Atom, Binary, Encoder, Env, OwnedBinary, Term};
rustler::atoms! { emerge_skia_frame, rgba8888, bgra8888, rgb888, bt709, iec61966_2_1, rgb, full, center, premultiplied, opaque, progressive, implicit }
#[derive(rustler::NifStruct)]
#[module = "VideoInterop.Rect"]
struct Rect {
    x: u32,
    y: u32,
    width: u32,
    height: u32,
}
#[derive(rustler::NifStruct)]
#[module = "VideoInterop.Binary.Plane"]
struct Plane {
    offset: usize,
    stride: usize,
}
#[derive(rustler::NifStruct)]
#[module = "VideoInterop.Binary"]
struct Storage<'a> {
    data: Binary<'a>,
    planes: Vec<Plane>,
}
#[derive(rustler::NifStruct)]
#[module = "VideoInterop.Binary.Format"]
struct StorageFormat {
    pixel_format: Atom,
    bw1_polarity: Option<Atom>,
}
#[derive(rustler::NifStruct)]
#[module = "VideoInterop.Colorimetry"]
struct Colorimetry {
    primaries: Atom,
    transfer: Atom,
    matrix: Atom,
    range: Atom,
    chroma_location: Atom,
}
#[derive(rustler::NifStruct)]
#[module = "VideoInterop.Format"]
struct Format {
    width: u32,
    height: u32,
    framerate: Option<(u32, u32)>,
    storage: StorageFormat,
    colorimetry: Colorimetry,
    pixel_aspect_ratio: (u32, u32),
    alpha_mode: Atom,
    interlace_mode: Atom,
    acquire_sync: Atom,
}
#[derive(rustler::NifStruct)]
#[module = "VideoInterop.Frame"]
struct Frame<'a> {
    coded_width: u32,
    coded_height: u32,
    visible_rect: Rect,
    format: Format,
    storage: Storage<'a>,
    acquire_sync: Atom,
    lease: Option<Atom>,
}

#[derive(Clone, Copy)]
pub(super) enum Channels {
    Rgba,
    Bgra,
    Rgb,
}
impl Channels {
    pub fn parse(s: &str) -> Result<Self, String> {
        match s {
            "rgba8888" => Ok(Self::Rgba),
            "bgra8888" => Ok(Self::Bgra),
            "rgb888" => Ok(Self::Rgb),
            _ => Err(format!("unsupported headless pixel format: {s}")),
        }
    }
    fn bpp(self) -> usize {
        if matches!(self, Self::Rgb) { 3 } else { 4 }
    }
    pub fn convert(self, rgba: &[u8], out: &mut [u8]) {
        match self {
            Self::Rgba => out.copy_from_slice(rgba),
            Self::Bgra => {
                for (pixel, [r, g, b, a]) in out
                    .as_chunks_mut::<4>()
                    .0
                    .iter_mut()
                    .zip(rgba.as_chunks::<4>().0)
                {
                    *pixel = [*b, *g, *r, *a]
                }
            }
            Self::Rgb => {
                for (pixel, src) in out
                    .as_chunks_mut::<3>()
                    .0
                    .iter_mut()
                    .zip(rgba.as_chunks::<4>().0)
                {
                    pixel.copy_from_slice(&src[..3])
                }
            }
        }
    }
    pub fn prepare(self, rgba: &[u8]) -> Result<OwnedBinary, String> {
        let mut data = OwnedBinary::new(rgba.len() / 4 * self.bpp())
            .ok_or("could not allocate headless frame")?;
        self.convert(rgba, data.as_mut_slice());
        Ok(data)
    }
    pub fn message<'a>(
        self,
        env: Env<'a>,
        tag: &str,
        width: u32,
        height: u32,
        data: OwnedBinary,
    ) -> Term<'a> {
        let stride = width as usize * self.bpp();
        let frame = Frame {
            coded_width: width,
            coded_height: height,
            visible_rect: Rect {
                x: 0,
                y: 0,
                width,
                height,
            },
            acquire_sync: implicit(),
            lease: None,
            storage: Storage {
                data: data.release(env),
                planes: vec![Plane { offset: 0, stride }],
            },
            format: Format {
                width,
                height,
                framerate: None,
                storage: StorageFormat {
                    pixel_format: match self {
                        Self::Rgba => rgba8888(),
                        Self::Bgra => bgra8888(),
                        Self::Rgb => rgb888(),
                    },
                    bw1_polarity: None,
                },
                colorimetry: Colorimetry {
                    primaries: bt709(),
                    transfer: iec61966_2_1(),
                    matrix: rgb(),
                    range: full(),
                    chroma_location: center(),
                },
                pixel_aspect_ratio: (1, 1),
                alpha_mode: if self.bpp() == 4 {
                    premultiplied()
                } else {
                    opaque()
                },
                interlace_mode: progressive(),
                acquire_sync: implicit(),
            },
        };
        if tag == "emerge_skia_frame" {
            (emerge_skia_frame(), frame).encode(env)
        } else {
            (tag, frame).encode(env)
        }
    }
}
