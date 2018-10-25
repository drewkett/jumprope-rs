// These tests are also adapted from the C code tests here:
// https://github.com/josephg/librope/blob/master/test/tests.c

#[cfg(test)]
mod test {
    extern crate jumprope;
    use self::jumprope::{Rope, JumpRope};

    extern crate rand;
    use self::rand::Rng;
    use self::rand::RngCore;
    
    const UCHARS: [char; 23] = [
      'a', 'b', 'c', '1', '2', '3', ' ', '\n', // ASCII
      '©', '¥', '½', // The Latin-1 suppliment (U+80 - U+ff)
      'Ύ', 'Δ', 'δ', 'Ϡ', // Greek (U+0370 - U+03FF)
      '←', '↯', '↻', '⇈', // Arrows (U+2190 – U+21FF)
      '𐆐', '𐆔', '𐆘', '𐆚', // Ancient roman symbols (U+10190 – U+101CF)
    ];

    const CHARS: &[u8; 83] = b" ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789!@#$%^&*()[]{}<>?,./";

    fn random_ascii_string(len: usize) -> String {
        let mut s = String::new();
        let mut rng = rand::thread_rng();
        for _ in 0..len {
            s.push(*rng.choose(CHARS).unwrap() as char);
        }
        s
    }

    fn check<'a, T: Rope<'a>>(r: &T, expected: &'a str) {
        r.check();
        r.print();
        assert_eq!(r.to_string(), expected);
        assert_eq!(r.len(), expected.len());
        assert_eq!(r.char_len(), expected.chars().count());
        assert!(*r == T::from(expected), "Rope comparison fails");
    }

    #[test]
    fn empty_rope_has_no_contents() {
        let mut r = JumpRope::new();
        check(&r, "");

        r.insert(0, "").unwrap();
        check(&r, "");
    }

    #[test]
    fn insert_at_location() {
        let mut r = JumpRope::new();

        r.insert(0, "AAA").unwrap();
        check(&r, "AAA");

        r.insert(0, "BBB").unwrap();
        check(&r, "BBBAAA");

        r.insert(6, "CCC").unwrap();
        check(&r, "BBBAAACCC");

        r.insert(5, "DDD").unwrap();
        check(&r, "BBBAADDDACCC");
    }

    #[test]
    fn new_string_has_content() {
        let r = JumpRope::new_from_str("hi there");
        check(&r, "hi there");

        let mut r = JumpRope::new_from_str("κόσμε");
        check(&r, "κόσμε");
        r.insert(2, "𝕐𝕆😘").unwrap();
        check(&r, "κό𝕐𝕆😘σμε");
    }

    #[test]
    fn del_at_location() {
        let mut r = JumpRope::new_from_str("012345678");

        r.del(8, 1).unwrap();
        check(&r, "01234567");
        
        r.del(0, 1).unwrap();
        check(&r, "1234567");
        
        r.del(5, 1).unwrap();
        check(&r, "123457");
        
        r.del(5, 1).unwrap();
        check(&r, "12345");
        
        r.del(0, 5).unwrap();
        check(&r, "");
    }

    #[test]
    fn del_past_end_of_string() {
        let mut r = JumpRope::new();

        r.del(0, 100).unwrap();
        check(&r, "");

        r.insert(0, "hi there").unwrap();
        r.del(3, 10).unwrap();
        check(&r, "hi ");
    }

    #[test]
    fn really_long_ascii_string() {
        let len = 2000;
        let s = random_ascii_string(len);

        let mut r = JumpRope::new_from_str(s.as_str());
        check(&r, s.as_str());

        // Delete everything but the first and last characters
        r.del(1, len - 2).unwrap();
        let expect = format!("{}{}", s.as_bytes()[0] as char, s.as_bytes()[len-1] as char);
        check(&r, expect.as_str());
    }
}
