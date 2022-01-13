use crate::{Emitter, Error};

#[derive(Default)]
pub(crate) struct CharValidator {
    last_4_bytes: u32,
    character_error: Option<Error>,
}

impl CharValidator {
    pub fn reset(&mut self) {
        self.last_4_bytes = 0;
    }

    #[inline]
    pub fn validate_bytes<E: Emitter>(&mut self, emitter: &mut E, next_bytes: &[u8]) {
        for &x in next_bytes {
            self.validate_byte(emitter, x);
        }
    }

    #[inline]
    pub fn validate_byte<E: Emitter>(&mut self, emitter: &mut E, next_byte: u8) {
        if next_byte < 128 {
            self.last_4_bytes = 0;
            self.flush_character_error(emitter);
            self.validate_last_4_bytes(emitter, next_byte as u32);
        } else if next_byte >= 192 {
            self.last_4_bytes = next_byte as u32;
        } else {
            self.last_4_bytes <<= 8;
            self.last_4_bytes |= next_byte as u32;
            self.validate_last_4_bytes(emitter, self.last_4_bytes);
        }
    }

    pub fn flush_character_error<E: Emitter>(&mut self, emitter: &mut E) {
        if let Some(e) = self.character_error.take() {
            emitter.emit_error(e);
        }
    }

    pub fn set_character_error<E: Emitter>(&mut self, emitter: &mut E, error: Error) {
        self.flush_character_error(emitter);
        if self.last_4_bytes == 0 {
            emitter.emit_error(error);
        } else {
            self.character_error = Some(error);
        }
    }

    #[inline]
    fn validate_last_4_bytes<E: Emitter>(&mut self, emitter: &mut E, last_4_bytes: u32) {
        // generated with Python 3:
        // ' | '.join(map(hex, sorted([int.from_bytes(chr(x).encode("utf8"), 'big') for x in nonchars])))
        match last_4_bytes {
            0xefb790 | 0xefb791 | 0xefb792 | 0xefb793 | 0xefb794 | 0xefb795 | 0xefb796
            | 0xefb797 | 0xefb798 | 0xefb799 | 0xefb79a | 0xefb79b | 0xefb79c | 0xefb79d
            | 0xefb79e | 0xefb79f | 0xefb7a0 | 0xefb7a1 | 0xefb7a2 | 0xefb7a3 | 0xefb7a4
            | 0xefb7a5 | 0xefb7a6 | 0xefb7a7 | 0xefb7a8 | 0xefb7a9 | 0xefb7aa | 0xefb7ab
            | 0xefb7ac | 0xefb7ad | 0xefb7ae | 0xefb7af | 0xefbfbe | 0xefbfbf | 0xf09fbfbe
            | 0xf09fbfbf | 0xf0afbfbe | 0xf0afbfbf | 0xf0bfbfbe | 0xf0bfbfbf | 0xf18fbfbe
            | 0xf18fbfbf | 0xf19fbfbe | 0xf19fbfbf | 0xf1afbfbe | 0xf1afbfbf | 0xf1bfbfbe
            | 0xf1bfbfbf | 0xf28fbfbe | 0xf28fbfbf | 0xf29fbfbe | 0xf29fbfbf | 0xf2afbfbe
            | 0xf2afbfbf | 0xf2bfbfbe | 0xf2bfbfbf | 0xf38fbfbe | 0xf38fbfbf | 0xf39fbfbe
            | 0xf39fbfbf | 0xf3afbfbe | 0xf3afbfbf | 0xf3bfbfbe | 0xf3bfbfbf | 0xf48fbfbe
            | 0xf48fbfbf => {
                emitter.emit_error(Error::NoncharacterInInputStream);
                self.flush_character_error(emitter);
            }
            0x1 | 0x2 | 0x3 | 0x4 | 0x5 | 0x6 | 0x7 | 0x8 | 0xb | 0xd | 0xe | 0xf | 0x10 | 0x11
            | 0x12 | 0x13 | 0x14 | 0x15 | 0x16 | 0x17 | 0x18 | 0x19 | 0x1a | 0x1b | 0x1c | 0x1d
            | 0x1e | 0x1f | 0x7f | 0xc280 | 0xc281 | 0xc282 | 0xc283 | 0xc284 | 0xc285 | 0xc286
            | 0xc287 | 0xc288 | 0xc289 | 0xc28a | 0xc28b | 0xc28c | 0xc28d | 0xc28e | 0xc28f
            | 0xc290 | 0xc291 | 0xc292 | 0xc293 | 0xc294 | 0xc295 | 0xc296 | 0xc297 | 0xc298
            | 0xc299 | 0xc29a | 0xc29b | 0xc29c | 0xc29d | 0xc29e | 0xc29f => {
                emitter.emit_error(Error::ControlCharacterInInputStream);
                self.flush_character_error(emitter);
            }

            _ => (),
        }
    }
}
