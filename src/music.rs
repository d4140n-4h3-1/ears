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

//! Play Music easily.

use libc::c_void;
use std::convert::TryInto;
use std::mem;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;
use std::thread;
use std::thread::sleep;
use std::time::Duration;
use std::vec::Vec;

use crate::audio_controller::AudioController;
use crate::audio_tags::{get_sound_tags, AudioTags, Tags};
use crate::error::SoundError;
use crate::internal::OpenAlData;
use crate::openal::{al, ffi};
use crate::reverb_effect::ReverbEffect;
use crate::sndfile::OpenMode::Read;
use crate::sndfile::SeekMode::SeekSet;
use crate::sndfile::{SndFile, SndInfo};
use crate::states::State;
use crate::states::State::{Initial, Paused, Playing, Stopped};

const BUFFER_COUNT: i32 = 2;

/**
 * Play Music easily.
 *
 * Simple class to play music easily in 2 lines.
 *
 * Music is played in their own task and the samples are loaded progressively
 * using circular buffers.
 *
 * Music maintains it's own cursor into the underlying file and will use that
 * cursor to continuously refill each buffer as it's processed by the source.
 *
 * They aren't associated to a SoundData like Sounds.
 *
 * # Examples
 * ```no_run
 * extern crate ears;
 * use ears::{Music, SoundError, AudioController};
 *
 * fn main() -> Result<(), SoundError> {
 *   let mut msc = Music::new("path/to/music.flac")?;
 *   msc.play();
 *   Ok(())
 * }
 * ```
 */
pub struct Music {
    /// The internal OpenAL source identifier
    al_source: u32,
    /// The internal OpenAL buffers
    al_buffers: [u32; 2],
    /// The file open with libmscfile
    file: Option<Box<SndFile>>,
    /// Information of the file
    file_infos: SndInfo,
    /// Quantity of sample to read each time
    sample_to_read: i64, // TODO: usize?
    /// Format of the sample
    sample_format: i32,
    /// Audio tags
    sound_tags: Tags,
    /// Current cursor into the music file
    cursor: Arc<AtomicI64>,
    /// State
    state: State,
    /// Whether this music is looping or not
    is_looping: bool,
    /// Channel to tell the thread, if is_looping changed
    looping_sender: Option<Sender<bool>>,

    /// Channel to tell the thread to set offset
    offset_sender: Option<Sender<i32>>,

    /// Thread which streams the music file
    thread_handle: Option<thread::JoinHandle<()>>,
}

// Recursively fill a buffer with data, returning the frame offset into
// the file when done. This can potentially read the file many times over
// if the source is set to loop.
//
// This is intentional as it is the only way to ensure the buffers will
// have enough data in them for uninterrupted playback, no matter how long
// or short the underlying file is.
//
// Note: The only difference between the "items" and "frames" versions of
// each read function is the units in which the object count is specified -
// calling sf_readf_short with a count argument of N, on a SNDFILE with C
// channels, is the same as calling sf_read_short with a count argument of
// N*C. The buffer pointed to by "ptr" should be the same number of bytes
// in each case.
//
// ref: http://www.mega-nerd.com/libsndfile/api.html#read
fn fill_buffer(
    samples: &mut Vec<i16>,
    sndfile: &mut SndFile,
    cursor: Arc<AtomicI64>,
    is_looping: bool,
) {
    // First, find where the buffer is currently filled to
    let buffer_position = samples.len();
    let cursor_position = cursor.load(Ordering::Relaxed);

    // Move the sound file to where we want to read from
    sndfile.seek(cursor_position, SeekSet);

    // Read data from sound file into the buffer, from the current buffer position onwards
    let read_amount = (samples.capacity() - samples.len()) as i64;
    let read_length = sndfile.read_i16(&mut samples[buffer_position..], read_amount) as usize;

    // Update the vector length manually
    unsafe {
        samples.set_len(buffer_position + read_length);
    }

    let channels = sndfile.get_sndinfo().channels as i64;
    let frames = sndfile.get_sndinfo().frames;

    // Calculate where the next cursor is at, based on how many 'items' were read
    // divided by the channels in the source sound file.
    let mut new_cursor_position = cursor_position + read_length as i64 / channels;

    // Modulo on new cursor position to wrap around if we're looping
    if is_looping {
        new_cursor_position = new_cursor_position % frames;
    }

    cursor.store(new_cursor_position, Ordering::Relaxed);

    // If we haven't reached capacity yet, keep recursing
    if samples.len() != samples.capacity() && read_length > 0 {
        fill_buffer(samples, sndfile, cursor, is_looping)
    }
}

// Becaused the Music source is playing buffered audio, we need to be
// able to calculate the offset into the full file ourselves
fn calculate_true_offset(
    info: &SndInfo,
    cursor: i64,
    buffer_size: i64,
    buffers_queued: i32,
    source_offset: i32,
) -> i32 {
    let queued_buffers_size = buffer_size / BUFFER_COUNT as i64 * buffers_queued as i64;
    let offset = cursor - queued_buffers_size + source_offset as i64;

    // This is a bit of a pro hack to deal with when the buffers wrap around
    // when looping... seems to be accurate though
    let offset = if offset < 0 {
        info.frames + offset
    } else {
        offset
    };

    offset.try_into().unwrap_or(0)
}

// Sets the new cursor from offset in seconds with reasonable accuracy
fn set_cursor_from_offset(info: &SndInfo, cursor: Arc<AtomicI64>, offset: f32) {
    let frames = info.frames as f32;
    let sample_rate = info.samplerate as f32;
    let duration_in_seconds = frames / sample_rate;

    cursor.store(
        (frames * offset / duration_in_seconds) as i64,
        Ordering::Relaxed,
    );
}

impl Music {
    /**
     * Create a new Music
     *
     * # Argument
     * * `path` - The path of the file to load the music
     *
     * # Return
     * A `Result` containing Ok(Music) on success, Err(SoundError)
     * if there has been an error.
     */
    pub fn new(path: &str) -> Result<Music, SoundError> {
        // Check that OpenAL is launched
        check_openal_context!(Err(SoundError::InvalidOpenALContext));

        // Retrieve File and Music datas
        let file = match SndFile::new(path, Read) {
            Ok(file) => Box::new(file),
            Err(err) => {
                return Err(SoundError::LoadError(err));
            }
        };
        let infos = file.get_sndinfo();

        // create the source and the buffers
        let mut source_id = 0;
        let mut buffer_ids = [0; BUFFER_COUNT as usize];
        // create the source
        al::alGenSources(1, &mut source_id);
        // create the buffers
        al::alGenBuffers(BUFFER_COUNT, &mut buffer_ids[0]);

        // Retrieve format information
        let format = match al::get_channels_format(infos.channels) {
            Some(fmt) => fmt,
            None => {
                return Err(SoundError::InvalidFormat);
            }
        };

        // Check if there is OpenAL internal error
        if let Some(err) = al::openal_has_error() {
            return Err(SoundError::InternalOpenALError(err));
        };

        let sound_tags = get_sound_tags(&*file);

        Ok(Music {
            al_source: source_id,
            al_buffers: buffer_ids,
            file: Some(file),
            sample_to_read: 50000 * (infos.channels as i64),
            file_infos: infos,
            sample_format: format,
            sound_tags: sound_tags,
            cursor: Arc::new(AtomicI64::new(0)),
            state: Initial,
            is_looping: false,
            looping_sender: None,
            offset_sender: None,
            thread_handle: None,
        })
    }

    fn process_music(&mut self) -> () {
        let (chan, port) = channel();
        let sample_t_r = self.sample_to_read;
        let sample_rate = self.file_infos.samplerate;
        let sample_format = self.sample_format;
        let al_source = self.al_source;
        let al_buffers = self.al_buffers;

        // create sample buffer and reserve the exact capacity we need
        let mut samples: Vec<i16> = Vec::with_capacity(sample_t_r as usize);

        fill_buffer(
            &mut samples,
            &mut self.file.as_mut().unwrap(),
            self.cursor.clone(),
            self.is_looping,
        );

        al::alBufferData(
            al_buffers[0],
            sample_format,
            samples.as_ptr() as *mut c_void,
            (mem::size_of::<i16>() * samples.len()) as i32,
            sample_rate,
        );

        samples.clear();

        fill_buffer(
            &mut samples,
            &mut self.file.as_mut().unwrap(),
            self.cursor.clone(),
            self.is_looping,
        );

        al::alBufferData(
            al_buffers[1],
            sample_format,
            samples.as_ptr() as *mut c_void,
            (mem::size_of::<i16>() * samples.len()) as i32,
            sample_rate,
        );

        // Queue the buffers
        al::alSourceQueueBuffers(al_source, 2, &al_buffers[0]);

        // Start playing
        al::alSourcePlay(al_source);

        let (looping_sender, looping_receiver): (Sender<bool>, Receiver<bool>) = channel();
        let (offset_sender, offset_receiver): (Sender<i32>, Receiver<i32>) = channel();

        self.looping_sender = Some(looping_sender);
        self.offset_sender = Some(offset_sender);

        let cursor = self.cursor.clone();
        let is_looping_clone = self.is_looping.clone();

        let thread = thread::Builder::new().name(String::from("ears-music"));
        self.thread_handle = Some(
            thread
                .spawn(move || {
                    match OpenAlData::check_al_context() {
                        Ok(_) => {}
                        Err(err) => {
                            println!("{}", err);
                        }
                    };
                    let mut file: SndFile = port.recv().ok().unwrap();
                    let mut status = ffi::AL_PLAYING;
                    let mut buffers_processed = 0;
                    let mut buffers_queued = 0;
                    let mut buf = 0;
                    let mut is_looping = is_looping_clone;
                    let mut offset_shift_restart = false;

                    while status != ffi::AL_STOPPED {
                        // wait a bit
                        sleep(Duration::from_millis(50));
                        if status == ffi::AL_PLAYING {
                            if let Ok(new_is_looping) = looping_receiver.try_recv() {
                                is_looping = new_is_looping;
                            }

                            if let Ok(offset) = offset_receiver.try_recv() {
                                // If we shift the offset, we need to stop and restart the source
                                // so that we can swap out the buffers in an instantaneous manner
                                al::alSourceStop(al_source);
                                offset_shift_restart = true;
                                cursor.store(offset.into(), Ordering::Relaxed);
                            }

                            al::alGetSourcei(
                                al_source,
                                ffi::AL_BUFFERS_QUEUED,
                                &mut buffers_queued,
                            );

                            al::alGetSourcei(
                                al_source,
                                ffi::AL_BUFFERS_PROCESSED,
                                &mut buffers_processed,
                            );

                            for _ in 0..buffers_processed {
                                al::alSourceUnqueueBuffers(al_source, 1, &mut buf);

                                samples.clear();

                                fill_buffer(&mut samples, &mut file, cursor.clone(), is_looping);

                                al::alBufferData(
                                    buf,
                                    sample_format,
                                    samples.as_ptr() as *mut c_void,
                                    (mem::size_of::<i16>() * samples.len()) as i32,
                                    sample_rate,
                                );
                                al::alSourceQueueBuffers(al_source, 1, &buf);
                            }

                            // After buffer refill restart
                            if offset_shift_restart {
                                al::alSourcePlay(al_source);
                                offset_shift_restart = false;
                            }
                        }
                        // Get source status
                        status = al::alGetState(al_source);
                    }
                    al::alSourcei(al_source, ffi::AL_BUFFER, 0);
                })
                .unwrap(),
        );
        let file = self.file.as_ref().unwrap().clone();
        chan.send(*file);
    }
}

impl AudioTags for Music {
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

impl AudioController for Music {
    /**
     * Play or resume the Music.
     */
    fn play(&mut self) -> () {
        check_openal_context!(());

        match self.get_state() {
            Paused => {
                al::alSourcePlay(self.al_source);
                return;
            }
            _ => {
                if self.is_playing() {
                    al::alSourceStop(self.al_source);
                    // wait a bit for openal terminate
                    sleep(Duration::from_millis(50));
                }
                self.file.as_mut().unwrap().seek(0, SeekSet);
                self.process_music();
            }
        }
    }

    /**
     * Pause the Music.
     */
    fn pause(&mut self) -> () {
        check_openal_context!(());

        al::alSourcePause(self.al_source)
    }

    /**
     * Stop the Music.
     */
    fn stop(&mut self) -> () {
        check_openal_context!(());

        al::alSourceStop(self.al_source);
    }

    /**
     * Connect a ReverbEffect to the Music
     */
    fn connect(&mut self, reverb_effect: &Option<ReverbEffect>) {
        check_openal_context!(());

        match reverb_effect {
            Some(reverb_effect) => {
                al::alSource3i(
                    self.al_source,
                    ffi::AL_AUXILIARY_SEND_FILTER,
                    reverb_effect.slot() as i32,
                    0,
                    ffi::AL_FILTER_NULL,
                );
            }
            None => {
                al::alSource3i(
                    self.al_source,
                    ffi::AL_AUXILIARY_SEND_FILTER,
                    ffi::AL_EFFECTSLOT_NULL,
                    0,
                    ffi::AL_FILTER_NULL,
                );
            }
        }
    }

    /**
     * Check if the Music is playing or not.
     *
     * # Return
     * True if the Music is playing, false otherwise.
     */
    fn is_playing(&self) -> bool {
        match self.get_state() {
            Playing => true,
            _ => false,
        }
    }

    /**
     * Get the current state of the Music
     *
     * # Return
     * The state of the music as a variant of the enum State
     */
    fn get_state(&self) -> State {
        check_openal_context!(Initial);

        let state = al::alGetState(self.al_source);

        match state {
            ffi::AL_INITIAL => Initial,
            ffi::AL_PLAYING => Playing,
            ffi::AL_PAUSED => Paused,
            ffi::AL_STOPPED => Stopped,
            _ => unreachable!(),
        }
    }

    /**
     * Set the playback position in the Music.
     *
     * # Argument
     * * `offset` - The frame to seek to
     */
    fn set_offset(&mut self, offset: i32) -> () {
        match self.offset_sender {
            Some(ref sender) => {
                sender.send(offset);
            }
            None => self.cursor.store(offset.into(), Ordering::Relaxed),
        }
    }

    /**
     * Get the current position in the Music.
     *
     * # Return
     * The current frame being played
     */
    fn get_offset(&self) -> i32 {
        check_openal_context!(0);

        let mut sample_offset: i32 = 0;
        al::alGetSourcei(self.al_source, ffi::AL_SAMPLE_OFFSET, &mut sample_offset);

        let mut buffers_queued: i32 = 0;
        al::alGetSourcei(self.al_source, ffi::AL_BUFFERS_QUEUED, &mut buffers_queued);

        let cursor = self.cursor.load(Ordering::Relaxed);
        let buffer_size = self.sample_to_read;

        calculate_true_offset(
            &self.file_infos,
            cursor,
            buffer_size,
            buffers_queued,
            sample_offset,
        )
    }

    /**
     * Set the volume of the Music.
     *
     * A value of 1.0 means unattenuated. Each division by 2 equals an attenuation
     * of about -6dB. Each multiplicaton by 2 equals an amplification of about
     * +6dB.
     *
     * # Argument
     * * `volume` - The volume of the Music, should be between 0.0 and 1.0
     */
    fn set_volume(&mut self, volume: f32) -> () {
        check_openal_context!(());

        al::alSourcef(self.al_source, ffi::AL_GAIN, volume);
    }

    /**
     * Get the volume of the Music.
     *
     * # Return
     * The volume of the Music between 0.0 and 1.0
     */
    fn get_volume(&self) -> f32 {
        check_openal_context!(0.);

        let mut volume: f32 = 0.;
        al::alGetSourcef(self.al_source, ffi::AL_GAIN, &mut volume);
        volume
    }

    /**
     * Set the minimal volume for a Music.
     *
     * The minimum volume allowed for a music, after distance and cone
     * attenation is applied (if applicable).
     *
     * # Argument
     * * `min_volume` - The new minimal volume of the Music should be
     * between 0.0 and 1.0
     */
    fn set_min_volume(&mut self, min_volume: f32) -> () {
        check_openal_context!(());

        al::alSourcef(self.al_source, ffi::AL_MIN_GAIN, min_volume);
    }

    /**
     * Get the minimal volume of the Music.
     *
     * # Return
     * The minimal volume of the Music between 0.0 and 1.0
     */
    fn get_min_volume(&self) -> f32 {
        check_openal_context!(0.);

        let mut volume: f32 = 0.;
        al::alGetSourcef(self.al_source, ffi::AL_MIN_GAIN, &mut volume);
        volume
    }

    /**
     * Set the maximal volume for a Music.
     *
     * The maximum volume allowed for a Music, after distance and cone
     * attenation is applied (if applicable).
     *
     * # Argument
     * * `max_volume` - The new maximal volume of the Music should be
     * between 0.0 and 1.0
     */
    fn set_max_volume(&mut self, max_volume: f32) -> () {
        check_openal_context!(());

        al::alSourcef(self.al_source, ffi::AL_MAX_GAIN, max_volume);
    }

    /**
     * Get the maximal volume of the Music.
     *
     * # Return
     * The maximal volume of the Music between 0.0 and 1.0
     */
    fn get_max_volume(&self) -> f32 {
        check_openal_context!(0.);

        let mut volume: f32 = 0.;
        al::alGetSourcef(self.al_source, ffi::AL_MAX_GAIN, &mut volume);
        volume
    }

    /**
     * Set the Music looping or not
     *
     * The default looping is false.
     *
     * # Arguments
     * `looping` - The new looping state.
     */
    fn set_looping(&mut self, looping: bool) -> () {
        if let Some(ref sender) = self.looping_sender {
            sender.send(looping);
        }
        self.is_looping = looping;
    }

    /**
     * Check if the Music is looping or not
     *
     * # Return
     * True if the Music is looping, false otherwise.
     */
    fn is_looping(&self) -> bool {
        self.is_looping
    }

    /**
     * Set the pitch of the Music.
     *
     * A multiplier for the frequency (sample rate) of the Music's buffer.
     *
     * Default pitch is 1.0.
     *
     * # Argument
     * * `new_pitch` - The new pitch of the Music in the range [0.5 - 2.0]
     */
    fn set_pitch(&mut self, pitch: f32) -> () {
        check_openal_context!(());

        al::alSourcef(self.al_source, ffi::AL_PITCH, pitch)
    }

    /**
     * Set the pitch of the Music.
     *
     * # Return
     * The pitch of the Music in the range [0.5 - 2.0]
     */
    fn get_pitch(&self) -> f32 {
        check_openal_context!(0.);

        let mut pitch = 0.;
        al::alGetSourcef(self.al_source, ffi::AL_PITCH, &mut pitch);
        pitch
    }

    /**
     * Set the position of the Music relative to the listener or absolute.
     *
     * Default position is absolute.
     *
     * # Argument
     * `relative` - True to set Music relative to the listener false to set the
     * Music position absolute.
     */
    fn set_relative(&mut self, relative: bool) -> () {
        check_openal_context!(());

        match relative {
            true => al::alSourcei(
                self.al_source,
                ffi::AL_SOURCE_RELATIVE,
                ffi::ALC_TRUE as i32,
            ),
            false => al::alSourcei(
                self.al_source,
                ffi::AL_SOURCE_RELATIVE,
                ffi::ALC_FALSE as i32,
            ),
        };
    }

    /**
     * Is the Music relative to the listener or not?
     *
     * # Return
     * True if the Music is relative to the listener false otherwise
     */
    fn is_relative(&mut self) -> bool {
        check_openal_context!(false);

        let mut boolean = 0;
        al::alGetSourcei(self.al_source, ffi::AL_SOURCE_RELATIVE, &mut boolean);
        match boolean as _ {
            ffi::ALC_TRUE => true,
            ffi::ALC_FALSE => false,
            _ => unreachable!(),
        }
    }

    /**
     * Set the Music location in three dimensional space.
     *
     * OpenAL, like OpenGL, uses a right handed coordinate system, where in a
     * frontal default view X (thumb) points right, Y points up (index finger),
     * and Z points towards the viewer/camera (middle finger).
     * To switch from a left handed coordinate system, flip the sign on the Z
     * coordinate.
     *
     * Default position is [0.0, 0.0, 0.0].
     *
     * # Argument
     * * `position` - A three dimensional vector of f32 containing the position
     * of the listener [x, y, z].
     */
    fn set_position(&mut self, position: [f32; 3]) -> () {
        check_openal_context!(());

        al::alSourcefv(self.al_source, ffi::AL_POSITION, &position[0]);
    }

    /**
     * Get the position of the Music in three dimensional space.
     *
     * # Return
     * A three dimensional vector of f32 containing the position of the
     * listener [x, y, z].
     */
    fn get_position(&self) -> [f32; 3] {
        check_openal_context!([0.; 3]);

        let mut position: [f32; 3] = [0.; 3];
        al::alGetSourcefv(self.al_source, ffi::AL_POSITION, &mut position[0]);
        position
    }

    /**
     * Set the direction of the Music.
     *
     * Specifies the current direction in local space.
     *
     * The default direction is: [0.0, 0.0, 0.0]
     *
     * # Argument
     * `direction` - The new direction of the Music.
     */
    fn set_direction(&mut self, direction: [f32; 3]) -> () {
        check_openal_context!(());

        al::alSourcefv(self.al_source, ffi::AL_DIRECTION, &direction[0]);
    }

    /**
     * Get the direction of the Music.
     *
     * # Return
     * The current direction of the Music.
     */
    fn get_direction(&self) -> [f32; 3] {
        check_openal_context!([0.; 3]);

        let mut direction: [f32; 3] = [0.; 3];
        al::alGetSourcefv(self.al_source, ffi::AL_DIRECTION, &mut direction[0]);
        direction
    }

    /**
     * Set the maximum distance of the Music.
     *
     * The distance above which the source is not attenuated any further with a
     * clamped distance model, or where attenuation reaches 0.0 gain for linear
     * distance models with a default rolloff factor.
     *
     * The default maximum distance is +inf.
     *
     * # Argument
     * `max_distance` - The new maximum distance in the range [0.0, +inf]
     */
    fn set_max_distance(&mut self, max_distance: f32) -> () {
        check_openal_context!(());

        al::alSourcef(self.al_source, ffi::AL_MAX_DISTANCE, max_distance);
    }

    /**
     * Get the maximum distance of the Music.
     *
     * # Return
     * The maximum distance of the Music in the range [0.0, +inf]
     */
    fn get_max_distance(&self) -> f32 {
        check_openal_context!(0.);

        let mut max_distance = 0.;
        al::alGetSourcef(self.al_source, ffi::AL_MAX_DISTANCE, &mut max_distance);
        max_distance
    }

    /**
     * Set the reference distance of the Music.
     *
     * The distance in units that no attenuation occurs.
     * At 0.0, no distance attenuation ever occurs on non-linear
     * attenuation models.
     *
     * The default distance reference is 1.
     *
     * # Argument
     * * `ref_distance` - The new reference distance of the Music.
     */
    fn set_reference_distance(&mut self, ref_distance: f32) -> () {
        check_openal_context!(());

        al::alSourcef(self.al_source, ffi::AL_REFERENCE_DISTANCE, ref_distance);
    }

    /**
     * Get the reference distance of the Music.
     *
     * # Return
     * The current reference distance of the Music.
     */
    fn get_reference_distance(&self) -> f32 {
        check_openal_context!(1.);

        let mut ref_distance = 0.;
        al::alGetSourcef(
            self.al_source,
            ffi::AL_REFERENCE_DISTANCE,
            &mut ref_distance,
        );
        ref_distance
    }

    /**
     * Set the attenuation of a Music.
     *
     * Multiplier to exaggerate or diminish distance attenuation.
     * At 0.0, no distance attenuation ever occurs.
     *
     * The default attenuation is 1.
     *
     * # Arguments
     * `attenuation` - The new attenuation for the Music in the range [0.0, 1.0].
     */
    fn set_attenuation(&mut self, attenuation: f32) -> () {
        check_openal_context!(());

        al::alSourcef(self.al_source, ffi::AL_ROLLOFF_FACTOR, attenuation);
    }

    /**
     * Get the attenuation of a Music.
     *
     * # Return
     * The current attenuation for the Music in the range [0.0, 1.0].
     */
    fn get_attenuation(&self) -> f32 {
        check_openal_context!(1.);

        let mut attenuation = 0.;
        al::alGetSourcef(self.al_source, ffi::AL_ROLLOFF_FACTOR, &mut attenuation);
        attenuation
    }

    /**
     * Enable or disable direct channel mode for a Music.
     *
     * Sometimes audio tracks are authored with their own spatialization
     * effects, where the AL's virtualization methods can cause a notable
     * decrease in audio quality.
     *
     * The AL_SOFT_direct_channels extension provides a mechanism for
     * applications to specify whether audio should be filtered according
     * to the AL's channel virtualization rules for multi-channel buffers.
     *
     * When set to true, the audio channels are not virtualized and play
     * directly on the matching output channels if they exist, otherwise
     * they are dropped. Applies only when the extension exists and when
     * playing non-mono buffers.
     *
     * [http://kcat.strangesoft.net/openal-extensions/SOFT_direct_channels.txt]()
     *
     * The default is false.
     *
     * # Argument
     * * `enabled` - true to enable direct channel mode, false to disable
     */
    fn set_direct_channel(&mut self, enabled: bool) -> () {
        if OpenAlData::direct_channel_capable() {
            let value = match enabled {
                true => ffi::AL_TRUE,
                false => ffi::AL_FALSE,
            };

            al::alSourcei(self.al_source, ffi::AL_DIRECT_CHANNELS_SOFT, value as i32);
        }
    }

    /**
     * Returns whether direct channel is enabled or not for a Music.
     *
     * Will always return false if the AL_SOFT_direct_channels extension
     * is not present.
     *
     * # Return
     * `true` if the Music is using direct channel mode
     * `false` otherwise
     */
    fn get_direct_channel(&self) -> bool {
        match OpenAlData::direct_channel_capable() {
            true => {
                let mut boolean = 0;
                al::alGetSourcei(self.al_source, ffi::AL_DIRECT_CHANNELS_SOFT, &mut boolean);

                match boolean as _ {
                    ffi::ALC_TRUE => true,
                    ffi::ALC_FALSE => false,
                    _ => unreachable!(),
                }
            }
            false => false,
        }
    }

    /**
     * Returns the duration of the Music.
     */
    fn get_duration(&self) -> Duration {
        let frames = self.file_infos.frames as u64;
        let sample_rate = self.file_infos.samplerate as u64;

        let seconds = frames / sample_rate;
        let nanoseconds = frames % sample_rate * 1_000_000_000 / sample_rate;

        Duration::new(seconds, nanoseconds as u32)
    }
}

impl Drop for Music {
    /// Destroy all the resources of the Music.
    fn drop(&mut self) -> () {
        self.stop();
        if let Some(handle) = self.thread_handle.take() {
            handle.join();
        }
        unsafe {
            al::alSourcei(self.al_source, ffi::AL_BUFFER, 0);
            ffi::alDeleteBuffers(2, &mut self.al_buffers[0]);
            ffi::alDeleteSources(1, &mut self.al_source);
        }
    }
}

#[cfg(test)]
mod test {
    #![allow(non_snake_case)]

    use audio_controller::AudioController;
    use music::Music;
    use states::State::{Paused, Playing, Stopped};

    #[test]
    #[ignore]
    fn music_create_OK() -> () {
        let msc = Music::new("res/shot.wav");

        assert!(msc.is_ok());
    }

    #[test]
    fn music_create_FAIL() -> () {
        let msc = Music::new("toto.wav");

        assert!(msc.is_err());
    }

    #[test]
    #[ignore]
    fn music_play_OK() -> () {
        let mut msc = Music::new("res/shot.wav").expect("Cannot create Music");

        msc.play();
        assert_eq!(msc.get_state() as i32, Playing as i32);
        msc.stop();
    }

    #[test]
    #[ignore]
    fn music_pause_OK() -> () {
        let mut msc = Music::new("res/shot.wav").expect("Cannot create Music");

        msc.play();
        msc.pause();
        assert_eq!(msc.get_state() as i32, Paused as i32);
        msc.stop();
    }

    #[test]
    #[ignore]
    fn music_stop_OK() -> () {
        let mut msc = Music::new("res/shot.wav").expect("Cannot create Music");

        msc.play();
        msc.stop();
        assert_eq!(msc.get_state() as i32, Stopped as i32);
        msc.stop();
    }

    #[test]
    #[ignore]
    fn music_is_playing_TRUE() -> () {
        let mut msc = Music::new("res/shot.wav").expect("Cannot create Music");

        msc.play();
        assert_eq!(msc.is_playing(), true);
        msc.stop();
    }

    #[test]
    #[ignore]
    fn music_is_playing_FALSE() -> () {
        let mut msc = Music::new("res/shot.wav").expect("Cannot create Music");

        assert_eq!(msc.is_playing(), false);
        msc.stop();
    }

    #[test]
    #[ignore]
    fn music_set_volume_OK() -> () {
        let mut msc = Music::new("res/shot.wav").expect("Cannot create Music");

        msc.set_volume(0.7);
        assert_eq!(msc.get_volume(), 0.7);
    }

    #[test]
    #[ignore]
    fn music_set_min_volume_OK() -> () {
        let mut msc = Music::new("res/shot.wav").expect("Cannot create Music");

        msc.set_min_volume(0.1);
        assert_eq!(msc.get_min_volume(), 0.1);
    }

    #[test]
    #[ignore]
    fn music_set_max_volume_OK() -> () {
        let mut msc = Music::new("res/shot.wav").expect("Cannot create Music");

        msc.set_max_volume(0.9);
        assert_eq!(msc.get_max_volume(), 0.9);
    }

    #[test]
    #[ignore]
    fn music_is_looping_TRUE() -> () {
        let mut msc = Music::new("res/shot.wav").expect("Cannot create Music");

        msc.set_looping(true);
        assert_eq!(msc.is_looping(), true);
    }

    #[test]
    #[ignore]
    fn music_is_looping_FALSE() -> () {
        let mut msc = Music::new("res/shot.wav").expect("Cannot create Music");

        msc.set_looping(false);
        assert_eq!(msc.is_looping(), false);
    }

    #[test]
    #[ignore]
    fn music_set_pitch_OK() -> () {
        let mut msc = Music::new("res/shot.wav").expect("Cannot create Music");

        msc.set_pitch(1.5);
        assert_eq!(msc.get_pitch(), 1.5);
    }

    #[test]
    #[ignore]
    fn music_set_relative_TRUE() -> () {
        let mut msc = Music::new("res/shot.wav").expect("Cannot create Music");

        msc.set_relative(true);
        assert_eq!(msc.is_relative(), true);
    }

    #[test]
    #[ignore]
    fn music_set_relative_FALSE() -> () {
        let mut msc = Music::new("res/shot.wav").expect("Cannot create Music");

        msc.set_relative(false);
        assert_eq!(msc.is_relative(), false);
    }

    // untill https://github.com/rust-lang/rust/issues/7622 is not fixed, slice comparsion is used

    #[test]
    #[ignore]
    fn music_set_position_OK() -> () {
        let mut msc = Music::new("res/shot.wav").expect("Cannot create Music");

        msc.set_position([50., 150., 250.]);
        let res = msc.get_position();
        assert_eq!([res[0], res[1], res[2]], [50f32, 150f32, 250f32]);
    }

    #[test]
    #[ignore]
    fn music_set_direction_OK() -> () {
        let mut msc = Music::new("res/shot.wav").expect("Cannot create Music");

        msc.set_direction([50., 150., 250.]);
        let res = msc.get_direction();
        assert_eq!([res[0], res[1], res[2]], [50f32, 150f32, 250f32]);
    }

    #[test]
    #[ignore]
    fn music_set_max_distance() -> () {
        let mut msc = Music::new("res/shot.wav").expect("Cannot create Music");

        msc.set_max_distance(70.);
        assert_eq!(msc.get_max_distance(), 70.);
    }

    #[test]
    #[ignore]
    fn music_set_reference_distance() -> () {
        let mut msc = Music::new("res/shot.wav").expect("Cannot create Music");

        msc.set_reference_distance(70.);
        assert_eq!(msc.get_reference_distance(), 70.);
    }

    #[test]
    #[ignore]
    fn music_set_attenuation() -> () {
        let mut msc = Music::new("res/shot.wav").expect("Cannot create Music");

        msc.set_attenuation(0.5f32);
        println!("{}", &msc.get_attenuation());
        assert_eq!(&msc.get_attenuation(), &0.5f32);
    }
}
