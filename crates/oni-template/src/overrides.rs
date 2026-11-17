//! Hand-authored layout corrections for templates where the **released C
//! source drifted from the released data**, so the auto-generated layout (in
//! [`crate::generated`]) does not match the on-disk bytes.
//!
//! These are applied on top of the generated layouts by [`crate::Registry`]
//! (an override replaces the generated layout for the same tag). Each is
//! validated the same way as the generated ones.
//!
//! ## SNDD — `SStSoundData`
//!
//! The released `BFW_SoundSystem2.h` has the `SStFormat f;` member **commented
//! out**, so the generated layout is just `flags/duration/num_bytes/data`. The
//! shipped data, however, carries the full `SStFormat` inline at offset 4
//! (confirmed byte-exact against OniSplit for all 2380 level0 sounds). This
//! override restores that true layout.

use crate::{Field, Kind, Layout};
use oni_fourcc::FourCc;

/// Layouts that take precedence over [`crate::generated::layouts`].
pub fn overrides() -> Vec<Layout> {
    vec![sndd()]
}

fn sndd() -> Layout {
    use Kind::*;
    Layout {
        tag: FourCc(0x534E_4444), // 'SNDD'
        name: "SStSoundData (data-accurate)".into(),
        fields: vec![
            Field::new("flags", U32),
            // SStFormat (a verbatim WAV `fmt ` chunk body), inline at offset 4.
            Field::new("wFormatTag", U16),
            Field::new("nChannels", U16),
            Field::new("nSamplesPerSec", U32),
            Field::new("nAvgBytesPerSec", U32),
            Field::new("nBlockAlign", U16),
            Field::new("wBitsPerSample", U16),
            Field::new("cbSize", U16),
            Field::new("wSamplesPerBlock", U16),
            Field::new("wNumCoef", U16),
            Field::new(
                "aCoef",
                Array {
                    len: 7,
                    elem: vec![Field::new("iCoef1", I16), Field::new("iCoef2", I16)],
                },
            ),
            Field::new("duration", U16),
            Field::new("num_bytes", U32),
            Field::new("data", RawPtr),
        ],
    }
}
