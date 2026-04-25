use bitflags::bitflags;

bitflags! {
    /// Content modalities supported by a model.
    ///
    /// Each bit represents one modality + direction capability so the full set
    /// fits in a single `u8`.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    #[repr(transparent)]
    pub struct Modality: u8 {
        /// Text input capability.
        const TEXT_INPUT = 1 << 0;
        /// Text output capability.
        const TEXT_OUTPUT = 1 << 1;
        /// Image input capability.
        const IMAGE_INPUT = 1 << 2;
        /// Image output capability.
        const IMAGE_OUTPUT = 1 << 3;
        /// Audio input capability.
        const AUDIO_INPUT = 1 << 4;
        /// Audio output capability.
        const AUDIO_OUTPUT = 1 << 5;
        /// Video input capability.
        const VIDEO_INPUT = 1 << 6;
        /// Video output capability.
        const VIDEO_OUTPUT = 1 << 7;

        /// Text input and output capability.
        const TEXT = Self::TEXT_INPUT.bits() | Self::TEXT_OUTPUT.bits();
        /// Image input and output capability.
        const IMAGE = Self::IMAGE_INPUT.bits() | Self::IMAGE_OUTPUT.bits();
        /// Audio input and output capability.
        const AUDIO = Self::AUDIO_INPUT.bits() | Self::AUDIO_OUTPUT.bits();
        /// Video input and output capability.
        const VIDEO = Self::VIDEO_INPUT.bits() | Self::VIDEO_OUTPUT.bits();
    }
}

impl Default for Modality {
    #[inline]
    fn default() -> Self {
        Self::TEXT
    }
}
