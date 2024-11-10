extern crate ears;
use ears::{AudioController, Sound};
use std::thread;

fn main() {
    let mut handles = Vec::new();
    for _ in 0..4 {
        let handle = thread::spawn(move || {
            let mut snd = Sound::new("res/shots.ogg").unwrap();
            snd.play();
            while snd.is_playing() {}
        });
        handles.push(handle);
    }

    for h in handles {
        match h.join() {
            Ok(_) => println!("Thread exited successfully"),
            Err(_) => println!("Thread died"),
        };
    }
}
