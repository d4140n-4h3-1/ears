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

/*!
 * __ears__ initialization (optional).
 *
 * This module provide a unique function to initialize __ears__.
 * Use this function in the case of you don't use __ears__ for the first time
 * in you program in the main task. This prevent that the context was created
 * and destroyed in a another task.
 */

use crate::internal::{OpenAlContextError, OpenAlData};
use crate::record_context::RecordContext;

/**
 * Initialize the internal context
 *
 * # Return
 * `Ok(())` if initialization is successful, `Err(OpenAlContextError)` otherwise
 *
 * # Example
 * ```no_run
 * fn main() -> Result<(), ears::OpenAlContextError> {
 *     ears::init()?;
 *     Ok(())
 * }
 * ```
 */
pub fn init() -> Result<(), OpenAlContextError> {
    return OpenAlData::check_al_context();
}

/**
 * Initialize the input device context
 *
 * # Return
 * `Ok(RecordContext)` if initialization is successful, `Err(OpenAlContextError)` otherwise
 *
 * # Example
 * ```no_run
 * fn main() -> Result<(), ears::OpenAlContextError> {
 *     ears::init_in()?;
 *     Ok(())
 * }
 * ```
 */
pub fn init_in() -> Result<RecordContext, OpenAlContextError> {
    return OpenAlData::check_al_input_context();
}

#[cfg(test)]
mod test {
    #![allow(non_snake_case)]

    use init;
    use init_in;
    use std::thread;

    #[test]
    #[ignore]
    fn test_init_ears_OK() -> () {
        assert!(init().is_ok())
    }

    #[test]
    #[ignore]
    fn test_init_in_with_normal_init_OK() -> () {
        init();
        assert!(init_in().is_ok())
    }

    #[test]
    #[ignore]
    fn test_init_in_alone_OK() -> () {
        assert!(init_in().is_ok())
    }

    #[test]
    #[ignore]
    fn test_init_in_in_another_task_OK() -> () {
        init();
        thread::spawn(move || assert!(init_in().is_err()));
    }
}
