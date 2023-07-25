use crate::arrayvec::ArrayVec;
use crate::{Emitter, Error};

#[derive(Debug)]
pub(crate) struct CharValidator {
    last_4_bytes: u32,
    character_error: ArrayVec<Error, 3>,
}

impl Default for CharValidator {
    fn default() -> Self {
        CharValidator {
            last_4_bytes: 0,
            character_error: ArrayVec::new(Error::EofInTag),
        }
    }
}

impl CharValidator {
    pub(crate) fn reset(&mut self) {
        self.last_4_bytes = 0;
    }

    #[inline]
    pub(crate) fn validate_bytes<E: Emitter>(&mut self, emitter: &mut E, next_bytes: &[u8]) {
        if !E::should_emit_errors() {
            return;
        }

        for &x in next_bytes {
            self.validate_byte(emitter, x);
        }
    }

    #[inline]
    pub(crate) fn validate_byte<E: Emitter>(&mut self, emitter: &mut E, next_byte: u8) {
        if next_byte < 128 {
            // start of character (ascii)
            self.last_4_bytes = 0;
            self.flush_character_error(emitter);
            self.validate_last_4_bytes(emitter, u32::from(next_byte));
        } else if next_byte >= 192 {
            // start of character (non-ascii)
            self.last_4_bytes = u32::from(next_byte);
            self.flush_character_error(emitter);
        } else {
            self.last_4_bytes <<= 8;
            self.last_4_bytes |= u32::from(next_byte);
            self.validate_last_4_bytes(emitter, self.last_4_bytes);
        }
    }

    pub(crate) fn flush_character_error<E: Emitter>(&mut self, emitter: &mut E) {
        for e in self.character_error.drain() {
            emitter.emit_error(*e);
        }
    }

    pub(crate) fn set_character_error<E: Emitter>(&mut self, emitter: &mut E, error: Error) {
        if self.last_4_bytes == 0 {
            emitter.emit_error(error);
        } else {
            self.character_error.push(error);
        }
    }

    #[inline]
    fn validate_last_4_bytes<E: Emitter>(&mut self, emitter: &mut E, last_4_bytes: u32) {
        // generated with Python 3:
        // ' | '.join(map(hex, sorted([int.from_bytes(chr(x).encode("utf8"), 'big') for x in nonchars])))
        match last_4_bytes {
            0x00ef_b790 | 0x00ef_b791 | 0x00ef_b792 | 0x00ef_b793 | 0x00ef_b794 | 0x00ef_b795
            | 0x00ef_b796 | 0x00ef_b797 | 0x00ef_b798 | 0x00ef_b799 | 0x00ef_b79a | 0x00ef_b79b
            | 0x00ef_b79c | 0x00ef_b79d | 0x00ef_b79e | 0x00ef_b79f | 0x00ef_b7a0 | 0x00ef_b7a1
            | 0x00ef_b7a2 | 0x00ef_b7a3 | 0x00ef_b7a4 | 0x00ef_b7a5 | 0x00ef_b7a6 | 0x00ef_b7a7
            | 0x00ef_b7a8 | 0x00ef_b7a9 | 0x00ef_b7aa | 0x00ef_b7ab | 0x00ef_b7ac | 0x00ef_b7ad
            | 0x00ef_b7ae | 0x00ef_b7af | 0x00ef_bfbe | 0x00ef_bfbf | 0xf09f_bfbe | 0xf09f_bfbf
            | 0xf0af_bfbe | 0xf0af_bfbf | 0xf0bf_bfbe | 0xf0bf_bfbf | 0xf18f_bfbe | 0xf18f_bfbf
            | 0xf19f_bfbe | 0xf19f_bfbf | 0xf1af_bfbe | 0xf1af_bfbf | 0xf1bf_bfbe | 0xf1bf_bfbf
            | 0xf28f_bfbe | 0xf28f_bfbf | 0xf29f_bfbe | 0xf29f_bfbf | 0xf2af_bfbe | 0xf2af_bfbf
            | 0xf2bf_bfbe | 0xf2bf_bfbf | 0xf38f_bfbe | 0xf38f_bfbf | 0xf39f_bfbe | 0xf39f_bfbf
            | 0xf3af_bfbe | 0xf3af_bfbf | 0xf3bf_bfbe | 0xf3bf_bfbf | 0xf48f_bfbe | 0xf48f_bfbf => {
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
