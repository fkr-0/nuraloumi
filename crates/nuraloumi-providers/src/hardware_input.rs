//! Linux evdev event decoding and bounded physical-button press filtering.
//!
//! The kernel input_event ABI uses two native unsigned-long time words,
//! followed by u16 type, u16 code, and i32 value. Decoding only the final
//! fields avoids depending on libc's time64 timeval layout on ARMv7/musl.
//! This module never opens an input device or changes its state.
use std::time::{Duration, Instant};

pub const EV_KEY: u16 = 0x01;
pub const EV_SW: u16 = 0x05;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InputEvent {
    pub kind: u16,
    pub code: u16,
    pub value: i32,
}

pub fn record_length(word_bytes: usize) -> Option<usize> {
    match word_bytes {
        4 | 8 => Some(word_bytes * 2 + 8),
        _ => None,
    }
}

pub fn decode_record(bytes: &[u8], word_bytes: usize) -> Option<InputEvent> {
    let length = record_length(word_bytes)?;
    if bytes.len() != length {
        return None;
    }
    let offset = word_bytes * 2;
    Some(InputEvent {
        kind: u16::from_ne_bytes(bytes[offset..offset + 2].try_into().ok()?),
        code: u16::from_ne_bytes(bytes[offset + 2..offset + 4].try_into().ok()?),
        value: i32::from_ne_bytes(bytes[offset + 4..offset + 8].try_into().ok()?),
    })
}

/// Bounded partial-frame buffer: no allocations except the returned batch.
pub struct EventDecoder {
    pending: [u8; 24],
    used: usize,
    word_bytes: usize,
}

impl Default for EventDecoder {
    fn default() -> Self {
        Self::new(std::mem::size_of::<usize>())
    }
}

impl EventDecoder {
    pub fn new(word_bytes: usize) -> Self {
        assert!(record_length(word_bytes).is_some());
        Self {
            pending: [0; 24],
            used: 0,
            word_bytes,
        }
    }

    pub fn push(&mut self, bytes: &[u8]) -> Vec<InputEvent> {
        let length = record_length(self.word_bytes).expect("validated word size");
        let mut events = Vec::with_capacity((self.used + bytes.len()) / length);
        for &byte in bytes {
            self.pending[self.used] = byte;
            self.used += 1;
            if self.used == length {
                if let Some(event) = decode_record(&self.pending[..length], self.word_bytes) {
                    events.push(event);
                }
                self.used = 0;
            }
        }
        events
    }
}

pub fn switch_level(event: InputEvent, code: u16) -> Option<u8> {
    if event.kind == EV_SW && event.code == code {
        match event.value {
            0 => Some(0),
            1 => Some(1),
            _ => None,
        }
    } else {
        None
    }
}

/// Key-down edge gate. Release re-arms the code, repeat never emits; bounded.
pub struct ButtonPressGate {
    cooldown: Duration,
    last: Vec<(u16, Instant, bool)>,
}

impl ButtonPressGate {
    pub fn new(cooldown: Duration) -> Self {
        Self {
            cooldown,
            last: Vec::new(),
        }
    }

    pub fn accept(&mut self, event: InputEvent, now: Instant) -> bool {
        if event.kind != EV_KEY {
            return false;
        }
        if event.value == 0 {
            if let Some((_, _, held)) = self
                .last
                .iter_mut()
                .find(|(code, _, _)| *code == event.code)
            {
                *held = false;
            }
            return false;
        }
        if event.value != 1 {
            return false;
        }
        if let Some((_, last, held)) = self
            .last
            .iter_mut()
            .find(|(code, _, _)| *code == event.code)
        {
            if *held {
                return false;
            }
            *held = true;
            if now.saturating_duration_since(*last) < self.cooldown {
                return false;
            }
            *last = now;
            return true;
        }
        if self.last.len() == 16 {
            self.last.remove(0);
        }
        self.last.push((event.code, now, true));
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bytes(word_bytes: usize, event: InputEvent) -> Vec<u8> {
        let mut record = vec![0; record_length(word_bytes).unwrap()];
        let offset = word_bytes * 2;
        record[offset..offset + 2].copy_from_slice(&event.kind.to_ne_bytes());
        record[offset + 2..offset + 4].copy_from_slice(&event.code.to_ne_bytes());
        record[offset + 4..offset + 8].copy_from_slice(&event.value.to_ne_bytes());
        record
    }

    #[test]
    fn decodes_native_and_armv7_time64_independent_frames() {
        let key = InputEvent {
            kind: EV_KEY,
            code: 115,
            value: 1,
        };
        for word_bytes in [4, 8] {
            let record = bytes(word_bytes, key);
            assert_eq!(decode_record(&record, word_bytes), Some(key));
            let mut decoder = EventDecoder::new(word_bytes);
            assert!(decoder.push(&record[..3]).is_empty());
            assert!(decoder.push(&record[3..record.len() - 1]).is_empty());
            assert_eq!(decoder.push(&record[record.len() - 1..]), vec![key]);
            assert_eq!(
                decoder.push(&[record.clone(), record].concat()),
                vec![key, key]
            );
        }
        assert_eq!(decode_record(&[0; 16], 2), None);
        assert_eq!(decode_record(&[0; 15], 4), None);
    }

    #[test]
    fn switch_level_rejects_other_codes_types_and_invalid_values() {
        let event = InputEvent {
            kind: EV_SW,
            code: 1,
            value: 0,
        };
        assert_eq!(switch_level(event, 1), Some(0));
        assert_eq!(switch_level(InputEvent { value: 1, ..event }, 1), Some(1));
        assert_eq!(switch_level(event, 0), None);
        assert_eq!(
            switch_level(
                InputEvent {
                    kind: EV_KEY,
                    ..event
                },
                1
            ),
            None
        );
        assert_eq!(switch_level(InputEvent { value: 2, ..event }, 1), None);
    }

    #[test]
    fn presses_are_bounded_and_releases_repeats_are_ignored() {
        let start = Instant::now();
        let mut gate = ButtonPressGate::new(Duration::from_millis(80));
        let down = InputEvent {
            kind: EV_KEY,
            code: 114,
            value: 1,
        };
        assert!(gate.accept(down, start));
        assert!(!gate.accept(down, start + Duration::from_millis(40)));
        assert!(!gate.accept(InputEvent { value: 2, ..down }, start));
        assert!(!gate.accept(InputEvent { value: 0, ..down }, start));
        assert!(!gate.accept(down, start + Duration::from_millis(40)));
        assert!(!gate.accept(InputEvent { value: 0, ..down }, start));
        assert!(gate.accept(InputEvent { code: 115, ..down }, start));
        assert!(gate.accept(down, start + Duration::from_millis(80)));
        assert!(!gate.accept(down, start + Duration::from_secs(3)));
        assert!(!gate.accept(
            InputEvent { value: 0, ..down },
            start + Duration::from_secs(3)
        ));
        assert!(gate.accept(down, start + Duration::from_secs(4)));
        for code in 1..30 {
            assert!(gate.accept(InputEvent { code, ..down }, start));
        }
        assert_eq!(gate.last.len(), 16);
    }
}
