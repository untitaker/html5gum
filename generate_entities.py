import json
import sys

key_and_value = list(json.load(sys.stdin).items())
# Sort by descending length so we match the largest prefix first
key_and_value.sort(key=lambda x: (-len(x[0]), x[0]))

with open("src/entities.rs", "w") as f:
    f.write("""
// @generated
// this file is autogenerated by
// curl https://html.spec.whatwg.org/entities.json | python generate_entities.py

pub struct CharRef {
    /// Name as it appears escaped in HTML
    pub name: &'static str,
    /// Unescaped character codepoints
    pub characters: &'static str,
}

pub fn try_read_character_reference<E>(first_char: char, mut try_read: impl FnMut(&str) -> Result<bool, E>) -> Result<Option<CharRef>, E> {
""")

    for key, value in key_and_value:
        assert key[0] == '&'
        key = key[1:]
        characters = ""
        for c in value['codepoints']:
            characters += r"\u{" + hex(c)[2:] + r"}"

        first_char = key[0]
        key = key[1:]
        f.write("""
        if first_char == '%(first_char)s' && try_read("%(key)s")? {
            return Ok(Some(CharRef { name: "%(key)s", characters: "%(characters)s" }));
        }
        """ % {"key": key, "characters": characters, "first_char": first_char})

    f.write(" Ok(None) }");
