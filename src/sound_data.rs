// The MIT License (MIT)
//
// Copyright (c) 2013 Jeremy Letang (letang.jeremy@gmail.com)
//
// Permission is hereby granted, free of charge, to any person obtaining a copy of
// this software and associated documentation files (the "Software"), to deal in
// the Software without restriction, including without limitation the rights to
// use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of
// the Software, and to permit persons to whom the Software is furnished to do so,
// subject to the following conditions:
//
// The above copyright notice and this permission notice shall be included in all
// copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
// IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS
// FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR
// COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER
// IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN
// CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.

//! The datas extracted from a sound file.

use libc::c_void;
use std::mem;
use std::vec::Vec;

use crate::audio_tags::{get_sound_tags, AudioTags, Tags};
use crate::error::SoundError;
use crate::internal::OpenAlData;
use crate::openal::{al, ffi};
use crate::sndfile::OpenMode::Read;
use crate::sndfile::{SndFile, SndInfo};

/**
 * Samples extracted from a file.
 *
 * SoundDatas are made to be shared between several Sound and played in the same
 * time.
 *
 * # Example
 * ```ignore
 * use ears::{Sound, SoundData, SoundError, AudioController};
 * use std::cell::RefCell;
 * use std::rc::Rc;
 *
 * fn main() -> Result<(), SoundError> {
 *   // Create a SoundData
 *   let snd_data = Rc::new(RefCell::new(SoundData::new("path/to/my/sound.wav")?));
 *
 *   // Create two Sound with the same SoundData
 *   let mut snd1 = Sound::new_with_data(snd_data.clone())?;
 *   let mut snd2 = Sound::new_with_data(snd_data.clone())?;
 *
 *   // Play the sounds
 *   snd1.play();
 *   snd2.play();
 *
 *   // Wait until snd2 is playing
 *   while snd2.is_playing() {}
 *   Ok(())
 * }
 * ```
 */
pub struct SoundData {
    /// The SoundTags who contains all the information of the sound
    sound_tags: Tags,
    /// The sndfile samples information
    snd_info: SndInfo,
    /// The total samples count of the Sound
    nb_sample: i64,
    /// The OpenAl internal identifier for the buffer
    al_buffer: u32,
}

impl SoundData {
    /**
     * Create a new SoundData.
     *
     * The SoundData contains all the information extracted from the
     * file: samples and tags.
     * It's an easy way to share the same samples between man Sounds objects.
     *
     * # Arguments
     * * `path` - The path of the file to load
     *
     * # Return
     * A `Result` containing Ok(SoundData) on success, Err(SoundError)
     * if there has been an error.
     */
    pub fn new(path: &str) -> Result<SoundData, SoundError> {
        check_openal_context!(Err(SoundError::InvalidOpenALContext));

        let mut file = match SndFile::new(path, Read) {
            Ok(file) => file,
            Err(err) => {
                return Err(SoundError::LoadError(err));
            }
        };

        let infos = file.get_sndinfo();

        let nb_sample = infos.channels as i64 * infos.frames;

        let mut samples = vec![0i16; nb_sample as usize];
        file.read_i16(&mut samples[..], nb_sample as i64);

        let mut buffer_id = 0;
        let len = mem::size_of::<i16>() * (samples.len());

        // Retrieve format informations
        let format = match al::get_channels_format(infos.channels) {
            Some(fmt) => fmt,
            None => {
                return Err(SoundError::InvalidFormat);
            }
        };

        al::alGenBuffers(1, &mut buffer_id);
        al::alBufferData(
            buffer_id,
            format,
            samples.as_ptr() as *mut c_void,
            len as i32,
            infos.samplerate,
        );

        if let Some(err) = al::openal_has_error() {
            return Err(SoundError::InternalOpenALError(err));
        };

        let sound_data = SoundData {
            sound_tags: get_sound_tags(&file),
            snd_info: infos,
            nb_sample: nb_sample,
            al_buffer: buffer_id,
        };
        file.close();

        Ok(sound_data)
    }
}

/**
 * Get the sound file infos.
 *
 * # Return
 * The struct SndInfo.
 */
pub fn get_sndinfo<'r>(s_data: &'r SoundData) -> &'r SndInfo {
    &s_data.snd_info
}

/**
 * Get the OpenAL identifier of the samples buffer.
 *
 * # Return
 * The OpenAL internal identifier for the samples buffer of the sound.
 */
#[doc(hidden)]
pub fn get_buffer(s_data: &SoundData) -> u32 {
    s_data.al_buffer
}

impl AudioTags for SoundData {
    /**
     * Get the tags of a Sound.
     *
     * # Return
     * A borrowed pointer to the internal struct SoundTags
     */
    fn get_tags(&self) -> Tags {
        self.sound_tags.clone()
    }
}

impl Drop for SoundData {
    /// Destroy all the resources attached to the SoundData
    fn drop(&mut self) -> () {
        unsafe {
            ffi::alDeleteBuffers(1, &mut self.al_buffer);
        }
    }
}

#[cfg(test)]
mod test {
    #![allow(non_snake_case)]

    #[allow(unused_variables)]
    use sound_data::SoundData;

    #[test]
    #[ignore]
    fn sounddata_create_OK() -> () {
        #![allow(unused_variables)]
        let snd_data = SoundData::new("res/shot.wav").unwrap();
    }

    #[test]
    #[ignore]
    #[should_panic]
    fn sounddata_create_FAIL() -> () {
        #![allow(unused_variables)]
        let snd_data = SoundData::new("toto.wav").unwrap();
    }
}
