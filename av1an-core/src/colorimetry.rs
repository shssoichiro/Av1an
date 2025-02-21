use av_data::pixel::{
  ChromaLocation, ColorPrimaries, MatrixCoefficients, TransferCharacteristic, YUVRange,
};

#[derive(Debug, Clone, Copy)]
pub struct Colorimetry {
  pub range: YUVRange,
  pub primaries: ColorPrimaries,
  pub matrix: MatrixCoefficients,
  pub transfer: TransferCharacteristic,
  pub chroma_location: ChromaLocation,
}

impl Colorimetry {
  pub fn is_hdr(&self) -> bool {
    self.transfer == TransferCharacteristic::HybridLogGamma
      || self.transfer == TransferCharacteristic::PerceptualQuantizer
  }
}
