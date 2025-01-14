// These tests are also adapted from the C code tests here:
// https://github.com/josephg/librope/blob/master/test/tests.c

#[cfg(test)]
mod test {
    use rand::prelude::*;

    use std::cmp::min;

    const UCHARS: [char; 23] = [
      'a', 'b', 'c', '1', '2', '3', ' ', '\n', // ASCII
      '©', '¥', '½', // The Latin-1 suppliment (U+80 - U+ff)
      'Ύ', 'Δ', 'δ', 'Ϡ', // Greek (U+0370 - U+03FF)
      '←', '↯', '↻', '⇈', // Arrows (U+2190 – U+21FF)
      '𐆐', '𐆔', '𐆘', '𐆚', // Ancient roman symbols (U+10190 – U+101CF)
    ];

    fn random_unicode_string(len: usize) -> String {
        let mut s = String::new();
        let mut rng = rand::thread_rng();
        for _ in 0..len {
            s.push(CHARS[rng.gen_range(0 .. UCHARS.len())] as char);
        }
        s
    }

    const CHARS: &[u8; 83] = b" ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789!@#$%^&*()[]{}<>?,./";

    // Gross. Find a way to reuse the code from random_unicode_string.
    fn random_ascii_string(len: usize) -> String {
        let mut s = String::new();
        let mut rng = rand::thread_rng();
        for _ in 0..len {
            s.push(CHARS[rng.gen_range(0 .. CHARS.len())] as char);
        }
        s
    }

    fn check(r: &JumpRope, expected: &str) {
        r.check();
        // println!("--- rope ---");
        // r.print();
        assert_eq!(r.to_string(), expected);
        assert_eq!(r.len_bytes(), expected.len());
        assert_eq!(r.len_chars(), expected.chars().count());
        assert!(*r == JumpRope::from(expected), "Rope comparison fails");

        let clone = r.clone();
        // println!("--- clone ---");
        // clone.print();
        clone.check();
        assert!(*r == clone, "Rope does not equal its clone");
    }

    #[test]
    fn empty_rope_has_no_contents() {
        let mut r = JumpRope::new();
        check(&r, "");

        r.insert(0, "");
        check(&r, "");
    }

    #[test]
    fn insert_at_location() {
        let mut r = JumpRope::new();

        r.insert(0, "AAA");
        check(&r, "AAA");

        r.insert(0, "BBB");
        check(&r, "BBBAAA");

        r.insert(6, "CCC");
        check(&r, "BBBAAACCC");

        r.insert(5, "DDD");
        check(&r, "BBBAADDDACCC");
    }

    #[test]
    fn new_string_has_content() {
        let r = JumpRope::from("hi there");
        check(&r, "hi there");

        let mut r = JumpRope::from("κόσμε");
        check(&r, "κόσμε");
        r.insert(2, "𝕐𝕆😘");
        check(&r, "κό𝕐𝕆😘σμε");
    }

    #[test]
    fn del_at_location() {
        let mut r = JumpRope::from("012345678");
        check(&r, "012345678");

        r.remove(8..9);
        check(&r, "01234567");

        r.remove(0..1);
        check(&r, "1234567");

        r.remove(5..6);
        check(&r, "123457");
        
        r.remove(5..6);
        check(&r, "12345");
        
        r.remove(0..5);
        check(&r, "");
    }

    #[test]
    fn del_past_end_of_string() {
        let mut r = JumpRope::new();

        r.remove(0..100);
        check(&r, "");

        r.insert(0, "hi there");
        r.remove(3..13);
        check(&r, "hi ");
    }

    #[test]
    fn really_long_ascii_string() {
        let len = 2000;
        let s = random_ascii_string(len);

        let mut r = JumpRope::from(s.as_str());
        check(&r, s.as_str());

        // Delete everything but the first and last characters
        r.remove(1..len - 1);
        let expect = format!("{}{}", s.as_bytes()[0] as char, s.as_bytes()[len-1] as char);
        check(&r, expect.as_str());
    }


    use std::ptr;
    use jumprope::JumpRope;

    fn string_insert_at(s: &mut String, char_pos: usize, contents: &str) {
        // If you try to write past the end of the string for now I'll just write at the end.
        // Panicing might be a better policy.
        let byte_pos = s.char_indices().skip(char_pos).next()
            .map(|(p, _)| p).unwrap_or(s.len());
        
        let old_len = s.len();
        let new_bytes = contents.len();

        // This didn't work because it didn't change the string's length
        //s.reserve(new_bytes);

        // This is sort of ugly but its fine.
        for _ in 0..new_bytes { s.push('\0'); }

        //println!("new bytes {} {} {}", new_bytes, byte_pos, s.len() - byte_pos);
        unsafe {
            let bytes = s.as_mut_vec().as_mut_ptr();
            ptr::copy(
                bytes.offset(byte_pos as isize),
                bytes.offset((byte_pos + new_bytes) as isize),
                old_len - byte_pos
            );
            ptr::copy_nonoverlapping(
                contents.as_ptr(),
                bytes.offset(byte_pos as isize),
                new_bytes
            );
        }
    }

    fn string_del_at(s: &mut String, pos: usize, length: usize) {
        let byte_range = {
            let mut iter = s.char_indices().map(|(p, _)| p).skip(pos).peekable();

            let start = iter.peek().map_or_else(|| s.len(), |&p| p);
            let mut iter = iter.skip(length).peekable();
            let end = iter.peek().map_or_else(|| s.len(), |&p| p);

            start..end
        };

        s.drain(byte_range);
    }


    #[test]
    fn random_edits() {
        let mut r = JumpRope::new();
        let mut s = String::new();
        
        // let mut rng = rand::thread_rng();
        let mut rng = SmallRng::seed_from_u64(2);

        for _i in 0..1000 {
            // println!("{}", _i);
            // r.print();
            check(&r, s.as_str());

            let len = s.chars().count();

            // println!("i {}: {}", i, len);
            
            if len == 0 || (len < 1000 && rng.gen::<f32>() < 0.5) {
                // Insert.
                let pos = rng.gen_range(0..len+1);
                // Sometimes generate strings longer than a single node to stress everything.
                let text = random_unicode_string(rng.gen_range(0..20));
                r.insert(pos, text.as_str());
                string_insert_at(&mut s, pos, text.as_str());
            } else {
                // Delete
                let pos = rng.gen_range(0..len);
                let dlen = min(rng.gen_range(0..10), len - pos);

                r.remove(pos..pos+dlen);
                string_del_at(&mut s, pos, dlen);
            }
        }
    }
}
