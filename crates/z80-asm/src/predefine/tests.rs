//! `--equ` texts, each with what PasmoNext 0.1.3 made of it.

use super::*;

const PROFILE: Profile = Profile::PASMONEXT_0_1_3;

#[test]
fn a_name_and_a_number_define_a_symbol() {
    for (text, name, value) in [
        ("flag", "flag", 0xFFFF),
        ("size=10", "size", 10),
        ("size=0FFh", "size", 0xFF),
        ("aa=$10", "aa", 0x10),
        ("aa = 1", "aa", 1),
        (" aa=1 ", "aa", 1),
        ("aa=1;x", "aa", 1),
        ("aa=0x10", "aa", 0x10),
        ("aa=10b", "aa", 2),
        ("aa=&h10", "aa", 0x10),
        ("aa=1h", "aa", 1),
        ("_x=1", "_x", 1),
        // Kept to 16 bits: 70000 is 1170h.
        ("aa=70000", "aa", 0x1170),
        // `$` is not part of a name.
        ("a$a=2", "aa", 2),
    ] {
        assert_eq!(
            predefine(text, PROFILE),
            Ok((name.to_owned(), value)),
            "{text}"
        );
    }
}

#[test]
fn the_name_takes_the_profiles_case() {
    let any_case = PROFILE.with_names_in_any_case();
    assert_eq!(predefine("Aa=3", any_case), Ok(("AA".to_owned(), 3)));
    assert_eq!(predefine("Aa=3", PROFILE), Ok(("Aa".to_owned(), 3)));
}

#[test]
fn pasmonext_refuses_anything_else_with_its_own_messages() {
    for (text, message) in [
        ("", "Can't predefine invalid identifier"),
        ("5=1", "Can't predefine invalid identifier"),
        ("ld=1", "Can't predefine invalid identifier"),
        ("aa=", "Invalid value for predefined symbol"),
        ("aa=-1", "Invalid value for predefined symbol"),
        ("aa==1", "Invalid value for predefined symbol"),
        ("aa=1,2", "Invalid value for predefined symbol"),
        ("aa=1+1", "Invalid value for predefined symbol"),
        ("aa=\"A\"", "Invalid value for predefined symbol"),
        ("aa:1", "Syntax error in predefined symbol"),
    ] {
        assert_eq!(predefine(text, PROFILE), Err(message.to_owned()), "{text}");
    }
}
