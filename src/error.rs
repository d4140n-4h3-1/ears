use crate::openal::al;
use crate::sndfile::SndFileError;
use std::error::Error;
use std::fmt;

/// All possible errors when opening a Sound or Music.
pub enum SoundError {
    /// Happens when OpenAL failed to load for some reason.
    InvalidOpenALContext,

    /// Error while loading music file.
    LoadError(SndFileError),

    /// Unrecognized music format.
    InvalidFormat,

    /// Internal OpenAL error.
    InternalOpenALError(al::AlError),
}

impl fmt::Display for SoundError {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        write!(
            fmt,
            "{}",
            match self {
                SoundError::InvalidOpenALContext => "invalid OpenAL context".to_string(),
                SoundError::LoadError(err) => format!("error while loading music file: {}", err),
                SoundError::InvalidFormat => "unrecognized music format".to_string(),
                SoundError::InternalOpenALError(err) => format!("internal OpenAL error: {}", err),
            }
        )
    }
}

impl fmt::Debug for SoundError {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        fmt::Display::fmt(self, fmt)
    }
}

impl Error for SoundError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            SoundError::InvalidOpenALContext => None,
            SoundError::LoadError(err) => Some(err),
            SoundError::InvalidFormat => None,
            SoundError::InternalOpenALError(err) => Some(err),
        }
    }
}
