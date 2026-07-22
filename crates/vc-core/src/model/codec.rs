use serde::{Deserialize, Serialize};
use std::fmt::{Display, Formatter};

macro_rules! string_enum {
    ($name:ident { $( $variant:ident => $value:literal ),+ $(,)? }) => {
        #[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
        #[serde(rename_all = "lowercase")]
        pub enum $name { $( $variant ),+ }

        impl $name {
            pub const fn as_str(self) -> &'static str {
                match self { $( Self::$variant => $value ),+ }
            }
        }

        impl Display for $name {
            fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result { f.write_str(self.as_str()) }
        }
    };
}

string_enum!(Codec {
    Hevc => "hevc",
    Av1 => "av1",
});

string_enum!(EncoderBackend {
    Auto => "auto",
    Cpu => "cpu",
    Nvenc => "nvenc",
    Qsv => "qsv",
    Amf => "amf",
    VideoToolbox => "videotoolbox",
});

string_enum!(DecodeAcceleration {
    Software => "software",
    VideoToolbox => "videotoolbox",
});

#[allow(clippy::derivable_impls)]
impl Default for DecodeAcceleration {
    fn default() -> Self {
        Self::Software
    }
}

string_enum!(AudioMode {
    Copy => "copy",
    Aac => "aac",
});

#[allow(clippy::derivable_impls)]
impl Default for AudioMode {
    fn default() -> Self {
        Self::Copy
    }
}

string_enum!(ContainerFormat {
    Mkv => "mkv",
    Mp4 => "mp4",
});

#[allow(clippy::derivable_impls)]
impl Default for ContainerFormat {
    fn default() -> Self {
        Self::Mp4
    }
}

string_enum!(PreviewSampleMode {
    Middle => "middle",
    Custom => "custom",
});
