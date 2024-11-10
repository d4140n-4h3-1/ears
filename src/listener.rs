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

//! Module for manage the listener in the scene.

use crate::internal::OpenAlData;
use crate::openal::{al, ffi};

/**
 * Set the global volume of the scene.
 *
 * A value of 1.0 means unattenuated. Each division by 2 equals an attenuation
 * of about -6dB. Each multiplicaton by 2 equals an amplification of about
 * +6dB.
 *
 * # Argument
 * * `volume` - The global volume for the scene, should be between 0. and 1.
 *
 * # Example
 * ```
 * # use ears::listener;
 * listener::set_volume(0.7f32);
 * ```
 */
pub fn set_volume(volume: f32) -> () {
    check_openal_context!(());
    al::alListenerf(ffi::AL_GAIN, volume);
}

/**
 * Get the global volume of the scene.
 *
 * # Return
 * The global volume of the scene between 0. and 1.
 *
 * # Example
 * ```
 * # use ears::listener;
 * let vol = listener::get_volume();
 * println!("Global volume: {}", vol);
 * ```
 */
pub fn get_volume() -> f32 {
    check_openal_context!(0.);

    let mut volume: f32 = 0.;
    al::alGetListenerf(ffi::AL_GAIN, &mut volume);
    volume
}

/**
 * Set the listener location in three dimensional space.
 *
 * OpenAL, like OpenGL, uses a right handed coordinate system, where in a
 * frontal default view X (thumb) points right, Y points up (index finger), and
 * Z points towards the viewer/camera (middle finger).
 * To switch from a left handed coordinate system, flip the sign on the Z
 * coordinate.
 *
 * Default is [0., 0., 0.].
 *
 * # Argument
 * * `position` - A three dimensional vector of f32 containing the position
 * of the listener [x, y, z].
 *
 * # Example
 * ```
 * # use ears::listener;
 * listener::set_position([45., 90., 35.]);
 */
pub fn set_position(position: [f32; 3]) -> () {
    check_openal_context!(());
    al::alListenerfv(ffi::AL_POSITION, &position[0]);
}

/**
 * Get the location of the listener in three dimensional space.
 *
 * # Return
 * A three dimensional vector of f32 containing the position
 * of the listener [x, y, z].
 *
 * # Example
 * ```
 * # use ears::listener;
 * let pos = listener::get_position();
 * println!("Listener position: {:?}", &pos);
 * ```
 */
pub fn get_position() -> [f32; 3] {
    check_openal_context!([0.; 3]);

    let mut position: [f32; 3] = [0.; 3];
    al::alGetListenerfv(ffi::AL_POSITION, &mut position[0]);
    position
}

/**
 * Set the orientation of the listener.
 *
 * Default orientation is : at[0.0, 0.0, -1.0] - up[0.0, 1.0, 0.0]
 *
 * # Arguments
 * * `orientation_at` - The front as a three dimensional vector [x, y, z].
 * * `orientation_up` - The top as a three dimensional vector [x, y, z].
 *
 * # Example
 * ```
 * # use ears::listener;
 * listener::set_orientation([0.3f32, -0.4f32, 0.9f32], [0.7f32, 0.3f32, 0.8f32]);
 * ```
 */
pub fn set_orientation(orientation_at: [f32; 3], orientation_up: [f32; 3]) {
    check_openal_context!(());
    let orientation: [f32; 6] = [
        orientation_at[0],
        orientation_at[1],
        orientation_at[2],
        orientation_up[0],
        orientation_up[1],
        orientation_up[2],
    ];
    al::alListenerfv(ffi::AL_ORIENTATION, &orientation[0]);
}

/**
 * Get the orientation of the listener.
 *
 * # Return
 * A tuple containing the orientation as two three dimensional vector [x, y, z].
 *
 * # Example
 * ```
 * # use ears::listener;
 * let (at, up) = listener::get_orientation();
 * println!("At orientation: {:?}", &at);
 * println!("Up orientation: {:?}", &up);
 * ```
 */
pub fn get_orientation() -> ([f32; 3], [f32; 3]) {
    check_openal_context!(([0.; 3], [0.; 3]));
    let mut orientation: [f32; 6] = [0.; 6];
    al::alGetListenerfv(ffi::AL_ORIENTATION, &mut orientation[0]);
    (
        [orientation[0], orientation[1], orientation[2]],
        [orientation[3], orientation[4], orientation[5]],
    )
}

/**
 * Set the velocity of the listener.
 *
 * Default velocity is [0.0, 0.0, 0.0].
 *
 * # Argument
 * * `velocity` - A three dimensional vector of f32 containing the velocity
 * of the sound [x, y, z].
 */
pub fn set_velocity(velocity: [f32; 3]) -> () {
    check_openal_context!(());

    al::alListenerfv(ffi::AL_VELOCITY, &velocity[0]);
}

/**
 * Get the velocity of the listener.
 *
 * # Return
 * A three dimensional vector of f32 containing the velocity
 * of the sound [x, y, z].
 */
pub fn get_velocity() -> [f32; 3] {
    check_openal_context!([0.0; 3]);

    let mut velocity: [f32; 3] = [0.0; 3];
    al::alGetListenerfv(ffi::AL_VELOCITY, &mut velocity[0]);
    velocity
}

#[cfg(test)]
mod test {
    use listener::{
        get_orientation, get_position, get_volume, set_orientation, set_position, set_volume,
    };

    #[test]
    #[ignore]
    pub fn listener_set_volume() -> () {
        set_volume(0.77);
        assert_eq!(get_volume(), 0.77);
    }

    // untill https://github.com/rust-lang/rust/issues/7622 is not used, slice comparsion is used

    #[test]
    #[ignore]
    pub fn listener_set_position() -> () {
        set_position([50f32, 150f32, 234f32]);
        let res = get_position();
        assert_eq!((res[0], res[1], res[2]), (50f32, 150f32, 234f32))
    }

    #[test]
    #[ignore]
    pub fn listener_set_orientation() -> () {
        set_orientation([50., 150., 234.], [277., 125., 71.]);
        let (s1, s2) = get_orientation();
        assert_eq!(s1, [50f32, 150f32, 234f32]);
        assert_eq!(s2, [277f32, 125f32, 71f32])
    }
}
