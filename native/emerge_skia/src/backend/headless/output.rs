#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum FramePixelFormat {
    Rgba8888Premul,
    Gray8Opaque,
}

impl FramePixelFormat {
    fn bytes_per_pixel(self) -> usize {
        match self {
            Self::Rgba8888Premul => 4,
            Self::Gray8Opaque => 1,
        }
    }
}

#[derive(Clone, Copy)]
pub(super) struct FrameSource<'a> {
    pub data: &'a [u8],
    pub width: u32,
    pub height: u32,
    pub row_bytes: usize,
    pub format: FramePixelFormat,
}

pub(super) fn packed_gray_stride(width: u32, bits: u8) -> Result<u32, String> {
    let pixels_per_byte = pixels_per_byte(bits)?;
    width
        .checked_add(u32::from(pixels_per_byte - 1))
        .map(|width| width / u32::from(pixels_per_byte))
        .ok_or_else(|| format!("Gray{bits} width overflow"))
}

pub(super) fn packed_gray_output_len(width: u32, height: u32, bits: u8) -> Result<usize, String> {
    usize::try_from(packed_gray_stride(width, bits)?)
        .ok()
        .and_then(|stride| {
            usize::try_from(height)
                .ok()
                .and_then(|height| stride.checked_mul(height))
        })
        .ok_or_else(|| format!("Gray{bits} output dimensions are too large"))
}

pub(super) fn policy_mask_len(width: u32, height: u32) -> Result<usize, String> {
    let pixels = usize::try_from(width)
        .ok()
        .and_then(|width| {
            usize::try_from(height)
                .ok()
                .and_then(|height| width.checked_mul(height))
        })
        .ok_or_else(|| "grayscale policy dimensions are too large".to_string())?;
    pixels
        .checked_add(7)
        .map(|pixels| pixels / 8)
        .ok_or_else(|| "grayscale policy dimensions are too large".to_string())
}

#[cfg(test)]
fn set_policy_dither(mask: &mut [u8], pixel_index: usize, dither: bool) {
    let byte = pixel_index / 8;
    let bit = 7 - pixel_index % 8;
    if dither {
        mask[byte] |= 1 << bit;
    } else {
        mask[byte] &= !(1 << bit);
    }
}

fn policy_dithers(mask: &[u8], pixel_index: usize) -> bool {
    let byte = pixel_index / 8;
    let bit = 7 - pixel_index % 8;
    mask[byte] & (1 << bit) != 0
}

pub(super) fn convert_packed_gray_into(
    source: FrameSource<'_>,
    bits: u8,
    bw1_polarity: &str,
    dither_policy: Option<&[u8]>,
    output: &mut [u8],
) -> Result<u32, String> {
    validate_source(source)?;
    let pixels_per_byte = pixels_per_byte(bits)?;
    if bits == 1 && !matches!(bw1_polarity, "one_is_black" | "one_is_white") {
        return Err(format!("unsupported BW1 polarity: {bw1_polarity}"));
    }

    let stride = packed_gray_stride(source.width, bits)?;
    let expected_output = packed_gray_output_len(source.width, source.height, bits)?;
    if output.len() != expected_output {
        return Err(format!(
            "Gray{bits} output length mismatch: expected {expected_output}, got {}",
            output.len()
        ));
    }
    if let Some(mask) = dither_policy {
        let expected = policy_mask_len(source.width, source.height)?;
        if mask.len() != expected {
            return Err(format!(
                "grayscale policy length mismatch: expected {expected}, got {}",
                mask.len()
            ));
        }
    }

    output.fill(0);
    let target = QuantizationTarget {
        bits,
        pixels_per_byte,
        max_value: (1_u8 << bits) - 1,
        bw1_one_is_black: bits == 1 && bw1_polarity == "one_is_black",
    };
    if let Some(mask) = dither_policy {
        convert_atkinson(source, target, mask, output, stride as usize);
    } else {
        convert_nearest(source, target, output, stride as usize);
    }
    Ok(stride)
}

fn pixels_per_byte(bits: u8) -> Result<u8, String> {
    match bits {
        1 | 2 => Ok(8 / bits),
        _ => Err(format!("unsupported packed grayscale bit depth: {bits}")),
    }
}

fn validate_source(source: FrameSource<'_>) -> Result<(), String> {
    let width =
        usize::try_from(source.width).map_err(|_| "frame width is too large".to_string())?;
    let height =
        usize::try_from(source.height).map_err(|_| "frame height is too large".to_string())?;
    let min_row_bytes = width
        .checked_mul(source.format.bytes_per_pixel())
        .ok_or_else(|| "frame row size overflow".to_string())?;
    if source.row_bytes < min_row_bytes {
        return Err(format!(
            "frame row stride is too short: expected at least {min_row_bytes}, got {}",
            source.row_bytes
        ));
    }
    let expected = source
        .row_bytes
        .checked_mul(height)
        .ok_or_else(|| "frame byte size overflow".to_string())?;
    if source.data.len() != expected {
        return Err(format!(
            "frame byte length mismatch: expected {expected}, got {}",
            source.data.len()
        ));
    }
    Ok(())
}

#[derive(Clone, Copy)]
struct QuantizationTarget {
    bits: u8,
    pixels_per_byte: u8,
    max_value: u8,
    bw1_one_is_black: bool,
}

impl QuantizationTarget {
    fn nearest_level(self, luma: i32) -> u8 {
        ((luma * i32::from(self.max_value) + 127) / 255) as u8
    }

    fn luma_for_level(self, level: u8) -> i32 {
        i32::from(level) * 255 / i32::from(self.max_value)
    }

    fn encoded_level(self, level: u8) -> u8 {
        if self.bw1_one_is_black {
            self.max_value - level
        } else {
            level
        }
    }
}

fn convert_nearest(
    source: FrameSource<'_>,
    target: QuantizationTarget,
    output: &mut [u8],
    output_stride: usize,
) {
    let width = source.width as usize;
    for y in 0..source.height as usize {
        for x in 0..width {
            let level = target.nearest_level(i32::from(luma_at(source, x, y)));
            write_level(output, output_stride, x, y, level, target);
        }
    }
}

fn convert_atkinson(
    source: FrameSource<'_>,
    target: QuantizationTarget,
    policy: &[u8],
    output: &mut [u8],
    output_stride: usize,
) {
    let width = source.width as usize;
    let height = source.height as usize;
    let padding = 2;
    let error_width = width + padding * 2;
    let mut errors = vec![vec![0_i32; error_width]; 3];

    for y in 0..height {
        let current = y % 3;
        let next = (y + 1) % 3;
        let next_two = (y + 2) % 3;
        if y + 2 < height {
            errors[next_two].fill(0);
        }

        for x in 0..width {
            let pixel_index = y * width + x;
            let error_x = x + padding;
            if !policy_dithers(policy, pixel_index) {
                errors[current][error_x] = 0;
                let level = target.nearest_level(i32::from(luma_at(source, x, y)));
                write_level(output, output_stride, x, y, level, target);
                continue;
            }

            let adjusted =
                (i32::from(luma_at(source, x, y)) + errors[current][error_x]).clamp(0, 255);
            let level = target.nearest_level(adjusted);
            write_level(output, output_stride, x, y, level, target);

            let share = (adjusted - target.luma_for_level(level)) / 8;
            if share == 0 {
                continue;
            }

            distribute_error(
                &mut errors,
                current,
                error_x + 1,
                share,
                policy,
                width,
                height,
                x + 1,
                y,
            );
            distribute_error(
                &mut errors,
                current,
                error_x + 2,
                share,
                policy,
                width,
                height,
                x + 2,
                y,
            );
            if y + 1 < height {
                if x > 0 {
                    distribute_error(
                        &mut errors,
                        next,
                        error_x - 1,
                        share,
                        policy,
                        width,
                        height,
                        x - 1,
                        y + 1,
                    );
                }
                distribute_error(
                    &mut errors,
                    next,
                    error_x,
                    share,
                    policy,
                    width,
                    height,
                    x,
                    y + 1,
                );
                distribute_error(
                    &mut errors,
                    next,
                    error_x + 1,
                    share,
                    policy,
                    width,
                    height,
                    x + 1,
                    y + 1,
                );
            }
            if y + 2 < height {
                distribute_error(
                    &mut errors,
                    next_two,
                    error_x,
                    share,
                    policy,
                    width,
                    height,
                    x,
                    y + 2,
                );
            }
        }
        errors[current].fill(0);
    }
}

#[allow(clippy::too_many_arguments)]
fn distribute_error(
    errors: &mut [Vec<i32>],
    row: usize,
    error_x: usize,
    share: i32,
    policy: &[u8],
    width: usize,
    height: usize,
    target_x: usize,
    target_y: usize,
) {
    if target_x >= width || target_y >= height {
        return;
    }
    let pixel_index = target_y * width + target_x;
    if policy_dithers(policy, pixel_index) {
        errors[row][error_x] += share;
    }
}

fn luma_at(source: FrameSource<'_>, x: usize, y: usize) -> u8 {
    let row = y * source.row_bytes;
    match source.format {
        FramePixelFormat::Gray8Opaque => source.data[row + x],
        FramePixelFormat::Rgba8888Premul => {
            let offset = row + x * 4;
            let rgba = &source.data[offset..offset + 4];
            let inverse_alpha = 255_u16 - u16::from(rgba[3]);
            let red = (u16::from(rgba[0]) + inverse_alpha).min(255);
            let green = (u16::from(rgba[1]) + inverse_alpha).min(255);
            let blue = (u16::from(rgba[2]) + inverse_alpha).min(255);
            ((red * 54 + green * 183 + blue * 19 + 128) >> 8) as u8
        }
    }
}

fn write_level(
    output: &mut [u8],
    stride: usize,
    x: usize,
    y: usize,
    level: u8,
    target: QuantizationTarget,
) {
    let pixel_in_byte = x % usize::from(target.pixels_per_byte);
    let shift = 8 - target.bits * (pixel_in_byte as u8 + 1);
    output[y * stride + x / usize::from(target.pixels_per_byte)] |=
        target.encoded_level(level) << shift;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rgba_source<'a>(data: &'a [u8], width: u32, height: u32) -> FrameSource<'a> {
        FrameSource {
            data,
            width,
            height,
            row_bytes: width as usize * 4,
            format: FramePixelFormat::Rgba8888Premul,
        }
    }

    fn gray_source<'a>(data: &'a [u8], width: u32, height: u32) -> FrameSource<'a> {
        FrameSource {
            data,
            width,
            height,
            row_bytes: width as usize,
            format: FramePixelFormat::Gray8Opaque,
        }
    }

    fn all_dither_policy(width: u32, height: u32) -> Vec<u8> {
        let mut policy = vec![0; policy_mask_len(width, height).unwrap()];
        for pixel in 0..width as usize * height as usize {
            set_policy_dither(&mut policy, pixel, true);
        }
        policy
    }

    #[test]
    fn packs_each_bw1_row_independently_and_zeros_tail_bits() {
        let gray = [0, 255, 0, 255, 0, 255];
        let mut output = vec![0; packed_gray_output_len(3, 2, 1).unwrap()];
        let stride = convert_packed_gray_into(
            gray_source(&gray, 3, 2),
            1,
            "one_is_black",
            None,
            &mut output,
        )
        .unwrap();

        assert_eq!(stride, 1);
        assert_eq!(output, [0b1010_0000, 0b0100_0000]);
    }

    #[test]
    fn packs_width_nine_for_both_bw1_polarities() {
        let gray = [255; 9];
        let mut white = vec![0; packed_gray_output_len(9, 1, 1).unwrap()];
        convert_packed_gray_into(
            gray_source(&gray, 9, 1),
            1,
            "one_is_white",
            None,
            &mut white,
        )
        .unwrap();
        assert_eq!(white, [0xff, 0x80]);

        let mut black = vec![0; packed_gray_output_len(9, 1, 1).unwrap()];
        convert_packed_gray_into(
            gray_source(&gray, 9, 1),
            1,
            "one_is_black",
            None,
            &mut black,
        )
        .unwrap();
        assert_eq!(black, [0x00, 0x00]);
    }

    #[test]
    fn packs_gray2_rows_independently_in_canonical_order() {
        let gray = [0, 85, 170, 255, 170, 85];
        let mut output = vec![0; packed_gray_output_len(3, 2, 2).unwrap()];
        let stride = convert_packed_gray_into(
            gray_source(&gray, 3, 2),
            2,
            "one_is_black",
            None,
            &mut output,
        )
        .unwrap();

        assert_eq!(stride, 1);
        assert_eq!(output, [0b0001_1000, 0b1110_0100]);
    }

    #[test]
    fn gray2_uses_nearest_level_boundaries() {
        let gray = [0, 42, 43, 127, 128, 212, 213, 255];
        let mut output = vec![0; packed_gray_output_len(8, 1, 2).unwrap()];
        convert_packed_gray_into(
            gray_source(&gray, 8, 1),
            2,
            "one_is_black",
            None,
            &mut output,
        )
        .unwrap();

        assert_eq!(output, [0x05, 0xaf]);
    }

    #[test]
    fn composites_premultiplied_rgba_over_white() {
        let rgba = [0, 0, 0, 0, 64, 64, 64, 128, 0, 0, 0, 255];
        let mut bw1 = vec![0; 1];
        convert_packed_gray_into(rgba_source(&rgba, 3, 1), 1, "one_is_white", None, &mut bw1)
            .unwrap();
        assert_eq!(bw1, [0b1100_0000]);

        let mut gray2 = vec![0; 1];
        convert_packed_gray_into(
            rgba_source(&rgba, 3, 1),
            2,
            "one_is_black",
            None,
            &mut gray2,
        )
        .unwrap();
        assert_eq!(gray2, [0b1110_0000]);
    }

    #[test]
    fn rejects_malformed_sources_outputs_policies_and_bit_depths() {
        let source = FrameSource {
            data: &[0, 0, 0],
            width: 1,
            height: 1,
            row_bytes: 4,
            format: FramePixelFormat::Rgba8888Premul,
        };
        assert!(convert_packed_gray_into(source, 2, "one_is_black", None, &mut [0]).is_err());

        let source = gray_source(&[0], 1, 1);
        assert!(convert_packed_gray_into(source, 2, "one_is_black", None, &mut []).is_err());
        assert!(convert_packed_gray_into(source, 4, "one_is_black", None, &mut [0]).is_err());
        assert!(convert_packed_gray_into(source, 1, "invalid", None, &mut [0]).is_err());
        assert!(convert_packed_gray_into(source, 2, "ignored", Some(&[]), &mut [0]).is_err());
    }

    #[test]
    fn atkinson_does_not_cross_protected_pixels() {
        let gray = [100, 100, 100, 100];
        let mut policy = vec![0];
        set_policy_dither(&mut policy, 0, true);
        set_policy_dither(&mut policy, 2, true);
        set_policy_dither(&mut policy, 3, true);
        let mut output = vec![0; 1];
        convert_packed_gray_into(
            gray_source(&gray, 4, 1),
            1,
            "one_is_white",
            Some(&policy),
            &mut output,
        )
        .unwrap();
        assert_eq!(output[0] & 0b0100_0000, 0);
    }

    #[test]
    fn gray2_atkinson_is_deterministic_and_changes_midtones() {
        let gray = [125; 16];
        let policy = all_dither_policy(8, 2);
        let render = || {
            let mut output = vec![0; packed_gray_output_len(8, 2, 2).unwrap()];
            convert_packed_gray_into(
                gray_source(&gray, 8, 2),
                2,
                "one_is_black",
                Some(&policy),
                &mut output,
            )
            .unwrap();
            output
        };
        let first = render();
        assert_eq!(first, render());
        assert_ne!(first, [0x55, 0x55, 0x55, 0x55]);
    }
}
