use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use av_data::pixel::{
  ChromaLocation, ColorPrimaries, MatrixCoefficients, TransferCharacteristic, YUVRange,
};
use ffmpeg::format::{input, Pixel};
use ffmpeg::media::Type as MediaType;
use ffmpeg::Error::StreamNotFound;
use path_abs::{PathAbs, PathInfo};

use crate::colorimetry::Colorimetry;
use crate::{into_array, into_vec};

pub fn compose_ffmpeg_pipe<S: Into<String>>(
  params: impl IntoIterator<Item = S>,
  pix_format: Pixel,
) -> Vec<String> {
  let mut p: Vec<String> = into_vec![
    "ffmpeg",
    "-y",
    "-hide_banner",
    "-loglevel",
    "error",
    "-i",
    "-",
  ];

  p.extend(params.into_iter().map(Into::into));

  p.extend(into_array![
    "-pix_fmt",
    pix_format.descriptor().unwrap().name(),
    "-strict",
    "-1",
    "-f",
    "yuv4mpegpipe",
    "-"
  ]);

  p
}

/// Get frame count using FFmpeg
#[tracing::instrument]
pub fn num_frames(source: &Path) -> Result<usize, ffmpeg::Error> {
  let mut ictx = input(source)?;
  let input = ictx
    .streams()
    .best(MediaType::Video)
    .ok_or(StreamNotFound)?;
  let video_stream_index = input.index();

  Ok(
    ictx
      .packets()
      .filter_map(Result::ok)
      .filter(|(stream, _)| stream.index() == video_stream_index)
      .count(),
  )
}

#[tracing::instrument]
pub fn frame_rate(source: &Path) -> Result<f64, ffmpeg::Error> {
  let ictx = input(source)?;
  let input = ictx
    .streams()
    .best(MediaType::Video)
    .ok_or(StreamNotFound)?;
  let rate = input.avg_frame_rate();
  Ok(f64::from(rate.numerator()) / f64::from(rate.denominator()))
}

#[tracing::instrument]
pub fn get_pixel_format(source: &Path) -> Result<Pixel, ffmpeg::Error> {
  let ictx = ffmpeg::format::input(source)?;

  let input = ictx
    .streams()
    .best(MediaType::Video)
    .ok_or(StreamNotFound)?;

  let decoder = ffmpeg::codec::context::Context::from_parameters(input.parameters())?
    .decoder()
    .video()?;

  Ok(decoder.format())
}

#[tracing::instrument]
pub fn resolution(source: &Path) -> Result<(u32, u32), ffmpeg::Error> {
  let ictx = ffmpeg::format::input(source)?;

  let input = ictx
    .streams()
    .best(MediaType::Video)
    .ok_or(StreamNotFound)?;

  let decoder = ffmpeg::codec::context::Context::from_parameters(input.parameters())?
    .decoder()
    .video()?;

  Ok((decoder.width(), decoder.height()))
}

#[tracing::instrument]
pub fn colorimetry(source: &Path) -> anyhow::Result<Colorimetry> {
  let ictx = ffmpeg::format::input(source)?;

  let input = ictx
    .streams()
    .best(MediaType::Video)
    .ok_or(StreamNotFound)?;

  let decoder = ffmpeg::codec::context::Context::from_parameters(input.parameters())?
    .decoder()
    .video()?;

  Ok(Colorimetry {
    range: match decoder.color_range() {
      // The safer assumption for video content is Limited
      ffmpeg::color::Range::Unspecified => YUVRange::Limited,
      ffmpeg::color::Range::MPEG => YUVRange::Limited,
      ffmpeg::color::Range::JPEG => YUVRange::Full,
    },
    primaries: match decoder.color_primaries() {
      ffmpeg::color::Primaries::Reserved0 => ColorPrimaries::Reserved0,
      ffmpeg::color::Primaries::BT709 => ColorPrimaries::BT709,
      ffmpeg::color::Primaries::Unspecified => ColorPrimaries::Unspecified,
      ffmpeg::color::Primaries::Reserved => ColorPrimaries::Reserved,
      ffmpeg::color::Primaries::BT470M => ColorPrimaries::BT470M,
      ffmpeg::color::Primaries::BT470BG => ColorPrimaries::BT470BG,
      ffmpeg::color::Primaries::SMPTE170M => ColorPrimaries::ST170M,
      ffmpeg::color::Primaries::SMPTE240M => ColorPrimaries::ST240M,
      ffmpeg::color::Primaries::Film => ColorPrimaries::Film,
      ffmpeg::color::Primaries::BT2020 => ColorPrimaries::BT2020,
      ffmpeg::color::Primaries::SMPTE428 => ColorPrimaries::ST428,
      ffmpeg::color::Primaries::SMPTE431 => ColorPrimaries::P3DCI,
      ffmpeg::color::Primaries::SMPTE432 => ColorPrimaries::P3Display,
      ffmpeg::color::Primaries::EBU3213 => ColorPrimaries::Tech3213,
    },
    matrix: match decoder.color_space() {
      ffmpeg::color::Space::RGB => MatrixCoefficients::Identity,
      ffmpeg::color::Space::BT709 => MatrixCoefficients::BT709,
      ffmpeg::color::Space::Unspecified => MatrixCoefficients::Unspecified,
      ffmpeg::color::Space::Reserved => MatrixCoefficients::Reserved,
      ffmpeg::color::Space::FCC => MatrixCoefficients::BT470M,
      ffmpeg::color::Space::BT470BG => MatrixCoefficients::BT470BG,
      ffmpeg::color::Space::SMPTE170M => MatrixCoefficients::ST170M,
      ffmpeg::color::Space::SMPTE240M => MatrixCoefficients::ST240M,
      ffmpeg::color::Space::YCGCO => MatrixCoefficients::YCgCo,
      ffmpeg::color::Space::BT2020NCL => MatrixCoefficients::BT2020NonConstantLuminance,
      ffmpeg::color::Space::BT2020CL => MatrixCoefficients::BT2020ConstantLuminance,
      ffmpeg::color::Space::SMPTE2085 => MatrixCoefficients::ST2085,
      ffmpeg::color::Space::ChromaDerivedNCL => {
        MatrixCoefficients::ChromaticityDerivedNonConstantLuminance
      }
      ffmpeg::color::Space::ChromaDerivedCL => {
        MatrixCoefficients::ChromaticityDerivedConstantLuminance
      }
      ffmpeg::color::Space::ICTCP => MatrixCoefficients::ICtCp,
      // FIXME: I don't know if these are the correct mappings,
      // but there's no public information on them because you have to PAY
      // for the freaking ISO spec!
      ffmpeg::color::Space::IPTC2 => MatrixCoefficients::ICtCp,
      ffmpeg::color::Space::YCGCORE => MatrixCoefficients::YCgCo,
      ffmpeg::color::Space::YCGCORO => MatrixCoefficients::YCgCo,
    },
    transfer: match decoder.color_transfer_characteristic() {
      ffmpeg::color::TransferCharacteristic::Reserved0 => TransferCharacteristic::Reserved0,
      ffmpeg::color::TransferCharacteristic::BT709 => TransferCharacteristic::BT1886,
      ffmpeg::color::TransferCharacteristic::Unspecified => TransferCharacteristic::Unspecified,
      ffmpeg::color::TransferCharacteristic::Reserved => TransferCharacteristic::Reserved,
      ffmpeg::color::TransferCharacteristic::GAMMA22 => TransferCharacteristic::BT470M,
      ffmpeg::color::TransferCharacteristic::GAMMA28 => TransferCharacteristic::BT470BG,
      ffmpeg::color::TransferCharacteristic::SMPTE170M => TransferCharacteristic::ST170M,
      ffmpeg::color::TransferCharacteristic::SMPTE240M => TransferCharacteristic::ST240M,
      ffmpeg::color::TransferCharacteristic::Linear => TransferCharacteristic::Linear,
      ffmpeg::color::TransferCharacteristic::Log => TransferCharacteristic::Logarithmic100,
      ffmpeg::color::TransferCharacteristic::LogSqrt => TransferCharacteristic::Logarithmic316,
      ffmpeg::color::TransferCharacteristic::IEC61966_2_4 => TransferCharacteristic::XVYCC,
      ffmpeg::color::TransferCharacteristic::BT1361_ECG => TransferCharacteristic::BT1361E,
      ffmpeg::color::TransferCharacteristic::IEC61966_2_1 => TransferCharacteristic::SRGB,
      ffmpeg::color::TransferCharacteristic::BT2020_10 => TransferCharacteristic::BT2020Ten,
      ffmpeg::color::TransferCharacteristic::BT2020_12 => TransferCharacteristic::BT2020Twelve,
      ffmpeg::color::TransferCharacteristic::SMPTE2084 => {
        TransferCharacteristic::PerceptualQuantizer
      }
      ffmpeg::color::TransferCharacteristic::SMPTE428 => TransferCharacteristic::ST428,
      ffmpeg::color::TransferCharacteristic::ARIB_STD_B67 => TransferCharacteristic::HybridLogGamma,
    },
    chroma_location: match decoder.chroma_location() {
      ffmpeg::chroma::Location::Unspecified => ChromaLocation::Unspecified,
      ffmpeg::chroma::Location::Left => ChromaLocation::Left,
      ffmpeg::chroma::Location::Center => ChromaLocation::Center,
      ffmpeg::chroma::Location::TopLeft => ChromaLocation::TopLeft,
      ffmpeg::chroma::Location::Top => ChromaLocation::Top,
      ffmpeg::chroma::Location::BottomLeft => ChromaLocation::BottomLeft,
      ffmpeg::chroma::Location::Bottom => ChromaLocation::Bottom,
    },
  })
}

/// Returns vec of all keyframes
#[tracing::instrument]
pub fn get_keyframes(source: &Path) -> Result<Vec<usize>, ffmpeg::Error> {
  let mut ictx = input(source)?;
  let input = ictx
    .streams()
    .best(MediaType::Video)
    .ok_or(StreamNotFound)?;
  let video_stream_index = input.index();

  let kfs = ictx
    .packets()
    .filter_map(Result::ok)
    .filter(|(stream, _)| stream.index() == video_stream_index)
    .map(|(_, packet)| packet)
    .enumerate()
    .filter(|(_, packet)| packet.is_key())
    .map(|(i, _)| i)
    .collect::<Vec<_>>();

  if kfs.is_empty() {
    return Ok(vec![0]);
  };

  Ok(kfs)
}

/// Returns true if input file have audio in it
pub fn has_audio(file: &Path) -> bool {
  let ictx = input(file).unwrap();
  ictx.streams().best(MediaType::Audio).is_some()
}

/// Encodes the audio using FFmpeg, blocking the current thread.
///
/// This function returns `Some(output)` if the audio exists and the audio
/// successfully encoded, or `None` otherwise.
#[must_use]
pub fn encode_audio<S: AsRef<OsStr>>(
  input: impl AsRef<Path> + std::fmt::Debug,
  temp: impl AsRef<Path> + std::fmt::Debug,
  audio_params: &[S],
) -> Option<PathBuf> {
  let input = input.as_ref();
  let temp = temp.as_ref();

  if has_audio(input) {
    let audio_file = Path::new(temp).join("audio.mkv");
    let mut encode_audio = Command::new("ffmpeg");

    encode_audio.stdout(Stdio::piped());
    encode_audio.stderr(Stdio::piped());

    encode_audio.args(["-y", "-hide_banner", "-loglevel", "error"]);
    encode_audio.args(["-i", input.to_str().unwrap()]);
    encode_audio.args(["-map_metadata", "0"]);
    encode_audio.args(["-map", "0", "-c", "copy", "-vn", "-dn"]);

    encode_audio.args(audio_params);
    encode_audio.arg(&audio_file);

    let output = encode_audio.output().unwrap();

    if !output.status.success() {
      warn!(
        "FFmpeg failed to encode audio!\n{:#?}\nParams: {:?}",
        output, encode_audio
      );
      return None;
    }

    Some(audio_file)
  } else {
    None
  }
}

/// Escapes paths in ffmpeg filters if on windows
pub fn escape_path_in_filter(path: impl AsRef<Path>) -> String {
  if cfg!(windows) {
    PathAbs::new(path.as_ref())
      .unwrap()
      .to_str()
      .unwrap()
      // This is needed because of how FFmpeg handles absolute file paths on Windows.
      // https://stackoverflow.com/questions/60440793/how-can-i-use-windows-absolute-paths-with-the-movie-filter-on-ffmpeg
      .replace('\\', "/")
      .replace(':', r"\\:")
  } else {
    PathAbs::new(path.as_ref())
      .unwrap()
      .to_str()
      .unwrap()
      .to_string()
  }
  .replace('[', r"\[")
  .replace(']', r"\]")
  .replace(',', "\\,")
}
